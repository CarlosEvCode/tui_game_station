use std::collections::HashMap;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};
use urlencoding::encode;

use crate::pipeline::{ScraperProvider, ScraperSearchParams, ScraperSearchResult};

const API_BASE_URL: &str = "https://api.thegamesdb.net/v1";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TheGamesDBAllowance {
    pub remaining_monthly_allowance: Option<u32>,
    pub extra_allowance: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TheGamesDBGameItem {
    id: u64,
    game_title: String,
    release_date: Option<String>,
    developers: Option<Vec<u64>>,
    publishers: Option<Vec<u64>>,
    genres: Option<Vec<u64>>,
    overview: Option<String>,
    rating: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TheGamesDBBoxartItem {
    side: Option<String>,
    filename: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TheGamesDBBaseUrl {
    medium: Option<String>,
    original: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TheGamesDBIncludeData {
    boxart: Option<TheGamesDBBoxartData>,
}

#[derive(Debug, Clone, Deserialize)]
struct TheGamesDBBoxartData {
    base_url: Option<TheGamesDBBaseUrl>,
    data: Option<HashMap<String, Vec<TheGamesDBBoxartItem>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TheGamesDBGameResponse {
    code: u32,
    remaining_monthly_allowance: Option<u32>,
    data: Option<TheGamesDBGameContainer>,
    include: Option<TheGamesDBIncludeData>,
}

#[derive(Debug, Clone, Deserialize)]
struct TheGamesDBGameContainer {
    games: Option<Vec<TheGamesDBGameItem>>,
}

pub struct TheGamesDBClient {
    client: Client,
    api_key: String,
    platform_map: HashMap<String, u32>,
}

impl TheGamesDBClient {
    pub fn new(api_key: String) -> Self {
        let mut platform_map = HashMap::new();
        // Mappings for TheGamesDB platform_id
        platform_map.insert("nes".to_string(), 7);
        platform_map.insert("snes".to_string(), 6);
        platform_map.insert("n64".to_string(), 3);
        platform_map.insert("gamecube".to_string(), 2);
        platform_map.insert("wii".to_string(), 9);
        platform_map.insert("wiiu".to_string(), 38);
        platform_map.insert("switch".to_string(), 4971);
        platform_map.insert("gb".to_string(), 4);
        platform_map.insert("gbc".to_string(), 41);
        platform_map.insert("gba".to_string(), 5);
        platform_map.insert("nds".to_string(), 8);
        platform_map.insert("3ds".to_string(), 4912);
        platform_map.insert("megadrive".to_string(), 18);
        platform_map.insert("genesis".to_string(), 18);
        platform_map.insert("master-system".to_string(), 35);
        platform_map.insert("gamegear".to_string(), 20);
        platform_map.insert("saturn".to_string(), 17);
        platform_map.insert("dreamcast".to_string(), 16);
        platform_map.insert("psx".to_string(), 10);
        platform_map.insert("ps1".to_string(), 10);
        platform_map.insert("ps2".to_string(), 11);
        platform_map.insert("ps3".to_string(), 12);
        platform_map.insert("psp".to_string(), 13);
        platform_map.insert("psvita".to_string(), 39);
        platform_map.insert("arcade".to_string(), 23);
        platform_map.insert("mame".to_string(), 23);
        platform_map.insert("neogeo".to_string(), 24);

        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            api_key,
            platform_map,
        }
    }

    pub fn get_platform_id(&self, slug: &str) -> Option<u32> {
        self.platform_map.get(slug).copied()
    }
}

#[async_trait]
impl ScraperProvider for TheGamesDBClient {
    fn provider_name(&self) -> &'static str {
        "thegamesdb"
    }

    async fn search(&self, params: &ScraperSearchParams) -> Result<Vec<ScraperSearchResult>> {
        if self.api_key.trim().is_empty() {
            return Err(anyhow!("TheGamesDB API key is empty"));
        }

        let mut url = if let Some(ref md5) = params.md5_hash {
            format!("{}/Games/ByGameHash?apikey={}&hash={}&filter[type]=md5", API_BASE_URL, self.api_key, md5)
        } else if let Some(ref crc) = params.crc32_hash {
            format!("{}/Games/ByGameHash?apikey={}&hash={}&filter[type]=crc", API_BASE_URL, self.api_key, crc)
        } else {
            format!(
                "{}/Games/ByGameName?apikey={}&name={}",
                API_BASE_URL,
                self.api_key,
                encode(&params.title)
            )
        };

        if let Some(plat_id) = self.get_platform_id(&params.platform_slug) {
            url.push_str(&format!("&filter[platform]={}", plat_id));
        }

        url.push_str("&fields=players,publishers,genres,overview,rating&include=boxart");

        debug!("TheGamesDB request URL: {}", url);

        let resp = self.client.get(&url).send().await?;
        let status = resp.status();

        if status.as_u16() == 429 || status.as_u16() == 403 {
            warn!("TheGamesDB HTTP {}: Quota reached or forbidden", status);
            return Err(anyhow!("TheGamesDB quota reached or invalid key (HTTP {})", status));
        } else if !status.is_success() {
            return Err(anyhow!("TheGamesDB HTTP error: {}", status));
        }

        let game_resp: TheGamesDBGameResponse = resp.json().await?;

        if let Some(rem) = game_resp.remaining_monthly_allowance {
            debug!("TheGamesDB remaining monthly allowance: {}", rem);
        }

        let mut results = Vec::new();
        if let Some(container) = game_resp.data {
            if let Some(games) = container.games {
                for g in games {
                    let mut cover_url = None;

                    if let Some(ref inc) = game_resp.include {
                        if let Some(ref boxart_data) = inc.boxart {
                            if let Some(ref base_url) = boxart_data.base_url {
                                let base = base_url.medium.as_ref().or(base_url.original.as_ref());
                                if let (Some(base_str), Some(ref data_map)) = (base, &boxart_data.data) {
                                    if let Some(items) = data_map.get(&g.id.to_string()) {
                                        if let Some(front) = items.iter().find(|i| i.side.as_deref() == Some("front")) {
                                            cover_url = Some(format!("{}{}", base_str, front.filename));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let release_year = g.release_date.as_ref().and_then(|d| {
                        if d.len() >= 4 {
                            d[..4].parse::<i32>().ok()
                        } else {
                            None
                        }
                    });

                    results.push(ScraperSearchResult {
                        provider_name: "thegamesdb".to_string(),
                        game_id: g.id.to_string(),
                        title: g.game_title,
                        release_year,
                        developer: None,
                        publisher: None,
                        description: g.overview,
                        genre: None,
                        rating: None,
                        cover_url,
                        banner_url: None,
                        icon_url: None,
                        screenshot_url: None,
                        fanart_url: None,
                        logo_url: None,
                    });
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thegamesdb_json_parsing() {
        let json_data = r#"{
          "code": 200,
          "status": "Success",
          "remaining_monthly_allowance": 980,
          "data": {
            "count": 1,
            "games": [
              {
                "id": 53,
                "game_title": "Sonic the Hedgehog",
                "release_date": "1991-06-23",
                "overview": "Join Sonic as he races through six zones."
              }
            ]
          },
          "include": {
            "boxart": {
              "base_url": {
                "medium": "https://cdn.thegamesdb.net/images/medium/"
              },
              "data": {
                "53": [
                  {
                    "id": 17438,
                    "type": "boxart",
                    "side": "front",
                    "filename": "boxart/front/53-1.jpg"
                  }
                ]
              }
            }
          }
        }"#;

        let game_resp: TheGamesDBGameResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(game_resp.remaining_monthly_allowance, Some(980));

        let container = game_resp.data.unwrap();
        let games = container.games.unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].game_title, "Sonic the Hedgehog");
        assert_eq!(games[0].release_date.as_deref(), Some("1991-06-23"));
    }
}
