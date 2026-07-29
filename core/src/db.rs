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
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM platforms", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }

        self.conn.execute_batch(
            "
            INSERT INTO platforms (slug, name, platform_type, extensions) VALUES
            ('3ds', 'Nintendo 3DS', 'emulator', '.3ds, .cia, .cci, .cxi'),
            ('nds', 'Nintendo DS', 'emulator', '.nds'),
            ('snes', 'Nintendo Super NES', 'emulator', '.snes, .smc, .sfc'),
            ('gba', 'Nintendo Game Boy Advance', 'emulator', '.gba'),
            ('n64', 'Nintendo 64', 'emulator', '.n64, .z64, .v64'),
            ('ps1', 'Sony PlayStation', 'emulator', '.iso, .cue, .bin, .chd, .pbp'),
            ('ps2', 'Sony PlayStation 2', 'emulator', '.iso, .chd, .bin'),
            ('psp', 'Sony PlayStation Portable', 'emulator', '.iso, .cso, .chd'),
            ('switch', 'Nintendo Switch', 'emulator', '.nsp, .xci'),
            ('windows', 'Windows Games', 'wine', '.exe, .bat'),
            ('linux', 'Linux Native', 'native', '.sh, .AppImage, .bin'),
            ('steam', 'Steam Games', 'steam', '');

            -- Seed preset runners
            INSERT INTO runners (platform_id, name, runner_type, download_url, download_filename, command_template)
            SELECT id, 'Azahar (3DS Emulator)', 'appimage', 'https://github.com/AzaharPlus/AzaharPlus/releases/download/v2126.0-A/azaharplus-2126.0-A-linux.AppImage', 'azahar.AppImage', '{runner} {rom}'
            FROM platforms WHERE slug = '3ds';

            INSERT INTO runners (platform_id, name, runner_type, command_template)
            SELECT id, 'Citra (3DS Emulator)', 'system', 'citra {rom}'
            FROM platforms WHERE slug = '3ds';
            ",
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

    pub fn save_scan_folder(&self, platform_id: i64, folder_path: &str, recursive: bool) -> Result<()> {
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
            let extensions = ext_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

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

    pub fn update_runner_config(&self, runner_id: i64, exe_path: &str, is_configured: bool) -> Result<()> {
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
        self.conn.execute("DELETE FROM games WHERE id = ?1", params![game_id])?;
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
