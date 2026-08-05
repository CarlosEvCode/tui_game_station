use anyhow::{anyhow, Result};
use reqwest::Client;
use scraper::downloader::DownloadEvent;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub release_notes: String,
}

/// Check GitHub API for latest release and compare with current app version.
pub async fn check_for_updates(current_version: &str) -> Result<Option<UpdateCheckResult>> {
    let client = Client::builder()
        .user_agent("tui-game-station-updater")
        .build()?;

    let url = "https://api.github.com/repos/CarlosEvCode/tui_game_station/releases/latest";
    let release: ReleaseInfo = client.get(url).send().await?.json().await?;

    let latest_tag = release.tag_name.trim_start_matches('v').to_string();

    if is_newer_version(current_version, &latest_tag) {
        let asset = release
            .assets
            .iter()
            .find(|a| a.name.contains("linux") && a.name.ends_with(".tar.gz"))
            .ok_or_else(|| anyhow!("No Linux release archive found in latest release"))?;

        Ok(Some(UpdateCheckResult {
            current_version: current_version.to_string(),
            latest_version: latest_tag,
            download_url: asset.browser_download_url.clone(),
            release_notes: release
                .body
                .unwrap_or_else(|| "No release notes provided.".to_string()),
        }))
    } else {
        Ok(None)
    }
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse_ver = |v: &str| -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };
    let c = parse_ver(current);
    let l = parse_ver(latest);
    l > c
}

pub async fn download_and_apply_update(
    download_url: &str,
    latest_version: &str,
    download_tx: mpsc::Sender<DownloadEvent>,
) -> Result<()> {
    use flate2::read::GzDecoder;
    use futures_util::StreamExt;
    use tar::Archive;

    let client = Client::builder()
        .user_agent("tui-game-station-updater")
        .build()?;

    let res = client.get(download_url).send().await?;
    let total_bytes = res.content_length().unwrap_or(0);

    let mut stream = res.bytes_stream();
    let mut downloaded_bytes: u64 = 0;
    let mut buffer = Vec::new();

    let task_name = format!("Updating to v{}", latest_version);

    while let Some(chunk_res) = stream.next().await {
        let chunk = match chunk_res {
            Ok(c) => c,
            Err(e) => {
                let err_msg = e.to_string();
                let _ = download_tx
                    .send(DownloadEvent {
                        downloaded: downloaded_bytes,
                        total: total_bytes,
                        percentage: 0.0,
                        finished: true,
                        error: Some(err_msg.clone()),
                        task_name: Some(task_name.clone()),
                    })
                    .await;
                return Err(anyhow::anyhow!(err_msg));
            }
        };

        buffer.extend_from_slice(&chunk);
        downloaded_bytes += chunk.len() as u64;

        let pct = if total_bytes > 0 {
            (downloaded_bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };

        let _ = download_tx
            .send(DownloadEvent {
                downloaded: downloaded_bytes,
                total: total_bytes,
                percentage: pct,
                finished: false,
                error: None,
                task_name: Some(task_name.clone()),
            })
            .await;
    }

    let gz = GzDecoder::new(&buffer[..]);
    let mut archive = Archive::new(gz);

    let temp_dir = std::env::temp_dir().join(format!("tui_game_station_update_{}", latest_version));
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    std::fs::create_dir_all(&temp_dir)?;

    archive.unpack(&temp_dir)?;

    fn find_bin(dir: &std::path::Path) -> Option<PathBuf> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(p) = find_bin(&path) {
                        return Some(p);
                    }
                } else if path.file_name().and_then(|s| s.to_str()) == Some("tui-game-station") {
                    return Some(path);
                }
            }
        }
        None
    }

    let new_bin = find_bin(&temp_dir)
        .ok_or_else(|| anyhow!("Extracted archive did not contain tui-game-station binary"))?;

    // Self-replace the running binary
    self_replace::self_replace(&new_bin)?;

    // Write a marker file so on next startup the app displays a Welcome Toast
    if let Some(app_dir) = dirs::data_dir() {
        let marker_dir = app_dir.join("tui_game_station");
        let _ = std::fs::create_dir_all(&marker_dir);
        let marker = marker_dir.join("welcome_new_version.txt");
        let _ = std::fs::write(marker, latest_version);
    }

    let _ = download_tx
        .send(DownloadEvent {
            downloaded: downloaded_bytes,
            total: total_bytes,
            percentage: 100.0,
            finished: true,
            error: None,
            task_name: Some(task_name),
        })
        .await;

    Ok(())
}
