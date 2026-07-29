use anyhow::Result;
use reqwest::Client;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

pub struct SteamCoverResolver;

impl SteamCoverResolver {
    /// Search for local Steam grid cover image in user's Steam installation
    pub fn find_local_steam_cover(appid: i64) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let steam_user_dirs = vec![
            home.join(".local/share/Steam/userdata"),
            home.join(".steam/steam/userdata"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/userdata"),
        ];

        let extensions = ["p.jpg", "p.png", "_hero.jpg", ".jpg", ".png"];

        for user_base in steam_user_dirs {
            if !user_base.exists() {
                continue;
            }

            if let Ok(entries) = fs::read_dir(&user_base) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let grid_dir = entry.path().join("config/grid");
                    if grid_dir.exists() {
                        for ext in &extensions {
                            let candidate = grid_dir.join(format!("{}{}", appid, ext));
                            if candidate.exists() {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Download and cache cover image from official Steam CDN
    pub async fn fetch_steam_cdn_cover(appid: i64) -> Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("tui_game_station")
            .join("media");

        fs::create_dir_all(&cache_dir)?;
        let cached_file = cache_dir.join(format!("steam_{}_cover.jpg", appid));

        if cached_file.exists() {
            return Ok(cached_file);
        }

        let primary_url = format!(
            "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg",
            appid
        );
        let fallback_url = format!(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
            appid
        );

        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .build()?;

        let resp = client.get(&primary_url).send().await;
        let bytes = match resp {
            Ok(r) if r.status().is_success() => r.bytes().await.ok(),
            _ => {
                let fall = client.get(&fallback_url).send().await;
                match fall {
                    Ok(r) if r.status().is_success() => r.bytes().await.ok(),
                    _ => None,
                }
            }
        };

        if let Some(data) = bytes {
            let mut file = File::create(&cached_file)?;
            file.write_all(&data)?;
            Ok(cached_file)
        } else {
            anyhow::bail!("Failed to download cover from Steam CDN for appid {}", appid)
        }
    }

    /// Resolve cover path (local grid first, then cached/downloaded CDN)
    pub async fn resolve_cover(appid: i64) -> Option<PathBuf> {
        if let Some(local) = Self::find_local_steam_cover(appid) {
            return Some(local);
        }

        Self::fetch_steam_cdn_cover(appid).await.ok()
    }
}
