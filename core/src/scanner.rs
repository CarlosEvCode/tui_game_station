use anyhow::Result;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::db::Database;
use crate::hash::HashCalculator;
use crate::models::{Game, GameComponent, Platform};
use crate::switch::SwitchCategory;

#[derive(Debug, Clone)]
pub struct ScanProgressEvent {
    pub current: usize,
    pub total: usize,
    pub current_title: String,
    pub finished: bool,
    pub added_count: usize,
    pub error_msg: Option<String>,
}

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
        progress_tx: Option<&std::sync::mpsc::Sender<ScanProgressEvent>>,
    ) -> Result<usize> {
        let folder = folder_path.as_ref();
        if !folder.exists() || !folder.is_dir() {
            anyhow::bail!(
                "Scan folder does not exist or is not a directory: {:?}",
                folder
            );
        }

        let dat_parser = if use_dat_auto_id
            && crate::dat_downloader::DatDownloader::supports_dat_identification(&platform.slug)
        {
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
        let paths = match platform.slug.as_str() {
            "ps1" => select_ps1_images(paths),
            "wii_u" => select_wii_u_images(paths),
            _ => paths,
        };

        // Nintendo Switch is grouped by Title ID (base + updates + DLCs become
        // one library entry), so it uses its own scan path.
        if platform.slug == "switch" {
            return Self::scan_switch_folder(
                db,
                platform,
                folder,
                recursive,
                calculate_hashes,
                progress_tx,
            );
        }

        let total_paths = paths.len();

        for (idx, path) in paths.into_iter().enumerate() {
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

            let extracted_serial =
                crate::serial_extractor::SerialExtractor::extract_serial(&path, &platform.slug);

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

            if let Some(tx) = progress_tx {
                let _ = tx.send(ScanProgressEvent {
                    current: idx + 1,
                    total: total_paths,
                    current_title: title.clone(),
                    finished: false,
                    added_count: count,
                    error_msg: None,
                });
            }

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
                components: Vec::new(),
                is_missing_base: false,
            };

            if db.insert_game(&game).is_ok() {
                count += 1;
            }
        }

        if let Some(tx) = progress_tx {
            let _ = tx.send(ScanProgressEvent {
                current: total_paths,
                total: total_paths,
                current_title: "Scan Completed".to_string(),
                finished: true,
                added_count: count,
                error_msg: None,
            });
        }

        Ok(count)
    }

    /// Switch-specific scan: group every file sharing a Title ID family into a
    /// single library entry. The Base file of the group is the reference
    /// (name + launch), while Updates/DLCs (and discarded duplicates) are
    /// stored as associated components — not as separate games.
    fn scan_switch_folder(
        db: &Database,
        platform: &Platform,
        folder: &Path,
        recursive: bool,
        calculate_hashes: bool,
        progress_tx: Option<&std::sync::mpsc::Sender<ScanProgressEvent>>,
    ) -> Result<usize> {
        let mut walker = WalkDir::new(folder);
        if !recursive {
            walker = walker.max_depth(1);
        }

        // Only extensions configured for the platform (nsp/xci/nca/nso) reach
        // the Title ID pipeline. Anything else (archives, random files) is
        // never a Switch entry or component, regardless of its name.
        let paths: Vec<PathBuf> = walker
            .into_iter()
            .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
            .filter(|path| path.is_file() && has_supported_extension(path, platform))
            .collect();

        let entries: Vec<SwitchEntry> = paths
            .iter()
            .map(|path| {
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
                let ext = extension_for(path);
                let info = crate::switch::extract_title_info(&file_name);
                let version = crate::switch::parse_version_tag(&file_name);
                SwitchEntry {
                    path: path.clone(),
                    file_name,
                    stem,
                    ext,
                    base_id: info.as_ref().map(|i| i.base_id.clone()),
                    type_code: info.as_ref().map(|i| i.type_code.clone()),
                    category: info.as_ref().map(|i| i.category),
                    version,
                    size: std::fs::metadata(path).map(|m| m.len() as i64).ok(),
                }
            })
            .collect();

        let total = entries.len();
        let mut count = 0;
        let mut groups: HashMap<String, Vec<SwitchEntry>> = HashMap::new();
        let mut fallback: Vec<SwitchEntry> = Vec::new();
        for entry in entries {
            match entry.base_id.clone() {
                Some(base_id) => groups.entry(base_id).or_default().push(entry),
                None => fallback.push(entry),
            }
        }

        // Files without a detectable Title ID keep the plain per-file flow.
        for entry in &fallback {
            let game = game_from_reference(
                platform,
                entry,
                clean_game_title(&entry.stem),
                false,
                Vec::new(),
                calculate_hashes,
            );
            let title = game.title.clone();
            if db.insert_game(&game).is_ok() {
                count += 1;
            }
            send_progress(progress_tx, count, total, &title, count, false);
        }

        // Grouped entries. Nintendo sometimes assigns DLC/AddOnContent a Title
        // ID in a neighbouring block (the 13th hex digit changes, e.g. base
        // 01007EF00011E000 vs DLC 01007EF00011F001), so a group may end up
        // without its Base: those orphans are re-associated by clean name.
        let mut primary: HashMap<String, Vec<SwitchEntry>> = HashMap::new();
        let mut orphans: Vec<Vec<SwitchEntry>> = Vec::new();
        for (base_id, group) in groups {
            if group
                .iter()
                .any(|e| e.category == Some(SwitchCategory::Base))
            {
                primary.insert(base_id, group);
            } else {
                orphans.push(group);
            }
        }

        // Canonical clean name per primary group, from its best Base file.
        let mut primary_names: Vec<(String, String)> = Vec::new();
        let mut base_ids: Vec<String> = primary.keys().cloned().collect();
        base_ids.sort();
        for base_id in base_ids {
            if let Some(base) = primary
                .get(&base_id)
                .and_then(|g| g.iter().find(|e| e.category == Some(SwitchCategory::Base)))
            {
                primary_names.push((clean_game_title(&base.stem), base_id.clone()));
            }
        }

        // Orphan groups (Update/DLC only) attach to the single primary group
        // whose clean name matches. Strict equality avoids false positives;
        // ambiguous (0 or several) matches stay as their own entry.
        let mut unmerged: Vec<Vec<SwitchEntry>> = Vec::new();
        for orphan in orphans {
            let name = orphan
                .iter()
                .find(|e| e.category == Some(SwitchCategory::Update))
                .or_else(|| {
                    orphan
                        .iter()
                        .find(|e| e.category == Some(SwitchCategory::Dlc))
                })
                .map(|e| clean_game_title(&e.stem))
                .unwrap_or_default();
            let matches: Vec<String> = primary_names
                .iter()
                .filter(|(n, _)| *n == name)
                .map(|(_, base_id)| base_id.clone())
                .collect();
            if matches.len() == 1 {
                if let Some(group) = primary.get_mut(&matches[0]) {
                    group.extend(orphan);
                    continue;
                }
            }
            unmerged.push(orphan);
        }

        // One library entry per remaining group; unmerged orphans become their
        // own entries flagged as missing their base. Deterministic order.
        let mut all: Vec<(String, Vec<SwitchEntry>)> = primary.into_iter().collect();
        for orphan in unmerged {
            let key = orphan
                .iter()
                .find_map(|e| e.base_id.clone())
                .unwrap_or_default();
            all.push((key, orphan));
        }
        all.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, group) in all {
            let title = group
                .iter()
                .find(|e| e.category == Some(SwitchCategory::Base))
                .or_else(|| {
                    group
                        .iter()
                        .find(|e| e.category == Some(SwitchCategory::Update))
                })
                .or_else(|| {
                    group
                        .iter()
                        .find(|e| e.category == Some(SwitchCategory::Dlc))
                })
                .map(|e| clean_game_title(&e.stem))
                .unwrap_or_default();
            let game = match build_switch_group_game(platform, group, calculate_hashes) {
                Some(game) => game,
                None => continue,
            };

            if db.insert_game(&game).is_ok() {
                count += 1;
                if let Some(file_path) = game.file_path.as_deref() {
                    if let Ok(Some(game_id)) = db.get_game_id_by_file_path(file_path) {
                        for comp in &game.components {
                            let _ = db.insert_game_component(game_id, comp);
                        }
                    }
                }
            }
            send_progress(progress_tx, count, total, &title, count, false);
        }

        send_progress(progress_tx, total, total, "Scan Completed", count, true);
        Ok(count)
    }
}

