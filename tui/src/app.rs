use anyhow::Result;
use game_core::db::Database;
use game_core::models::{Game, Platform, PlatformType, Runner};
use game_core::scanner::Scanner;
use game_core::steam_scanner::SteamScanner;
use runner::GameRunner;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedPane {
    Platforms,
    Games,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalState {
    None,
    AddGameStep1Type {
        selected_type_idx: usize,
    },
    AddGameForm {
        game_type: PlatformType,
        selected_field: usize,
        title: String,
        platform_idx: usize,
        file_path: String,
        working_dir: String,
        wine_prefix: String,
        steam_appid: String,
        custom_command: String,
    },
    ManageRunnersStep1Platform {
        selected_platform_idx: usize,
    },
    ManageRunnersStep2Config {
        platform: Platform,
        runners: Vec<Runner>,
        selected_runner_idx: usize,
        exe_path_input: String,
    },
}

pub enum Action {
    NextPlatform,
    PrevPlatform,
    NextGame,
    PrevGame,
    TogglePane,
    ToggleShowAllPlatforms,
    LaunchGame,
    ScanCurrentFolder,
    ScanSteamGames,

    // File Picker Action
    OpenFilePicker,

    // Add Game Modal Actions
    OpenAddGameModal,
    CloseModal,
    ModalSelectNext,
    ModalSelectPrev,
    ModalConfirmStep1,
    ModalNextField,
    ModalPrevField,
    ModalInputChar(char),
    ModalBackspace,
    SaveModalGame,

    // Manage Runners Modal Actions
    OpenManageRunnersModal,
    RunnerModalConfirmPlatform,
    SaveRunnerConfig,
    ResetRunnerConfig,

    Quit,
    SetStatus(String),
}

pub struct App {
    pub db: Database,
    pub platforms: Vec<Platform>,
    pub selected_platform_idx: usize,
    pub games: Vec<Game>,
    pub selected_game_idx: usize,
    pub focused_pane: FocusedPane,
    pub modal_state: ModalState,
    pub show_all_platforms: bool,
    pub status_msg: String,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let db = Database::open_default()?;

        let steam_added = SteamScanner::scan_steam_games(&db).unwrap_or(0);

        let show_all_platforms = false;
        let platforms = db.get_active_platforms(show_all_platforms)?;

        let mut app = Self {
            db,
            platforms,
            selected_platform_idx: 0,
            games: Vec::new(),
            selected_game_idx: 0,
            focused_pane: FocusedPane::Platforms,
            modal_state: ModalState::None,
            show_all_platforms,
            status_msg: if steam_added > 0 {
                format!("Detectados {} juegos de Steam automáticamente!", steam_added)
            } else {
                "TUI Game Station listo! [m] Configurar Emuladores/Runners | [a] Agregar | [f] Seleccionar Archivo".to_string()
            },
            should_quit: false,
        };

        app.load_games_for_selected_platform();
        Ok(app)
    }

    pub fn load_platforms(&mut self) {
        if let Ok(platforms) = self.db.get_active_platforms(self.show_all_platforms) {
            self.platforms = platforms;
            if self.selected_platform_idx >= self.platforms.len() {
                self.selected_platform_idx = 0;
            }
            self.load_games_for_selected_platform();
        }
    }

    pub fn load_games_for_selected_platform(&mut self) {
        if self.platforms.is_empty() {
            self.games.clear();
            self.selected_game_idx = 0;
            return;
        }

        let p = &self.platforms[self.selected_platform_idx];
        if let Ok(games) = self.db.get_games_for_platform(p.id) {
            self.games = games;
            self.selected_game_idx = 0;
        } else {
            self.games.clear();
            self.selected_game_idx = 0;
        }
    }

    pub async fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                if self.modal_state != ModalState::None {
                    self.modal_state = ModalState::None;
                } else {
                    self.should_quit = true;
                }
            }
            Action::TogglePane => {
                if self.modal_state == ModalState::None {
                    self.focused_pane = match self.focused_pane {
                        FocusedPane::Platforms => FocusedPane::Games,
                        FocusedPane::Games => FocusedPane::Platforms,
                    };
                }
            }
            Action::ToggleShowAllPlatforms => {
                self.show_all_platforms = !self.show_all_platforms;
                self.load_platforms();
                self.status_msg = if self.show_all_platforms {
                    "Mostrando TODAS las plataformas preseteadas.".to_string()
                } else {
                    "Filtro activo: Mostrando solo plataformas instaladas/con juegos.".to_string()
                };
            }
            Action::NextPlatform => {
                if self.modal_state == ModalState::None && !self.platforms.is_empty() {
                    self.selected_platform_idx = (self.selected_platform_idx + 1) % self.platforms.len();
                    self.load_games_for_selected_platform();
                }
            }
            Action::PrevPlatform => {
                if self.modal_state == ModalState::None && !self.platforms.is_empty() {
                    if self.selected_platform_idx == 0 {
                        self.selected_platform_idx = self.platforms.len() - 1;
                    } else {
                        self.selected_platform_idx -= 1;
                    }
                    self.load_games_for_selected_platform();
                }
            }
            Action::NextGame => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    self.selected_game_idx = (self.selected_game_idx + 1) % self.games.len();
                }
            }
            Action::PrevGame => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    if self.selected_game_idx == 0 {
                        self.selected_game_idx = self.games.len() - 1;
                    } else {
                        self.selected_game_idx -= 1;
                    }
                }
            }
            Action::LaunchGame => {
                if self.games.is_empty() {
                    self.status_msg = "No hay juegos seleccionados para ejecutar.".to_string();
                    return;
                }

                let game = self.games[self.selected_game_idx].clone();
                let runner = self.db.get_runner_for_platform(game.platform_id).ok().flatten();

                self.status_msg = format!("Ejecutando {}...", game.title);

                match GameRunner::launch_game(&game, runner.as_ref()).await {
                    Ok(status) => {
                        self.status_msg = format!("Juego finalizado con código: {:?}", status.code());
                    }
                    Err(err) => {
                        self.status_msg = format!("Error al ejecutar juego: {}", err);
                    }
                }
            }
            Action::ScanCurrentFolder => {
                if self.platforms.is_empty() {
                    return;
                }
                let platform = self.platforms[self.selected_platform_idx].clone();

                let default_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home")).join("Juegos");
                self.status_msg = format!("Escaneando carpeta: {:?} para {}...", default_dir, platform.name);

                if default_dir.exists() {
                    match Scanner::scan_folder(&self.db, &platform, &default_dir, true, false) {
                        Ok(added) => {
                            self.status_msg = format!("Escaneo finalizado: {} juegos agregados/actualizados.", added);
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error durante el escaneo: {}", err);
                        }
                    }
                } else {
                    self.status_msg = format!("Carpeta no encontrada: {:?}. Crea la carpeta ~/Juegos", default_dir);
                }
            }
            Action::ScanSteamGames => {
                self.status_msg = "Buscando juegos de Steam instalados...".to_string();
                match SteamScanner::scan_steam_games(&self.db) {
                    Ok(added) => {
                        self.status_msg = format!("Escaneo de Steam completado: {} juegos en biblioteca.", added);
                        self.load_platforms();
                    }
                    Err(err) => {
                        self.status_msg = format!("Error detectando Steam: {}", err);
                    }
                }
            }

            // File Picker
            Action::OpenFilePicker => {
                if let Some(picked) = rfd::FileDialog::new().pick_file() {
                    let path_str = picked.to_string_lossy().to_string();
                    match self.modal_state {
                        ModalState::ManageRunnersStep2Config { ref mut exe_path_input, .. } => {
                            *exe_path_input = path_str.clone();
                            self.status_msg = format!("Archivo seleccionado: {}", path_str);
                        }
                        ModalState::AddGameForm {
                            ref mut file_path,
                            ref mut title,
                            selected_field,
                            game_type: ref gtype,
                            ..
                        } => {
                            let filename = picked.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                            if title.is_empty() && !filename.is_empty() {
                                *title = filename;
                            }

                            match gtype {
                                PlatformType::Emulator if selected_field == 2 => {
                                    *file_path = path_str.clone();
                                }
                                PlatformType::Native | PlatformType::Wine if selected_field == 1 => {
                                    *file_path = path_str.clone();
                                }
                                _ => {
                                    *file_path = path_str.clone();
                                }
                            }
                            self.status_msg = format!("Archivo de juego seleccionado: {}", path_str);
                        }
                        _ => {}
                    }
                }
            }

            // Runner Manager Actions
            Action::OpenManageRunnersModal => {
                self.modal_state = ModalState::ManageRunnersStep1Platform {
                    selected_platform_idx: 0,
                };
            }
            Action::RunnerModalConfirmPlatform => {
                if let ModalState::ManageRunnersStep1Platform { selected_platform_idx } = self.modal_state {
                    let all_platforms = self.db.get_platforms().unwrap_or_default();
                    if let Some(p) = all_platforms.get(selected_platform_idx) {
                        let runners = self.db.get_runners_for_platform(p.id).unwrap_or_default();
                        let default_exe = runners.first().and_then(|r| r.executable_path.clone()).unwrap_or_default();

                        self.modal_state = ModalState::ManageRunnersStep2Config {
                            platform: p.clone(),
                            runners,
                            selected_runner_idx: 0,
                            exe_path_input: default_exe,
                        };
                    }
                }
            }
            Action::SaveRunnerConfig => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runners,
                    selected_runner_idx,
                    ref exe_path_input,
                    ..
                } = self.modal_state.clone()
                {
                    let trimmed_path = exe_path_input.trim();
                    if trimmed_path.is_empty() {
                        self.status_msg = "❌ Error: Debes ingresar o seleccionar la ruta del ejecutable / .AppImage.".to_string();
                        return;
                    }

                    if !Path::new(trimmed_path).exists() {
                        self.status_msg = format!("❌ Error: El archivo no existe en el sistema: '{}'. Selecciona un ejecutable existente.", trimmed_path);
                        return;
                    }

                    if let Some(runner) = runners.get(selected_runner_idx) {
                        match self.db.update_runner_config(runner.id, trimmed_path, true) {
                            Ok(_) => {
                                self.status_msg = format!("✅ Runner '{}' configurado correctamente. Plataforma activada!", runner.name);
                                self.modal_state = ModalState::None;
                                self.load_platforms();
                            }
                            Err(err) => {
                                self.status_msg = format!("Error guardando runner: {}", err);
                            }
                        }
                    }
                }
            }
            Action::ResetRunnerConfig => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runners,
                    selected_runner_idx,
                    ..
                } = self.modal_state.clone()
                {
                    if let Some(runner) = runners.get(selected_runner_idx) {
                        match self.db.reset_runner_config(runner.id) {
                            Ok(_) => {
                                self.status_msg = format!("Runner '{}' desactivado correctamente.", runner.name);
                                self.modal_state = ModalState::None;
                                self.load_platforms();
                            }
                            Err(err) => {
                                self.status_msg = format!("Error desactivando runner: {}", err);
                            }
                        }
                    }
                }
            }

            // Add Game Modal Actions
            Action::OpenAddGameModal => {
                self.modal_state = ModalState::AddGameStep1Type {
                    selected_type_idx: 0,
                };
            }
            Action::CloseModal => {
                self.modal_state = ModalState::None;
            }
            Action::ModalSelectNext => {
                match self.modal_state {
                    ModalState::AddGameStep1Type { ref mut selected_type_idx } => {
                        *selected_type_idx = (*selected_type_idx + 1) % 4;
                    }
                    ModalState::AddGameForm { game_type: PlatformType::Emulator, ref mut platform_idx, .. } => {
                        if !self.platforms.is_empty() {
                            *platform_idx = (*platform_idx + 1) % self.platforms.len();
                        }
                    }
                    ModalState::ManageRunnersStep1Platform { ref mut selected_platform_idx } => {
                        let total = self.db.get_platforms().unwrap_or_default().len();
                        if total > 0 {
                            *selected_platform_idx = (*selected_platform_idx + 1) % total;
                        }
                    }
                    ModalState::ManageRunnersStep2Config { ref runners, ref mut selected_runner_idx, ref mut exe_path_input, .. } => {
                        if !runners.is_empty() {
                            *selected_runner_idx = (*selected_runner_idx + 1) % runners.len();
                            if let Some(r) = runners.get(*selected_runner_idx) {
                                *exe_path_input = r.executable_path.clone().unwrap_or_default();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::ModalSelectPrev => {
                match self.modal_state {
                    ModalState::AddGameStep1Type { ref mut selected_type_idx } => {
                        if *selected_type_idx == 0 {
                            *selected_type_idx = 3;
                        } else {
                            *selected_type_idx -= 1;
                        }
                    }
                    ModalState::AddGameForm { game_type: PlatformType::Emulator, ref mut platform_idx, .. } => {
                        if !self.platforms.is_empty() {
                            if *platform_idx == 0 {
                                *platform_idx = self.platforms.len() - 1;
                            } else {
                                *platform_idx -= 1;
                            }
                        }
                    }
                    ModalState::ManageRunnersStep1Platform { ref mut selected_platform_idx } => {
                        let total = self.db.get_platforms().unwrap_or_default().len();
                        if total > 0 {
                            if *selected_platform_idx == 0 {
                                *selected_platform_idx = total - 1;
                            } else {
                                *selected_platform_idx -= 1;
                            }
                        }
                    }
                    ModalState::ManageRunnersStep2Config { ref runners, ref mut selected_runner_idx, ref mut exe_path_input, .. } => {
                        if !runners.is_empty() {
                            if *selected_runner_idx == 0 {
                                *selected_runner_idx = runners.len() - 1;
                            } else {
                                *selected_runner_idx -= 1;
                            }
                            if let Some(r) = runners.get(*selected_runner_idx) {
                                *exe_path_input = r.executable_path.clone().unwrap_or_default();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Action::ModalConfirmStep1 => {
                if let ModalState::AddGameStep1Type { selected_type_idx } = self.modal_state {
                    let game_type = match selected_type_idx {
                        0 => PlatformType::Emulator,
                        1 => PlatformType::Native,
                        2 => PlatformType::Wine,
                        _ => PlatformType::Steam,
                    };

                    self.modal_state = ModalState::AddGameForm {
                        game_type,
                        selected_field: 0,
                        title: String::new(),
                        platform_idx: self.selected_platform_idx,
                        file_path: String::new(),
                        working_dir: String::new(),
                        wine_prefix: String::new(),
                        steam_appid: String::new(),
                        custom_command: String::new(),
                    };
                }
            }
            Action::ModalNextField => {
                if let ModalState::AddGameForm { game_type: ref gtype, ref mut selected_field, .. } = self.modal_state {
                    let total_fields = match gtype {
                        PlatformType::Emulator => 4,
                        PlatformType::Native => 5,
                        PlatformType::Wine => 5,
                        PlatformType::Steam => 3,
                    };
                    *selected_field = (*selected_field + 1) % total_fields;
                }
            }
            Action::ModalPrevField => {
                if let ModalState::AddGameForm { game_type: ref gtype, ref mut selected_field, .. } = self.modal_state {
                    let total_fields = match gtype {
                        PlatformType::Emulator => 4,
                        PlatformType::Native => 5,
                        PlatformType::Wine => 5,
                        PlatformType::Steam => 3,
                    };
                    if *selected_field == 0 {
                        *selected_field = total_fields - 1;
                    } else {
                        *selected_field -= 1;
                    }
                }
            }
            Action::ModalInputChar(ch) => {
                if let ModalState::AddGameForm {
                    ref mut title,
                    ref mut file_path,
                    ref mut working_dir,
                    ref mut wine_prefix,
                    ref mut steam_appid,
                    ref mut custom_command,
                    selected_field,
                    game_type: ref gtype,
                    ..
                } = self.modal_state
                {
                    match gtype {
                        PlatformType::Emulator => match selected_field {
                            0 => title.push(ch),
                            2 => file_path.push(ch),
                            _ => {}
                        },
                        PlatformType::Native => match selected_field {
                            0 => title.push(ch),
                            1 => file_path.push(ch),
                            2 => working_dir.push(ch),
                            3 => custom_command.push(ch),
                            _ => {}
                        },
                        PlatformType::Wine => match selected_field {
                            0 => title.push(ch),
                            1 => file_path.push(ch),
                            2 => wine_prefix.push(ch),
                            3 => working_dir.push(ch),
                            _ => {}
                        },
                        PlatformType::Steam => match selected_field {
                            0 => title.push(ch),
                            1 => {
                                if ch.is_ascii_digit() {
                                    steam_appid.push(ch);
                                }
                            }
                            _ => {}
                        },
                    }
                } else if let ModalState::ManageRunnersStep2Config { ref mut exe_path_input, .. } = self.modal_state {
                    exe_path_input.push(ch);
                }
            }
            Action::ModalBackspace => {
                if let ModalState::AddGameForm {
                    ref mut title,
                    ref mut file_path,
                    ref mut working_dir,
                    ref mut wine_prefix,
                    ref mut steam_appid,
                    ref mut custom_command,
                    selected_field,
                    game_type: ref gtype,
                    ..
                } = self.modal_state
                {
                    let target_str = match gtype {
                        PlatformType::Emulator => match selected_field {
                            0 => Some(title),
                            2 => Some(file_path),
                            _ => None,
                        },
                        PlatformType::Native => match selected_field {
                            0 => Some(title),
                            1 => Some(file_path),
                            2 => Some(working_dir),
                            3 => Some(custom_command),
                            _ => None,
                        },
                        PlatformType::Wine => match selected_field {
                            0 => Some(title),
                            1 => Some(file_path),
                            2 => Some(wine_prefix),
                            3 => Some(working_dir),
                            _ => None,
                        },
                        PlatformType::Steam => match selected_field {
                            0 => Some(title),
                            1 => Some(steam_appid),
                            2 => None,
                            _ => None,
                        },
                    };

                    if let Some(s) = target_str {
                        s.pop();
                    }
                } else if let ModalState::ManageRunnersStep2Config { ref mut exe_path_input, .. } = self.modal_state {
                    exe_path_input.pop();
                }
            }
            Action::SaveModalGame => {
                if let ModalState::AddGameForm {
                    ref game_type,
                    ref title,
                    platform_idx,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref steam_appid,
                    ref custom_command,
                    ..
                } = self.modal_state.clone()
                {
                    if title.trim().is_empty() {
                        self.status_msg = "El título del juego no puede estar vacío.".to_string();
                        return;
                    }

                    let platform_id = match game_type {
                        PlatformType::Emulator => {
                            if platform_idx < self.platforms.len() {
                                self.platforms[platform_idx].id
                            } else {
                                self.platforms[0].id
                            }
                        }
                        PlatformType::Native => {
                            self.platforms.iter().find(|p| p.slug == "linux").map(|p| p.id).unwrap_or(1)
                        }
                        PlatformType::Wine => {
                            self.platforms.iter().find(|p| p.slug == "windows").map(|p| p.id).unwrap_or(1)
                        }
                        PlatformType::Steam => {
                            self.platforms.iter().find(|p| p.slug == "steam").map(|p| p.id).unwrap_or(1)
                        }
                    };

                    let steam_id = steam_appid.parse::<i64>().ok();

                    let game = Game {
                        id: 0,
                        platform_id,
                        title: title.clone(),
                        sort_title: None,
                        game_type: game_type.to_string(),
                        file_path: if file_path.is_empty() { None } else { Some(file_path.clone()) },
                        working_dir: if working_dir.is_empty() { None } else { Some(working_dir.clone()) },
                        custom_command: if custom_command.is_empty() { None } else { Some(custom_command.clone()) },
                        env_vars: None,
                        wine_prefix: if wine_prefix.is_empty() { None } else { Some(wine_prefix.clone()) },
                        wine_runner_id: None,
                        steam_appid: steam_id,
                        file_name: file_path.split('/').last().map(|s| s.to_string()),
                        file_extension: file_path.split('.').last().map(|s| format!(".{}", s)),
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
                    };

                    match self.db.insert_game(&game) {
                        Ok(_) => {
                            self.status_msg = format!("Juego '{}' guardado correctamente.", title);
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error al guardar juego: {}", err);
                        }
                    }
                }
            }
            Action::SetStatus(msg) => {
                self.status_msg = msg;
            }
        }
    }
}
