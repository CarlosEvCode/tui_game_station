use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadEvent {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
    pub finished: bool,
    pub error: Option<String>,
    pub task_name: Option<String>,
}

pub struct RunnerDownloader;

impl RunnerDownloader {
    /// Returns default storage path for runners: ~/.local/share/tui_game_station/runners/<folder>/<filename>
    pub fn get_runner_dir(subfolder: &str) -> Result<PathBuf> {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station")
            .join("runners")
            .join(subfolder);

        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create runner dir: {:?}", dir))?;

        Ok(dir)
    }

    /// Resolves the real download URL for Eden (via Gitea API) if the URL is an Eden sentinel.
    /// Eden's CDN does not expose a /latest/ path for AppImages, so we query the Gitea API
    /// to get the actual versioned URL at download time.
    pub async fn resolve_eden_download_url(url: &str) -> String {
        const EDEN_SENTINEL: &str = "git.eden-emu.dev";
        const EDEN_CDN: &str = "stable.eden-emu.dev";
        // Only resolve if this looks like an Eden URL
        if !url.contains(EDEN_SENTINEL) && !url.contains(EDEN_CDN) {
            return url.to_string();
        }
        let api_url = "https://git.eden-emu.dev/api/v1/repos/eden-emu/eden/releases/latest";
        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .build()
            .unwrap_or_default();
        let Ok(resp) = client.get(api_url).send().await else {
            return url.to_string();
        };
        let Ok(json) = resp.json::<serde_json::Value>().await else {
            return url.to_string();
        };
        // Find the amd64 PGO AppImage asset (not .zsync)
        if let Some(assets) = json["assets"].as_array() {
            for asset in assets {
                let name = asset["name"].as_str().unwrap_or("");
                if name.contains("amd64-clang-pgo") && name.ends_with(".AppImage") {
                    if let Some(dl_url) = asset["browser_download_url"].as_str() {
                        return dl_url.to_string();
                    }
                }
            }
        }
        url.to_string()
    }

    /// Resolves a MAME download URL from the pkgforge MAME-AppImage GitHub
    /// latest-release API. The seeded URL points at the JSON endpoint; this
    /// queries it and picks the current `anylinux-x86_64` AppImage asset, so
    /// neither the version nor the asset filename are ever hardcoded.
    pub async fn resolve_mame_download_url(url: &str) -> Result<String> {
        if !url.contains("pkgforge-dev/MAME-AppImage") {
            return Ok(url.to_string());
        }
        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .build()?;
        let resp = client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("Failed to query GitHub for the latest MAME release")?;
        if !resp.status().is_success() {
            anyhow::bail!("MAME release lookup failed with HTTP {}", resp.status());
        }
        let json = resp.json::<serde_json::Value>().await?;
        Self::select_mame_appimage_asset(&json).with_context(|| {
            "No anylinux-x86_64 AppImage asset found in the latest MAME release".to_string()
        })
    }

    /// Picks the MAME AppImage asset (contains `anylinux-x86_64`, ends with
    /// `.AppImage`, excluding `.zsync` sidecars) from a GitHub release JSON.
    fn select_mame_appimage_asset(json: &serde_json::Value) -> Option<String> {
        for asset in json["assets"].as_array()? {
            let name = asset["name"].as_str().unwrap_or("");
            if name.contains("anylinux-x86_64") && name.ends_with(".AppImage") {
                if let Some(dl_url) = asset["browser_download_url"].as_str() {
                    return Some(dl_url.to_string());
                }
            }
        }
        None
    }

    /// Download file from URL with chunk progress reporting to mpsc channel.
    pub async fn download_with_progress<P: AsRef<Path>>(
        url: &str,
        dest_path: P,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<()> {
        let dest_path = dest_path.as_ref();
        // Resolve dynamic URLs (e.g. Eden and MAME have no stable /latest/
        // AppImage path; the seed points at a release API endpoint).
        let eden_url = Self::resolve_eden_download_url(url).await;
        let resolved_url = Self::resolve_mame_download_url(&eden_url).await?;
        let result = Self::download_file(&resolved_url, dest_path, &tx).await;
        match result {
            Ok(()) => {
                Self::make_executable(dest_path);
                Self::send_finished(&tx, None).await;
                Ok(())
            }
            Err(error) => {
                Self::send_finished(&tx, Some(error.to_string())).await;
                Err(error)
            }
        }
    }

    /// Download a ZIP runner, extract one AppImage from it, then remove the archive.
    pub async fn download_zip_appimage_with_progress<P: AsRef<Path>, Q: AsRef<Path>>(
        url: &str,
        archive_path: P,
        executable_path: Q,
        archive_entry: &str,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<()> {
        let archive_path = archive_path.as_ref();
        let executable_path = executable_path.as_ref();
        let result = async {
            Self::download_file(url, archive_path, &tx).await?;
            let output_dir = executable_path
                .parent()
                .context("AppImage output path has no parent directory")?;
            let output = Command::new("unzip")
                .args(["-o", "-j"])
                .arg(archive_path)
                .arg(archive_entry)
                .arg("-d")
                .arg(output_dir)
                .output()
                .context(
                    "Failed to start unzip; install the 'unzip' package to download melonDS",
                )?;
            if !output.status.success() || !executable_path.is_file() {
                anyhow::bail!(
                    "Could not extract '{}' from the downloaded ZIP",
                    archive_entry
                );
            }
            Self::make_executable(executable_path);
            std::fs::remove_file(archive_path).with_context(|| {
                format!("Failed to remove temporary archive: {:?}", archive_path)
            })?;
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                Self::send_finished(&tx, None).await;
                Ok(())
            }
            Err(error) => {
                Self::send_finished(&tx, Some(error.to_string())).await;
                Err(error)
            }
        }
    }

    /// Download a .7z runner archive (e.g. RetroArch.7z), extract it using 7z/7zr into target_dir,
    /// find the inner AppImage, make it executable, clean up the .7z archive, and return the AppImage PathBuf.
    pub async fn download_7z_appimage_with_progress<P: AsRef<Path>, Q: AsRef<Path>>(
        url: &str,
        archive_path: P,
        target_dir: Q,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<PathBuf> {
        let archive_path = archive_path.as_ref();
        let target_dir = target_dir.as_ref();

        let result: Result<PathBuf> = async {
            Self::download_file(url, archive_path, &tx).await?;

            game_core::retroarch_manager::extract_7z(archive_path, target_dir)?;

            let appimage_path = Self::find_appimage_in_dir(target_dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "No se encontró el ejecutable .AppImage dentro de la carpeta extraída de RetroArch"
                )
            })?;

            Self::make_executable(&appimage_path);

            let _ = std::fs::remove_file(archive_path);

            Ok(appimage_path)
        }
        .await;

        match result {
            Ok(path) => {
                Self::send_finished(&tx, None).await;
                Ok(path)
            }
            Err(error) => {
                Self::send_finished(&tx, Some(error.to_string())).await;
                Err(error)
            }
        }
    }