fn send_progress(
    tx: Option<&std::sync::mpsc::Sender<ScanProgressEvent>>,
    current: usize,
    total: usize,
    title: &str,
    count: usize,
    finished: bool,
) {
    if let Some(tx) = tx {
        let _ = tx.send(ScanProgressEvent {
            current,
            total,
            current_title: title.to_string(),
            finished,
            added_count: count,
            error_msg: None,
        });
    }
}

/// A file the Switch scanner looked at, with its parsed Title ID info.
struct SwitchEntry {
    path: PathBuf,
    file_name: String,
    stem: String,
    ext: String,
    base_id: Option<String>,
    type_code: Option<String>,
    category: Option<SwitchCategory>,
    version: Option<u32>,
    size: Option<i64>,
}

struct FileMeta {
    size: Option<i64>,
    crc32: Option<String>,
    md5: Option<String>,
    sha1: Option<String>,
}

fn file_meta(path: &Path, calculate_hashes: bool) -> FileMeta {
    if calculate_hashes {
        if let Ok(hashes) = HashCalculator::calculate_hashes(path) {
            return FileMeta {
                size: Some(hashes.file_size as i64),
                crc32: Some(hashes.crc32),
                md5: Some(hashes.md5),
                sha1: Some(hashes.sha1),
            };
        }
        return FileMeta {
            size: None,
            crc32: None,
            md5: None,
            sha1: None,
        };
    }
    let size = std::fs::metadata(path).map(|m| m.len() as i64).ok();
    FileMeta {
        size,
        crc32: None,
        md5: None,
        sha1: None,
    }
}

