use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("HTTP request failed: {0}")]
  Http(#[from] reqwest::Error),

  #[error("Failed to parse URL: {0}")]
  UrlParse(#[from] url::ParseError),

  #[error("JSON error: {0}")]
  Json(#[from] serde_json::Error),

  #[error("IO error: {0}")]
  Io(#[from] io::Error),

  #[error("No authorization code found in the provided URL")]
  NoAuthCode,

  #[error("Session expired and refresh failed — please log in again")]
  SessionExpired,

  #[error("Game not found: {0}")]
  GameNotFound(String),

  #[error("Download failed for {url}: {reason}")]
  DownloadFailed { url: String, reason: String },
}

pub type Result<T> = std::result::Result<T, Error>;
