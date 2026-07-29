use anyhow::Result;
use std::ffi::OsStr;
use std::path::Path;
use walkdir::WalkDir;

use crate::db::Database;
use crate::hash::HashCalculator;
use crate::models::{Game, Platform};

pub struct Scanner;

impl Scanner {
    /// Scan a folder for a given platform, calculating hashes and inserting games into the DB.
    pub fn scan_folder<P: AsRef<Path>>(
        db: &Database,
        platform: &Platform,
        folder_path: P,
        recursive: bool,
        calculate_hashes: bool,
    ) -> Result<usize> {
        let folder = folder_path.as_ref();
        if !folder.exists() || !folder.is_dir() {
            anyhow::bail!("Scan folder does not exist or is not a directory: {:?}", folder);
        }

        let mut count = 0;
        let mut walker = WalkDir::new(folder);
        if !recursive {
            walker = walker.max_depth(1);
        }

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(OsStr::to_str)
                .map(|s| format!(".{}", s.to_lowercase()))
                .unwrap_or_default();

            if !platform.default_extensions.is_empty()
                && !platform.default_extensions.iter().any(|e| e.to_lowercase() == ext)
            {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("Unknown")
                .to_string();

            let stem = path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or(&file_name)
                .to_string();

            let title = clean_game_title(&stem);

            let (crc32, md5, sha1, size) = if calculate_hashes {
                if let Ok(hashes) = HashCalculator::calculate_hashes(path) {
                    (
                        Some(hashes.crc32),
                        Some(hashes.md5),
                        Some(hashes.sha1),
                        Some(hashes.file_size as i64),
                    )
                } else {
                    (None, None, None, None)
                }
            } else {
                let size = std::fs::metadata(path).map(|m| m.len() as i64).ok();
                (None, None, None, size)
            };

            let game = Game {
                id: 0,
                platform_id: platform.id,
                title,
                sort_title: None,
                game_type: platform.platform_type.to_string(),
                file_path: Some(path.to_string_lossy().to_string()),
                working_dir: path.parent().map(|p| p.to_string_lossy().to_string()),
                custom_command: None,
                env_vars: None,
                wine_prefix: None,
                wine_runner_id: None,
                steam_appid: None,
                file_name: Some(file_name),
                file_extension: Some(ext),
                file_size: size,
                file_hash_crc32: crc32,
                file_hash_md5: md5,
                file_hash_sha1: sha1,
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
            };

            if db.insert_game(&game).is_ok() {
                count += 1;
            }
        }

        Ok(count)
    }
}

/// Helper function to clean common scene/ROM release tags from filenames (e.g. "(USA)", "[!]").
fn clean_game_title(raw: &str) -> String {
    let mut title = raw.to_string();
    
    // Remove content inside brackets/parentheses for cleaner presentation while preserving title
    if let Some(idx) = title.find('(') {
        title = title[..idx].trim().to_string();
    }
    if let Some(idx) = title.find('[') {
        title = title[..idx].trim().to_string();
    }

    if title.is_empty() {
        raw.to_string()
    } else {
        title
    }
}
