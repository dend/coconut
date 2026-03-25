use reqwest::header::LOCATION;
use reqwest::redirect::Policy;
use reqwest::Client;

use crate::auth;
use crate::auth::token::Token;
use crate::error::{Error, Result};
use crate::gog::GOG_EMBED_URL;
use crate::gog::models::{FilteredProductsResponse, GameDetails};

pub struct GogClient {
  http: Client,
  no_redirect: Client,
  token: Token,
}

impl GogClient {
  pub async fn new() -> Result<Self> {
    let token = auth::ensure_token().await?;
    let http = Client::new();
    let no_redirect = Client::builder()
      .redirect(Policy::none())
      .build()?;
    Ok(Self {
      http,
      no_redirect,
      token,
    })
  }

  pub fn http(&self) -> &Client {
    &self.http
  }

  async fn get_json<T: serde::de::DeserializeOwned>(
    &mut self,
    url: &str,
  ) -> Result<T> {
    let resp = self
      .http
      .get(url)
      .query(&[("access_token", &self.token.access_token)])
      .send()
      .await?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
      self.token = auth::refresh(&self.token).await?;
      let resp = self
        .http
        .get(url)
        .query(&[("access_token", &self.token.access_token)])
        .send()
        .await?
        .error_for_status()?;
      return Ok(resp.json().await?);
    }

    let resp = resp.error_for_status()?;
    Ok(resp.json().await?)
  }

  pub async fn get_filtered_products(
    &mut self,
    page: u32,
  ) -> Result<FilteredProductsResponse> {
    let url = format!(
      "{GOG_EMBED_URL}/account/getFilteredProducts?mediaType=1&page={page}"
    );
    self.get_json(&url).await
  }

  pub async fn get_game_details(
    &mut self,
    product_id: u64,
  ) -> Result<GameDetails> {
    let url = format!(
      "{GOG_EMBED_URL}/account/gameDetails/{product_id}.json"
    );
    self.get_json(&url).await
  }

  pub async fn resolve_download_url(
    &mut self,
    manual_url: &str,
  ) -> Result<String> {
    let url = format!("{GOG_EMBED_URL}{manual_url}");
    let resp = self
      .no_redirect
      .get(&url)
      .query(&[("access_token", &self.token.access_token)])
      .send()
      .await?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
      self.token = auth::refresh(&self.token).await?;
      let resp = self
        .no_redirect
        .get(&url)
        .query(&[("access_token", &self.token.access_token)])
        .send()
        .await?;
      return Self::extract_location(resp);
    }

    Self::extract_location(resp)
  }

  fn extract_location(resp: reqwest::Response) -> Result<String> {
    resp
      .headers()
      .get(LOCATION)
      .and_then(|v| v.to_str().ok())
      .map(|s| s.to_string())
      .ok_or_else(|| {
        Error::DownloadFailed {
          url: resp.url().to_string(),
          reason: "no redirect location in download response"
            .to_string(),
        }
      })
  }
}
