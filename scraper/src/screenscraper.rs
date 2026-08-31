use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use tracing::{debug, warn};
use urlencoding::encode;

use crate::pipeline::{ScraperProvider, ScraperSearchParams, ScraperSearchResult};
use crate::title_cleaner::TitleCleaner;

const API_BASE_URL: &str = "https://api.screenscraper.fr/api2";
const DEV_ID: &str = "crlsmgllns12";
const DEV_PASSWORD: &str = "q3bwH45OOHw";
const SOFT_NAME: &str = "TuiGameStation";

/// Structure tracking quota information returned by ScreenScraper in `ssuser` node
#[derive(Debug, Clone, Default)]
pub struct ScreenScraperQuota {
    pub requests_today: u32,
    pub max_requests_per_day: u32,
    pub remaining_today: u32,
}

pub struct ScreenScraperClient {
    client: Client,
    user_id: Option<String>,
    user_password: Option<String>,
    platform_map: HashMap<String, u32>,
}

impl ScreenScraperClient {
    pub fn new(user_id: Option<String>, user_password: Option<String>) -> Self {
        let mut platform_map = HashMap::new();
        // Mappings derived from ScreenScraper API systemeid
        platform_map.insert("nes".to_string(), 3);
        platform_map.insert("snes".to_string(), 4);
        platform_map.insert("n64".to_string(), 14);
        platform_map.insert("gamecube".to_string(), 13);
        platform_map.insert("wii".to_string(), 16);
        platform_map.insert("wiiu".to_string(), 18);
        platform_map.insert("switch".to_string(), 225);
        platform_map.insert("gb".to_string(), 9);
        platform_map.insert("gbc".to_string(), 10);
        platform_map.insert("gba".to_string(), 12);
        platform_map.insert("nds".to_string(), 15);
        platform_map.insert("3ds".to_string(), 17);
        platform_map.insert("megadrive".to_string(), 1);
        platform_map.insert("genesis".to_string(), 1);
        platform_map.insert("master-system".to_string(), 2);
        platform_map.insert("gamegear".to_string(), 21);
        platform_map.insert("saturn".to_string(), 22);
        platform_map.insert("dreamcast".to_string(), 23);
        platform_map.insert("psx".to_string(), 57);
        platform_map.insert("ps1".to_string(), 57);
        platform_map.insert("ps2".to_string(), 58);
        platform_map.insert("ps3".to_string(), 59);
        platform_map.insert("psp".to_string(), 61);
        platform_map.insert("psvita".to_string(), 62);
        platform_map.insert("arcade".to_string(), 75);
        platform_map.insert("mame".to_string(), 75);
        platform_map.insert("neogeo".to_string(), 142);

        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            user_id,
            user_password,
            platform_map,
        }
    }

    /// Map internal platform slug to ScreenScraper `systemeid`
    pub fn get_system_id(&self, slug: &str) -> Option<u32> {
        self.platform_map.get(slug).copied()
    }

    /// Parse ScreenScraper XML response into ScraperSearchResult list and Quota (default options)
    pub fn parse_xml_response(
        &self,
        xml_text: &str,
    ) -> Result<(Vec<ScraperSearchResult>, ScreenScraperQuota)> {
        self.parse_xml_response_with_options(xml_text, None, None, None, None)
    }

    /// Parse ScreenScraper XML response with region preference and media type preferences
    pub fn parse_xml_response_with_options(
        &self,
        xml_text: &str,
        preferred_region: Option<&str>,
        cover_pref: Option<&str>,
        banner_pref: Option<&str>,
        icon_pref: Option<&str>,
    ) -> Result<(Vec<ScraperSearchResult>, ScreenScraperQuota)> {
        if xml_text.starts_with("Erreur : Rom") || xml_text.contains("not found") {
            return Ok((Vec::new(), ScreenScraperQuota::default()));
        }

        let doc = roxmltree::Document::parse(xml_text)
            .map_err(|e| anyhow!("Failed to parse ScreenScraper XML: {}", e))?;

        let mut quota = ScreenScraperQuota::default();
        if let Some(user_node) = doc.descendants().find(|n| n.has_tag_name("ssuser")) {
            if let Some(today) = user_node
                .children()
                .find(|n| n.has_tag_name("requeststoday"))
            {
                quota.requests_today = today.text().unwrap_or("0").parse().unwrap_or(0);
            }
            if let Some(max_day) = user_node
                .children()
                .find(|n| n.has_tag_name("maxrequestsperday"))
            {
                quota.max_requests_per_day = max_day.text().unwrap_or("0").parse().unwrap_or(0);
            }
            quota.remaining_today = quota
                .max_requests_per_day
                .saturating_sub(quota.requests_today);
        }

        let user_reg = preferred_region.unwrap_or("us").to_lowercase();
        let region_fallback_order = vec![user_reg.as_str(), "wor", "us", "eu", "jp", "ss"];

        let mut results = Vec::new();
        let juegos_node = doc
            .descendants()
            .find(|n| n.has_tag_name("jeux"))
            .unwrap_or_else(|| doc.root_element());

        for game_node in juegos_node.children().filter(|n| n.has_tag_name("jeu")) {
            let game_id = game_node.attribute("id").unwrap_or("").to_string();

            // Extract Name (regions fallback: user_region, wor, us, eu, jp, ss)
            let mut title = String::new();
            if let Some(noms_node) = game_node.children().find(|n| n.has_tag_name("noms")) {
                for reg in &region_fallback_order {
                    if let Some(nom) = noms_node
                        .children()
                        .find(|n| n.has_tag_name("nom") && n.attribute("region") == Some(*reg))
                    {
                        if let Some(txt) = nom.text() {
                            title = txt
                                .replace("&nbsp;", " ")
                                .replace("&#x26;", "&")
                                .replace("&#39;", "'");
                            break;
                        }
                    }
                }
                if title.is_empty() {
                    if let Some(nom) = noms_node.children().find(|n| n.has_tag_name("nom")) {
                        if let Some(txt) = nom.text() {
                            title = txt
                                .replace("&nbsp;", " ")
                                .replace("&#x26;", "&")
                                .replace("&#39;", "'");
                        }
                    }
                }
            }

            // Filter out ZZZ(notgame) responses
            if title.to_uppercase().starts_with("ZZZ(NOTGAME)") {
                debug!("Ignoring ScreenScraper ZZZ(notgame) entry: {}", title);
                continue;
            }

            // Release year / Date
            let mut release_year = None;
            if let Some(dates_node) = game_node.children().find(|n| n.has_tag_name("dates")) {
                if let Some(date_node) = dates_node.children().find(|n| n.has_tag_name("date")) {
                    if let Some(txt) = date_node.text() {
                        if txt.len() >= 4 {
                            if let Ok(y) = txt[..4].parse::<i32>() {
                                release_year = Some(y);
                            }
                        }
                    }
                }
            }

            // Developer
            let developer = game_node
                .children()
                .find(|n| n.has_tag_name("developpeur"))
                .and_then(|n| n.text())
                .map(|t| t.replace("&nbsp;", " "));

            // Publisher
            let publisher = game_node
                .children()
                .find(|n| n.has_tag_name("editeur"))
                .and_then(|n| n.text())
                .map(|t| t.replace("&nbsp;", " "));

            // Description
            let mut description = None;
            if let Some(syn_node) = game_node.children().find(|n| n.has_tag_name("synopsis")) {
                for lang in &["en", "wor", "es", "fr"] {
                    if let Some(s) = syn_node
                        .children()
                        .find(|n| n.has_tag_name("synopsis") && n.attribute("langue") == Some(lang))
                    {
                        if let Some(txt) = s.text() {
                            description = Some(txt.replace("&nbsp;", " ").replace("&quot;", "\""));
                            break;
                        }
                    }
                }
            }

            // Genre
            let mut genre = None;
            if let Some(genres_node) = game_node.children().find(|n| n.has_tag_name("genres")) {
                if let Some(g_node) = genres_node.children().find(|n| n.has_tag_name("genre")) {
                    genre = g_node.text().map(|t| t.to_string());
                }
            }

            // Rating note/20 -> 0-5
            let mut rating = None;
            if let Some(note_node) = game_node.children().find(|n| n.has_tag_name("note")) {
                if let Some(txt) = note_node.text() {
                    if let Ok(val) = txt.parse::<f32>() {
                        let normalized = ((val / 20.0 * 5.0) * 10.0).round() / 10.0;
                        rating = Some(normalized.clamp(0.0, 5.0));
                    }
                }
            }

            // Region-aware media extraction helper
            let get_media_url_by_type = |target_types: &[&str]| -> Option<String> {
                if let Some(medias_node) = game_node.children().find(|n| n.has_tag_name("medias")) {
                    for target_type in target_types {
                        let matching_nodes: Vec<_> = medias_node
                            .children()
                            .filter(|n| {
                                n.has_tag_name("media")
                                    && n.attribute("type").unwrap_or("").to_lowercase()
                                        == *target_type
                            })
                            .collect();

                        if matching_nodes.is_empty() {
                            continue;
                        }

                        // Try finding node matching region fallback order
                        for reg in &region_fallback_order {
                            if let Some(m) = matching_nodes
                                .iter()
                                .find(|n| n.attribute("region") == Some(*reg))
                            {
                                if let Some(txt) = m.text() {
                                    return Some(txt.replace(' ', "%20"));
                                }
                            }
                        }

                        // Fallback to first available matching node
                        if let Some(m) = matching_nodes.first() {
                            if let Some(txt) = m.text() {
                                return Some(txt.replace(' ', "%20"));
                            }
                        }
                    }
                }
                None
            };

            let cover_targets: Vec<&str> = match cover_pref.unwrap_or("box-2d") {
                "box-3d" => vec![
                    "box-3d",
                    "box-3d-front",
                    "box-2d",
                    "box-2d-front",
                    "flyer",
                    "poster",
                ],
                "support-2d" => vec![
                    "support-2d",
                    "support-3d",
                    "media-2d",
                    "media-3d",
                    "box-2d",
                    "box-3d",
                ],
                "support-3d" => vec![
                    "support-3d",
                    "support-2d",
                    "media-3d",
                    "media-2d",
                    "box-3d",
                    "box-2d",
                ],
                "mix-recalboxv1" => vec![
                    "mix-recalboxv1",
                    "mix-recalbox1",
                    "recalboxv1",
                    "mix-recalboxv2",
                    "box-2d",
                    "box-3d",
                ],
                "mix-recalboxv2" => vec![
                    "mix-recalboxv2",
                    "mix-recalbox2",
                    "recalboxv2",
                    "mix-recalboxv1",
                    "box-2d",
                    "box-3d",
                ],
                _ => vec![
                    "box-2d",
                    "box-2d-front",
                    "box-3d",
                    "box-3d-front",
                    "flyer",
                    "poster",
                    "box",
                ],
            };

            let banner_targets: Vec<&str> = match banner_pref.unwrap_or("fanart") {
                "screenmarque" => vec![
                    "screenmarque",
                    "wheel-hd",
                    "wheel",
                    "fanart",
                    "ss",
                    "sstitle",
                ],
                "ss" => vec!["ss", "sstitle", "screenshot", "fanart", "wheel"],
                "sstitle" => vec!["sstitle", "ss", "screenshot", "fanart", "wheel"],
                "wheel" => vec!["wheel", "wheel-hd", "screenmarque", "fanart"],
                _ => vec![
                    "fanart",
                    "screenmarque",
                    "wheel-hd",
                    "wheel",
                    "ss",
                    "sstitle",
                    "banner",
                ],
            };

            let icon_targets: Vec<&str> = match icon_pref.unwrap_or("wheel") {
                "steel" => vec![
                    "wheel-steel",
                    "wheel-carbon",
                    "wheel",
                    "wheel-hd",
                    "support-2d",
                ],
                "carbon" => vec![
                    "wheel-carbon",
                    "wheel-steel",
                    "wheel",
                    "wheel-hd",
                    "support-2d",
                ],
                "support-2d" => vec!["support-2d", "support-3d", "media-2d", "wheel"],
                "support-3d" => vec!["support-3d", "support-2d", "media-3d", "wheel"],
                _ => vec![
                    "wheel",
                    "wheel-hd",
                    "wheel-steel",
                    "wheel-carbon",
                    "steam-icon",
                    "icon",
                    "support-2d",
                ],
            };

            let cover_url = get_media_url_by_type(&cover_targets);
            let banner_url = get_media_url_by_type(&banner_targets);
            let icon_url = get_media_url_by_type(&icon_targets);
            let screenshot_url = get_media_url_by_type(&["ss", "sstitle", "screenshot"]);
            let fanart_url = get_media_url_by_type(&["fanart"]);
            let logo_url = get_media_url_by_type(&["wheel", "wheel-hd"]);

            results.push(ScraperSearchResult {
                provider_name: "screenscraper".to_string(),
                game_id,
                title,
                release_year,
                developer,
                publisher,
                description,
                genre,
                rating,
                cover_url,
                banner_url,
                icon_url,
                screenshot_url,
                fanart_url,
                logo_url,
            });
        }

        Ok((results, quota))
    }

    /// Search for game candidates and return both results and ScreenScraperQuota tracking info
    pub async fn search_with_quota(
        &self,
        params: &ScraperSearchParams,
    ) -> Result<(Vec<ScraperSearchResult>, ScreenScraperQuota)> {
        let is_arcade = params.platform_slug == "arcade" || params.platform_slug == "mame";
        let (query, _single_search) =
            TitleCleaner::prepare_search_query_esde(&params.title, is_arcade);

        let endpoint = if params.automatic_mode || params.md5_hash.is_some() {
            "jeuInfos.php"
        } else {
            "jeuRecherche.php"
        };

        let mut url = format!(
            "{}/{}?devid={}&devpassword={}&softname={}&output=xml",
            API_BASE_URL,
            endpoint,
            DEV_ID,
            DEV_PASSWORD,
            encode(SOFT_NAME)
        );

        if let (Some(u), Some(p)) = (&self.user_id, &self.user_password) {
            if !u.trim().is_empty() && !p.trim().is_empty() {
                url.push_str(&format!(
                    "&ssid={}&sspassword={}",
                    encode(u.trim()),
                    encode(p.trim())
                ));
            }
        }

        if let Some(sys_id) = self.get_system_id(&params.platform_slug) {
            url.push_str(&format!("&systemeid={}", sys_id));
        }

        if endpoint == "jeuInfos.php" {
            if let Some(ref md5) = params.md5_hash {
                url.push_str(&format!("&md5={}", md5));
            }
            if let Some(size) = params.file_size {
                url.push_str(&format!("&romtaille={}", size));
            }
            if let Some(ref crc) = params.crc32_hash {
                url.push_str(&format!("&crc={}", crc));
            }
            url.push_str(&format!("&romnom={}", encode(&query)));
        } else {
            url.push_str(&format!("&recherche={}", encode(&query)));
        }

        debug!("ScreenScraper request URL: {}", url);

        let resp = self.client.get(&url).send().await?;
        let status = resp.status();

        if status.as_u16() == 430 {
            warn!("ScreenScraper HTTP 430: Daily quota exceeded");
            return Err(anyhow!("ScreenScraper daily quota exceeded (HTTP 430)"));
        } else if status.as_u16() == 401 {
            warn!("ScreenScraper HTTP 401: Server busy or invalid auth");
            return Err(anyhow!(
                "ScreenScraper server busy or unauthenticated (HTTP 401)"
            ));
        } else if !status.is_success() {
            return Err(anyhow!("ScreenScraper HTTP error: {}", status));
        }

        let body = resp.text().await?;
        let (mut results, quota) = self.parse_xml_response_with_options(
            &body,
            params.preferred_region.as_deref(),
            params.cover_pref.as_deref(),
            params.banner_pref.as_deref(),
            params.icon_pref.as_deref(),
        )?;

        if results.is_empty() {
            let mut fallback_url = format!(
                "{}/jeuRecherche.php?devid={}&devpassword={}&softname={}&output=xml&recherche={}",
                API_BASE_URL,
                DEV_ID,
                DEV_PASSWORD,
                encode(SOFT_NAME),
                encode(&query)
            );
            if let (Some(u), Some(p)) = (&self.user_id, &self.user_password) {
                if !u.trim().is_empty() && !p.trim().is_empty() {
                    fallback_url.push_str(&format!(
                        "&ssid={}&sspassword={}",
                        encode(u.trim()),
                        encode(p.trim())
                    ));
                }
            }
            // First try with systemeid
            if let Some(sys_id) = self.get_system_id(&params.platform_slug) {
                let mut sys_url = fallback_url.clone();
                sys_url.push_str(&format!("&systemeid={}", sys_id));
                if let Ok(fb_resp) = self.client.get(&sys_url).send().await {
                    if fb_resp.status().is_success() {
                        if let Ok(fb_body) = fb_resp.text().await {
                            if let Ok((fb_results, _)) = self.parse_xml_response_with_options(
                                &fb_body,
                                params.preferred_region.as_deref(),
                                params.cover_pref.as_deref(),
                                params.banner_pref.as_deref(),
                                params.icon_pref.as_deref(),
                            ) {
                                results = fb_results;
                            }
                        }
                    }
                }
            }
            // Global search without systemeid if system-filtered search yielded no candidates
            if results.is_empty() {
                if let Ok(fb_resp) = self.client.get(&fallback_url).send().await {
                    if fb_resp.status().is_success() {
                        if let Ok(fb_body) = fb_resp.text().await {
                            if let Ok((fb_results, _)) = self.parse_xml_response_with_options(
                                &fb_body,
                                params.preferred_region.as_deref(),
                                params.cover_pref.as_deref(),
                                params.banner_pref.as_deref(),
                                params.icon_pref.as_deref(),
                            ) {
                                results = fb_results;
                            }
                        }
                    }
                }
            }
        }

        debug!(
            "ScreenScraper response parsed: {} results, quota remaining today: {}/{}",
            results.len(),
            quota.remaining_today,
            quota.max_requests_per_day
        );

        Ok((results, quota))
    }
}

