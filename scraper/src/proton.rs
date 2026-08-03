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
pub enum TargetLauncher {
    TUIGameStation,
    Steam,
    Heroic,
    Lutris,
}

impl TargetLauncher {
    pub fn is_installed(&self) -> bool {
        let home = dirs::home_dir().unwrap_or_default();
        match self {
            TargetLauncher::TUIGameStation => true,
            TargetLauncher::Steam => {
                home.join(".local/share/Steam").exists()
                    || home.join(".steam").exists()
                    || home.join(".var/app/com.valvesoftware.Steam").exists()
            }
            TargetLauncher::Heroic => {
                home.join(".config/heroic").exists()
                    || home.join(".local/share/heroic").exists()
                    || home.join(".var/app/com.heroicgameslauncher.hgl").exists()
            }
            TargetLauncher::Lutris => {
                home.join(".local/share/lutris").exists()
                    || home.join(".config/lutris").exists()
                    || home.join(".var/app/net.lutris.Lutris").exists()
            }
        }
    }

    pub fn all() -> Vec<TargetLauncher> {
        let mut list = Vec::new();
        list.push(TargetLauncher::TUIGameStation);

        if TargetLauncher::Steam.is_installed() {
            list.push(TargetLauncher::Steam);
        }
        if TargetLauncher::Heroic.is_installed() {
            list.push(TargetLauncher::Heroic);
        }
        if TargetLauncher::Lutris.is_installed() {
            list.push(TargetLauncher::Lutris);
        }

        list
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TargetLauncher::TUIGameStation => "TUI Game Station",
            TargetLauncher::Steam => "Steam",
            TargetLauncher::Heroic => "Heroic Games Launcher",
            TargetLauncher::Lutris => "Lutris",
        }
    }

    pub fn valid_repos(&self) -> Vec<ProtonRepo> {
        match self {
            TargetLauncher::Steam => vec![
                ProtonRepo::GEProton,
                ProtonRepo::ProtonCachyOS,
                ProtonRepo::DWProton,
                ProtonRepo::ProtonEM,
                ProtonRepo::ProtonTkg,
                ProtonRepo::Boxtron,
                ProtonRepo::Luxtorpeda,
                ProtonRepo::Roberta,
            ],
            TargetLauncher::Heroic => vec![
                ProtonRepo::GEProton,
                ProtonRepo::ProtonCachyOS,
                ProtonRepo::DWProton,
                ProtonRepo::ProtonEM,
                ProtonRepo::ProtonTkg,
                ProtonRepo::WineVanillaKron4ek,
                ProtonRepo::WineStagingKron4ek,
                ProtonRepo::WineProtonKron4ek,
                ProtonRepo::Boxtron,
                ProtonRepo::Luxtorpeda,
                ProtonRepo::Roberta,
            ],
            TargetLauncher::Lutris => vec![
                ProtonRepo::GEProton,
                ProtonRepo::ProtonCachyOS,
                ProtonRepo::DWProton,
                ProtonRepo::ProtonEM,
                ProtonRepo::ProtonTkg,
                ProtonRepo::WineVanillaKron4ek,
                ProtonRepo::WineStagingKron4ek,
                ProtonRepo::WineProtonKron4ek,
                ProtonRepo::Boxtron,
                ProtonRepo::Luxtorpeda,
                ProtonRepo::Roberta,
                ProtonRepo::DXVK,
                ProtonRepo::VKD3DProton,
            ],
            TargetLauncher::TUIGameStation => vec![
                ProtonRepo::GEProton,
                ProtonRepo::ProtonCachyOS,
                ProtonRepo::DWProton,
                ProtonRepo::ProtonEM,
                ProtonRepo::ProtonTkg,
                ProtonRepo::WineVanillaKron4ek,
                ProtonRepo::WineStagingKron4ek,
                ProtonRepo::WineProtonKron4ek,
                ProtonRepo::Boxtron,
                ProtonRepo::Luxtorpeda,
                ProtonRepo::Roberta,
                ProtonRepo::DXVK,
                ProtonRepo::VKD3DProton,
            ],
        }
    }

    pub fn installation_dir(&self, repo: ProtonRepo) -> std::path::PathBuf {
        let home = dirs::home_dir().unwrap_or_default();
        match self {
            TargetLauncher::TUIGameStation => {
                home.join(".local/share/tui_game_station/runners/wine")
            }
            TargetLauncher::Steam => {
                let flatpak = home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d");
                let native = home.join(".local/share/Steam/compatibilitytools.d");
                if native.exists() || !flatpak.parent().is_some_and(|p| p.exists()) {
                    native
                } else {
                    flatpak
                }
            }
            TargetLauncher::Heroic => {
                let is_flatpak = home.join(".var/app/com.heroicgameslauncher.hgl").exists()
                    && !home.join(".config/heroic").exists();
                let base = if is_flatpak {
                    home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/tools")
                } else {
                    home.join(".config/heroic/tools")
                };
                match repo {
                    ProtonRepo::WineVanillaKron4ek | ProtonRepo::WineStagingKron4ek | ProtonRepo::WineProtonKron4ek => {
                        base.join("wine")
                    }
                    _ => base.join("proton"),
                }
            }
            TargetLauncher::Lutris => {
                let is_flatpak = home.join(".var/app/net.lutris.Lutris").exists()
                    && !home.join(".local/share/lutris").exists();
                let base = if is_flatpak {
                    home.join(".var/app/net.lutris.Lutris/data/lutris")
                } else {
                    home.join(".local/share/lutris")
                };
                match repo {
                    ProtonRepo::DXVK | ProtonRepo::VKD3DProton => {
                        base.join("runtime")
                    }
                    _ => base.join("runners/wine"),
                }
            }
        }
    }

    pub fn next(&self) -> Self {
        let available = Self::all();
        if available.is_empty() {
            return TargetLauncher::TUIGameStation;
        }
        let pos = available.iter().position(|l| l == self).unwrap_or(0);
        let next_pos = (pos + 1) % available.len();
        available[next_pos]
    }

    pub fn prev(&self) -> Self {
        let available = Self::all();
        if available.is_empty() {
            return TargetLauncher::TUIGameStation;
        }
        let pos = available.iter().position(|l| l == self).unwrap_or(0);
        let prev_pos = if pos == 0 { available.len() - 1 } else { pos - 1 };
        available[prev_pos]
    }
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
    DXVK,
    VKD3DProton,
}

