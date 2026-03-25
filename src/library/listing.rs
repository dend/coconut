use indicatif::{ProgressBar, ProgressStyle};

use crate::error::Result;
use crate::gog::client::GogClient;
use crate::gog::models::Product;

pub async fn fetch_all_products(
  client: &mut GogClient,
) -> Result<Vec<Product>> {
  let spinner = ProgressBar::new_spinner();
  spinner.set_style(
    ProgressStyle::with_template("{spinner:.cyan} {msg}")
      .unwrap(),
  );
  spinner.set_message("Fetching library...");
  spinner.enable_steady_tick(std::time::Duration::from_millis(80));

  let mut all = Vec::new();
  let mut page = 1;

  loop {
    let resp = client.get_filtered_products(page).await?;
    let total = resp.total_products;
    all.extend(resp.products);
    spinner.set_message(format!(
      "Fetching library... {}/{total} games",
      all.len()
    ));
    if page >= resp.total_pages {
      break;
    }
    page += 1;
  }

  spinner.finish_with_message(format!(
    "Found {} games in library",
    all.len()
  ));
  Ok(all)
}
