use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamGridDBImage {
    pub id: u64,
    pub score: i32,
    pub style: Option<String>,
    pub url: String,
    pub thumb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SteamGridDBResponse {
    pub success: bool,
    pub data: Vec<SteamGridDBImage>,
}

pub struct MediaScraper {
    client: Client,
    api_key: Option<String>,
    cache_dir: PathBuf,
}

impl MediaScraper {
    pub fn new(api_key: Option<String>) -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("tui_game_station")
            .join("media");

        let _ = std::fs::create_dir_all(&cache_dir);

        let client = Client::builder()
            .user_agent("tui_game_station/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            api_key,
            cache_dir,
        }
    }

    /// Search game artwork on SteamGridDB by title
    pub async fn fetch_game_covers(&self, game_title: &str) -> Result<Vec<SteamGridDBImage>> {
        let api_key = match &self.api_key {
            Some(key) if !key.is_empty() => key,
            _ => anyhow::bail!("SteamGridDB API key is not configured"),
        };

        // 1. Search game ID on SGDB
        let search_url = format!(
            "https://www.steamgriddb.com/api/v2/search/autocomplete/{}",
            urlencoding::encode(game_title)
        );

        let res = self
            .client
            .get(&search_url)
            .bearer_auth(api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("SteamGridDB search request failed with status: {}", res.status());
        }

        let search_data: serde_json::Value = res.json().await?;
        let game_id = search_data["data"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj["id"].as_u64());

        let sgdb_game_id = match game_id {
            Some(id) => id,
            None => anyhow::bail!("No game found on SteamGridDB for query: {}", game_title),
        };

        // 2. Fetch grids/covers for game ID
        let grids_url = format!("https://www.steamgriddb.com/api/v2/grids/game/{}", sgdb_game_id);
        let grids_res = self
            .client
            .get(&grids_url)
            .bearer_auth(api_key)
            .send()
            .await?;

        let parsed: SteamGridDBResponse = grids_res.json().await?;
        Ok(parsed.data)
    }

    /// Download artwork image to local cache folder and return local filepath
    pub async fn download_image(&self, url: &str, filename: &str) -> Result<PathBuf> {
        let target_path = self.cache_dir.join(filename);
        if target_path.exists() {
            return Ok(target_path);
        }

        let resp = self.client.get(url).send().await?.bytes().await?;
        let mut file = File::create(&target_path)
            .with_context(|| format!("Failed to create cached image file at {:?}", target_path))?;
        file.write_all(&resp)?;

        Ok(target_path)
    }
}
