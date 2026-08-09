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

    /// Download and cache cover/banner/icon media image from official Steam CDN
    pub async fn fetch_steam_cdn_media(appid: i64, media_type: &str) -> Result<PathBuf> {
        let sub_folder = match media_type {
            "banner" => "banners",
            "icon" => "icons",
            _ => "covers",
        };
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("tui_game_station")
            .join("media")
            .join(sub_folder);

        fs::create_dir_all(&cache_dir)?;
        let cached_file = cache_dir.join(format!("steam_{}_{}.jpg", appid, media_type));

        if cached_file.exists() {
            return Ok(cached_file);
        }

        let urls = match media_type {
            "banner" => vec![
                format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg", appid),
                format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/library_hero.jpg", appid),
                format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg", appid),
            ],
            "icon" => vec![
                format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/logo.png", appid),
                format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/icon.png", appid),
                format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/logo.png", appid),
            ],
            _ => vec![
                format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{}/library_600x900.jpg", appid),
                format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg", appid),
            ],
        };

        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .build()?;

        let mut bytes = None;
        for url in urls {
            if let Ok(r) = client.get(&url).send().await {
                if r.status().is_success() {
                    if let Ok(b) = r.bytes().await {
                        bytes = Some(b);
                        break;
                    }
                }
            }
        }

        if let Some(data) = bytes {
            let mut file = File::create(&cached_file)?;
            file.write_all(&data)?;
            Ok(cached_file)
        } else {
            anyhow::bail!(
                "Failed to download {} from Steam CDN for appid {}",
                media_type,
                appid
            )
        }
    }

    /// Resolve cover path (local grid first, then cached/downloaded CDN)
    pub async fn resolve_cover(appid: i64) -> Option<PathBuf> {
        Self::resolve_media(appid, "cover").await
    }

    /// Resolve media path for any media type (cover, banner, icon)
    pub async fn resolve_media(appid: i64, media_type: &str) -> Option<PathBuf> {
        if media_type == "cover" {
            if let Some(local) = Self::find_local_steam_cover(appid) {
                return Some(local);
            }
        }

        Self::fetch_steam_cdn_media(appid, media_type).await.ok()
    }
}
