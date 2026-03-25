#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilteredProductsResponse {
  pub total_products: u32,
  pub total_pages: u32,
  pub products_per_page: u32,
  pub page: u32,
  pub products: Vec<Product>,
  #[serde(default)]
  pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
  pub id: u64,
  pub title: String,
  pub slug: String,
  #[serde(default)]
  pub image: String,
  #[serde(default)]
  pub category: String,
  pub works_on: WorksOn,
  #[serde(default)]
  pub is_game: bool,
  #[serde(default)]
  pub dlc_count: u32,
  #[serde(default)]
  pub updates: u32,
  #[serde(default)]
  pub is_in_development: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorksOn {
  #[serde(default)]
  pub windows: bool,
  #[serde(default)]
  pub mac: bool,
  #[serde(default)]
  pub linux: bool,
}

impl WorksOn {
  pub fn summary(&self) -> String {
    let mut parts = Vec::new();
    if self.windows {
      parts.push("W");
    }
    if self.mac {
      parts.push("M");
    }
    if self.linux {
      parts.push("L");
    }
    parts.join("/")
  }
}

#[derive(Debug, Deserialize)]
pub struct Tag {
  pub id: String,
  pub name: String,
  #[serde(rename = "productCount")]
  pub product_count: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetails {
  pub title: String,
  #[serde(default)]
  pub background_image: String,
  #[serde(default)]
  pub cd_key: String,
  pub downloads: Vec<(String, PlatformDownloads)>,
  #[serde(default)]
  pub extras: Vec<Extra>,
  #[serde(default)]
  pub dlcs: Vec<serde_json::Value>,
  #[serde(default)]
  pub simple_galaxy_installers: Vec<GalaxyInstaller>,
  pub changelog: Option<String>,
  #[serde(default)]
  pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlatformDownloads {
  #[serde(default)]
  pub windows: Vec<Installer>,
  #[serde(default)]
  pub linux: Vec<Installer>,
  #[serde(default)]
  pub mac: Vec<Installer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installer {
  pub manual_url: String,
  pub name: String,
  pub version: Option<String>,
  #[serde(default)]
  pub date: Option<String>,
  pub size: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
  pub manual_url: String,
  pub name: String,
  #[serde(rename = "type")]
  pub extra_type: String,
  pub size: String,
}

#[derive(Debug, Deserialize)]
pub struct GalaxyInstaller {
  pub path: String,
  pub os: String,
}