#[async_trait]
impl ScraperProvider for ScreenScraperClient {
    fn provider_name(&self) -> &'static str {
        "screenscraper"
    }

    async fn search(&self, params: &ScraperSearchParams) -> Result<Vec<ScraperSearchResult>> {
        let (results, _) = self.search_with_quota(params).await?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screenscraper_xml_parsing() {
        let sample_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <ssuser>
    <requeststoday>15</requeststoday>
    <maxrequestsperday>3000</maxrequestsperday>
  </ssuser>
  <jeux>
    <jeu id="12345">
      <noms>
        <nom region="us">Super Mario World</nom>
      </noms>
      <developpeur>Nintendo</developpeur>
      <editeur>Nintendo</editeur>
      <synopsis>
        <synopsis langue="en">A classic platforming adventure on Dinosaur Land.</synopsis>
      </synopsis>
      <dates>
        <date region="us">1990-11-21</date>
      </dates>
      <note>19</note>
      <medias>
        <media type="box-2d">https://example.com/cover.png</media>
        <media type="banner">https://example.com/banner.png</media>
      </medias>
    </jeu>
  </jeux>
</Data>"#;

        let client = ScreenScraperClient::new(None, None);
        let (results, quota) = client.parse_xml_response(sample_xml).unwrap();

        assert_eq!(quota.requests_today, 15);
        assert_eq!(quota.max_requests_per_day, 3000);
        assert_eq!(quota.remaining_today, 2985);

        assert_eq!(results.len(), 1);
        let game = &results[0];
        assert_eq!(game.game_id, "12345");
        assert_eq!(game.title, "Super Mario World");
        assert_eq!(game.developer.as_deref(), Some("Nintendo"));
        assert_eq!(game.publisher.as_deref(), Some("Nintendo"));
        assert_eq!(game.release_year, Some(1990));
        assert_eq!(game.rating, Some(4.8));
        assert_eq!(
            game.cover_url.as_deref(),
            Some("https://example.com/cover.png")
        );
        assert_eq!(
            game.banner_url.as_deref(),
            Some("https://example.com/banner.png")
        );
    }

    #[test]
    fn test_screenscraper_zzz_notgame_filtering() {
        let sample_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <jeux>
    <jeu id="999">
      <noms>
        <nom region="us">ZZZ(notgame): Setup Utility</nom>
      </noms>
    </jeu>
  </jeux>
</Data>"#;

        let client = ScreenScraperClient::new(None, None);
        let (results, _) = client.parse_xml_response(sample_xml).unwrap();
        assert_eq!(results.len(), 0);
    }
}
