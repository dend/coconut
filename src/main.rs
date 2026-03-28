mod auth;
mod config;
mod download;
mod error;
mod gog;
mod library;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use console::style;
use dialoguer::{Confirm, Input};

use crate::gog::client::GogClient;
use crate::library::listing::fetch_all_products;
use crate::library::sync::{SyncOptions, run_sync};

#[derive(Parser)]
#[command(name = "coconut", about = "GOG library sync tool")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Authenticate with GOG
  Auth {
    #[command(subcommand)]
    action: AuthAction,
  },
  /// Manage your GOG library
  Library {
    #[command(subcommand)]
    action: LibraryAction,
  },
}

#[derive(Subcommand)]
enum AuthAction {
  /// Log in to GOG (opens browser)
  Login,
  /// Show current auth status
  Status,
  /// Log out (remove stored tokens)
  Logout,
}

#[derive(Subcommand)]
enum LibraryAction {
  /// List all owned games
  List {
    /// Filter by platform: windows, linux, mac
    #[arg(long)]
    platform: Option<String>,
  },
  /// Show details for a specific game
  Info {
    /// Game slug or title substring
    game: String,
  },
  /// Sync game files to local storage
  Sync {
    /// Only sync a specific game (by slug or substring of title)
    #[arg(long)]
    game: Option<String>,
    /// Filter by platform: windows, linux, mac
    #[arg(long)]
    platform: Option<String>,
    /// Primary directory to sync files into
    #[arg(long)]
    sync_dir: Option<PathBuf>,
    /// Overflow directory when primary runs out of space
    #[arg(long)]
    overflow_dir: Option<PathBuf>,
    /// Re-download all files even if already present
    #[arg(long)]
    force: bool,
  },
}

#[tokio::main]
async fn main() {
  let cli = Cli::parse();

  let result = match cli.command {
    Commands::Auth { action } => match action {
      AuthAction::Login => cmd_login().await,
      AuthAction::Status => cmd_status(),
      AuthAction::Logout => cmd_logout(),
    },
    Commands::Library { action } => match action {
      LibraryAction::List { platform } => {
        cmd_library_list(platform).await
      }
      LibraryAction::Info { game } => {
        cmd_library_info(game).await
      }
      LibraryAction::Sync {
        game,
        platform,
        sync_dir,
        overflow_dir,
        force,
      } => {
        cmd_library_sync(
          game,
          platform,
          sync_dir,
          overflow_dir,
          force,
        )
        .await
      }
    },
  };

  if let Err(e) = result {
    eprintln!("{} {e}", style("Error:").red().bold());
    std::process::exit(1);
  }
}

async fn cmd_login() -> error::Result<()> {
  let token = auth::login().await?;
  eprintln!();
  println!(
    "{} Logged in successfully.",
    style("✓").green()
  );
  println!("  User ID: {}", token.user_id);
  println!(
    "  Token expires at: {}",
    format_timestamp(token.expires_at())
  );
  Ok(())
}

fn cmd_status() -> error::Result<()> {
  match config::load_token()? {
    None => {
      println!(
        "{} Not logged in.",
        style("●").dim()
      );
    }
    Some(token) => {
      println!(
        "{} Logged in.",
        style("●").green()
      );
      println!("  User ID: {}", token.user_id);
      if token.is_expired() {
        println!(
          "  Token:   {} (will refresh on next use)",
          style("expired").yellow()
        );
      } else {
        println!(
          "  Token:   expires at {}",
          format_timestamp(token.expires_at())
        );
      }
      println!(
        "  File:    {}",
        style(config::token_path().display()).dim()
      );
    }
  }
  Ok(())
}

fn cmd_logout() -> error::Result<()> {
  config::delete_token()?;
  println!(
    "{} Logged out. Token removed.",
    style("✓").green()
  );
  Ok(())
}

async fn cmd_library_list(
  platform: Option<String>,
) -> error::Result<()> {
  let mut client = GogClient::new().await?;
  let products = fetch_all_products(&mut client).await?;

  println!();
  println!(
    "{}",
    style(format!(
      " {:>6}  {:<50}  {:>6}",
      "ID", "TITLE", "PLATFORMS"
    ))
    .dim()
  );
  println!(
    "{}",
    style("─".repeat(70)).dim()
  );

  let mut count = 0;
  for p in &products {
    // Apply platform filter
    if let Some(ref pf) = platform {
      let matches = match pf.to_lowercase().as_str() {
        "windows" | "win" => p.works_on.windows,
        "linux" => p.works_on.linux,
        "mac" | "macos" => p.works_on.mac,
        _ => true,
      };
      if !matches {
        continue;
      }
    }

    let platforms = p.works_on.summary();
    let title = if p.title.len() > 50 {
      format!("{}...", &p.title[..47])
    } else {
      p.title.clone()
    };
    println!(
      " {:>6}  {:<50}  {:>6}",
      style(p.id).dim(),
      style(title).bold(),
      platforms
    );
    count += 1;
  }

  println!(
    "{}",
    style("─".repeat(70)).dim()
  );
  println!(
    " {} games",
    style(count).cyan().bold()
  );

  Ok(())
}

