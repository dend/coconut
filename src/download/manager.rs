use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use indicatif::ProgressBar;
use reqwest::Client;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{Error, Result};

/// Extract the real filename from a CDN URL.
/// Tries the Content-Disposition header first, then falls back to the URL path.
pub async fn resolve_filename(
  http: &Client,
  cdn_url: &str,
) -> Result<String> {
  let resp = http.head(cdn_url).send().await?.error_for_status()?;

  // Try Content-Disposition: attachment; filename="setup_game.exe"
  if let Some(name) = resp
    .headers()
    .get("content-disposition")
    .and_then(|cd| cd.to_str().ok())
    .and_then(parse_content_disposition)
  {
    return Ok(name);
  }

  // Fall back to last segment of URL path
  let url = resp.url().clone();
  let path = url.path();
  if let Some(segment) = path.rsplit('/').next() {
    let decoded =
      urlencoding::decode(segment).unwrap_or(segment.into());
    if !decoded.is_empty() {
      return Ok(decoded.into_owned());
    }
  }

  // Last resort: use a hash of the URL
  Ok(format!("download_{:x}", hash_str(cdn_url)))
}

fn parse_content_disposition(header: &str) -> Option<String> {
  // Match filename*=UTF-8''name (RFC 5987) or filename="name" or filename=name
  for part in header.split(';') {
    let part = part.trim();
    if let Some(rest) = part.strip_prefix("filename*=") {
      // RFC 5987: e.g. UTF-8''my%20file.exe
      if let Some(name) = rest.split("''").nth(1) {
        let decoded =
          urlencoding::decode(name).unwrap_or(name.into());
        return Some(decoded.into_owned());
      }
    } else if let Some(rest) = part.strip_prefix("filename=") {
      let name = rest.trim_matches('"');
      if !name.is_empty() {
        return Some(name.to_string());
      }
    }
  }
  None
}

fn hash_str(s: &str) -> u64 {
  use std::hash::{Hash, Hasher};
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  s.hash(&mut hasher);
  hasher.finish()
}

pub async fn download_file(
  http: &Client,
  cdn_url: &str,
  dest: &Path,
  progress: &ProgressBar,
) -> Result<u64> {
  let part_path = part_path_for(dest);

  // Ensure parent directory exists
  if let Some(parent) = dest.parent() {
    fs::create_dir_all(parent).await?;
  }

  // Check for existing partial download
  let existing_size = match fs::metadata(&part_path).await {
    Ok(meta) => meta.len(),
    Err(_) => 0,
  };

  // HEAD request to get total size
  let head_resp = http.head(cdn_url).send().await?.error_for_status()?;
  let total_size = head_resp
    .content_length()
    .unwrap_or(0);

  if total_size > 0 {
    progress.set_length(total_size);
  }

  // If partial file is already complete
  if existing_size > 0 && existing_size >= total_size && total_size > 0 {
    fs::rename(&part_path, dest).await?;
    progress.set_position(total_size);
    return Ok(total_size);
  }

  // Build request with Range header for resume
  let mut req = http.get(cdn_url);
  if existing_size > 0 {
    req = req.header("Range", format!("bytes={existing_size}-"));
    progress.set_position(existing_size);
  }

  let resp = req.send().await?;

  // If server doesn't support range, start over
  let (mut file, start_pos) =
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
      let file = fs::OpenOptions::new()
        .append(true)
        .open(&part_path)
        .await?;
      (file, existing_size)
    } else {
      let resp_status = resp.status();
      if !resp_status.is_success() {
        return Err(Error::DownloadFailed {
          url: cdn_url.to_string(),
          reason: format!("HTTP {resp_status}"),
        });
      }
      let file = fs::File::create(&part_path).await?;
      progress.set_position(0);
      (file, 0)
    };

  // Stream response body to file with a 60s stall timeout
  let mut downloaded = start_pos;
  let mut stream = resp.bytes_stream();
  let stall_timeout = Duration::from_secs(60);
  loop {
    match tokio::time::timeout(stall_timeout, stream.next()).await
    {
      Ok(Some(chunk)) => {
        let chunk = chunk.map_err(Error::Http)?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress.set_position(downloaded);
      }
      Ok(None) => break, // stream finished
      Err(_) => {
        return Err(Error::DownloadFailed {
          url: cdn_url.to_string(),
          reason: "stalled for 60s".to_string(),
        });
      }
    }
  }
  file.flush().await?;
  drop(file);

  // Rename .part to final destination
  fs::rename(&part_path, dest).await?;
  Ok(downloaded)
}

fn part_path_for(dest: &Path) -> PathBuf {
  let mut name = dest
    .file_name()
    .unwrap_or_default()
    .to_os_string();
  name.push(".part");
  dest.with_file_name(name)
}