impl ProtonRepo {
    pub fn api_url(&self) -> &'static str {
        match self {
            ProtonRepo::GEProton => "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases",
            ProtonRepo::ProtonCachyOS => "https://api.github.com/repos/CachyOS/proton-cachyos/releases",
            ProtonRepo::DWProton => "https://dawn.wine/api/v1/repos/dawn-winery/dwproton/releases",
            ProtonRepo::ProtonEM => "https://api.github.com/repos/Etaash-mathamsetty/Proton/releases",
            ProtonRepo::WineVanillaKron4ek | ProtonRepo::WineStagingKron4ek | ProtonRepo::WineProtonKron4ek => {
                "https://api.github.com/repos/Kron4ek/Wine-Builds/releases"
            }
            ProtonRepo::ProtonTkg => "https://api.github.com/repos/Frogging-Family/wine-tkg-git/releases",
            ProtonRepo::Boxtron => "https://api.github.com/repos/dreamer/boxtron/releases",
            ProtonRepo::Luxtorpeda => "https://api.github.com/repos/luxtorpeda-dev/luxtorpeda/releases",
            ProtonRepo::Roberta => "https://api.github.com/repos/dreamer/roberta/releases",
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
            ProtonRepo::Roberta => ProtonRepo::DXVK,
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
            ProtonRepo::DXVK => ProtonRepo::Roberta,
            ProtonRepo::VKD3DProton => ProtonRepo::DXVK,
        }
    }
}

