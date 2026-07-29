use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

use crate::models::{Game, Platform, PlatformType, Runner};

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
                is_default BOOLEAN DEFAULT 0
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
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;
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

        // Preserve a previously configured 3DS runner while normalising the old
        // display names and launch templates to the current preset convention.
        self.conn.execute(
            "UPDATE runners
             SET name = CASE name
                    WHEN 'Azahar (3DS Emulator)' THEN 'Azahar'
                    WHEN 'Citra (3DS Emulator)' THEN 'Citra'
                    ELSE name END,
                 command_template = '\"{executable_path}\" \"{rom}\"'
             WHERE platform_id = (SELECT id FROM platforms WHERE slug = '3ds')
               AND name IN ('Azahar (3DS Emulator)', 'Citra (3DS Emulator)')",
            [],
        )?;

        // Download links are direct GitHub release assets, not the HTML release page.
        // This makes the [w] action download the actual file after GitHub redirects.
        let runners = [
            ("3ds", "Azahar", "appimage", Some("https://github.com/AzaharPlus/AzaharPlus/releases/download/v2126.0-A/azaharplus-2126.0-A-linux.AppImage"), Some("azahar.AppImage")),
            ("3ds", "Citra", "system", None, None),
            ("ps1", "DuckStation", "appimage", Some("https://github.com/stenzek/duckstation/releases/latest/download/DuckStation-x64.AppImage"), Some("DuckStation-x64.AppImage")),
            ("ps2", "PCSX2", "appimage", Some("https://github.com/PCSX2/pcsx2/releases/latest/download/pcsx2-v2.6.3-linux-appimage-x64-Qt.AppImage"), Some("pcsx2-v2.6.3-linux-appimage-x64-Qt.AppImage")),
            ("gamecube", "Dolphin", "appimage", Some("https://github.com/pkgforge-dev/Dolphin-emu-AppImage/releases/latest/download/Dolphin_Emulator-2606-anylinux-x86_64.AppImage"), Some("Dolphin_Emulator-2606-anylinux-x86_64.AppImage")),
            ("wii", "Dolphin", "appimage", Some("https://github.com/pkgforge-dev/Dolphin-emu-AppImage/releases/latest/download/Dolphin_Emulator-2606-anylinux-x86_64.AppImage"), Some("Dolphin_Emulator-2606-anylinux-x86_64.AppImage")),
            ("wii_u", "Cemu", "appimage", Some("https://github.com/cemu-project/Cemu/releases/latest/download/Cemu-2.6-x86_64.AppImage"), Some("Cemu-2.6-x86_64.AppImage")),
            ("mame", "MAME", "system", None, None),
            ("psp", "PPSSPP", "appimage", Some("https://github.com/hrydgard/ppsspp/releases/latest/download/PPSSPP-v1.20.4-anylinux-x86_64.AppImage"), Some("PPSSPP-v1.20.4-anylinux-x86_64.AppImage")),
            ("dreamcast", "Redream", "appimage", None, None),
            ("switch", "Ryujinx", "appimage", None, None),
            ("nds", "melonDS", "appimage", Some("https://github.com/melonDS-emu/melonDS/releases/latest/download/melonDS-1.1-appimage-x86_64.zip"), Some("melonDS-1.1-appimage-x86_64.zip")),
            ("nds", "DeSmuME", "system", None, None),
            ("gba", "mGBA", "appimage", None, None),
            ("nes", "Mesen", "appimage", None, None),
            ("vita", "Vita3K", "appimage", None, None),
            ("n64", "Mupen64Plus", "system", None, None),
            ("snes", "Snes9x", "appimage", None, None),
            ("gb", "SameBoy", "appimage", None, None),
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
                "UPDATE runners SET runner_type = ?3, command_template = '\"{executable_path}\" \"{rom}\"',
                 download_url = ?4, download_filename = ?5
                 WHERE platform_id = (SELECT id FROM platforms WHERE slug = ?1) AND name = ?2",
                params![platform_slug, name, runner_type, download_url, download_filename],
            )?;
        }

        // A previous Azahar preset used an obsolete {runner} placeholder.
        self.conn.execute(
            "UPDATE runners SET command_template = '\"{executable_path}\" \"{rom}\"'
             WHERE command_template LIKE '%{runner}%'",
            [],
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

    pub fn get_active_platforms(&self, show_all: bool) -> Result<Vec<Platform>> {
        let all = self.get_platforms()?;
        if show_all {
            return Ok(all);
        }

        let mut active = Vec::new();
        for p in all {
            let game_count = self.get_game_count_for_platform(p.id)?;
            let runner = self.get_runner_for_platform(p.id)?;

            if game_count > 0 || runner.is_some() || p.slug == "steam" || p.slug == "linux" {
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
            "SELECT id, platform_id, name, runner_type, executable_path, command_template, default_env, download_url, download_filename, is_default FROM runners WHERE platform_id = ?1",
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
        Ok(runners.into_iter().find(|r| r.executable_path.is_some()))
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
    // Games Queries
    // ----------------------------------------------------
    pub fn get_games_for_platform(&self, platform_id: i64) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, title, sort_title, game_type, file_path, working_dir, custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid, file_name, file_extension, file_size, file_hash_crc32, file_hash_md5, file_hash_sha1, serial, release_year, developer, publisher, description, genre, rating, favorite, play_count, play_time_seconds, last_played_at, created_at, updated_at FROM games WHERE platform_id = ?1 ORDER BY title ASC",
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
            })
        })?;

        let mut games = Vec::new();
        for g in rows {
            games.push(g?);
        }
        Ok(games)
    }

    pub fn insert_game(&self, game: &Game) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO games (
                platform_id, title, sort_title, game_type, file_path, working_dir,
                custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid,
                file_name, file_extension, file_size, file_hash_crc32, file_hash_md5, file_hash_sha1
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(file_path) DO UPDATE SET
                title = excluded.title,
                file_size = excluded.file_size,
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
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
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
        assert_eq!(cemu.command_template, "\"{executable_path}\" \"{rom}\"");
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

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
