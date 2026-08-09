use serde::{Deserialize, Serialize};

/// Standard parameters provided to any scraper backend (ScreenScraper, TheGamesDB, SteamGridDB, etc.)
#[derive(Debug, Clone, Default)]
pub struct ScraperSearchParams {
    /// Raw title or cleaned title override
    pub title: String,
    pub platform_slug: String,
    pub platform_id: Option<i64>,
    pub md5_hash: Option<String>,
    pub crc32_hash: Option<String>,
    pub sha1_hash: Option<String>,
    pub file_size: Option<u64>,
    pub serial: Option<String>,
    /// If true, performs strict automatic match (e.g. hash match or exact name match).
    /// If false, performs broader search (e.g. text query for user selection).
    pub automatic_mode: bool,
}

/// Metadata and asset URLs returned by a scraper backend
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScraperSearchResult {
    pub provider_name: String,
    pub game_id: String,
    pub title: String,
    pub release_year: Option<i32>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub rating: Option<f32>,
    pub cover_url: Option<String>,
    pub banner_url: Option<String>,
    pub icon_url: Option<String>,
    pub screenshot_url: Option<String>,
    pub fanart_url: Option<String>,
    pub logo_url: Option<String>,
}

/// Agnostic scraper backend trait
#[async_trait::async_trait]
pub trait ScraperProvider: Send + Sync {
    /// Identifier for the provider (e.g., "screenscraper", "thegamesdb", "steamgriddb")
    fn provider_name(&self) -> &'static str;

    /// Search for game candidates or exact match given agnostic parameters
    async fn search(&self, params: &ScraperSearchParams) -> anyhow::Result<Vec<ScraperSearchResult>>;
}
