use anyhow::{Context, Result};
use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::mpsc;
use crate::downloader::{DownloadEvent, RunnerDownloader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtonReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtonRelease {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub asset: Option<ProtonReleaseAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonRepo {
    GEProton,
    ProtonCachyOS,
}

impl ProtonRepo {
    pub fn api_url(&self) -> &'static str {
        match self {
            ProtonRepo::GEProton => "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases",
            ProtonRepo::ProtonCachyOS => "https://api.github.com/repos/cachyos/proton-cachyos/releases",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ProtonRepo::GEProton => "GE-Proton (GloriousEggroll)",
            ProtonRepo::ProtonCachyOS => "Proton-CachyOS (CachyOS)",
        }
    }
}

pub struct ProtonDownloaderClient;

impl ProtonDownloaderClient {
    /// Fetch list of releases from GitHub repository
    pub async fn fetch_releases(repo: ProtonRepo, page: usize, per_page: usize) -> Result<Vec<ProtonRelease>> {
        let url = format!("{}?page={}&per_page={}", repo.api_url(), page, per_page);
        let client = reqwest::Client::new();

        let resp = client
            .get(&url)
            .header(USER_AGENT, "tui-game-station/0.1.0")
            .send()
            .await?
            .error_for_status()?;

        let json_array: serde_json::Value = resp.json().await?;
        let mut releases = Vec::new();

        if let Some(arr) = json_array.as_array() {
            for item in arr {
                let tag_name = item["tag_name"].as_str().unwrap_or("").to_string();
                let name = item["name"].as_str().unwrap_or(&tag_name).to_string();
                let published_at = item["published_at"].as_str().unwrap_or("").to_string();

                let mut chosen_asset = None;
                if let Some(assets) = item["assets"].as_array() {
                    for asset in assets {
                        let asset_name = asset["name"].as_str().unwrap_or("");
                        let lower = asset_name.to_lowercase();
                        let download_url = asset["browser_download_url"].as_str().unwrap_or("").to_string();
                        let size = asset["size"].as_u64().unwrap_or(0);

                        if lower.ends_with(".tar.gz")
                            && !lower.contains(".sha")
                            && !lower.contains(".md5")
                            && !lower.contains(".asc")
                            && !lower.contains("aarch64")
                        {
                            chosen_asset = Some(ProtonReleaseAsset {
                                name: asset_name.to_string(),
                                download_url,
                                size,
                            });
                            break;
                        }
                    }
                }

                if chosen_asset.is_some() {
                    releases.push(ProtonRelease {
                        tag_name,
                        name,
                        published_at,
                        asset: chosen_asset,
                    });
                }
            }
        }

        Ok(releases)
    }

    /// Download .tar.gz release archive with progress, extract into target_dir using `tar -xzf`, and remove temp archive
    pub async fn download_and_extract(
        release: &ProtonRelease,
        target_dir: &Path,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<PathBuf> {
        let asset = release
            .asset
            .as_ref()
            .context("No downloadable .tar.gz asset found for this release")?;

        let temp_dir = std::env::temp_dir().join("tui_game_station_downloads");
        std::fs::create_dir_all(&temp_dir)?;

        let temp_archive = temp_dir.join(&asset.name);

        // 1. Download file with progress reporting
        RunnerDownloader::download_file(&asset.download_url, &temp_archive, &tx).await?;

        // 2. Ensure target installation directory exists
        std::fs::create_dir_all(target_dir)?;

        // 3. Extract using tar -xzf
        let status = Command::new("tar")
            .args(["-xzf"])
            .arg(&temp_archive)
            .arg("-C")
            .arg(target_dir)
            .status()
            .context("Failed to execute system 'tar' command to extract Proton archive")?;

        if !status.success() {
            let _ = std::fs::remove_file(&temp_archive);
            anyhow::bail!("Extraction of '{}' failed with exit code: {:?}", asset.name, status.code());
        }

        // 4. Remove temp archive
        let _ = std::fs::remove_file(&temp_archive);

        tx.send(DownloadEvent {
            downloaded: asset.size,
            total: asset.size,
            percentage: 100.0,
            finished: true,
            error: None,
        })
        .await
        .ok();

        Ok(target_dir.join(&release.tag_name))
    }
}
