//! Nintendo Switch Title ID parsing and grouping helpers.
//!
//! Switch filenames carry a 16-hex-digit Title ID in brackets, e.g.
//! `Super Mario Odyssey [0100000000010000][v0].xci`. The Title ID alone is
//! enough to know whether a file is the base game, an update or a DLC, so the
//! scanner can group every file of the same game (across subfolders) into a
//! single library entry without any external database.
//!
//! Business rule: the first 13 digits identify the game "family"; the last 3
//! digits the type: `000` = base, `800` = update, anything else = DLC.

use std::sync::OnceLock;

use regex::Regex;

/// Category of a Switch file within a game family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchCategory {
    Base,
    Update,
    Dlc,
}

impl SwitchCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwitchCategory::Base => "base",
            SwitchCategory::Update => "update",
            SwitchCategory::Dlc => "dlc",
        }
    }
}

/// Parsed info derived from a file's Title ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleInfo {
    /// The full 16-hex Title ID (uppercased).
    pub title_id: String,
    /// Family key: first 13 hex digits + "000". Used to group all files of one game.
    pub base_id: String,
    /// Last 3 hex digits.
    pub type_code: String,
    pub category: SwitchCategory,
}

fn title_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([0-9A-Fa-f]{16})\]").expect("valid title id regex"))
}

fn version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[v(\d+)\]").expect("valid version regex"))
}

/// Extract the Title ID from a file/folder name.
///
/// Matches any bracketed group of exactly 16 hex digits. If several matches
/// exist (rare, e.g. a version tag that happens to look like hex), the FIRST
/// one wins: scene convention puts the Title ID right after the game name and
/// before the `[vXXXXXX]` version tag, and version tags are decimal so they
/// never match the 16-hex pattern anyway.
pub fn extract_title_id(file_name: &str) -> Option<String> {
    title_id_re()
        .captures(file_name)
        .map(|caps| caps[1].to_ascii_uppercase())
}

/// Classify a Title ID into its category + base_id per the business rule above.
pub fn classify_title_id(title_id: &str) -> TitleInfo {
    let title_id = title_id.to_ascii_uppercase();
    let family_len = title_id.len().saturating_sub(3);
    let type_code = title_id[family_len..].to_string();
    let base_id = format!("{}000", &title_id[..family_len]);
    let category = match type_code.as_str() {
        "000" => SwitchCategory::Base,
        "800" => SwitchCategory::Update,
        _ => SwitchCategory::Dlc,
    };
    TitleInfo {
        title_id,
        base_id,
        type_code,
        category,
    }
}

/// Extract + classify in one step. `None` when the name has no Title ID.
pub fn extract_title_info(file_name: &str) -> Option<TitleInfo> {
    let title_id = extract_title_id(file_name)?;
    Some(classify_title_id(&title_id))
}

/// Parse the numeric `[v393216]` update version tag (decimal). `None` if absent.
pub fn parse_version_tag(file_name: &str) -> Option<u32> {
    version_re()
        .captures(file_name)
        .and_then(|caps| caps[1].parse::<u32>().ok())
}

/// True for archive extensions that are not directly launchable until extracted.
pub fn is_archive_ext(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), ".zip" | ".rar" | ".7z")
}

/// Lower value = preferred format: .xci > .nsp > archive > anything else.
pub fn format_priority(ext: &str) -> u8 {
    match ext.to_ascii_lowercase().as_str() {
        ".xci" => 0,
        ".nsp" => 1,
        _ if is_archive_ext(ext) => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title_id_from_typical_names() {
        assert_eq!(
            extract_title_id("Super Mario Odyssey [0100000000010000][v0].xci"),
            Some("0100000000010000".to_string())
        );
        // lowercase ids are accepted and normalized to uppercase
        assert_eq!(
            extract_title_id("zelda [0100abcd00ef0000][v0].nsp"),
            Some("0100ABCD00EF0000".to_string())
        );
        // version tags are decimal and never match
        assert_eq!(extract_title_id("Zelda BOTW [v196608].nsp"), None);
        assert_eq!(extract_title_id("Shovel Knight.nsp"), None);
    }

    #[test]
    fn takes_first_match_when_several_brackets() {
        assert_eq!(
            extract_title_id("Game (AAAA000000000000) [0100000000010000][v0].xci"),
            Some("0100000000010000".to_string())
        );
    }

    #[test]
    fn classifies_base_update_and_dlc() {
        let base = classify_title_id("0100000000010000");
        assert_eq!(base.category, SwitchCategory::Base);
        assert_eq!(base.base_id, "0100000000010000");

        let update = classify_title_id("0100000000010800");
        assert_eq!(update.category, SwitchCategory::Update);
        assert_eq!(update.base_id, "0100000000010000");
        assert_eq!(update.type_code, "800");

        let dlc = classify_title_id("0100000000010001");
        assert_eq!(dlc.category, SwitchCategory::Dlc);
        assert_eq!(dlc.base_id, "0100000000010000");
    }

    #[test]
    fn parses_decimal_version_tag() {
        assert_eq!(
            parse_version_tag("Game [v196608][0100000000010000].nsp"),
            Some(196608)
        );
        assert_eq!(parse_version_tag("Game [v0].xci"), Some(0));
        assert_eq!(parse_version_tag("Game.xci"), None);
    }

    #[test]
    fn format_priority_prefers_xci_over_nsp_over_archives() {
        assert!(format_priority(".xci") < format_priority(".nsp"));
        assert!(format_priority(".nsp") < format_priority(".zip"));
        assert!(format_priority(".zip") < format_priority(".nca"));
    }
}