pub struct ProtonDownloaderClient;

impl ProtonDownloaderClient {
    /// Fetch list of releases from GitHub repository
    pub async fn fetch_releases(repo: ProtonRepo, page: usize, per_page: usize) -> Result<Vec<ProtonRelease>> {
        let fetch_limit = match repo {
            ProtonRepo::WineVanillaKron4ek | ProtonRepo::WineStagingKron4ek | ProtonRepo::WineProtonKron4ek => 60,
            _ => per_page,
        };
        let url = if repo == ProtonRepo::DWProton {
            format!("{}?page={}&limit={}", repo.api_url(), page, fetch_limit)
        } else {
            format!("{}?page={}&per_page={}", repo.api_url(), page, fetch_limit)
        };
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
                let raw_title = item["name"].as_str().unwrap_or("").to_string();

                let tag_low = tag_name.to_lowercase();
                let title_low = raw_title.to_lowercase();

                // Specific release-level filtering for Kron4ek multi-build repository
                if repo == ProtonRepo::WineVanillaKron4ek || repo == ProtonRepo::WineStagingKron4ek {
                    if tag_low.contains("proton") || title_low.contains("proton") {
                        continue;
                    }
                } else if repo == ProtonRepo::WineProtonKron4ek
                    && !tag_low.contains("proton") && !title_low.contains("proton") {
                        continue;
                    }

                let display_name = match repo {
                    ProtonRepo::WineVanillaKron4ek => format!("wine-{}", tag_name),
                    ProtonRepo::WineStagingKron4ek => format!("wine-staging-{}", tag_name),
                    ProtonRepo::WineProtonKron4ek => if !tag_name.is_empty() { tag_name.clone() } else { format!("wine-proton-{}", raw_title) },
                    _ => if !tag_name.is_empty() {
                        tag_name.clone()
                    } else if !raw_title.is_empty() {
                        raw_title
                    } else {
                        "Unknown".to_string()
                    },
                };
                let published_at = item["published_at"].as_str().unwrap_or("").to_string();

                let mut chosen_asset = None;
                if let Some(assets) = item["assets"].as_array() {
                    let mut best_score = -1;
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
                            && !lower.contains(".torrent")
                            && !lower.contains("aarch64")
                            && !lower.contains("arm64");

                        if !is_valid_archive {
                            continue;
                        }

                        // Specific filtering for Kron4ek multi-build repository
                        if repo == ProtonRepo::WineVanillaKron4ek {
                            if lower.contains("staging") || lower.contains("proton") || lower.contains("tkg") {
                                continue;
                            }
                        } else if repo == ProtonRepo::WineStagingKron4ek {
                            if !lower.contains("staging") || lower.contains("tkg") || lower.contains("proton") {
                                continue;
                            }
                        } else if repo == ProtonRepo::WineProtonKron4ek
                            && !lower.contains("proton") && !lower.contains("tkg") {
                                continue;
                            }

                        // For CachyOS, skip arm64 binaries if present
                        if repo == ProtonRepo::ProtonCachyOS && lower.contains("arm64") {
                            continue;
                        }

                        // Score assets: preferred arch amd64-wow64 (3) > amd64 (2) > x86 (1)
                        let arch_score = if lower.contains("amd64-wow64") || lower.contains("wow64") {
                            3
                        } else if lower.contains("amd64") || lower.contains("x86_64") {
                            2
                        } else {
                            1
                        };

                        if arch_score > best_score {
                            best_score = arch_score;
                            chosen_asset = Some(ProtonReleaseAsset {
                                name: asset_name.to_string(),
                                download_url,
                                size,
                            });
                        }
                    }
                }

