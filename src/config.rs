use std::fs;
use std::path::PathBuf;

use crate::auth::token::Token;
use crate::error::Result;

fn config_dir() -> PathBuf {
  dirs::config_dir()
    .expect("could not determine config directory")
    .join("coconut")
}

pub fn token_path() -> PathBuf {
  config_dir().join("token.json")
}

pub fn save_token(token: &Token) -> Result<()> {
  let path = token_path();
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  let json = serde_json::to_string_pretty(token)?;
  fs::write(&path, &json)?;

  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
  }

  Ok(())
}

pub fn load_token() -> Result<Option<Token>> {
  let path = token_path();
  if !path.exists() {
    return Ok(None);
  }
  let json = fs::read_to_string(&path)?;
  let token: Token = serde_json::from_str(&json)?;
  Ok(Some(token))
}

pub fn delete_token() -> Result<()> {
  let path = token_path();
  if path.exists() {
    fs::remove_file(&path)?;
  }
  Ok(())
}

pub fn default_sync_dir() -> PathBuf {
  dirs::home_dir()
    .expect("could not determine home directory")
    .join("coconut")
    .join("library")
}
