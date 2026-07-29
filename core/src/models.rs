use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlatformType {
    Emulator,
    Native,
    Wine,
    Steam,
}

impl ToString for PlatformType {
    fn to_string(&self) -> String {
        match self {
            PlatformType::Emulator => "emulator".to_string(),
            PlatformType::Native => "native".to_string(),
            PlatformType::Wine => "wine".to_string(),
            PlatformType::Steam => "steam".to_string(),
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub platform_type: PlatformType,
    pub default_extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    pub id: i64,
    pub platform_id: Option<i64>,
    pub name: String,
    pub runner_type: String, // 'libretro', 'standalone_emulator', 'wine', 'proton', 'native', 'steam'
    pub executable_path: Option<String>,
    pub command_template: String,
    pub default_env: Option<String>,
    pub is_default: bool,
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
    pub game_type: String, // 'emulator', 'native', 'wine', 'steam'
    
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMedia {
    pub id: i64,
    pub game_id: i64,
    pub media_type: String, // 'cover', 'grid', 'hero', 'logo', 'banner'
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
