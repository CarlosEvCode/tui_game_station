use game_core::db::Database;
use game_core::models::Game;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorOverrideChoice {
    /// `None` for "Heredado (...)", `Some(id)` for specific configured runner.
    pub runner_id: Option<i64>,
    pub display_label: String,
}

pub struct EditGameFormHelper;

impl EditGameFormHelper {
    /// Build list of choices for the Emulator selector in Edit Game Details.
    /// Choice 0: "Heredado (mostrando entre paréntesis el efectivo actual, ej. 'Heredado (Citron — de carpeta)')"
    /// Choices 1..N: Configured emulators compatible with the game's platform.
    pub fn get_emulator_choices(db: &Database, game: &Game) -> Vec<EmulatorOverrideChoice> {
        let mut choices = Vec::new();

        // 1. Calculate effective inherited emulator and label (without game-level override)
        let inherited_runner = db
            .get_runner_for_game(game.platform_id, game.folder_id, None)
            .ok()
            .flatten();

        let inherited_label = if let Some(ref r) = inherited_runner {
            // Determine if the effective runner came from folder or platform
            let came_from_folder = if let Some(fid) = game.folder_id {
                if let Ok(Some(folder)) = db.get_scanned_folder(fid) {
                    if let Some(folder_emu_id) = folder.assigned_emulator_id {
                        folder_emu_id == r.id
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if came_from_folder {
                format!("Heredado ({} — de carpeta)", r.name)
            } else {
                format!("Heredado ({} — de plataforma)", r.name)
            }
        } else {
            "Heredado (Sin emulador)".to_string()
        };

        choices.push(EmulatorOverrideChoice {
            runner_id: None,
            display_label: inherited_label,
        });

        // 2. Add configured runners for this platform
        if let Ok(runners) = db.get_runners_for_platform(game.platform_id) {
            for r in runners {
                if r.executable_path.is_some() {
                    choices.push(EmulatorOverrideChoice {
                        runner_id: Some(r.id),
                        display_label: r.name.clone(),
                    });
                }
            }
        }

        choices
    }

    /// Return index of currently selected choice matching `emulator_override`.
    pub fn get_current_choice_idx(
        choices: &[EmulatorOverrideChoice],
        emulator_override: Option<i64>,
    ) -> usize {
        match emulator_override {
            None => 0,
            Some(id) => choices
                .iter()
                .position(|c| c.runner_id == Some(id))
                .unwrap_or(0),
        }
    }

    /// Cycle emulator override choice left (prev=true) or right (prev=false).
    pub fn cycle_choice(
        choices: &[EmulatorOverrideChoice],
        current_override: Option<i64>,
        prev: bool,
    ) -> Option<i64> {
        if choices.is_empty() {
            return None;
        }
        let curr_idx = Self::get_current_choice_idx(choices, current_override);
        let next_idx = if prev {
            if curr_idx == 0 {
                choices.len() - 1
            } else {
                curr_idx - 1
            }
        } else {
            (curr_idx + 1) % choices.len()
        };
        choices[next_idx].runner_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::db::Database;
    use std::path::PathBuf;

    fn test_db() -> (Database, PathBuf) {
        let mut path = std::env::temp_dir();
        path.push(format!("tui_edit_game_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::open(&path).unwrap();
        (db, path)
    }

    #[test]
    fn test_get_emulator_choices_labels() {
        let (db, path) = test_db();
        let switch = db.get_platform_by_slug("switch").unwrap().unwrap();

        let runners = db.get_runners_for_platform(switch.id).unwrap();
        let ryujinx = runners.iter().find(|r| r.name == "Ryujinx").unwrap();
        let citron = runners.iter().find(|r| r.name == "Citron").unwrap();
        db.update_runner_config(ryujinx.id, "/fake/ryujinx", true).unwrap();
        db.update_runner_config(citron.id, "/fake/citron", true).unwrap();

        let mut game = Game {
            id: 0,
            platform_id: switch.id,
            folder_id: None,
            emulator_override: None,
            title: "Zelda".to_string(),
            sort_title: None,
            game_type: "emulator".to_string(),
            file_path: Some("/fake/zelda.nsp".to_string()),
            working_dir: None,
            custom_command: None,
            env_vars: None,
            wine_prefix: None,
            wine_runner_id: None,
            steam_appid: None,
            file_name: None,
            file_extension: None,
            file_size: None,
            file_hash_crc32: None,
            file_hash_md5: None,
            file_hash_sha1: None,
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
            components: Vec::new(),
            is_missing_base: false,
        };

        // 1. Inherited from platform
        let choices = EditGameFormHelper::get_emulator_choices(&db, &game);
        assert_eq!(choices[0].display_label, "Heredado (Ryujinx — de plataforma)");

        // 2. Inherited from folder
        let folder_id = db.save_scan_folder(switch.id, "/fake/folder", true).unwrap();
        db.set_folder_assigned_emulator(folder_id, Some(citron.id)).unwrap();
        game.folder_id = Some(folder_id);

        let choices = EditGameFormHelper::get_emulator_choices(&db, &game);
        assert_eq!(choices[0].display_label, "Heredado (Citron — de carpeta)");
        assert_eq!(choices.len(), 3); // Heredado, Ryujinx, Citron

        // 3. Cycle choices
        let curr = EditGameFormHelper::cycle_choice(&choices, None, false);
        assert_eq!(curr, Some(ryujinx.id));
        let curr = EditGameFormHelper::cycle_choice(&choices, curr, false);
        assert_eq!(curr, Some(citron.id));
        let curr = EditGameFormHelper::cycle_choice(&choices, curr, false);
        assert_eq!(curr, None); // Back to inherited

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