/// Turn one Switch group into a single game entry plus its components.
/// `None` can't happen for a real group (there is always >= 1 file), but is
/// kept as a safe return so callers don't have to unwrap.
fn build_switch_group_game(
    platform: &Platform,
    entries: Vec<SwitchEntry>,
    calculate_hashes: bool,
) -> Option<Game> {
    let mut bases: Vec<&SwitchEntry> = Vec::new();
    let mut updates: Vec<&SwitchEntry> = Vec::new();
    let mut dlcs: Vec<&SwitchEntry> = Vec::new();
    for entry in &entries {
        match entry.category {
            Some(SwitchCategory::Base) => bases.push(entry),
            Some(SwitchCategory::Update) => updates.push(entry),
            Some(SwitchCategory::Dlc) => dlcs.push(entry),
            None => {}
        }
    }

    let base_sel = resolve_category(bases, true);
    let update_sel = resolve_category(updates, false);
    let dlc_sel = resolve_category(dlcs, false);

    let best_base = base_sel
        .iter()
        .find(|(_, discarded)| !*discarded)
        .map(|(e, _)| *e);
    let best_update = update_sel
        .iter()
        .find(|(_, discarded)| !*discarded)
        .map(|(e, _)| *e);
    let best_dlc = dlc_sel
        .iter()
        .find(|(_, discarded)| !*discarded)
        .map(|(e, _)| *e);

    let reference = best_base.or(best_update).or(best_dlc)?;
    let missing_base = best_base.is_none();

    // Every non-reference file becomes a component (updates, DLCs, and any
    // duplicates that lost disambiguation — kept for reference, not deleted).
    let mut components = Vec::new();
    for (entry, discarded) in base_sel
        .iter()
        .chain(update_sel.iter())
        .chain(dlc_sel.iter())
    {
        if std::ptr::eq(*entry, reference) {
            continue;
        }
        components.push(GameComponent {
            id: 0,
            game_id: 0,
            category: entry
                .category
                .map(|c| c.as_str().to_string())
                .unwrap_or_else(|| "dlc".to_string()),
            file_path: entry.path.to_string_lossy().to_string(),
            file_name: Some(entry.file_name.clone()),
            file_extension: Some(entry.ext.clone()),
            file_size: entry.size,
            is_launchable: !crate::switch::is_archive_ext(&entry.ext),
            title_id: entry.title_id(),
            version: entry.version.map(|v| v as i64),
            discarded: *discarded,
        });
    }

    Some(game_from_reference(
        platform,
        reference,
        clean_game_title(&reference.stem),
        missing_base,
        components,
        calculate_hashes,
    ))
}

