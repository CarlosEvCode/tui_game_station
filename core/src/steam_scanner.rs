use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::db::Database;
use crate::models::Game;

const STEAM_DIRS: &[&str] = &[
    ".local/share/Steam/steamapps",
    ".steam/steam/steamapps",
    ".steam/debian-installation/steamapps",
    ".steam/root/steamapps",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps",
];

const EXCLUDED_APPIDS: &[i64] = &[
    221410,  // Steam for Linux
    228980,  // Steamworks Common Redistributables
    1070560, // Steam Linux Runtime
    1391110, // Steam Linux Runtime - Soldier
    1628350, // Steam Linux Runtime - Sniper
    1493710, // Proton Experimental
];

pub struct SteamScanner;

impl SteamScanner {
    /// Scan local system for installed Steam games via appmanifest_*.acf files
    pub fn scan_steam_games(db: &Database) -> Result<usize> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
        let mut steamapps_paths = Vec::new();

        // 1. Find root steamapps directories
        for rel in STEAM_DIRS {
            let path = home.join(rel);
            if path.exists() && path.is_dir() {
                steamapps_paths.push(path.clone());
                // Parse libraryfolders.vdf for additional library paths
                let lib_vdf = path.join("libraryfolders.vdf");
                if lib_vdf.exists() {
                    if let Ok(extra_paths) = parse_libraryfolders_vdf(&lib_vdf) {
                        for p in extra_paths {
                            let extra_steamapps = p.join("steamapps");
                            if extra_steamapps.exists()
                                && !steamapps_paths.contains(&extra_steamapps)
                            {
                                steamapps_paths.push(extra_steamapps);
                            }
                        }
                    }
                }
            }
        }

        if steamapps_paths.is_empty() {
            return Ok(0);
        }

        // Get steam platform ID from DB
        let platforms = db.get_platforms()?;
        let steam_platform = match platforms.iter().find(|p| p.slug == "steam") {
            Some(p) => p,
            None => anyhow::bail!("Steam platform not found in database"),
        };

        let mut count = 0;

        // 2. Parse appmanifest_*.acf files
        for steamapps in steamapps_paths {
            let entries = match fs::read_dir(&steamapps) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.filter_map(|e| e.ok()) {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.starts_with("appmanifest_") && fname.ends_with(".acf") {
                    let acf_path = entry.path();
                    if let Ok((appid, name, size)) = parse_acf_file(&acf_path) {
                        if EXCLUDED_APPIDS.contains(&appid) || is_steam_tool(&name) {
                            continue;
                        }

                        let game = Game {
                            id: 0,
                            platform_id: steam_platform.id,
                            title: name,
                            sort_title: None,
                            game_type: "steam".to_string(),
                            file_path: Some(format!("steam://rungameid/{}", appid)),
                            working_dir: None,
                            custom_command: None,
                            env_vars: None,
                            wine_prefix: None,
                            wine_runner_id: None,
                            steam_appid: Some(appid),
                            file_name: None,
                            file_extension: None,
                            file_size: Some(size),
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

                        if db.insert_game(&game).is_ok() {
                            count += 1;
                        }
                    }
                }
            }
        }

        Ok(count)
    }
}

fn is_steam_tool(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("steam linux runtime")
        || lower.contains("proton")
        || lower.contains("steamworks")
        || lower.contains("steamvr")
        || lower.contains("steam controller")
        || lower.contains("dedicated server")
        || lower.contains("anti-cheat")
        || lower.contains("redistributable")
        || (lower.starts_with("steam ") && (lower.contains("runtime") || lower.contains("sdk")))
}

/// Simple Key-Value VDF parser for appmanifest_*.acf files
fn parse_acf_file(path: &Path) -> Result<(i64, String, i64)> {
    let content = fs::read_to_string(path)?;
    let mut appid = 0i64;
    let mut name = String::new();
    let mut size = 0i64;

    for line in content.lines() {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed
            .split('"')
            .filter(|s| !s.trim().is_empty())
            .collect();
        if parts.len() >= 2 {
            let key = parts[0].to_lowercase();
            let val = parts[1];

            if key == "appid" {
                appid = val.parse().unwrap_or(0);
            } else if key == "name" {
                name = val.to_string();
            } else if key == "sizeondisk" {
                size = val.parse().unwrap_or(0);
            }
        }
    }

    if appid > 0 && !name.is_empty() {
        Ok((appid, name, size))
    } else {
        anyhow::bail!("Invalid ACF manifest: {:?}", path)
    }
}

/// Simple parser for libraryfolders.vdf to find additional Steam library paths
fn parse_libraryfolders_vdf(path: &Path) -> Result<Vec<PathBuf>> {
    let content = fs::read_to_string(path)?;
    let mut paths = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed
            .split('"')
            .filter(|s| !s.trim().is_empty())
            .collect();
        if parts.len() >= 2 && parts[0].to_lowercase() == "path" {
            let p = PathBuf::from(parts[1]);
            if p.exists() {
                paths.push(p);
            }
        }
    }

    Ok(paths)
}
