use anyhow::Result;
use game_core::db::Database;
use game_core::models::{Game, Platform, PlatformType};
use game_core::scanner::Scanner;
use game_core::steam_scanner::SteamScanner;
use ratatui_image::protocol::StatefulProtocol;
use runner::GameRunner;
use scraper::downloader::{DownloadEvent, RunnerDownloader};
use scraper::steam_cover::SteamCoverResolver;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

use crate::cover_renderer::CoverManager;

pub struct LoadedCoverEvent {
    pub game_id: i64,
    pub media_type: String,
    pub protocol: StatefulProtocol,
}

pub struct LoadedPreviewEvent {
    pub url: String,
    pub protocol: StatefulProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedPane {
    Platforms,
    Games,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    CoverCard,
    BannerCard,
    IconCard,
    Table,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadProgressState {
    pub runner_id: i64,
    pub runner_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percentage: f64,
    pub is_finished: bool,
    pub error_msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalState {
    None,
    AddGameStep1Type {
        selected_type_idx: usize,
    },
    ScanFolderStep1Platform {
        selected_platform_idx: usize,
    },
    ScanFolderForm {
        platform: Platform,
        folder_path: String,
        extensions_input: String,
        recursive: bool,
        selected_field: usize,
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
    EditGameForm {
        game_id: i64,
        game_type: PlatformType,
        selected_field: usize,
        title: String,
        file_path: String,
        working_dir: String,
        wine_prefix: String,
        steam_appid: String,
        custom_command: String,
    },
    ConfigureApiKeyInput {
        input: String,
    },
    AppSettings {
        api_key_input: String,
        selected_field: usize,
    },
    VisualMediaSelector {
        game_id: i64,
        game_title: String,
        search_query: String,
        active_tab: usize, // 0: Candidates, 1: Covers, 2: Banners, 3: Icons
        is_searching: bool,
        candidates: Vec<scraper::steamgriddb::SteamGridSearchResult>,
        selected_candidate_idx: usize,
        selected_candidate_id: Option<i64>,
        covers: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_cover_idx: usize,
        banners: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_banner_idx: usize,
        icons: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_icon_idx: usize,
    },
    ManageRunnersStep1Platform {
        selected_runner_idx: usize,
    },
    ManageRunnersStep2Config {
        runner_info: game_core::models::UniqueRunnerInfo,
        exe_path_input: String,
    },
}

pub enum Action {
    NextPlatform,
    PrevPlatform,
    NextGame,
    PrevGame,
    TogglePane,
    ToggleViewMode,
    ToggleShowAllPlatforms,
    LaunchGame,
    ScanCurrentFolder,
    ScanSteamGames,

    // File / Folder Picker Actions
    OpenFilePicker,
    OpenFolderPicker,

    // Add Game & Scan Modal Actions
    OpenAddGameModal,
    OpenEditGameModal,
    SaveEditGameModal,
    CloseModal,
    ModalSelectNext,
    ModalSelectPrev,
    ModalConfirmStep1,
    ScanModalConfirmPlatform,
    ModalNextField,
    ModalPrevField,
    ModalInputChar(char),
    ModalBackspace,
    ModalToggleCheckbox,
    SaveModalGame,
    StartFolderScan,
    QuickRescanPlatform,
    ToggleSelectGame,
    DeleteSelectedGames,
    FetchGameMedia,
    SaveApiKey,
    OpenSettingsModal,
    SaveAppSettings,
    OpenVisualMediaModal,
    SearchVisualMedia,
    SelectVisualMediaCandidate,
    SwitchVisualMediaTab,
    SetVisualMediaTab(usize),
    ApplyVisualMediaSelection,

    // Manage Runners Modal Actions
    OpenManageRunnersModal,
    RunnerModalConfirmPlatform,
    SaveRunnerConfig,
    ResetRunnerConfig,
    StartRunnerDownload,
    UpdateDownloadProgress(DownloadEvent),
    DeleteRunnerDownload,

    Quit,
    SetStatus(String),
}

pub struct App {
    pub db: Database,
    pub platforms: Vec<Platform>,
    pub selected_platform_idx: usize,
    pub games: Vec<Game>,
    pub selected_game_idx: usize,
    pub selected_game_ids: HashSet<i64>,
    pub focused_pane: FocusedPane,
    pub view_mode: ViewMode,
    pub modal_state: ModalState,
    pub download_progress: Option<DownloadProgressState>,
    pub download_rx: Option<mpsc::Receiver<DownloadEvent>>,
    pub cover_tx: mpsc::Sender<LoadedCoverEvent>,
    pub cover_rx: mpsc::Receiver<LoadedCoverEvent>,
    pub preview_tx: mpsc::Sender<LoadedPreviewEvent>,
    pub preview_rx: mpsc::Receiver<LoadedPreviewEvent>,
    pub cover_manager: CoverManager,
    pub pending_cover_requests: HashSet<i64>,
    pub media_protocols: HashMap<(i64, String), StatefulProtocol>,
    pub visual_preview_protocol: Option<StatefulProtocol>,
    pub visual_preview_url: Option<String>,
    pub visual_preview_loading: bool,
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
        let (cover_tx, cover_rx) = mpsc::channel::<LoadedCoverEvent>(50);
        let (preview_tx, preview_rx) = mpsc::channel::<LoadedPreviewEvent>(50);
        let cover_manager = CoverManager::new();

        let mut app = Self {
            db,
            platforms,
            selected_platform_idx: 0,
            games: Vec::new(),
            selected_game_idx: 0,
            selected_game_ids: HashSet::new(),
            focused_pane: FocusedPane::Platforms,
            view_mode: ViewMode::CoverCard,
            modal_state: ModalState::None,
            download_progress: None,
            download_rx: None,
            cover_tx,
            cover_rx,
            preview_tx,
            preview_rx,
            cover_manager,
            pending_cover_requests: HashSet::new(),
            media_protocols: HashMap::new(),
            visual_preview_protocol: None,
            visual_preview_url: None,
            visual_preview_loading: false,
            show_all_platforms,
            status_msg: if steam_added > 0 {
                format!(
                    "Detectados {} juegos de Steam automáticamente!",
                    steam_added
                )
            } else {
                "TUI Game Station listo! [v] Cambiar Vista | [m] Configurar Emuladores | [a] Escanear/Agregar ROMs".to_string()
            },
            should_quit: false,
        };

        app.load_games_for_selected_platform();
        Ok(app)
    }

    /// Platforms list for Runner Manager (excludes Linux Native & Steam)
    pub fn get_runner_platforms(&self) -> Vec<Platform> {
        self.db
            .get_platforms()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.slug != "linux" && p.slug != "steam")
            .collect()
    }

    /// Platforms list for Scan ROMs Folder (ONLY configured emulators with executable path!)
    pub fn get_configured_emulator_platforms(&self) -> Vec<Platform> {
        self.db
            .get_platforms()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| {
                p.platform_type == PlatformType::Emulator && {
                    let runner = self.db.get_runner_for_platform(p.id).ok().flatten();
                    runner.and_then(|r| r.executable_path).is_some()
                }
            })
            .collect()
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

        self.trigger_async_cover_fetch();
    }

