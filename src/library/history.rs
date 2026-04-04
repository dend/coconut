use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Global download history stored in the user's config directory.
/// Tracks every file ever successfully downloaded, regardless of
/// where it was saved. Survives file moves and deletions.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DownloadHistory {
  pub entries: HashMap<String, HistoryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
  pub game_id: u64,
  pub game_slug: String,
  pub manual_url: String,
  pub filename: String,
  pub version: Option<String>,
  pub size_bytes: u64,
  pub downloaded_at: u64,
  pub downloaded_to: String,
}

impl DownloadHistory {
  pub fn load() -> Result<Self> {
    let path = history_path();
    if !path.exists() {
      return Ok(Self::default());
    }
    let json = fs::read_to_string(&path)?;
    let history: DownloadHistory =
      serde_json::from_str(&json)?;
    Ok(history)
  }

  pub fn save(&self) -> Result<()> {
    let path = history_path();
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(self)?;
    fs::write(&path, json)?;
    Ok(())
  }

  pub fn key(game_id: u64, manual_url: &str) -> String {
    format!("{game_id}:{manual_url}")
  }

  pub fn has(
    &self,
    game_id: u64,
    manual_url: &str,
    version: Option<&str>,
  ) -> bool {
    let key = Self::key(game_id, manual_url);
    match self.entries.get(&key) {
      None => false,
      Some(entry) => match (version, &entry.version) {
        (Some(new_ver), Some(old_ver)) => new_ver == old_ver,
        _ => true,
      },
    }
  }

  #[allow(clippy::too_many_arguments)]
  pub fn record(
    &mut self,
    game_id: u64,
    game_slug: &str,
    manual_url: &str,
    filename: &str,
    version: Option<String>,
    size_bytes: u64,
    downloaded_to: &str,
  ) {
    let key = Self::key(game_id, manual_url);
    self.entries.insert(
      key,
      HistoryEntry {
        game_id,
        game_slug: game_slug.to_string(),
        manual_url: manual_url.to_string(),
        filename: filename.to_string(),
        version,
        size_bytes,
        downloaded_at: crate::library::manifest::now_unix(),
        downloaded_to: downloaded_to.to_string(),
      },
    );
  }
}

/// Import entries from a per-directory manifest into the global
/// history. Entries already present in history are left as-is
/// (the history entry may have richer data like filename).
/// Returns the number of newly imported entries.
pub fn backfill_from_manifest(
  history: &mut DownloadHistory,
  manifest: &crate::library::manifest::SyncManifest,
  sync_dir: &std::path::Path,
) -> usize {
  let mut imported = 0;
  for (key, entry) in &manifest.entries {
    if !history.entries.contains_key(key) {
      let full_path =
        sync_dir.join(&entry.file_path);
      let filename = full_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
      history.entries.insert(
        key.clone(),
        HistoryEntry {
          game_id: entry.game_id,
          game_slug: entry.game_slug.clone(),
          manual_url: entry.manual_url.clone(),
          filename,
          version: entry.version.clone(),
          size_bytes: entry.size_bytes,
          downloaded_at: entry.downloaded_at,
          downloaded_to: full_path
            .to_string_lossy()
            .to_string(),
        },
      );
      imported += 1;
    }
  }
  imported
}

fn history_path() -> PathBuf {
  dirs::config_dir()
    .expect("could not determine config directory")
    .join("coconut")
    .join("download_history.json")
}
