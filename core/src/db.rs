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

            CREATE TABLE IF NOT EXISTS local_dat_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform_slug TEXT NOT NULL,
                name TEXT NOT NULL,
                crc32 TEXT,
                md5 TEXT,
                sha1 TEXT,
                serial TEXT,
                developer TEXT,
                publisher TEXT,
                release_year INTEGER
            );

            CREATE TABLE IF NOT EXISTS game_media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                media_type TEXT NOT NULL,
                file_path TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT,
                UNIQUE(game_id, media_type)
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            ",
        )?;

        // Auto-migration for existing DB files
        let _ = self.conn.execute("ALTER TABLE runners ADD COLUMN is_configured BOOLEAN DEFAULT 0", []);

        Ok(())
    }

    fn seed_defaults(&self) -> Result<()> {
        let platforms = vec![
            ("3ds", "Nintendo 3DS", "emulator", ".3ds,.cia,.cci"),
            ("snes", "Nintendo Super NES", "emulator", ".sfc,.smc,.zip,.7z"),
            ("nes", "Nintendo NES", "emulator", ".nes,.unf,.zip,.7z"),
            ("gba", "Nintendo Game Boy Advance", "emulator", ".gba,.zip,.7z"),
            ("gb", "Nintendo Game Boy / Color", "emulator", ".gb,.gbc,.zip,.7z"),
            ("n64", "Nintendo 64", "emulator", ".z64,.v64,.n64,.zip"),
            ("gamecube", "Nintendo GameCube", "emulator", ".iso,.gcz,.rvz,.ciso"),
            ("wii", "Nintendo Wii", "emulator", ".iso,.wbfs,.rvz"),
            ("wii_u", "Nintendo Wii U", "emulator", ".wud,.wux,.rpx,.wua"),
            ("ds", "Nintendo DS", "emulator", ".nds,.ds"),
            ("switch", "Nintendo Switch", "emulator", ".nsp,.xci,.nca,.nso"),
            ("ps1", "Sony PlayStation", "emulator", ".bin,.chd,.pbp,.cue,.iso,.img"),
            ("ps2", "Sony PlayStation 2", "emulator", ".iso,.chd,.cso"),
            ("psp", "Sony PlayStation Portable", "emulator", ".iso,.cso,.pbp"),
            ("vita", "Sony PlayStation Vita", "emulator", ".vpk,.zip"),
            ("dreamcast", "Sega Dreamcast", "emulator", ".chd,.gdi,.cdi"),
            ("mame", "Arcade / MAME", "emulator", ".zip,.7z"),
            ("linux", "Linux Native Games", "native", ".sh,.x86_64,.bin,.appimage,.desktop"),
            ("windows", "Windows Games (Wine/Proton)", "wine", ".exe,.bat,.lnk"),
            ("steam", "Steam Games", "steam", ""),
        ];

        for (slug, name, ptype, exts) in platforms {
            self.conn.execute(
                "INSERT INTO platforms (slug, name, platform_type, extensions)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(slug) DO UPDATE SET extensions = ?4",
                params![slug, name, ptype, exts],
            )?;
        }

        // Seed preset runners per platform
        let default_runners = vec![
            ("3ds", "Azahar (AppImage / Binary)", "appimage", "\"{executable_path}\" \"{rom}\""),
            ("3ds", "Citra (AppImage / Binary)", "appimage", "\"{executable_path}\" \"{rom}\""),
            ("snes", "Snes9x (Libretro)", "libretro", "retroarch -L /usr/lib/libretro/snes9x_libretro.so \"{rom}\""),
            ("ps1", "DuckStation (Standalone)", "standalone_emulator", "duckstation-qt \"{rom}\""),
            ("ps2", "PCSX2 (Standalone)", "standalone_emulator", "pcsx2-qt \"{rom}\""),
            ("gamecube", "Dolphin (Standalone)", "standalone_emulator", "dolphin-emu -e \"{rom}\""),
            ("wii", "Dolphin (Standalone)", "standalone_emulator", "dolphin-emu -e \"{rom}\""),
            ("gba", "mGBA (Libretro)", "libretro", "retroarch -L /usr/lib/libretro/mgba_libretro.so \"{rom}\""),
            ("linux", "Native Binary", "native", "\"{file_path}\""),
            ("windows", "Wine System", "wine", "wine \"{file_path}\""),
            ("steam", "Steam Launcher", "steam", "steam steam://rungameid/{steam_appid}"),
        ];

        for (p_slug, r_name, r_type, cmd) in default_runners {
            let platform_id: Option<i64> = self.conn
                .query_row("SELECT id FROM platforms WHERE slug = ?1", params![p_slug], |r| r.get(0))
                .ok();

            if let Some(pid) = platform_id {
                let count: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM runners WHERE platform_id = ?1 AND name = ?2",
                    params![pid, r_name],
                    |r| r.get(0),
                )?;

                if count == 0 {
                    self.conn.execute(
                        "INSERT INTO runners (platform_id, name, runner_type, command_template, is_configured, is_default)
                         VALUES (?1, ?2, ?3, ?4, 0, 1)",
                        params![pid, r_name, r_type, cmd],
                    )?;
                }
            }
        }

        Ok(())
    }

    pub fn get_platforms(&self) -> Result<Vec<Platform>> {
        self.get_active_platforms(true)
    }

    /// Return platforms filtered by active status (has games OR user configured runner)
    pub fn get_active_platforms(&self, show_all: bool) -> Result<Vec<Platform>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, platform_type, extensions FROM platforms ORDER BY name ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let slug: String = row.get(1)?;
            let name: String = row.get(2)?;
            let ptype_str: String = row.get(3)?;
            let exts_str: String = row.get(4)?;

            let default_extensions = exts_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            Ok(Platform {
                id,
                slug,
                name,
                platform_type: PlatformType::from(ptype_str.as_str()),
                default_extensions,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            let platform = r?;
            if show_all {
                list.push(platform);
            } else {
                // Check if platform has games in DB
                let game_count: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM games WHERE platform_id = ?1",
                    params![platform.id],
                    |r| r.get(0),
                ).unwrap_or(0);

                if game_count > 0 {
                    list.push(platform);
                    continue;
                }

                // Check if any runner is explicitly configured for this platform
                let configured_runner_count: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM runners WHERE platform_id = ?1 AND is_configured = 1",
                    params![platform.id],
                    |r| r.get(0),
                ).unwrap_or(0);

                if configured_runner_count > 0 {
                    list.push(platform);
                    continue;
                }

                // Always include Native Linux and Windows Wine platforms for manual game additions
                if platform.slug == "linux" || platform.slug == "windows" {
                    list.push(platform);
                }
            }
        }

        Ok(list)
    }

    pub fn get_runners_for_platform(&self, platform_id: i64) -> Result<Vec<Runner>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, name, runner_type, executable_path, command_template, default_env, is_default
             FROM runners
             WHERE platform_id = ?1
             ORDER BY is_default DESC, name ASC",
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
                is_default: row.get(7)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn update_runner_config(&self, runner_id: i64, executable_path: &str, is_default: bool) -> Result<()> {
        let runner_platform_id: i64 = self.conn.query_row(
            "SELECT platform_id FROM runners WHERE id = ?1",
            params![runner_id],
            |r| r.get(0),
        )?;

        if is_default {
            // Unset previous defaults for this platform
            self.conn.execute(
                "UPDATE runners SET is_default = 0 WHERE platform_id = ?1",
                params![runner_platform_id],
            )?;
        }

        self.conn.execute(
            "UPDATE runners SET executable_path = ?1, is_configured = 1, is_default = ?2 WHERE id = ?3",
            params![executable_path, is_default, runner_id],
        )?;

        Ok(())
    }

    pub fn get_games_for_platform(&self, platform_id: i64) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, title, sort_title, game_type, file_path, working_dir, custom_command,
                    env_vars, wine_prefix, wine_runner_id, steam_appid, file_name, file_extension, file_size,
                    file_hash_crc32, file_hash_md5, file_hash_sha1, serial, release_year, developer, publisher,
                    description, genre, rating, favorite, play_count, play_time_seconds, last_played_at,
                    created_at, updated_at
             FROM games
             WHERE platform_id = ?1
             ORDER BY favorite DESC, title ASC",
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

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn insert_game(&self, game: &Game) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO games (platform_id, title, sort_title, game_type, file_path, working_dir,
                                custom_command, env_vars, wine_prefix, wine_runner_id, steam_appid,
                                file_name, file_extension, file_size, file_hash_crc32, file_hash_md5,
                                file_hash_sha1, serial, release_year, developer, publisher, description,
                                genre, rating, favorite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
             ON CONFLICT(file_path) DO UPDATE SET title = ?2, file_hash_crc32 = ?15, file_hash_md5 = ?16",
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
                game.release_year,
                game.developer,
                game.publisher,
                game.description,
                game.genre,
                game.rating,
                game.favorite,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_runner_for_platform(&self, platform_id: i64) -> Result<Option<Runner>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_id, name, runner_type, executable_path, command_template, default_env, is_default
             FROM runners
             WHERE platform_id = ?1
             ORDER BY is_default DESC, is_configured DESC, id ASC
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![platform_id], |row| {
            Ok(Runner {
                id: row.get(0)?,
                platform_id: row.get(1)?,
                name: row.get(2)?,
                runner_type: row.get(3)?,
                executable_path: row.get(4)?,
                command_template: row.get(5)?,
                default_env: row.get(6)?,
                is_default: row.get(7)?,
            })
        })?;

        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }
}