impl SwitchEntry {
    fn title_id(&self) -> Option<String> {
        // Rebuild the title id from base_id's family + type_code so we keep a
        // single source of truth for the grouping key.
        let family = self.base_id.as_deref()?.get(..13)?;
        let code = self.type_code.as_deref()?;
        Some(format!("{}{}", family, code))
    }
}

/// Choose the best file per type_code within one category. Returns `(entry,
/// discarded)`: the winner of each type_code group has `discarded = false`,
/// the losers keep `discarded = true` so nothing is dropped without a trace.
fn resolve_category<'a>(
    entries: Vec<&'a SwitchEntry>,
    is_base: bool,
) -> Vec<(&'a SwitchEntry, bool)> {
    let mut by_type: HashMap<&str, Vec<&'a SwitchEntry>> = HashMap::new();
    for entry in entries {
        by_type
            .entry(entry.type_code.as_deref().unwrap_or_default())
            .or_default()
            .push(entry);
    }

    let mut out = Vec::new();
    for (_, mut group) in by_type {
        group.sort_by(|a, b| compare_files(a, b, is_base));
        for (i, entry) in group.into_iter().enumerate() {
            out.push((entry, i != 0));
        }
    }
    out
}

/// Order two files of the same category: "better" sorts first.
/// - Base/DLC of the same type_code: format priority (.xci > .nsp > archive).
/// - Updates: highest version wins, then format priority.
fn compare_files(a: &SwitchEntry, b: &SwitchEntry, is_base: bool) -> Ordering {
    let format_cmp =
        crate::switch::format_priority(&a.ext).cmp(&crate::switch::format_priority(&b.ext));
    let version_cmp = b.version.cmp(&a.version);
    if is_base {
        format_cmp.then(version_cmp)
    } else {
        version_cmp.then(format_cmp)
    }
}

