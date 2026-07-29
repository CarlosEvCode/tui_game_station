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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtonRepo {
    GEProton,
    ProtonCachyOS,
    DWProton,
    ProtonEM,
    WineVanillaKron4ek,
    WineStagingKron4ek,
    WineProtonKron4ek,
    ProtonTkg,
    Boxtron,
    Luxtorpeda,
    Roberta,
    SteamTinkerLaunch,
    DXVK,
    VKD3DProton,
}

impl ProtonRepo {
    pub fn api_url(&self) -> &'static str {
        match self {
            ProtonRepo::GEProton => "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases",
            ProtonRepo::ProtonCachyOS => "https://api.github.com/repos/CachyOS/proton-cachyos/releases",
            ProtonRepo::DWProton => "https://api.github.com/repos/dawn-winery/dwproton/releases",
            ProtonRepo::ProtonEM => "https://api.github.com/repos/Etaash-mathamsetty/Proton/releases",
            ProtonRepo::WineVanillaKron4ek | ProtonRepo::WineStagingKron4ek | ProtonRepo::WineProtonKron4ek => {
                "https://api.github.com/repos/Kron4ek/Wine-Builds/releases"
            }
            ProtonRepo::ProtonTkg => "https://api.github.com/repos/Frogging-Family/wine-tkg-git/releases",
            ProtonRepo::Boxtron => "https://api.github.com/repos/dreamer/boxtron/releases",
            ProtonRepo::Luxtorpeda => "https://api.github.com/repos/luxtorpeda-dev/luxtorpeda/releases",
            ProtonRepo::Roberta => "https://api.github.com/repos/dreamer/roberta/releases",
            ProtonRepo::SteamTinkerLaunch => "https://api.github.com/repos/sonic2kk/steamtinkerlaunch/releases",
            ProtonRepo::DXVK => "https://api.github.com/repos/doitsujin/dxvk/releases",
            ProtonRepo::VKD3DProton => "https://api.github.com/repos/HansKristian-Work/vkd3d-proton/releases",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ProtonRepo::GEProton => "GE-Proton (GloriousEggroll)",
            ProtonRepo::ProtonCachyOS => "Proton-CachyOS (CachyOS)",
            ProtonRepo::DWProton => "DW-Proton (Dawn Winery)",
            ProtonRepo::ProtonEM => "Proton-EM (Etaash Mathamsetty)",
            ProtonRepo::WineVanillaKron4ek => "Wine-Vanilla (Kron4ek)",
            ProtonRepo::WineStagingKron4ek => "Wine-Staging (Kron4ek)",
            ProtonRepo::WineProtonKron4ek => "Wine-Proton (Kron4ek)",
            ProtonRepo::ProtonTkg => "Proton-Tkg (Frogging-Family)",
            ProtonRepo::Boxtron => "Boxtron (MS-DOS)",
            ProtonRepo::Luxtorpeda => "Luxtorpeda",
            ProtonRepo::Roberta => "Roberta (ScummVM)",
            ProtonRepo::SteamTinkerLaunch => "SteamTinkerLaunch",
            ProtonRepo::DXVK => "DXVK (doitsujin)",
            ProtonRepo::VKD3DProton => "VKD3D-Proton (HansKristian)",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            ProtonRepo::GEProton => ProtonRepo::ProtonCachyOS,
            ProtonRepo::ProtonCachyOS => ProtonRepo::DWProton,
            ProtonRepo::DWProton => ProtonRepo::ProtonEM,
            ProtonRepo::ProtonEM => ProtonRepo::WineVanillaKron4ek,
            ProtonRepo::WineVanillaKron4ek => ProtonRepo::WineStagingKron4ek,
            ProtonRepo::WineStagingKron4ek => ProtonRepo::WineProtonKron4ek,
            ProtonRepo::WineProtonKron4ek => ProtonRepo::ProtonTkg,
            ProtonRepo::ProtonTkg => ProtonRepo::Boxtron,
            ProtonRepo::Boxtron => ProtonRepo::Luxtorpeda,
            ProtonRepo::Luxtorpeda => ProtonRepo::Roberta,
            ProtonRepo::Roberta => ProtonRepo::SteamTinkerLaunch,
            ProtonRepo::SteamTinkerLaunch => ProtonRepo::DXVK,
            ProtonRepo::DXVK => ProtonRepo::VKD3DProton,
            ProtonRepo::VKD3DProton => ProtonRepo::GEProton,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            ProtonRepo::GEProton => ProtonRepo::VKD3DProton,
            ProtonRepo::ProtonCachyOS => ProtonRepo::GEProton,
            ProtonRepo::DWProton => ProtonRepo::ProtonCachyOS,
            ProtonRepo::ProtonEM => ProtonRepo::DWProton,
            ProtonRepo::WineVanillaKron4ek => ProtonRepo::ProtonEM,
            ProtonRepo::WineStagingKron4ek => ProtonRepo::WineVanillaKron4ek,
            ProtonRepo::WineProtonKron4ek => ProtonRepo::WineStagingKron4ek,
            ProtonRepo::ProtonTkg => ProtonRepo::WineProtonKron4ek,
            ProtonRepo::Boxtron => ProtonRepo::ProtonTkg,
            ProtonRepo::Luxtorpeda => ProtonRepo::Boxtron,
            ProtonRepo::Roberta => ProtonRepo::Luxtorpeda,
            ProtonRepo::SteamTinkerLaunch => ProtonRepo::Roberta,
            ProtonRepo::DXVK => ProtonRepo::SteamTinkerLaunch,
            ProtonRepo::VKD3DProton => ProtonRepo::DXVK,
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

                // Specific filtering for Kron4ek multi-build repository
                if repo == ProtonRepo::WineVanillaKron4ek {
                    let tag_low = tag_name.to_lowercase();
                    let name_low = name.to_lowercase();
                    if tag_low.contains("staging") || tag_low.contains("proton") || name_low.contains("staging") || name_low.contains("proton") {
                        continue;
                    }
                } else if repo == ProtonRepo::WineStagingKron4ek {
                    let tag_low = tag_name.to_lowercase();
                    let name_low = name.to_lowercase();
                    if !tag_low.contains("staging") && !name_low.contains("staging") {
                        continue;
                    }
                } else if repo == ProtonRepo::WineProtonKron4ek {
                    let tag_low = tag_name.to_lowercase();
                    let name_low = name.to_lowercase();
                    if !tag_low.contains("proton") && !name_low.contains("proton") {
                        continue;
                    }
                }

                let mut chosen_asset = None;
                if let Some(assets) = item["assets"].as_array() {
                    for asset in assets {
                        let asset_name = asset["name"].as_str().unwrap_or("");
                        let lower = asset_name.to_lowercase();
                        let download_url = asset["browser_download_url"].as_str().unwrap_or("").to_string();
                        let size = asset["size"].as_u64().unwrap_or(0);

                        let is_valid_archive = (lower.ends_with(".tar.gz")
                            || lower.ends_with(".tar.xz")
                            || lower.ends_with(".tar.zst")
                            || lower.ends_with(".tgz")
                            || lower.ends_with(".zip"))
                            && !lower.contains(".sha")
                            && !lower.contains(".md5")
                            && !lower.contains(".asc")
                            && !lower.contains("aarch64")
                            && !lower.contains("arm64");

                        if is_valid_archive {
                            // For CachyOS, skip arm64 binaries if present
                            if repo == ProtonRepo::ProtonCachyOS && lower.contains("arm64") {
                                continue;
                            }

                            chosen_asset = Some(ProtonReleaseAsset {
                                name: asset_name.to_string(),
                                download_url,
                                size,
                            });
                            break;
                        }
                    }
                }

                if let Some(asset) = chosen_asset {
                    releases.push(ProtonRelease {
                        tag_name,
                        name,
                        published_at,
                        asset: Some(asset),
                    });
                }
            }
        }

        Ok(releases)
    }

    /// Download release archive with progress, extract into target_dir, and remove temp archive
    pub async fn download_and_extract(
        release: &ProtonRelease,
        target_dir: &Path,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<PathBuf> {
        let asset = release
            .asset
            .as_ref()
            .context("No downloadable archive asset found for this release")?;

        let temp_dir = std::env::temp_dir().join("tui_game_station_downloads");
        std::fs::create_dir_all(&temp_dir)?;

        let temp_archive = temp_dir.join(&asset.name);

        // 1. Download file with progress reporting
        RunnerDownloader::download_file(&asset.download_url, &temp_archive, &tx).await?;

        // 2. Ensure target installation directory exists
        std::fs::create_dir_all(target_dir)?;

        // 3. Extract using unzip for .zip or system tar for archives
        let lower_name = asset.name.to_lowercase();
        let status = if lower_name.ends_with(".zip") {
            Command::new("unzip")
                .arg("-o")
                .arg(&temp_archive)
                .arg("-d")
                .arg(target_dir)
                .status()
                .context("Failed to execute system 'unzip' command to extract archive")?
        } else {
            Command::new("tar")
                .args(["-xf"])
                .arg(&temp_archive)
                .arg("-C")
                .arg(target_dir)
                .status()
                .context("Failed to execute system 'tar' command to extract archive")?
        };

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

        Ok(target_dir.to_path_buf())
    }
}
