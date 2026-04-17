use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEBRID_BASE: &str = "https://api.real-debrid.com/rest/1.0";

#[derive(Deserialize, Serialize, Debug)]
pub struct Download {
    pub id: String,
    pub filename: String,
    pub link: String,
    #[serde(default)]
    pub filesize: i64,
    #[serde(default)]
    pub download: String,
    #[serde(default)]
    pub generated: String,
}

pub async fn list_downloads(token: &str) -> Result<Vec<Download>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{DEBRID_BASE}/downloads"))
        .bearer_auth(token)
        .query(&[("page", "1"), ("limit", "100")])
        .send()
        .await
        .context("real-debrid API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("real-debrid API returned {status}: {body}");
    }

    let downloads: Vec<Download> = resp.json().await.context("failed to parse downloads")?;
    Ok(downloads)
}