    fn find_appimage_in_dir<P: AsRef<Path>>(dir: P) -> Option<PathBuf> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext.eq_ignore_ascii_case("appimage") {
                            return Some(path);
                        }
                    }
                } else if path.is_dir() {
                    if let Some(found) = Self::find_appimage_in_dir(&path) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    pub async fn download_file(
        url: &str,
        dest_path: &Path,
        tx: &mpsc::Sender<DownloadEvent>,
    ) -> Result<()> {
        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        let res = client.get(url).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("Download HTTP request failed: {}", res.status());
        }

        let total_size = res.content_length().unwrap_or(0);
        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;

        let mut file = File::create(dest_path)
            .with_context(|| format!("Failed to create destination file: {:?}", dest_path))?;

        while let Some(item) = stream.next().await {
            let chunk = item?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            let _ = tx
                .send(DownloadEvent {
                    downloaded,
                    total: total_size,
                    percentage,
                    finished: false,
                    error: None,
                    task_name: None,
                })
                .await;
        }

        file.flush()?;

        Ok(())
    }

    fn make_executable(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(path, perms);
            }
        }
    }

    async fn send_finished(tx: &mpsc::Sender<DownloadEvent>, error: Option<String>) {
        let _ = tx
            .send(DownloadEvent {
                downloaded: 0,
                total: 0,
                percentage: 100.0,
                finished: true,
                error,
                task_name: None,
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mame_asset_selection_prefers_anylinux_x86_64_appimage() {
        let json = serde_json::json!({
            "tag_name": "mame-0.268",
            "assets": [
                {
                    "name": "MAME-0.268-anylinux-x86_64.AppImage.zsync",
                    "browser_download_url": "https://github.com/pkgforge-dev/MAME-AppImage/releases/download/mame-0.268/MAME-0.268-anylinux-x86_64.AppImage.zsync"
                },
                {
                    "name": "MAME-0.268.tar.gz",
                    "browser_download_url": "https://github.com/pkgforge-dev/MAME-AppImage/releases/download/mame-0.268/MAME-0.268.tar.gz"
                },
                {
                    "name": "MAME-0.268-anylinux-x86_64.AppImage",
                    "browser_download_url": "https://github.com/pkgforge-dev/MAME-AppImage/releases/download/mame-0.268/MAME-0.268-anylinux-x86_64.AppImage"
                }
            ]
        });
        assert_eq!(
            RunnerDownloader::select_mame_appimage_asset(&json).as_deref(),
            Some("https://github.com/pkgforge-dev/MAME-AppImage/releases/download/mame-0.268/MAME-0.268-anylinux-x86_64.AppImage")
        );
    }

    #[test]
    fn mame_asset_selection_returns_none_without_a_matching_appimage() {
        let json = serde_json::json!({
            "assets": [
                {"name": "MAME-0.268.tar.gz", "browser_download_url": "https://example.com/MAME.tar.gz"}
            ]
        });
        assert_eq!(RunnerDownloader::select_mame_appimage_asset(&json), None);
    }

    #[tokio::test]
    async fn mame_resolution_passes_through_non_mame_urls() {
        let url = "https://github.com/cemu-project/Cemu/releases/latest/download/Cemu-2.6-x86_64.AppImage";
        assert_eq!(
            RunnerDownloader::resolve_mame_download_url(url)
                .await
                .unwrap(),
            url
        );
    }

    #[test]
    fn test_find_appimage_in_dir_recursive() {
        let temp_dir = std::env::temp_dir().join(format!("test_find_appimage_{}", std::process::id()));
        let sub_dir = temp_dir.join("subfolder");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let dummy_appimage = sub_dir.join("Test-x86_64.AppImage");
        std::fs::write(&dummy_appimage, "dummy").unwrap();

        let found = RunnerDownloader::find_appimage_in_dir(&temp_dir);
        assert_eq!(found, Some(dummy_appimage));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
