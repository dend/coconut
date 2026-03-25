use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::download::manager::{download_file, resolve_filename};
use crate::error::Result;
use crate::gog::client::GogClient;
use crate::gog::models::{GameDetails, Product};
use crate::library::listing::fetch_all_products;
use crate::library::manifest::{ManifestEntry, SyncManifest};

pub struct SyncOptions {
  pub sync_dir: PathBuf,
  pub game_filter: Option<String>,
  pub platform_filter: Option<String>,
  pub force: bool,
}

struct DownloadJob {
  game_id: u64,
  game_slug: String,
  manual_url: String,
  dest_dir: PathBuf,
  version: Option<String>,
  display_name: String,
}

pub async fn run_sync(opts: SyncOptions) -> Result<()> {
  let mut client = GogClient::new().await?;
  let products = fetch_all_products(&mut client).await?;

  // Apply game filter
  let products: Vec<&Product> = products
    .iter()
    .filter(|p| {
      if let Some(ref filter) = opts.game_filter {
        let f = filter.to_lowercase();
        p.slug.to_lowercase().contains(&f)
          || p.title.to_lowercase().contains(&f)
      } else {
        true
      }
    })
    .collect();

  if products.is_empty() {
    println!(
      "{}",
      style("No games matched the filter.").yellow()
    );
    return Ok(());
  }

  println!(
    "{}",
    style(format!("Syncing {} games...", products.len())).cyan()
  );

  // Load manifest
  let mut manifest = SyncManifest::load(&opts.sync_dir)?;
  let mut jobs: Vec<DownloadJob> = Vec::new();

  // Fetch details for each game and build download jobs
  let detail_spinner = ProgressBar::new(products.len() as u64);
  detail_spinner.set_style(
    ProgressStyle::with_template(
      "{spinner:.cyan} Fetching game details... {pos}/{len}",
    )
    .unwrap(),
  );
  detail_spinner
    .enable_steady_tick(std::time::Duration::from_millis(80));

  for product in &products {
    let details = match client
      .get_game_details(product.id)
      .await
    {
      Ok(d) => d,
      Err(e) => {
        eprintln!(
          "  {} Failed to fetch details for {}: {e}",
          style("!").yellow(),
          product.title
        );
        detail_spinner.inc(1);
        continue;
      }
    };
    detail_spinner.inc(1);

    collect_jobs(
      product.id,
      &product.slug,
      &details,
      &opts,
      &manifest,
      &mut jobs,
    );
  }
  detail_spinner.finish_and_clear();

  if jobs.is_empty() {
    println!(
      "{}",
      style("Everything is up to date!").green()
    );
    return Ok(());
  }

  println!(
    "{}",
    style(format!("{} files to download", jobs.len())).cyan()
  );

  // Download files
  let multi = MultiProgress::new();
  let mut downloaded = 0u32;
  let mut failed = 0u32;

  for (i, job) in jobs.iter().enumerate() {
    let prefix = format!("[{}/{}]", i + 1, jobs.len());
    let pb = multi.add(ProgressBar::new(0));
    pb.set_style(
      ProgressStyle::with_template(&format!(
        "{prefix} {{spinner:.green}} {{msg}}\n    \
         [{{bar:40.cyan/dim}}] {{bytes}}/{{total_bytes}} \
         ({{bytes_per_sec}}, {{eta}})"
      ))
      .unwrap()
      .progress_chars("━╸─"),
    );
    pb.set_message(job.display_name.clone());

    // Resolve CDN URL
    let cdn_url = match client
      .resolve_download_url(&job.manual_url)
      .await
    {
      Ok(url) => url,
      Err(e) => {
        pb.set_style(
          ProgressStyle::with_template(&format!(
            "{prefix} {{msg}}"
          ))
          .unwrap(),
        );
        pb.finish_with_message(format!(
          "{} {} (resolve failed: {e})",
          style("✗").red(),
          job.display_name
        ));
        failed += 1;
        continue;
      }
    };

    // Resolve real filename from CDN
    let filename = match resolve_filename(
      client.http(),
      &cdn_url,
    )
    .await
    {
      Ok(name) => sanitize_filename(&name),
      Err(_) => {
        // Fall back to manual_url last segment
        job
          .manual_url
          .rsplit('/')
          .next()
          .unwrap_or("download")
          .to_string()
      }
    };

    let dest_path = job.dest_dir.join(&filename);
    pb.set_message(format!(
      "{} / {}",
      job.display_name, filename
    ));

    const MAX_RETRIES: u32 = 10;
    let mut last_err = None;
    let mut size_bytes = 0u64;
    for attempt in 1..=MAX_RETRIES {
      match download_file(
        client.http(),
        &cdn_url,
        &dest_path,
        &pb,
      )
      .await
      {
        Ok(bytes) => {
          size_bytes = bytes;
          last_err = None;
          break;
        }
        Err(e) => {
          if attempt < MAX_RETRIES {
            pb.set_message(format!(
              "{} / {} (retry {attempt}/{MAX_RETRIES})",
              job.display_name, filename
            ));
          }
          last_err = Some(e);
        }
      }
    }

    if let Some(e) = last_err {
      pb.set_style(
        ProgressStyle::with_template(&format!(
          "{prefix} {{msg}}"
        ))
        .unwrap(),
      );
      pb.finish_with_message(format!(
        "{} {} ({e})",
        style("✗").red(),
        job.display_name,
      ));
      failed += 1;
    } else {
      pb.set_style(
        ProgressStyle::with_template(&format!(
          "{prefix} {{msg}}"
        ))
        .unwrap(),
      );
      pb.finish_with_message(format!(
        "{} {} / {} ({})",
        style("✓").green(),
        job.display_name,
        filename,
        format_bytes(size_bytes)
      ));
      downloaded += 1;

      let key =
        SyncManifest::key(job.game_id, &job.manual_url);
      manifest.entries.insert(
        key,
        ManifestEntry {
          game_id: job.game_id,
          game_slug: job.game_slug.clone(),
          file_path: dest_path
            .strip_prefix(&opts.sync_dir)
            .unwrap_or(&dest_path)
            .to_string_lossy()
            .to_string(),
          manual_url: job.manual_url.clone(),
          version: job.version.clone(),
          size_bytes,
          downloaded_at: now_unix(),
        },
      );
      manifest.save(&opts.sync_dir)?;
    }
  }

  // Summary
  println!();
  println!(
    "{} {} downloaded, {} failed, {} already up to date",
    style("Done!").green().bold(),
    downloaded,
    failed,
    manifest.entries.len() - downloaded as usize
  );

  Ok(())
}