/// Build a `Game` row using `reference` as the main file. `title` is the
/// cleaned name; `components` and `missing_base` are Switch-specific.
fn game_from_reference(
    platform: &Platform,
    reference: &SwitchEntry,
    title: String,
    missing_base: bool,
    components: Vec<GameComponent>,
    calculate_hashes: bool,
) -> Game {
    let meta = file_meta(&reference.path, calculate_hashes);
    Game {
        id: 0,
        platform_id: platform.id,
        title,
        sort_title: None,
        game_type: platform.platform_type.to_string(),
        file_path: Some(reference.path.to_string_lossy().to_string()),
        working_dir: reference
            .path
            .parent()
            .map(|p| p.to_string_lossy().to_string()),
        custom_command: None,
        env_vars: None,
        wine_prefix: None,
        wine_runner_id: None,
        steam_appid: None,
        file_name: Some(reference.file_name.clone()),
        file_extension: Some(reference.ext.clone()),
        file_size: meta.size,
        file_hash_crc32: meta.crc32,
        file_hash_md5: meta.md5,
        file_hash_sha1: meta.sha1,
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
        components,
        is_missing_base: missing_base,
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

fn select_wii_u_images(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen_game_dirs = std::collections::HashSet::new();
    let mut selected = Vec::new();

    for path in paths {
        let ext = extension_for(&path);
        if ext == ".rpx" {
            if let Some(code_dir) = path.parent() {
                if code_dir.file_name().and_then(|n| n.to_str()) == Some("code") {
                    if let Some(game_dir) = code_dir.parent() {
                        let game_dir_buf = game_dir.to_path_buf();
                        if seen_game_dirs.contains(&game_dir_buf) {
                            continue;
                        }

                        let app_xml = code_dir.join("app.xml");
                        if let Ok(contents) = std::fs::read_to_string(&app_xml) {
                            if let Some(title_id) = xml_tag_value(&contents, "title_id") {
                                let title_id = title_id.to_ascii_lowercase();
                                if title_id.starts_with("0005000e")
                                    || title_id.starts_with("0005000c")
                                {
                                    continue;
                                }
                            }
                        }

                        seen_game_dirs.insert(game_dir_buf);
                        selected.push(path);
                        continue;
                    }
                }
            }
        }
        selected.push(path);
    }

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

    #[test]
    fn switch_scan_groups_files_by_title_id_into_single_entries() {
        use super::Scanner;
        use crate::db::Database;

        let root = std::env::temp_dir().join(format!(
            "tui_game_station_switch_scan_test_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        let files = [
            "Super Mario Odyssey [01006710031A0000][v0].xci",
            "Super Mario Odyssey Update [01006710031A0800][v196608].nsp",
            "The Legend of Zelda Breath of the Wild [01007EF00011E000][v0].xci",
            "The Legend of Zelda Breath of the Wild Update [01007EF00011E800][v655360].nsp",
            "The Legend of Zelda Breath of the Wild [DLC Pack 1 The Master Trials] [01007EF00011F001].nsp",
            "The Legend of Zelda Breath of the Wild [DLC Pack 2 The Champions' Ballad] [01007EF00011F002].nsp",
            "Super Mario 3D World [010049900F546000][v0].xci",
            "Shovel Knight [010057D002BBE000][v0].xci",
            "Super Mario Maker 2 [01009B90006DC000][v0].xci",
            "Super Mario Maker 2 [01009B90006DC000][v0].nsp",
            "Super Mario Maker 2 [01009B90006DC000][v0].part1.rar",
            "Super Mario Maker 2 [01009B90006DC000][v0].part2.rar",
        ];
        for f in &files {
            fs::write(root.join(f), vec![0u8; 16]).unwrap();
        }

        let db = Database::open(":memory:").unwrap();
        let platform = db
            .get_platforms()
            .unwrap()
            .into_iter()
            .find(|p| p.slug == "switch")
            .expect("switch platform seeded");

        let added = Scanner::scan_folder(&db, &platform, &root, true, false, false, None).unwrap();
        assert_eq!(added, 5, "one library entry per Title ID family");

        let games = db.get_games_for_platform(platform.id).unwrap();
        assert_eq!(games.len(), 5);

        let titles: Vec<&str> = games.iter().map(|g| g.title.as_str()).collect();
        for expected in [
            "Super Mario Odyssey",
            "The Legend of Zelda Breath of the Wild",
            "Super Mario 3D World",
            "Shovel Knight",
            "Super Mario Maker 2",
        ] {
            assert!(titles.contains(&expected), "missing {expected}");
        }

        let odyssey = games
            .iter()
            .find(|g| g.title == "Super Mario Odyssey")
            .unwrap();
        assert!(!odyssey.is_missing_base);
        assert!(odyssey.file_path.as_deref().unwrap().ends_with(".xci"));
        assert_eq!(odyssey.components.len(), 1);
        let update = &odyssey.components[0];
        assert_eq!(update.category, "update");
        assert_eq!(update.version, Some(196608));
        assert!(!update.discarded);
        assert!(update.is_launchable);

        let botw = games
            .iter()
            .find(|g| g.title == "The Legend of Zelda Breath of the Wild")
            .unwrap();
        assert!(!botw.is_missing_base);
        assert!(botw.file_path.as_deref().unwrap().ends_with(".xci"));
        assert_eq!(botw.components.len(), 3);
        assert!(botw.components.iter().any(|c| c.category == "update"));
        assert_eq!(
            botw.components
                .iter()
                .filter(|c| c.category == "dlc")
                .count(),
            2
        );
        for dlc in botw.components.iter().filter(|c| c.category == "dlc") {
            assert!(
                dlc.title_id.as_deref() == Some("01007EF00011F001")
                    || dlc.title_id.as_deref() == Some("01007EF00011F002"),
                "real BOTW DLC title ids belong to the F-family block"
            );
        }

        let mm2 = games
            .iter()
            .find(|g| g.title == "Super Mario Maker 2")
            .unwrap();
        assert!(!mm2.is_missing_base);
        assert!(mm2.file_path.as_deref().unwrap().ends_with(".xci"));
        assert_eq!(mm2.components.len(), 1, ".rar files are not scanned at all");
        let nsp = &mm2.components[0];
        assert!(nsp.discarded);
        assert!(nsp.is_launchable);

        for g in &games {
            if matches!(g.title.as_str(), "Super Mario 3D World" | "Shovel Knight") {
                assert!(g.components.is_empty(), "loose games have no components");
            }
            assert!(
                g.file_extension.as_deref() != Some(".rar"),
                "no entry may be an archive"
            );
            for comp in &g.components {
                assert_ne!(
                    comp.file_extension.as_deref(),
                    Some(".rar"),
                    "no component may be an archive"
                );
            }
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_scan_groups_real_zelda_botw_ids_into_one_entry() {
        use super::Scanner;
        use crate::db::Database;

        // Real Title IDs from a dump of Zelda BOTW: the DLCs live in a
        // neighbouring block (13th digit E vs F), so the 13-char prefix rule
        // alone would split them off as a second entry.
        let root = std::env::temp_dir().join(format!(
            "tui_game_station_zelda_real_ids_test_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        let files = [
            "The Legend of Zelda Breath of the Wild [01007EF00011E000][v0].xci",
            "The Legend of Zelda Breath of the Wild [Update] [01007EF00011E800][v655360].nsp",
            "The Legend of Zelda Breath of the Wild [DLC Pack 1 The Master Trials] [01007EF00011F001].nsp",
            "The Legend of Zelda Breath of the Wild [DLC Pack 2 The Champions' Ballad] [01007EF00011F002].nsp",
        ];
        for f in &files {
            fs::write(root.join(f), vec![0u8; 16]).unwrap();
        }

        let db = Database::open(":memory:").unwrap();
        let platform = db
            .get_platforms()
            .unwrap()
            .into_iter()
            .find(|p| p.slug == "switch")
            .expect("switch platform seeded");

        let added = Scanner::scan_folder(&db, &platform, &root, true, false, false, None).unwrap();
        assert_eq!(
            added, 1,
            "base + update + both DLCs collapse into one entry"
        );

        let games = db.get_games_for_platform(platform.id).unwrap();
        assert_eq!(games.len(), 1);
        let botw = &games[0];
        assert_eq!(botw.title, "The Legend of Zelda Breath of the Wild");
        assert!(!botw.is_missing_base);
        assert!(botw.file_path.as_deref().unwrap().ends_with(".xci"));
        assert_eq!(botw.components.len(), 3);

        let update = botw
            .components
            .iter()
            .find(|c| c.category == "update")
            .unwrap();
        assert_eq!(update.version, Some(655360));
        let dlcs: Vec<&str> = botw
            .components
            .iter()
            .filter(|c| c.category == "dlc")
            .map(|c| c.title_id.as_deref().unwrap())
            .collect();
        assert!(dlcs.contains(&"01007EF00011F001"));
        assert!(dlcs.contains(&"01007EF00011F002"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn switch_scan_ignores_extensions_not_configured_for_platform() {
        use super::Scanner;
        use crate::db::Database;

        let root = std::env::temp_dir().join(format!(
            "tui_game_station_switch_ignore_ext_test_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        let files = [
            "Super Mario Maker 2 [01009B90006DC000][v0].xci",
            "Super Mario Maker 2 [01009B90006DC000][v0].part1.rar",
            "Super Mario Maker 2 [01009B90006DC000][v0].part2.rar",
            "Fake Game [0100ABCDEF123000][v0].zip",
            "Random Readme.txt",
        ];
        for f in &files {
            fs::write(root.join(f), vec![0u8; 16]).unwrap();
        }

        let db = Database::open(":memory:").unwrap();
        let platform = db
            .get_platforms()
            .unwrap()
            .into_iter()
            .find(|p| p.slug == "switch")
            .expect("switch platform seeded");

        let added = Scanner::scan_folder(&db, &platform, &root, true, false, false, None).unwrap();
        assert_eq!(added, 1, "only the .xci is a valid Switch file");

        let games = db.get_games_for_platform(platform.id).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Super Mario Maker 2");
        assert!(games[0].components.is_empty(), "no component from archives");
        assert_eq!(games[0].file_extension.as_deref(), Some(".xci"));

        fs::remove_dir_all(root).unwrap();
    }
}
