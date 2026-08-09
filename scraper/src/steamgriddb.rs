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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SteamGridSearchResult {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    base_url: String,
    media_dir: PathBuf,
}

impl SteamGridDBClient {
    pub fn new(api_key: Option<String>) -> Self {
        let key =
            api_key.unwrap_or_else(|| std::env::var("STEAMGRIDDB_API_KEY").unwrap_or_default());

        Self {
            api_key: key,
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            base_url: BASE_URL.to_string(),
            media_dir: Self::get_media_dir(),
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
        Ok(self.search_game_checked(raw_title).await?.unwrap_or_default())
    }

    /// Search returning the raw API outcome so callers can tell "no results"
    /// (`Ok(Some(empty))`) apart from an API failure (`Ok(None)` on a
    /// `success:false` body, or `Err` on HTTP/network errors).
    async fn search_game_checked(&self, raw_title: &str) -> Result<Option<Vec<SteamGridSearchResult>>> {
        let url = format!(
            "{}/search/autocomplete/{}",
            self.base_url,
            urlencoding::encode(raw_title)
        );
        self.request_json::<Vec<SteamGridSearchResult>>(&url).await
    }

    /// Get images of type: "grids" (cover), "heroes" (banner), or "icons" (icon)
    pub async fn get_images(
        &self,
        sgdb_game_id: i64,
        image_type: &str,
    ) -> Result<Vec<SteamGridImageItem>> {
        let params = match image_type {
            "grids" => "?dimensions=600x900,660x930,342x482&limit=30",
            "heroes" => "?limit=30",
            "icons" => "?limit=30",
            _ => "?limit=30",
        };

        let url = format!(
            "{}/{}/game/{}{}",
            self.base_url, image_type, sgdb_game_id, params
        );
        let body = self.request_json::<Vec<SteamGridImageItem>>(&url).await?;
        let res = body.unwrap_or_default();
        if res.is_empty() && image_type == "grids" {
            let fallback_url = format!("{}/grids/game/{}?limit=30", self.base_url, sgdb_game_id);
            if let Ok(Some(fallback_res)) = self
                .request_json::<Vec<SteamGridImageItem>>(&fallback_url)
                .await
            {
                return Ok(fallback_res);
            }
        }
        Ok(res)
    }

    /// Download 3 media types (Cover, Banner, Icon) for a game and store locally with DB status persistence
    pub async fn download_all_media_for_game(
        &self,
        db_path: Option<PathBuf>,
        game_id: i64,
        raw_title: &str,
        force: bool,
    ) -> Result<DownloadedMediaResult> {
        let media_dir = self.media_dir.clone();
        fs::create_dir_all(media_dir.join("covers"))?;
        fs::create_dir_all(media_dir.join("banners"))?;
        fs::create_dir_all(media_dir.join("icons"))?;

        let cover_dest = media_dir.join("covers").join(format!("{}.jpg", game_id));
        let banner_dest = media_dir.join("banners").join(format!("{}.jpg", game_id));
        let icon_dest = media_dir.join("icons").join(format!("{}.png", game_id));

        let db = db_path.and_then(|p| game_core::db::Database::open(&p).ok());

        // Check DB statuses
        let cover_status = db
            .as_ref()
            .and_then(|d| d.get_media_status(game_id, "cover").ok().flatten());
        let banner_status = db
            .as_ref()
            .and_then(|d| d.get_media_status(game_id, "banner").ok().flatten());
        let icon_status = db
            .as_ref()
            .and_then(|d| d.get_media_status(game_id, "icon").ok().flatten());

        let need_cover =
            force || (!cover_dest.is_file() && cover_status.as_deref() != Some("not_found"));
        let need_banner =
            force || (!banner_dest.is_file() && banner_status.as_deref() != Some("not_found"));
        let need_icon =
            force || (!icon_dest.is_file() && icon_status.as_deref() != Some("not_found"));

        if !need_cover && !need_banner && !need_icon {
            return Ok(DownloadedMediaResult {
                cover_path: if cover_dest.is_file() {
                    Some(cover_dest)
                } else {
                    None
                },
                banner_path: if banner_dest.is_file() {
                    Some(banner_dest)
                } else {
                    None
                },
                icon_path: if icon_dest.is_file() {
                    Some(icon_dest)
                } else {
                    None
                },
            });
        }

        let cleaned = TitleCleaner::clean_title(raw_title);
        let mut candidates_to_try = vec![raw_title.to_string()];
        if !cleaned.is_empty() && cleaned != raw_title {
            candidates_to_try.push(cleaned);
        }

        let mut sgdb_id = None;
        let mut search_failed = false;
        for cand in &candidates_to_try {
            tracing::info!("[SteamGridDB] Searching candidate: '{}'", cand);
            match self.search_game_checked(cand).await {
                Ok(Some(res)) => {
                    if let Some(first) = res.first() {
                        sgdb_id = Some(first.id);
                        tracing::info!(
                            "[SteamGridDB] Candidate match for '{}' -> SGDB ID: {} ({})",
                            cand,
                            first.id,
                            first.name
                        );
                        break;
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        "[SteamGridDB] API reported an error (success:false) for candidate '{}'",
                        cand
                    );
                    search_failed = true;
                }
                Err(err) => {
                    tracing::warn!(
                        "[SteamGridDB] Search request failed for candidate '{}': {err:#}",
                        cand
                    );
                    search_failed = true;
                }
            }
        }

        let sgdb_id = match sgdb_id {
            Some(id) => id,
            None => {
                if search_failed {
                    // The request itself failed (bad key, rate limit, network).
                    // Record a retryable status instead of permanently marking
                    // the game as "not found", so a later session re-attempts.
                    tracing::warn!(
                        "[SteamGridDB] Search failed (retryable) for '{}'",
                        raw_title
                    );
                    if let Some(ref d) = db {
                        let _ = d.record_media_status(game_id, "cover", "failed", None, None);
                        let _ = d.record_media_status(game_id, "banner", "failed", None, None);
                        let _ = d.record_media_status(game_id, "icon", "failed", None, None);
                    }
                    anyhow::bail!(
                        "SteamGridDB search failed (retryable) for '{}'",
                        raw_title
                    );
                }
                tracing::warn!(
                    "[SteamGridDB] No candidates found on SteamGridDB for '{}'",
                    raw_title
                );
                if let Some(ref d) = db {
                    let _ = d.record_media_status(game_id, "cover", "not_found", None, None);
                    let _ = d.record_media_status(game_id, "banner", "not_found", None, None);
                    let _ = d.record_media_status(game_id, "icon", "not_found", None, None);
                }
                anyhow::bail!("No candidate found on SteamGridDB for '{}'", raw_title);
            }
        };

        // 1. Cover / Grid
        let cover_path = if cover_dest.is_file() && !force {
            Some(cover_dest.clone())
        } else if need_cover {
            if let Ok(covers) = self.get_images(sgdb_id, "grids").await {
                if let Some(c) = covers.first() {
                    if self
                        .download_file_to_path(&c.url, &cover_dest)
                        .await
                        .is_ok()
                    {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(
                                game_id,
                                "cover",
                                "downloaded",
                                Some(&cover_dest.to_string_lossy()),
                                Some(&c.url),
                            );
                        }
                        Some(cover_dest)
                    } else {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(game_id, "cover", "failed", None, None);
                        }
                        None
                    }
                } else {
                    if let Some(ref d) = db {
                        let _ = d.record_media_status(game_id, "cover", "not_found", None, None);
                    }
                    None
                }
            } else {
                None
            }
        } else if cover_dest.is_file() {
            Some(cover_dest.clone())
        } else {
            None
        };