fn collect_jobs(
  game_id: u64,
  slug: &str,
  details: &GameDetails,
  opts: &SyncOptions,
  manifest: &SyncManifest,
  jobs: &mut Vec<DownloadJob>,
) {
  // Installers
  for (language, platforms) in &details.downloads {
    let platform_list: Vec<(&str, &[crate::gog::models::Installer])> =
      vec![
        ("windows", &platforms.windows),
        ("linux", &platforms.linux),
        ("mac", &platforms.mac),
      ];

    for (platform, installers) in platform_list {
      if opts
        .platform_filter
        .as_ref()
        .is_some_and(|pf| platform != pf.to_lowercase())
      {
        continue;
      }
      for installer in installers.iter() {
        if !opts.force
          && manifest.has(
            game_id,
            &installer.manual_url,
            installer.version.as_deref(),
          )
        {
          continue;
        }

        let mut dest_dir = opts.sync_dir.join(slug).join(platform);
        if language != "English" {
          dest_dir =
            dest_dir.join(sanitize_filename(language));
        }

        jobs.push(DownloadJob {
          game_id,
          game_slug: slug.to_string(),
          manual_url: installer.manual_url.clone(),
          dest_dir,
          version: installer.version.clone(),
          display_name: format!(
            "{} / {platform}",
            details.title
          ),
        });
      }
    }
  }

  // Extras
  for extra in &details.extras {
    if !opts.force
      && manifest.has(game_id, &extra.manual_url, None)
    {
      continue;
    }

    let dest_dir = opts.sync_dir.join(slug).join("extras");

    jobs.push(DownloadJob {
      game_id,
      game_slug: slug.to_string(),
      manual_url: extra.manual_url.clone(),
      dest_dir,
      version: None,
      display_name: format!(
        "{} / extras",
        details.title
      ),
    });
  }
}

fn sanitize_filename(name: &str) -> String {
  name
    .chars()
    .map(|c| match c {
      '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => {
        '_'
      }
      c => c,
    })
    .collect()
}

fn now_unix() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system clock before UNIX epoch")
    .as_secs()
}

fn format_bytes(bytes: u64) -> String {
  const KB: u64 = 1024;
  const MB: u64 = 1024 * KB;
  const GB: u64 = 1024 * MB;
  if bytes >= GB {
    format!("{:.1} GB", bytes as f64 / GB as f64)
  } else if bytes >= MB {
    format!("{:.1} MB", bytes as f64 / MB as f64)
  } else if bytes >= KB {
    format!("{:.0} KB", bytes as f64 / KB as f64)
  } else {
    format!("{bytes} B")
  }
}
