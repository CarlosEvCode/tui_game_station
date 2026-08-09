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

/// Selectable scraper source matching ES-DE architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScraperSource {
    ScreenScraper,
    TheGamesDB,
}

impl ScraperSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScraperSource::ScreenScraper => "screenscraper",
            ScraperSource::TheGamesDB => "thegamesdb",
        }
    }
}

impl Default for ScraperSource {
    fn default() -> Self {
        ScraperSource::ScreenScraper
    }
}

pub struct ScraperPipelineManager {
    screenscraper: crate::screenscraper::ScreenScraperClient,
    thegamesdb: crate::thegamesdb::TheGamesDBClient,
}

impl ScraperPipelineManager {
    pub fn new(
        ss_user_id: Option<String>,
        ss_user_password: Option<String>,
        tgdb_api_key: String,
    ) -> Self {
        Self {
            screenscraper: crate::screenscraper::ScreenScraperClient::new(ss_user_id, ss_user_password),
            thegamesdb: crate::thegamesdb::TheGamesDBClient::new(tgdb_api_key),
        }
    }

    /// Dispatch search request to the chosen scraper source (screenscraper or thegamesdb)
    pub async fn search(
        &self,
        source: ScraperSource,
        params: &ScraperSearchParams,
    ) -> anyhow::Result<Vec<ScraperSearchResult>> {
        match source {
            ScraperSource::ScreenScraper => self.screenscraper.search(params).await,
            ScraperSource::TheGamesDB => self.thegamesdb.search(params).await,
        }
    }
}