async fn cmd_library_info(
  game: String,
) -> error::Result<()> {
  let mut client = GogClient::new().await?;
  let products = fetch_all_products(&mut client).await?;

  let filter = game.to_lowercase();
  let product = products
    .iter()
    .find(|p| {
      p.slug.to_lowercase() == filter
        || p.title.to_lowercase().contains(&filter)
    })
    .ok_or_else(|| error::Error::GameNotFound(game.clone()))?;

  let details =
    client.get_game_details(product.id).await?;

  println!();
  println!(
    "  {}",
    style(&details.title).bold()
  );
  println!(
    "  {}",
    style(format!(
      "ID: {} | Slug: {}",
      product.id, product.slug
    ))
    .dim()
  );
  println!(
    "  Platforms: {}",
    product.works_on.summary()
  );
  println!();

  // Downloads
  for (language, platforms) in &details.downloads {
    let platform_list = [
      ("windows", &platforms.windows),
      ("linux", &platforms.linux),
      ("mac", &platforms.mac),
    ];
    for (platform, installers) in &platform_list {
      if installers.is_empty() {
        continue;
      }
      println!(
        "  {} ({language}):",
        style(platform).cyan()
      );
      for inst in *installers {
        let ver = inst
          .version
          .as_deref()
          .unwrap_or("—");
        println!(
          "    {} {} [{}]",
          style("↓").green(),
          inst.name,
          style(format!("{}, v{ver}", inst.size)).dim()
        );
      }
    }
  }

  // Extras
  if !details.extras.is_empty() {
    println!(
      "  {}:",
      style("extras").cyan()
    );
    for extra in &details.extras {
      println!(
        "    {} {} [{}]",
        style("↓").green(),
        extra.name,
        style(format!("{}, {}", extra.size, extra.extra_type))
          .dim()
      );
    }
  }

  // Features
  if !details.features.is_empty() {
    println!();
    println!(
      "  Features: {}",
      details.features.join(", ")
    );
  }

  println!();
  Ok(())
}

async fn cmd_library_sync(
  game: Option<String>,
  platform: Option<String>,
  sync_dir: Option<PathBuf>,
  overflow_dir: Option<PathBuf>,
  force: bool,
) -> error::Result<()> {
  let sync_dir = match sync_dir {
    Some(dir) => dir,
    None => {
      let default = config::default_sync_dir();
      if !default.exists() {
        println!(
          "Sync directory: {}",
          style(default.display()).bold()
        );
        let confirmed = Confirm::new()
          .with_prompt("Use this directory?")
          .default(true)
          .interact()
          .unwrap_or(true);

        if !confirmed {
          let custom: String = Input::new()
            .with_prompt("Enter sync directory")
            .interact_text()
            .map_err(|e| {
              error::Error::Io(std::io::Error::other(e))
            })?;
          PathBuf::from(shellexpand(&custom))
        } else {
          default
        }
      } else {
        default
      }
    }
  };

  let overflow_dir = overflow_dir
    .map(|p| PathBuf::from(shellexpand(&p.to_string_lossy())));

  std::fs::create_dir_all(&sync_dir)?;
  if let Some(ref od) = overflow_dir {
    std::fs::create_dir_all(od)?;
  }

  run_sync(SyncOptions {
    sync_dir,
    overflow_dir,
    game_filter: game,
    platform_filter: platform,
    force,
  })
  .await
}

fn shellexpand(path: &str) -> String {
  if let Some(rest) = path.strip_prefix("~/").and_then(|r| {
    dirs::home_dir().map(|h| h.join(r))
  }) {
    return rest.to_string_lossy().to_string();
  }
  path.to_string()
}

fn format_timestamp(unix_secs: u64) -> String {
  let secs = unix_secs as i64;
  let days_since_epoch = secs / 86400;
  let time_of_day = secs % 86400;
  let hours = time_of_day / 3600;
  let minutes = (time_of_day % 3600) / 60;

  let (year, month, day) = days_to_date(days_since_epoch);
  format!(
    "{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02} UTC"
  )
}

fn days_to_date(days: i64) -> (i64, i64, i64) {
  // Algorithm from http://howardhinnant.github.io/date_algorithms.html
  let z = days + 719468;
  let era = if z >= 0 { z } else { z - 146096 } / 146097;
  let doe = z - era * 146097;
  let yoe =
    (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  let y = yoe + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  let y = if m <= 2 { y + 1 } else { y };
  (y, m, d)
}
