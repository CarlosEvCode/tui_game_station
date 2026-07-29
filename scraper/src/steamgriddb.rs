use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

use crate::title_cleaner::TitleCleaner;

const BASE_URL: &str = "https://www.steamgriddb.com/api/v2";

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamGridSearchResult {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamGridImageItem {
    pub id: i64,
    pub url: String,
    pub thumb: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DownloadedMediaResult {
    pub cover_path: Option<PathBuf>,
    pub banner_path: Option<PathBuf>,
    pub icon_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
}

pub struct SteamGridDBClient {
    api_key: String,
    client: Client,
}

impl SteamGridDBClient {
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key.unwrap_or_else(|| {
            std::env::var("STEAMGRIDDB_API_KEY").unwrap_or_default()
        });

        Self {
            api_key: key,
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn get_media_dir() -> PathBuf {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station")
            .join("media");
        let _ = fs::create_dir_all(dir.join("covers"));
        let _ = fs::create_dir_all(dir.join("banners"));
        let _ = fs::create_dir_all(dir.join("icons"));
        dir
    }

    /// Search game by title on SteamGridDB using cleaned search query
    pub async fn search_game(&self, raw_title: &str) -> Result<Vec<SteamGridSearchResult>> {
        let url = format!("{}/search/autocomplete/{}", BASE_URL, urlencoding::encode(raw_title));
        let body = self.request_json::<Vec<SteamGridSearchResult>>(&url).await?;
        Ok(body.unwrap_or_default())
    }

    /// Get images of type: "grids" (cover), "heroes" (banner), or "icons" (icon)
    pub async fn get_images(&self, sgdb_game_id: i64, image_type: &str) -> Result<Vec<SteamGridImageItem>> {
        let params = match image_type {
            "grids" => "?limit=30",
            "heroes" => "?limit=30",
            "icons" => "?limit=30",
            _ => "?limit=30",
        };

        let url = format!("{}/{}/game/{}{}", BASE_URL, image_type, sgdb_game_id, params);
        let body = self.request_json::<Vec<SteamGridImageItem>>(&url).await?;
        Ok(body.unwrap_or_default())
    }

    /// Download 3 media types (Cover, Banner, Icon) for a game and store locally
    pub async fn download_all_media_for_game(
        &self,
        game_id: i64,
        raw_title: &str,
    ) -> Result<DownloadedMediaResult> {
        let cleaned = TitleCleaner::clean_title(raw_title);
        let mut candidates_to_try = vec![raw_title.to_string()];
        if !cleaned.is_empty() && cleaned != raw_title {
            candidates_to_try.push(cleaned);
        }

        let mut sgdb_id = None;
        for cand in &candidates_to_try {
            tracing::info!("[SteamGridDB] Searching candidate: '{}'", cand);
            if let Ok(res) = self.search_game(cand).await {
                if let Some(first) = res.first() {
                    sgdb_id = Some(first.id);
                    tracing::info!("[SteamGridDB] Candidate match for '{}' -> SGDB ID: {} ({})", cand, first.id, first.name);
                    break;
                }
            }
        }

        let sgdb_id = match sgdb_id {
            Some(id) => id,
            None => {
                tracing::warn!("[SteamGridDB] No candidates found on SteamGridDB for '{}'", raw_title);
                anyhow::bail!("No candidate found on SteamGridDB for '{}'", raw_title);
            }
        };

        let media_dir = Self::get_media_dir();
        fs::create_dir_all(media_dir.join("covers"))?;
        fs::create_dir_all(media_dir.join("banners"))?;
        fs::create_dir_all(media_dir.join("icons"))?;

        // 1. Cover / Grid
        let cover_dest = media_dir.join("covers").join(format!("{}.jpg", game_id));
        let cover_path = if let Ok(covers) = self.get_images(sgdb_id, "grids").await {
            if let Some(c) = covers.first() {
                if self.download_file_to_path(&c.url, &cover_dest).await.is_ok() {
                    Some(cover_dest)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // 2. Banner / Hero
        let banner_dest = media_dir.join("banners").join(format!("{}.jpg", game_id));
        let banner_path = if let Ok(banners) = self.get_images(sgdb_id, "heroes").await {
            if let Some(b) = banners.first() {
                if self.download_file_to_path(&b.url, &banner_dest).await.is_ok() {
                    Some(banner_dest)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // 3. Icon
        let icon_dest = media_dir.join("icons").join(format!("{}.png", game_id));
        let icon_path = if let Ok(icons) = self.get_images(sgdb_id, "icons").await {
            if let Some(i) = icons.first() {
                if self.download_file_to_path(&i.url, &icon_dest).await.is_ok() {
                    Some(icon_dest)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        tracing::info!("[SteamGridDB] Media download completed for game_id={}. Cover: {:?}", game_id, cover_path);

        Ok(DownloadedMediaResult {
            cover_path,
            banner_path,
            icon_path,
        })
    }

    async fn download_file_to_path(&self, url: &str, dest: &PathBuf) -> Result<()> {
        let resp = self
            .client
            .get(url)
            .header("User-Agent", USER_AGENTS[0])
            .send()
            .await?
            .error_for_status()?;
        let bytes = resp.bytes().await?;
        fs::write(dest, bytes)?;
        tracing::info!("[SteamGridDB] Saved media to {:?}", dest);
        Ok(())
    }

    async fn request_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<Option<T>> {
        let mut retries = 0;
        let max_retries = 3;

        while retries <= max_retries {
            let ua = USER_AGENTS[retries % USER_AGENTS.len()];
            let mut req = self.client.get(url).header("User-Agent", ua);
            if !self.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }

            let resp = req.send().await;
            match resp {
                Ok(response) => {
                    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let delay = Duration::from_secs(1 << retries);
                        sleep(delay).await;
                        retries += 1;
                        continue;
                    }

                    if response.status().is_success() {
                        let parsed: ApiResponse<T> = response.json().await.with_context(|| format!("Failed to parse JSON for {}", url))?;
                        if parsed.success {
                            return Ok(parsed.data);
                        } else {
                            return Ok(None);
                        }
                    } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                        anyhow::bail!("SteamGridDB API 401 Unauthorized: Invalid or missing API Key");
                    } else {
                        anyhow::bail!("SteamGridDB API Error: HTTP {}", response.status());
                    }
                }
                Err(err) => {
                    if retries == max_retries {
                        return Err(err.into());
                    }
                    sleep(Duration::from_secs(1)).await;
                    retries += 1;
                }
            }
        }

        Ok(None)
    }
}
