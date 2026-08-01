use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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
        use_dat_auto_id: bool,
    ) -> Result<usize> {
        let folder = folder_path.as_ref();
        if !folder.exists() || !folder.is_dir() {
            anyhow::bail!(
                "Scan folder does not exist or is not a directory: {:?}",
                folder
            );
        }

        let dat_parser = if use_dat_auto_id {
            let dat_path = crate::dat_downloader::DatDownloader::get_local_dat_path(&platform.slug);
            if dat_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&dat_path) {
                    Some(crate::dat_parser::DatParser::parse(&content))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut count = 0;
        let mut walker = WalkDir::new(folder);
        if !recursive {
            walker = walker.max_depth(1);
        }

        let paths: Vec<PathBuf> = walker
            .into_iter()
            .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
            .filter(|path| path.is_file() && has_supported_extension(path, platform))
            .collect();
        let paths = if platform.slug == "ps1" {
            select_ps1_images(paths)
        } else {
            paths
        };

        for path in paths {
            let ext = extension_for(&path);

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

            let (crc32, md5, sha1, size) = if calculate_hashes {
                if let Ok(hashes) = HashCalculator::calculate_hashes(&path) {
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
                let size = std::fs::metadata(&path).map(|m| m.len() as i64).ok();
                (None, None, None, size)
            };

            let extracted_serial = crate::serial_extractor::SerialExtractor::extract_serial(&path, &platform.slug);

            let dat_title = if let Some(ref parser) = dat_parser {
                if let Some(ref s) = extracted_serial {
                    parser.resolve_by_serial(s).cloned()
                } else if let Some(ref m) = md5 {
                    parser.resolve_by_hash(m).cloned()
                } else if let Some(ref c) = crc32 {
                    parser.resolve_by_hash(c).cloned()
                } else {
                    None
                }
            } else {
                None
            };

            let title = if let Some(dt) = dat_title {
                dt
            } else if platform.slug == "wii_u" && ext == ".rpx" {
                match wii_u_directory_title(&path) {
                    Some(Some(title)) => title,
                    Some(None) => continue,
                    None => clean_game_title(&stem),
                }
            } else {
                clean_game_title(&stem)
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
                serial: extracted_serial,
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

fn has_supported_extension(path: &Path, platform: &Platform) -> bool {
    let ext = extension_for(path);
    platform.default_extensions.is_empty()
        || platform
            .default_extensions
            .iter()
            .any(|supported| supported.to_lowercase() == ext)
}

fn extension_for(path: &Path) -> String {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|extension| format!(".{}", extension.to_lowercase()))
        .unwrap_or_default()
}

/// Select one launchable image per PS1 game without importing the data tracks
/// referenced by a CUE sheet.  CUE is safest for multi-track discs; CHD and PBP
/// are self-contained fallbacks, and raw BIN is used only when no better option
/// is present.
fn select_ps1_images(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let cue_references: HashSet<PathBuf> = paths
        .iter()
        .filter(|path| extension_for(path) == ".cue")
        .flat_map(|cue_path| cue_referenced_files(cue_path))
        .collect();

    let mut best_by_name: HashMap<String, PathBuf> = HashMap::new();
    for path in paths {
        if extension_for(&path) == ".bin" && cue_references.contains(&path_identity(&path)) {
            continue;
        }

        let name = path
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let replace_existing = best_by_name
            .get(&name)
            .map(|existing| ps1_priority(&path) < ps1_priority(existing))
            .unwrap_or(true);
        if replace_existing {
            best_by_name.insert(name, path);
        }
    }

    let mut selected: Vec<PathBuf> = best_by_name.into_values().collect();
    selected.sort();
    selected
}

fn ps1_priority(path: &Path) -> u8 {
    match extension_for(path).as_str() {
        ".cue" => 0,
        ".chd" => 1,
        ".pbp" => 2,
        ".bin" => 3,
        _ => u8::MAX,
    }
}

fn cue_referenced_files(cue_path: &Path) -> Vec<PathBuf> {
    let parent = match cue_path.parent() {
        Some(parent) => parent,
        None => return Vec::new(),
    };
    let contents = match std::fs::read_to_string(cue_path) {
        Ok(contents) => contents,
        Err(_) => return Vec::new(),
    };

    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.to_ascii_uppercase().starts_with("FILE ") {
                return None;
            }
            let remainder = line[5..].trim_start();
            let file_name = if let Some(rest) = remainder.strip_prefix('"') {
                rest.split('"').next()?
            } else {
                remainder.split_whitespace().next()?
            };
            Some(path_identity(&parent.join(file_name)))
        })
        .collect()
}

fn path_identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
    use super::{clean_game_title, select_ps1_images, wii_u_directory_title, xml_tag_value};
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

    #[test]
    fn ps1_cue_keeps_the_descriptor_and_ignores_its_bin_tracks() {
        let root = std::env::temp_dir().join(format!(
            "tui_game_station_ps1_scanner_test_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let cue = root.join("Game.cue");
        let track_one = root.join("Game (Track 1).bin");
        let track_two = root.join("Game (Track 2).bin");
        let chd = root.join("Game.chd");
        fs::write(&track_one, []).unwrap();
        fs::write(&track_two, []).unwrap();
        fs::write(&chd, []).unwrap();
        fs::write(
            &cue,
            "FILE \"Game (Track 1).bin\" BINARY\nFILE \"Game (Track 2).bin\" BINARY\n",
        )
        .unwrap();

        assert_eq!(
            select_ps1_images(vec![track_one, track_two, chd, cue.clone()]),
            vec![cue]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