    pub fn trigger_async_cover_fetch(&mut self) {
        if self.games.is_empty() || self.selected_game_idx >= self.games.len() {
            return;
        }

        let game = &self.games[self.selected_game_idx];
        let game_id = game.id;
        let title = game.title.clone();
        let appid = game.steam_appid;

        let (media_type, media_sub_dir) = match self.view_mode {
            ViewMode::CoverCard => ("cover", "covers"),
            ViewMode::BannerCard => ("banner", "banners"),
            ViewMode::IconCard => ("icon", "icons"),
            ViewMode::Table => ("cover", "covers"),
        };

        if self.pending_cover_requests.contains(&game_id) {
            return;
        }

        let cover_status = self.db.get_media_status(game_id, media_type).ok().flatten();
        if cover_status.as_deref() == Some("not_found") {
            return;
        }

        self.pending_cover_requests.insert(game_id);
        let tx = self.cover_tx.clone();
        let manager = self.cover_manager.clone();
        let db_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
        let media_type_str = media_type.to_string();
        let sub_dir_str = media_sub_dir.to_string();

        tokio::spawn(async move {
            let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();
            let target_dir = media_dir.join(&sub_dir_str);
            let local_cover = vec![
                target_dir.join(format!("{}.jpg", game_id)),
                target_dir.join(format!("{}.png", game_id)),
                target_dir.join(format!("{}.webp", game_id)),
            ]
            .into_iter()
            .find(|p| p.exists());

            let cover_path = if let Some(path) = local_cover {
                Some(path)
            } else if media_type_str == "cover" && appid.is_some() {
                SteamCoverResolver::resolve_cover(appid.unwrap()).await
            } else {
                let client = scraper::steamgriddb::SteamGridDBClient::new(db_key);
                let db_path = dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                    .join("tui_game_station")
                    .join("game_station.db");
                if let Ok(res) = client.download_all_media_for_game(Some(db_path), game_id, &title, false).await {
                    match media_type_str.as_str() {
                        "banner" => res.banner_path,
                        "icon" => res.icon_path,
                        _ => res.cover_path,
                    }
                } else {
                    None
                }
            };

            if let Some(path) = cover_path {
                if let Some(protocol) = manager.load_protocol_from_file(&path) {
                    let _ = tx.send(LoadedCoverEvent { game_id, media_type: media_type_str, protocol }).await;
                }
            }
        });
    }

    pub async fn check_download_events(&mut self) {
        let mut events = Vec::new();
        if let Some(ref mut rx) = self.download_rx {
            while let Ok(evt) = rx.try_recv() {
                events.push(evt);
            }
        }
        for evt in events {
            self.update(Action::UpdateDownloadProgress(evt)).await;
        }

        // Receive loaded cover events from background task non-blocking
        while let Ok(loaded) = self.cover_rx.try_recv() {
            self.pending_cover_requests.remove(&loaded.game_id);
            self.media_protocols.insert((loaded.game_id, loaded.media_type), loaded.protocol);
        }

        // Receive loaded preview events for Visual Media Selector
        while let Ok(loaded) = self.preview_rx.try_recv() {
            if self.visual_preview_url.as_deref() == Some(&loaded.url) {
                self.visual_preview_protocol = Some(loaded.protocol);
                self.visual_preview_loading = false;
            }
        }
    }

