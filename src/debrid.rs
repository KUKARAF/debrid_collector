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

#[derive(Deserialize, Debug)]
pub struct Torrent {
    pub id: String,
    pub filename: String,
    pub status: String,
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub bytes: i64,
    #[serde(default)]
    pub added: String,
}

#[derive(Deserialize, Debug)]
pub struct TorrentInfo {
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct UnrestrictedLink {
    pub filename: String,
    pub download: String,
    #[serde(default)]
    pub filesize: i64,
}

pub async fn list_torrents(token: &str) -> Result<Vec<Torrent>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{DEBRID_BASE}/torrents"))
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

    let torrents: Vec<Torrent> = resp.json().await.context("failed to parse torrents")?;
    Ok(torrents)
}

pub async fn torrent_info(token: &str, id: &str) -> Result<TorrentInfo> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{DEBRID_BASE}/torrents/info/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .context("real-debrid API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("real-debrid API returned {status}: {body}");
    }

    let info: TorrentInfo = resp.json().await.context("failed to parse torrent info")?;
    Ok(info)
}

pub async fn delete_torrent(token: &str, id: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{DEBRID_BASE}/torrents/delete/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .context("real-debrid API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("real-debrid API returned {status}: {body}");
    }

    Ok(())
}

pub async fn unrestrict_link(token: &str, link: &str) -> Result<UnrestrictedLink> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{DEBRID_BASE}/unrestrict/link"))
        .bearer_auth(token)
        .form(&[("link", link)])
        .send()
        .await
        .context("real-debrid API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("real-debrid API returned {status}: {body}");
    }

    let result: UnrestrictedLink = resp.json().await.context("failed to parse unrestricted link")?;
    Ok(result)
}
