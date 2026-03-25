use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Token {
  pub access_token: String,
  pub refresh_token: String,
  pub expires_in: u64,
  pub user_id: String,
  pub token_type: String,
  pub session_id: String,
  #[serde(default = "now_unix")]
  pub obtained_at: u64,
}

impl Token {
  pub fn is_expired(&self) -> bool {
    let now = now_unix();
    self.obtained_at + self.expires_in <= now + 60
  }

  pub fn expires_at(&self) -> u64 {
    self.obtained_at + self.expires_in
  }
}

fn now_unix() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system clock before UNIX epoch")
    .as_secs()
}
