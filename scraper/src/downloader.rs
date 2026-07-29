use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
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
        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        let res = client.get(url).send().await?;
        if !res.status().is_success() {
            let err_msg = format!("Download HTTP request failed: {}", res.status());
            let _ = tx.send(DownloadEvent {
                downloaded: 0,
                total: 0,
                percentage: 0.0,
                finished: true,
                error: Some(err_msg.clone()),
            }).await;
            anyhow::bail!(err_msg);
        }

        let total_size = res.content_length().unwrap_or(0);
        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;

        let mut file = File::create(dest_path.as_ref())
            .with_context(|| format!("Failed to create destination file: {:?}", dest_path.as_ref()))?;

        while let Some(item) = stream.next().await {
            let chunk = item?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            let _ = tx.send(DownloadEvent {
                downloaded,
                total: total_size,
                percentage,
                finished: false,
                error: None,
            }).await;
        }

        file.flush()?;

        // Set executable permissions on Linux
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(dest_path.as_ref()) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(dest_path.as_ref(), perms);
            }
        }

        let _ = tx.send(DownloadEvent {
            downloaded,
            total: total_size,
            percentage: 100.0,
            finished: true,
            error: None,
        }).await;

        Ok(())
    }
}