                if chosen_asset.is_none() {
                    let fallback_url = item["tarball_url"].as_str()
                        .map(|s| s.to_string())
                        .or_else(|| {
                            if !tag_name.is_empty() {
                                Some(format!("https://github.com/sonic2kk/steamtinkerlaunch/archive/refs/tags/{}.tar.gz", tag_name))
                            } else {
                                None
                            }
                        });

                    if let Some(download_url) = fallback_url {
                        chosen_asset = Some(ProtonReleaseAsset {
                            name: format!("{}.tar.gz", display_name),
                            download_url,
                            size: 0,
                        });
                    }
                }

                if let Some(asset) = chosen_asset {
                    releases.push(ProtonRelease {
                        tag_name,
                        name: display_name,
                        published_at,
                        asset: Some(asset),
                    });
                }
            }
        }

        if per_page > 0 && releases.len() > per_page {
            releases.truncate(per_page);
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

        // 5. Post-extraction normalization for SteamTinkerLaunch versioned folder
        if let Ok(entries) = std::fs::read_dir(target_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let folder_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if folder_name.starts_with("steamtinkerlaunch-") {
                        let dest = target_dir.join("steamtinkerlaunch");
                        if !dest.exists() {
                            let _ = std::fs::rename(&path, &dest);
                        }
                    }
                }
            }
        }

        tx.send(DownloadEvent {
            downloaded: asset.size,
            total: asset.size,
            percentage: 100.0,
            finished: true,
            error: None,
            task_name: None,
        })
        .await
        .ok();

        Ok(target_dir.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_dwproton_releases() {
        let releases = ProtonDownloaderClient::fetch_releases(ProtonRepo::DWProton, 1, 5).await;
        assert!(releases.is_ok(), "Fetching DWProton releases failed: {:?}", releases.err());
        let list = releases.unwrap();
        assert!(!list.is_empty(), "DWProton releases list is empty");
        let first = &list[0];
        assert!(first.asset.is_some(), "First release has no asset");
        let asset = first.asset.as_ref().unwrap();
        assert!(asset.name.contains("dwproton"), "Asset name should contain dwproton: {}", asset.name);
    }

    #[tokio::test]
    async fn test_fetch_boxtron_releases() {
        let releases = ProtonDownloaderClient::fetch_releases(ProtonRepo::Boxtron, 1, 5).await;
        assert!(releases.is_ok(), "Fetching Boxtron releases failed: {:?}", releases.err());
        let list = releases.unwrap();
        assert!(!list.is_empty(), "Boxtron releases list is empty");
        let first = &list[0];
        assert!(first.asset.is_some(), "First release has no asset");
        let asset = first.asset.as_ref().unwrap();
        assert!(asset.name.contains("boxtron"), "Asset name should contain boxtron: {}", asset.name);
    }

    #[tokio::test]
    async fn test_fetch_kron4ek_releases() {
        if let Ok(vanilla) = ProtonDownloaderClient::fetch_releases(ProtonRepo::WineVanillaKron4ek, 1, 5).await {
            if !vanilla.is_empty() {
                let v_asset = vanilla[0].asset.as_ref().unwrap();
                assert!(!v_asset.name.contains("staging") && !v_asset.name.contains("proton"), "Vanilla asset should not contain staging/proton: {}", v_asset.name);
            }
        }

        if let Ok(staging) = ProtonDownloaderClient::fetch_releases(ProtonRepo::WineStagingKron4ek, 1, 5).await {
            if !staging.is_empty() {
                let s_asset = staging[0].asset.as_ref().unwrap();
                assert!(s_asset.name.contains("staging"), "Staging asset should contain staging: {}", s_asset.name);
            }
        }

        if let Ok(proton) = ProtonDownloaderClient::fetch_releases(ProtonRepo::WineProtonKron4ek, 1, 5).await {
            if !proton.is_empty() {
                let p_rel = &proton[0];
                assert!(p_rel.tag_name.contains("proton"), "WineProtonKron4ek release tag should contain proton: {}", p_rel.tag_name);
            }
        }
    }
}