    pub fn update_visual_media_preview(&mut self) {
        if let ModalState::VisualMediaSelector {
            active_tab,
            ref covers,
            selected_cover_idx,
            ref banners,
            selected_banner_idx,
            ref icons,
            selected_icon_idx,
            ..
        } = self.modal_state
        {
            let target_url = match active_tab {
                1 => covers.get(selected_cover_idx).map(|c| c.thumb.as_ref().unwrap_or(&c.url).clone()),
                2 => banners.get(selected_banner_idx).map(|b| b.thumb.as_ref().unwrap_or(&b.url).clone()),
                3 => icons.get(selected_icon_idx).map(|i| i.thumb.as_ref().unwrap_or(&i.url).clone()),
                _ => None,
            };

            if let Some(url) = target_url {
                if self.visual_preview_url.as_deref() == Some(&url) {
                    return;
                }

                self.visual_preview_url = Some(url.clone());
                self.visual_preview_loading = true;
                self.visual_preview_protocol = None;

                let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();
                let cache_dir = media_dir.join("preview_cache");
                let _ = std::fs::create_dir_all(&cache_dir);

                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&url, &mut hasher);
                let url_hash = format!("{:x}", std::hash::Hasher::finish(&hasher));
                let cache_path = cache_dir.join(format!("{}.jpg", url_hash));

                if cache_path.exists() {
                    if let Some(protocol) = self.cover_manager.load_protocol_from_file(&cache_path) {
                        self.visual_preview_protocol = Some(protocol);
                        self.visual_preview_loading = false;
                        return;
                    }
                }

                let key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                let tx = self.preview_tx.clone();
                let manager = self.cover_manager.clone();
                let url_to_fetch = url.clone();
                tokio::spawn(async move {
                    if client.download_file_to_path(&url_to_fetch, &cache_path).await.is_ok() {
                        if let Some(protocol) = manager.load_protocol_from_file(&cache_path) {
                            let _ = tx.send(LoadedPreviewEvent { url: url_to_fetch, protocol }).await;
                        }
                    }
                });
            } else {
                self.visual_preview_url = None;
                self.visual_preview_loading = false;
                self.visual_preview_protocol = None;
            }
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
            Action::ToggleViewMode => {
                self.view_mode = match self.view_mode {
                    ViewMode::CoverCard => ViewMode::BannerCard,
                    ViewMode::BannerCard => ViewMode::IconCard,
                    ViewMode::IconCard => ViewMode::Table,
                    ViewMode::Table => ViewMode::CoverCard,
                };
                self.status_msg = match self.view_mode {
                    ViewMode::CoverCard => "Vista: TARJETAS (Covers Poster)".to_string(),
                    ViewMode::BannerCard => "Vista: HERO BANNERS".to_string(),
                    ViewMode::IconCard => "Vista: ICONOS".to_string(),
                    ViewMode::Table => "Vista: TABLA DETALLADA".to_string(),
                };
                self.trigger_async_cover_fetch();
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
                    self.selected_platform_idx =
                        (self.selected_platform_idx + 1) % self.platforms.len();
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
                    self.trigger_async_cover_fetch();
                }
            }
            Action::PrevGame => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    if self.selected_game_idx == 0 {
                        self.selected_game_idx = self.games.len() - 1;
                    } else {
                        self.selected_game_idx -= 1;
                    }
                    self.trigger_async_cover_fetch();
                }
            }
            Action::LaunchGame => {
                if self.games.is_empty() {
                    self.status_msg = "No hay juegos seleccionados para ejecutar.".to_string();
                    return;
                }

                let game = self.games[self.selected_game_idx].clone();
                let runner = self
                    .db
                    .get_runner_for_platform(game.platform_id)
                    .ok()
                    .flatten();

                self.status_msg = format!("Ejecutando {}...", game.title);

                match GameRunner::launch_game(&game, runner.as_ref()).await {
                    Ok(status) => {
                        self.status_msg =
                            format!("Juego finalizado con código: {:?}", status.code());
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

                let default_dir = dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/home"))
                    .join("Juegos");
                self.status_msg = format!(
                    "Escaneando carpeta: {:?} para {}...",
                    default_dir, platform.name
                );

                if default_dir.exists() {
                    match Scanner::scan_folder(&self.db, &platform, &default_dir, true, false) {
                        Ok(added) => {
                            self.status_msg = format!(
                                "Escaneo finalizado: {} juegos agregados/actualizados.",
                                added
                            );
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error durante el escaneo: {}", err);
                        }
                    }
                } else {
                    self.status_msg = format!(
                        "Carpeta no encontrada: {:?}. Crea la carpeta ~/Juegos",
                        default_dir
                    );
                }
            }
            Action::ScanSteamGames => {
                self.status_msg = "Buscando juegos de Steam instalados...".to_string();
                match SteamScanner::scan_steam_games(&self.db) {
                    Ok(added) => {
                        self.status_msg = format!(
                            "Escaneo de Steam completado: {} juegos en biblioteca.",
                            added
                        );
                        self.load_platforms();
                    }
                    Err(err) => {
                        self.status_msg = format!("Error detectando Steam: {}", err);
                    }
                }
            }

            // File & Folder Pickers
            Action::OpenFolderPicker => {
                if let Some(picked) = rfd::FileDialog::new().pick_folder() {
                    let path_str = picked.to_string_lossy().to_string();
                    if let ModalState::ScanFolderForm {
                        ref mut folder_path,
                        ..
                    } = self.modal_state
                    {
                        *folder_path = path_str.clone();
                        self.status_msg = format!("Folder selected: {}", path_str);
                    }
                }
            }
            Action::OpenFilePicker => {
                if let Some(picked) = rfd::FileDialog::new().pick_file() {
                    let path_str = picked.to_string_lossy().to_string();
                    match self.modal_state {
                        ModalState::ManageRunnersStep2Config {
                            ref mut exe_path_input,
                            ..
                        } => {
                            *exe_path_input = path_str.clone();
                            self.status_msg = format!("File selected: {}", path_str);
                        }
                        ModalState::ScanFolderForm {
                            ref mut folder_path,
                            ..
                        } => {
                            *folder_path = path_str.clone();
                            self.status_msg = format!("Folder path set: {}", path_str);
                        }
                        ModalState::AddGameForm {
                            ref mut file_path,
                            ref mut title,
                            selected_field,
                            game_type: ref gtype,
                            ..
                        } => {
                            let filename = picked
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            if title.is_empty() && !filename.is_empty() {
                                *title = filename;
                            }

                            match gtype {
                                PlatformType::Emulator if selected_field == 2 => {
                                    *file_path = path_str.clone();
                                }
                                PlatformType::Native | PlatformType::Wine
                                    if selected_field == 1 =>
                                {
                                    *file_path = path_str.clone();
                                }
                                _ => {
                                    *file_path = path_str.clone();
                                }
                            }
                            self.status_msg = format!("File selected: {}", path_str);
                        }
                        _ => {}
                    }
                }
            }

            // Runner Manager Actions
            Action::OpenManageRunnersModal => {
                self.modal_state = ModalState::ManageRunnersStep1Platform {
                    selected_runner_idx: 0,
                };
            }
            Action::RunnerModalConfirmPlatform => {
                if let ModalState::ManageRunnersStep1Platform {
                    selected_runner_idx,
                } = self.modal_state
                {
                    let unique_runners = self.db.get_unique_runners().unwrap_or_default();
                    if let Some(r) = unique_runners.get(selected_runner_idx) {
                        let exe_path = r.executable_path.clone().unwrap_or_default();
                        self.modal_state = ModalState::ManageRunnersStep2Config {
                            runner_info: r.clone(),
                            exe_path_input: exe_path,
                        };
                    }
                }
            }
            Action::SaveRunnerConfig => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ref exe_path_input,
                    ..
                } = self.modal_state.clone()
                {
                    let trimmed_path = exe_path_input.trim();
                    if trimmed_path.is_empty() {
                        self.status_msg =
                            "Error: Executable / .AppImage path cannot be empty.".to_string();
                        return;
                    }

                    if !Path::new(trimmed_path).exists() {
                        self.status_msg = format!("Error: File does not exist on system: '{}'. Select a valid executable.", trimmed_path);
                        return;
                    }

                    match self.db.update_runner_by_name(&runner_info.name, trimmed_path) {
                        Ok(_) => {
                            self.status_msg = format!(
                                "[OK] Emulator '{}' ({}) configured successfully!",
                                runner_info.name, runner_info.console_initials
                            );
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error saving runner: {}", err);
                        }
                    }
                }
            }
            Action::ResetRunnerConfig => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ..
                } = self.modal_state.clone()
                {
                    match self.db.reset_runner_by_name(&runner_info.name) {
                        Ok(_) => {
                            self.status_msg = format!(
                                "Emulator '{}' ({}) deactivated successfully.",
                                runner_info.name, runner_info.console_initials
                            );
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error deactivating runner: {}", err);
                        }
                    }
                }
            }
            Action::DeleteRunnerDownload => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ..
                } = self.modal_state.clone()
                {
                    if let Some(exe_path) = &runner_info.executable_path {
                        let path = PathBuf::from(exe_path);
                        if path.exists() {
                            let _ = std::fs::remove_file(&path);
                        }
                        let _ = self.db.reset_runner_by_name(&runner_info.name);
                        self.status_msg = format!(
                            "[Deleted] Emulator '{}' executable deleted from disk and deactivated.",
                            runner_info.name
                        );
                        self.modal_state = ModalState::None;
                        self.load_platforms();
                    }
                }
            }
            Action::StartRunnerDownload => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ..
                } = self.modal_state.clone()
                {
                    let download_url = match &runner_info.download_url {
                        Some(url) if !url.is_empty() => url.clone(),
                        _ => {
                            self.status_msg = format!(
                                "[Error] No official download URL configured for '{}'.",
                                runner_info.name
                            );
                            return;
                        }
                    };

                    let download_filename = runner_info
                        .download_filename
                        .clone()
                        .unwrap_or_else(|| format!("{}.AppImage", runner_info.name.to_lowercase()));

                    let target_dir = match RunnerDownloader::get_runner_dir("emulators") {
                        Ok(d) => d,
                        Err(e) => {
                            self.status_msg =
                                format!("Error creating download directory: {}", e);
                            return;
                        }
                    };

                    let dest_path = target_dir.join(&download_filename);
                    let is_melonds_archive = runner_info.name == "melonDS"
                        && download_filename
                            .eq_ignore_ascii_case("melonDS-1.1-appimage-x86_64.zip");
                    let executable_path = if is_melonds_archive {
                        target_dir.join("melonDS-x86_64.AppImage")
                    } else {
                        dest_path.clone()
                    };
                    let executable_path_str = executable_path.to_string_lossy().to_string();

                    self.download_progress = Some(DownloadProgressState {
                        runner_id: 0,
                        runner_name: runner_info.name.clone(),
                        downloaded_bytes: 0,
                        total_bytes: 0,
                        percentage: 0.0,
                        is_finished: false,
                        error_msg: None,
                    });

                    self.status_msg = format!("Downloading {}...", runner_info.name);

                    let (tx, rx) = mpsc::channel::<DownloadEvent>(100);
                    self.download_rx = Some(rx);

                    let runner_name = runner_info.name.clone();
                    let db_path = dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                        .join("tui_game_station")
                        .join("game_station.db");

                    tokio::spawn(async move {
                        let result = if is_melonds_archive {
                            RunnerDownloader::download_zip_appimage_with_progress(
                                &download_url,
                                &dest_path,
                                &executable_path,
                                "melonDS-x86_64.AppImage",
                                tx,
                            )
                            .await
                        } else {
                            RunnerDownloader::download_with_progress(
                                &download_url,
                                &dest_path,
                                tx,
                            )
                            .await
                        };

                        if result.is_ok() {
                            if let Ok(db) = Database::open(&db_path) {
                                let _ = db.update_runner_by_name(&runner_name, &executable_path_str);
                            }
                        }
                    });
                }
            }
            Action::UpdateDownloadProgress(event) => {
                if let Some(ref mut progress) = self.download_progress {
                    progress.downloaded_bytes = event.downloaded;
                    progress.total_bytes = event.total;
                    progress.percentage = event.percentage;
                    progress.is_finished = event.finished;
                    progress.error_msg = event.error.clone();

                    if event.finished {
                        let name = progress.runner_name.clone();
                        self.download_progress = None;
                        self.download_rx = None;

                        if let Some(err) = event.error {
                            self.status_msg = format!("[Error] Download failed: {}", err);
                        } else {
                            self.status_msg =
                                format!("[OK] Download of '{}' completed successfully!", name);
                            let sel = self.selected_game_idx;
                            self.load_platforms();
                            if sel < self.games.len() {
                                self.selected_game_idx = sel;
                            }
                            self.trigger_async_cover_fetch();

                            if let ModalState::ManageRunnersStep2Config {
                                ref mut runner_info,
                                ref mut exe_path_input,
                            } = self.modal_state
                            {
                                if runner_info.name == name {
                                    if let Ok(unique_runners) = self.db.get_unique_runners() {
                                        if let Some(updated) = unique_runners.into_iter().find(|r| r.name == name) {
                                            *exe_path_input = updated.executable_path.clone().unwrap_or_default();
                                            *runner_info = updated;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Add / Edit Game & Scan Modal Actions
            Action::OpenAddGameModal => {
                self.modal_state = ModalState::AddGameStep1Type {
                    selected_type_idx: 0,
                };
            }
            Action::OpenEditGameModal => {
                if self.modal_state == ModalState::None && !self.games.is_empty() && self.selected_game_idx < self.games.len() {
                    let game = &self.games[self.selected_game_idx];
                    let gtype = PlatformType::from(game.game_type.as_str());

                    self.modal_state = ModalState::EditGameForm {
                        game_id: game.id,
                        game_type: gtype,
                        selected_field: 0,
                        title: game.title.clone(),
                        file_path: game.file_path.clone().unwrap_or_default(),
                        working_dir: game.working_dir.clone().unwrap_or_default(),
                        custom_command: game.custom_command.clone().unwrap_or_default(),
                        wine_prefix: game.wine_prefix.clone().unwrap_or_default(),
                        steam_appid: game.steam_appid.map(|id| id.to_string()).unwrap_or_default(),
                    };
                }
            }
            Action::SaveEditGameModal => {
                if let ModalState::EditGameForm {
                    game_id,
                    ref title,
                    ref file_path,
                    ref working_dir,
                    ref custom_command,
                    ref wine_prefix,
                    ref steam_appid,
                    ..
                } = self.modal_state.clone()
                {
                    if title.trim().is_empty() {
                        self.status_msg = "Error: Game title cannot be empty.".to_string();
                        return;
                    }

                    if let Some(pos) = self.games.iter().position(|g| g.id == game_id) {
                        let mut game = self.games[pos].clone();
                        game.title = title.trim().to_string();
                        game.file_path = if file_path.trim().is_empty() { None } else { Some(file_path.trim().to_string()) };
                        game.working_dir = if working_dir.trim().is_empty() { None } else { Some(working_dir.trim().to_string()) };
                        game.custom_command = if custom_command.trim().is_empty() { None } else { Some(custom_command.trim().to_string()) };
                        game.wine_prefix = if wine_prefix.trim().is_empty() { None } else { Some(wine_prefix.trim().to_string()) };
                        game.steam_appid = steam_appid.trim().parse::<i64>().ok();

                        if self.db.update_game(&game).is_ok() {
                            self.status_msg = format!("[OK] Updated details for '{}'!", game.title);
                            self.modal_state = ModalState::None;
                            let sel = self.selected_game_idx;
                            self.load_platforms();
                            if sel < self.games.len() {
                                self.selected_game_idx = sel;
                            }
                        }
                    }
                }
            }
            Action::CloseModal => {
                self.modal_state = ModalState::None;
            }
            Action::ModalSelectNext => {
                let total_configured_emulators = self.get_configured_emulator_platforms().len();
                let total_unique_runners = self.db.get_unique_runners().map(|r| r.len()).unwrap_or(0);

                match self.modal_state {
                    ModalState::AddGameStep1Type {
                        ref mut selected_type_idx,
                    } => {
                        *selected_type_idx = (*selected_type_idx + 1) % 4;
                    }
                    ModalState::ScanFolderStep1Platform {
                        ref mut selected_platform_idx,
                    } => {
                        if total_configured_emulators > 0 {
                            *selected_platform_idx =
                                (*selected_platform_idx + 1) % total_configured_emulators;
                        }
                    }
                    ModalState::AddGameForm {
                        game_type: PlatformType::Emulator,
                        ref mut platform_idx,
                        ..
                    } => {
                        if !self.platforms.is_empty() {
                            *platform_idx = (*platform_idx + 1) % self.platforms.len();
                        }
                    }
                    ModalState::ManageRunnersStep1Platform {
                        ref mut selected_runner_idx,
                    } => {
                        if total_unique_runners > 0 {
                            *selected_runner_idx =
                                (*selected_runner_idx + 1) % total_unique_runners;
                        }
                    }
                    ModalState::ManageRunnersStep2Config { .. } => {}
                    ModalState::VisualMediaSelector {
                        active_tab,
                        ref candidates,
                        ref mut selected_candidate_idx,
                        ref covers,
                        ref mut selected_cover_idx,
                        ref banners,
                        ref mut selected_banner_idx,
                        ref icons,
                        ref mut selected_icon_idx,
                        ..
                    } => match active_tab {
                        0 => {
                            if !candidates.is_empty() {
                                *selected_candidate_idx =
                                    (*selected_candidate_idx + 1) % candidates.len();
                            }
                        }
                        1 => {
                            if !covers.is_empty() {
                                *selected_cover_idx = (*selected_cover_idx + 1) % covers.len();
                            }
                        }
                        2 => {
                            if !banners.is_empty() {
                                *selected_banner_idx = (*selected_banner_idx + 1) % banners.len();
                            }
                        }
                        3 => {
                            if !icons.is_empty() {
                                *selected_icon_idx = (*selected_icon_idx + 1) % icons.len();
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                self.update_visual_media_preview();
            }
            Action::ModalSelectPrev => {
                let total_configured_emulators = self.get_configured_emulator_platforms().len();
                let total_unique_runners = self.db.get_unique_runners().map(|r| r.len()).unwrap_or(0);

                match self.modal_state {
                    ModalState::AddGameStep1Type {
                        ref mut selected_type_idx,
                    } => {
                        if *selected_type_idx == 0 {
                            *selected_type_idx = 3;
                        } else {
                            *selected_type_idx -= 1;
                        }
                    }
                    ModalState::ScanFolderStep1Platform {
                        ref mut selected_platform_idx,
                    } => {
                        if total_configured_emulators > 0 {
                            if *selected_platform_idx == 0 {
                                *selected_platform_idx = total_configured_emulators - 1;
                            } else {
                                *selected_platform_idx -= 1;
                            }
                        }
                    }
                    ModalState::AddGameForm {
                        game_type: PlatformType::Emulator,
                        ref mut platform_idx,
                        ..
                    } => {
                        if !self.platforms.is_empty() {
                            if *platform_idx == 0 {
                                *platform_idx = self.platforms.len() - 1;
                            } else {
                                *platform_idx -= 1;
                            }
                        }
                    }
                    ModalState::ManageRunnersStep1Platform {
                        ref mut selected_runner_idx,
                    } => {
                        if total_unique_runners > 0 {
                            if *selected_runner_idx == 0 {
                                *selected_runner_idx = total_unique_runners - 1;
                            } else {
                                *selected_runner_idx -= 1;
                            }
                        }
                    }
                    ModalState::ManageRunnersStep2Config { .. } => {}
                    ModalState::VisualMediaSelector {
                        active_tab,
                        ref candidates,
                        ref mut selected_candidate_idx,
                        ref covers,
                        ref mut selected_cover_idx,
                        ref banners,
                        ref mut selected_banner_idx,
                        ref icons,
                        ref mut selected_icon_idx,
                        ..
                    } => match active_tab {
                        0 => {
                            if !candidates.is_empty() {
                                if *selected_candidate_idx == 0 {
                                    *selected_candidate_idx = candidates.len() - 1;
                                } else {
                                    *selected_candidate_idx -= 1;
                                }
                            }
                        }
                        1 => {
                            if !covers.is_empty() {
                                if *selected_cover_idx == 0 {
                                    *selected_cover_idx = covers.len() - 1;
                                } else {
                                    *selected_cover_idx -= 1;
                                }
                            }
                        }
                        2 => {
                            if !banners.is_empty() {
                                if *selected_banner_idx == 0 {
                                    *selected_banner_idx = banners.len() - 1;
                                } else {
                                    *selected_banner_idx -= 1;
                                }
                            }
                        }
                        3 => {
                            if !icons.is_empty() {
                                if *selected_icon_idx == 0 {
                                    *selected_icon_idx = icons.len() - 1;
                                } else {
                                    *selected_icon_idx -= 1;
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                self.update_visual_media_preview();
            }
            Action::ModalConfirmStep1 => {
                if let ModalState::AddGameStep1Type { selected_type_idx } = self.modal_state {
                    if selected_type_idx == 0 {
                        let emulator_platforms = self.get_configured_emulator_platforms();
                        if emulator_platforms.is_empty() {
                            self.status_msg = "Error: No emulators configured. Press [m] to configure a runner first.".to_string();
                            return;
                        }
                        self.modal_state = ModalState::ScanFolderStep1Platform {
                            selected_platform_idx: 0,
                        };
                    } else {
                        let gtype = match selected_type_idx {
                            1 => PlatformType::Native,
                            2 => PlatformType::Wine,
                            3 => PlatformType::Steam,
                            _ => PlatformType::Emulator,
                        };
                        self.modal_state = ModalState::AddGameForm {
                            game_type: gtype,
                            selected_field: 0,
                            title: String::new(),
                            platform_idx: 0,
                            file_path: String::new(),
                            working_dir: String::new(),
                            wine_prefix: String::new(),
                            steam_appid: String::new(),
                            custom_command: String::new(),
                        };
                    }
                }
            }
            Action::ScanModalConfirmPlatform => {
                if let ModalState::ScanFolderStep1Platform {
                    selected_platform_idx,
                } = self.modal_state
                {
                    let emulator_platforms = self.get_configured_emulator_platforms();
                    if let Some(p) = emulator_platforms.get(selected_platform_idx) {
                        let default_exts = p.default_extensions.join(", ");
                        let saved_folder = self
                            .db
                            .get_scan_folder_for_platform(p.id)
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| {
                                dirs::home_dir()
                                    .unwrap_or_else(|| PathBuf::from("/home"))
                                    .join("Juegos")
                                    .to_string_lossy()
                                    .to_string()
                            });

                        self.modal_state = ModalState::ScanFolderForm {
                            platform: p.clone(),
                            folder_path: saved_folder,
                            extensions_input: default_exts,
                            recursive: true,
                            selected_field: 0,
                        };
                    }
                }
            }
            Action::ModalToggleCheckbox => {
                if let ModalState::ScanFolderForm {
                    ref mut recursive,
                    selected_field,
                    ..
                } = self.modal_state
                {
                    if selected_field == 2 {
                        *recursive = !*recursive;
                    }
                }
            }
            Action::ModalNextField => match self.modal_state {
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ..
                } => {
                    let total_fields = match gtype {
                        PlatformType::Emulator => 4,
                        PlatformType::Native => 5,
                        PlatformType::Wine => 5,
                        PlatformType::Steam => 3,
                    };
                    *selected_field = (*selected_field + 1) % total_fields;
                }
                ModalState::ScanFolderForm {
                    ref mut selected_field,
                    ..
                } => {
                    *selected_field = (*selected_field + 1) % 4;
                }
                _ => {}
            },
            Action::ModalPrevField => match self.modal_state {
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ..
                } => {
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
                ModalState::ScanFolderForm {
                    ref mut selected_field,
                    ..
                } => {
                    if *selected_field == 0 {
                        *selected_field = 3;
                    } else {
                        *selected_field -= 1;
                    }
                }
                _ => {}
            },
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
                }
                | ModalState::EditGameForm {
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
                            3 => custom_command.push(ch),
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
                            4 => custom_command.push(ch),
                            _ => {}
                        },
                        PlatformType::Steam => match selected_field {
                            0 => title.push(ch),
                            1 => {
                                if ch.is_ascii_digit() {
                                    steam_appid.push(ch);
                                }
                            }
                            2 => custom_command.push(ch),
                            _ => {}
                        },
                    }
                } else if let ModalState::ScanFolderForm {
                    ref mut folder_path,
                    ref mut extensions_input,
                    selected_field,
                    ..
                } = self.modal_state
                {
                    match selected_field {
                        0 => folder_path.push(ch),
                        1 => extensions_input.push(ch),
                        _ => {}
                    }
                } else if let ModalState::ManageRunnersStep2Config {
                    ref mut exe_path_input,
                    ..
                } = self.modal_state
                {
                    exe_path_input.push(ch);
                } else if let ModalState::ConfigureApiKeyInput { ref mut input } = self.modal_state
                {
                    input.push(ch);
                } else if let ModalState::AppSettings {
                    ref mut api_key_input,
                    ..
                } = self.modal_state
                {
                    api_key_input.push(ch);
                } else if let ModalState::VisualMediaSelector {
                    active_tab: 0,
                    ref mut search_query,
                    ref mut candidates,
                    ref mut selected_candidate_idx,
                    ..
                } = self.modal_state
                {
                    search_query.push(ch);
                    candidates.clear();
                    *selected_candidate_idx = 0;
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
                }
                | ModalState::EditGameForm {
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
                            3 => Some(custom_command),
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
                            4 => Some(custom_command),
                            _ => None,
                        },
                        PlatformType::Steam => match selected_field {
                            0 => Some(title),
                            1 => Some(steam_appid),
                            2 => Some(custom_command),
                            _ => None,
                        },
                    };

                    if let Some(s) = target_str {
                        s.pop();
                    }
                } else if let ModalState::ScanFolderForm {
                    ref mut folder_path,
                    ref mut extensions_input,
                    selected_field,
                    ..
                } = self.modal_state
                {
                    match selected_field {
                        0 => {
                            folder_path.pop();
                        }
                        1 => {
                            extensions_input.pop();
                        }
                        _ => {}
                    }
                } else if let ModalState::ManageRunnersStep2Config {
                    ref mut exe_path_input,
                    ..
                } = self.modal_state
                {
                    exe_path_input.pop();
                } else if let ModalState::ConfigureApiKeyInput { ref mut input } = self.modal_state
                {
                    input.pop();
                } else if let ModalState::AppSettings {
                    ref mut api_key_input,
                    ..
                } = self.modal_state
                {
                    api_key_input.pop();
                } else if let ModalState::VisualMediaSelector {
                    active_tab: 0,
                    ref mut search_query,
                    ref mut candidates,
                    ref mut selected_candidate_idx,
                    ..
                } = self.modal_state
                {
                    search_query.pop();
                    candidates.clear();
                    *selected_candidate_idx = 0;
                }
            }
            Action::StartFolderScan => {
                if let ModalState::ScanFolderForm {
                    ref platform,
                    ref folder_path,
                    ref extensions_input,
                    recursive,
                    ..
                } = self.modal_state.clone()
                {
                    let path = PathBuf::from(folder_path.trim());
                    if !path.exists() {
                        self.status_msg =
                            format!("[Error] Folder path does not exist: '{}'", folder_path);
                        return;
                    }

                    let selected_extensions: Vec<String> = extensions_input
                        .split(',')
                        .map(str::trim)
                        .filter(|extension| !extension.is_empty())
                        .map(|extension| {
                            let extension = extension.to_ascii_lowercase();
                            if extension.starts_with('.') {
                                extension
                            } else {
                                format!(".{}", extension)
                            }
                        })
                        .collect();
                    if selected_extensions.is_empty() {
                        self.status_msg =
                            "Error: enter at least one ROM extension to scan.".to_string();
                        return;
                    }

                    // The extension field in the scan form is intentionally per-scan:
                    // it can narrow a platform's registry defaults without changing the
                    // globally supported formats stored in SQLite.
                    let mut scan_platform = platform.clone();
                    scan_platform.default_extensions = selected_extensions;

                    let _ = self
                        .db
                        .save_scan_folder(platform.id, folder_path.trim(), recursive);
                    self.status_msg = format!("Scanning ROMs folder for {}...", platform.name);

                    match Scanner::scan_folder(&self.db, &scan_platform, &path, recursive, false) {
                        Ok(added) => {
                            self.status_msg = format!(
                                "[OK] Scan completed: {} ROMs imported/updated from '{}'.",
                                added, folder_path
                            );
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error during scan: {}", err);
                        }
                    }
                }
            }
            Action::QuickRescanPlatform => {
                if self.modal_state == ModalState::None && !self.platforms.is_empty() {
                    let platform = self.platforms[self.selected_platform_idx].clone();
                    if let Ok(Some(saved_path)) = self.db.get_scan_folder_for_platform(platform.id)
                    {
                        let path = PathBuf::from(&saved_path);
                        if path.exists() {
                            self.status_msg =
                                format!("Quick re-scanning '{}' saved folder...", platform.name);
                            match Scanner::scan_folder(&self.db, &platform, &path, true, false) {
                                Ok(added) => {
                                    self.status_msg = format!(
                                        "[OK] Quick re-scan finished: {} ROMs updated from '{}'.",
                                        added, saved_path
                                    );
                                    self.load_platforms();
                                }
                                Err(err) => {
                                    self.status_msg =
                                        format!("Error during quick re-scan: {}", err);
                                }
                            }
                        } else {
                            self.status_msg =
                                format!("[Error] Saved folder not found: '{}'", saved_path);
                        }
                    } else {
                        self.status_msg = format!(
                            "No saved ROM folder for '{}'. Press [a] to scan a folder.",
                            platform.name
                        );
                    }
                }
            }
            Action::ToggleSelectGame => {
                if self.modal_state == ModalState::None
                    && self.focused_pane == FocusedPane::Games
                    && !self.games.is_empty()
                {
                    let game_id = self.games[self.selected_game_idx].id;
                    if self.selected_game_ids.contains(&game_id) {
                        self.selected_game_ids.remove(&game_id);
                    } else {
                        self.selected_game_ids.insert(game_id);
                    }
                    if self.selected_game_idx + 1 < self.games.len() {
                        self.selected_game_idx += 1;
                        self.trigger_async_cover_fetch();
                    }
                }
            }
            Action::DeleteSelectedGames => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    if !self.selected_game_ids.is_empty() {
                        let ids: Vec<i64> = self.selected_game_ids.iter().copied().collect();
                        let count = self.db.delete_games(&ids).unwrap_or(0);
                        self.selected_game_ids.clear();
                        self.status_msg =
                            format!("[OK] Removed {} selected game(s) from database.", count);
                        self.load_platforms();
                    } else if self.selected_game_idx < self.games.len() {
                        let game = &self.games[self.selected_game_idx];
                        let title = game.title.clone();
                        if self.db.delete_game(game.id).is_ok() {
                            self.status_msg = format!("[OK] Removed '{}' from database.", title);
                            self.load_platforms();
                        }
                    }
                }
            }
            Action::FetchGameMedia => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    let api_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                    if api_key
                        .as_ref()
                        .map(|k| k.trim().is_empty())
                        .unwrap_or(true)
                    {
                        self.modal_state = ModalState::ConfigureApiKeyInput {
                            input: String::new(),
                        };
                        self.status_msg =
                            "[API Key Required] Enter your SteamGridDB API key to fetch media."
                                .to_string();
                        return;
                    }

                    let target_games: Vec<Game> = if !self.selected_game_ids.is_empty() {
                        self.games
                            .iter()
                            .filter(|g| self.selected_game_ids.contains(&g.id))
                            .cloned()
                            .collect()
                    } else if self.selected_game_idx < self.games.len() {
                        vec![self.games[self.selected_game_idx].clone()]
                    } else {
                        Vec::new()
                    };

                    if target_games.is_empty() {
                        return;
                    }

                    for g in &target_games {
                        self.media_protocols.remove(&(g.id, "cover".to_string()));
                        self.media_protocols.remove(&(g.id, "banner".to_string()));
                        self.media_protocols.remove(&(g.id, "icon".to_string()));
                        self.pending_cover_requests.remove(&g.id);
                    }

                    let total_games = target_games.len();
                    self.download_progress = Some(DownloadProgressState {
                        runner_id: 0,
                        runner_name: format!("SteamGridDB Media (0/{})", total_games),
                        downloaded_bytes: 0,
                        total_bytes: total_games as u64,
                        percentage: 0.0,
                        is_finished: false,
                        error_msg: None,
                    });

                    self.status_msg = format!(
                        "Fetching SteamGridDB media (Cover, Banner, Icon) for {} game(s)...",
                        total_games
                    );
                    let tx = self.cover_tx.clone();
                    let (progress_tx, progress_rx) = mpsc::channel::<DownloadEvent>(100);
                    self.download_rx = Some(progress_rx);

                    let key_str = api_key.unwrap();
                    let manager = self.cover_manager.clone();

                    tokio::spawn(async move {
                        let client = scraper::steamgriddb::SteamGridDBClient::new(Some(key_str));
                        let db_path = dirs::data_dir()
                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                            .join("tui_game_station")
                            .join("game_station.db");
                        let mut log_lines = Vec::new();
                        log_lines.push(format!(
                            "[DEBUG] Starting FetchGameMedia for {} games...",
                            target_games.len()
                        ));

                        for (idx, game) in target_games.iter().enumerate() {
                            log_lines.push(format!(
                                "[DEBUG] Game ID={}, Title='{}'",
                                game.id, game.title
                            ));
                            let _ = progress_tx
                                .send(DownloadEvent {
                                    downloaded: (idx + 1) as u64,
                                    total: total_games as u64,
                                    percentage: (((idx + 1) as f64 / total_games as f64) * 100.0),
                                    finished: false,
                                    error: None,
                                })
                                .await;

                            match client
                                .download_all_media_for_game(Some(db_path.clone()), game.id, &game.title, false)
                                .await
                            {
                                Ok(res) => {
                                    log_lines.push(format!(
                                        "  [OK] Cover={:?}, Banner={:?}, Icon={:?}",
                                        res.cover_path, res.banner_path, res.icon_path
                                    ));
                                    if let Some(path) = res.cover_path {
                                        if let Some(protocol) =
                                            manager.load_protocol_from_file(&path)
                                        {
                                            let _ = tx
                                                .send(LoadedCoverEvent {
                                                    game_id: game.id,
                                                    media_type: "cover".to_string(),
                                                    protocol,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                Err(err) => {
                                    log_lines.push(format!(
                                        "  [ERROR] Failed to download media: {:?}",
                                        err
                                    ));
                                }
                            }
                        }

                        let _ = progress_tx
                            .send(DownloadEvent {
                                downloaded: total_games as u64,
                                total: total_games as u64,
                                percentage: 100.0,
                                finished: true,
                                error: None,
                            })
                            .await;

                        let log_path = dirs::data_dir()
                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                            .join("tui_game_station")
                            .join("tui_debug.log");
                        let _ = std::fs::write(log_path, log_lines.join("\n"));
                    });
                }
            }
            Action::SaveApiKey => {
                if let ModalState::ConfigureApiKeyInput { ref input } = self.modal_state.clone() {
                    let trimmed = input.trim();
                    if trimmed.is_empty() {
                        self.status_msg = "Error: API Key cannot be empty.".to_string();
                        return;
                    }

                    if self.db.set_setting("steamgriddb_api_key", trimmed).is_ok() {
                        self.status_msg =
                            "[OK] SteamGridDB API Key saved successfully!".to_string();
                        self.modal_state = ModalState::None;
                    }
                }
            }
            Action::OpenSettingsModal => {
                let current_key = self
                    .db
                    .get_setting("steamgriddb_api_key")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                self.modal_state = ModalState::AppSettings {
                    api_key_input: current_key,
                    selected_field: 0,
                };
                self.status_msg =
                    "Settings menu opened. Edit API Key and press Enter to save.".to_string();
            }
            Action::SaveAppSettings => {
                if let ModalState::AppSettings { ref api_key_input, .. } = self.modal_state.clone() {
                    let trimmed = api_key_input.trim();
                    if self.db.set_setting("steamgriddb_api_key", trimmed).is_ok() {
                        self.status_msg = "[OK] Settings updated successfully!".to_string();
                        self.modal_state = ModalState::None;
                    }
                }
            }
            Action::OpenVisualMediaModal => {
                if self.modal_state == ModalState::None && !self.games.is_empty() && self.selected_game_idx < self.games.len() {
                    let game = &self.games[self.selected_game_idx];
                    let game_id = game.id;
                    let title = game.title.clone();
                    let cleaned = scraper::title_cleaner::TitleCleaner::clean_title(&title);
                    let query = if cleaned.is_empty() { title.clone() } else { cleaned };

                    self.modal_state = ModalState::VisualMediaSelector {
                        game_id,
                        game_title: title,
                        search_query: query.clone(),
                        active_tab: 0,
                        is_searching: true,
                        candidates: Vec::new(),
                        selected_candidate_idx: 0,
                        selected_candidate_id: None,
                        covers: Vec::new(),
                        selected_cover_idx: 0,
                        banners: Vec::new(),
                        selected_banner_idx: 0,
                        icons: Vec::new(),
                        selected_icon_idx: 0,
                    };
                    self.status_msg = format!("Searching SteamGridDB for '{}'...", query);

                    let key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                    let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                    if let Ok(results) = client.search_game(&query).await {
                        if let ModalState::VisualMediaSelector {
                            ref mut is_searching,
                            ref mut candidates,
                            ref mut selected_candidate_idx,
                            ..
                        } = self.modal_state
                        {
                            *is_searching = false;
                            *candidates = results;
                            *selected_candidate_idx = 0;
                            self.status_msg = format!("[OK] Found {} candidate(s) on SteamGridDB.", candidates.len());
                        }
                    }
                }
            }
            Action::SearchVisualMedia => {
                if let ModalState::VisualMediaSelector { ref search_query, .. } = self.modal_state.clone() {
                    let key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                    let query = search_query.clone();

                    let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                    if let Ok(results) = client.search_game(&query).await {
                        if let ModalState::VisualMediaSelector {
                            ref mut is_searching,
                            ref mut candidates,
                            ref mut selected_candidate_idx,
                            ..
                        } = self.modal_state
                        {
                            *is_searching = false;
                            *candidates = results;
                            *selected_candidate_idx = 0;
                            self.status_msg = format!("[OK] Found {} candidate(s) on SteamGridDB.", candidates.len());
                        }
                    }
                }
            }
            Action::SelectVisualMediaCandidate => {
                if let ModalState::VisualMediaSelector {
                    ref candidates,
                    selected_candidate_idx,
                    ..
                } = self.modal_state.clone()
                {
                    if let Some(cand) = candidates.get(selected_candidate_idx) {
                        let sgdb_id = cand.id;
                        let cand_name = cand.name.clone();
                        let key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                        let client = scraper::steamgriddb::SteamGridDBClient::new(key);

                        self.status_msg = format!("Loading media options for '{}'...", cand_name);

                        let new_covers = client.get_images(sgdb_id, "grids").await.unwrap_or_default();
                        let new_banners = client.get_images(sgdb_id, "heroes").await.unwrap_or_default();
                        let new_icons = client.get_images(sgdb_id, "icons").await.unwrap_or_default();

                        let c_count = new_covers.len();
                        let b_count = new_banners.len();
                        let i_count = new_icons.len();

                        if let ModalState::VisualMediaSelector {
                            ref mut selected_candidate_id,
                            ref mut covers,
                            ref mut banners,
                            ref mut icons,
                            ref mut active_tab,
                            ..
                        } = self.modal_state
                        {
                            *selected_candidate_id = Some(sgdb_id);
                            *covers = new_covers;
                            *banners = new_banners;
                            *icons = new_icons;
                            *active_tab = 1; // Switch to Covers tab
                            self.status_msg = format!("[OK] Candidate '{}' selected. {} covers, {} banners, {} icons loaded.", cand_name, c_count, b_count, i_count);
                        }
                        self.update_visual_media_preview();
                    }
                }
            }
            Action::SwitchVisualMediaTab => {
                if let ModalState::VisualMediaSelector { ref mut active_tab, .. } = self.modal_state {
                    *active_tab = (*active_tab + 1) % 4;
                }
                self.update_visual_media_preview();
            }
            Action::SetVisualMediaTab(tab_idx) => {
                if let ModalState::VisualMediaSelector { ref mut active_tab, .. } = self.modal_state {
                    *active_tab = tab_idx % 4;
                }
                self.update_visual_media_preview();
            }
            Action::ApplyVisualMediaSelection => {
                if let ModalState::VisualMediaSelector {
                    game_id,
                    active_tab,
                    ref covers,
                    selected_cover_idx,
                    ref banners,
                    selected_banner_idx,
                    ref icons,
                    selected_icon_idx,
                    ..
                } = self.modal_state.clone()
                {
                    let key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                    let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                    let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();

                    match active_tab {
                        1 => {
                            if let Some(c) = covers.get(selected_cover_idx) {
                                let dest = media_dir.join("covers").join(format!("{}.jpg", game_id));
                                let tx = self.cover_tx.clone();
                                let manager = self.cover_manager.clone();
                                let url = c.url.clone();
                                tokio::spawn(async move {
                                    if client.download_file_to_path(&url, &dest).await.is_ok() {
                                        let db_path = dirs::data_dir()
                                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                            .join("tui_game_station")
                                            .join("game_station.db");
                                        if let Ok(db) = Database::open(&db_path) {
                                            let _ = db.record_media_status(game_id, "cover", "downloaded", Some(&dest.to_string_lossy()), Some(&url));
                                        }
                                        if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                            let _ = tx.send(LoadedCoverEvent { game_id, media_type: "cover".to_string(), protocol }).await;
                                        }
                                    }
                                });
                                self.media_protocols.remove(&(game_id, "cover".to_string()));
                                self.pending_cover_requests.remove(&game_id);
                                self.modal_state = ModalState::None;
                                self.status_msg = "[OK] Custom Cover updated!".to_string();
                            }
                        }
                        2 => {
                            if let Some(b) = banners.get(selected_banner_idx) {
                                let dest = media_dir.join("banners").join(format!("{}.jpg", game_id));
                                let tx = self.cover_tx.clone();
                                let manager = self.cover_manager.clone();
                                let url = b.url.clone();
                                tokio::spawn(async move {
                                    if client.download_file_to_path(&url, &dest).await.is_ok() {
                                        let db_path = dirs::data_dir()
                                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                            .join("tui_game_station")
                                            .join("game_station.db");
                                        if let Ok(db) = Database::open(&db_path) {
                                            let _ = db.record_media_status(game_id, "banner", "downloaded", Some(&dest.to_string_lossy()), Some(&url));
                                        }
                                        if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                            let _ = tx.send(LoadedCoverEvent { game_id, media_type: "banner".to_string(), protocol }).await;
                                        }
                                    }
                                });
                                self.media_protocols.remove(&(game_id, "banner".to_string()));
                                self.pending_cover_requests.remove(&game_id);
                                self.modal_state = ModalState::None;
                                self.status_msg = "[OK] Custom Banner updated!".to_string();
                            }
                        }
                        3 => {
                            if let Some(i) = icons.get(selected_icon_idx) {
                                let dest = media_dir.join("icons").join(format!("{}.png", game_id));
                                let tx = self.cover_tx.clone();
                                let manager = self.cover_manager.clone();
                                let url = i.url.clone();
                                tokio::spawn(async move {
                                    if client.download_file_to_path(&url, &dest).await.is_ok() {
                                        let db_path = dirs::data_dir()
                                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                            .join("tui_game_station")
                                            .join("game_station.db");
                                        if let Ok(db) = Database::open(&db_path) {
                                            let _ = db.record_media_status(game_id, "icon", "downloaded", Some(&dest.to_string_lossy()), Some(&url));
                                        }
                                        if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                            let _ = tx.send(LoadedCoverEvent { game_id, media_type: "icon".to_string(), protocol }).await;
                                        }
                                    }
                                });
                                self.media_protocols.remove(&(game_id, "icon".to_string()));
                                self.pending_cover_requests.remove(&game_id);
                                self.modal_state = ModalState::None;
                                self.status_msg = "[OK] Custom Icon updated!".to_string();
                            }
                        }
                        _ => {}
                    }
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
                        self.status_msg = "Game title cannot be empty.".to_string();
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
                        PlatformType::Native => self
                            .platforms
                            .iter()
                            .find(|p| p.slug == "linux")
                            .map(|p| p.id)
                            .unwrap_or(1),
                        PlatformType::Wine => self
                            .platforms
                            .iter()
                            .find(|p| p.slug == "windows")
                            .map(|p| p.id)
                            .unwrap_or(1),
                        PlatformType::Steam => self
                            .platforms
                            .iter()
                            .find(|p| p.slug == "steam")
                            .map(|p| p.id)
                            .unwrap_or(1),
                    };

                    let steam_id = steam_appid.parse::<i64>().ok();

                    let game = Game {
                        id: 0,
                        platform_id,
                        title: title.clone(),
                        sort_title: None,
                        game_type: game_type.to_string(),
                        file_path: if file_path.is_empty() {
                            None
                        } else {
                            Some(file_path.clone())
                        },
                        working_dir: if working_dir.is_empty() {
                            None
                        } else {
                            Some(working_dir.clone())
                        },
                        custom_command: if custom_command.is_empty() {
                            None
                        } else {
                            Some(custom_command.clone())
                        },
                        env_vars: None,
                        wine_prefix: if wine_prefix.is_empty() {
                            None
                        } else {
                            Some(wine_prefix.clone())
                        },
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
                            self.status_msg = format!("Game '{}' saved successfully.", title);
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error saving game: {}", err);
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