        // 2. Banner / Hero
        let banner_path = if banner_dest.is_file() && !force {
            Some(banner_dest.clone())
        } else if need_banner {
            if let Ok(banners) = self.get_images(sgdb_id, "heroes").await {
                if let Some(b) = banners.first() {
                    if self
                        .download_file_to_path(&b.url, &banner_dest)
                        .await
                        .is_ok()
                    {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(
                                game_id,
                                "banner",
                                "downloaded",
                                Some(&banner_dest.to_string_lossy()),
                                Some(&b.url),
                            );
                        }
                        Some(banner_dest)
                    } else {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(game_id, "banner", "failed", None, None);
                        }
                        None
                    }
                } else {
                    if let Some(ref d) = db {
                        let _ = d.record_media_status(game_id, "banner", "not_found", None, None);
                    }
                    None
                }
            } else {
                None
            }
        } else if banner_dest.is_file() {
            Some(banner_dest.clone())
        } else {
            None
        };

        // 3. Icon
        let icon_path = if icon_dest.is_file() && !force {
            Some(icon_dest.clone())
        } else if need_icon {
            if let Ok(icons) = self.get_images(sgdb_id, "icons").await {
                if let Some(i) = icons.first() {
                    if self.download_file_to_path(&i.url, &icon_dest).await.is_ok() {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(
                                game_id,
                                "icon",
                                "downloaded",
                                Some(&icon_dest.to_string_lossy()),
                                Some(&i.url),
                            );
                        }
                        Some(icon_dest)
                    } else {
                        if let Some(ref d) = db {
                            let _ = d.record_media_status(game_id, "icon", "failed", None, None);
                        }
                        None
                    }
                } else {
                    if let Some(ref d) = db {
                        let _ = d.record_media_status(game_id, "icon", "not_found", None, None);
                    }
                    None
                }
            } else {
                None
            }
        } else if icon_dest.is_file() {
            Some(icon_dest.clone())
        } else {
            None
        };

        tracing::info!(
            "[SteamGridDB] Media download completed for game_id={}. Cover: {:?}",
            game_id,
            cover_path
        );

        Ok(DownloadedMediaResult {
            cover_path,
            banner_path,
            icon_path,
        })
    }

    pub async fn download_file_to_path(&self, url: &str, dest: &PathBuf) -> Result<()> {
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
                        let parsed: ApiResponse<T> = response
                            .json()
                            .await
                            .with_context(|| format!("Failed to parse JSON for {}", url))?;
                        if parsed.success {
                            return Ok(parsed.data);
                        } else {
                            return Ok(None);
                        }
                    } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                        anyhow::bail!(
                            "SteamGridDB API 401 Unauthorized: Invalid or missing API Key"
                        );
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

        anyhow::bail!("SteamGridDB API Error: rate limited (HTTP 429) after retries");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// Tiny HTTP server that answers every request with a canned response, so
    /// the client can be exercised without touching the network.
    fn spawn_mock_server(
        status_line: &'static str,
        body: &'static [u8],
        max_requests: usize,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..max_requests {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let header = format!(
                        "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status_line,
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
            }
        });
        format!("http://{}", addr)
    }

    fn test_client(base_url: String) -> SteamGridDBClient {
        SteamGridDBClient {
            api_key: "test-key".to_string(),
            client: Client::new(),
            base_url,
            media_dir: std::env::temp_dir().join(format!(
                "tui_game_station_sgdb_test_media_{}",
                std::process::id()
            )),
        }
    }

    fn temp_db(game_id: i64) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tui_game_station_sgdb_test_{}_{}.db",
            std::process::id(),
            game_id
        ));
        let _ = fs::remove_file(&path);
        path
    }

    /// Seed a real `games` row (the media table has a FK on `game_id`) and
    /// return its id.
    fn seed_test_game(db_path: &PathBuf) -> i64 {
        let db = game_core::db::Database::open(db_path).unwrap();
        let platform_id = db.get_platforms().unwrap()[0].id;
        let game = game_core::models::Game {
            id: 0,
            platform_id,
            folder_id: None,
            emulator_override: None,
            core_override: None,
            title: "Zelda".to_string(),
            sort_title: None,
            game_type: "rom".to_string(),
            file_path: Some(db_path.to_string_lossy().to_string()),
            working_dir: None,
            custom_command: None,
            env_vars: None,
            wine_prefix: None,
            wine_runner_id: None,
            steam_appid: None,
            file_name: Some("game.nds".to_string()),
            file_extension: Some(".nds".to_string()),
            file_size: None,
            file_hash_crc32: None,
            file_hash_md5: None,
            file_hash_sha1: None,
            serial: None,
            release_year: None,
            developer: None,
            publisher: None,
            description: None,
            genre: None,
            rating: None,
            favorite: false,
            play_count: 0,
            play_time_seconds: 0,
            last_played_at: None,
            created_at: String::new(),
            updated_at: String::new(),
            components: Vec::new(),
            is_missing_base: false,
        };
        db.insert_game(&game).unwrap()
    }

    #[tokio::test]
    async fn api_error_during_search_does_not_poison_media_as_not_found() {
        let base = spawn_mock_server(
            "HTTP/1.1 401 Unauthorized",
            br#"{"success":false,"errors":["Authentication Required"]}"#,
            4,
        );
        let client = test_client(base);
        let db_path = temp_db(1);
        let game_id = seed_test_game(&db_path);

        let res = client
            .download_all_media_for_game(Some(db_path.clone()), game_id, "Zelda", false)
            .await;
        assert!(res.is_err(), "a 401 must surface as an error");

        let db = game_core::db::Database::open(&db_path).unwrap();
        for media_type in ["cover", "banner", "icon"] {
            let status = db.get_media_status(game_id, media_type).unwrap();
            assert_ne!(
                status.as_deref(),
                Some("not_found"),
                "{} must not be poisoned as not_found on API error",
                media_type
            );
            assert_eq!(
                status.as_deref(),
                Some("failed"),
                "{} should be recorded as retryable 'failed'",
                media_type
            );
        }
        let _ = fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn api_error_body_during_search_does_not_poison_media_as_not_found() {
        // HTTP 200 with success:false is how the API reports errors like a bad
        // key; it must NOT be confused with "no results".
        let base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            br#"{"success":false,"errors":["Authentication Required"]}"#,
            4,
        );
        let client = test_client(base);
        let db_path = temp_db(2);
        let game_id = seed_test_game(&db_path);

        let res = client
            .download_all_media_for_game(Some(db_path.clone()), game_id, "Zelda", false)
            .await;
        assert!(res.is_err());

        let db = game_core::db::Database::open(&db_path).unwrap();
        for media_type in ["cover", "banner", "icon"] {
            assert_ne!(
                db.get_media_status(game_id, media_type).unwrap().as_deref(),
                Some("not_found"),
                "{} must not be poisoned as not_found on an API error body",
                media_type
            );
            assert_eq!(
                db.get_media_status(game_id, media_type).unwrap().as_deref(),
                Some("failed"),
                "{} should be recorded as retryable 'failed'",
                media_type
            );
        }
        let _ = fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn genuine_empty_search_records_not_found() {
        let base = spawn_mock_server("HTTP/1.1 200 OK", br#"{"success":true,"data":[]}"#, 4);
        let client = test_client(base);
        let db_path = temp_db(3);
        let game_id = seed_test_game(&db_path);

        let res = client
            .download_all_media_for_game(Some(db_path.clone()), game_id, "Zelda", false)
            .await;
        assert!(res.is_err(), "no candidate must surface as an error");

        let db = game_core::db::Database::open(&db_path).unwrap();
        for media_type in ["cover", "banner", "icon"] {
            assert_eq!(
                db.get_media_status(game_id, media_type).unwrap().as_deref(),
                Some("not_found"),
                "{} should be a genuine not_found",
                media_type
            );
        }
        let _ = fs::remove_file(&db_path);
    }
}
