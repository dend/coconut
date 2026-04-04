use std::path::PathBuf;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::download::manager::{download_file, resolve_filename};
use crate::error::Result;
use crate::gog::client::GogClient;
use crate::gog::models::{GameDetails, Product};
use crate::library::history::DownloadHistory;
use crate::library::listing::fetch_all_products;
use crate::library::manifest::{ManifestEntry, SyncManifest, now_unix};

pub struct SyncOptions {
  pub sync_dir: PathBuf,
  pub overflow_dir: Option<PathBuf>,
  pub game_filter: Option<String>,
  pub platform_filter: Option<String>,
  pub force: bool,
  pub backfill_history: bool,
}

struct DownloadJob {
  game_id: u64,
  game_slug: String,
  manual_url: String,
  /// Relative path within the sync root (e.g. "game_slug/windows")
  relative_dir: PathBuf,
  version: Option<String>,
  display_name: String,
}

/// Minimum free space (in bytes) before switching to overflow.
/// 1 GB buffer to avoid filling a disk completely.
const MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;

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
  if let Some(ref od) = opts.overflow_dir {
    println!(
      "  Primary:  {}",
      style(opts.sync_dir.display()).bold()
    );
    println!(
      "  Overflow: {}",
      style(od.display()).bold()
    );
  }

  // Load manifests and global download history
  let mut manifest = SyncManifest::load(&opts.sync_dir)?;
  let mut overflow_manifest = opts
    .overflow_dir
    .as_ref()
    .map(|od| SyncManifest::load(od))
    .transpose()?
    .unwrap_or_default();
  let mut history = DownloadHistory::load()?;

  if opts.backfill_history {
    let mut backfilled = 0;
    backfilled +=
      crate::library::history::backfill_from_manifest(
        &mut history,
        &manifest,
        &opts.sync_dir,
      );
    if let Some(ref od) = opts.overflow_dir {
      backfilled +=
        crate::library::history::backfill_from_manifest(
          &mut history,
          &overflow_manifest,
          od,
        );
    }
    if backfilled > 0 {
      history.save()?;
      println!(
        "  {} Imported {backfilled} entries from existing manifests into download history",
        style("i").cyan()
      );
    }
  }

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
      &overflow_manifest,
      &history,
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
  let mut using_overflow = false;

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
        job
          .manual_url
          .rsplit('/')
          .next()
          .unwrap_or("download")
          .to_string()
      }
    };

    // Pick destination: primary or overflow based on free space
    let (dest_root, is_overflow) =
      pick_dest_root(&opts, using_overflow);
    let dest_path =
      dest_root.join(&job.relative_dir).join(&filename);

    if is_overflow && !using_overflow {
      using_overflow = true;
      println!(
        "  {} Primary disk low on space, switching to overflow: {}",
        style("!").yellow(),
        style(dest_root.display()).bold()
      );
    }

    pb.set_message(format!(
      "{} / {}{}",
      job.display_name,
      filename,
      if is_overflow {
        format!(" → {}", style("overflow").yellow())
      } else {
        String::new()
      }
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

      let entry = ManifestEntry {
        game_id: job.game_id,
        game_slug: job.game_slug.clone(),
        file_path: dest_path
          .strip_prefix(dest_root)
          .unwrap_or(&dest_path)
          .to_string_lossy()
          .to_string(),
        manual_url: job.manual_url.clone(),
        version: job.version.clone(),
        size_bytes,
        downloaded_at: now_unix(),
      };

      let key =
        SyncManifest::key(job.game_id, &job.manual_url);
      if is_overflow {
        overflow_manifest
          .entries
          .insert(key, entry);
        overflow_manifest.save(
          opts.overflow_dir.as_ref().unwrap(),
        )?;
      } else {
        manifest.entries.insert(key, entry);
        manifest.save(&opts.sync_dir)?;
      }

      // Record in global download history
      history.record(
        job.game_id,
        &job.game_slug,
        &job.manual_url,
        &filename,
        job.version.clone(),
        size_bytes,
        &dest_path.to_string_lossy(),
      );
      history.save()?;
    }
  }

  // Summary
  println!();
  println!(
    "{} {} downloaded, {} failed, {} already up to date",
    style("Done!").green().bold(),
    downloaded,
    failed,
    manifest.entries.len() + overflow_manifest.entries.len()
      - downloaded as usize
  );

  Ok(())
}

/// Check available disk space and decide which root to use.
fn pick_dest_root(
  opts: &SyncOptions,
  already_overflowing: bool,
) -> (&PathBuf, bool) {
  if already_overflowing
    && let Some(ref od) = opts.overflow_dir
  {
    return (od, true);
  }

  if let Some(ref od) = opts.overflow_dir
    && let Ok(free) = fs2_available_space(&opts.sync_dir)
    && free < MIN_FREE_BYTES
  {
    return (od, true);
  }

  (&opts.sync_dir, false)
}

/// Get available space on the filesystem containing the given path.
fn fs2_available_space(path: &std::path::Path) -> std::io::Result<u64> {
  #[cfg(unix)]
  {
    use std::ffi::CString;
    let c_path = CString::new(
      path.to_string_lossy().as_bytes(),
    )
    .map_err(|e| {
      std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
    })?;
    unsafe {
      let mut stat: libc::statvfs = std::mem::zeroed();
      if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
        Ok(stat.f_bavail * stat.f_frsize)
      } else {
        Err(std::io::Error::last_os_error())
      }
    }
  }
  #[cfg(not(unix))]
  {
    let _ = path;
    Ok(u64::MAX)
  }
}

#[allow(clippy::too_many_arguments)]
fn collect_jobs(
  game_id: u64,
  slug: &str,
  details: &GameDetails,
  opts: &SyncOptions,
  manifest: &SyncManifest,
  overflow_manifest: &SyncManifest,
  history: &DownloadHistory,
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
        let version = installer.version.as_deref();
        if !opts.force
          && (manifest.has(
            game_id,
            &installer.manual_url,
            version,
          ) || overflow_manifest.has(
            game_id,
            &installer.manual_url,
            version,
          ) || history.has(
            game_id,
            &installer.manual_url,
            version,
          ))
        {
          continue;
        }

        let mut relative_dir =
          PathBuf::from(slug).join(platform);
        if language != "English" {
          relative_dir =
            relative_dir.join(sanitize_filename(language));
        }

        jobs.push(DownloadJob {
          game_id,
          game_slug: slug.to_string(),
          manual_url: installer.manual_url.clone(),
          relative_dir,
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
      && (manifest.has(game_id, &extra.manual_url, None)
        || overflow_manifest
          .has(game_id, &extra.manual_url, None)
        || history.has(game_id, &extra.manual_url, None))
    {
      continue;
    }

    let relative_dir = PathBuf::from(slug).join("extras");

    jobs.push(DownloadJob {
      game_id,
      game_slug: slug.to_string(),
      manual_url: extra.manual_url.clone(),
      relative_dir,
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
