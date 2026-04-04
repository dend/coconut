use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SyncManifest {
  pub entries: HashMap<String, ManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
  pub game_id: u64,
  pub game_slug: String,
  pub file_path: String,
  pub manual_url: String,
  pub version: Option<String>,
  pub size_bytes: u64,
  pub downloaded_at: u64,
}

impl SyncManifest {
  pub fn load(sync_dir: &Path) -> Result<Self> {
    let path = manifest_path(sync_dir);
    if !path.exists() {
      return Ok(Self::default());
    }
    let json = fs::read_to_string(&path)?;
    let manifest: SyncManifest = serde_json::from_str(&json)?;
    Ok(manifest)
  }

  pub fn save(&self, sync_dir: &Path) -> Result<()> {
    let path = manifest_path(sync_dir);
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
}

fn manifest_path(sync_dir: &Path) -> PathBuf {
  sync_dir.join(".coconut-manifest.json")
}

pub fn now_unix() -> u64 {
  use std::time::{SystemTime, UNIX_EPOCH};
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system clock before UNIX epoch")
    .as_secs()
}
