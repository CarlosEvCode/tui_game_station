use anyhow::{Context, Result};
use std::path::PathBuf;
use crate::dat_parser::DatParser;

pub struct DatDownloader;

impl DatDownloader {
    /// Check if a platform slug supports and benefits from DAT auto-identification.
    pub fn supports_dat_identification(platform_slug: &str) -> bool {
        match platform_slug {
            "ps1" | "arcade" | "mame" | "snes" | "nes" | "n64" | "gb" | "gbc" | "gba" | "genesis" | "megadrive" | "dreamcast" | "master_system" | "game_gear" => true,
            _ => false,
        }
    }

    /// Resolve relative DAT file path on libretro-database for a platform slug.
    pub fn get_dat_relative_path(platform_slug: &str) -> Option<&'static str> {
        if !Self::supports_dat_identification(platform_slug) {
            return None;
        }

        match platform_slug {
            "ps1" => Some("redump/Sony - PlayStation.dat"),
            "snes" => Some("no-intro/Nintendo - Super Nintendo Entertainment System.dat"),
            "nes" => Some("no-intro/Nintendo - Nintendo Entertainment System.dat"),
            "n64" => Some("no-intro/Nintendo - Nintendo 64.dat"),
            "gb" => Some("no-intro/Nintendo - Game Boy.dat"),
            "gbc" => Some("no-intro/Nintendo - Game Boy Color.dat"),
            "gba" => Some("no-intro/Nintendo - Game Boy Advance.dat"),
            "megadrive" | "genesis" => Some("no-intro/Sega - Mega Drive - Genesis.dat"),
            "dreamcast" => Some("redump/Sega - Dreamcast.dat"),
            "master_system" => Some("no-intro/Sega - Master System - Mark III.dat"),
            "game_gear" => Some("no-intro/Sega - Game Gear.dat"),
            _ => None,
        }
    }

    pub fn get_local_dat_path(platform_slug: &str) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station")
            .join("databases")
            .join(format!("{}.dat", platform_slug))
    }

    /// Check if local DAT file exists for a platform.
    pub fn is_dat_cached(platform_slug: &str) -> bool {
        Self::get_local_dat_path(platform_slug).exists()
    }

    /// Ensure the DAT file for the platform is available locally. If missing, download it asynchronously.
    pub async fn ensure_dat_downloaded(platform_slug: &str) -> Result<PathBuf> {
        let local_path = Self::get_local_dat_path(platform_slug);
        if local_path.exists() {
            return Ok(local_path);
        }

        let rel_path = Self::get_dat_relative_path(platform_slug)
            .with_context(|| format!("Platform {} does not have DAT database support", platform_slug))?;

        let url = format!(
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/{}",
            urlencoding::encode(rel_path).replace("%2F", "/")
        );

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let resp = client
            .get(&url)
            .header("User-Agent", "TUIGameStation/1.2.0 (Linux x86_64)")
            .send()
            .await
            .with_context(|| format!("Failed to download DAT for {}", platform_slug))?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to download DAT file (HTTP status {})", resp.status());
        }

        let content = resp.text().await.context("Failed to read DAT response body")?;
        std::fs::write(&local_path, content)?;

        Ok(local_path)
    }

    /// Load and parse the DAT file for a given platform.
    pub async fn load_dat_parser(platform_slug: &str) -> Result<Option<DatParser>> {
        if Self::get_dat_relative_path(platform_slug).is_none() {
            return Ok(None);
        }

        let dat_path = Self::ensure_dat_downloaded(platform_slug).await?;
        let content = std::fs::read_to_string(&dat_path)?;
        Ok(Some(DatParser::parse(&content)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_dat_relative_path() {
        assert_eq!(
            DatDownloader::get_dat_relative_path("ps1"),
            Some("redump/Sony - PlayStation.dat")
        );
        assert_eq!(
            DatDownloader::get_dat_relative_path("gba"),
            Some("no-intro/Nintendo - Game Boy Advance.dat")
        );
        assert_eq!(DatDownloader::get_dat_relative_path("ds"), None);
        assert_eq!(DatDownloader::get_dat_relative_path("ps2"), None);
        assert_eq!(DatDownloader::get_dat_relative_path("unknown"), None);
    }
}
