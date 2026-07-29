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

    /// Download file from URL with chunk progress reporting to mpsc channel.
    pub async fn download_with_progress<P: AsRef<Path>>(
        url: &str,
        dest_path: P,
        tx: mpsc::Sender<DownloadEvent>,
    ) -> Result<()> {
        let dest_path = dest_path.as_ref();
        let result = Self::download_file(url, dest_path, &tx).await;
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
            let status = Command::new("unzip")
                .args(["-o", "-j"])
                .arg(archive_path)
                .arg(archive_entry)
                .arg("-d")
                .arg(output_dir)
                .status()
                .context(
                    "Failed to start unzip; install the 'unzip' package to download melonDS",
                )?;
            if !status.success() || !executable_path.is_file() {
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
            })
            .await;
    }
}
