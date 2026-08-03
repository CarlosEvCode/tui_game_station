use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlatformType {
    Emulator,
    Native,
    Wine,
    Steam,
}

impl std::fmt::Display for PlatformType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PlatformType::Emulator => "emulator",
            PlatformType::Native => "native",
            PlatformType::Wine => "wine",
            PlatformType::Steam => "steam",
        };
        f.write_str(s)
    }
}

impl From<&str> for PlatformType {
    fn from(s: &str) -> Self {
        match s {
            "native" => PlatformType::Native,
            "wine" => PlatformType::Wine,
            "steam" => PlatformType::Steam,
            _ => PlatformType::Emulator,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Platform {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub platform_type: PlatformType,
    pub default_extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Runner {
    pub id: i64,
    pub platform_id: Option<i64>,
    pub name: String,
    pub runner_type: String,
    pub executable_path: Option<String>,
    pub command_template: String,
    pub default_env: Option<String>,
    pub download_url: Option<String>,
    pub download_filename: Option<String>,
    pub is_default: bool,
    /// JSON payload for emulator options (`emulator_options` map + `custom_args`).
    pub env_vars: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniqueRunnerInfo {
    pub name: String,
    pub console_initials: String,
    pub executable_path: Option<String>,
    pub download_url: Option<String>,
    pub download_filename: Option<String>,
    pub runner_type: String,
    pub is_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFolder {
    pub id: i64,
    pub platform_id: i64,
    pub path: String,
    pub recursive: bool,
    pub last_scanned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: i64,
    pub platform_id: i64,
    pub title: String,
    pub sort_title: Option<String>,
    pub game_type: String,
    
    pub file_path: Option<String>,
    pub working_dir: Option<String>,
    pub custom_command: Option<String>,
    pub env_vars: Option<String>,
    
    pub wine_prefix: Option<String>,
    pub wine_runner_id: Option<i64>,
    pub steam_appid: Option<i64>,

    pub file_name: Option<String>,
    pub file_extension: Option<String>,
    pub file_size: Option<i64>,
    pub file_hash_crc32: Option<String>,
    pub file_hash_md5: Option<String>,
    pub file_hash_sha1: Option<String>,
    pub serial: Option<String>,

    pub release_year: Option<i32>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub rating: Option<f64>,
    pub favorite: bool,
    pub play_count: i64,
    pub play_time_seconds: i64,
    pub last_played_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,

    /// Associated Switch files of the same game (updates, DLCs, discarded
    /// duplicates, archives). Only populated for Switch games; empty otherwise.
    #[serde(default)]
    pub components: Vec<GameComponent>,

    /// True when a Switch group had Update/DLC files but no Base file. The UI
    /// can later warn "missing base game" instead of launching it as playable.
    #[serde(default)]
    pub is_missing_base: bool,
}

/// One additional file belonging to a Switch game entry (update, DLC, or a
/// discarded duplicate). Kept out of the `games` table so updates/DLCs don't
/// appear as separate library entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameComponent {
    pub id: i64,
    pub game_id: i64,
    /// "base" | "update" | "dlc"
    pub category: String,
    pub file_path: String,
    pub file_name: Option<String>,
    pub file_extension: Option<String>,
    pub file_size: Option<i64>,
    /// False for archives (.zip/.rar/.7z) that need extracting before play.
    pub is_launchable: bool,
    pub title_id: Option<String>,
    /// Numeric update version from the "[v393216]" tag, if present.
    pub version: Option<i64>,
    /// True for duplicates that lost disambiguation (kept for reference/debug).
    pub discarded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMedia {
    pub id: i64,
    pub game_id: i64,
    pub media_type: String,
    pub file_path: String,
    pub source: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDatEntry {
    pub id: i64,
    pub platform_slug: String,
    pub name: String,
    pub crc32: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub serial: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub release_year: Option<i32>,
}
