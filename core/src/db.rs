use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::models::{Game, GameComponent, Platform, PlatformType, Runner};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let db_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station");

        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("Failed to create DB directory: {:?}", db_dir))?;

        let db_path = db_dir.join("game_station.db");
        Self::open(db_path)
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .with_context(|| format!("Failed to open SQLite database at {:?}", path.as_ref()))?;

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;",
        )?;

        let db = Database { conn };
        db.init_schema()?;
        db.seed_defaults()?;

        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS platforms (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                platform_type TEXT NOT NULL DEFAULT 'emulator',
                extensions TEXT NOT NULL DEFAULT '',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS runners (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform_id INTEGER REFERENCES platforms(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                runner_type TEXT NOT NULL,
                executable_path TEXT,
                command_template TEXT NOT NULL,
                default_env TEXT,
                download_url TEXT,
                download_filename TEXT,
                is_configured BOOLEAN DEFAULT 0,
                is_default BOOLEAN DEFAULT 0,
                env_vars TEXT
            );

            CREATE TABLE IF NOT EXISTS scan_folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform_id INTEGER NOT NULL REFERENCES platforms(id) ON DELETE CASCADE,
                path TEXT NOT NULL UNIQUE,
                recursive BOOLEAN DEFAULT 1,
                last_scanned_at DATETIME
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS games (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform_id INTEGER NOT NULL REFERENCES platforms(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                sort_title TEXT,
                game_type TEXT NOT NULL DEFAULT 'emulator',
                file_path TEXT UNIQUE,
                working_dir TEXT,
                custom_command TEXT,
                env_vars TEXT,
                wine_prefix TEXT,
                wine_runner_id INTEGER REFERENCES runners(id),
                steam_appid INTEGER,
                file_name TEXT,
                file_extension TEXT,
                file_size INTEGER,
                file_hash_crc32 TEXT,
                file_hash_md5 TEXT,
                file_hash_sha1 TEXT,
                serial TEXT,
                release_year INTEGER,
                developer TEXT,
                publisher TEXT,
                description TEXT,
                genre TEXT,
                rating REAL,
                favorite BOOLEAN DEFAULT 0,
                play_count INTEGER DEFAULT 0,
                play_time_seconds INTEGER DEFAULT 0,
                last_played_at DATETIME,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                is_missing_base BOOLEAN DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS game_components (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                category TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_name TEXT,
                file_extension TEXT,
                file_size INTEGER,
                is_launchable BOOLEAN DEFAULT 1,
                title_id TEXT,
                version INTEGER,
                discarded BOOLEAN DEFAULT 0,
                UNIQUE(game_id, file_path)
            );

            CREATE TABLE IF NOT EXISTS game_media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                media_type TEXT NOT NULL,
                file_path TEXT,
                source TEXT NOT NULL DEFAULT 'steamgriddb',
                url TEXT,
                status TEXT NOT NULL DEFAULT 'downloaded',
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(game_id, media_type)
            );
            ",
        )?;

        // Migrate pre-existing databases that lack the is_missing_base column.
        let has_missing_base = self
            .conn
            .prepare("PRAGMA table_info(games)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "is_missing_base");
        if !has_missing_base {
            self.conn.execute(
                "ALTER TABLE games ADD COLUMN is_missing_base BOOLEAN DEFAULT 0",
                [],
            )?;
        }

        // Migrate pre-existing databases that lack the runners.env_vars column
        // (stores the emulator options JSON: emulator_options map + custom_args).
        let has_runner_env = self
            .conn
            .prepare("PRAGMA table_info(runners)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "env_vars");
        if !has_runner_env {
            self.conn.execute("ALTER TABLE runners ADD COLUMN env_vars TEXT", [])?;
        }

        Ok(())
    }

    fn seed_defaults(&self) -> Result<()> {
        // This is deliberately an upsert rather than a first-run-only seed.  Older
        // installations receive new systems and corrected extension lists on startup.
        let platforms = [
            ("3ds", "Nintendo 3DS", "emulator", ".3ds, .cia, .cci, .cxi"),
            ("nds", "Nintendo DS", "emulator", ".nds, .ds"),
            (
                "snes",
                "Nintendo Super NES",
                "emulator",
                ".sfc, .smc, .zip, .7z",
            ),
            (
                "gba",
                "Nintendo Game Boy Advance",
                "emulator",
                ".gba, .zip, .7z",
            ),
            ("n64", "Nintendo 64", "emulator", ".z64, .v64, .n64, .zip"),
            (
                "ps1",
                "Sony PlayStation",
                "emulator",
                ".bin, .chd, .pbp, .cue",
            ),
            ("ps2", "Sony PlayStation 2", "emulator", ".iso, .chd"),
            (
                "psp",
                "Sony PlayStation Portable",
                "emulator",
                ".iso, .cso, .pbp",
            ),
            (
                "gamecube",
                "Nintendo GameCube",
                "emulator",
                ".iso, .gcz, .rvz",
            ),
            ("wii", "Nintendo Wii", "emulator", ".iso, .wbfs, .rvz"),
            // Unpacked Wii U games are identified through code/app.rpx.  The scanner
            // reads meta/meta.xml for their title and rejects update/DLC title IDs.
            (
                "wii_u",
                "Nintendo Wii U",
                "emulator",
                ".wua, .rpx, .wud, .wux",
            ),
            ("mame", "Arcade (MAME)", "emulator", ".zip, .7z"),
            (
                "dreamcast",
                "Sega Dreamcast",
                "emulator",
                ".chd, .gdi, .cdi",
            ),
            (
                "switch",
                "Nintendo Switch",
                "emulator",
                ".nsp, .xci, .nca, .nso",
            ),
            ("nes", "Nintendo NES", "emulator", ".nes, .zip, .7z"),
            ("vita", "Sony PlayStation Vita", "emulator", ".vpk, .zip"),
            (
                "gb",
                "Nintendo Game Boy (Color)",
                "emulator",
                ".gb, .gbc, .zip, .7z",
            ),
            ("windows", "Windows Games", "wine", ".exe, .bat"),
            ("linux", "Linux Native", "native", ".sh, .AppImage, .bin"),
            ("steam", "Steam Games", "steam", ""),
        ];

        for (slug, name, platform_type, extensions) in platforms {
            self.conn.execute(
                "INSERT INTO platforms (slug, name, platform_type, extensions) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(slug) DO UPDATE SET name = excluded.name, platform_type = excluded.platform_type, extensions = excluded.extensions",
                params![slug, name, platform_type, extensions],
            )?;
        }

        // Normalise legacy runner display names to clean canonical names
        // and delete any duplicate runner entries per platform.
        let canonical_rules = [
            ("%azahar%", "Azahar"),
            ("%dolphin%", "Dolphin"),
            ("%duckstation%", "DuckStation"),
            ("%pcsx2%", "PCSX2"),
            ("%ppsspp%", "PPSSPP"),
            ("%cemu%", "Cemu"),
            ("%ryujinx%", "Ryujinx"),
            ("%eden%", "Eden"),
            ("%melonds%", "melonDS"),
            ("%redream%", "Redream"),
            ("%vita3k%", "Vita3K"),
            ("%mame%", "MAME"),
        ];

        for (pattern, canonical_name) in canonical_rules {
            self.conn.execute(
                "UPDATE runners SET name = ?2 WHERE LOWER(name) LIKE ?1",
                params![pattern, canonical_name],
            )?;
        }

        // Purge obsolete emulators and system wine from runners table
        self.conn.execute(
            "DELETE FROM runners WHERE LOWER(TRIM(name)) IN ('citra', 'desmume', 'mesen', 'sameboy', 'snes9x', 'mgba', 'mupen64plus', 'wine', 'wine system')",
            [],
        )?;

        self.conn.execute(
            "DELETE FROM runners WHERE id NOT IN (
                SELECT MIN(id) FROM runners GROUP BY platform_id, LOWER(TRIM(name))
             )",
            [],
        )?;

        // Download links are direct GitHub release assets, not the HTML release page.
        // This makes the [w] action download the actual file after GitHub redirects.
        let runners = [
            ("3ds", "Azahar", "appimage", Some("https://github.com/AzaharPlus/AzaharPlus/releases/download/AZAHAR_PLUS_2126_0_A/azaharplus-2126.0-A-linux.AppImage"), Some("azahar.AppImage")),
            ("ps1", "DuckStation", "appimage", Some("https://github.com/stenzek/duckstation/releases/latest/download/DuckStation-x64.AppImage"), Some("DuckStation-x64.AppImage")),
            ("ps2", "PCSX2", "appimage", Some("https://github.com/PCSX2/pcsx2/releases/latest/download/pcsx2-v2.6.3-linux-appimage-x64-Qt.AppImage"), Some("pcsx2-v2.6.3-linux-appimage-x64-Qt.AppImage")),
            ("gamecube", "Dolphin", "appimage", Some("https://github.com/pkgforge-dev/Dolphin-emu-AppImage/releases/latest/download/Dolphin_Emulator-2606-anylinux-x86_64.AppImage"), Some("Dolphin_Emulator-2606-anylinux-x86_64.AppImage")),
            ("wii", "Dolphin", "appimage", Some("https://github.com/pkgforge-dev/Dolphin-emu-AppImage/releases/latest/download/Dolphin_Emulator-2606-anylinux-x86_64.AppImage"), Some("Dolphin_Emulator-2606-anylinux-x86_64.AppImage")),
            ("wii_u", "Cemu", "appimage", Some("https://github.com/cemu-project/Cemu/releases/latest/download/Cemu-2.6-x86_64.AppImage"), Some("Cemu-2.6-x86_64.AppImage")),
            // MAME: the URL is the GitHub latest-release API endpoint, resolved at
            // download time to the current anylinux-x86_64 AppImage asset (see
            // RunnerDownloader::resolve_mame_download_url). Version/asset filename
            // are never hardcoded so the download keeps working across releases.
            ("mame", "MAME", "appimage", Some("https://api.github.com/repos/pkgforge-dev/MAME-AppImage/releases/latest"), Some("MAME.AppImage")),
            ("psp", "PPSSPP", "appimage", Some("https://github.com/hrydgard/ppsspp/releases/latest/download/PPSSPP-v1.20.4-anylinux-x86_64.AppImage"), Some("PPSSPP-v1.20.4-anylinux-x86_64.AppImage")),
            ("dreamcast", "Redream", "appimage", None, None),
            ("switch", "Ryujinx", "appimage", None, None),
            ("switch", "Eden", "appimage", Some("https://stable.eden-emu.dev/v0.2.1/Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage"), Some("Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage")),
            ("nds", "melonDS", "appimage", Some("https://github.com/melonDS-emu/melonDS/releases/latest/download/melonDS-1.1-appimage-x86_64.zip"), Some("melonDS-1.1-appimage-x86_64.zip")),
            ("vita", "Vita3K", "appimage", None, None),
        ];

        for (platform_slug, name, runner_type, download_url, download_filename) in runners {
            self.conn.execute(
                "INSERT INTO runners (platform_id, name, runner_type, command_template, download_url, download_filename)
                 SELECT id, ?2, ?3, '\"{executable_path}\" \"{rom}\"', ?4, ?5 FROM platforms
                 WHERE slug = ?1 AND NOT EXISTS (
                    SELECT 1 FROM runners r WHERE r.platform_id = platforms.id AND r.name = ?2
                 )",
                params![platform_slug, name, runner_type, download_url, download_filename],
            )?;
            self.conn.execute(
                "UPDATE runners SET download_url = ?4, download_filename = ?5 WHERE platform_id = (SELECT id FROM platforms WHERE slug = ?1) AND name = ?2",
                params![platform_slug, name, runner_type, download_url, download_filename],
            )?;
        }

        // Auto-migrate any games mis-assigned to SNES/emulator platforms due to legacy platform_id lookups
        self.conn.execute(
            "UPDATE games SET platform_id = (SELECT id FROM platforms WHERE slug = 'windows') WHERE game_type = 'wine'",
            [],
        )?;
        self.conn.execute(
            "UPDATE games SET platform_id = (SELECT id FROM platforms WHERE slug = 'linux') WHERE game_type = 'native'",
            [],
        )?;
        self.conn.execute(
            "UPDATE games SET platform_id = (SELECT id FROM platforms WHERE slug = 'steam') WHERE game_type = 'steam'",
            [],
        )?;

        // A previous Azahar preset used an obsolete {runner} placeholder.
        self.conn.execute(
            "UPDATE runners SET command_template = '\"{executable_path}\" \"{rom}\"'
             WHERE command_template LIKE '%{runner}%'",
            [],
        )?;

        // Cemu requires -g before the ROM path to auto-launch games.
        self.conn.execute(
            "UPDATE runners SET command_template = '\"{executable_path}\" -g \"{rom}\"'
             WHERE name = 'Cemu' AND command_template NOT LIKE '%-g%'",
            [],
        )?;

        Ok(())
    }

    pub fn get_platform_slug_by_runner_name(&self, runner_name: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.slug FROM platforms p JOIN runners r ON r.platform_id = p.id WHERE r.name = ?1 LIMIT 1"
        )?;
        let mut rows = stmt.query(params![runner_name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_unique_runners(&self) -> Result<Vec<crate::models::UniqueRunnerInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.name, r.runner_type,
                    MAX(r.executable_path) as exe_path,
                    MAX(r.download_url) as dl_url,
                    MAX(r.download_filename) as dl_fn,
                    GROUP_CONCAT(p.slug, ',') as slugs,
                    MAX(r.is_configured) as is_cfg
             FROM runners r
             JOIN platforms p ON r.platform_id = p.id
             WHERE p.slug NOT IN ('linux', 'steam', 'windows')
             GROUP BY LOWER(TRIM(r.name))
             ORDER BY is_cfg DESC, r.name ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let runner_type: String = row.get(1)?;
            let executable_path: Option<String> = row.get(2)?;
            let download_url: Option<String> = row.get(3)?;
            let download_filename: Option<String> = row.get(4)?;
            let slugs_str: String = row.get(5)?;
            let is_cfg: bool = row.get(6)?;

            let slugs: Vec<&str> = slugs_str.split(',').collect();
            let initials = match name.as_str() {
                "Dolphin" => "GC, Wii".to_string(),
                "PCSX2" => "PS2".to_string(),
                "DuckStation" => "PS1".to_string(),
                "PPSSPP" => "PSP".to_string(),
                "Azahar" => "3DS".to_string(),
                "Cemu" => "Wii U".to_string(),
                "Ryujinx" => "Switch".to_string(),
                "Eden" => "Switch".to_string(),
                "melonDS" => "NDS".to_string(),
                "Redream" => "DC".to_string(),
                "Vita3K" => "PS Vita".to_string(),
                "MAME" => "Arcade".to_string(),
                _ => slugs.iter().map(|s| s.to_uppercase()).collect::<Vec<_>>().join(", "),
            };

            Ok(crate::models::UniqueRunnerInfo {
                name,
                console_initials: initials,
                executable_path,
                download_url,
                download_filename,
                runner_type,
                is_configured: is_cfg,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn update_runner_by_name(&self, runner_name: &str, exe_path: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE runners SET executable_path = ?2, is_configured = 1 WHERE name = ?1",
            params![runner_name, exe_path],
        )?;
        Ok(())
    }

    pub fn reset_runner_by_name(&self, runner_name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE runners SET executable_path = NULL, is_configured = 0 WHERE name = ?1",
            params![runner_name],
        )?;
        Ok(())
    }

    pub fn toggle_runner_configured(&self, runner_name: &str, is_configured: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE runners SET is_configured = ?2 WHERE name = ?1",
            params![runner_name, if is_configured { 1 } else { 0 }],
        )?;
        Ok(())
    }

    /// Read the emulator-options JSON stored in the runner's `env_vars` column.
    pub fn get_runner_env_by_name(&self, runner_name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT env_vars FROM runners WHERE name = ?1 LIMIT 1")?;
        let mut rows = stmt.query(params![runner_name])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }

    /// Persist the emulator-options JSON into the runner's `env_vars` column.
    pub fn update_runner_env_by_name(&self, runner_name: &str, env_vars: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE runners SET env_vars = ?2 WHERE name = ?1",
            params![runner_name, env_vars],
        )?;
        Ok(())
    }

    // ----------------------------------------------------
    // App Settings Queries
    // ----------------------------------------------------
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ----------------------------------------------------
    // Scan Folders Persistence
    // ----------------------------------------------------
    pub fn get_scan_folder_for_platform(&self, platform_id: i64) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM scan_folders WHERE platform_id = ?1 ORDER BY last_scanned_at DESC LIMIT 1"
        )?;
        let mut rows = stmt.query(params![platform_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn save_scan_folder(
        &self,
        platform_id: i64,
        folder_path: &str,
        recursive: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scan_folders (platform_id, path, recursive, last_scanned_at)
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(path) DO UPDATE SET
                platform_id = excluded.platform_id,
                recursive = excluded.recursive,
                last_scanned_at = CURRENT_TIMESTAMP",
            params![platform_id, folder_path, recursive],
        )?;
        Ok(())
    }

    // ----------------------------------------------------
    // Platforms & Runners Queries
    // ----------------------------------------------------
    pub fn get_platforms(&self) -> Result<Vec<Platform>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, platform_type, extensions, created_at FROM platforms ORDER BY name ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let ptype_str: String = row.get(3)?;
            let ptype = PlatformType::from(ptype_str.as_str());
            let ext_str: String = row.get(4)?;
            let extensions = ext_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            Ok(Platform {
                id: row.get(0)?,
                slug: row.get(1)?,
                name: row.get(2)?,
                platform_type: ptype,
                default_extensions: extensions,
            })
        })?;

        let mut platforms = Vec::new();
        for r in rows {
            platforms.push(r?);
        }
        Ok(platforms)
    }

    pub fn get_platform_by_slug(&self, slug: &str) -> Result<Option<Platform>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, platform_type, extensions FROM platforms WHERE slug = ?1",
        )?;
        let mut rows = stmt.query(params![slug])?;
        if let Some(row) = rows.next()? {
            let ptype_str: String = row.get(3)?;
            let ptype = PlatformType::from(ptype_str.as_str());
            let ext_str: String = row.get(4)?;
            let extensions = ext_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            Ok(Some(Platform {
                id: row.get(0)?,
                slug: row.get(1)?,
                name: row.get(2)?,
                platform_type: ptype,
                default_extensions: extensions,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_active_platforms(&self, show_all: bool) -> Result<Vec<Platform>> {
        let all = self.get_platforms()?;
        if show_all {
            return Ok(all);
        }

        let mut active = Vec::new();
        for p in all {
            let game_count = self.get_game_count_for_platform(p.id)?;

            if game_count > 0 {
                active.push(p);
            }
        }

        Ok(active)
    }

    pub fn get_game_count_for_platform(&self, platform_id: i64) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM games WHERE platform_id = ?1",
            params![platform_id],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn get_runners_for_platform(&self, platform_id: i64) -> Result<Vec<Runner>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, name, runner_type, executable_path, command_template, default_env, download_url, download_filename, is_default, env_vars FROM runners WHERE platform_id = ?1",
        )?;

        let rows = stmt.query_map(params![platform_id], |row| {
            Ok(Runner {
                id: row.get(0)?,
                platform_id: row.get(1)?,
                name: row.get(2)?,
                runner_type: row.get(3)?,
                executable_path: row.get(4)?,
                command_template: row.get(5)?,
                default_env: row.get(6)?,
                download_url: row.get(7)?,
                download_filename: row.get(8)?,
                is_default: row.get(9)?,
                env_vars: row.get(10)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn get_runner_for_platform(&self, platform_id: i64) -> Result<Option<Runner>> {
        let runners = self.get_runners_for_platform(platform_id)?;
        if let Some(r) = runners.iter().find(|r| r.is_default).cloned() {
            return Ok(Some(r));
        }
        // Multi-runner platforms (e.g. Switch: Ryujinx + Eden) may only have one
        // emulator installed. Prefer a configured runner so the platform reads
        // as "ready" and launches with the emulator that is actually installed.
        if let Some(r) = runners.iter().find(|r| r.executable_path.is_some()).cloned() {
            return Ok(Some(r));
        }
        Ok(runners.into_iter().next())
    }

    pub fn update_runner_config(
        &self,
        runner_id: i64,
        exe_path: &str,
        is_configured: bool,
    ) -> Result<()> {
        let _ = is_configured;
        self.conn.execute(
            "UPDATE runners SET executable_path = ?1, is_configured = 1 WHERE id = ?2",
            params![exe_path, runner_id],
        )?;
        Ok(())
    }

    pub fn reset_runner_config(&self, runner_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE runners SET executable_path = NULL, is_configured = 0 WHERE id = ?1",
            params![runner_id],
        )?;
        Ok(())
    }

    // ----------------------------------------------------
    // Media cache queries
    // ----------------------------------------------------
    pub fn media_statuses(&self, game_id: i64) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT media_type, status FROM game_media WHERE game_id = ?1",
        )?;
        let rows = stmt.query_map(params![game_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_media_status(&self, game_id: i64, media_type: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM game_media WHERE game_id = ?1 AND media_type = ?2",
        )?;
        let mut rows = stmt.query(params![game_id, media_type])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn record_media_status(
        &self,
        game_id: i64,
        media_type: &str,
        status: &str,
        file_path: Option<&str>,
        url: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO game_media (game_id, media_type, file_path, source, url, status)
             VALUES (?1, ?2, ?3, 'steamgriddb', ?4, ?5)
             ON CONFLICT(game_id, media_type) DO UPDATE SET
                file_path = excluded.file_path, url = excluded.url, status = excluded.status,
                updated_at = CURRENT_TIMESTAMP",
            params![game_id, media_type, file_path, url, status],
        )?;
        Ok(())
    }

    // ----------------------------------------------------
    // Games Queries
    // ----------------------------------------------------
    pub fn get_games_for_platform(&self, platform_id: i64) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, title, sort_title, game_type, file_path, working_dir, custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid, file_name, file_extension, file_size, file_hash_crc32, file_hash_md5, file_hash_sha1, serial, release_year, developer, publisher, description, genre, rating, favorite, play_count, play_time_seconds, last_played_at, created_at, updated_at, is_missing_base FROM games WHERE platform_id = ?1 ORDER BY title ASC",
        )?;

        let rows = stmt.query_map(params![platform_id], |row| {
            Ok(Game {
                id: row.get(0)?,
                platform_id: row.get(1)?,
                title: row.get(2)?,
                sort_title: row.get(3)?,
                game_type: row.get(4)?,
                file_path: row.get(5)?,
                working_dir: row.get(6)?,
                custom_command: row.get(7)?,
                env_vars: row.get(8)?,
                wine_prefix: row.get(9)?,
                wine_runner_id: row.get(10)?,
                steam_appid: row.get(11)?,
                file_name: row.get(12)?,
                file_extension: row.get(13)?,
                file_size: row.get(14)?,
                file_hash_crc32: row.get(15)?,
                file_hash_md5: row.get(16)?,
                file_hash_sha1: row.get(17)?,
                serial: row.get(18)?,
                release_year: row.get(19)?,
                developer: row.get(20)?,
                publisher: row.get(21)?,
                description: row.get(22)?,
                genre: row.get(23)?,
                rating: row.get(24)?,
                favorite: row.get(25)?,
                play_count: row.get(26)?,
                play_time_seconds: row.get(27)?,
                last_played_at: row.get(28)?,
                created_at: row.get(29)?,
                updated_at: row.get(30)?,
                is_missing_base: row.get(31)?,
                components: Vec::new(),
            })
        })?;

        let mut games = Vec::new();
        for g in rows {
            games.push(g?);
        }
        self.attach_components(&mut games, Some(platform_id))?;
        Ok(games)
    }

    pub fn get_all_games(&self) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, title, sort_title, game_type, file_path, working_dir, custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid, file_name, file_extension, file_size, file_hash_crc32, file_hash_md5, file_hash_sha1, serial, release_year, developer, publisher, description, genre, rating, favorite, play_count, play_time_seconds, last_played_at, created_at, updated_at, is_missing_base FROM games ORDER BY title ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Game {
                id: row.get(0)?,
                platform_id: row.get(1)?,
                title: row.get(2)?,
                sort_title: row.get(3)?,
                game_type: row.get(4)?,
                file_path: row.get(5)?,
                working_dir: row.get(6)?,
                custom_command: row.get(7)?,
                env_vars: row.get(8)?,
                wine_prefix: row.get(9)?,
                wine_runner_id: row.get(10)?,
                steam_appid: row.get(11)?,
                file_name: row.get(12)?,
                file_extension: row.get(13)?,
                file_size: row.get(14)?,
                file_hash_crc32: row.get(15)?,
                file_hash_md5: row.get(16)?,
                file_hash_sha1: row.get(17)?,
                serial: row.get(18)?,
                release_year: row.get(19)?,
                developer: row.get(20)?,
                publisher: row.get(21)?,
                description: row.get(22)?,
                genre: row.get(23)?,
                rating: row.get(24)?,
                favorite: row.get(25)?,
                play_count: row.get(26)?,
                play_time_seconds: row.get(27)?,
                last_played_at: row.get(28)?,
                created_at: row.get(29)?,
                updated_at: row.get(30)?,
                is_missing_base: row.get(31)?,
                components: Vec::new(),
            })
        })?;

        let mut games = Vec::new();
        for g in rows {
            games.push(g?);
        }
        self.attach_components(&mut games, None)?;
        Ok(games)
    }

    pub fn insert_game(&self, game: &Game) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO games (
                platform_id, title, sort_title, game_type, file_path, working_dir,
                custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid,
                file_name, file_extension, file_size, file_hash_crc32, file_hash_md5, file_hash_sha1, serial,
                is_missing_base
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            ON CONFLICT(file_path) DO UPDATE SET
                title = excluded.title,
                serial = excluded.serial,
                file_hash_md5 = excluded.file_hash_md5,
                file_hash_crc32 = excluded.file_hash_crc32,
                file_size = excluded.file_size,
                is_missing_base = excluded.is_missing_base,
                updated_at = CURRENT_TIMESTAMP",
            params![
                game.platform_id,
                game.title,
                game.sort_title,
                game.game_type,
                game.file_path,
                game.working_dir,
                game.custom_command,
                game.env_vars,
                game.wine_prefix,
                game.wine_runner_id,
                game.steam_appid,
                game.file_name,
                game.file_extension,
                game.file_size,
                game.file_hash_crc32,
                game.file_hash_md5,
                game.file_hash_sha1,
                game.serial,
                game.is_missing_base,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Return the rowid of the game owning `file_path`, if any. Used after an
    /// upsert so Switch components are linked to the correct (possibly reused)
    /// game row instead of a guessed rowid.
    pub fn get_game_id_by_file_path(&self, file_path: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM games WHERE file_path = ?1")?;
        let mut rows = stmt.query(params![file_path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn insert_game_component(&self, game_id: i64, comp: &GameComponent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO game_components
                (game_id, category, file_path, file_name, file_extension, file_size, is_launchable, title_id, version, discarded)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(game_id, file_path) DO UPDATE SET
                category = excluded.category,
                file_name = excluded.file_name,
                file_extension = excluded.file_extension,
                file_size = excluded.file_size,
                is_launchable = excluded.is_launchable,
                title_id = excluded.title_id,
                version = excluded.version,
                discarded = excluded.discarded",
            params![
                game_id,
                comp.category,
                comp.file_path,
                comp.file_name,
                comp.file_extension,
                comp.file_size,
                comp.is_launchable,
                comp.title_id,
                comp.version,
                comp.discarded,
            ],
        )?;
        Ok(())
    }

    /// Fill `game.components` for the given games from the game_components table.
    fn attach_components(&self, games: &mut [Game], platform_id: Option<i64>) -> Result<()> {
        let mut stmt = if platform_id.is_some() {
            self.conn.prepare(
                "SELECT gc.game_id, gc.id, gc.category, gc.file_path, gc.file_name, gc.file_extension, gc.file_size, gc.is_launchable, gc.title_id, gc.version, gc.discarded
                 FROM game_components gc JOIN games g ON gc.game_id = g.id
                 WHERE g.platform_id = ?1",
            )?
        } else {
            self.conn.prepare(
                "SELECT gc.game_id, gc.id, gc.category, gc.file_path, gc.file_name, gc.file_extension, gc.file_size, gc.is_launchable, gc.title_id, gc.version, gc.discarded
                 FROM game_components gc",
            )?
        };

        let mut by_game: HashMap<i64, Vec<GameComponent>> = HashMap::new();
        if let Some(pid) = platform_id {
            let rows = stmt.query_map(params![pid], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    GameComponent {
                        id: row.get(1)?,
                        game_id: row.get(0)?,
                        category: row.get(2)?,
                        file_path: row.get(3)?,
                        file_name: row.get(4)?,
                        file_extension: row.get(5)?,
                        file_size: row.get(6)?,
                        is_launchable: row.get(7)?,
                        title_id: row.get(8)?,
                        version: row.get(9)?,
                        discarded: row.get(10)?,
                    },
                ))
            })?;
            for r in rows {
                let (gid, comp) = r?;
                by_game.entry(gid).or_default().push(comp);
            }
        } else {
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    GameComponent {
                        id: row.get(1)?,
                        game_id: row.get(0)?,
                        category: row.get(2)?,
                        file_path: row.get(3)?,
                        file_name: row.get(4)?,
                        file_extension: row.get(5)?,
                        file_size: row.get(6)?,
                        is_launchable: row.get(7)?,
                        title_id: row.get(8)?,
                        version: row.get(9)?,
                        discarded: row.get(10)?,
                    },
                ))
            })?;
            for r in rows {
                let (gid, comp) = r?;
                by_game.entry(gid).or_default().push(comp);
            }
        }

        for game in games {
            game.components = by_game.remove(&game.id).unwrap_or_default();
        }
        Ok(())
    }

    pub fn update_game(&self, game: &Game) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET
                title = ?1,
                file_path = ?2,
                working_dir = ?3,
                custom_command = ?4,
                wine_prefix = ?5,
                steam_appid = ?6,
                env_vars = ?7,
                updated_at = CURRENT_TIMESTAMP
             WHERE id = ?8",
            params![
                game.title,
                game.file_path,
                game.working_dir,
                game.custom_command,
                game.wine_prefix,
                game.steam_appid,
                game.env_vars,
                game.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_game(&self, game_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM games WHERE id = ?1", params![game_id])?;
        Ok(())
    }

    pub fn delete_games(&self, game_ids: &[i64]) -> Result<usize> {
        let mut deleted = 0;
        for id in game_ids {
            if self.delete_game(*id).is_ok() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn seeds_the_full_emulator_registry_and_runner_presets() {
        let path = std::env::temp_dir().join(format!(
            "tui_game_station_db_test_{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let db = Database::open(&path).unwrap();

        let platforms = db.get_platforms().unwrap();
        let wii_u = platforms
            .iter()
            .find(|platform| platform.slug == "wii_u")
            .unwrap();
        assert!(wii_u.default_extensions.contains(&".rpx".to_string()));
        assert!(platforms.iter().any(|platform| platform.slug == "gamecube"));
        assert!(platforms.iter().any(|platform| platform.slug == "mame"));

        let cemu = db
            .get_runners_for_platform(wii_u.id)
            .unwrap()
            .into_iter()
            .find(|runner| runner.name == "Cemu")
            .unwrap();
        assert_eq!(cemu.command_template, "\"{executable_path}\" -g \"{rom}\"");
        assert_eq!(
            cemu.download_url.as_deref(),
            Some("https://github.com/cemu-project/Cemu/releases/latest/download/Cemu-2.6-x86_64.AppImage")
        );

        let nds = platforms
            .iter()
            .find(|platform| platform.slug == "nds")
            .unwrap();
        let melonds = db
            .get_runners_for_platform(nds.id)
            .unwrap()
            .into_iter()
            .find(|runner| runner.name == "melonDS")
            .unwrap();
        assert_eq!(
            melonds.download_filename.as_deref(),
            Some("melonDS-1.1-appimage-x86_64.zip")
        );

        let mame = platforms
            .iter()
            .find(|platform| platform.slug == "mame")
            .unwrap();
        let mame_runner = db
            .get_runners_for_platform(mame.id)
            .unwrap()
            .into_iter()
            .find(|runner| runner.name == "MAME")
            .unwrap();
        assert_eq!(
            mame_runner.download_url.as_deref(),
            Some("https://api.github.com/repos/pkgforge-dev/MAME-AppImage/releases/latest")
        );
        assert_eq!(mame_runner.download_filename.as_deref(), Some("MAME.AppImage"));
        assert_eq!(mame_runner.runner_type, "appimage");

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
