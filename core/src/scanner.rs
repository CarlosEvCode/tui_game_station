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
            anyhow::bail!(
                "Scan folder does not exist or is not a directory: {:?}",
                folder
            );
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
                && !platform
                    .default_extensions
                    .iter()
                    .any(|e| e.to_lowercase() == ext)
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

            // Cemu's unpacked format is a game directory containing a `code/`
            // directory and `meta/meta.xml`.  Treat its RPX as one game,
            // use its metadata title, and avoid importing updates/DLC as games.
            let title = if platform.slug == "wii_u" && ext == ".rpx" {
                match wii_u_directory_title(path) {
                    Some(Some(title)) => title,
                    Some(None) => continue,
                    None => clean_game_title(&stem),
                }
            } else {
                clean_game_title(&stem)
            };

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

/// Returns `Some(Some(title))` for an unpacked Wii U base game,
/// `Some(None)` for an update/DLC directory, and `None` for a regular RPX.
fn wii_u_directory_title(rpx_path: &Path) -> Option<Option<String>> {
    // The old application accepted any RPX inside code/, not only app.rpx.
    // Some dumped games use a title-specific executable name.
    if rpx_path.parent()?.file_name()?.to_str()? != "code" {
        return None;
    }

    let code_dir = rpx_path.parent()?;
    let game_dir = code_dir.parent()?;
    let app_xml = code_dir.join("app.xml");

    if let Ok(contents) = std::fs::read_to_string(&app_xml) {
        if let Some(title_id) = xml_tag_value(&contents, "title_id") {
            let title_id = title_id.to_ascii_lowercase();
            // 0005000e = update, 0005000c = DLC.
            if title_id.starts_with("0005000e") || title_id.starts_with("0005000c") {
                return Some(None);
            }
        }
    }

    let meta_xml = game_dir.join("meta").join("meta.xml");
    if let Ok(contents) = std::fs::read_to_string(meta_xml) {
        for tag in ["longname_es", "longname_en", "shortname_es", "shortname_en"] {
            if let Some(title) = xml_tag_value(&contents, tag).filter(|value| !value.is_empty()) {
                return Some(Some(title));
            }
        }
    }

    let folder_name = game_dir.file_name()?.to_str()?.trim();
    if matches!(folder_name, "code" | "WiiU" | "Wii U") {
        return None;
    }
    Some(Some(clean_game_title(folder_name)))
}

fn xml_tag_value(contents: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let start = contents.find(&open)?;
    let after_open = &contents[start..];
    let value_start = after_open.find('>')? + 1;
    let after_value_start = &after_open[value_start..];
    let close = format!("</{}>", tag);
    let value_end = after_value_start.find(&close)?;
    let value = after_value_start[..value_end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(value.replace("&amp;", "&").trim().to_string())
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

#[cfg(test)]
mod tests {
    use super::{clean_game_title, wii_u_directory_title, xml_tag_value};
    use std::fs;

    #[test]
    fn extracts_xml_values_with_attributes_and_whitespace() {
        assert_eq!(
            xml_tag_value(
                "<longname_en type=\"string\"> Mario &amp; Luigi </longname_en>",
                "longname_en"
            ),
            Some("Mario & Luigi".to_string())
        );
    }

    #[test]
    fn cleans_release_tags() {
        assert_eq!(clean_game_title("Game Name (USA) [v1.0]"), "Game Name");
    }

    #[test]
    fn identifies_unpacked_wii_u_base_games_and_skips_updates() {
        let root = std::env::temp_dir().join(format!(
            "tui_game_station_scanner_test_{}",
            std::process::id()
        ));
        let game_code = root.join("Base Game").join("code");
        fs::create_dir_all(root.join("Base Game").join("meta")).unwrap();
        fs::create_dir_all(&game_code).unwrap();
        fs::write(game_code.join("title_specific_name.rpx"), []).unwrap();
        fs::write(
            game_code.join("app.xml"),
            "<title_id>0005000012345678</title_id>",
        )
        .unwrap();
        fs::write(
            root.join("Base Game").join("meta").join("meta.xml"),
            "<longname_es>Juego de prueba</longname_es>",
        )
        .unwrap();

        assert_eq!(
            wii_u_directory_title(&game_code.join("title_specific_name.rpx")),
            Some(Some("Juego de prueba".to_string()))
        );

        fs::write(
            game_code.join("app.xml"),
            "<title_id>0005000E12345678</title_id>",
        )
        .unwrap();
        assert_eq!(
            wii_u_directory_title(&game_code.join("title_specific_name.rpx")),
            Some(None)
        );

        fs::remove_dir_all(root).unwrap();
    }
}
