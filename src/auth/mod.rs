pub mod constants;
pub mod token;

use std::io::{self, BufRead};

use reqwest::Client;
use url::Url;

use crate::config;
use crate::error::{Error, Result};
use constants::*;
use token::Token;

pub async fn login() -> Result<Token> {
  let auth_url = format!(
    "{GOG_AUTH_BASE_URL}/auth\
     ?client_id={GOG_GALAXY_CLIENT_ID}\
     &redirect_uri={}\
     &response_type=code\
     &layout=client2",
    urlencoding(GOG_REDIRECT_URI),
  );

  if open::that(&auth_url).is_err() {
    eprintln!("Could not open browser. Please visit this URL manually:");
    eprintln!("{auth_url}");
  } else {
    eprintln!("Opened GOG login in your browser.");
  }

  eprintln!();
  eprintln!("After logging in, copy the URL from your browser's address bar and paste it here:");
  eprint!("> ");

  let line = io::stdin()
    .lock()
    .lines()
    .next()
    .ok_or(Error::NoAuthCode)?
    .map_err(Error::Io)?;

  let code = extract_code(&line)?;
  let token = exchange_code(&code).await?;
  config::save_token(&token)?;
  Ok(token)
}

pub async fn refresh(token: &Token) -> Result<Token> {
  let client = Client::new();
  let url = format!("{GOG_AUTH_BASE_URL}/token");

  let resp = client
    .get(&url)
    .query(&[
      ("client_id", GOG_GALAXY_CLIENT_ID),
      ("client_secret", GOG_GALAXY_CLIENT_SECRET),
      ("grant_type", "refresh_token"),
      ("refresh_token", &token.refresh_token),
    ])
    .send()
    .await?
    .error_for_status()
    .map_err(|_| Error::SessionExpired)?;

  let new_token: Token = resp.json().await?;
  config::save_token(&new_token)?;
  Ok(new_token)
}

pub async fn ensure_token() -> Result<Token> {
  let stored = config::load_token()?;

  match stored {
    None => login().await,
    Some(token) if token.is_expired() => {
      match refresh(&token).await {
        Ok(new_token) => Ok(new_token),
        Err(_) => {
          eprintln!("Token refresh failed. Please log in again.");
          login().await
        }
      }
    }
    Some(token) => Ok(token),
  }
}

async fn exchange_code(code: &str) -> Result<Token> {
  let client = Client::new();
  let url = format!("{GOG_AUTH_BASE_URL}/token");

  let resp = client
    .get(&url)
    .query(&[
      ("client_id", GOG_GALAXY_CLIENT_ID),
      ("client_secret", GOG_GALAXY_CLIENT_SECRET),
      ("grant_type", "authorization_code"),
      ("code", code),
      ("redirect_uri", GOG_REDIRECT_URI),
    ])
    .send()
    .await?
    .error_for_status()?;

  let token: Token = resp.json().await?;
  Ok(token)
}

fn extract_code(input: &str) -> Result<String> {
  let trimmed = input.trim();

  // If the user pasted just the code itself
  if !trimmed.contains("://") {
    return Ok(trimmed.to_string());
  }

  let url = Url::parse(trimmed)?;
  url
    .query_pairs()
    .find(|(key, _)| key == "code")
    .map(|(_, value)| value.into_owned())
    .ok_or(Error::NoAuthCode)
}

fn urlencoding(s: &str) -> String {
  url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
