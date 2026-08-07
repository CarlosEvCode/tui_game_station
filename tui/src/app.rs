use anyhow::Result;
use game_core::db::Database;
use game_core::models::{Game, Platform, PlatformType, Runner, ScannedFolder};
use game_core::scanner::Scanner;
use game_core::steam_scanner::SteamScanner;
use ratatui_image::protocol::StatefulProtocol;
use runner::GameRunner;
use scraper::downloader::{DownloadEvent, RunnerDownloader};
use scraper::steam_cover::SteamCoverResolver;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::cover_renderer::CoverManager;

/// Input draining right after a game closes: keep discarding stale gameplay
/// input until the stream has been quiet for this long. Events can trickle in
/// one-by-one for a couple of seconds (e.g. a leaked key press or a still-held
/// gamepad button), so a fixed short window is not enough.
pub const INPUT_DRAIN_QUIET: Duration = Duration::from_millis(300);
/// Absolute ceiling for the post-game input drain, so a held/broken key cannot
/// stall the TUI forever. Reaching it logs a warning and releases input.
pub const INPUT_DRAIN_MAX: Duration = Duration::from_millis(3000);

/// Whether the TUI is (temporarily) discarding stale input after a game exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSafeMode {
    /// Normal operation: input is forwarded to the UI.
    Active,
    /// Post-game drain active: stale input is discarded and destructive
    /// actions (quit, launch, delete...) are rejected until the drain ends.
    Locked,
}

/// What the TUI is currently running in the background (emulator/game) so the
/// UI can show a "Juego en ejecución" indicator and offer a force-close.
pub struct RunningGame {
    pub title: String,
    pub runner_name: Option<String>,
    pub started_at: Instant,
}

/// Result of a background game launch, sent back to the TUI when it exits.
pub type GameExitResult = std::result::Result<std::process::ExitStatus, String>;

/// Actions that must never fire while post-game input draining is active:
/// quitting, launching games/emulators, destructive deletes/installs/scans.
/// Navigation and rendering actions are harmless and may proceed.
fn is_destructive_action(action: &Action) -> bool {
    matches!(
        action,
        Action::Quit
            | Action::LaunchGame
            | Action::OpenRunnerStandalone
            | Action::StartRunnerDownload
            | Action::StartAppUpdate { .. }
            | Action::DeleteSelectedGames
            | Action::ConfirmDeleteGameExecution
            | Action::ConfirmDeleteRunnerExecution
            | Action::ScanCurrentFolder
            | Action::StartFolderScan
            | Action::RescanFolder
            | Action::StartAddAndScan
            | Action::ConfirmDeleteFolderExecution
            | Action::KillWineProcesses
    )
}

/// Guardian central (TAREA 5): while a game is running, the ONLY valid
/// interaction with the launcher is force-closing it. Everything else —
/// navigation, launching, quitting, deletes — is ignored regardless of which
/// input source produced it. This is the single source of truth for input
/// protection: keyboard, mouse and (hot-plugged) gamepad events all funnel
/// through the action dispatcher, so no input thread needs to "know" that a
/// game is running.
fn is_action_allowed_while_game_running(action: &Action) -> bool {
    matches!(action, Action::ForceCloseGame)
}

/// Item in the flat focusable sequence of Modal 1 (ScanFolderForm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanFolderItem {
    FolderEmulator(usize),
    FolderCore(usize),
    AddNewFolder,
    DownloadCores,
}

pub(crate) fn is_retroarch_runner(runner: &Runner) -> bool {
    runner.name == "RetroArch" || runner.runner_type == "retroarch"
}

/// Helper to check if a folder row should display a Core sub-row.
/// RetroArch assigned or RetroArch effective default when assigned is None.
pub(crate) fn folder_has_core(db: &Database, platform_id: i64, folder: &ScannedFolder) -> bool {
    let runners = db.get_runners_for_platform(platform_id).unwrap_or_default();
    let runner = match folder.assigned_emulator_id {
        Some(id) => runners.into_iter().find(|r| r.id == id),
        None => runners.into_iter().find(|r| r.is_active),
    };
    if let Some(r) = runner {
        is_retroarch_runner(&r) || game_core::options::emulator_default_core(&r.name).is_some()
    } else {
        false
    }
}

/// Build the flat focus sequence of items for Modal 1 (ScanFolderForm).
pub fn build_scan_folder_items(db: &Database, platform_id: i64, folders: &[ScannedFolder]) -> Vec<ScanFolderItem> {
    let mut items = Vec::new();
    for (i, folder) in folders.iter().enumerate() {
        items.push(ScanFolderItem::FolderEmulator(i));
        if folder_has_core(db, platform_id, folder) {
            items.push(ScanFolderItem::FolderCore(i));
        }
    }
    items.push(ScanFolderItem::AddNewFolder);
    if scan_folder_platform_has_retroarch(db, platform_id) {
        items.push(ScanFolderItem::DownloadCores);
    }
    items
}

/// Whether the platform has a RetroArch runner configured (active or not).
///
/// The folder manager offers `[ Download Cores ]` whenever RetroArch is a
/// possible emulator for the platform — e.g. NDS with melonDS active but
/// RetroArch configured — not only when RetroArch is the active runner.
pub(crate) fn scan_folder_platform_has_retroarch(db: &Database, platform_id: i64) -> bool {
    db.get_runners_for_platform(platform_id)
        .unwrap_or_default()
        .iter()
        .any(is_retroarch_runner)
}

pub(crate) fn scan_folder_supports_dat(slug: &str) -> bool {
    game_core::dat_downloader::DatDownloader::supports_dat_identification(slug)
}

pub(crate) fn scan_folder_add_has_core(db: &Database, platform_id: i64, add_emu_id: Option<i64>) -> bool {
    let runners = db.get_runners_for_platform(platform_id).unwrap_or_default();
    let runner = match add_emu_id {
        Some(id) => runners.into_iter().find(|r| r.id == id),
        None => runners.into_iter().find(|r| r.is_active),
    };
    if let Some(r) = runner {
        is_retroarch_runner(&r) || game_core::options::emulator_default_core(&r.name).is_some()
    } else {
        false
    }
}

pub(crate) fn scan_folder_add_emu_idx(supports_dat: bool) -> usize {
    3 + if supports_dat { 1 } else { 0 }
}

pub(crate) fn scan_folder_add_core_idx(supports_dat: bool) -> usize {
    scan_folder_add_emu_idx(supports_dat) + 1
}

pub(crate) fn scan_folder_add_dl_core_idx(supports_dat: bool) -> usize {
    scan_folder_add_core_idx(supports_dat) + 1
}

pub(crate) fn scan_folder_add_action_index(supports_dat: bool, has_core: bool) -> usize {
    scan_folder_add_emu_idx(supports_dat) + 1 + if has_core { 2 } else { 0 }
}

pub(crate) fn scan_folder_add_scan_index(supports_dat: bool, has_core: bool) -> usize {
    scan_folder_add_action_index(supports_dat, has_core) + 1
}

pub(crate) fn scan_folder_add_form_total(supports_dat: bool, has_core: bool) -> usize {
    scan_folder_add_scan_index(supports_dat, has_core) + 1
}

/// Total fields of the simplified single-folder scan form: path, extensions,
/// recursive, optional DAT toggle, emulator, optional core, optional [Download Cores] and the final
/// [Add & Scan] button (which sits at `scan_folder_add_action_index`).
pub(crate) fn simple_scan_form_total(supports_dat: bool, has_core: bool) -> usize {
    scan_folder_add_action_index(supports_dat, has_core) + 1
}

/// Real cores for the emulator currently selected in an add-scan form.
///
/// For RetroArch this is filtered by the `.so` files actually present in the
/// cores dir resolved for the runner — the catalog alone is not enough, it
/// lists cores that may not be downloaded. Other core-based emulators fall
/// back to their catalog/TOML core list. Returns `(key, display_label)` in
/// catalog order.
pub(crate) fn add_scan_available_cores(
    db: &Database,
    platform: &Platform,
    emu_id: Option<i64>,
) -> Vec<(String, String)> {
    let runners = db.get_runners_for_platform(platform.id).unwrap_or_default();
    let Some(runner) = emu_id
        .and_then(|id| runners.iter().find(|r| r.id == id))
        .or_else(|| runners.iter().find(|r| r.is_active))
    else {
        return Vec::new();
    };
    let is_retroarch = runner.name == "RetroArch" || runner.runner_type == "retroarch";
    if is_retroarch {
        game_core::retroarch_manager::available_retroarch_cores_for_platform(runner, &platform.slug)
            .into_iter()
            .map(|c| (c.key, c.name))
            .collect()
    } else if game_core::options::emulator_default_core(&runner.name).is_some() {
        game_core::options::emulator_cores_for_platform(&runner.name, &platform.slug)
    } else {
        Vec::new()
    }
}

/// Label for the Emulador field in the add/scan forms.
///
/// "Default" (no override) only makes sense once the platform already has a
/// scan folder to inherit from. For the very first folder there is nothing to
/// inherit yet, so the effective platform emulator is shown instead.
pub(crate) fn add_scan_emulator_label(
    db: &Database,
    platform: &Platform,
    emu_id: Option<i64>,
    runners: &[Runner],
) -> String {
    if let Some(id) = emu_id {
        return runners
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Default".to_string());
    }
    let has_folders = db
        .get_scan_folders_for_platform(platform.id)
        .map(|folders| !folders.is_empty())
        .unwrap_or(false);
    if has_folders {
        "Default".to_string()
    } else {
        db.get_runner_for_platform(platform.id)
            .ok()
            .flatten()
            .map(|r| r.name)
            .unwrap_or_else(|| "Default".to_string())
    }
}

pub(crate) fn scan_folder_section0_total(num_folders: usize) -> usize {
    num_folders + 1
}

fn track_scan_folder_row(field: usize, num_folders: usize, selected_row: &mut usize) {
    if field < num_folders {
        *selected_row = field;
    } else if num_folders == 0 {
        *selected_row = 0;
    }
}

/// Which editable text field (if any) a game form field index maps to, so the
/// text cursor can be tracked per field. `is_edit` distinguishes the
/// EditGameForm layout from the AddGameForm layout (the emulator ROM path
/// moves between field 1 and field 2).
enum FormTextField {
    Path,
    Workdir,
    Prefix,
    Cmd,
}

fn form_text_field(gtype: &PlatformType, is_edit: bool, field: usize) -> Option<FormTextField> {
    match gtype {
        PlatformType::Emulator => match field {
            1 if is_edit => Some(FormTextField::Path),
            2 if !is_edit => Some(FormTextField::Path),
            _ => None,
        },
        PlatformType::Native => match field {
            1 => Some(FormTextField::Path),
            2 => Some(FormTextField::Workdir),
            3 => Some(FormTextField::Cmd),
            _ => None,
        },
        PlatformType::Wine => match field {
            1 => Some(FormTextField::Path),
            2 => Some(FormTextField::Prefix),
            3 => Some(FormTextField::Workdir),
            5 => Some(FormTextField::Cmd),
            _ => None,
        },
        PlatformType::Steam => match field {
            2 => Some(FormTextField::Cmd),
            _ => None,
        },
    }
}

/// Move the cursor of a form text field to the end of its text (the usual
/// position when a field gains focus). Called after field navigation so a
/// stale cursor from a previous field never lands mid-string.
fn sync_form_field_cursor(
    gtype: &PlatformType,
    is_edit: bool,
    field: usize,
    path_cursor: &mut usize,
    workdir_cursor: &mut usize,
    prefix_cursor: &mut usize,
    cmd_cursor: &mut usize,
    path_len: usize,
    workdir_len: usize,
    prefix_len: usize,
    cmd_len: usize,
) {
    match form_text_field(gtype, is_edit, field) {
        Some(FormTextField::Path) => *path_cursor = path_len,
        Some(FormTextField::Workdir) => *workdir_cursor = workdir_len,
        Some(FormTextField::Prefix) => *prefix_cursor = prefix_len,
        Some(FormTextField::Cmd) => *cmd_cursor = cmd_len,
        None => {}
    }
}

pub struct WineToolCommand {
    pub exe: String,
    pub args: Vec<String>,
    pub envs: HashMap<String, String>,
}

/// Cycle an emulator option's value by one step. Toggles flip on/off (both
/// directions do the same); choices move through their value list.
pub fn cycle_runner_option(
    options: &[game_core::options::EmulatorOption],
    values: &mut game_core::options::RunnerOptions,
    idx: usize,
    backward: bool,
) {
    use game_core::options::EmulatorOptionKind;
    let Some(opt) = options.get(idx) else {
        return;
    };
    let current = values
        .get(&opt.key)
        .cloned()
        .unwrap_or_else(|| opt.default.clone());
    let next = match &opt.kind {
        EmulatorOptionKind::Toggle => {
            if current == "1" {
                "0".to_string()
            } else {
                "1".to_string()
            }
        }
        EmulatorOptionKind::Choice(choices) => {
            if choices.is_empty() {
                current
            } else {
                let pos = choices.iter().position(|c| c == &current).unwrap_or(0);
                let next_pos = if backward {
                    if pos == 0 {
                        choices.len() - 1
                    } else {
                        pos - 1
                    }
                } else {
                    (pos + 1) % choices.len()
                };
                choices[next_pos].clone()
            }
        }
    };
    values.insert(opt.key.clone(), next);
}

/// Next index when cycling a list of `len` items that wraps around both
/// directions. `current` is the selected item (or `None` when nothing is
/// selected yet). A single-item list always resolves to index 0.
pub fn cycle_index(current: Option<usize>, len: usize, backward: bool) -> usize {
    match current {
        Some(i) if len > 1 => (i + if backward { len - 1 } else { 1 }) % len,
        _ => 0,
    }
}

pub struct LoadedCoverEvent {
    pub game_id: i64,
    pub media_type: String,
    pub protocol: Option<StatefulProtocol>,
}

pub struct LoadedPreviewEvent {
    pub url: String,
    pub protocol: StatefulProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedPane {
    Search,
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

#[derive(Debug, Clone, PartialEq)]
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
        folders: Vec<ScannedFolder>,
        selected_index: usize,
    },
    /// Windows Games manager (reached with [E] on the Windows platform):
    /// lists every manually-registered Windows game (title + working dir) and
    /// offers [ + Add New Game ] which jumps straight into the Wine details
    /// form. `selected_idx` indexes into `games`, with `games.len()` being the
    /// [ + Add New Game ] button row.
    WindowsGamesManager {
        games: Vec<Game>,
        selected_idx: usize,
    },
    /// Simplified single-folder "Add Game" flow reached from [A] → Scan Folder.
    /// Registers one folder and scans it immediately with the chosen emulator
    /// and core (no folder manager, no multi-selection).
    AddFolderScanForm {
        platform: Platform,
        folder_path: String,
        extensions_input: String,
        recursive: bool,
        use_dat_auto_id: bool,
        add_emulator_id: Option<i64>,
        add_core: Option<String>,
        selected_field: usize,
        parent_modal: Option<Box<ModalState>>,
    },
    DownloadCoreModal {
        platform: Platform,
        runner: Runner,
        cores: Vec<game_core::core_catalog::CoreInfo>,
        downloaded_keys: std::collections::HashSet<String>,
        selected_idx: usize,
        parent_state: Box<ModalState>,
    },
    AddGameForm {
        game_type: PlatformType,
        selected_field: usize,
        parent_modal: Option<Box<ModalState>>,
        title: String,
        platform_idx: usize,
        file_path: String,
        working_dir: String,
        wine_prefix: String,
        steam_appid: String,
        custom_command: String,
        gamemode: bool,
        mangohud: bool,
        gamescope: bool,
        esync: bool,
        fsync: bool,
        dxvk: bool,
        vkd3d: bool,
        cursor_pos: usize,
        path_cursor: usize,
        workdir_cursor: usize,
        prefix_cursor: usize,
        cmd_cursor: usize,
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
        gamemode: bool,
        mangohud: bool,
        gamescope: bool,
        esync: bool,
        fsync: bool,
        dxvk: bool,
        vkd3d: bool,
        cursor_pos: usize,
        path_cursor: usize,
        workdir_cursor: usize,
        prefix_cursor: usize,
        cmd_cursor: usize,
        emulator_override: Option<i64>,
        core_override: Option<String>,
        parent_modal: Option<Box<ModalState>>,
    },
    ConfigureApiKeyInput {
        input: String,
    },
    AppSettings {
        api_key_input: String,
        selected_field: usize,
        is_editing_api_key: bool,
        cursor_pos: usize,
    },
    WelcomeWizard {
        step: usize,
        sgdb_api_key: String,
        active_field: usize,
        cursor_pos: usize,
    },
    VisualMediaSelector {
        game_id: i64,
        game_title: String,
        search_query: String,
        active_tab: usize,      // 0: Candidates, 1: Covers, 2: Banners, 3: Icons
        focused_section: usize, // 0: Tabs, 1: Search Query, 2: Candidates / Results List
        cursor_pos: usize,      // Cursor position in search_query
        is_searching: bool,
        candidates: Vec<scraper::steamgriddb::SteamGridSearchResult>,
        selected_candidate_idx: usize,
        selected_candidate_id: Option<i64>,
        selected_candidate_name: Option<String>,
        covers: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_cover_idx: usize,
        chosen_cover_idx: Option<usize>,
        banners: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_banner_idx: usize,
        chosen_banner_idx: Option<usize>,
        icons: Vec<scraper::steamgriddb::SteamGridImageItem>,
        selected_icon_idx: usize,
        chosen_icon_idx: Option<usize>,
    },
    ManageRunnersStep1Platform {
        selected_platform_idx: usize,
    },
    ManageRunnersStep2Config {
        runner_info: game_core::models::UniqueRunnerInfo,
        exe_path_input: String,
        options: Vec<game_core::options::EmulatorOption>,
        option_values: game_core::options::RunnerOptions,
        custom_args: String,
        selected_row: usize,
        selected_action_idx: usize,
        cursor_pos: usize,
    },
    SelectDetectedEmulatorModal {
        runner_name: String,
        candidates: Vec<game_core::emulator_detector::DetectedEmulator>,
        selected_idx: usize,
        parent_modal: Box<ModalState>,
    },
    ManageWineRunners {
        installed_runners: Vec<game_core::runner_detector::InstalledWineRunner>,
        selected_idx: usize,
    },
    ProtonDownloader {
        step: usize,
        selected_launcher_idx: usize,
        selected_tool_idx: usize,
        releases: Vec<scraper::proton::ProtonRelease>,
        selected_release_idx: usize,
        is_loading: bool,
        download_event: Option<scraper::downloader::DownloadEvent>,
    },
    SelectWineRunnerPicker {
        installed_runners: Vec<game_core::runner_detector::InstalledWineRunner>,
        selected_idx: usize,
        parent_modal: Option<Box<ModalState>>,
    },
    WineToolsMenu {
        selected_idx: usize,
    },
    EditCustomArgsInput {
        input: String,
        cursor_pos: usize,
        parent_modal: Box<ModalState>,
    },
    ConfirmDeleteGame {
        game_ids: Vec<i64>,
        display_title: String,
        selected_option: usize,
    },
    ConfirmDeleteRunner {
        runner_info: game_core::models::UniqueRunnerInfo,
        selected_option: usize,
    },
    ConfirmDeleteFolder {
        platform_id: i64,
        folder_ids: Vec<i64>,
        display: String,
        game_count: usize,
        selected_option: usize,
        parent_modal: Option<Box<ModalState>>,
    },
    PlatformSelector {
        selected_idx: usize,
    },
    CheatsheetModal {
        selected_category_idx: usize,
    },
    FuzzySearchModal {
        query: String,
        cursor_pos: usize,
    },
    About,
    UpdateAvailable {
        new_version: String,
        download_url: String,
        release_notes: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPictureFocus {
    Carousel,
    PlatformBar,
    Search,
}

/// Action bus for the TUI. Some variants are reserved for upcoming features
/// (fuzzy search, wine tooling, batch scanning) and are not yet dispatched.
#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug)]
pub enum Action {
    OpenAboutModal,
    CheckForUpdates {
        silent: bool,
    },
    StartAppUpdate {
        download_url: String,
        new_version: String,
    },
    NextPlatform,
    PrevPlatform,
    /// Cycle the active emulator (or core, when the active emulator is
    /// core-based) with the ◀ ▶ selector in the platforms pane.
    CycleActiveEmulatorNext,
    CycleActiveEmulatorPrev,
    NextGame,
    PrevGame,
    OpenPlatformSelectorModal,
    ConfirmPlatformSelectorModal,
    OpenCheatsheetModal,
    OpenWelcomeWizardModal,
    OpenFuzzySearchModal,
    UpdateFuzzySearchQuery(String),
    ClearFuzzySearch,
    AddToast(String, crate::toast::ToastKind),
    ToggleBigPictureFocus,
    OpenWineRunnerManager,
    OpenProtonDownloader,
    ProtonDownloaderSelectNext,
    ProtonDownloaderSelectPrev,
    ProtonDownloaderConfirm,
    ProtonDownloaderBack,
    FetchProtonReleases,
    StartProtonDownload,
    OpenWineRunnerPicker,
    SelectWineRunnerFromPicker,
    CycleWineRunner(i32),
    DeleteInstalledWineRunner,
    OpenCustomArgsEditor,
    SaveCustomArgsInput,
    OpenWinecfg,
    OpenWinetricks,
    KillWineProcesses,
    OpenWineToolsMenu,
    SelectWineTool,
    FormNavLeft,
    FormNavRight,
    TogglePane,
    ToggleViewMode,
    ToggleShowAllPlatforms,
    LaunchGame,
    OpenGameDetail,
    CloseGameDetail,
    DetailNextAction,
    DetailPrevAction,
    ScanCurrentFolder,
    ScanSteamGames,

    // File / Folder Picker Actions
    OpenFilePicker,
    OpenFolderPicker,

    // Add Game & Scan Modal Actions
    OpenAddGameModal,
    /// Open the Wine "Add Game Details" form directly (Windows Games manager).
    OpenAddGameWineForm,
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
    /// Re-scan a specific existing scan folder from the folder manager.
    RescanFolder,
    /// Re-assign the emulator pinned to a scan folder (◀ ▶ on its row).
    CycleFolderEmulator(bool),
    /// Re-assign the core pinned to a scan folder (◀ ▶ on its core row).
    CycleFolderCore(bool),
    CycleEditGameEmulator(bool),
    OpenConfirmDeleteFolder,
    /// Open delete confirmation dialog for a specific folder index in ScanFolderForm.
    OpenConfirmDeleteFolderForIndex(usize),
    ConfirmDeleteFolderExecution,
    ToggleConfirmDeleteFolderOption,
    /// Open Modal 2 (Add Folder Modal) from Modal 1.
    OpenAddFolderModal,
    /// Register the folder currently filled in the "Add New Folder" pane and
    /// keep the manager open so more folders can be queued before scanning.
    AddFolder,
    /// Register the folder filled in the simplified Add-Game scan form and
    /// scan it immediately with the chosen emulator/core ([Añadir y Escanear]).
    StartAddAndScan,
    /// Open the core download picker modal from scan folder or edit game forms.
    OpenDownloadCoreModal,
    /// Download the currently selected core in DownloadCoreModal.
    TriggerDownloadCore,
    /// Open the full folder manager (`ScanFolderForm`) for the selected
    /// platform ([e] while the Platforms panel is focused).
    OpenFolderManagerForPlatform,
    /// Toggle a scan folder row in/out of the multi-selection ([Space]).
    ToggleSelectFolder,
    /// Switch the folder-manager modal focus between the "Registered Folders"
    /// pane and the "Add New Folder" form (Tab / gamepad bumpers).
    SwitchScanFolderPane,
    QuickRescanPlatform,
    ToggleSelectGame,
    ToggleBigPictureMode,
    DeleteSelectedGames,
    OpenConfirmDeleteModal,
    ConfirmDeleteGameExecution,
    ToggleConfirmDeleteOption,
    FetchGameMedia,
    SaveApiKey,
    OpenSettingsModal,
    SaveAppSettings,
    OpenVisualMediaModal,
    SearchVisualMedia,
    SelectVisualMediaCandidate,
    SwitchVisualMediaTab,
    SwitchVisualMediaTabPrev,
    VisualMediaNavUp,
    VisualMediaNavDown,
    VisualMediaNavLeft,
    VisualMediaNavRight,
    SetVisualMediaTab(usize),
    ApplyVisualMediaSelection,

    // Manage Runners Modal Actions
    OpenManageRunnersModal,
    RunnerModalConfirmPlatform,
    OpenDetectEmulatorModal,
    SelectDetectedEmulatorCandidate,
    SaveRunnerConfig,
    ResetRunnerConfig,
    ToggleRunnerActiveState,
    StartRunnerDownload,
    UpdateDownloadProgress(DownloadEvent),
    DeleteRunnerDownload,
    OpenConfirmDeleteRunnerModal,
    ToggleConfirmDeleteRunnerOption,
    ConfirmDeleteRunnerExecution,
    OpenRunnerStandalone,
    /// Kill the currently running game (real process + tree + FUSE mount) from
    /// the "Juego en ejecución" indicator. Available whenever a game runs.
    ForceCloseGame,

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
    pub scan_rx: Option<mpsc::Receiver<game_core::scanner::ScanProgressEvent>>,
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
    pub is_big_picture: bool,
    #[allow(dead_code)]
    pub big_picture_cols: usize,
    pub big_picture_focus: BigPictureFocus,
    pub big_picture_in_detail: bool,
    pub detail_action_idx: usize,
    pub status_msg: String,
    pub should_quit: bool,
    pub pending_wine_tool: Option<WineToolCommand>,
    pub toasts: Vec<crate::toast::Toast>,
    pub search_query: String,
    pub is_search_active: bool,
    pub update_rx:
        Option<mpsc::Receiver<Result<Option<crate::updater::UpdateCheckResult>, String>>>,
    pub is_manual_update_check: bool,
    pub gamepad_rx: Option<std::sync::mpsc::Receiver<crate::gamepad::GamepadEvent>>,
    pub gamepad_suspended: Option<Arc<AtomicBool>>,
    pub active_gamepad_name: Option<String>,
    pub active_input_source: InputSource,
    pub needs_terminal_clear: bool,
    pub input_safe_mode: InputSafeMode,
    pub input_drain_started_at: Option<Instant>,
    pub input_drain_last_event_at: Option<Instant>,
    pub log_next_input: bool,
    pub running_game: Option<RunningGame>,
    pub game_exit_rx: Option<mpsc::Receiver<GameExitResult>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    Keyboard,
    Gamepad(String),
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
        let (gamepad_rx, gamepad_suspended) = match crate::gamepad::spawn_gamepad_thread() {
            Some((rx, flag)) => (Some(rx), Some(flag)),
            None => (None, None),
        };

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
            scan_rx: None,
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
            is_big_picture: false,
            big_picture_cols: 4,
            big_picture_focus: BigPictureFocus::Carousel,
            big_picture_in_detail: false,
            detail_action_idx: 0,
            status_msg: "TUI Game Station ready!".to_string(),
            should_quit: false,
            pending_wine_tool: None,
            toasts: Vec::new(),
            search_query: String::new(),
            is_search_active: false,
            update_rx: None,
            is_manual_update_check: false,
            gamepad_rx,
            gamepad_suspended,
            active_gamepad_name: None,
            active_input_source: InputSource::Keyboard,
            needs_terminal_clear: false,
            input_safe_mode: InputSafeMode::Active,
            input_drain_started_at: None,
            input_drain_last_event_at: None,
            log_next_input: false,
            running_game: None,
            game_exit_rx: None,
        };

        if let Some(app_dir) = dirs::data_dir() {
            let marker = app_dir
                .join("tui_game_station")
                .join("welcome_new_version.txt");
            if marker.exists() {
                if let Ok(ver) = std::fs::read_to_string(&marker) {
                    app.show_toast(
                        format!("[Welcome] Up to date (v{}).", ver.trim()),
                        crate::toast::ToastKind::Success,
                    );
                }
                let _ = std::fs::remove_file(marker);
            }
        }

        let is_first_run = app
            .db
            .get_setting("first_run_completed")
            .ok()
            .flatten()
            .map(|v| v != "true")
            .unwrap_or(true);

        if is_first_run {
            let api_key = app
                .db
                .get_setting("steamgriddb_api_key")
                .ok()
                .flatten()
                .unwrap_or_default();
            let key_len = api_key.len();
            app.modal_state = ModalState::WelcomeWizard {
                step: 0,
                sgdb_api_key: api_key,
                active_field: 0,
                cursor_pos: key_len,
            };
        } else {
            let (tx, rx) = mpsc::channel(1);
            app.update_rx = Some(rx);
            tokio::spawn(async move {
                let result = crate::updater::check_for_updates(env!("CARGO_PKG_VERSION"))
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(result).await;
            });
        }

        if steam_added > 0 {
            app.show_toast(
                format!("Detected {} Steam games automatically!", steam_added),
                crate::toast::ToastKind::Success,
            );
        }

        // Settle the active emulator of every platform on first launch (e.g. a
        // previous session may have deleted the active emulator's executable).
        app.revalidate_active_emulators();
        app.load_games_for_selected_platform();
        Ok(app)
    }

    pub fn finish_welcome_wizard(&mut self, sgdb_api_key: &str) {
        let _ = self.db.set_setting("first_run_completed", "true");
        if !sgdb_api_key.trim().is_empty() {
            let _ = self
                .db
                .set_setting("steamgriddb_api_key", sgdb_api_key.trim());
        }
        self.modal_state = ModalState::None;
        self.show_toast("Welcome setup completed!", crate::toast::ToastKind::Success);
    }

    pub fn show_toast(&mut self, msg: impl Into<String>, kind: crate::toast::ToastKind) {
        self.toasts.push(crate::toast::Toast::new(msg, kind));
    }

    pub fn sync_platform_selection_with_game(&mut self) {
        if self.is_search_active
            && !self.games.is_empty()
            && self.selected_game_idx < self.games.len()
        {
            let pid = self.games[self.selected_game_idx].platform_id;
            if let Some(idx) = self.platforms.iter().position(|p| p.id == pid) {
                self.selected_platform_idx = idx;
            }
        }
    }

    pub fn filter_games_by_search(&mut self) {
        if self.search_query.trim().is_empty() {
            self.is_search_active = false;
            self.load_games_for_selected_platform();
            return;
        }

        self.is_search_active = true;
        let q = self.search_query.to_lowercase();
        let all_games = self.db.get_all_games().unwrap_or_default();
        self.games = all_games
            .into_iter()
            .filter(|g| g.title.to_lowercase().contains(&q))
            .collect();
        self.selected_game_idx = 0;
        self.sync_platform_selection_with_game();
        self.trigger_async_cover_fetch();
    }

    pub fn apply_cli_args(&mut self, cli_args: &crate::cli::CliArgs) {
        if let Some(ref target) = cli_args.platform {
            if let Some(idx) = self
                .platforms
                .iter()
                .position(|p| p.name.to_lowercase().contains(&target.to_lowercase()))
            {
                self.selected_platform_idx = idx;
                self.load_games_for_selected_platform();
            }
        }

        if cli_args.big_picture {
            self.is_big_picture = true;
            self.preload_visible_covers();
            self.status_msg = "[MODE] Started directly in BIG PICTURE Mode".to_string();
        }
    }

    /// Platforms list for Runner Manager (excludes Linux Native & Steam)
    #[allow(dead_code)]
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
                    // A platform is "configured" if ANY of its runners has an executable path.
                    // This covers multi-runner platforms like Switch (Ryujinx + Eden).
                    let runners = self.db.get_runners_for_platform(p.id).unwrap_or_default();
                    runners.iter().any(|r| r.executable_path.is_some())
                }
            })
            .collect()
    }

    pub fn load_platforms(&mut self) {
        // Auto-switch the active emulator away from any emulator whose
        // executable disappeared (deleted via [m]/[w] or manually).
        self.revalidate_active_emulators();
        if let Ok(platforms) = self.db.get_active_platforms(self.show_all_platforms) {
            self.platforms = platforms;
            if self.selected_platform_idx >= self.platforms.len() {
                self.selected_platform_idx = 0;
            }
            self.load_games_for_selected_platform();
        }
    }

    /// Kick off a background ROM scan for `path` under `platform`, linking the
    /// produced games to `folder_id` (or leaving them legacy when `None`).
    /// Shared by the folder-manager "add & scan", the per-folder re-scan and the
    /// quick re-scan.
    fn begin_scan(
        &mut self,
        platform: Platform,
        path: PathBuf,
        recursive: bool,
        use_dat_auto_id: bool,
        folder_id: Option<i64>,
    ) {
        if !path.exists() {
            self.status_msg = format!("[Error] Folder path does not exist: '{}'", path.display());
            return;
        }

        let (scan_tx, scan_rx) = mpsc::channel::<game_core::scanner::ScanProgressEvent>(100);
        self.scan_rx = Some(scan_rx);
        self.download_progress = Some(DownloadProgressState {
            runner_id: 0,
            runner_name: format!("Scanning ROMs: {}", platform.name),
            downloaded_bytes: 0,
            total_bytes: 1,
            percentage: 0.0,
            is_finished: false,
            error_msg: None,
        });
        self.modal_state = ModalState::None;
        self.status_msg = format!("Scanning & Identifying ROMs for {}...", platform.name);

        let db_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station")
            .join("game_station.db");

        tokio::spawn(async move {
            if use_dat_auto_id {
                let slug = platform.slug.clone();
                let _ = game_core::dat_downloader::DatDownloader::ensure_dat_downloaded(&slug)
                    .await;
            }

            let (sync_tx, sync_rx) = std::sync::mpsc::channel();
            let scan_tx_clone = scan_tx.clone();

            tokio::task::spawn_blocking(move || {
                if let Ok(db) = Database::open(&db_path) {
                    let _ = Scanner::scan_folder(
                        &db,
                        &platform,
                        &path,
                        recursive,
                        false,
                        use_dat_auto_id,
                        folder_id,
                        Some(&sync_tx),
                    );
                }
            });

            while let Ok(evt) = sync_rx.recv() {
                let _ = scan_tx_clone.send(evt).await;
            }
        });
    }

    /// Kick off a sequential background scan of several folders through a single
    /// progress stream. Only the last folder's event carries `finished: true`,
    /// with the total number of imported/updated ROMs.
    fn begin_scan_many(&mut self, jobs: Vec<(Platform, PathBuf, bool, bool, Option<i64>)>) {
        if jobs.is_empty() {
            return;
        }
        let platform_name = jobs[0].0.name.clone();
        let total_jobs = jobs.len();
        let (scan_tx, scan_rx) = mpsc::channel::<game_core::scanner::ScanProgressEvent>(100);
        self.scan_rx = Some(scan_rx);
        self.download_progress = Some(DownloadProgressState {
            runner_id: 0,
            runner_name: format!("Scanning ROMs: {}", platform_name),
            downloaded_bytes: 0,
            total_bytes: 1,
            percentage: 0.0,
            is_finished: false,
            error_msg: None,
        });
        self.modal_state = ModalState::None;
        self.status_msg = format!(
            "Scanning & Identifying ROMs for {} ({} folder{})...",
            platform_name,
            total_jobs,
            if total_jobs == 1 { "" } else { "s" }
        );

        let db_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("tui_game_station")
            .join("game_station.db");

        tokio::spawn(async move {
            if jobs.iter().any(|j| j.3) {
                let slug = jobs[0].0.slug.clone();
                let _ = game_core::dat_downloader::DatDownloader::ensure_dat_downloaded(&slug).await;
            }

            let mut total_added = 0usize;
            for (idx, (platform, path, recursive, use_dat, folder_id)) in
                jobs.into_iter().enumerate()
            {
                let folder_num = idx + 1;
                let (sync_tx, sync_rx) = std::sync::mpsc::channel();
                let scan_tx_clone = scan_tx.clone();
                let platform = platform.clone();
                let path = path.clone();
                let db_path = db_path.clone();

                let _ = scan_tx_clone
                    .send(game_core::scanner::ScanProgressEvent {
                        current: folder_num,
                        total: total_jobs,
                        current_title: format!(
                            "Folder {}/{}: {}",
                            folder_num,
                            total_jobs,
                            path.display()
                        ),
                        finished: false,
                        added_count: 0,
                        error_msg: None,
                    })
                    .await;

                tokio::task::spawn_blocking(move || {
                    if let Ok(db) = Database::open(&db_path) {
                        let _ = Scanner::scan_folder(
                            &db,
                            &platform,
                            &path,
                            recursive,
                            false,
                            use_dat,
                            folder_id,
                            Some(&sync_tx),
                        );
                    }
                });

                while let Ok(mut evt) = sync_rx.recv() {
                    if evt.finished {
                        total_added += evt.added_count;
                        if folder_num == total_jobs {
                            evt.added_count = total_added;
                        } else {
                            evt.finished = false;
                            evt.current_title = format!(
                                "Folder {}/{} scanned.",
                                folder_num, total_jobs
                            );
                        }
                    }
                    let _ = scan_tx_clone.send(evt).await;
                }
            }
        });
    }

    /// Rebuild the folder-manager modal (`ScanFolderForm`) for a platform after
    /// a folder was added or deleted.
    fn reload_scan_folder_modal(&mut self, platform_id: i64) {
        let Some(platform) = self
            .platforms
            .iter()
            .find(|p| p.id == platform_id)
            .cloned()
            .or_else(|| {
                self.db
                    .get_platforms()
                    .ok()?
                    .into_iter()
                    .find(|p| p.id == platform_id)
            })
        else {
            self.modal_state = ModalState::None;
            return;
        };
        let folders = self
            .db
            .get_scan_folders_for_platform(platform.id)
            .unwrap_or_default();
        let selected_index = if let ModalState::ScanFolderForm { selected_index, .. } = self.modal_state {
            selected_index
        } else {
            0
        };
        let items = build_scan_folder_items(&self.db, platform.id, &folders);
        let valid_index = selected_index.min(items.len().saturating_sub(1));
        self.modal_state = ModalState::ScanFolderForm {
            platform,
            folders,
            selected_index: valid_index,
        };
    }

    pub fn open_confirm_delete_folder_for_index(&mut self, folder_idx: usize) {
        if let ModalState::ScanFolderForm {
            ref platform,
            ref folders,
            ..
        } = self.modal_state
        {
            if let Some(folder) = folders.get(folder_idx) {
                let game_count = self.db.get_game_count_for_folder(folder.id).unwrap_or(0);
                let parent_modal = Some(Box::new(self.modal_state.clone()));
                self.modal_state = ModalState::ConfirmDeleteFolder {
                    platform_id: platform.id,
                    folder_ids: vec![folder.id],
                    display: folder.path.clone(),
                    game_count,
                    selected_option: 0,
                    parent_modal,
                };
            }
        }
    }

    /// Opens the "Add Game Details" form for `gtype`. The current modal (if
    /// any) becomes the parent so [Esc] returns to it.
    fn open_add_game_form(&mut self, gtype: PlatformType) {
        let parent_modal = if self.modal_state == ModalState::None {
            None
        } else {
            Some(Box::new(self.modal_state.clone()))
        };
        self.modal_state = ModalState::AddGameForm {
            game_type: gtype,
            selected_field: 0,
            parent_modal,
            title: String::new(),
            platform_idx: 0,
            file_path: String::new(),
            working_dir: String::new(),
            wine_prefix: String::new(),
            steam_appid: String::new(),
            custom_command: String::new(),
            gamemode: false,
            mangohud: false,
            gamescope: false,
            esync: false,
            fsync: false,
            dxvk: false,
            vkd3d: false,
            cursor_pos: 0,
            path_cursor: 0,
            workdir_cursor: 0,
            prefix_cursor: 0,
            cmd_cursor: 0,
        };
    }

    /// Opens the "Edit Game Details" form for `game`. The current modal (if
    /// any) becomes the parent so [Esc] returns to it.
    fn open_edit_game_form(&mut self, game: Game, gtype: PlatformType) {
        let parent_modal = if self.modal_state == ModalState::None {
            None
        } else {
            Some(Box::new(self.modal_state.clone()))
        };
        let title_str = game.title.clone();
        let cpos = title_str.len();
        let env_str = game.env_vars.as_deref().unwrap_or_default();
        let gamemode = env_str.contains("GAMEMODE=1");
        let mangohud = env_str.contains("MANGOHUD=1");
        let gamescope = env_str.contains("GAMESCOPE=1");
        let esync = env_str.contains("WINEESYNC=1");
        let fsync = env_str.contains("WINEFSYNC=1");
        let dxvk = env_str.contains("DXVK_ASYNC=1");
        let vkd3d = env_str.contains("VKD3D_CONFIG=enable_async");
        self.modal_state = ModalState::EditGameForm {
            game_id: game.id,
            game_type: gtype,
            selected_field: 0,
            title: title_str,
            file_path: game.file_path.clone().unwrap_or_default(),
            working_dir: game.working_dir.clone().unwrap_or_default(),
            custom_command: game.custom_command.clone().unwrap_or_default(),
            wine_prefix: game.wine_prefix.clone().unwrap_or_default(),
            steam_appid: game
                .steam_appid
                .map(|id| id.to_string())
                .unwrap_or_default(),
            gamemode,
            mangohud,
            gamescope,
            esync,
            fsync,
            dxvk,
            vkd3d,
            cursor_pos: cpos,
            path_cursor: game.file_path.clone().unwrap_or_default().len(),
            workdir_cursor: game.working_dir.clone().unwrap_or_default().len(),
            prefix_cursor: game.wine_prefix.clone().unwrap_or_default().len(),
            cmd_cursor: game.custom_command.clone().unwrap_or_default().len(),
            emulator_override: game.emulator_override,
            core_override: game.core_override.clone(),
            parent_modal,
        };
    }

    /// Handles [Enter] on the Windows Games manager list: opens the edit form
    /// for the selected game, or the Wine details form for [ + Add New Game ].
    pub(crate) async fn handle_windows_games_enter(&mut self) {
        if let ModalState::WindowsGamesManager { games, selected_idx } = &self.modal_state {
            if *selected_idx == games.len() {
                self.update(Action::OpenAddGameWineForm).await;
            } else if let Some(game) = games.get(*selected_idx) {
                let gtype = PlatformType::from(game.game_type.as_str());
                self.open_edit_game_form(game.clone(), gtype);
            }
        }
    }

    /// Helper for `Action::ModalConfirm`: handles [Enter] on `ScanFolderForm` items.
    pub(crate) async fn handle_scan_form_enter(&mut self) {
        let (items, selected_index) = match &self.modal_state {
            ModalState::ScanFolderForm { platform, folders, selected_index } => {
                (build_scan_folder_items(&self.db, platform.id, folders), *selected_index)
            }
            _ => return,
        };
        let item = items.get(selected_index).cloned().unwrap_or(ScanFolderItem::AddNewFolder);
        match item {
            ScanFolderItem::FolderEmulator(i) | ScanFolderItem::FolderCore(i) => {
                // Enter on a folder row applies the emulator/core selection
                // (already persisted when cycling) and re-scans ONLY when the
                // folder contains genuinely new game files. Already-imported
                // games are never re-identified and the task slider is not
                // spawned when there is nothing new; [R] forces a full rescan.
                if let ModalState::ScanFolderForm {
                    ref platform,
                    ref folders,
                    ..
                } = self.modal_state
                {
                    if let Some(folder) = folders.get(i) {
                        let count = self.db.get_game_count_for_folder(folder.id).unwrap_or(0);
                        let path = PathBuf::from(&folder.path);
                        let has_new = game_core::scanner::Scanner::folder_has_new_games(
                            &self.db,
                            platform,
                            &path,
                            folder.recursive,
                            folder.id,
                        )
                        .unwrap_or(false);
                        if has_new {
                            self.status_msg =
                                format!("Scanning '{}' for new games...", folder.path);
                            self.update(Action::RescanFolder).await;
                        } else {
                            self.status_msg = format!(
                                "Folder up to date ({} game{}). Press [R] to force a full rescan.",
                                count,
                                if count == 1 { "" } else { "s" }
                            );
                        }
                        return;
                    }
                }
                self.update(Action::RescanFolder).await;
            }
            ScanFolderItem::AddNewFolder => {
                // Enter on [+ Agregar carpeta nueva] opens Modal 2 (AddFolderScanForm)
                self.update(Action::OpenAddFolderModal).await;
            }
            ScanFolderItem::DownloadCores => {
                self.update(Action::OpenDownloadCoreModal).await;
            }
        }
    }

    /// Enter on the simplified single-folder scan form: browse on Path, skip
    /// text fields, toggle the recursive/DAT checkboxes, open Download Cores modal,
    /// or start the add+scan on the final [Add & Scan] button.
    pub(crate) async fn handle_add_scan_form_enter(&mut self) {
        let ModalState::AddFolderScanForm {
            ref platform,
            add_emulator_id,
            selected_field,
            ..
        } = self.modal_state
        else {
            return;
        };
        let supports_dat = scan_folder_supports_dat(&platform.slug);
        let has_core = scan_folder_add_has_core(&self.db, platform.id, add_emulator_id);
        let dl_core_idx = scan_folder_add_dl_core_idx(supports_dat);
        let action_idx = scan_folder_add_action_index(supports_dat, has_core);
        match selected_field {
            0 => self.update(Action::OpenFolderPicker).await,
            1 => self.update(Action::ModalNextField).await,
            f if has_core && f == dl_core_idx => self.update(Action::OpenDownloadCoreModal).await,
            f if f == action_idx => self.update(Action::StartAddAndScan).await,
            _ => self.update(Action::ModalToggleCheckbox).await,
        }
    }

    /// Number of registered folder rows in the folder manager (rows occupy
    /// fields 0..N-1; field N is the [DELETE SELECTED] button).
    pub fn scan_folder_num_rows(&self) -> usize {
        match &self.modal_state {
            ModalState::ScanFolderForm { folders, .. } => folders.len(),
            _ => 0,
        }
    }

    /// Runners of a platform whose executable path is configured. These are the
    /// only emulators the "Emulador: ◀ ▶" selector cycles through.
    fn configured_runners_for(&self, platform_id: i64) -> Vec<Runner> {
        self.db
            .get_runners_for_platform(platform_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.executable_path.as_ref().is_some_and(|ex| !ex.trim().is_empty()))
            .collect()
    }

    /// Current `(platform, emulator, core)` selection shared by the folder
    /// manager and the simplified add-scan form.
    fn add_scan_form_selection(&self) -> Option<(Platform, Option<i64>, Option<String>)> {
        match &self.modal_state {
            ModalState::AddFolderScanForm {
                platform,
                add_emulator_id,
                add_core,
                ..
            } => Some((platform.clone(), *add_emulator_id, add_core.clone())),
            _ => None,
        }
    }

    fn set_add_scan_form_selection(&mut self, emu: Option<i64>, core: Option<String>) {
        match &mut self.modal_state {
            ModalState::AddFolderScanForm {
                add_emulator_id,
                add_core,
                ..
            } => {
                *add_emulator_id = emu;
                *add_core = core;
            }
            _ => {}
        }
    }

    /// When the selected emulator needs a core and none is chosen yet, seed the
    /// first core that is actually available on disk (for RetroArch this is
    /// the downloaded `.so` list, not the raw catalog).
    fn seed_add_scan_core(&mut self) {
        let Some((platform, emu, core)) = self.add_scan_form_selection() else {
            return;
        };
        if core.is_some() {
            return;
        }
        if !scan_folder_add_has_core(&self.db, platform.id, emu) {
            return;
        }
        let available = add_scan_available_cores(&self.db, &platform, emu);
        if let Some((key, _)) = available.first() {
            self.set_add_scan_form_selection(emu, Some(key.clone()));
        }
    }

    pub fn cycle_add_folder_emulator(&mut self, backward: bool) {
        let Some((platform, current_emu, current_core)) = self.add_scan_form_selection() else {
            return;
        };
        let configured = self.configured_runners_for(platform.id);
        if configured.is_empty() {
            return;
        }

        // "Default" (no override) is only offered once the platform already has
        // a scan folder to inherit from. For the very first folder there is
        // nothing to inherit, so the selector wraps around the real emulators.
        let allow_default = self
            .db
            .get_scan_folders_for_platform(platform.id)
            .map(|folders| !folders.is_empty())
            .unwrap_or(false);

        let current = current_emu.and_then(|id| configured.iter().position(|r| r.id == id));
        let next_idx = if let Some(idx) = current {
            if !backward {
                if idx + 1 < configured.len() {
                    Some(idx + 1)
                } else if allow_default {
                    None
                } else {
                    Some(0)
                }
            } else if idx > 0 {
                Some(idx - 1)
            } else if allow_default {
                None
            } else {
                Some(configured.len() - 1)
            }
        } else if backward {
            Some(configured.len() - 1)
        } else {
            Some(0)
        };
        let next_id = next_idx.map(|i| configured[i].id);

        let has_core = scan_folder_add_has_core(&self.db, platform.id, next_id);
        let mut next_core = current_core;
        if has_core {
            // Seed/refresh the core from the cores actually available on disk
            // (for RetroArch this filters by the downloaded `.so` files).
            let available = add_scan_available_cores(&self.db, &platform, next_id);
            let still_valid = next_core
                .as_deref()
                .map(|k| available.iter().any(|(ck, _)| ck == k))
                .unwrap_or(false);
            if !still_valid {
                next_core = available.first().map(|(k, _)| k.clone());
            }
        } else {
            next_core = None;
        }
        self.set_add_scan_form_selection(next_id, next_core);
    }

    pub fn cycle_add_folder_core(&mut self, backward: bool) {
        let Some((platform, emu, current_core)) = self.add_scan_form_selection() else {
            return;
        };
        let available = add_scan_available_cores(&self.db, &platform, emu);
        if available.is_empty() {
            return;
        }
        let curr_pos = current_core
            .as_deref()
            .and_then(|k| available.iter().position(|(ck, _)| ck == k))
            .unwrap_or(0);
        let next_pos = if backward {
            if curr_pos == 0 {
                available.len() - 1
            } else {
                curr_pos - 1
            }
        } else {
            (curr_pos + 1) % available.len()
        };
        self.set_add_scan_form_selection(emu, Some(available[next_pos].0.clone()));
    }

    pub fn cycle_edit_game_emulator(&mut self, backward: bool) {
        if let ModalState::EditGameForm {
            game_id,
            game_type: PlatformType::Emulator,
            ref mut emulator_override,
            ..
        } = self.modal_state
        {
            if let Some(pos) = self.games.iter().position(|g| g.id == game_id) {
                let game = self.games[pos].clone();
                let choices = crate::edit_game_details::EditGameFormHelper::get_emulator_choices(&self.db, &game);
                let new_override = crate::edit_game_details::EditGameFormHelper::cycle_choice(&choices, *emulator_override, backward);
                *emulator_override = new_override;
                let _ = self.db.set_game_emulator_override(game_id, new_override);
                if let Some(g) = self.games.get_mut(pos) {
                    g.emulator_override = new_override;
                }
                let choice_idx = crate::edit_game_details::EditGameFormHelper::get_current_choice_idx(&choices, new_override);
                let choice_label = &choices[choice_idx].display_label;
                self.status_msg = format!("Emulator assigned to game: {}", choice_label);
            }
        }
    }

    pub fn cycle_edit_game_core(&mut self, backward: bool) {
        if let ModalState::EditGameForm {
            game_id,
            game_type: PlatformType::Emulator,
            ref emulator_override,
            ref mut core_override,
            ..
        } = self.modal_state
        {
            if let Some(pos) = self.games.iter().position(|g| g.id == game_id) {
                let game = self.games[pos].clone();
                let platform_slug = self
                    .db
                    .get_platforms()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|p| p.id == game.platform_id)
                    .map(|p| p.slug)
                    .unwrap_or_default();

                let emu_name = if let Some(id) = *emulator_override {
                    self.db
                        .get_runners_for_platform(game.platform_id)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|r| r.id == id)
                        .map(|r| r.name)
                        .unwrap_or_else(|| "RetroArch".to_string())
                } else {
                    self.db
                        .get_runner_for_game(game.platform_id, game.folder_id, None)
                        .ok()
                        .flatten()
                        .map(|r| r.name)
                        .unwrap_or_else(|| "RetroArch".to_string())
                };

                let choices = crate::edit_game_details::EditGameFormHelper::get_core_choices(
                    &platform_slug,
                    &emu_name,
                );
                let new_core = crate::edit_game_details::EditGameFormHelper::cycle_core_choice(
                    &choices,
                    core_override.as_deref(),
                    backward,
                );
                *core_override = new_core.clone();
                let _ = self.db.set_game_core_override(game_id, new_core.as_deref());
                if let Some(g) = self.games.get_mut(pos) {
                    g.core_override = new_core.clone();
                }
                let choice_label = choices
                    .iter()
                    .find(|c| c.core_key == new_core)
                    .map(|c| c.display_label.clone())
                    .unwrap_or_else(|| "Default".to_string());
                self.status_msg = format!("Core assigned to game: {}", choice_label);
            }
        }
    }

    /// Platform-parameterized version used both from the main navigation
    /// (via `cycle_active_selector_for`) and from the Scan Folder form, where
    /// the target platform may differ from the currently selected one.
    pub fn cycle_active_emulator_for(&mut self, platform: &Platform, backward: bool) {
        let configured = self.configured_runners_for(platform.id);
        if configured.is_empty() {
            self.status_msg = format!(
                "No emulator configured for {}. Press [m] to configure one.",
                platform.name
            );
            return;
        }
        if configured.len() == 1 {
            self.status_msg = format!(
                "{} has only one configured emulator: {}.",
                platform.name, configured[0].name
            );
            return;
        }

        let active_id = self
            .db
            .get_active_runner_for_platform(platform.id)
            .ok()
            .flatten()
            .map(|r| r.id);
        let current_idx = active_id
            .and_then(|id| configured.iter().position(|r| r.id == id));
        let next_idx = cycle_index(current_idx, configured.len(), backward);
        let next = &configured[next_idx];

        if current_idx == Some(next_idx) {
            return;
        }
        match self.db.set_active_runner(platform.id, next.id) {
            Ok(_) => {
                self.status_msg =
                    format!("Active emulator for {}: {}", platform.name, next.name);
            }
            Err(err) => {
                self.status_msg = format!("Error changing active emulator: {}", err);
            }
        }
    }

    /// Platform-parameterized version used both from the main navigation and
    /// from the Scan Folder form.
    pub fn cycle_active_core_for(&mut self, platform: &Platform, backward: bool) {
        let Some(active) = self
            .db
            .get_active_runner_for_platform(platform.id)
            .ok()
            .flatten()
        else {
            return;
        };
        let cores = game_core::options::emulator_cores_for_platform(&active.name, &platform.slug);
        if cores.is_empty() {
            return;
        }

        let env_json = self
            .db
            .get_runner_env_by_name(&active.name)
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut env = game_core::options::from_env_json(&env_json);
        let current = env
            .active_core
            .clone()
            .or_else(|| game_core::options::emulator_default_core_for_platform(&active.name, &platform.slug));
        let Some(next) = game_core::options::next_core_key(&cores, current.as_deref(), backward)
        else {
            return;
        };
        env.active_core = Some(next.clone());
        let _ = self.db.update_runner_env_by_name(
            &active.name,
            Some(&game_core::options::to_env_json(&env)),
        );
        let label = game_core::options::emulator_core_label_for_platform(&active.name, &platform.slug, &next)
            .unwrap_or_else(|| next.clone());
        self.status_msg = format!("Core for {}: {}", active.name, label);
    }

    /// Drive the ◀ ▶ selector for a given platform: core-based active emulators
    /// cycle their nested "Núcleo" row, everything else cycles the emulator.
    /// Shared by the main navigation and the Scan Folder form.
    pub fn cycle_active_selector_for(&mut self, platform: &Platform, backward: bool) {
        let requires_core = self
            .db
            .get_active_runner_for_platform(platform.id)
            .ok()
            .flatten()
            .map(|r| game_core::options::emulator_requires_core_selection(&r.name))
            .unwrap_or(false);
        if requires_core {
            self.cycle_active_core_for(platform, backward);
        } else {
            self.cycle_active_emulator_for(platform, backward);
        }
    }

    /// Info rendered by the "Emulador activo" box of the selected platform:
    /// `(emulator name, Option<core label>)`. The core is only present when the
    /// active emulator requires core selection. `None` when the platform has no
    /// emulator at all.
    pub fn active_emulator_selector_info(&self) -> Option<(String, Option<String>)> {
        let platform = self.platforms.get(self.selected_platform_idx)?;
        self.active_emulator_selector_info_for(platform.id)
    }

    /// Platform-parameterized version used both from the main navigation and
    /// from the Scan Folder form.
    pub fn active_emulator_selector_info_for(
        &self,
        platform_id: i64,
    ) -> Option<(String, Option<String>)> {
        let active = self
            .db
            .get_active_runner_for_platform(platform_id)
            .ok()
            .flatten()?;
        let platform_slug = self
            .db
            .get_platforms()
            .ok()
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.id == platform_id)
            .map(|p| p.slug)
            .unwrap_or_default();
        let core_label = if game_core::options::emulator_requires_core_selection(&active.name) {
            let env_json = self
                .db
                .get_runner_env_by_name(&active.name)
                .ok()
                .flatten()
                .unwrap_or_default();
            let env = game_core::options::from_env_json(&env_json);
            let current = env
                .active_core
                .or_else(|| game_core::options::emulator_default_core_for_platform(&active.name, &platform_slug));
            current.and_then(|key| game_core::options::emulator_core_label_for_platform(&active.name, &platform_slug, &key))
        } else {
            None
        };
        Some((active.name.clone(), core_label))
    }

    /// Auto-switch: whenever the active emulator's executable is gone from disk
    /// (deleted via [m], [w], or manually), move the active marker to the first
    /// configured emulator of the same platform. Platforms with no configured
    /// emulator are left untouched.
    fn revalidate_active_emulators(&mut self) {
        let Ok(platforms) = self.db.get_platforms() else {
            return;
        };
        for platform in platforms {
            let Ok(Some(active)) = self.db.get_active_runner_for_platform(platform.id) else {
                continue;
            };
            let active_ok = active.executable_path.as_ref().is_some_and(|ex| {
                let ex = ex.trim();
                !ex.is_empty() && (runner::is_executable_command(ex) || Path::new(ex).exists())
            });
            if active_ok {
                continue;
            }
            let configured = self.configured_runners_for(platform.id);
            if let Some(candidate) = configured.iter().find(|r| {
                r.id != active.id
                    && r.executable_path.as_ref().is_some_and(|ex| {
                        let ex = ex.trim();
                        !ex.is_empty()
                            && (runner::is_executable_command(ex) || Path::new(ex).exists())
                    })
            }) {
                let _ = self.db.set_active_runner(platform.id, candidate.id);
            }
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

        self.load_local_covers_for_loaded_games();
        self.trigger_async_cover_fetch();
        self.trigger_auto_bulk_media_fetch();
    }

    pub fn load_local_covers_for_loaded_games(&mut self) {
        if self.games.is_empty() {
            return;
        }
        let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir().join("covers");
        let tx = self.cover_tx.clone();
        let manager = self.cover_manager.clone();

        for game in &self.games {
            let game_id = game.id;
            let local_cover = vec![
                media_dir.join(format!("{}.jpg", game_id)),
                media_dir.join(format!("{}.png", game_id)),
                media_dir.join(format!("{}.webp", game_id)),
            ]
            .into_iter()
            .find(|p| p.exists());

            if let Some(path) = local_cover {
                let tx_c = tx.clone();
                let manager_c = manager.clone();
                let path_c = path.clone();
                tokio::spawn(async move {
                    if let Some(protocol) = manager_c.load_protocol_from_file(&path_c) {
                        let _ = tx_c
                            .send(LoadedCoverEvent {
                                game_id,
                                media_type: "cover".to_string(),
                                protocol: Some(protocol),
                            })
                            .await;
                    }

                    if let Some(protocol_hb) = manager_c.load_halfblocks_protocol_from_file(&path_c)
                    {
                        let _ = tx_c
                            .send(LoadedCoverEvent {
                                game_id,
                                media_type: "cover_hb".to_string(),
                                protocol: Some(protocol_hb),
                            })
                            .await;
                    }
                });
            }
        }
    }

    pub fn trigger_auto_bulk_media_fetch(&mut self) {
        if self.games.is_empty() || self.download_progress.is_some() {
            return;
        }

        let api_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
        if api_key
            .as_ref()
            .map(|k| k.trim().is_empty())
            .unwrap_or(true)
        {
            return;
        }

        let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir().join("covers");
        let target_games: Vec<Game> = self
            .games
            .iter()
            .filter(|g| {
                if self.pending_cover_requests.contains(&g.id) {
                    return false;
                }
                let j = media_dir.join(format!("{}.jpg", g.id));
                let p = media_dir.join(format!("{}.png", g.id));
                let w = media_dir.join(format!("{}.webp", g.id));
                !j.exists() && !p.exists() && !w.exists()
            })
            .cloned()
            .collect();

        if target_games.is_empty() {
            return;
        }

        for g in &target_games {
            self.pending_cover_requests.insert(g.id);
        }

        let total_games = target_games.len();
        self.download_progress = Some(DownloadProgressState {
            runner_id: 0,
            runner_name: format!(
                "SteamGridDB Media (0/{}) - {}",
                total_games, target_games[0].title
            ),
            downloaded_bytes: 0,
            total_bytes: total_games as u64,
            percentage: 0.0,
            is_finished: false,
            error_msg: None,
        });

        self.status_msg = format!(
            "Auto-fetching SteamGridDB media for {} missing game(s)...",
            total_games
        );

        let (progress_tx, progress_rx) = mpsc::channel::<DownloadEvent>(100);
        self.download_rx = Some(progress_rx);
        let tx = self.cover_tx.clone();
        let key_str = api_key.unwrap();
        let manager = self.cover_manager.clone();

        tokio::spawn(async move {
            let client =
                std::sync::Arc::new(scraper::steamgriddb::SteamGridDBClient::new(Some(key_str)));
            let db_path = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                .join("tui_game_station")
                .join("game_station.db");

            let completed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(3));
            let mut tasks = Vec::new();

            for game in target_games {
                let sem = semaphore.clone();
                let client_c = client.clone();
                let db_path_c = db_path.clone();
                let tx_c = tx.clone();
                let progress_tx_c = progress_tx.clone();
                let manager_c = manager.clone();
                let counter_c = completed_count.clone();
                let total = total_games;

                tasks.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await;

                    let res = client_c
                        .download_all_media_for_game(Some(db_path_c), game.id, &game.title, false)
                        .await;

                    let protocol = match res {
                        Ok(ref media) => {
                            if let Some(ref path) = media.cover_path {
                                manager_c.load_protocol_from_file(path)
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    };

                    let _ = tx_c
                        .send(LoadedCoverEvent {
                            game_id: game.id,
                            media_type: "cover".to_string(),
                            protocol,
                        })
                        .await;

                    let finished_so_far =
                        counter_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    let item_title = format!(
                        "SteamGridDB Media ({}/{}) - {}",
                        finished_so_far, total, game.title
                    );

                    let _ = progress_tx_c
                        .send(DownloadEvent {
                            downloaded: finished_so_far as u64,
                            total: total as u64,
                            percentage: ((finished_so_far as f64 / total as f64) * 100.0),
                            finished: false,
                            error: None,
                            task_name: Some(item_title),
                        })
                        .await;
                }));
            }

            for t in tasks {
                let _ = t.await;
            }

            let _ = progress_tx
                .send(DownloadEvent {
                    downloaded: total_games as u64,
                    total: total_games as u64,
                    percentage: 100.0,
                    finished: true,
                    error: None,
                    task_name: Some(format!(
                        "SteamGridDB Media ({}/{} Completed)",
                        total_games, total_games
                    )),
                })
                .await;
        });
    }

    pub fn trigger_async_cover_fetch(&mut self) {
        if self.games.is_empty() || self.selected_game_idx >= self.games.len() {
            return;
        }

        if self.is_big_picture {
            self.preload_visible_covers();
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
            } else if let Some(appid) = appid.filter(|_| media_type_str == "cover") {
                SteamCoverResolver::resolve_cover(appid).await
            } else {
                let client = scraper::steamgriddb::SteamGridDBClient::new(db_key);
                let db_path = dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                    .join("tui_game_station")
                    .join("game_station.db");
                if let Ok(res) = client
                    .download_all_media_for_game(Some(db_path), game_id, &title, false)
                    .await
                {
                    match media_type_str.as_str() {
                        "banner" => res.banner_path,
                        "icon" => res.icon_path,
                        _ => res.cover_path,
                    }
                } else {
                    None
                }
            };

            let protocol = if let Some(path) = cover_path {
                manager.load_native_protocol_from_file(&path)
            } else {
                None
            };

            let _ = tx
                .send(LoadedCoverEvent {
                    game_id,
                    media_type: media_type_str,
                    protocol,
                })
                .await;
        });
    }

    pub fn preload_game_detail_media(&mut self) {
        if self.games.is_empty() || self.selected_game_idx >= self.games.len() {
            return;
        }
        let game = &self.games[self.selected_game_idx];
        let game_id = game.id;
        let title = game.title.clone();

        // Focused media (cover): native / high-fidelity, same pipeline as the
        // featured game in the carousel center.
        if !self
            .media_protocols
            .contains_key(&(game_id, "cover".to_string()))
        {
            let tx = self.cover_tx.clone();
            let manager = self.cover_manager.clone();
            let id = game_id;
            tokio::spawn(async move {
                let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();
                let target_dir = media_dir.join("covers");
                let local = ["jpg", "png", "webp"]
                    .into_iter()
                    .map(|e| target_dir.join(format!("{}.{}", id, e)))
                    .find(|p| p.exists());
                if let Some(path) = local {
                    if let Some(protocol) = manager.load_native_protocol_from_file(&path) {
                        let _ = tx
                            .send(LoadedCoverEvent {
                                game_id: id,
                                media_type: "cover".to_string(),
                                protocol: Some(protocol),
                            })
                            .await;
                    }
                }
            });
        }

        // Background media (banner, icon): halfblocks / low-fidelity, same
        // pipeline as the side games in the carousel. The cover is the ONLY
        // native (high-fidelity) image in this view, so icon and banner must
        // stay on the unicode halfblocks path to avoid two kitty/sixel slots.
        for (media_type, sub_dir, ext) in [
            ("banner", "banners", vec!["jpg", "png", "webp"]),
            ("icon", "icons", vec!["png", "jpg", "webp"]),
        ] {
            let media_key = format!("{}_hb", media_type);
            if self
                .media_protocols
                .contains_key(&(game_id, media_key.clone()))
            {
                continue;
            }
            let db_status = self.db.get_media_status(game_id, media_type).ok().flatten();
            if db_status.as_deref() == Some("not_found") {
                continue;
            }

            let tx = self.cover_tx.clone();
            let manager = self.cover_manager.clone();
            let db_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
            let media_type_s = media_type.to_string();
            let sub_dir_s = sub_dir.to_string();
            let exts = ext.clone();
            let title_c = title.clone();
            let key = media_key;

            tokio::spawn(async move {
                let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();
                let target_dir = media_dir.join(&sub_dir_s);
                let local = exts
                    .into_iter()
                    .map(|e| target_dir.join(format!("{}.{}", game_id, e)))
                    .find(|p| p.exists());

                let path = if let Some(p) = local {
                    Some(p)
                } else {
                    let client = scraper::steamgriddb::SteamGridDBClient::new(db_key);
                    let db_path = dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                        .join("tui_game_station")
                        .join("game_station.db");
                    if let Ok(res) = client
                        .download_all_media_for_game(Some(db_path), game_id, &title_c, false)
                        .await
                    {
                        match media_type_s.as_str() {
                            "banner" => res.banner_path,
                            "icon" => res.icon_path,
                            _ => None,
                        }
                    } else {
                        None
                    }
                };

                let protocol = path.and_then(|p| match media_type_s.as_str() {
                    "banner" => manager.load_halfblocks_banner_protocol_from_file(&p),
                    "icon" => manager.load_halfblocks_protocol_from_file(&p),
                    _ => manager.load_native_protocol_from_file(&p),
                });
                let _ = tx
                    .send(LoadedCoverEvent {
                        game_id,
                        media_type: key,
                        protocol,
                    })
                    .await;
            });
        }
    }

    pub fn preload_visible_covers(&mut self) {
        if self.games.is_empty() {
            return;
        }
        let sel = self.selected_game_idx;
        let mut range = Vec::new();
        if sel > 0 {
            range.push(sel - 1);
        }
        if sel + 1 < self.games.len() {
            range.push(sel + 1);
        }

        for idx in range {
            let game_id = self.games[idx].id;
            let title = self.games[idx].title.clone();
            let appid = self.games[idx].steam_appid;

            if self
                .media_protocols
                .contains_key(&(game_id, "cover_hb".to_string()))
            {
                continue;
            }
            if self.pending_cover_requests.contains(&game_id) {
                continue;
            }

            self.pending_cover_requests.insert(game_id);
            let tx = self.cover_tx.clone();
            let manager = self.cover_manager.clone();
            let db_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();

            tokio::spawn(async move {
                let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();
                let target_dir = media_dir.join("covers");
                let local_cover = vec![
                    target_dir.join(format!("{}.jpg", game_id)),
                    target_dir.join(format!("{}.png", game_id)),
                    target_dir.join(format!("{}.webp", game_id)),
                ]
                .into_iter()
                .find(|p| p.exists());

                let cover_path = if let Some(path) = local_cover {
                    Some(path)
                } else if let Some(appid) = appid {
                    SteamCoverResolver::resolve_cover(appid).await
                } else {
                    let client = scraper::steamgriddb::SteamGridDBClient::new(db_key);
                    let db_path = dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                        .join("tui_game_station")
                        .join("game_station.db");
                    if let Ok(res) = client
                        .download_all_media_for_game(Some(db_path), game_id, &title, false)
                        .await
                    {
                        res.cover_path
                    } else {
                        None
                    }
                };

                let protocol = if let Some(path) = cover_path {
                    manager.load_halfblocks_protocol_from_file(&path)
                } else {
                    None
                };
                let _ = tx
                    .send(LoadedCoverEvent {
                        game_id,
                        media_type: "cover_hb".to_string(),
                        protocol,
                    })
                    .await;
            });
        }
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
            if let Some(proto) = loaded.protocol {
                self.media_protocols
                    .insert((loaded.game_id, loaded.media_type), proto);
            }
        }

        // Receive loaded preview events for Visual Media Selector
        while let Ok(loaded) = self.preview_rx.try_recv() {
            if self.visual_preview_url.as_deref() == Some(&loaded.url) {
                self.visual_preview_protocol = Some(loaded.protocol);
                self.visual_preview_loading = false;
            }
        }

        // Receive live scan progress events non-blocking
        let mut scan_done = false;
        let mut scan_added = 0;
        if let Some(ref mut rx) = self.scan_rx {
            while let Ok(evt) = rx.try_recv() {
                if evt.finished {
                    scan_done = true;
                    scan_added = evt.added_count;
                } else {
                    self.download_progress = Some(DownloadProgressState {
                        runner_id: 0,
                        runner_name: format!("Scanning: {}", evt.current_title),
                        downloaded_bytes: evt.current as u64,
                        total_bytes: evt.total as u64,
                        percentage: (evt.current as f64 / evt.total.max(1) as f64) * 100.0,
                        is_finished: false,
                        error_msg: None,
                    });
                    self.status_msg = format!(
                        "[Identifying {}/{}] {}",
                        evt.current, evt.total, evt.current_title
                    );
                }
            }
        }
        if scan_done {
            self.download_progress = None;
            self.scan_rx = None;
            self.status_msg = format!(
                "[OK] Scan completed: {} ROMs imported/updated.",
                scan_added
            );
            self.load_platforms();
        }

        if let Some(ref mut rx) = self.update_rx {
            if let Ok(res) = rx.try_recv() {
                self.update_rx = None;
                match res {
                    Ok(Some(up)) => {
                        self.status_msg = format!("[Updater] New version: v{}", up.latest_version);
                        self.modal_state = ModalState::UpdateAvailable {
                            new_version: up.latest_version,
                            download_url: up.download_url,
                            release_notes: up.release_notes,
                        };
                    }
                    Ok(None) => {
                        if self.is_manual_update_check {
                            self.status_msg =
                                format!("[OK] App is up to date (v{}).", env!("CARGO_PKG_VERSION"));
                            self.show_toast(
                                format!("[OK] Up to date (v{}).", env!("CARGO_PKG_VERSION")),
                                crate::toast::ToastKind::Success,
                            );
                        }
                    }
                    Err(e) => {
                        if self.is_manual_update_check {
                            self.status_msg =
                                format!("[Updater Error] Failed to check for updates: {}", e);
                        }
                    }
                }
            }
        }

        self.poll_gamepad_events().await;
    }

    /// Poll for a background game/emulator having exited; when it has, restore
    /// the TUI window, flush stale input and open the post-game drain.
    pub async fn check_game_exit(&mut self) {
        let Some(rx) = &mut self.game_exit_rx else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.game_exit_rx = None;
        self.running_game = None;

        crate::window_helper::restore_active_window();
        self.resume_input_after_game();

        match result {
            Ok(status) => {
                self.status_msg = match status.code() {
                    Some(code) => format!("Game exited with code: {code}"),
                    None => "Game exited (terminated by signal)".to_string(),
                };
            }
            Err(err) => {
                self.status_msg = format!("[Error] {}", err);
            }
        }
        self.needs_terminal_clear = true;
    }

    /// Launch a game in the background, keeping the TUI alive so the running
    /// indicator and the force-close action stay usable while the game runs.
    fn start_game_background(&mut self, game: Game, runner: Option<Runner>) {
        self.suspend_gamepad_input();
        crate::window_helper::minimize_active_window();
        self.running_game = Some(RunningGame {
            title: game.title.clone(),
            runner_name: runner.as_ref().map(|r| r.name.clone()),
            started_at: Instant::now(),
        });
        let (tx, rx) = mpsc::channel::<GameExitResult>(1);
        self.game_exit_rx = Some(rx);
        tokio::spawn(async move {
            let result = GameRunner::launch_game(&game, runner.as_ref()).await;
            let _ = tx.send(result.map_err(|e| e.to_string())).await;
        });
    }

    /// Launch an emulator standalone (no ROM) in the background.
    fn start_standalone_background(&mut self, runner: Runner, title: String) {
        self.suspend_gamepad_input();
        crate::window_helper::minimize_active_window();
        self.running_game = Some(RunningGame {
            runner_name: Some(runner.name.clone()),
            title: title.clone(),
            started_at: Instant::now(),
        });
        let (tx, rx) = mpsc::channel::<GameExitResult>(1);
        self.game_exit_rx = Some(rx);
        tokio::spawn(async move {
            let result = GameRunner::launch_standalone(&runner).await;
            let _ = tx.send(result.map_err(|e| e.to_string())).await;
        });
    }

    /// Stop forwarding gamepad events while a game/emulator has focus, so
    /// gameplay button presses don't pile up in the channel and get replayed
    /// as TUI commands when the game closes.
    pub fn suspend_gamepad_input(&self) {
        if let Some(ref flag) = self.gamepad_suspended {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// True while the TUI is discarding stale input right after a game closes.
    pub fn is_in_input_grace(&self) -> bool {
        self.input_safe_mode == InputSafeMode::Locked
    }

    /// Called once a game has exited: drop any keyboard/gamepad events queued
    /// while the game had focus, resume gamepad forwarding, and open a
    /// quiet-based drain so late/held input can't trigger a phantom relaunch.
    pub fn resume_input_after_game(&mut self) {
        let mut discarded_keys = 0u32;
        while let Ok(true) = crossterm::event::poll(Duration::ZERO) {
            if crossterm::event::read().is_ok() {
                discarded_keys += 1;
            } else {
                break;
            }
        }

        let mut discarded_gamepad = 0u32;
        if let Some(ref rx) = self.gamepad_rx {
            while let Ok(_evt) = rx.try_recv() {
                discarded_gamepad += 1;
            }
        }

        if let Some(ref flag) = self.gamepad_suspended {
            flag.store(false, Ordering::Relaxed);
        }

        tracing::info!(
            "[resume] flushed queued input: {} keyboard events, {} gamepad events; starting quiet-based drain",
            discarded_keys,
            discarded_gamepad
        );

        self.input_safe_mode = InputSafeMode::Locked;
        let now = Instant::now();
        self.input_drain_started_at = Some(now);
        self.input_drain_last_event_at = Some(now);
        self.log_next_input = true;
    }

    /// Main-loop helper: while the post-game drain is open, discard any input
    /// that still arrives (held buttons, focus-transfer leftovers, a leaked key
    /// that would relaunch/quit) instead of dispatching it. The drain ends once
    /// the input stream has been quiet for `INPUT_DRAIN_QUIET`, or after
    /// `INPUT_DRAIN_MAX` at the latest. Returns true while still draining.
    pub fn drain_stale_input(&mut self) -> bool {
        if self.input_safe_mode != InputSafeMode::Locked {
            return false;
        }
        let started = self.input_drain_started_at.unwrap_or_else(Instant::now);
        let now = Instant::now();

        let mut discarded = 0u32;
        while let Ok(true) = crossterm::event::poll(Duration::ZERO) {
            if crossterm::event::read().is_ok() {
                discarded += 1;
            } else {
                break;
            }
        }
        if let Some(ref rx) = self.gamepad_rx {
            while let Ok(_evt) = rx.try_recv() {
                discarded += 1;
            }
        }
        if discarded > 0 {
            self.input_drain_last_event_at = Some(Instant::now());
            tracing::info!("[resume] drain: discarded {discarded} late input events");
        }

        let last = self.input_drain_last_event_at.unwrap_or(started);
        if now.duration_since(last) >= INPUT_DRAIN_QUIET {
            self.input_safe_mode = InputSafeMode::Active;
            self.input_drain_started_at = None;
            self.input_drain_last_event_at = None;
            tracing::info!(
                "[resume] drain complete: input quiet for {:?}",
                now.duration_since(last)
            );
            return false;
        }
        if now.duration_since(started) >= INPUT_DRAIN_MAX {
            self.input_safe_mode = InputSafeMode::Active;
            self.input_drain_started_at = None;
            self.input_drain_last_event_at = None;
            tracing::warn!(
                "[resume] drain cut short after {:?} (max {:?}); input may still be stale",
                now.duration_since(started),
                INPUT_DRAIN_MAX
            );
            return false;
        }
        true
    }

    pub async fn poll_gamepad_events(&mut self) {
        let mut events = Vec::new();

        if let Some(ref mut rx) = self.gamepad_rx {
            while let Ok(evt) = rx.try_recv() {
                events.push(evt);
            }
        }

        if self.is_in_input_grace() {
            if !events.is_empty() {
                tracing::info!(
                    "[resume] grace window: discarded {} late gamepad events",
                    events.len()
                );
            }
            return;
        }

        for evt in events {
            match evt {
                crate::gamepad::GamepadEvent::Connected { name } => {
                    self.active_gamepad_name = Some(name.clone());
                    self.active_input_source = InputSource::Gamepad(name.clone());
                    self.show_toast(
                        format!("[Controller] Connected: {}", name),
                        crate::toast::ToastKind::Success,
                    );
                }
                crate::gamepad::GamepadEvent::Disconnected { name } => {
                    if let Some(ref current) = self.active_gamepad_name {
                        if current == &name {
                            self.active_gamepad_name = None;
                            self.active_input_source = InputSource::Keyboard;
                        }
                    }
                    self.show_toast(
                        format!("[Controller] Disconnected: {}", name),
                        crate::toast::ToastKind::Success,
                    );
                }
                crate::gamepad::GamepadEvent::Action { action, name } => {
                    if self.log_next_input {
                        self.log_next_input = false;
                        tracing::info!(
                            "[resume] first post-resume input: gamepad action {action:?}"
                        );
                    }
                    self.active_gamepad_name = Some(name.clone());
                    self.active_input_source = InputSource::Gamepad(name);
                    self.handle_gamepad_action(action).await;
                }
            }
        }
    }

    pub async fn handle_gamepad_action(&mut self, action: crate::gamepad::GamepadAction) {
        if self.big_picture_in_detail {
            match action {
                crate::gamepad::GamepadAction::Confirm => {
                    if self.detail_action_idx == 0 {
                        self.update(Action::LaunchGame).await;
                    } else {
                        self.show_toast(
                            "This action will be available soon.",
                            crate::toast::ToastKind::Info,
                        );
                    }
                }
                crate::gamepad::GamepadAction::Back => {
                    self.update(Action::CloseGameDetail).await;
                }
                crate::gamepad::GamepadAction::Left => {
                    self.update(Action::DetailPrevAction).await;
                }
                crate::gamepad::GamepadAction::Right => {
                    self.update(Action::DetailNextAction).await;
                }
                _ => {}
            }
            return;
        }

        // Dedicated controller navigation for the Emulator Options popup.
        if matches!(
            self.modal_state,
            ModalState::ManageRunnersStep2Config { .. }
        ) {
            self.handle_runner_step2_gamepad(action).await;
            return;
        }
        match action {
            crate::gamepad::GamepadAction::Up => {
                if self.modal_state != ModalState::None {
                    if matches!(
                        self.modal_state,
                        ModalState::ScanFolderForm { .. } | ModalState::AddFolderScanForm { .. }
                    ) {
                        self.update(Action::ModalPrevField).await;
                    } else {
                        self.update(Action::ModalSelectPrev).await;
                    }
                } else if self.is_big_picture {
                    self.update(Action::PrevGame).await;
                } else {
                    match self.focused_pane {
                        FocusedPane::Platforms => self.update(Action::PrevPlatform).await,
                        FocusedPane::Games => self.update(Action::PrevGame).await,
                        _ => {}
                    }
                }
            }
            crate::gamepad::GamepadAction::Down => {
                if self.modal_state != ModalState::None {
                    if matches!(
                        self.modal_state,
                        ModalState::ScanFolderForm { .. } | ModalState::AddFolderScanForm { .. }
                    ) {
                        self.update(Action::ModalNextField).await;
                    } else {
                        self.update(Action::ModalSelectNext).await;
                    }
                } else if self.is_big_picture {
                    self.update(Action::NextGame).await;
                } else {
                    match self.focused_pane {
                        FocusedPane::Platforms => self.update(Action::NextPlatform).await,
                        FocusedPane::Games => self.update(Action::NextGame).await,
                        _ => {}
                    }
                }
            }
            crate::gamepad::GamepadAction::Left => {
                if self.modal_state != ModalState::None {
                    match &mut self.modal_state {
                        ModalState::VisualMediaSelector { .. } => {
                            self.update(Action::VisualMediaNavLeft).await;
                        }
                        ModalState::EditCustomArgsInput { .. } => {
                            self.update(Action::FormNavLeft).await;
                        }
                        ModalState::ConfirmDeleteGame { .. } => {
                            self.update(Action::ToggleConfirmDeleteOption).await;
                        }
                        ModalState::ConfirmDeleteRunner { .. } => {
                            self.update(Action::ToggleConfirmDeleteRunnerOption).await;
                        }

                        ModalState::AddFolderScanForm {
                            ref platform,
                            add_emulator_id,
                            selected_field,
                            ..
                        } => {
                            let dat = scan_folder_supports_dat(&platform.slug);
                            let emu_idx = scan_folder_add_emu_idx(dat);
                            let has_core = scan_folder_add_has_core(&self.db, platform.id, *add_emulator_id);
                            let core_idx = scan_folder_add_core_idx(dat);
                            if *selected_field == emu_idx {
                                self.cycle_add_folder_emulator(true);
                            } else if has_core && *selected_field == core_idx {
                                self.cycle_add_folder_core(true);
                            }
                        }
                        ModalState::ScanFolderForm {
                            ref platform,
                            ref folders,
                            selected_index,
                        } => {
                            let items = build_scan_folder_items(&self.db, platform.id, folders);
                            match items.get(*selected_index) {
                                Some(ScanFolderItem::FolderEmulator(_)) => {
                                    self.update(Action::CycleFolderEmulator(true)).await;
                                }
                                Some(ScanFolderItem::FolderCore(_)) => {
                                    self.update(Action::CycleFolderCore(true)).await;
                                }
                                _ => {}
                            }
                        }
                        ModalState::EditGameForm {
                            game_id,
                            game_type: PlatformType::Emulator,
                            emulator_override,
                            selected_field,
                            ..
                        } => {
                            let platform_id = self.games.iter().find(|g| g.id == *game_id).map(|g| g.platform_id).unwrap_or(0);
                            let has_core = scan_folder_add_has_core(&self.db, platform_id, *emulator_override);
                            if *selected_field == 2 {
                                self.cycle_edit_game_emulator(true);
                            } else if has_core && *selected_field == 3 {
                                self.cycle_edit_game_core(true);
                            }
                        }
                        ModalState::ConfirmDeleteFolder { .. } => {
                            self.update(Action::ToggleConfirmDeleteFolderOption).await;
                        }
                        _ => {}
                    }
                } else if self.is_big_picture
                    || (self.focused_pane == FocusedPane::Games
                        && self.view_mode == ViewMode::CoverCard)
                {
                    self.update(Action::PrevGame).await;
                } else if self.focused_pane == FocusedPane::Platforms {
                    self.update(Action::CycleActiveEmulatorPrev).await;
                }
            }
            crate::gamepad::GamepadAction::Right => {
                if self.modal_state != ModalState::None {
                    match &mut self.modal_state {
                        ModalState::VisualMediaSelector { .. } => {
                            self.update(Action::VisualMediaNavRight).await;
                        }
                        ModalState::EditCustomArgsInput { .. } => {
                            self.update(Action::FormNavRight).await;
                        }
                        ModalState::ConfirmDeleteGame { .. } => {
                            self.update(Action::ToggleConfirmDeleteOption).await;
                        }
                        ModalState::ConfirmDeleteRunner { .. } => {
                            self.update(Action::ToggleConfirmDeleteRunnerOption).await;
                        }

                        ModalState::AddFolderScanForm {
                            ref platform,
                            add_emulator_id,
                            selected_field,
                            ..
                        } => {
                            let dat = scan_folder_supports_dat(&platform.slug);
                            let emu_idx = scan_folder_add_emu_idx(dat);
                            let has_core = scan_folder_add_has_core(&self.db, platform.id, *add_emulator_id);
                            let core_idx = scan_folder_add_core_idx(dat);
                            if *selected_field == emu_idx {
                                self.cycle_add_folder_emulator(false);
                            } else if has_core && *selected_field == core_idx {
                                self.cycle_add_folder_core(false);
                            }
                        }
                        ModalState::ScanFolderForm {
                            ref platform,
                            ref folders,
                            selected_index,
                        } => {
                            let items = build_scan_folder_items(&self.db, platform.id, folders);
                            match items.get(*selected_index) {
                                Some(ScanFolderItem::FolderEmulator(_)) => {
                                    self.update(Action::CycleFolderEmulator(false)).await;
                                }
                                Some(ScanFolderItem::FolderCore(_)) => {
                                    self.update(Action::CycleFolderCore(false)).await;
                                }
                                _ => {}
                            }
                        }
                        ModalState::EditGameForm {
                            game_id,
                            game_type: PlatformType::Emulator,
                            emulator_override,
                            selected_field,
                            ..
                        } => {
                            let platform_id = self.games.iter().find(|g| g.id == *game_id).map(|g| g.platform_id).unwrap_or(0);
                            let has_core = scan_folder_add_has_core(&self.db, platform_id, *emulator_override);
                            if *selected_field == 2 {
                                self.cycle_edit_game_emulator(false);
                            } else if has_core && *selected_field == 3 {
                                self.cycle_edit_game_core(false);
                            }
                        }
                        ModalState::ConfirmDeleteFolder { .. } => {
                            self.update(Action::ToggleConfirmDeleteFolderOption).await;
                        }
                        _ => {}
                    }
                } else if self.is_big_picture
                    || (self.focused_pane == FocusedPane::Games
                        && self.view_mode == ViewMode::CoverCard)
                {
                    self.update(Action::NextGame).await;
                } else if self.focused_pane == FocusedPane::Platforms {
                    self.update(Action::CycleActiveEmulatorNext).await;
                }
            }
            crate::gamepad::GamepadAction::Confirm => {
                if self.modal_state != ModalState::None {
                    match self.modal_state.clone() {
                        ModalState::About => {
                            self.update(Action::CheckForUpdates { silent: false }).await;
                        }
                        ModalState::UpdateAvailable {
                            download_url,
                            new_version,
                            ..
                        } => {
                            self.update(Action::StartAppUpdate {
                                download_url,
                                new_version,
                            })
                            .await;
                        }
                        ModalState::AppSettings {
                            selected_field,
                            is_editing_api_key,
                            ..
                        } => {
                            if selected_field == 0 {
                                if !is_editing_api_key {
                                    if let ModalState::AppSettings {
                                        ref mut is_editing_api_key,
                                        ..
                                    } = self.modal_state
                                    {
                                        *is_editing_api_key = true;
                                    }
                                }
                            } else if selected_field == 1 {
                                self.update(Action::ResetRunnerConfig).await;
                            } else if selected_field == 2 {
                                self.modal_state = ModalState::About;
                            } else if selected_field == 3 {
                                self.update(Action::CheckForUpdates { silent: false }).await;
                            } else if selected_field == 4 {
                                self.modal_state = ModalState::None;
                            }
                        }
                        ModalState::ConfirmDeleteGame { .. } => {
                            self.update(Action::ConfirmDeleteGameExecution).await;
                        }
                        ModalState::ConfirmDeleteFolder { .. } => {
                            self.update(Action::ConfirmDeleteFolderExecution).await;
                        }
                        ModalState::ScanFolderForm { .. } => {
                            self.handle_scan_form_enter().await;
                        }
                        ModalState::AddFolderScanForm { .. } => {
                            self.handle_add_scan_form_enter().await;
                        }
                        ModalState::EditCustomArgsInput { .. } => {
                            self.update(Action::SaveCustomArgsInput).await;
                        }
                        ModalState::VisualMediaSelector { .. } => {
                            self.update(Action::ApplyVisualMediaSelection).await;
                        }
                        ModalState::ProtonDownloader { .. } => {
                            self.update(Action::StartProtonDownload).await;
                        }
                        ModalState::PlatformSelector { .. } => {
                            self.update(Action::ConfirmPlatformSelectorModal).await;
                        }
                        ModalState::WelcomeWizard {
                            ref sgdb_api_key, ..
                        } => {
                            let key = sgdb_api_key.clone();
                            self.finish_welcome_wizard(&key);
                        }
                        _ => {}
                    }
                } else if self.is_big_picture {
                    self.update(Action::OpenGameDetail).await;
                } else {
                    match self.focused_pane {
                        FocusedPane::Platforms => {
                            self.focused_pane = FocusedPane::Games;
                        }
                        FocusedPane::Games => {
                            self.update(Action::LaunchGame).await;
                        }
                        _ => {}
                    }
                }
            }
            crate::gamepad::GamepadAction::Back => {
                if self.modal_state != ModalState::None {
                    self.update(Action::CloseModal).await;
                } else if self.is_big_picture {
                    self.update(Action::ToggleBigPictureMode).await;
                } else if self.focused_pane == FocusedPane::Games {
                    self.focused_pane = FocusedPane::Platforms;
                }
            }
            crate::gamepad::GamepadAction::ToggleViewMode => {
                if self.modal_state == ModalState::None {
                    if self.is_big_picture {
                        self.update(Action::OpenPlatformSelectorModal).await;
                    } else {
                        self.update(Action::ToggleViewMode).await;
                    }
                }
            }
            crate::gamepad::GamepadAction::ToggleSelectGame => {
                if self.modal_state == ModalState::None && !self.is_big_picture {
                    self.update(Action::ToggleSelectGame).await;
                } else if matches!(self.modal_state, ModalState::ScanFolderForm { .. }) {
                    self.update(Action::ToggleSelectFolder).await;
                }
            }
            crate::gamepad::GamepadAction::DeleteSelected => {
                if self.modal_state == ModalState::None
                    && !self.is_big_picture
                    && !self.games.is_empty()
                {
                    self.update(Action::OpenConfirmDeleteModal).await;
                } else if matches!(self.modal_state, ModalState::ScanFolderForm { .. }) {
                    self.update(Action::OpenConfirmDeleteFolder).await;
                }
            }
            crate::gamepad::GamepadAction::NextTab => {
                if self.modal_state != ModalState::None {
                    if let ModalState::VisualMediaSelector { .. } = self.modal_state {
                        self.update(Action::SwitchVisualMediaTab).await;
                    } else if matches!(self.modal_state, ModalState::ScanFolderForm { .. }) {
                        self.update(Action::SwitchScanFolderPane).await;
                    }
                } else {
                    self.update(Action::NextPlatform).await;
                }
            }
            crate::gamepad::GamepadAction::PrevTab => {
                if self.modal_state != ModalState::None {
                    if let ModalState::VisualMediaSelector { .. } = self.modal_state {
                        self.update(Action::SwitchVisualMediaTabPrev).await;
                    } else if matches!(self.modal_state, ModalState::ScanFolderForm { .. }) {
                        self.update(Action::SwitchScanFolderPane).await;
                    }
                } else {
                    self.update(Action::PrevPlatform).await;
                }
            }
            crate::gamepad::GamepadAction::ToggleBigPicture => {
                if self.modal_state == ModalState::None {
                    self.update(Action::ToggleBigPictureMode).await;
                }
            }
            crate::gamepad::GamepadAction::OpenMenu => {
                if self.modal_state == ModalState::None {
                    self.update(Action::OpenSettingsModal).await;
                }
            }
        }
    }

    /// Controller navigation for the Emulator Options popup (Step 2 config).
    async fn handle_runner_step2_gamepad(&mut self, action: crate::gamepad::GamepadAction) {
        use crate::gamepad::GamepadAction;
        match action {
            GamepadAction::Up => {
                if let ModalState::ManageRunnersStep2Config {
                    ref options,
                    ref mut selected_row,
                    ..
                } = self.modal_state
                {
                    let total = options.len() + 3;
                    *selected_row = if *selected_row == 0 {
                        total - 1
                    } else {
                        *selected_row - 1
                    };
                }
            }
            GamepadAction::Down => {
                if let ModalState::ManageRunnersStep2Config {
                    ref options,
                    ref mut selected_row,
                    ..
                } = self.modal_state
                {
                    let total = options.len() + 3;
                    *selected_row = (*selected_row + 1) % total;
                }
            }
            GamepadAction::Left | GamepadAction::Right => {
                let backward = matches!(action, GamepadAction::Left);
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ref exe_path_input,
                    ref options,
                    ref mut option_values,
                    ref selected_row,
                    ..
                } = self.modal_state
                {
                    if *selected_row == 0 {
                        if let ModalState::ManageRunnersStep2Config {
                            ref exe_path_input,
                            ref mut cursor_pos,
                            ..
                        } = self.modal_state
                        {
                            if backward {
                                if *cursor_pos > 0 {
                                    *cursor_pos -= 1;
                                }
                            } else if *cursor_pos < exe_path_input.len() {
                                *cursor_pos += 1;
                            }
                        }
                    } else if *selected_row >= 1 && *selected_row <= options.len() {
                        cycle_runner_option(options, option_values, *selected_row - 1, backward);
                    } else if *selected_row == options.len() + 1 {
                        if let ModalState::ManageRunnersStep2Config {
                            ref custom_args,
                            ref mut cursor_pos,
                            ..
                        } = self.modal_state
                        {
                            if backward {
                                if *cursor_pos > 0 {
                                    *cursor_pos -= 1;
                                }
                            } else if *cursor_pos < custom_args.len() {
                                *cursor_pos += 1;
                            }
                        }
                    } else {
                        let has_executable = !exe_path_input.trim().is_empty()
                            && (runner::is_executable_command(exe_path_input.trim())
                                || std::path::Path::new(exe_path_input.trim()).exists());
                        let download_url = runner_info.download_url.is_some();
                        let mut total_btns = 2;
                        if download_url {
                            total_btns += 1;
                        }
                        if has_executable {
                            total_btns += 2;
                        }
                        if let ModalState::ManageRunnersStep2Config {
                            ref mut selected_action_idx,
                            ..
                        } = self.modal_state
                        {
                            if backward {
                                if *selected_action_idx > 0 {
                                    *selected_action_idx -= 1;
                                }
                            } else if *selected_action_idx + 1 < total_btns {
                                *selected_action_idx += 1;
                            }
                        }
                    }
                }
            }
            GamepadAction::Confirm => {
                let snapshot = match self.modal_state.clone() {
                    ModalState::ManageRunnersStep2Config {
                        ref runner_info,
                        ref exe_path_input,
                        ref options,
                        selected_row,
                        selected_action_idx,
                        ..
                    } => Some((
                        runner_info.clone(),
                        exe_path_input.clone(),
                        options.clone(),
                        selected_row,
                        selected_action_idx,
                    )),
                    _ => None,
                };
                let Some((runner_info, exe_path_input, options, selected_row, selected_action_idx)) =
                    snapshot
                else {
                    return;
                };
                let has_executable = !exe_path_input.trim().is_empty()
                    && (runner::is_executable_command(exe_path_input.trim())
                        || std::path::Path::new(exe_path_input.trim()).exists());

                if selected_row == 0 {
                    if exe_path_input.trim().is_empty() {
                        self.update(Action::OpenFilePicker).await;
                    } else if let ModalState::ManageRunnersStep2Config {
                        ref mut selected_row,
                        ref options,
                        ..
                    } = self.modal_state
                    {
                        *selected_row = options.len() + 2;
                    }
                } else if selected_row >= 1 && selected_row <= options.len() {
                    if let ModalState::ManageRunnersStep2Config {
                        ref options,
                        ref mut option_values,
                        ref selected_row,
                        ..
                    } = self.modal_state
                    {
                        cycle_runner_option(options, option_values, selected_row - 1, false);
                    }
                } else if selected_row == options.len() + 1 {
                    self.update(Action::OpenCustomArgsEditor).await;
                } else {
                    let mut actions = vec!["browse"];
                    if runner_info.download_url.is_some() {
                        actions.push("download");
                    }
                    actions.push("save");
                    if has_executable {
                        actions.push("open");
                    }
                    if has_executable {
                        actions.push("delete");
                    }
                    let act = actions.get(selected_action_idx).copied().unwrap_or("save");
                    match act {
                        "browse" => self.update(Action::OpenFilePicker).await,
                        "download" => self.update(Action::StartRunnerDownload).await,
                        "save" => self.update(Action::SaveRunnerConfig).await,
                        "open" => self.update(Action::OpenRunnerStandalone).await,
                        "toggle_active" => self.update(Action::ToggleRunnerActiveState).await,
                        "delete" => self.update(Action::OpenConfirmDeleteRunnerModal).await,
                        _ => {}
                    }
                }
            }
            GamepadAction::Back => {
                self.update(Action::CloseModal).await;
            }
            _ => {}
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
                1 => covers
                    .get(selected_cover_idx)
                    .map(|c| c.thumb.as_ref().unwrap_or(&c.url).clone()),
                2 => banners
                    .get(selected_banner_idx)
                    .map(|b| b.thumb.as_ref().unwrap_or(&b.url).clone()),
                3 => icons
                    .get(selected_icon_idx)
                    .map(|i| i.thumb.as_ref().unwrap_or(&i.url).clone()),
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
                    if let Some(protocol) = self.cover_manager.load_protocol_from_file(&cache_path)
                    {
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
                    if client
                        .download_file_to_path(&url_to_fetch, &cache_path)
                        .await
                        .is_ok()
                    {
                        if let Some(protocol) = manager.load_protocol_from_file(&cache_path) {
                            let _ = tx
                                .send(LoadedPreviewEvent {
                                    url: url_to_fetch,
                                    protocol,
                                })
                                .await;
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
        // Guardián central (TAREA 5): mientras un juego está corriendo, la
        // única interacción válida con el launcher es forzarlo a cerrar. Todas
        // las fuentes de input (teclado, gamepad — incluso uno conectado en
        // caliente durante la partida — y ratón) convergen aquí, así que este
        // es el único punto que necesita filtrar qué se ejecuta.
        if self.running_game.is_some() && !is_action_allowed_while_game_running(&action) {
            tracing::info!("acción ignorada mientras un juego está en ejecución: {action:?}");
            return;
        }
        // Input-safe-mode guard (TAREA 3): while the post-game drain is open,
        // a stale key/button must never quit, relaunch or delete anything.
        if self.input_safe_mode == InputSafeMode::Locked && is_destructive_action(&action) {
            tracing::warn!("acción destructiva bloqueada durante input-safe-mode: {action:?}");
            return;
        }
        match action {
            Action::Quit => {
                if self.running_game.is_some() {
                    self.status_msg = "A game is currently running. Close it or press [F] to force close before quitting.".to_string();
                    return;
                }
                if self.modal_state != ModalState::None {
                    self.modal_state = ModalState::None;
                } else {
                    self.should_quit = true;
                }
            }
            Action::ForceCloseGame => {
                if self.running_game.is_none() {
                    self.status_msg = "No game is currently running to close.".to_string();
                    return;
                }
                match GameRunner::force_close_current_game() {
                    Ok(summary) => {
                        self.status_msg = format!("[OK] {}", summary);
                        self.show_toast(summary, crate::toast::ToastKind::Warning);
                    }
                    Err(err) => {
                        self.status_msg = format!("[Error] {}", err);
                    }
                }
                self.needs_terminal_clear = true;
            }
            Action::TogglePane => {
                if self.modal_state == ModalState::None {
                    self.focused_pane = match self.focused_pane {
                        FocusedPane::Search => FocusedPane::Platforms,
                        FocusedPane::Platforms => FocusedPane::Games,
                        FocusedPane::Games => FocusedPane::Search,
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
                    ViewMode::CoverCard => "[View Mode] Cover Cards (Vertical Poster)".to_string(),
                    ViewMode::BannerCard => "[View Mode] Hero Banners (Horizontal)".to_string(),
                    ViewMode::IconCard => "[View Mode] Square Icons".to_string(),
                    ViewMode::Table => "[View Mode] Detailed Table List".to_string(),
                };
                self.trigger_async_cover_fetch();
            }
            Action::ToggleBigPictureMode => {
                self.is_big_picture = !self.is_big_picture;
                self.big_picture_in_detail = false;
                self.needs_terminal_clear = true;
                if self.is_big_picture {
                    self.preload_visible_covers();
                    self.status_msg = "[MODE] Switched to Big Picture Mode".to_string();
                    crate::window_helper::set_fullscreen(true);
                } else {
                    crate::window_helper::set_fullscreen(false);
                }
            }
            Action::OpenGameDetail => {
                if self.games.is_empty() || self.selected_game_idx >= self.games.len() {
                    return;
                }
                self.big_picture_in_detail = true;
                self.detail_action_idx = 0;
                self.preload_game_detail_media();
            }
            Action::CloseGameDetail => {
                self.big_picture_in_detail = false;
            }
            Action::DetailNextAction => {
                if self.big_picture_in_detail {
                    self.detail_action_idx =
                        (self.detail_action_idx + 1) % crate::ui::DETAIL_ACTIONS.len();
                }
            }
            Action::DetailPrevAction => {
                if self.big_picture_in_detail {
                    let total = crate::ui::DETAIL_ACTIONS.len();
                    self.detail_action_idx = (self.detail_action_idx + total - 1) % total;
                }
            }
            Action::OpenCheatsheetModal => {
                self.modal_state = ModalState::CheatsheetModal {
                    selected_category_idx: 0,
                };
            }
            Action::OpenWelcomeWizardModal => {
                let api_key = self
                    .db
                    .get_setting("steamgriddb_api_key")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let key_len = api_key.len();
                self.modal_state = ModalState::WelcomeWizard {
                    step: 0,
                    sgdb_api_key: api_key,
                    active_field: 0,
                    cursor_pos: key_len,
                };
            }
            Action::OpenFuzzySearchModal => {
                let q = self.search_query.clone();
                let len = q.len();
                self.modal_state = ModalState::FuzzySearchModal {
                    query: q,
                    cursor_pos: len,
                };
            }
            Action::UpdateFuzzySearchQuery(new_q) => {
                self.search_query = new_q;
                self.is_search_active = !self.search_query.is_empty();
                self.filter_games_by_search();
            }
            Action::ClearFuzzySearch => {
                self.search_query.clear();
                self.is_search_active = false;
                self.load_games_for_selected_platform();
            }
            Action::AddToast(msg, kind) => {
                self.show_toast(msg, kind);
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
            Action::OpenPlatformSelectorModal => {
                self.modal_state = ModalState::PlatformSelector {
                    selected_idx: self.selected_platform_idx,
                };
            }
            Action::ConfirmPlatformSelectorModal => {
                if let ModalState::PlatformSelector { selected_idx } = self.modal_state {
                    if selected_idx < self.platforms.len() {
                        self.selected_platform_idx = selected_idx;
                        self.load_games_for_selected_platform();
                    }
                    self.modal_state = ModalState::None;
                }
            }
            Action::ToggleBigPictureFocus => {
                self.big_picture_focus = match self.big_picture_focus {
                    BigPictureFocus::Carousel => BigPictureFocus::PlatformBar,
                    BigPictureFocus::PlatformBar => BigPictureFocus::Search,
                    BigPictureFocus::Search => BigPictureFocus::Carousel,
                };
            }
            Action::NextPlatform => {
                if !self.platforms.is_empty() {
                    self.selected_platform_idx =
                        (self.selected_platform_idx + 1) % self.platforms.len();
                    self.load_games_for_selected_platform();
                }
            }
            Action::PrevPlatform => {
                if !self.platforms.is_empty() {
                    if self.selected_platform_idx == 0 {
                        self.selected_platform_idx = self.platforms.len() - 1;
                    } else {
                        self.selected_platform_idx -= 1;
                    }
                    self.load_games_for_selected_platform();
                }
            }
            Action::CycleActiveEmulatorNext | Action::CycleActiveEmulatorPrev => {
                let backward = matches!(action, Action::CycleActiveEmulatorPrev);
                if self.modal_state != ModalState::None || self.platforms.is_empty() {
                    return;
                }
                // Core-based emulators (RetroArch-style) run a selected core, so
                // the ◀ ▶ selector drives the nested "Núcleo" row instead of
                // switching emulators (there is only one anyway).
                let platform = self.platforms[self.selected_platform_idx].clone();
                self.cycle_active_selector_for(&platform, backward);
            }
            Action::NextGame => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    self.selected_game_idx = (self.selected_game_idx + 1) % self.games.len();
                    self.sync_platform_selection_with_game();
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
                    self.sync_platform_selection_with_game();
                    self.trigger_async_cover_fetch();
                }
            }
            Action::LaunchGame => {
                if self.games.is_empty() {
                    self.status_msg = "No games available to launch.".to_string();
                    return;
                }
                if self.running_game.is_some() {
                    self.status_msg =
                        "Un juego ya está en ejecución. Ciérralo o presiona [F] para forzar su cierre."
                            .to_string();
                    return;
                }

                let game = self.games[self.selected_game_idx].clone();
                let runner = self
                    .db
                    .get_runner_for_game(game.platform_id, game.folder_id, game.emulator_override)
                    .ok()
                    .flatten();

                if game.game_type == "emulator" {
                    let is_valid = match &runner {
                        Some(r) => match &r.executable_path {
                            Some(p) => {
                                let pt = p.trim();
                                !pt.is_empty()
                                    && (pt.contains(' ')
                                        || pt.starts_with("flatpak run")
                                        || PathBuf::from(pt).exists())
                            }
                            None => false,
                        },
                        None => false,
                    };

                    if !is_valid {
                        let name = runner
                            .as_ref()
                            .map(|r| r.name.as_str())
                            .unwrap_or("emulator");
                        self.status_msg = format!("Configure emulator '{}' [m]", name);
                        self.show_toast(
                            format!("Emulator '{}' not configured. Press [m] to set up.", name),
                            crate::toast::ToastKind::Warning,
                        );
                        return;
                    }
                }

                self.status_msg = format!("Launching {}...", game.title);
                self.start_game_background(game, runner);
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
                    "Scanning folder: {:?} for {}...",
                    default_dir, platform.name
                );

                if default_dir.exists() {
                    let (scan_tx, scan_rx) =
                        mpsc::channel::<game_core::scanner::ScanProgressEvent>(100);
                    self.scan_rx = Some(scan_rx);
                    self.download_progress = Some(DownloadProgressState {
                        runner_id: 0,
                        runner_name: format!("Scanning: {}", platform.name),
                        downloaded_bytes: 0,
                        total_bytes: 1,
                        percentage: 0.0,
                        is_finished: false,
                        error_msg: None,
                    });
                    self.status_msg =
                        format!("Scanning & Identifying ROMs for {}...", platform.name);

                    let db_path = dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                        .join("tui_game_station")
                        .join("game_station.db");

                    tokio::spawn(async move {
                        let slug = platform.slug.clone();
                        let _ =
                            game_core::dat_downloader::DatDownloader::ensure_dat_downloaded(&slug)
                                .await;

                        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
                        let scan_tx_clone = scan_tx.clone();

                        tokio::task::spawn_blocking(move || {
                            if let Ok(db) = Database::open(&db_path) {
                                let _ = Scanner::scan_folder(
                                    &db,
                                    &platform,
                                    &default_dir,
                                    true,
                                    false,
                                    true,
                                    None,
                                    Some(&sync_tx),
                                );
                            }
                        });

                        while let Ok(evt) = sync_rx.recv() {
                            let _ = scan_tx_clone.send(evt).await;
                        }
                    });
                } else {
                    self.status_msg = format!(
                        "Folder not found: {:?}. Please create folder ~/Juegos",
                        default_dir
                    );
                }
            }
            Action::ScanSteamGames => {
                self.status_msg = "Scanning for installed Steam games...".to_string();
                match SteamScanner::scan_steam_games(&self.db) {
                    Ok(added) => {
                        self.status_msg =
                            format!("Steam scan completed: {} game(s) in library.", added);
                        self.load_platforms();
                    }
                    Err(err) => {
                        self.status_msg = format!("Error detecting Steam: {}", err);
                    }
                }
            }

            // File & Folder Pickers
            Action::OpenFolderPicker => {
                if let Some(picked) = rfd::FileDialog::new().pick_folder() {
                    let path_str = picked.to_string_lossy().to_string();
                    match self.modal_state {
                        ModalState::AddFolderScanForm {
                            ref mut folder_path,
                            ..
                        } => {
                            *folder_path = path_str.clone();
                            self.status_msg = format!("Folder selected: {}", path_str);
                        }
                        ModalState::AddGameForm {
                            ref mut working_dir,
                            ref mut wine_prefix,
                            selected_field,
                            game_type: ref gtype,
                            ..
                        }
                        | ModalState::EditGameForm {
                            ref mut working_dir,
                            ref mut wine_prefix,
                            selected_field,
                            game_type: ref gtype,
                            ..
                        } => match gtype {
                            PlatformType::Native if selected_field == 2 => {
                                *working_dir = path_str.clone();
                                self.status_msg = format!("Working directory set: {}", path_str);
                            }
                            PlatformType::Wine if selected_field == 2 => {
                                *wine_prefix = path_str.clone();
                                self.status_msg = format!("WINEPREFIX set: {}", path_str);
                            }
                            PlatformType::Wine if selected_field == 3 => {
                                *working_dir = path_str.clone();
                                self.status_msg = format!("Working directory set: {}", path_str);
                            }
                            _ => {}
                        },
                        _ => {}
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

                        ModalState::AddGameForm {
                            ref mut file_path,
                            ref mut working_dir,
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
                                    self.status_msg = format!("ROM file selected: {}", path_str);
                                }
                                PlatformType::Native | PlatformType::Wine
                                    if selected_field == 1 =>
                                {
                                    *file_path = path_str.clone();
                                    if let Some(parent) = picked.parent() {
                                        *working_dir = parent.to_string_lossy().to_string();
                                    }
                                    self.status_msg =
                                        format!("Executable file selected: {}", path_str);
                                }
                                _ => {}
                            }
                        }
                        ModalState::EditGameForm {
                            ref mut file_path,
                            ref mut working_dir,
                            selected_field,
                            game_type: ref gtype,
                            ..
                        } => match gtype {
                            PlatformType::Emulator if selected_field == 1 => {
                                *file_path = path_str.clone();
                                self.status_msg = format!("ROM file selected: {}", path_str);
                            }
                            PlatformType::Native | PlatformType::Wine if selected_field == 1 => {
                                *file_path = path_str.clone();
                                if let Some(parent) = picked.parent() {
                                    *working_dir = parent.to_string_lossy().to_string();
                                }
                                self.status_msg = format!("Executable file selected: {}", path_str);
                            }
                            _ => {}
                        },
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
                if let ModalState::ManageRunnersStep1Platform {
                    selected_platform_idx,
                } = self.modal_state
                {
                    let unique_runners = self.db.get_unique_runners().unwrap_or_default();
                    if let Some(r) = unique_runners.get(selected_platform_idx) {
                        let exe_path = r.executable_path.clone().unwrap_or_default();
                        let len = exe_path.len();
                        let env = self
                            .db
                            .get_runner_env_by_name(&r.name)
                            .ok()
                            .flatten()
                            .map(|json| game_core::options::from_env_json(&json))
                            .unwrap_or_default();
                        let defs =
                            game_core::options::load_emulator_options(&r.name).unwrap_or_default();
                        let stored = env.emulator_options.clone().unwrap_or_default();
                        let mut merged = game_core::options::merge_runner_options(&defs, &stored);
                        // Preload options backed by a config file with their REAL
                        // current value from the emulator config; fall back to the
                        // TOML default when the file/key is missing.
                        for opt in &defs {
                            if let Some(real) = game_core::options::read_config_value(opt) {
                                if game_core::options::value_is_valid(opt, &real) {
                                    merged.insert(opt.key.clone(), real);
                                }
                            }
                        }
                        self.modal_state = ModalState::ManageRunnersStep2Config {
                            runner_info: r.clone(),
                            exe_path_input: exe_path,
                            options: defs,
                            option_values: merged,
                            custom_args: env.custom_args.unwrap_or_default(),
                            selected_row: 0,
                            selected_action_idx: 0,
                            cursor_pos: len,
                        };
                    }
                }
            }
            Action::SaveRunnerConfig => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ref exe_path_input,
                    ref options,
                    ref option_values,
                    ref custom_args,
                    ..
                } = self.modal_state.clone()
                {
                    let trimmed_path = exe_path_input.trim();
                    if trimmed_path.is_empty() {
                        self.status_msg =
                            "Error: Executable / .AppImage path cannot be empty.".to_string();
                        return;
                    }

                    let is_command = trimmed_path.contains(' ') || trimmed_path.starts_with("flatpak run");
                    if !is_command && !Path::new(trimmed_path).exists() {
                        self.status_msg = format!("Error: File does not exist on system: '{}'. Select a valid executable.", trimmed_path);
                        return;
                    }

                    let env_json =
                        game_core::options::build_env_json(options, option_values, custom_args);

                    let source = if trimmed_path.starts_with("flatpak run") {
                        Some("flatpak")
                    } else if trimmed_path.to_lowercase().contains(".appimage") {
                        Some("appimage")
                    } else {
                        Some("system")
                    };

                    match self
                        .db
                        .update_runner_by_name_with_source(&runner_info.name, trimmed_path, source)
                    {
                        Ok(_) => {
                            let _ = self
                                .db
                                .update_runner_env_by_name(&runner_info.name, Some(&env_json));
                            self.trigger_async_dat_download_by_runner(&runner_info.name);
                            self.status_msg = format!(
                                "[OK] Emulator '{}' ({}) configured successfully!",
                                runner_info.name, runner_info.console_initials
                            );
                            // Apply config-file targets. Failures (missing file,
                            // key absent, ...) never break the save: they are
                            // logged and surfaced as a non-blocking warning.
                            let failures =
                                game_core::options::apply_config_patches(options, option_values);
                            for failure in &failures {
                                tracing::warn!(
                                    "config_target patch failed for '{}': {}",
                                    failure.option_key,
                                    failure.message
                                );
                            }
                            if !failures.is_empty() {
                                let keys = failures
                                    .iter()
                                    .map(|f| f.option_key.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                self.status_msg = format!(
                                    "{} - Warning: no se pudo aplicar la opcion(es) [{}] al archivo de config del emulador.",
                                    self.status_msg, keys
                                );
                            }
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error saving runner: {}", err);
                        }
                    }
                }
            }
            Action::ResetRunnerConfig | Action::ToggleRunnerActiveState => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info, ..
                } = self.modal_state.clone()
                {
                    let new_state = !runner_info.is_configured;
                    match self
                        .db
                        .toggle_runner_configured(&runner_info.name, new_state)
                    {
                        Ok(_) => {
                            if new_state {
                                self.trigger_async_dat_download_by_runner(&runner_info.name);
                            }
                            self.status_msg = format!(
                                "Emulator '{}' ({}) {} successfully.",
                                runner_info.name,
                                runner_info.console_initials,
                                if new_state {
                                    "activated"
                                } else {
                                    "deactivated"
                                }
                            );
                            self.modal_state = ModalState::None;
                            self.load_platforms();
                        }
                        Err(err) => {
                            self.status_msg = format!("Error updating runner state: {}", err);
                        }
                    }
                }
            }
            Action::OpenConfirmDeleteRunnerModal => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ref exe_path_input,
                    ..
                } = self.modal_state.clone()
                {
                    let mut confirmed = runner_info.clone();
                    let trimmed = exe_path_input.trim();
                    if !trimmed.is_empty() {
                        confirmed.executable_path = Some(trimmed.to_string());
                    }
                    self.modal_state = ModalState::ConfirmDeleteRunner {
                        runner_info: confirmed,
                        selected_option: 0,
                    };
                }
            }
            Action::ToggleConfirmDeleteRunnerOption => {
                if let ModalState::ConfirmDeleteRunner {
                    ref mut selected_option,
                    ..
                } = self.modal_state
                {
                    *selected_option = if *selected_option == 0 { 1 } else { 0 };
                }
            }
            Action::ConfirmDeleteRunnerExecution | Action::DeleteRunnerDownload => {
                if let ModalState::ConfirmDeleteRunner {
                    runner_info,
                    selected_option,
                } = self.modal_state.clone()
                {
                    if selected_option == 1 {
                        if let Some(exe_path) = &runner_info.executable_path {
                            let path = PathBuf::from(exe_path);
                            if path.exists() {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                        let _ = self.db.reset_runner_by_name(&runner_info.name);
                        self.status_msg = format!(
                            "[Deleted] Emulator '{}' executable deleted from disk.",
                            runner_info.name
                        );
                        self.modal_state = ModalState::None;
                        self.load_platforms();
                    } else {
                        self.modal_state = ModalState::None;
                    }
                } else if let ModalState::ManageRunnersStep2Config {
                    ref runner_info, ..
                } = self.modal_state.clone()
                {
                    if let Some(exe_path) = &runner_info.executable_path {
                        let path = PathBuf::from(exe_path);
                        if path.exists() {
                            let _ = std::fs::remove_file(&path);
                        }
                        let _ = self.db.reset_runner_by_name(&runner_info.name);
                        self.status_msg = format!(
                            "[Deleted] Emulator '{}' executable deleted from disk.",
                            runner_info.name
                        );
                        self.modal_state = ModalState::None;
                        self.load_platforms();
                    }
                }
            }
            Action::OpenRunnerStandalone => {
                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info,
                    ref exe_path_input,
                    ref options,
                    ref option_values,
                    ref custom_args,
                    ..
                } = self.modal_state.clone()
                {
                    let exe = exe_path_input.trim();
                    if exe.is_empty() {
                        self.status_msg = format!(
                            "Configure the executable for '{}' before opening it.",
                            runner_info.name
                        );
                        return;
                    }
                    if !Path::new(exe).exists() && !runner::is_executable_command(exe) {
                        self.status_msg = format!(
                            "Executable/AppImage for '{}' does not exist on disk ({}).",
                            runner_info.name, exe
                        );
                        return;
                    }

                    let env_json =
                        game_core::options::build_env_json(options, option_values, custom_args);
                    let runner = game_core::models::Runner {
                        id: 0,
                        platform_id: None,
                        name: runner_info.name.clone(),
                        runner_type: runner_info.runner_type.clone(),
                        executable_path: Some(exe.to_string()),
                        command_template: String::new(),
                        default_env: None,
                        download_url: runner_info.download_url.clone(),
                        download_filename: runner_info.download_filename.clone(),
                        is_default: false,
                        is_active: false,
                        env_vars: Some(env_json),
                        source: None,
                    };

                    self.status_msg = format!("Opening {}...", runner_info.name);
                    self.start_standalone_background(runner, runner_info.name.clone());
                }
            }
            Action::StartRunnerDownload => {
                if self.download_progress.is_some() {
                    self.status_msg = "[Warning] A download task is already in progress. Please wait for it to complete.".to_string();
                    return;
                }

                if let ModalState::ManageRunnersStep2Config {
                    ref runner_info, ..
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
                            self.status_msg = format!("Error creating download directory: {}", e);
                            return;
                        }
                    };

                    let dest_path = target_dir.join(&download_filename);
                    let is_retroarch_7z = runner_info.name == "RetroArch"
                        || download_filename.to_lowercase().ends_with(".7z");
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
                        if is_retroarch_7z {
                            let managed_retroarch_dir =
                                game_core::retroarch_manager::get_retroarch_managed_dir();
                            let result = RunnerDownloader::download_7z_appimage_with_progress(
                                &download_url,
                                &dest_path,
                                &managed_retroarch_dir,
                                tx,
                            )
                            .await;

                            if let Ok(appimage_path) = result {
                                if let Ok(db) = Database::open(&db_path) {
                                    let _ = db.update_runner_by_name_with_source(
                                        &runner_name,
                                        &appimage_path.to_string_lossy(),
                                        Some("Downloaded"),
                                    );
                                }
                            }
                        } else {
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
                                RunnerDownloader::download_with_progress(&download_url, &dest_path, tx)
                                    .await
                            };

                            if result.is_ok() {
                                if let Ok(db) = Database::open(&db_path) {
                                    let _ = db.update_runner_by_name_with_source(
                                        &runner_name,
                                        &executable_path_str,
                                        Some("Downloaded"),
                                    );
                                }
                            }
                        }
                    });
                }
            }
            Action::OpenWineRunnerManager => {
                let installed_runners =
                    game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                self.modal_state = ModalState::ManageWineRunners {
                    installed_runners,
                    selected_idx: 0,
                };
            }
            Action::OpenProtonDownloader => {
                self.modal_state = ModalState::ProtonDownloader {
                    step: 0,
                    selected_launcher_idx: 0,
                    selected_tool_idx: 0,
                    releases: Vec::new(),
                    selected_release_idx: 0,
                    is_loading: false,
                    download_event: None,
                };
                self.status_msg =
                    "[ Step 1/3 ] Select Target Launcher with [Up/Down] and press [Enter]."
                        .to_string();
            }
            Action::ProtonDownloaderSelectNext => {
                if let ModalState::ProtonDownloader {
                    step,
                    ref mut selected_launcher_idx,
                    ref mut selected_tool_idx,
                    ref mut selected_release_idx,
                    ref releases,
                    ..
                } = self.modal_state
                {
                    match step {
                        0 => {
                            let launchers = scraper::proton::TargetLauncher::all();
                            if !launchers.is_empty() {
                                *selected_launcher_idx =
                                    (*selected_launcher_idx + 1) % launchers.len();
                            }
                        }
                        1 => {
                            let cur_launcher = *selected_launcher_idx;
                            let launcher = scraper::proton::TargetLauncher::all()[cur_launcher];
                            let valid_tools = launcher.valid_repos();
                            if !valid_tools.is_empty() {
                                *selected_tool_idx = (*selected_tool_idx + 1) % valid_tools.len();
                            }
                        }
                        2 if !releases.is_empty() => {
                            *selected_release_idx = (*selected_release_idx + 1) % releases.len();
                        }
                        _ => {}
                    }
                }
            }
            Action::ProtonDownloaderSelectPrev => {
                if let ModalState::ProtonDownloader {
                    step,
                    ref mut selected_launcher_idx,
                    ref mut selected_tool_idx,
                    ref mut selected_release_idx,
                    ref releases,
                    ..
                } = self.modal_state
                {
                    match step {
                        0 => {
                            let launchers = scraper::proton::TargetLauncher::all();
                            if !launchers.is_empty() {
                                if *selected_launcher_idx == 0 {
                                    *selected_launcher_idx = launchers.len() - 1;
                                } else {
                                    *selected_launcher_idx -= 1;
                                }
                            }
                        }
                        1 => {
                            let cur_launcher = *selected_launcher_idx;
                            let launcher = scraper::proton::TargetLauncher::all()[cur_launcher];
                            let valid_tools = launcher.valid_repos();
                            if !valid_tools.is_empty() {
                                if *selected_tool_idx == 0 {
                                    *selected_tool_idx = valid_tools.len() - 1;
                                } else {
                                    *selected_tool_idx -= 1;
                                }
                            }
                        }
                        2 if !releases.is_empty() => {
                            if *selected_release_idx == 0 {
                                *selected_release_idx = releases.len() - 1;
                            } else {
                                *selected_release_idx -= 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Action::ProtonDownloaderBack => {
                if let ModalState::ProtonDownloader { ref mut step, .. } = self.modal_state {
                    if *step > 0 {
                        *step -= 1;
                    } else {
                        self.modal_state = ModalState::None;
                    }
                }
            }
            Action::ProtonDownloaderConfirm => {
                if let ModalState::ProtonDownloader {
                    step,
                    selected_launcher_idx,
                    selected_tool_idx,
                    selected_release_idx,
                    ref releases,
                    ..
                } = self.modal_state.clone()
                {
                    let launchers = scraper::proton::TargetLauncher::all();
                    let launcher = launchers
                        .get(selected_launcher_idx)
                        .copied()
                        .unwrap_or(scraper::proton::TargetLauncher::Steam);

                    match step {
                        0 => {
                            if let ModalState::ProtonDownloader {
                                ref mut step,
                                ref mut selected_tool_idx,
                                ..
                            } = self.modal_state
                            {
                                *step = 1;
                                *selected_tool_idx = 0;
                                self.status_msg = format!("[ Step 2/3 ] {} -> Select Tool/Runner with [Up/Down] and press [Enter].", launcher.display_name());
                            }
                        }
                        1 => {
                            let valid_tools = launcher.valid_repos();
                            if let Some(&tool) = valid_tools.get(selected_tool_idx) {
                                if let ModalState::ProtonDownloader {
                                    ref mut step,
                                    ref mut is_loading,
                                    ref mut releases,
                                    ref mut selected_release_idx,
                                    ..
                                } = self.modal_state
                                {
                                    *step = 2;
                                    *is_loading = true;
                                    *releases = Vec::new();
                                    *selected_release_idx = 0;
                                    self.status_msg =
                                        format!("Fetching releases for {}...", tool.display_name());
                                }

                                if let Ok(fetched) =
                                    scraper::proton::ProtonDownloaderClient::fetch_releases(
                                        tool, 1, 12,
                                    )
                                    .await
                                {
                                    if let ModalState::ProtonDownloader {
                                        ref mut releases,
                                        ref mut is_loading,
                                        ..
                                    } = self.modal_state
                                    {
                                        *releases = fetched;
                                        *is_loading = false;
                                        self.status_msg = format!(
                                            "[OK] Loaded {} release(s) for {}.",
                                            releases.len(),
                                            tool.display_name()
                                        );
                                    }
                                }
                            }
                        }
                        2 if !releases.is_empty() => {
                            if self.download_progress.is_some() {
                                self.status_msg = "[Warning] A download/extraction task is already in progress. Please wait for it to complete.".to_string();
                                return;
                            }

                            let valid_tools = launcher.valid_repos();
                            if let (Some(&tool), Some(release)) = (
                                valid_tools.get(selected_tool_idx),
                                releases.get(selected_release_idx),
                            ) {
                                let target_dir = launcher.installation_dir(tool);

                                let (tx, rx) = mpsc::channel::<DownloadEvent>(100);
                                self.download_rx = Some(rx);
                                self.download_progress = Some(DownloadProgressState {
                                    runner_id: 0,
                                    runner_name: release.name.clone(),
                                    downloaded_bytes: 0,
                                    total_bytes: release
                                        .asset
                                        .as_ref()
                                        .map(|a| a.size)
                                        .unwrap_or(0),
                                    percentage: 0.0,
                                    is_finished: false,
                                    error_msg: None,
                                });
                                let rel_clone = release.clone();

                                self.status_msg =
                                    format!("Downloading & extracting {}...", release.name);

                                tokio::spawn(async move {
                                    let _ = scraper::proton::ProtonDownloaderClient::download_and_extract(
                                            &rel_clone,
                                            &target_dir,
                                            tx,
                                        )
                                        .await;
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Action::FetchProtonReleases => {}
            Action::StartProtonDownload => {
                if self.download_progress.is_some() {
                    self.status_msg = "[Warning] A download/extraction task is already in progress. Please wait for it to complete.".to_string();
                    return;
                }

                if let ModalState::ProtonDownloader {
                    selected_launcher_idx,
                    selected_tool_idx,
                    ref releases,
                    selected_release_idx,
                    ..
                } = self.modal_state.clone()
                {
                    let launchers = scraper::proton::TargetLauncher::all();
                    let launcher = launchers
                        .get(selected_launcher_idx)
                        .copied()
                        .unwrap_or(scraper::proton::TargetLauncher::Steam);
                    let valid_tools = launcher.valid_repos();
                    if let (Some(&tool), Some(release)) = (
                        valid_tools.get(selected_tool_idx),
                        releases.get(selected_release_idx),
                    ) {
                        let target_dir = launcher.installation_dir(tool);

                        let (tx, rx) = mpsc::channel::<DownloadEvent>(100);
                        self.download_rx = Some(rx);
                        self.download_progress = Some(DownloadProgressState {
                            runner_id: 0,
                            runner_name: release.name.clone(),
                            downloaded_bytes: 0,
                            total_bytes: release.asset.as_ref().map(|a| a.size).unwrap_or(0),
                            percentage: 0.0,
                            is_finished: false,
                            error_msg: None,
                        });
                        let rel_clone = release.clone();

                        self.status_msg = format!("Downloading & extracting {}...", release.name);

                        tokio::spawn(async move {
                            let _ = scraper::proton::ProtonDownloaderClient::download_and_extract(
                                &rel_clone,
                                &target_dir,
                                tx,
                            )
                            .await;
                        });
                    }
                }
            }
            Action::OpenWinecfg => {
                if let Some(game) = self.games.get(self.selected_game_idx) {
                    if game.game_type == "wine" {
                        let wine_prefix = game.wine_prefix.clone().unwrap_or_else(|| {
                            if let Some(ref wdir) = game.working_dir {
                                if !wdir.trim().is_empty() {
                                    return std::path::PathBuf::from(wdir)
                                        .join("prefix")
                                        .to_string_lossy()
                                        .to_string();
                                }
                            }
                            let data_dir = dirs::data_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"));
                            data_dir
                                .join("tui_game_station")
                                .join("wineprefixes")
                                .join(format!("p_{}", game.id))
                                .to_string_lossy()
                                .to_string()
                        });
                        let _ = std::fs::create_dir_all(&wine_prefix);
                        self.status_msg = format!("Opening winecfg for prefix: {}...", wine_prefix);
                        tokio::spawn(async move {
                            let _ = tokio::process::Command::new("winecfg")
                                .env("WINEPREFIX", &wine_prefix)
                                .spawn();
                        });
                    } else {
                        self.status_msg =
                            "[Warning] winecfg is only applicable to Wine/Windows games."
                                .to_string();
                    }
                }
            }
            Action::OpenWinetricks => {
                if let Some(game) = self.games.get(self.selected_game_idx) {
                    if game.game_type == "wine" {
                        let wine_prefix = game.wine_prefix.clone().unwrap_or_else(|| {
                            if let Some(ref wdir) = game.working_dir {
                                if !wdir.trim().is_empty() {
                                    return std::path::PathBuf::from(wdir)
                                        .join("prefix")
                                        .to_string_lossy()
                                        .to_string();
                                }
                            }
                            let data_dir = dirs::data_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"));
                            data_dir
                                .join("tui_game_station")
                                .join("wineprefixes")
                                .join(format!("p_{}", game.id))
                                .to_string_lossy()
                                .to_string()
                        });
                        let _ = std::fs::create_dir_all(&wine_prefix);
                        self.status_msg =
                            format!("Opening winetricks for prefix: {}...", wine_prefix);
                        tokio::spawn(async move {
                            let _ = tokio::process::Command::new("winetricks")
                                .env("WINEPREFIX", &wine_prefix)
                                .spawn();
                        });
                    } else {
                        self.status_msg =
                            "[Warning] winetricks is only applicable to Wine/Windows games."
                                .to_string();
                    }
                }
            }
            Action::KillWineProcesses => {
                if let Some(game) = self.games.get(self.selected_game_idx) {
                    if game.game_type == "wine" {
                        let wine_prefix = game.wine_prefix.clone().unwrap_or_default();
                        self.status_msg =
                            format!("Killing Wine processes for prefix: {}...", wine_prefix);
                        tokio::spawn(async move {
                            let _ = tokio::process::Command::new("wineserver")
                                .arg("-k")
                                .env("WINEPREFIX", &wine_prefix)
                                .output()
                                .await;
                        });
                    } else {
                        self.status_msg =
                            "[Warning] Kill Wine is only applicable to Wine/Windows games."
                                .to_string();
                    }
                }
            }
            Action::OpenWineToolsMenu => {
                if self
                    .games
                    .get(self.selected_game_idx)
                    .map(|g| g.game_type.as_str())
                    == Some("wine")
                {
                    self.modal_state = ModalState::WineToolsMenu { selected_idx: 0 };
                } else {
                    self.status_msg =
                        "[Warning] Wine tools are only available for Wine/Windows games."
                            .to_string();
                }
            }
            Action::SelectWineTool => {
                if let ModalState::WineToolsMenu { selected_idx } = self.modal_state {
                    self.modal_state = ModalState::None;
                    if let Some(game) = self
                        .games
                        .get(self.selected_game_idx)
                        .filter(|g| g.game_type == "wine")
                    {
                        let wine_prefix = game.wine_prefix.clone().unwrap_or_else(|| {
                            if let Some(ref wdir) = game.working_dir {
                                if !wdir.trim().is_empty() {
                                    return std::path::PathBuf::from(wdir)
                                        .join("prefix")
                                        .to_string_lossy()
                                        .to_string();
                                }
                            }
                            let data_dir = dirs::data_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"));
                            data_dir
                                .join("tui_game_station")
                                .join("wineprefixes")
                                .join(format!("p_{}", game.id))
                                .to_string_lossy()
                                .to_string()
                        });
                        let _ = std::fs::create_dir_all(&wine_prefix);

                        let (exe, args, msg) = match selected_idx {
                            0 => (
                                "winecfg".to_string(),
                                Vec::new(),
                                format!("Opening winecfg for prefix: {}...", wine_prefix),
                            ),
                            1 => (
                                "winetricks".to_string(),
                                Vec::new(),
                                format!("Opening winetricks for prefix: {}...", wine_prefix),
                            ),
                            2 => (
                                "wineserver".to_string(),
                                vec!["-k".to_string()],
                                format!("Killing Wine processes for prefix: {}...", wine_prefix),
                            ),
                            3 => (
                                "xdg-open".to_string(),
                                vec![wine_prefix.clone()],
                                format!("Opening prefix folder: {}...", wine_prefix),
                            ),
                            _ => return,
                        };
                        self.status_msg = msg;
                        let mut envs = HashMap::new();
                        envs.insert("WINEPREFIX".to_string(), wine_prefix);
                        self.pending_wine_tool = Some(WineToolCommand { exe, args, envs });
                    } else {
                        self.status_msg =
                            "[Warning] Wine tools are only available for Wine/Windows games."
                                .to_string();
                    }
                }
            }
            Action::OpenWineRunnerPicker => {
                let installed_runners =
                    game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                if installed_runners.is_empty() {
                    self.status_msg = "No installed Wine/Proton runners detected. Press [m] to download GE-Proton.".to_string();
                    return;
                }
                let parent_modal = match self.modal_state {
                    ModalState::AddGameForm { .. } | ModalState::EditGameForm { .. } => {
                        Some(Box::new(self.modal_state.clone()))
                    }
                    _ => None,
                };
                self.modal_state = ModalState::SelectWineRunnerPicker {
                    installed_runners,
                    selected_idx: 0,
                    parent_modal,
                };
            }
            Action::SelectWineRunnerFromPicker => {
                if let ModalState::SelectWineRunnerPicker {
                    ref installed_runners,
                    selected_idx,
                    ref parent_modal,
                } = self.modal_state.clone()
                {
                    if let Some(runner) = installed_runners.get(selected_idx) {
                        let cmd_str = match runner.kind {
                            game_core::runner_detector::RunnerKind::Proton => {
                                format!(
                                    "\"{}\" run \"{{file_path}}\"",
                                    runner.binary_path.display()
                                )
                            }
                            game_core::runner_detector::RunnerKind::Wine => {
                                format!("\"{}\" \"{{file_path}}\"", runner.binary_path.display())
                            }
                        };

                        if let Some(mut parent) = parent_modal.clone() {
                            if let ModalState::AddGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            }
                            | ModalState::EditGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            } = *parent
                            {
                                *custom_command = cmd_str;
                            }
                            self.modal_state = *parent;
                        } else {
                            self.modal_state = ModalState::None;
                        }

                        self.status_msg = format!(
                            "[OK] Selected runner '{}' ({})!",
                            runner.name,
                            runner.location.display_name()
                        );
                    }
                }
            }
            Action::OpenCustomArgsEditor => {
                let args = if let ModalState::AddGameForm {
                    ref custom_command, ..
                }
                | ModalState::EditGameForm {
                    ref custom_command, ..
                } = self.modal_state
                {
                    Some(custom_command.clone())
                } else if let ModalState::ManageRunnersStep2Config {
                    ref custom_args, ..
                } = self.modal_state
                {
                    Some(custom_args.clone())
                } else {
                    None
                };
                if let Some(args) = args {
                    let cpos = args.len();
                    let parent = Box::new(self.modal_state.clone());
                    self.modal_state = ModalState::EditCustomArgsInput {
                        input: args,
                        cursor_pos: cpos,
                        parent_modal: parent,
                    };
                }
            }
            Action::SaveCustomArgsInput => {
                if let ModalState::EditCustomArgsInput {
                    ref input,
                    ref parent_modal,
                    ..
                } = self.modal_state.clone()
                {
                    let mut parent = parent_modal.clone();
                    if let ModalState::AddGameForm {
                        ref mut custom_command,
                        ..
                    }
                    | ModalState::EditGameForm {
                        ref mut custom_command,
                        ..
                    } = *parent
                    {
                        *custom_command = input.clone();
                    } else if let ModalState::ManageRunnersStep2Config {
                        ref mut custom_args,
                        ..
                    } = *parent
                    {
                        *custom_args = input.clone();
                    }
                    self.modal_state = *parent;
                    self.status_msg = "[OK] Custom launcher arguments updated.".to_string();
                }
            }
            Action::FormNavLeft => {
                let is_edit = matches!(self.modal_state, ModalState::EditGameForm { .. });
                match self.modal_state {
                ModalState::AddGameForm {
                    selected_field: 0,
                    ref mut cursor_pos,
                    ..
                }
                | ModalState::EditGameForm {
                    selected_field: 0,
                    ref mut cursor_pos,
                    ..
                } => {
                    *cursor_pos = cursor_pos.saturating_sub(1);
                }
                ModalState::EditCustomArgsInput {
                    ref mut cursor_pos, ..
                } => {
                    *cursor_pos = cursor_pos.saturating_sub(1);
                }
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    selected_field,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: ref gtype,
                    selected_field,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ..
                } if form_text_field(gtype, is_edit, selected_field).is_some() => {
                    match form_text_field(gtype, is_edit, selected_field) {
                        Some(FormTextField::Path) => {
                            *path_cursor = path_cursor.saturating_sub(1)
                        }
                        Some(FormTextField::Workdir) => {
                            *workdir_cursor = workdir_cursor.saturating_sub(1)
                        }
                        Some(FormTextField::Prefix) => {
                            *prefix_cursor = prefix_cursor.saturating_sub(1)
                        }
                        Some(FormTextField::Cmd) => {
                            *cmd_cursor = cmd_cursor.saturating_sub(1)
                        }
                        None => {}
                    }
                }
                ModalState::AddGameForm {
                    game_type: PlatformType::Wine,
                    selected_field: 4,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: PlatformType::Wine,
                    selected_field: 4,
                    ..
                } => {
                    let installed =
                        game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                    if !installed.is_empty() {
                        let current_cmd = match self.modal_state {
                            ModalState::AddGameForm {
                                game_type: PlatformType::Wine,
                                ref custom_command,
                                ..
                            }
                            | ModalState::EditGameForm {
                                game_type: PlatformType::Wine,
                                ref custom_command,
                                ..
                            } => custom_command.clone(),
                            _ => String::new(),
                        };
                        let current_idx = installed
                            .iter()
                            .position(|r| {
                                let runner_str = match r.kind {
                                    game_core::runner_detector::RunnerKind::Proton => format!(
                                        "\"{}\" run \"{{file_path}}\"",
                                        r.binary_path.display()
                                    ),
                                    game_core::runner_detector::RunnerKind::Wine => {
                                        format!("\"{}\" \"{{file_path}}\"", r.binary_path.display())
                                    }
                                };
                                current_cmd == runner_str || current_cmd.contains(&r.name)
                            })
                            .unwrap_or(0);
                        let next_idx = if current_idx == 0 {
                            installed.len() - 1
                        } else {
                            current_idx - 1
                        };
                        let selected_runner = &installed[next_idx];
                        let new_cmd = match selected_runner.kind {
                            game_core::runner_detector::RunnerKind::Proton => format!(
                                "\"{}\" run \"{{file_path}}\"",
                                selected_runner.binary_path.display()
                            ),
                            game_core::runner_detector::RunnerKind::Wine => format!(
                                "\"{}\" \"{{file_path}}\"",
                                selected_runner.binary_path.display()
                            ),
                        };
                        match self.modal_state {
                            ModalState::AddGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            }
                            | ModalState::EditGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            } => {
                                *custom_command = new_cmd;
                            }
                            _ => {}
                        }
                    }
                }
                ModalState::EditGameForm {
                    game_type: PlatformType::Emulator,
                    selected_field: 2,
                    ..
                } => {
                    self.cycle_edit_game_emulator(true);
                }
                _ => {}
            }
            }
            Action::FormNavRight => {
                let is_edit = matches!(self.modal_state, ModalState::EditGameForm { .. });
                match self.modal_state {
                ModalState::AddGameForm {
                    selected_field: 0,
                    ref title,
                    ref mut cursor_pos,
                    ..
                }
                | ModalState::EditGameForm {
                    selected_field: 0,
                    ref title,
                    ref mut cursor_pos,
                    ..
                } => {
                    *cursor_pos = (*cursor_pos + 1).min(title.len());
                }
                ModalState::EditGameForm {
                    game_type: PlatformType::Emulator,
                    selected_field: 2,
                    ..
                } => {
                    self.cycle_edit_game_emulator(false);
                }
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    selected_field,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: ref gtype,
                    selected_field,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    ..
                } if form_text_field(gtype, is_edit, selected_field).is_some() => {
                    match form_text_field(gtype, is_edit, selected_field) {
                        Some(FormTextField::Path) => {
                            *path_cursor = (*path_cursor + 1).min(file_path.len())
                        }
                        Some(FormTextField::Workdir) => {
                            *workdir_cursor = (*workdir_cursor + 1).min(working_dir.len())
                        }
                        Some(FormTextField::Prefix) => {
                            *prefix_cursor = (*prefix_cursor + 1).min(wine_prefix.len())
                        }
                        Some(FormTextField::Cmd) => {
                            *cmd_cursor = (*cmd_cursor + 1).min(custom_command.len())
                        }
                        None => {}
                    }
                }
                ModalState::EditCustomArgsInput {
                    ref mut cursor_pos,
                    ref input,
                    ..
                } => {
                    *cursor_pos = (*cursor_pos + 1).min(input.len());
                }
                ModalState::AddGameForm {
                    game_type: PlatformType::Wine,
                    selected_field: 4,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: PlatformType::Wine,
                    selected_field: 4,
                    ..
                } => {
                    let installed =
                        game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                    if !installed.is_empty() {
                        let current_cmd = match self.modal_state {
                            ModalState::AddGameForm {
                                game_type: PlatformType::Wine,
                                ref custom_command,
                                ..
                            }
                            | ModalState::EditGameForm {
                                game_type: PlatformType::Wine,
                                ref custom_command,
                                ..
                            } => custom_command.clone(),
                            _ => String::new(),
                        };
                        let current_idx = installed
                            .iter()
                            .position(|r| {
                                let runner_str = match r.kind {
                                    game_core::runner_detector::RunnerKind::Proton => format!(
                                        "\"{}\" run \"{{file_path}}\"",
                                        r.binary_path.display()
                                    ),
                                    game_core::runner_detector::RunnerKind::Wine => {
                                        format!("\"{}\" \"{{file_path}}\"", r.binary_path.display())
                                    }
                                };
                                current_cmd == runner_str || current_cmd.contains(&r.name)
                            })
                            .unwrap_or(0);
                        let next_idx = (current_idx + 1) % installed.len();
                        let selected_runner = &installed[next_idx];
                        let new_cmd = match selected_runner.kind {
                            game_core::runner_detector::RunnerKind::Proton => format!(
                                "\"{}\" run \"{{file_path}}\"",
                                selected_runner.binary_path.display()
                            ),
                            game_core::runner_detector::RunnerKind::Wine => format!(
                                "\"{}\" \"{{file_path}}\"",
                                selected_runner.binary_path.display()
                            ),
                        };
                        match self.modal_state {
                            ModalState::AddGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            }
                            | ModalState::EditGameForm {
                                game_type: PlatformType::Wine,
                                ref mut custom_command,
                                ..
                            } => {
                                *custom_command = new_cmd;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            }
            Action::CycleWineRunner(step) => {
                let installed =
                    game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                if installed.is_empty() {
                    self.status_msg = "[Warning] No installed Wine/Proton runners detected. Press [m] to download GE-Proton.".to_string();
                    return;
                }

                let current_cmd = match self.modal_state {
                    ModalState::AddGameForm {
                        game_type: PlatformType::Wine,
                        ref custom_command,
                        ..
                    }
                    | ModalState::EditGameForm {
                        game_type: PlatformType::Wine,
                        ref custom_command,
                        ..
                    } => custom_command.clone(),
                    _ => return,
                };

                let mut current_idx = None;
                for (idx, r) in installed.iter().enumerate() {
                    if !current_cmd.is_empty()
                        && (current_cmd.contains(&r.name)
                            || current_cmd.contains(r.binary_path.to_str().unwrap_or("")))
                    {
                        current_idx = Some(idx);
                        break;
                    }
                }

                let next_idx = match current_idx {
                    Some(i) => {
                        if step > 0 {
                            (i + 1) % installed.len()
                        } else if i == 0 {
                            installed.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };

                let runner = &installed[next_idx];
                let cmd_str = match runner.kind {
                    game_core::runner_detector::RunnerKind::Proton => {
                        format!("\"{}\" run \"{{file_path}}\"", runner.binary_path.display())
                    }
                    game_core::runner_detector::RunnerKind::Wine => {
                        format!("\"{}\" \"{{file_path}}\"", runner.binary_path.display())
                    }
                };

                if let ModalState::AddGameForm {
                    game_type: PlatformType::Wine,
                    ref mut custom_command,
                    ..
                }
                | ModalState::EditGameForm {
                    game_type: PlatformType::Wine,
                    ref mut custom_command,
                    ..
                } = self.modal_state
                {
                    *custom_command = cmd_str;
                    self.status_msg = format!(
                        "Selected Runner: {} ({})",
                        runner.name,
                        runner.location.display_name()
                    );
                }
            }
            Action::DeleteInstalledWineRunner => {
                if let ModalState::ManageWineRunners {
                    ref mut installed_runners,
                    selected_idx,
                    ..
                } = self.modal_state
                {
                    if let Some(runner) = installed_runners.get(selected_idx) {
                        let path = runner.base_path.clone();
                        if path.exists() {
                            let _ = std::fs::remove_dir_all(&path);
                            self.status_msg =
                                format!("[OK] Removed runner '{}' from disk.", runner.name);
                        }
                    }
                    *installed_runners =
                        game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                }
            }
            Action::UpdateDownloadProgress(event) => {
                if let ModalState::ProtonDownloader {
                    ref mut download_event,
                    ..
                } = self.modal_state
                {
                    *download_event = Some(event.clone());
                }

                if let Some(ref mut progress) = self.download_progress {
                    progress.downloaded_bytes = event.downloaded;
                    progress.total_bytes = event.total;
                    progress.percentage = event.percentage;
                    progress.is_finished = event.finished;
                    progress.error_msg = event.error.clone();
                    if let Some(ref name) = event.task_name {
                        progress.runner_name = name.clone();
                        self.status_msg = format!("[Media Fetch] {}", name);
                    }

                    if event.finished {
                        let name = progress.runner_name.clone();
                        self.download_progress = None;
                        self.download_rx = None;

                        if name.starts_with("Updating to v") {
                            if let Some(err) = event.error {
                                self.status_msg = format!("[Updater Error] Update failed: {}", err);
                            } else {
                                self.status_msg =
                                    "[OK] Update installed! Please restart app.".to_string();
                                self.show_toast(
                                    "[OK] Update installed! Please restart app.".to_string(),
                                    crate::toast::ToastKind::Success,
                                );
                                self.should_quit = true;
                            }
                        } else if let Some(err) = event.error {
                            self.status_msg = format!("[Error] Download failed: {}", err);
                        } else {
                            self.status_msg =
                                format!("[OK] Download of '{}' completed successfully!", name);

                            // Sync DB immediately on main thread for downloaded AppImage
                            if let Ok(td) = RunnerDownloader::get_runner_dir("emulators") {
                                let fn_name = format!("{}.AppImage", name.to_lowercase());
                                let exe = if name == "melonDS" {
                                    td.join("melonDS-x86_64.AppImage")
                                } else {
                                    td.join(fn_name)
                                };
                                if exe.exists() {
                                    let _ = self
                                        .db
                                        .update_runner_by_name(&name, &exe.to_string_lossy());
                                }
                            }

                            let sel = self.selected_game_idx;
                            self.load_platforms();
                            if sel < self.games.len() {
                                self.selected_game_idx = sel;
                            }

                            if matches!(self.modal_state, ModalState::ProtonDownloader { .. }) {
                                let installed_runners = game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
                                self.modal_state = ModalState::ManageWineRunners {
                                    installed_runners,
                                    selected_idx: 0,
                                };
                            }

                            if matches!(
                                self.modal_state,
                                ModalState::ManageRunnersStep2Config { .. }
                            ) {
                                self.modal_state = ModalState::None;
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
            Action::OpenAddGameWineForm => {
                // [ + Add New Game ] in the Windows Games manager: jump straight
                // into the Wine details form, skipping the type selector.
                self.open_add_game_form(PlatformType::Wine);
            }
            Action::OpenEditGameModal => {
                if self.modal_state == ModalState::None
                    && !self.games.is_empty()
                    && self.selected_game_idx < self.games.len()
                {
                    let game = &self.games[self.selected_game_idx];
                    let gtype = PlatformType::from(game.game_type.as_str());
                    self.open_edit_game_form(game.clone(), gtype);
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
                    gamemode,
                    mangohud,
                    gamescope,
                    esync,
                    fsync,
                    dxvk,
                    vkd3d,
                    emulator_override,
                    ref core_override,
                    ref parent_modal,
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
                        game.file_path = if file_path.trim().is_empty() {
                            None
                        } else {
                            Some(file_path.trim().to_string())
                        };
                        game.working_dir = if working_dir.trim().is_empty() {
                            None
                        } else {
                            Some(working_dir.trim().to_string())
                        };
                        game.custom_command = if custom_command.trim().is_empty() {
                            None
                        } else {
                            Some(custom_command.trim().to_string())
                        };
                        game.wine_prefix = if wine_prefix.trim().is_empty() {
                            if let Some(ref wdir) = game.working_dir {
                                let p = std::path::PathBuf::from(wdir).join("prefix");
                                let _ = std::fs::create_dir_all(&p);
                                Some(p.to_string_lossy().to_string())
                            } else {
                                None
                            }
                        } else {
                            let _ = std::fs::create_dir_all(wine_prefix.trim());
                            Some(wine_prefix.trim().to_string())
                        };
                        game.steam_appid = steam_appid.trim().parse::<i64>().ok();

                        let mut env_flags = Vec::new();
                        if gamemode {
                            env_flags.push("GAMEMODE=1");
                        }
                        if mangohud {
                            env_flags.push("MANGOHUD=1");
                        }
                        if gamescope {
                            env_flags.push("GAMESCOPE=1");
                        }
                        if esync {
                            env_flags.push("WINEESYNC=1");
                        }
                        if fsync {
                            env_flags.push("WINEFSYNC=1");
                        }
                        if dxvk {
                            env_flags.push("DXVK_ASYNC=1");
                        }
                        if vkd3d {
                            env_flags.push("VKD3D_CONFIG=enable_async");
                        }
                        game.env_vars = if env_flags.is_empty() {
                            None
                        } else {
                            Some(env_flags.join(" "))
                        };

                        game.emulator_override = emulator_override;
                        game.core_override = core_override.clone();

                        let target_slug = match game.game_type.as_str() {
                            "wine" => Some("windows"),
                            "native" => Some("linux"),
                            "steam" => Some("steam"),
                            _ => None,
                        };
                        if let Some(slug) = target_slug {
                            if let Ok(Some(target_p)) = self.db.get_platform_by_slug(slug) {
                                game.platform_id = target_p.id;
                            }
                        }

                        if self.db.update_game(&game).is_ok() {
                            self.status_msg = format!("[OK] Updated details for '{}'!", game.title);
                            if let Some(ModalState::WindowsGamesManager { .. }) =
                                parent_modal.as_deref()
                            {
                                let games = self
                                    .db
                                    .get_games_for_platform(game.platform_id)
                                    .unwrap_or_default();
                                self.games = games.clone();
                                self.modal_state = ModalState::WindowsGamesManager {
                                    games,
                                    selected_idx: 0,
                                };
                            } else {
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
            }
            Action::CloseModal => match self.modal_state.clone() {
                ModalState::SelectWineRunnerPicker {
                    parent_modal: Some(parent),
                    ..
                } => {
                    self.modal_state = *parent;
                }
                ModalState::EditCustomArgsInput { parent_modal, .. } => {
                    self.modal_state = *parent_modal;
                }
                ModalState::WineToolsMenu { .. } => {
                    self.modal_state = ModalState::None;
                }
                ModalState::DownloadCoreModal { parent_state, .. } => {
                    self.modal_state = *parent_state;
                }
                ModalState::SelectDetectedEmulatorModal { parent_modal, .. } => {
                    self.modal_state = *parent_modal;
                }
                ModalState::AddFolderScanForm { parent_modal: Some(parent), .. } => {
                    self.modal_state = *parent;
                }
                ModalState::AddGameForm { parent_modal: Some(parent), .. } => {
                    self.modal_state = *parent;
                }
                ModalState::EditGameForm { parent_modal: Some(parent), .. } => {
                    self.modal_state = *parent;
                }
                ModalState::ScanFolderStep1Platform { .. } => {
                    self.modal_state = ModalState::AddGameStep1Type { selected_type_idx: 0 };
                }
                _ => {
                    self.modal_state = ModalState::None;
                }
            },
            Action::ModalSelectNext => {
                let total_configured_emulators = self.get_configured_emulator_platforms().len();
                let total_unique_runners =
                    self.db.get_unique_runners().map(|r| r.len()).unwrap_or(0);

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
                        ref mut selected_platform_idx,
                    } => {
                        if total_unique_runners > 0 {
                            *selected_platform_idx =
                                (*selected_platform_idx + 1) % total_unique_runners;
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
                        3 if !icons.is_empty() => {
                            *selected_icon_idx = (*selected_icon_idx + 1) % icons.len();
                        }
                        _ => {}
                    },
                    ModalState::ManageWineRunners {
                        ref installed_runners,
                        ref mut selected_idx,
                    } => {
                        if !installed_runners.is_empty() {
                            *selected_idx = (*selected_idx + 1) % installed_runners.len();
                        }
                    }
                    ModalState::ProtonDownloader {
                        ref releases,
                        ref mut selected_release_idx,
                        ..
                    } => {
                        if !releases.is_empty() {
                            *selected_release_idx = (*selected_release_idx + 1) % releases.len();
                        }
                    }
                    ModalState::SelectWineRunnerPicker {
                        ref installed_runners,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !installed_runners.is_empty() {
                            *selected_idx = (*selected_idx + 1) % installed_runners.len();
                        }
                    }
                    ModalState::WineToolsMenu {
                        ref mut selected_idx,
                    } => {
                        *selected_idx = (*selected_idx + 1) % 4;
                    }
                    ModalState::PlatformSelector {
                        ref mut selected_idx,
                    } if !self.platforms.is_empty() => {
                        *selected_idx = (*selected_idx + 1) % self.platforms.len();
                    }
                    ModalState::WindowsGamesManager {
                        ref games,
                        ref mut selected_idx,
                    } => {
                        let len = games.len() + 1;
                        *selected_idx = (*selected_idx + 1) % len;
                    }
                    ModalState::DownloadCoreModal {
                        ref cores,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !cores.is_empty() {
                            *selected_idx = (*selected_idx + 1) % cores.len();
                        }
                    }
                    ModalState::SelectDetectedEmulatorModal {
                        ref candidates,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !candidates.is_empty() {
                            *selected_idx = (*selected_idx + 1) % candidates.len();
                        }
                    }
                    _ => {}
                }
                self.update_visual_media_preview();
            }
            Action::ModalSelectPrev => {
                let total_configured_emulators = self.get_configured_emulator_platforms().len();
                let total_unique_runners =
                    self.db.get_unique_runners().map(|r| r.len()).unwrap_or(0);

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
                        ref mut selected_platform_idx,
                    } => {
                        if total_unique_runners > 0 {
                            if *selected_platform_idx == 0 {
                                *selected_platform_idx = total_unique_runners - 1;
                            } else {
                                *selected_platform_idx -= 1;
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
                        3 if !icons.is_empty() => {
                            if *selected_icon_idx == 0 {
                                *selected_icon_idx = icons.len() - 1;
                            } else {
                                *selected_icon_idx -= 1;
                            }
                        }
                        _ => {}
                    },
                    ModalState::ManageWineRunners {
                        ref installed_runners,
                        ref mut selected_idx,
                    } => {
                        if !installed_runners.is_empty() {
                            if *selected_idx == 0 {
                                *selected_idx = installed_runners.len() - 1;
                            } else {
                                *selected_idx -= 1;
                            }
                        }
                    }
                    ModalState::ProtonDownloader {
                        ref releases,
                        ref mut selected_release_idx,
                        ..
                    } => {
                        if !releases.is_empty() {
                            if *selected_release_idx == 0 {
                                *selected_release_idx = releases.len() - 1;
                            } else {
                                *selected_release_idx -= 1;
                            }
                        }
                    }
                    ModalState::SelectWineRunnerPicker {
                        ref installed_runners,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !installed_runners.is_empty() {
                            if *selected_idx == 0 {
                                *selected_idx = installed_runners.len() - 1;
                            } else {
                                *selected_idx -= 1;
                            }
                        }
                    }
                    ModalState::WineToolsMenu {
                        ref mut selected_idx,
                    } => {
                        if *selected_idx == 0 {
                            *selected_idx = 3;
                        } else {
                            *selected_idx -= 1;
                        }
                    }
                    ModalState::PlatformSelector {
                        ref mut selected_idx,
                    } if !self.platforms.is_empty() => {
                        if *selected_idx == 0 {
                            *selected_idx = self.platforms.len() - 1;
                        } else {
                            *selected_idx -= 1;
                        }
                    }
                    ModalState::WindowsGamesManager {
                        ref games,
                        ref mut selected_idx,
                    } => {
                        let len = games.len() + 1;
                        if *selected_idx == 0 {
                            *selected_idx = len - 1;
                        } else {
                            *selected_idx -= 1;
                        }
                    }
                    ModalState::DownloadCoreModal {
                        ref cores,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !cores.is_empty() {
                            if *selected_idx == 0 {
                                *selected_idx = cores.len() - 1;
                            } else {
                                *selected_idx -= 1;
                            }
                        }
                    }
                    ModalState::SelectDetectedEmulatorModal {
                        ref candidates,
                        ref mut selected_idx,
                        ..
                    } => {
                        if !candidates.is_empty() {
                            if *selected_idx == 0 {
                                *selected_idx = candidates.len() - 1;
                            } else {
                                *selected_idx -= 1;
                            }
                        }
                    }
                    _ => {}
                }
                self.update_visual_media_preview();
            }
            Action::ModalConfirmStep1 => {
                if let ModalState::AddGameStep1Type { selected_type_idx } = self.modal_state.clone() {
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
                        self.open_add_game_form(gtype);
                    }
                }
            }
            Action::ScanModalConfirmPlatform => {
                if let ModalState::ScanFolderStep1Platform {
                    selected_platform_idx,
                } = self.modal_state.clone()
                {
                    let emulator_platforms = self.get_configured_emulator_platforms();
                    if let Some(p) = emulator_platforms.get(selected_platform_idx) {
                        let default_exts = p.default_extensions.join(", ");
                        let saved_folder = self
                            .db
                            .get_scan_folder_for_platform(p.id)
                            .ok()
                            .flatten()
                            .or_else(|| {
                                self.db
                                    .get_scan_folders_for_platform(p.id)
                                    .ok()
                                    .and_then(|folders| folders.first().map(|f| f.path.clone()))
                            })
                            .unwrap_or_else(|| {
                                dirs::home_dir()
                                    .unwrap_or_else(|| PathBuf::from("/home"))
                                    .join("Juegos")
                                    .to_string_lossy()
                                    .to_string()
                            });

                        self.modal_state = ModalState::AddFolderScanForm {
                            platform: p.clone(),
                            folder_path: saved_folder,
                            extensions_input: default_exts,
                            recursive: true,
                            use_dat_auto_id: game_core::dat_downloader::DatDownloader::supports_dat_identification(&p.slug),
                            add_emulator_id: None,
                            add_core: None,
                            selected_field: 0,
                            parent_modal: Some(Box::new(self.modal_state.clone())),
                        };
                        self.seed_add_scan_core();
                    }
                }
            }
            Action::ModalToggleCheckbox => {
                let toggle =
                    |field: usize, offset: usize, gm: &mut bool, mh: &mut bool, gs: &mut bool| {
                        if field == offset {
                            *gm = !*gm;
                        } else if field == offset + 1 {
                            *mh = !*mh;
                        } else if field == offset + 2 {
                            *gs = !*gs;
                        }
                    };
                let toggle_wine = |field: usize,
                                   gm: &mut bool,
                                   mh: &mut bool,
                                   gs: &mut bool,
                                   es: &mut bool,
                                   fs: &mut bool,
                                   dx: &mut bool,
                                   vk: &mut bool| {
                    match field {
                        6 => *gm = !*gm,
                        7 => *mh = !*mh,
                        8 => *gs = !*gs,
                        9 => *es = !*es,
                        10 => *fs = !*fs,
                        11 => *dx = !*dx,
                        12 => *vk = !*vk,
                        _ => {}
                    }
                };
                let offset_for = |gtype: &PlatformType| -> usize {
                    match gtype {
                        PlatformType::Native => 4,
                        _ => 3,
                    }
                };

                if let ModalState::AddGameForm {
                    ref mut gamemode,
                    ref mut mangohud,
                    ref mut gamescope,
                    ref mut esync,
                    ref mut fsync,
                    ref mut dxvk,
                    ref mut vkd3d,
                    selected_field,
                    game_type: ref gtype,
                    ..
                } = self.modal_state
                {
                    match gtype {
                        PlatformType::Wine => {
                            toggle_wine(
                                selected_field,
                                gamemode,
                                mangohud,
                                gamescope,
                                esync,
                                fsync,
                                dxvk,
                                vkd3d,
                            );
                        }
                        _ => {
                            let off = offset_for(gtype);
                            toggle(selected_field, off, gamemode, mangohud, gamescope);
                        }
                    }
                } else if let ModalState::EditGameForm {
                    ref mut gamemode,
                    ref mut mangohud,
                    ref mut gamescope,
                    ref mut esync,
                    ref mut fsync,
                    ref mut dxvk,
                    ref mut vkd3d,
                    selected_field,
                    game_type: ref gtype,
                    ..
                } = self.modal_state
                {
                    match gtype {
                        PlatformType::Wine => {
                            toggle_wine(
                                selected_field,
                                gamemode,
                                mangohud,
                                gamescope,
                                esync,
                                fsync,
                                dxvk,
                                vkd3d,
                            );
                        }
                        _ => {
                            let off = offset_for(gtype);
                            toggle(selected_field, off, gamemode, mangohud, gamescope);
                        }
                    }
                } else if let ModalState::AddFolderScanForm {
                    ref platform,
                    ref mut recursive,
                    ref mut use_dat_auto_id,
                    selected_field,
                    ..
                } = self.modal_state
                {
                    let dat = scan_folder_supports_dat(&platform.slug);
                    if selected_field == 2 {
                        *recursive = !*recursive;
                    } else if dat && selected_field == 3 {
                        *use_dat_auto_id = !*use_dat_auto_id;
                    }
                } else if let ModalState::VisualMediaSelector {
                    active_tab,
                    selected_cover_idx,
                    ref mut chosen_cover_idx,
                    selected_banner_idx,
                    ref mut chosen_banner_idx,
                    selected_icon_idx,
                    ref mut chosen_icon_idx,
                    ..
                } = self.modal_state
                {
                    match active_tab {
                        1 => {
                            *chosen_cover_idx = if *chosen_cover_idx == Some(selected_cover_idx) {
                                None
                            } else {
                                Some(selected_cover_idx)
                            };
                        }
                        2 => {
                            *chosen_banner_idx = if *chosen_banner_idx == Some(selected_banner_idx)
                            {
                                None
                            } else {
                                Some(selected_banner_idx)
                            };
                        }
                        3 => {
                            *chosen_icon_idx = if *chosen_icon_idx == Some(selected_icon_idx) {
                                None
                            } else {
                                Some(selected_icon_idx)
                            };
                        }
                        _ => {}
                    }
                }
            }
            Action::ModalNextField => match self.modal_state {
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ref title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    ..
                } => {
                    let total_fields = match gtype {
                        PlatformType::Emulator => 7,
                        PlatformType::Native => 8,
                        PlatformType::Wine => 14,
                        PlatformType::Steam => 7,
                    };
                    *selected_field = (*selected_field + 1) % total_fields;
                    if *selected_field == 0 {
                        *cursor_pos = title.len();
                    }
                    sync_form_field_cursor(
                        gtype,
                        false,
                        *selected_field,
                        path_cursor,
                        workdir_cursor,
                        prefix_cursor,
                        cmd_cursor,
                        file_path.len(),
                        working_dir.len(),
                        wine_prefix.len(),
                        custom_command.len(),
                    );
                }
                ModalState::EditGameForm {
                    game_id,
                    game_type: ref gtype,
                    ref mut selected_field,
                    ref title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    emulator_override,
                    ..
                } => {
                    let platform_id = self.games.iter().find(|g| g.id == game_id).map(|g| g.platform_id).unwrap_or(0);
                    let has_core = scan_folder_add_has_core(&self.db, platform_id, emulator_override);
                    let total_fields = match gtype {
                        PlatformType::Emulator => 8 + if has_core { 1 } else { 0 },
                        PlatformType::Native => 8,
                        PlatformType::Wine => 14,
                        PlatformType::Steam => 7,
                    };
                    *selected_field = (*selected_field + 1) % total_fields;
                    if *selected_field == 0 {
                        *cursor_pos = title.len();
                    }
                    sync_form_field_cursor(
                        gtype,
                        true,
                        *selected_field,
                        path_cursor,
                        workdir_cursor,
                        prefix_cursor,
                        cmd_cursor,
                        file_path.len(),
                        working_dir.len(),
                        wine_prefix.len(),
                        custom_command.len(),
                    );
                }
                ModalState::ScanFolderForm {
                    ref platform,
                    ref folders,
                    ref mut selected_index,
                } => {
                    let items = build_scan_folder_items(&self.db, platform.id, folders);
                    if !items.is_empty() {
                        *selected_index = (*selected_index + 1) % items.len();
                    }
                }
                ModalState::AddFolderScanForm {
                    ref platform,
                    add_emulator_id,
                    ref mut selected_field,
                    ..
                } => {
                    let has_core = scan_folder_add_has_core(&self.db, platform.id, add_emulator_id);
                    let total = simple_scan_form_total(
                        scan_folder_supports_dat(&platform.slug),
                        has_core,
                    );
                    *selected_field = (*selected_field + 1) % total;
                }
                ModalState::ManageRunnersStep2Config {
                    ref mut selected_row,
                    ..
                } => {
                    *selected_row = 1;
                }
                ModalState::AppSettings {
                    ref mut selected_field,
                    ..
                } => {
                    *selected_field = (*selected_field + 1) % 5;
                }
                _ => {}
            },
            Action::ModalPrevField => match self.modal_state {
                ModalState::AppSettings {
                    ref mut selected_field,
                    ..
                } => {
                    if *selected_field == 0 {
                        *selected_field = 4;
                    } else {
                        *selected_field -= 1;
                    }
                }
                ModalState::AddGameForm {
                    game_type: ref gtype,
                    ref mut selected_field,
                    ref title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    ..
                } => {
                    let total_fields = match gtype {
                        PlatformType::Emulator => 7,
                        PlatformType::Native => 8,
                        PlatformType::Wine => 14,
                        PlatformType::Steam => 7,
                    };
                    if *selected_field == 0 {
                        *selected_field = total_fields - 1;
                    } else {
                        *selected_field -= 1;
                    }
                    if *selected_field == 0 {
                        *cursor_pos = title.len();
                    }
                    sync_form_field_cursor(
                        gtype,
                        false,
                        *selected_field,
                        path_cursor,
                        workdir_cursor,
                        prefix_cursor,
                        cmd_cursor,
                        file_path.len(),
                        working_dir.len(),
                        wine_prefix.len(),
                        custom_command.len(),
                    );
                }
                ModalState::EditGameForm {
                    game_id,
                    game_type: ref gtype,
                    ref mut selected_field,
                    ref title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
                    ref file_path,
                    ref working_dir,
                    ref wine_prefix,
                    ref custom_command,
                    emulator_override,
                    ..
                } => {
                    let platform_id = self.games.iter().find(|g| g.id == game_id).map(|g| g.platform_id).unwrap_or(0);
                    let has_core = scan_folder_add_has_core(&self.db, platform_id, emulator_override);
                    let total_fields = match gtype {
                        PlatformType::Emulator => 8 + if has_core { 1 } else { 0 },
                        PlatformType::Native => 8,
                        PlatformType::Wine => 14,
                        PlatformType::Steam => 7,
                    };
                    if *selected_field == 0 {
                        *selected_field = total_fields - 1;
                    } else {
                        *selected_field -= 1;
                    }
                    if *selected_field == 0 {
                        *cursor_pos = title.len();
                    }
                    sync_form_field_cursor(
                        gtype,
                        true,
                        *selected_field,
                        path_cursor,
                        workdir_cursor,
                        prefix_cursor,
                        cmd_cursor,
                        file_path.len(),
                        working_dir.len(),
                        wine_prefix.len(),
                        custom_command.len(),
                    );
                }
                ModalState::ScanFolderForm {
                    ref platform,
                    ref folders,
                    ref mut selected_index,
                } => {
                    let items = build_scan_folder_items(&self.db, platform.id, folders);
                    if !items.is_empty() {
                        *selected_index = if *selected_index == 0 {
                            items.len() - 1
                        } else {
                            *selected_index - 1
                        };
                    }
                }
                ModalState::AddFolderScanForm {
                    ref platform,
                    add_emulator_id,
                    ref mut selected_field,
                    ..
                } => {
                    let has_core = scan_folder_add_has_core(&self.db, platform.id, add_emulator_id);
                    let total = simple_scan_form_total(
                        scan_folder_supports_dat(&platform.slug),
                        has_core,
                    );
                    *selected_field = if *selected_field == 0 {
                        total - 1
                    } else {
                        *selected_field - 1
                    };
                }
                ModalState::ManageRunnersStep2Config {
                    ref mut selected_row,
                    ..
                } => {
                    *selected_row = 0;
                }
                _ => {}
            },
            Action::ModalInputChar(ch) => {
                let is_edit = matches!(self.modal_state, ModalState::EditGameForm { .. });
                if let ModalState::AddGameForm {
                    ref mut title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
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
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
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
                    if selected_field == 0 {
                        let pos = (*cursor_pos).min(title.len());
                        title.insert(pos, ch);
                        *cursor_pos = pos + 1;
                    } else {
                        match gtype {
                            PlatformType::Emulator => match selected_field {
                                1 if is_edit => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    file_path.insert(pos, ch);
                                    *path_cursor = pos + 1;
                                }
                                2 if !is_edit => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    file_path.insert(pos, ch);
                                    *path_cursor = pos + 1;
                                }
                                3 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    custom_command.insert(pos, ch);
                                    *cmd_cursor = pos + 1;
                                }
                                _ => {}
                            },
                            PlatformType::Native => match selected_field {
                                1 => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    file_path.insert(pos, ch);
                                    *path_cursor = pos + 1;
                                    if let Some(parent) = std::path::Path::new(file_path).parent() {
                                        if !parent.as_os_str().is_empty() {
                                            *working_dir = parent.to_string_lossy().to_string();
                                        }
                                    }
                                }
                                2 => {
                                    let pos = (*workdir_cursor).min(working_dir.len());
                                    working_dir.insert(pos, ch);
                                    *workdir_cursor = pos + 1;
                                }
                                3 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    custom_command.insert(pos, ch);
                                    *cmd_cursor = pos + 1;
                                }
                                _ => {}
                            },
                            PlatformType::Wine => match selected_field {
                                1 => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    file_path.insert(pos, ch);
                                    *path_cursor = pos + 1;
                                    if let Some(parent) = std::path::Path::new(file_path).parent() {
                                        if !parent.as_os_str().is_empty() {
                                            *working_dir = parent.to_string_lossy().to_string();
                                            if wine_prefix.trim().is_empty() {
                                                *wine_prefix = parent
                                                    .join("prefix")
                                                    .to_string_lossy()
                                                    .to_string();
                                            }
                                        }
                                    }
                                }
                                2 => {
                                    let pos = (*prefix_cursor).min(wine_prefix.len());
                                    wine_prefix.insert(pos, ch);
                                    *prefix_cursor = pos + 1;
                                }
                                3 => {
                                    let pos = (*workdir_cursor).min(working_dir.len());
                                    working_dir.insert(pos, ch);
                                    *workdir_cursor = pos + 1;
                                }
                                4 => {}
                                5 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    custom_command.insert(pos, ch);
                                    *cmd_cursor = pos + 1;
                                }
                                _ => {}
                            },
                            PlatformType::Steam => match selected_field {
                                1 => {
                                    if ch.is_ascii_digit() {
                                        steam_appid.push(ch);
                                    }
                                }
                                2 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    custom_command.insert(pos, ch);
                                    *cmd_cursor = pos + 1;
                                }
                                _ => {}
                            },
                        }
                    }
                } else if let ModalState::ManageRunnersStep2Config {
                    ref options,
                    ref selected_row,
                    ref mut exe_path_input,
                    ref mut custom_args,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    if *selected_row == 0 {
                        let pos = (*cursor_pos).min(exe_path_input.len());
                        exe_path_input.insert(pos, ch);
                        *cursor_pos = pos + 1;
                    } else if *selected_row == options.len() + 1 {
                        let pos = (*cursor_pos).min(custom_args.len());
                        custom_args.insert(pos, ch);
                        *cursor_pos = pos + 1;
                    }
                } else if let ModalState::ConfigureApiKeyInput { ref mut input } = self.modal_state
                {
                    input.push(ch);
                } else if let ModalState::AppSettings {
                    selected_field: 0,
                    is_editing_api_key: true,
                    ref mut api_key_input,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(api_key_input.len());
                    api_key_input.insert(pos, ch);
                    *cursor_pos = pos + 1;
                } else if let ModalState::VisualMediaSelector {
                    active_tab: 0,
                    focused_section: 1,
                    ref mut search_query,
                    ref mut cursor_pos,
                    ref mut candidates,
                    ref mut selected_candidate_idx,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(search_query.len());
                    search_query.insert(pos, ch);
                    *cursor_pos = pos + 1;
                    candidates.clear();
                    *selected_candidate_idx = 0;
                } else if let ModalState::EditCustomArgsInput {
                    ref mut input,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(input.len());
                    input.insert(pos, ch);
                    *cursor_pos = pos + 1;
                } else if let ModalState::WelcomeWizard {
                    step: 2,
                    ref mut sgdb_api_key,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(sgdb_api_key.len());
                    sgdb_api_key.insert(pos, ch);
                    *cursor_pos = pos + 1;
                }
            }
            Action::ModalBackspace => {
                let is_edit = matches!(self.modal_state, ModalState::EditGameForm { .. });
                if let ModalState::AddGameForm {
                    ref mut title,
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
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
                    ref mut cursor_pos,
                    ref mut path_cursor,
                    ref mut workdir_cursor,
                    ref mut prefix_cursor,
                    ref mut cmd_cursor,
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
                    if selected_field == 0 {
                        if *cursor_pos > 0 && !title.is_empty() {
                            let pos = (*cursor_pos - 1).min(title.len() - 1);
                            title.remove(pos);
                            *cursor_pos = pos;
                        }
                    } else {
                        match gtype {
                            PlatformType::Emulator => match selected_field {
                                1 if is_edit => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    if pos > 0 && !file_path.is_empty() {
                                        file_path.remove(pos - 1);
                                        *path_cursor = pos - 1;
                                    }
                                }
                                2 if !is_edit => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    if pos > 0 && !file_path.is_empty() {
                                        file_path.remove(pos - 1);
                                        *path_cursor = pos - 1;
                                    }
                                }
                                3 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    if pos > 0 && !custom_command.is_empty() {
                                        custom_command.remove(pos - 1);
                                        *cmd_cursor = pos - 1;
                                    }
                                }
                                _ => {}
                            },
                            PlatformType::Native => match selected_field {
                                1 => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    if pos > 0 && !file_path.is_empty() {
                                        file_path.remove(pos - 1);
                                        *path_cursor = pos - 1;
                                    }
                                    if let Some(parent) = std::path::Path::new(file_path).parent() {
                                        if !parent.as_os_str().is_empty() {
                                            *working_dir = parent.to_string_lossy().to_string();
                                        } else {
                                            working_dir.clear();
                                        }
                                    }
                                }
                                2 => {
                                    let pos = (*workdir_cursor).min(working_dir.len());
                                    if pos > 0 && !working_dir.is_empty() {
                                        working_dir.remove(pos - 1);
                                        *workdir_cursor = pos - 1;
                                    }
                                }
                                3 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    if pos > 0 && !custom_command.is_empty() {
                                        custom_command.remove(pos - 1);
                                        *cmd_cursor = pos - 1;
                                    }
                                }
                                _ => {}
                            },
                            PlatformType::Wine => match selected_field {
                                1 => {
                                    let pos = (*path_cursor).min(file_path.len());
                                    if pos > 0 && !file_path.is_empty() {
                                        file_path.remove(pos - 1);
                                        *path_cursor = pos - 1;
                                    }
                                    if let Some(parent) = std::path::Path::new(file_path).parent() {
                                        if !parent.as_os_str().is_empty() {
                                            *working_dir = parent.to_string_lossy().to_string();
                                        } else {
                                            working_dir.clear();
                                        }
                                    }
                                }
                                2 => {
                                    let pos = (*prefix_cursor).min(wine_prefix.len());
                                    if pos > 0 && !wine_prefix.is_empty() {
                                        wine_prefix.remove(pos - 1);
                                        *prefix_cursor = pos - 1;
                                    }
                                }
                                3 => {
                                    let pos = (*workdir_cursor).min(working_dir.len());
                                    if pos > 0 && !working_dir.is_empty() {
                                        working_dir.remove(pos - 1);
                                        *workdir_cursor = pos - 1;
                                    }
                                }
                                4 => {}
                                5 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    if pos > 0 && !custom_command.is_empty() {
                                        custom_command.remove(pos - 1);
                                        *cmd_cursor = pos - 1;
                                    }
                                }
                                _ => {}
                            },
                            PlatformType::Steam => match selected_field {
                                1 => {
                                    steam_appid.pop();
                                }
                                2 => {
                                    let pos = (*cmd_cursor).min(custom_command.len());
                                    if pos > 0 && !custom_command.is_empty() {
                                        custom_command.remove(pos - 1);
                                        *cmd_cursor = pos - 1;
                                    }
                                }
                                _ => {}
                            },
                        }
                    }
                } else if let ModalState::ManageRunnersStep2Config {
                    ref options,
                    ref selected_row,
                    ref mut exe_path_input,
                    ref mut custom_args,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    if *selected_row == 0 {
                        let pos = (*cursor_pos).min(exe_path_input.len());
                        if pos > 0 && !exe_path_input.is_empty() {
                            exe_path_input.remove(pos - 1);
                            *cursor_pos = pos - 1;
                        }
                    } else if *selected_row == options.len() + 1 {
                        let pos = (*cursor_pos).min(custom_args.len());
                        if pos > 0 && !custom_args.is_empty() {
                            custom_args.remove(pos - 1);
                            *cursor_pos = pos - 1;
                        }
                    }
                } else if let ModalState::ConfigureApiKeyInput { ref mut input } = self.modal_state
                {
                    input.pop();
                } else if let ModalState::AppSettings {
                    selected_field: 0,
                    is_editing_api_key: true,
                    ref mut api_key_input,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(api_key_input.len());
                    if pos > 0 && !api_key_input.is_empty() {
                        api_key_input.remove(pos - 1);
                        *cursor_pos = pos - 1;
                    }
                } else if let ModalState::VisualMediaSelector {
                    active_tab: 0,
                    focused_section: 1,
                    ref mut search_query,
                    ref mut cursor_pos,
                    ref mut candidates,
                    ref mut selected_candidate_idx,
                    ..
                } = self.modal_state
                {
                    if *cursor_pos > 0 && !search_query.is_empty() {
                        let pos = (*cursor_pos - 1).min(search_query.len() - 1);
                        search_query.remove(pos);
                        *cursor_pos = pos;
                        candidates.clear();
                        *selected_candidate_idx = 0;
                    }
                } else if let ModalState::EditCustomArgsInput {
                    ref mut input,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(input.len());
                    if pos > 0 {
                        input.remove(pos - 1);
                        *cursor_pos = pos - 1;
                    }
                } else if let ModalState::WelcomeWizard {
                    step: 2,
                    ref mut sgdb_api_key,
                    ref mut cursor_pos,
                    ..
                } = self.modal_state
                {
                    let pos = (*cursor_pos).min(sgdb_api_key.len());
                    if pos > 0 {
                        sgdb_api_key.remove(pos - 1);
                        *cursor_pos = pos - 1;
                    }
                }
            }
            Action::StartFolderScan => {
                if let ModalState::ScanFolderForm {
                    ref platform,
                    ref folders,
                    ..
                } = self.modal_state.clone()
                {
                    if folders.is_empty() {
                        self.status_msg =
                            "Error: no folders registered. Use [ + Add New Folder ] first.".to_string();
                        return;
                    }
                    let use_dat_auto_id = scan_folder_supports_dat(&platform.slug);
                    let mut jobs: Vec<(Platform, PathBuf, bool, bool, Option<i64>)> = Vec::new();
                    for folder in folders {
                        let path = PathBuf::from(&folder.path);
                        if !path.exists() || !path.is_dir() {
                            self.status_msg = format!(
                                "[Error] Folder not found, skipping: '{}'",
                                folder.path
                            );
                            continue;
                        }
                        jobs.push((
                            platform.clone(),
                            path,
                            folder.recursive,
                            use_dat_auto_id,
                            Some(folder.id),
                        ));
                    }
                    if jobs.is_empty() {
                        self.status_msg = "[Error] No valid folders to scan.".to_string();
                        return;
                    }
                    self.begin_scan_many(jobs);
                }
            }
            Action::RescanFolder => {
                if let ModalState::ScanFolderForm {
                    ref platform,
                    ref folders,
                    selected_index,
                    ..
                } = self.modal_state.clone()
                {
                    let items = build_scan_folder_items(&self.db, platform.id, &folders);
                    let row = match items.get(selected_index) {
                        Some(ScanFolderItem::FolderEmulator(i)) | Some(ScanFolderItem::FolderCore(i)) => *i,
                        _ => return,
                    };
                    if row >= folders.len() {
                        return;
                    }
                    let folder = folders[row].clone();
                    let path = PathBuf::from(&folder.path);
                    if !path.exists() {
                        self.status_msg = format!(
                            "[Error] Saved folder not found: '{}'",
                            folder.path
                        );
                        return;
                    }

                    let selected_extensions = platform.default_extensions.clone();
                    let use_dat_auto_id = scan_folder_supports_dat(&platform.slug);

                    let mut scan_platform = platform.clone();
                    scan_platform.default_extensions = selected_extensions;

                    let _ = self.db.touch_scan_folder(folder.id);
                    self.begin_scan(
                        scan_platform,
                        path,
                        folder.recursive,
                        use_dat_auto_id,
                        Some(folder.id),
                    );
                }
            }
            Action::CycleFolderEmulator(backward) => {
                let platform_id;
                let platform_name;
                let folder_path;
                let folder_index;
                let (folder_id, current_id, configured, no_runners) = {
                    let ModalState::ScanFolderForm {
                        ref platform,
                        ref folders,
                        selected_index,
                    } = self.modal_state
                    else {
                        return;
                    };
                    let items = build_scan_folder_items(&self.db, platform.id, folders);
                    let row = match items.get(selected_index) {
                        Some(ScanFolderItem::FolderEmulator(i)) | Some(ScanFolderItem::FolderCore(i)) => *i,
                        _ => return,
                    };
                    if row >= folders.len() {
                        return;
                    }
                    folder_index = row;
                    platform_id = platform.id;
                    platform_name = platform.name.clone();
                    folder_path = folders[row].path.clone();
                    let configured = self.configured_runners_for(platform.id);
                    let no_runners = configured.is_empty();
                    (folders[row].id, folders[row].assigned_emulator_id, configured, no_runners)
                };
                if no_runners {
                    self.status_msg = format!(
                        "No hay emulador configurado para {}. Presiona [m] para configurar uno.",
                        platform_name
                    );
                    return;
                }
                // Cycle: heredado -> emulador 1..n -> heredado.
                let current = current_id.and_then(|id| configured.iter().position(|r| r.id == id));
                let next_idx = if let Some(idx) = current {
                    if !backward {
                        if idx + 1 < configured.len() {
                            Some(idx + 1)
                        } else {
                            None
                        }
                    } else if idx > 0 {
                        Some(idx - 1)
                    } else {
                        None
                    }
                } else if backward {
                    Some(configured.len() - 1)
                } else {
                    Some(0)
                };
                let next_id = next_idx.map(|i| configured[i].id);
                if let Ok(()) = self.db.set_folder_assigned_emulator(folder_id, next_id) {
                    let label = match next_id {
                        Some(id) => configured
                            .iter()
                            .find(|r| r.id == id)
                            .map(|r| r.name.clone())
                            .unwrap_or_else(|| "?".to_string()),
                        None => "Default".to_string(),
                    };
                    self.status_msg = format!("Carpeta {} → {}", folder_path, label);
                    self.reload_scan_folder_modal(platform_id);
                }
            }
            Action::CycleFolderCore(backward) => {
                let platform_id;
                let folder_id;
                let emu_id;
                let current_core;
                {
                    let ModalState::ScanFolderForm {
                        ref platform,
                        ref folders,
                        selected_index,
                    } = self.modal_state
                    else {
                        return;
                    };
                    let items = build_scan_folder_items(&self.db, platform.id, folders);
                    let row = match items.get(selected_index) {
                        Some(ScanFolderItem::FolderCore(i)) => *i,
                        _ => return,
                    };
                    if row >= folders.len() {
                        return;
                    }
                    platform_id = platform.id;
                    folder_id = folders[row].id;
                    emu_id = folders[row].assigned_emulator_id;
                    current_core = folders[row].assigned_core.clone();
                }
                let platform = match self.platforms.iter().find(|p| p.id == platform_id).cloned() {
                    Some(p) => p,
                    None => return,
                };
                let available = add_scan_available_cores(&self.db, &platform, emu_id);
                if available.is_empty() {
                    return;
                }
                let curr_pos = current_core
                    .as_deref()
                    .and_then(|k| available.iter().position(|(ck, _)| ck == k));
                let next_pos = if let Some(pos) = curr_pos {
                    if !backward {
                        if pos + 1 < available.len() {
                            Some(pos + 1)
                        } else {
                            None
                        }
                    } else if pos > 0 {
                        Some(pos - 1)
                    } else {
                        None
                    }
                } else if backward {
                    Some(available.len() - 1)
                } else {
                    Some(0)
                };
                let next_core_key = next_pos.map(|i| available[i].0.clone());
                let _ = self.db.set_folder_assigned_core(folder_id, next_core_key.as_deref());
                self.reload_scan_folder_modal(platform_id);
            }
            Action::CycleEditGameEmulator(backward) => {
                self.cycle_edit_game_emulator(backward);
            }
            Action::OpenConfirmDeleteFolder => {
                let folder_idx = match &self.modal_state {
                    ModalState::ScanFolderForm {
                        platform,
                        folders,
                        selected_index,
                    } => {
                        let items = build_scan_folder_items(&self.db, platform.id, folders);
                        match items.get(*selected_index) {
                            Some(ScanFolderItem::FolderEmulator(i)) | Some(ScanFolderItem::FolderCore(i)) => Some(*i),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(idx) = folder_idx {
                    self.open_confirm_delete_folder_for_index(idx);
                }
            }
            Action::OpenConfirmDeleteFolderForIndex(folder_idx) => {
                self.open_confirm_delete_folder_for_index(folder_idx);
            }
            Action::ToggleConfirmDeleteFolderOption => {
                if let ModalState::ConfirmDeleteFolder {
                    ref mut selected_option,
                    ..
                } = self.modal_state
                {
                    *selected_option = if *selected_option == 0 { 1 } else { 0 };
                }
            }
            Action::SwitchScanFolderPane => {}
            Action::ConfirmDeleteFolderExecution => {
                if let ModalState::ConfirmDeleteFolder {
                    platform_id,
                    folder_ids,
                    display,
                    selected_option,
                    parent_modal,
                    ..
                } = self.modal_state.clone()
                {
                    if selected_option == 1 {
                        let mut ok = 0;
                        let mut err: Option<String> = None;
                        for folder_id in &folder_ids {
                            match self.db.delete_scan_folder(*folder_id, true) {
                                Ok(_) => ok += 1,
                                Err(e) => {
                                    err = Some(format!("{}", e));
                                    break;
                                }
                            }
                        }
                        self.status_msg = match err {
                            Some(e) => format!("Error deleting folder: {}", e),
                            None => format!("[OK] Removed folder '{}'.", display),
                        };
                    }
                    if let Some(parent) = parent_modal {
                        self.modal_state = *parent;
                        self.reload_scan_folder_modal(platform_id);
                    } else {
                        self.reload_scan_folder_modal(platform_id);
                    }
                }
            }
            Action::OpenAddFolderModal => {
                if let ModalState::ScanFolderForm { ref platform, .. } = self.modal_state {
                    let default_exts = platform.default_extensions.join(", ");
                    let parent_state = self.modal_state.clone();
                    self.modal_state = ModalState::AddFolderScanForm {
                        platform: platform.clone(),
                        folder_path: String::new(),
                        extensions_input: default_exts,
                        recursive: true,
                        use_dat_auto_id: scan_folder_supports_dat(&platform.slug),
                        add_emulator_id: None,
                        add_core: None,
                        selected_field: 0,
                        parent_modal: Some(Box::new(parent_state)),
                    };
                    self.seed_add_scan_core();
                }
            }
            Action::AddFolder => {
                let ModalState::AddFolderScanForm {
                    ref platform,
                    ref folder_path,
                    ref extensions_input,
                    recursive,
                    add_emulator_id,
                    ref add_core,
                    ref parent_modal,
                    ..
                } = self.modal_state.clone()
                else {
                    return;
                };
                let path = folder_path.trim();
                if path.is_empty() {
                    self.status_msg =
                        "Error: enter a folder path or press [Enter] on Path to browse.".to_string();
                    return;
                }
                let pathbuf = PathBuf::from(path);
                if !pathbuf.exists() || !pathbuf.is_dir() {
                    self.status_msg = format!(
                        "Error: folder does not exist or is not a directory: '{}'",
                        path
                    );
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
                let folder_id = match self.db.save_scan_folder(platform.id, path, recursive) {
                    Ok(id) => id,
                    Err(e) => {
                        self.status_msg = format!("Error registering folder: {}", e);
                        return;
                    }
                };
                if add_emulator_id.is_some() {
                    let _ = self.db.set_folder_assigned_emulator(folder_id, add_emulator_id);
                }
                if add_core.is_some() {
                    let _ = self.db.set_folder_assigned_core(folder_id, add_core.as_deref());
                }
                self.status_msg = format!("[OK] Folder registered: '{}'.", path);
                let platform_id = platform.id;
                let path_owned = path.to_string();
                if let Some(parent) = parent_modal {
                    self.modal_state = *parent.clone();
                }
                self.reload_scan_folder_modal(platform_id);
                if let ModalState::ScanFolderForm {
                    ref folders,
                    ref mut selected_index,
                    ..
                } = self.modal_state
                {
                    let items = build_scan_folder_items(&self.db, platform_id, folders);
                    let idx = items.iter().position(|item| match item {
                        ScanFolderItem::FolderEmulator(i) => folders.get(*i).map(|f| &f.path) == Some(&path_owned),
                        _ => false,
                    }).unwrap_or(0);
                    *selected_index = idx;
                }
            }
            Action::OpenDetectEmulatorModal => {
                if let ModalState::ManageRunnersStep2Config { ref runner_info, .. } = self.modal_state.clone() {
                    let detector = game_core::emulator_detector::EmulatorDetector::new();
                    let candidates = detector.detect_for_emulator(&runner_info.name);

                    if candidates.is_empty() {
                        self.status_msg = format!("No installed versions detected for '{}'.", runner_info.name);
                    } else if candidates.len() == 1 {
                        let cmd = candidates[0].launch_command();
                        if let ModalState::ManageRunnersStep2Config { ref mut exe_path_input, ref mut cursor_pos, .. } = self.modal_state {
                            *exe_path_input = cmd.clone();
                            *cursor_pos = cmd.len();
                        }
                        self.status_msg = format!("[OK] Detected and set executable for '{}': {}", runner_info.name, cmd);
                    } else {
                        self.modal_state = ModalState::SelectDetectedEmulatorModal {
                            runner_name: runner_info.name.clone(),
                            candidates,
                            selected_idx: 0,
                            parent_modal: Box::new(self.modal_state.clone()),
                        };
                    }
                }
            }
            Action::SelectDetectedEmulatorCandidate => {
                if let ModalState::SelectDetectedEmulatorModal {
                    ref candidates,
                    selected_idx,
                    parent_modal,
                    ..
                } = self.modal_state.clone()
                {
                    if let Some(cand) = candidates.get(selected_idx) {
                        let cmd = cand.launch_command();
                        let mut new_parent = *parent_modal;
                        if let ModalState::ManageRunnersStep2Config { ref mut exe_path_input, ref mut cursor_pos, .. } = new_parent {
                            *exe_path_input = cmd.clone();
                            *cursor_pos = cmd.len();
                        }
                        self.modal_state = new_parent;
                        self.status_msg = format!("[OK] Selected detected executable: {}", cmd);
                    }
                }
            }
            Action::OpenDownloadCoreModal => {
                let current_modal = self.modal_state.clone();
                let (platform, runner_id) = match &current_modal {
                    ModalState::ScanFolderForm { platform, .. } => (platform.clone(), None),
                    ModalState::AddFolderScanForm { platform, add_emulator_id, .. } => (platform.clone(), *add_emulator_id),
                    _ => return,
                };
                let runners = self.db.get_runners_for_platform(platform.id).unwrap_or_default();
                let runner = match runner_id {
                    Some(id) => runners.into_iter().find(|r| r.id == id),
                    None => {
                        // From the folder manager there is no emulator override:
                        // prefer the active runner, but fall back to a configured
                        // RetroArch one when the active emulator is not RetroArch
                        // (e.g. NDS active=melonDS but RetroArch is configured).
                        let active = runners.iter().find(|r| r.is_active);
                        match active {
                            Some(r) if is_retroarch_runner(r) => Some(r.clone()),
                            _ => runners
                                .iter()
                                .find(|r| is_retroarch_runner(r))
                                .cloned(),
                        }
                    }
                };
                if let Some(r) = runner {
                    let cores = game_core::core_catalog::cores_for_platform(&platform.slug);
                    let cores_dir = game_core::retroarch_manager::resolve_retroarch_cores_dir(&r);
                    let downloaded_keys: std::collections::HashSet<String> = cores
                        .iter()
                        .filter(|c| cores_dir.join(&c.so_file).is_file())
                        .map(|c| c.key.clone())
                        .collect();
                    self.modal_state = ModalState::DownloadCoreModal {
                        platform,
                        runner: r,
                        cores,
                        downloaded_keys,
                        selected_idx: 0,
                        parent_state: Box::new(current_modal),
                    };
                }
            }
            Action::TriggerDownloadCore => {
                if let ModalState::DownloadCoreModal {
                    platform: _,
                    ref runner,
                    ref cores,
                    ref mut downloaded_keys,
                    selected_idx,
                    ..
                } = self.modal_state.clone()
                {
                    if let Some(core) = cores.get(selected_idx) {
                        let cores_dir = game_core::retroarch_manager::resolve_retroarch_cores_dir(&runner);
                        let core_info = core.clone();
                        let (tx, rx) = mpsc::channel::<DownloadEvent>(100);
                        self.download_rx = Some(rx);
                        self.download_progress = Some(DownloadProgressState {
                            runner_id: 0,
                            runner_name: format!("Core: {}", core_info.name),
                            downloaded_bytes: 0,
                            total_bytes: 0,
                            percentage: 0.0,
                            is_finished: false,
                            error_msg: None,
                        });
                        self.status_msg = format!("Downloading core {}...", core_info.name);

                        let mut new_keys = downloaded_keys.clone();
                        new_keys.insert(core_info.key.clone());
                        if let ModalState::DownloadCoreModal { ref mut downloaded_keys, .. } = self.modal_state {
                            *downloaded_keys = new_keys;
                        }

                        tokio::spawn(async move {
                            let res = game_core::core_catalog::ensure_core_downloaded(&core_info, &cores_dir).await;
                            let _ = tx.send(DownloadEvent {
                                downloaded: 100,
                                total: 100,
                                percentage: 100.0,
                                finished: true,
                                error: res.err().map(|e| e.to_string()),
                                task_name: Some(format!("Core: {}", core_info.name)),
                            }).await;
                        });
                    }
                }
            }
            Action::StartAddAndScan => {
                let ModalState::AddFolderScanForm {
                    ref platform,
                    ref folder_path,
                    ref extensions_input,
                    recursive,
                    use_dat_auto_id,
                    add_emulator_id,
                    ref add_core,
                    ..
                } = self.modal_state.clone()
                else {
                    return;
                };
                let path = folder_path.trim();
                if path.is_empty() {
                    self.status_msg =
                        "Error: enter a folder path or press [Enter] on Path to browse.".to_string();
                    return;
                }
                let pathbuf = PathBuf::from(path);
                if !pathbuf.exists() || !pathbuf.is_dir() {
                    self.status_msg = format!(
                        "Error: folder does not exist or is not a directory: '{}'",
                        path
                    );
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
                let folder_id = match self.db.save_scan_folder(platform.id, path, recursive) {
                    Ok(id) => id,
                    Err(e) => {
                        self.status_msg = format!("Error registering folder: {}", e);
                        return;
                    }
                };
                if add_emulator_id.is_some() {
                    let _ = self.db.set_folder_assigned_emulator(folder_id, add_emulator_id);
                }
                if add_core.is_some() {
                    let _ = self.db.set_folder_assigned_core(folder_id, add_core.as_deref());
                }
                let mut scan_platform = platform.clone();
                scan_platform.default_extensions = selected_extensions;
                self.status_msg = format!("[OK] Folder registered: '{}'.", path);
                self.begin_scan(
                    scan_platform,
                    pathbuf,
                    recursive,
                    use_dat_auto_id,
                    Some(folder_id),
                );
            }
            Action::OpenFolderManagerForPlatform => {
                if self.modal_state == ModalState::None
                    && !self.platforms.is_empty()
                    && self.selected_platform_idx < self.platforms.len()
                {
                    let platform = &self.platforms[self.selected_platform_idx];
                    match platform.slug.as_str() {
                        // Steam games live in the Steam library, not in scan
                        // folders: there is nothing to manage here.
                        "steam" => {}
                        // Windows games are registered manually via the Wine
                        // details form; the manager is a browsable list of
                        // installed titles (no scan folders involved).
                        "windows" => {
                            let games = self
                                .db
                                .get_games_for_platform(platform.id)
                                .unwrap_or_default();
                            self.games = games.clone();
                            self.selected_game_idx = 0;
                            self.modal_state = ModalState::WindowsGamesManager {
                                games,
                                selected_idx: 0,
                            };
                        }
                        _ => self.reload_scan_folder_modal(platform.id),
                    }
                }
            }
            Action::ToggleSelectFolder => {}
            Action::QuickRescanPlatform => {
                if self.modal_state == ModalState::None && !self.platforms.is_empty() {
                    let platform = self.platforms[self.selected_platform_idx].clone();
                    if let Ok(Some(saved_path)) = self.db.get_scan_folder_for_platform(platform.id)
                    {
                        let path = PathBuf::from(&saved_path);
                        if path.exists() {
                            let folder_id = self
                                .db
                                .get_scan_folders_for_platform(platform.id)
                                .ok()
                                .and_then(|folders| {
                                    folders
                                        .into_iter()
                                        .find(|f| f.path == saved_path)
                                        .map(|f| f.id)
                                });
                            let (scan_tx, scan_rx) =
                                mpsc::channel::<game_core::scanner::ScanProgressEvent>(100);
                            self.scan_rx = Some(scan_rx);
                            self.download_progress = Some(DownloadProgressState {
                                runner_id: 0,
                                runner_name: format!("Quick Re-scanning: {}", platform.name),
                                downloaded_bytes: 0,
                                total_bytes: 1,
                                percentage: 0.0,
                                is_finished: false,
                                error_msg: None,
                            });
                            self.status_msg =
                                format!("Re-scanning & Identifying ROMs for {}...", platform.name);

                            let db_path = dirs::data_dir()
                                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                .join("tui_game_station")
                                .join("game_station.db");

                            tokio::spawn(async move {
                                let slug = platform.slug.clone();
                                let supports_dat = game_core::dat_downloader::DatDownloader::supports_dat_identification(&slug);
                                if supports_dat {
                                    let _ = game_core::dat_downloader::DatDownloader::ensure_dat_downloaded(&slug).await;
                                }

                                let (sync_tx, sync_rx) = std::sync::mpsc::channel();
                                let scan_tx_clone = scan_tx.clone();

                                tokio::task::spawn_blocking(move || {
                                    if let Ok(db) = Database::open(&db_path) {
                                        let _ = Scanner::scan_folder(
                                            &db,
                                            &platform,
                                            &path,
                                            true,
                                            false,
                                            supports_dat,
                                            folder_id,
                                            Some(&sync_tx),
                                        );
                                    }
                                });

                                while let Ok(evt) = sync_rx.recv() {
                                    let _ = scan_tx_clone.send(evt).await;
                                }
                            });
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
            Action::DeleteSelectedGames | Action::OpenConfirmDeleteModal => {
                if self.modal_state == ModalState::None && !self.games.is_empty() {
                    let (game_ids, display_title) = if !self.selected_game_ids.is_empty() {
                        let ids: Vec<i64> = self.selected_game_ids.iter().copied().collect();
                        let title = format!("{} selected games", ids.len());
                        (ids, title)
                    } else if self.selected_game_idx < self.games.len() {
                        let game = &self.games[self.selected_game_idx];
                        (vec![game.id], game.title.clone())
                    } else {
                        return;
                    };

                    self.modal_state = ModalState::ConfirmDeleteGame {
                        game_ids,
                        display_title,
                        selected_option: 0,
                    };
                }
            }
            Action::ToggleConfirmDeleteOption => {
                if let ModalState::ConfirmDeleteGame {
                    ref mut selected_option,
                    ..
                } = self.modal_state
                {
                    *selected_option = if *selected_option == 0 { 1 } else { 0 };
                }
            }
            Action::ConfirmDeleteGameExecution => {
                if let ModalState::ConfirmDeleteGame {
                    game_ids,
                    display_title,
                    selected_option,
                } = self.modal_state.clone()
                {
                    if selected_option == 1 {
                        let count = self.db.delete_games(&game_ids).unwrap_or(0);
                        self.selected_game_ids.clear();
                        self.status_msg = format!(
                            "[OK] Removed {} ('{}') from library.",
                            if game_ids.len() > 1 {
                                format!("{} games", count)
                            } else {
                                "game".to_string()
                            },
                            display_title
                        );
                        self.modal_state = ModalState::None;
                        self.load_platforms();
                    } else {
                        self.modal_state = ModalState::None;
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
                        self.media_protocols.remove(&(g.id, "icon_hb".to_string()));
                        self.pending_cover_requests.insert(g.id);
                    }

                    let total_games = target_games.len();
                    self.download_progress = Some(DownloadProgressState {
                        runner_id: 0,
                        runner_name: format!(
                            "SteamGridDB Media (0/{}) - {}",
                            total_games, target_games[0].title
                        ),
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
                        let client = std::sync::Arc::new(
                            scraper::steamgriddb::SteamGridDBClient::new(Some(key_str)),
                        );
                        let db_path = dirs::data_dir()
                            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                            .join("tui_game_station")
                            .join("game_station.db");

                        let completed_count =
                            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(3));
                        let mut tasks = Vec::new();

                        for game in target_games {
                            let sem = semaphore.clone();
                            let client_c = client.clone();
                            let db_path_c = db_path.clone();
                            let tx_c = tx.clone();
                            let progress_tx_c = progress_tx.clone();
                            let manager_c = manager.clone();
                            let counter_c = completed_count.clone();
                            let total = total_games;

                            tasks.push(tokio::spawn(async move {
                                let _permit = sem.acquire().await;

                                let res = client_c
                                    .download_all_media_for_game(
                                        Some(db_path_c),
                                        game.id,
                                        &game.title,
                                        true,
                                    )
                                    .await;

                                let protocol = match res {
                                    Ok(ref media) => {
                                        if let Some(ref path) = media.cover_path {
                                            manager_c.load_protocol_from_file(path)
                                        } else {
                                            None
                                        }
                                    }
                                    Err(_) => None,
                                };

                                let _ = tx_c
                                    .send(LoadedCoverEvent {
                                        game_id: game.id,
                                        media_type: "cover".to_string(),
                                        protocol,
                                    })
                                    .await;

                                let finished_so_far =
                                    counter_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                                let item_title = format!(
                                    "SteamGridDB Media ({}/{}) - {}",
                                    finished_so_far, total, game.title
                                );

                                let _ = progress_tx_c
                                    .send(DownloadEvent {
                                        downloaded: finished_so_far as u64,
                                        total: total as u64,
                                        percentage: ((finished_so_far as f64 / total as f64)
                                            * 100.0),
                                        finished: false,
                                        error: None,
                                        task_name: Some(item_title),
                                    })
                                    .await;
                            }));
                        }

                        for t in tasks {
                            let _ = t.await;
                        }

                        let _ = progress_tx
                            .send(DownloadEvent {
                                downloaded: total_games as u64,
                                total: total_games as u64,
                                percentage: 100.0,
                                finished: true,
                                error: None,
                                task_name: Some(format!(
                                    "SteamGridDB Media ({}/{} Completed)",
                                    total_games, total_games
                                )),
                            })
                            .await;
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
                let len = current_key.len();
                self.modal_state = ModalState::AppSettings {
                    api_key_input: current_key,
                    selected_field: 0,
                    is_editing_api_key: false,
                    cursor_pos: len,
                };
                self.status_msg =
                    "Settings menu opened. Edit API Key and press Enter to save.".to_string();
            }
            Action::OpenAboutModal => {
                self.modal_state = ModalState::About;
                self.status_msg =
                    format!("[About] TUI Game Station v{}", env!("CARGO_PKG_VERSION"));
            }
            Action::CheckForUpdates { silent } => {
                self.is_manual_update_check = !silent;
                if !silent {
                    self.status_msg = "[Updater] Checking GitHub...".to_string();
                }
                let (tx, rx) = mpsc::channel(1);
                self.update_rx = Some(rx);
                let current_ver = env!("CARGO_PKG_VERSION").to_string();

                tokio::spawn(async move {
                    let result = crate::updater::check_for_updates(&current_ver)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(result).await;
                });
            }
            Action::StartAppUpdate {
                download_url,
                new_version,
            } => {
                self.modal_state = ModalState::None;
                let (tx, rx) = mpsc::channel(50);
                self.download_rx = Some(rx);
                let task_name = format!("Updating to v{}", new_version);
                self.download_progress = Some(DownloadProgressState {
                    runner_id: 0,
                    runner_name: task_name.clone(),
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    percentage: 0.0,
                    is_finished: false,
                    error_msg: None,
                });
                self.status_msg = format!("[Updater] Downloading v{}...", new_version);

                tokio::spawn(async move {
                    if let Err(e) = crate::updater::download_and_apply_update(
                        &download_url,
                        &new_version,
                        tx.clone(),
                    )
                    .await
                    {
                        let _ = tx
                            .send(DownloadEvent {
                                downloaded: 0,
                                total: 0,
                                percentage: 0.0,
                                finished: true,
                                error: Some(e.to_string()),
                                task_name: Some(task_name),
                            })
                            .await;
                    }
                });
            }
            Action::SaveAppSettings => {
                if let ModalState::AppSettings {
                    ref api_key_input, ..
                } = self.modal_state.clone()
                {
                    let trimmed = api_key_input.trim();
                    if self.db.set_setting("steamgriddb_api_key", trimmed).is_ok() {
                        self.status_msg = "[OK] Settings updated successfully!".to_string();
                        self.modal_state = ModalState::None;
                    }
                }
            }
            Action::OpenVisualMediaModal => {
                if self.modal_state == ModalState::None
                    && !self.games.is_empty()
                    && self.selected_game_idx < self.games.len()
                {
                    let game = &self.games[self.selected_game_idx];
                    let game_id = game.id;
                    let title = game.title.clone();
                    let cleaned = scraper::title_cleaner::TitleCleaner::clean_title(&title);
                    let query = if cleaned.is_empty() {
                        title.clone()
                    } else {
                        cleaned
                    };

                    self.modal_state = ModalState::VisualMediaSelector {
                        game_id,
                        game_title: title,
                        search_query: query.clone(),
                        active_tab: 0,
                        focused_section: 1,
                        cursor_pos: query.len(),
                        is_searching: true,
                        candidates: Vec::new(),
                        selected_candidate_idx: 0,
                        selected_candidate_id: None,
                        selected_candidate_name: None,
                        covers: Vec::new(),
                        selected_cover_idx: 0,
                        chosen_cover_idx: None,
                        banners: Vec::new(),
                        selected_banner_idx: 0,
                        chosen_banner_idx: None,
                        icons: Vec::new(),
                        selected_icon_idx: 0,
                        chosen_icon_idx: None,
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
                            self.status_msg = format!(
                                "[OK] Found {} candidate(s) on SteamGridDB.",
                                candidates.len()
                            );
                        }
                    }
                }
            }
            Action::SearchVisualMedia => {
                if let ModalState::VisualMediaSelector {
                    ref search_query, ..
                } = self.modal_state.clone()
                {
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
                            self.status_msg = format!(
                                "[OK] Found {} candidate(s) on SteamGridDB.",
                                candidates.len()
                            );
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

                        let new_covers = client
                            .get_images(sgdb_id, "grids")
                            .await
                            .unwrap_or_default();
                        let new_banners = client
                            .get_images(sgdb_id, "heroes")
                            .await
                            .unwrap_or_default();
                        let new_icons = client
                            .get_images(sgdb_id, "icons")
                            .await
                            .unwrap_or_default();

                        let has_covers = !new_covers.is_empty();
                        let has_banners = !new_banners.is_empty();
                        let has_icons = !new_icons.is_empty();

                        if let ModalState::VisualMediaSelector {
                            ref mut selected_candidate_id,
                            ref mut selected_candidate_name,
                            ref mut covers,
                            ref mut chosen_cover_idx,
                            ref mut banners,
                            ref mut chosen_banner_idx,
                            ref mut icons,
                            ref mut chosen_icon_idx,
                            ref mut active_tab,
                            ..
                        } = self.modal_state
                        {
                            *selected_candidate_id = Some(sgdb_id);
                            *selected_candidate_name = Some(cand_name.clone());
                            *covers = new_covers;
                            *chosen_cover_idx = if has_covers { Some(0) } else { None };
                            *banners = new_banners;
                            *chosen_banner_idx = if has_banners { Some(0) } else { None };
                            *icons = new_icons;
                            *chosen_icon_idx = if has_icons { Some(0) } else { None };
                            *active_tab = 1; // Switch to Covers tab
                            self.status_msg = format!("[OK] Selected candidate '{}'. Cover, Banner & Icon pre-selected. Press [Enter] to apply all.", cand_name);
                        }
                        self.update_visual_media_preview();
                    }
                }
            }
            Action::SwitchVisualMediaTab => {
                if let ModalState::VisualMediaSelector {
                    ref mut active_tab, ..
                } = self.modal_state
                {
                    *active_tab = (*active_tab + 1) % 4;
                }
                self.update_visual_media_preview();
            }
            Action::SwitchVisualMediaTabPrev => {
                if let ModalState::VisualMediaSelector {
                    ref mut active_tab, ..
                } = self.modal_state
                {
                    *active_tab = (*active_tab + 3) % 4;
                }
                self.update_visual_media_preview();
            }
            Action::VisualMediaNavUp => {
                if let ModalState::VisualMediaSelector {
                    ref mut focused_section,
                    active_tab,
                    ref mut selected_candidate_idx,
                    ref mut selected_cover_idx,
                    ref mut selected_banner_idx,
                    ref mut selected_icon_idx,
                    ..
                } = self.modal_state
                {
                    match *focused_section {
                        1 => *focused_section = 0,
                        2 => {
                            let curr_idx = match active_tab {
                                0 => *selected_candidate_idx,
                                1 => *selected_cover_idx,
                                2 => *selected_banner_idx,
                                _ => *selected_icon_idx,
                            };
                            if curr_idx == 0 {
                                *focused_section = if active_tab == 0 { 1 } else { 0 };
                            } else {
                                match active_tab {
                                    0 => {
                                        *selected_candidate_idx =
                                            selected_candidate_idx.saturating_sub(1)
                                    }
                                    1 => *selected_cover_idx = selected_cover_idx.saturating_sub(1),
                                    2 => {
                                        *selected_banner_idx = selected_banner_idx.saturating_sub(1)
                                    }
                                    _ => *selected_icon_idx = selected_icon_idx.saturating_sub(1),
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.update_visual_media_preview();
            }
            Action::VisualMediaNavDown => {
                if let ModalState::VisualMediaSelector {
                    ref mut focused_section,
                    active_tab,
                    ref mut selected_candidate_idx,
                    ref candidates,
                    ref mut selected_cover_idx,
                    ref covers,
                    ref mut selected_banner_idx,
                    ref banners,
                    ref mut selected_icon_idx,
                    ref icons,
                    ..
                } = self.modal_state
                {
                    match *focused_section {
                        0 => *focused_section = if active_tab == 0 { 1 } else { 2 },
                        1 => *focused_section = 2,
                        2 => match active_tab {
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
                                    *selected_banner_idx =
                                        (*selected_banner_idx + 1) % banners.len();
                                }
                            }
                            _ => {
                                if !icons.is_empty() {
                                    *selected_icon_idx = (*selected_icon_idx + 1) % icons.len();
                                }
                            }
                        },
                        _ => {}
                    }
                }
                self.update_visual_media_preview();
            }
            Action::VisualMediaNavLeft => {
                if let ModalState::VisualMediaSelector {
                    focused_section,
                    ref mut active_tab,
                    ref mut cursor_pos,
                    ref mut selected_candidate_idx,
                    ref mut selected_cover_idx,
                    ref mut selected_banner_idx,
                    ref mut selected_icon_idx,
                    ..
                } = self.modal_state
                {
                    match focused_section {
                        0 => *active_tab = (*active_tab + 3) % 4,
                        1 => *cursor_pos = cursor_pos.saturating_sub(1),
                        2 => match *active_tab {
                            0 => *selected_candidate_idx = selected_candidate_idx.saturating_sub(1),
                            1 => *selected_cover_idx = selected_cover_idx.saturating_sub(1),
                            2 => *selected_banner_idx = selected_banner_idx.saturating_sub(1),
                            _ => *selected_icon_idx = selected_icon_idx.saturating_sub(1),
                        },
                        _ => {}
                    }
                }
                self.update_visual_media_preview();
            }
            Action::VisualMediaNavRight => {
                if let ModalState::VisualMediaSelector {
                    focused_section,
                    ref mut active_tab,
                    ref search_query,
                    ref mut cursor_pos,
                    ref mut selected_candidate_idx,
                    ref candidates,
                    ref mut selected_cover_idx,
                    ref covers,
                    ref mut selected_banner_idx,
                    ref banners,
                    ref mut selected_icon_idx,
                    ref icons,
                    ..
                } = self.modal_state
                {
                    match focused_section {
                        0 => *active_tab = (*active_tab + 1) % 4,
                        1 => *cursor_pos = (*cursor_pos + 1).min(search_query.len()),
                        2 => match *active_tab {
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
                                    *selected_banner_idx =
                                        (*selected_banner_idx + 1) % banners.len();
                                }
                            }
                            _ => {
                                if !icons.is_empty() {
                                    *selected_icon_idx = (*selected_icon_idx + 1) % icons.len();
                                }
                            }
                        },
                        _ => {}
                    }
                }
                self.update_visual_media_preview();
            }
            Action::SetVisualMediaTab(tab_idx) => {
                if let ModalState::VisualMediaSelector {
                    ref mut active_tab, ..
                } = self.modal_state
                {
                    *active_tab = tab_idx % 4;
                }
                self.update_visual_media_preview();
            }
            Action::ApplyVisualMediaSelection => {
                if let ModalState::VisualMediaSelector {
                    game_id,
                    ref selected_candidate_name,
                    ref covers,
                    chosen_cover_idx,
                    ref banners,
                    chosen_banner_idx,
                    ref icons,
                    chosen_icon_idx,
                    ..
                } = self.modal_state.clone()
                {
                    let api_key = self.db.get_setting("steamgriddb_api_key").ok().flatten();
                    let media_dir = scraper::steamgriddb::SteamGridDBClient::get_media_dir();

                    let mut updated_media = Vec::new();

                    // 1. Cover
                    if let Some(c_idx) = chosen_cover_idx {
                        if let Some(c) = covers.get(c_idx) {
                            let dest = media_dir.join("covers").join(format!("{}.jpg", game_id));
                            let tx = self.cover_tx.clone();
                            let manager = self.cover_manager.clone();
                            let url = c.url.clone();
                            let key = api_key.clone();
                            tokio::spawn(async move {
                                let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                                if client.download_file_to_path(&url, &dest).await.is_ok() {
                                    let db_path = dirs::data_dir()
                                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                        .join("tui_game_station")
                                        .join("game_station.db");
                                    if let Ok(db) = Database::open(&db_path) {
                                        let _ = db.record_media_status(
                                            game_id,
                                            "cover",
                                            "downloaded",
                                            Some(&dest.to_string_lossy()),
                                            Some(&url),
                                        );
                                    }
                                    if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                        let _ = tx
                                            .send(LoadedCoverEvent {
                                                game_id,
                                                media_type: "cover".to_string(),
                                                protocol: Some(protocol),
                                            })
                                            .await;
                                    }
                                }
                            });
                            self.media_protocols.remove(&(game_id, "cover".to_string()));
                            self.pending_cover_requests.remove(&game_id);
                            updated_media.push("Cover");
                        }
                    }

                    // 2. Banner
                    if let Some(b_idx) = chosen_banner_idx {
                        if let Some(b) = banners.get(b_idx) {
                            let dest = media_dir.join("banners").join(format!("{}.jpg", game_id));
                            let tx = self.cover_tx.clone();
                            let manager = self.cover_manager.clone();
                            let url = b.url.clone();
                            let key = api_key.clone();
                            tokio::spawn(async move {
                                let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                                if client.download_file_to_path(&url, &dest).await.is_ok() {
                                    let db_path = dirs::data_dir()
                                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                        .join("tui_game_station")
                                        .join("game_station.db");
                                    if let Ok(db) = Database::open(&db_path) {
                                        let _ = db.record_media_status(
                                            game_id,
                                            "banner",
                                            "downloaded",
                                            Some(&dest.to_string_lossy()),
                                            Some(&url),
                                        );
                                    }
                                    if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                        let _ = tx
                                            .send(LoadedCoverEvent {
                                                game_id,
                                                media_type: "banner".to_string(),
                                                protocol: Some(protocol),
                                            })
                                            .await;
                                    }
                                }
                            });
                            self.media_protocols
                                .remove(&(game_id, "banner".to_string()));
                            self.pending_cover_requests.remove(&game_id);
                            updated_media.push("Banner");
                        }
                    }

                    // 3. Icon
                    if let Some(i_idx) = chosen_icon_idx {
                        if let Some(i) = icons.get(i_idx) {
                            let dest = media_dir.join("icons").join(format!("{}.png", game_id));
                            let tx = self.cover_tx.clone();
                            let manager = self.cover_manager.clone();
                            let url = i.url.clone();
                            let key = api_key.clone();
                            tokio::spawn(async move {
                                let client = scraper::steamgriddb::SteamGridDBClient::new(key);
                                if client.download_file_to_path(&url, &dest).await.is_ok() {
                                    let db_path = dirs::data_dir()
                                        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                                        .join("tui_game_station")
                                        .join("game_station.db");
                                    if let Ok(db) = Database::open(&db_path) {
                                        let _ = db.record_media_status(
                                            game_id,
                                            "icon",
                                            "downloaded",
                                            Some(&dest.to_string_lossy()),
                                            Some(&url),
                                        );
                                    }
                                    if let Some(protocol) = manager.load_protocol_from_file(&dest) {
                                        let _ = tx
                                            .send(LoadedCoverEvent {
                                                game_id,
                                                media_type: "icon".to_string(),
                                                protocol: Some(protocol),
                                            })
                                            .await;
                                    }
                                }
                            });
                            self.media_protocols.remove(&(game_id, "icon".to_string()));
                            self.media_protocols
                                .remove(&(game_id, "icon_hb".to_string()));
                            self.pending_cover_requests.remove(&game_id);
                            updated_media.push("Icon");
                        }
                    }

                    // 4. Update Game Title
                    let mut title_msg = String::new();
                    if let Some(ref new_title) = selected_candidate_name {
                        if let Some(pos) = self.games.iter().position(|g| g.id == game_id) {
                            let mut game = self.games[pos].clone();
                            game.title = new_title.clone();
                            if self.db.update_game(&game).is_ok() {
                                title_msg = format!(" & updated title to '{}'", new_title);
                                let sel = self.selected_game_idx;
                                self.load_platforms();
                                if sel < self.games.len() {
                                    self.selected_game_idx = sel;
                                }
                            }
                        }
                    }

                    self.modal_state = ModalState::None;
                    if updated_media.is_empty() {
                        self.status_msg = format!(
                            "[OK] Updated title to '{}'!",
                            selected_candidate_name.clone().unwrap_or_default()
                        );
                    } else {
                        self.status_msg = format!(
                            "[OK] Applied media ({}){}!",
                            updated_media.join(", "),
                            title_msg
                        );
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
                    gamemode,
                    mangohud,
                    gamescope,
                    esync,
                    fsync,
                    dxvk,
                    vkd3d,
                    ref parent_modal,
                    ..
                } = self.modal_state.clone()
                {
                    if title.trim().is_empty() {
                        self.status_msg = "Game title cannot be empty.".to_string();
                        return;
                    }

                    let all_db_platforms = self.db.get_platforms().unwrap_or_default();
                    let platform_id = match game_type {
                        PlatformType::Emulator => {
                            if platform_idx < self.platforms.len() {
                                self.platforms[platform_idx].id
                            } else if !self.platforms.is_empty() {
                                self.platforms[0].id
                            } else {
                                1
                            }
                        }
                        PlatformType::Native => all_db_platforms
                            .iter()
                            .find(|p| p.slug == "linux")
                            .map(|p| p.id)
                            .unwrap_or(1),
                        PlatformType::Wine => all_db_platforms
                            .iter()
                            .find(|p| p.slug == "windows")
                            .map(|p| p.id)
                            .unwrap_or(1),
                        PlatformType::Steam => all_db_platforms
                            .iter()
                            .find(|p| p.slug == "steam")
                            .map(|p| p.id)
                            .unwrap_or(1),
                    };

                    let steam_id = steam_appid.parse::<i64>().ok();

                    let mut env_flags = Vec::new();
                    if gamemode {
                        env_flags.push("GAMEMODE=1");
                    }
                    if mangohud {
                        env_flags.push("MANGOHUD=1");
                    }
                    if gamescope {
                        env_flags.push("GAMESCOPE=1");
                    }
                    if esync {
                        env_flags.push("WINEESYNC=1");
                    }
                    if fsync {
                        env_flags.push("WINEFSYNC=1");
                    }
                    if dxvk {
                        env_flags.push("DXVK_ASYNC=1");
                    }
                    if vkd3d {
                        env_flags.push("VKD3D_CONFIG=enable_async");
                    }

                    let game = Game {
                        id: 0,
                        platform_id,
                        folder_id: None,
                        emulator_override: None,
                        core_override: None,
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
                        env_vars: if env_flags.is_empty() {
                            None
                        } else {
                            Some(env_flags.join(" "))
                        },
                        wine_prefix: if wine_prefix.trim().is_empty() {
                            if !working_dir.trim().is_empty() {
                                let p = std::path::PathBuf::from(working_dir.trim()).join("prefix");
                                let _ = std::fs::create_dir_all(&p);
                                Some(p.to_string_lossy().to_string())
                            } else {
                                None
                            }
                        } else {
                            let _ = std::fs::create_dir_all(wine_prefix.trim());
                            Some(wine_prefix.trim().to_string())
                        },
                        wine_runner_id: None,
                        steam_appid: steam_id,
                        file_name: file_path.split('/').next_back().map(|s| s.to_string()),
                        file_extension: file_path.split('.').next_back().map(|s| format!(".{}", s)),
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

                    match self.db.insert_game(&game) {
                        Ok(_) => {
                            self.status_msg = format!("Game '{}' saved successfully.", title);
                            if let Some(ModalState::WindowsGamesManager { .. }) =
                                parent_modal.as_deref()
                            {
                                let games = self
                                    .db
                                    .get_games_for_platform(platform_id)
                                    .unwrap_or_default();
                                self.games = games.clone();
                                self.modal_state = ModalState::WindowsGamesManager {
                                    games,
                                    selected_idx: 0,
                                };
                            } else {
                                self.modal_state = ModalState::None;
                                self.load_platforms();
                                if let Some(pos) = self
                                    .platforms
                                    .iter()
                                    .position(|p| p.id == platform_id)
                                {
                                    self.selected_platform_idx = pos;
                                    self.load_games_for_selected_platform();
                                }
                            }
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

    pub fn trigger_async_dat_download_by_runner(&mut self, runner_name: &str) {
        if let Ok(Some(slug)) = self.db.get_platform_slug_by_runner_name(runner_name) {
            self.trigger_async_dat_download(&slug);
        }
    }

    pub fn trigger_async_dat_download(&mut self, platform_slug: &str) {
        if game_core::dat_downloader::DatDownloader::get_dat_relative_path(platform_slug).is_none()
        {
            return;
        }

        let slug = platform_slug.to_string();
        if !game_core::dat_downloader::DatDownloader::is_dat_cached(&slug) {
            self.status_msg = format!(
                "[DAT] Downloading database for {} in background...",
                slug.to_uppercase()
            );
            tokio::spawn(async move {
                let _ =
                    game_core::dat_downloader::DatDownloader::ensure_dat_downloaded(&slug).await;
            });
        }
    }
}

pub fn get_clipboard_text() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste").arg("-n").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    if let Ok(output) = std::process::Command::new("xsel")
        .args(["-b", "-o"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both end-to-end tests below rewrite the process-global `XDG_DATA_HOME`
    /// / `XDG_CACHE_HOME` to isolate their DB. Tests run in parallel threads,
    /// so they must be serialized or they open each other's database and trip
    /// SQLite's locking (and leak temp dirs).
    static XDG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn destructive_actions_are_rejected_during_input_safe_mode() {
        assert!(is_destructive_action(&Action::Quit));
        assert!(is_destructive_action(&Action::LaunchGame));
        assert!(is_destructive_action(&Action::OpenRunnerStandalone));
        assert!(is_destructive_action(&Action::DeleteSelectedGames));
        assert!(is_destructive_action(&Action::ConfirmDeleteFolderExecution));
        assert!(is_destructive_action(&Action::RescanFolder));
        assert!(is_destructive_action(&Action::StartRunnerDownload));
        assert!(is_destructive_action(&Action::StartAppUpdate {
            download_url: String::new(),
            new_version: String::new(),
        }));
        assert!(!is_destructive_action(&Action::NextGame));
        assert!(!is_destructive_action(&Action::PrevPlatform));
        assert!(!is_destructive_action(&Action::ToggleViewMode));
        assert!(!is_destructive_action(&Action::CloseModal));
    }

    /// Guardián central: mientras un juego está corriendo, NINGUNA acción del
    /// launcher es válida salvo `ForceCloseGame`. Cubre explícitamente el caso
    /// del bug: un gamepad conectado en caliente durante la partida — cada
    /// botón del mando se traduce en una de estas acciones y todas convergen
    /// en el mismo despachador.
    #[test]
    fn game_running_rejects_all_actions_except_force_close() {
        assert!(is_action_allowed_while_game_running(
            &Action::ForceCloseGame
        ));

        let rejected = [
            // Destructivas / lanzamiento / salida.
            Action::Quit,
            Action::LaunchGame,
            Action::OpenRunnerStandalone,
            Action::StartRunnerDownload,
            Action::StartAppUpdate {
                download_url: String::new(),
                new_version: String::new(),
            },
            Action::DeleteSelectedGames,
            Action::OpenConfirmDeleteModal,
            Action::ConfirmDeleteGameExecution,
            Action::ConfirmDeleteRunnerExecution,
            Action::ScanCurrentFolder,
            Action::StartFolderScan,
            Action::KillWineProcesses,
            // Navegación normal (debe ignorarse también).
            Action::NextGame,
            Action::PrevGame,
            Action::NextPlatform,
            Action::PrevPlatform,
            Action::TogglePane,
            Action::ToggleViewMode,
            Action::ToggleSelectGame,
            Action::ToggleBigPictureMode,
            Action::CloseGameDetail,
            Action::DetailNextAction,
            Action::DetailPrevAction,
            Action::OpenSettingsModal,
            Action::OpenCheatsheetModal,
            Action::OpenManageRunnersModal,
            Action::OpenWineToolsMenu,
            Action::FetchGameMedia,
            Action::QuickRescanPlatform,
            Action::ScanSteamGames,
            Action::ToggleShowAllPlatforms,
            Action::ModalSelectNext,
            Action::ModalSelectPrev,
            Action::SwitchVisualMediaTab,
            Action::SwitchVisualMediaTabPrev,
            Action::ToggleConfirmDeleteRunnerOption,
            Action::ToggleConfirmDeleteOption,
        ];
        for action in &rejected {
            assert!(
                !is_action_allowed_while_game_running(action),
                "la acción {action:?} debería estar bloqueada mientras un juego corre"
            );
        }
    }

    /// End-to-end del guardián en el despachador: con un juego corriendo, una
    /// acción de navegación (ToggleViewMode) se ignora; `ForceCloseGame` sí se
    /// despacha. Sin juego corriendo, la navegación vuelve a funcionar. El
    /// entorno (DB, caché) se aísla en un directorio temporal vía XDG.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn dispatcher_blocks_all_input_while_a_game_runs_except_force_close() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_test_data_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let mut app = App::new().expect("App::new should work with isolated dirs");
        let view_before = app.view_mode;

        // Simula el juego en curso (mando conectado en caliente, partida viva).
        app.running_game = Some(RunningGame {
            title: "Hot-plug test".to_string(),
            runner_name: None,
            started_at: std::time::Instant::now(),
        });

        app.update(Action::ToggleViewMode).await;
        assert_eq!(
            app.view_mode, view_before,
            "navegación no debe ejecutarse mientras un juego corre"
        );

        // ForceCloseGame sí llega al runner (sin proceso real devuelve un
        // error de estado; lo importante es que pasó el guardián).
        app.update(Action::ForceCloseGame).await;
        assert!(
            app.status_msg.starts_with("[Error]") || app.status_msg.starts_with("[OK]"),
            "ForceCloseGame debe despacharse: {}",
            app.status_msg
        );

        // Sin juego corriendo, la navegación vuelve a ejecutarse.
        app.running_game = None;
        app.update(Action::ToggleViewMode).await;
        assert_ne!(
            app.view_mode, view_before,
            "sin juego corriendo la navegación debe ejecutarse"
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// La etiqueta del campo "Emulador" de los formularios de escaneo solo
    /// ofrece "Default" cuando la plataforma ya tiene una carpeta que heredar.
    /// Para la primera carpeta muestra el emulador efectivo de la plataforma.
    #[test]
    fn add_scan_emulator_label_omits_default_for_first_folder() {
        let mut path = std::env::temp_dir();
        path.push(format!("tui_scan_emu_label_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::open(&path).unwrap();
        let switch = db.get_platform_by_slug("switch").unwrap().unwrap();

        let runners = db.get_runners_for_platform(switch.id).unwrap();
        assert!(db.get_scan_folders_for_platform(switch.id).unwrap().is_empty());

        // Primera carpeta: nada que heredar → el emulador efectivo, no "Default".
        let label = add_scan_emulator_label(&db, &switch, None, &runners);
        assert_eq!(label, "Ryujinx");

        // Con una carpeta ya registrada, "Default" (heredar) es válido.
        db.save_scan_folder(switch.id, "/fake/switch", true).unwrap();
        let label = add_scan_emulator_label(&db, &switch, None, &runners);
        assert_eq!(label, "Default");

        // Emulador explícito → su nombre.
        let ryujinx = runners.iter().find(|r| r.name == "Ryujinx").unwrap();
        let label = add_scan_emulator_label(&db, &switch, Some(ryujinx.id), &runners);
        assert_eq!(label, "Ryujinx");

        drop(db);
        let _ = std::fs::remove_file(path);
    }

    /// El selector "Emulador" del formulario de escaneo no debe permitir caer
    /// en "Default" (sin override) mientras no exista ninguna carpeta para la
    /// plataforma: en la primera carpeta los ◀ ▶ rodean entre emuladores
    /// reales. Una vez registrada una carpeta, "Default" vuelve a ser opción.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn add_scan_emulator_selector_skips_default_when_no_folders_exist() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_scan_cycle_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let switch = app
            .platforms
            .iter()
            .find(|p| p.slug == "switch")
            .expect("switch platform")
            .clone();

        // Configura Ryujinx y Citron: el selector tiene dos emuladores reales.
        let runners = app.db.get_runners_for_platform(switch.id).unwrap();
        let ryujinx = runners.iter().find(|r| r.name == "Ryujinx").unwrap().id;
        let citron = runners.iter().find(|r| r.name == "Citron").unwrap().id;
        app.db.update_runner_config(ryujinx, "/fake/ryujinx", true).unwrap();
        app.db.update_runner_config(citron, "/fake/citron", true).unwrap();

        app.modal_state = ModalState::AddFolderScanForm {
            platform: switch.clone(),
            folder_path: "/fake/switch".to_string(),
            extensions_input: ".nsp".to_string(),
            recursive: true,
            use_dat_auto_id: false,
            add_emulator_id: None,
            add_core: None,
            selected_field: 0,
            parent_modal: None,
        };

        // Sin carpetas: los ◀ ▶ rodean sin pasar por "Default" (None).
        app.cycle_add_folder_emulator(false);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, Some(ryujinx), "forward picks the first emulator");
        app.cycle_add_folder_emulator(false);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, Some(citron), "forward reaches the last emulator");
        app.cycle_add_folder_emulator(false);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, Some(ryujinx), "forward wraps, never lands on None");
        app.cycle_add_folder_emulator(true);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, Some(citron), "backward wraps, never lands on None");

        // Con una carpeta registrada, "Default" (None) vuelve a ser alcanzable.
        app.db.save_scan_folder(switch.id, "/fake/switch", true).unwrap();
        app.cycle_add_folder_emulator(true);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, Some(ryujinx), "backward to first emulator");
        app.cycle_add_folder_emulator(true);
        let (_, emu, _) = app.add_scan_form_selection().unwrap();
        assert_eq!(emu, None, "backward reaches Default once a folder exists");

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// End-to-end del selector "Emulador Activo (◀ ▶)" y de la fila anidada
    /// "Núcleo": inyecta el emulador ficticio core-based `testcore` junto a
    /// Ryujinx en Switch y comprueba que ◀ ▶ alterna entre emuladores, que con
    /// el emulador activo core-based los ◀ ▶ giran el núcleo, y que el
    /// auto-switch recupera un emulador configurado cuando el ejecutable del
    /// activo desaparece.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn active_emulator_selector_cycles_emulators_and_cores() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_selector_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let fake_dir = tmp.join("fake");
        std::fs::create_dir_all(&fake_dir).expect("mkdir");
        let ryujinx_exe = fake_dir.join("ryujinx");
        let testcore_exe = fake_dir.join("testcore");
        std::fs::write(&ryujinx_exe, "#!/bin/sh\n").expect("write ryujinx exe");
        std::fs::write(&testcore_exe, "#!/bin/sh\n").expect("write testcore exe");
        let ryujinx_exe_str = ryujinx_exe.to_string_lossy().into_owned();
        let testcore_exe_str = testcore_exe.to_string_lossy().into_owned();

        {
            let db = Database::open_default().expect("open DB");
            db.set_setting("first_run_completed", "true").expect("no wizard");
            let switch = db
                .get_platform_by_slug("switch")
                .expect("query")
                .expect("switch platform");
            let testcore_id = db
                .insert_runner(switch.id, "testcore", "appimage")
                .expect("insert testcore");
            let runners = db.get_runners_for_platform(switch.id).expect("runners");
            let ryujinx = runners
                .iter()
                .find(|r| r.name == "Ryujinx")
                .expect("Ryujinx seeded");
            db.update_runner_config(ryujinx.id, &ryujinx_exe_str, true)
                .expect("configure ryujinx");
            db.update_runner_config(testcore_id, &testcore_exe_str, true)
                .expect("configure testcore");
        }

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let switch_idx = app
            .platforms
            .iter()
            .position(|p| p.slug == "switch")
            .expect("switch among platforms");
        app.selected_platform_idx = switch_idx;

        // Activo inicial: Ryujinx (primera runner configurada, no core-based).
        let (name, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(name, "Ryujinx");
        assert!(core.is_none());

        // ◀ ▶ alterna entre los emuladores configurados.
        app.update(Action::CycleActiveEmulatorNext).await;
        let (name, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(name, "testcore");
        assert_eq!(core.as_deref(), Some("mGBA"));

        // Con testcore activo (core-based) los ◀ ▶ giran el núcleo:
        // default mgba --prev--> genesis_plus --next--> mgba (wrap) --next--> snes9x.
        app.update(Action::CycleActiveEmulatorPrev).await;
        let (_, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(core.as_deref(), Some("Genesis Plus GX"));
        app.update(Action::CycleActiveEmulatorNext).await;
        let (_, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(core.as_deref(), Some("mGBA"));
        app.update(Action::CycleActiveEmulatorNext).await;
        let (_, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(core.as_deref(), Some("Snes9x"));

        // Auto-switch: si el ejecutable del emulador activo desaparece, el
        // siguiente load_platforms mueve el flag al primer configurado vivo.
        std::fs::remove_file(&testcore_exe).expect("remove testcore exe");
        app.load_platforms();
        let (name, _) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(name, "Ryujinx");

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Regresión del flujo "Scan ROMs Folder": el selector "Emulador Activo
    /// (◀ ▶)" debe ser visible y funcional DENTRO del formulario de escaneo
    /// (modal `ScanFolderForm`), no solo en la navegación normal. Cambiarlo
    /// ahí persiste y se refleja al volver a la navegación normal.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn scan_folder_form_can_change_active_emulator() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_scanform_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let fake_dir = tmp.join("fake");
        std::fs::create_dir_all(&fake_dir).expect("mkdir");
        let ryujinx_exe = fake_dir.join("ryujinx");
        let testcore_exe = fake_dir.join("testcore");
        std::fs::write(&ryujinx_exe, "#!/bin/sh\n").expect("write ryujinx exe");
        std::fs::write(&testcore_exe, "#!/bin/sh\n").expect("write testcore exe");
        let ryujinx_exe_str = ryujinx_exe.to_string_lossy().into_owned();
        let testcore_exe_str = testcore_exe.to_string_lossy().into_owned();

        {
            let db = Database::open_default().expect("open DB");
            db.set_setting("first_run_completed", "true").expect("no wizard");
            let switch = db
                .get_platform_by_slug("switch")
                .expect("query")
                .expect("switch platform");
            let testcore_id = db
                .insert_runner(switch.id, "testcore", "appimage")
                .expect("insert testcore");
            let runners = db.get_runners_for_platform(switch.id).expect("runners");
            let ryujinx = runners
                .iter()
                .find(|r| r.name == "Ryujinx")
                .expect("Ryujinx seeded");
            db.update_runner_config(ryujinx.id, &ryujinx_exe_str, true)
                .expect("configure ryujinx");
            db.update_runner_config(testcore_id, &testcore_exe_str, true)
                .expect("configure testcore");
        }

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let switch = app
            .platforms
            .iter()
            .find(|p| p.slug == "switch")
            .expect("switch among platforms")
            .clone();

        // Abre el formulario de Scan ROMs Folder para Switch con el campo 0
        // ("Emulador Activo") enfocado, como lo hace el flujo real.
        app.modal_state = ModalState::ScanFolderForm {
            platform: switch.clone(),
            folders: Vec::new(),
            selected_index: 0,
        };

        // Activo inicial: Ryujinx.
        let (name, core) = app
            .active_emulator_selector_info_for(switch.id)
            .expect("selector info");
        assert_eq!(name, "Ryujinx");
        assert!(core.is_none());

        // ▶ desde el formulario cambia el emulador activo (esto fallaba antes:
        // el guardián de modal_state bloqueaba el ciclo dentro del modal).
        app.cycle_active_selector_for(&switch, false);
        let (name, core) = app
            .active_emulator_selector_info_for(switch.id)
            .expect("selector info");
        assert_eq!(name, "testcore");
        assert_eq!(core.as_deref(), Some("mGBA"));

        // Con testcore activo (core-based), ◀ ▶ giran el núcleo desde el modal.
        app.cycle_active_selector_for(&switch, true);
        let (_, core) = app
            .active_emulator_selector_info_for(switch.id)
            .expect("selector info");
        assert_eq!(core.as_deref(), Some("Genesis Plus GX"));
        app.cycle_active_selector_for(&switch, true);
        let (_, core) = app
            .active_emulator_selector_info_for(switch.id)
            .expect("selector info");
        assert_eq!(core.as_deref(), Some("Snes9x"));

        // El cambio persiste al salir del modal y volver a la navegación normal.
        app.modal_state = ModalState::None;
        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == switch.id)
            .expect("switch index");
        let (name, core) = app.active_emulator_selector_info().expect("selector info");
        assert_eq!(name, "testcore");
        assert_eq!(core.as_deref(), Some("Snes9x"));

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Verificación de RENDER (no solo de lógica): el texto "Emulador Activo"
    /// con el emulador activo debe aparecer dibujado tanto en la navegación
    /// normal como dentro del modal "Scan ROMs Folder" de la plataforma.
    #[tokio::test]
    async fn active_emulator_selector_is_rendered_in_navigation_and_scan_folder() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_render_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let fake_dir = tmp.join("fake");
        std::fs::create_dir_all(&fake_dir).expect("mkdir");
        let ryujinx_exe = fake_dir.join("ryujinx");
        std::fs::write(&ryujinx_exe, "#!/bin/sh\n").expect("write ryujinx exe");
        let ryujinx_exe_str = ryujinx_exe.to_string_lossy().into_owned();

        {
            let db = Database::open_default().expect("open DB");
            db.set_setting("first_run_completed", "true").expect("no wizard");
            let switch = db
                .get_platform_by_slug("switch")
                .expect("query")
                .expect("switch platform");
            let runners = db.get_runners_for_platform(switch.id).expect("runners");
            let ryujinx = runners
                .iter()
                .find(|r| r.name == "Ryujinx")
                .expect("Ryujinx seeded");
            db.update_runner_config(ryujinx.id, &ryujinx_exe_str, true)
                .expect("configure ryujinx");
        }

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let switch = app
            .platforms
            .iter()
            .find(|p| p.slug == "switch")
            .expect("switch among platforms")
            .clone();
        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == switch.id)
            .expect("switch index");

        let render_text = |app: &mut App| {
            let backend = ratatui::backend::TestBackend::new(100, 40);
            let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
            terminal
                .draw(|f| crate::ui::render_ui(f, app))
                .expect("draw");
            let buf = terminal.backend().buffer().clone();
            buf.content().iter().map(|c| c.symbol()).collect::<String>()
        };

        // Navegación normal: la caja "Emulador ◀ ▶" está visible.
        let nav_text = render_text(&mut app);
        assert!(
            nav_text.contains("Emulador ◀ ▶"),
            "selector ausente en la navegación normal"
        );
        assert!(nav_text.contains("Ryujinx"), "emulador activo no se muestra");

        // Scan Folder: el selector se dibuja DENTRO del formulario de escaneo.
        app.modal_state = ModalState::ScanFolderForm {
            platform: switch.clone(),
            folders: Vec::new(),
            selected_index: 0,
        };
        let scan_text = render_text(&mut app);
        assert!(
            scan_text.contains("Emulador: "),
            "selector ausente en el flujo de Scan Folder"
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Folder-manager modal: a scan folder row re-assigns its emulator with
    /// ◀ ▶ (persisted), field navigation keeps `selected_row` in sync for the
    /// bottom [DELETE] button, and the confirm-delete flow unlinks or wipes the
    /// folder (and its games).
    #[tokio::test]
    async fn folder_manager_reassigns_emulator_navigates_and_deletes() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_folder_mgr_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let fake_dir = tmp.join("fake");
        std::fs::create_dir_all(&fake_dir).expect("mkdir");
        let fake_path = fake_dir.to_string_lossy().into_owned();
        std::fs::write(fake_dir.join("game.nsp"), "nsp").expect("write rom");

        let (ryujinx_exe, citron_exe) = {
            let db = Database::open_default().expect("open DB");
            db.set_setting("first_run_completed", "true").expect("no wizard");
            let switch = db
                .get_platform_by_slug("switch")
                .expect("query")
                .expect("switch platform");
            let runners = db.get_runners_for_platform(switch.id).expect("runners");
            let ryujinx = runners.iter().find(|r| r.name == "Ryujinx").unwrap();
            let citron = runners.iter().find(|r| r.name == "Citron").unwrap();
            let ryujinx_exe = fake_dir.join("ryujinx").to_string_lossy().into_owned();
            let citron_exe = fake_dir.join("citron").to_string_lossy().into_owned();
            std::fs::write(&ryujinx_exe, "#!/bin/sh\n").unwrap();
            std::fs::write(&citron_exe, "#!/bin/sh\n").unwrap();
            db.update_runner_config(ryujinx.id, &ryujinx_exe, true)
                .expect("configure ryujinx");
            db.update_runner_config(citron.id, &citron_exe, true)
                .expect("configure citron");
            db.save_scan_folder(switch.id, &fake_path, true)
                .expect("save folder");
            (ryujinx_exe, citron_exe)
        };
        let _ = (ryujinx_exe, citron_exe);

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let switch = app
            .platforms
            .iter()
            .find(|p| p.slug == "switch")
            .expect("switch among platforms")
            .clone();
        let folders = app
            .db
            .get_scan_folders_for_platform(switch.id)
            .expect("folders");
        assert_eq!(folders.len(), 1);

        let open_modal = |app: &mut App, selected_index: usize| {
            let platform = app
                .platforms
                .iter()
                .find(|p| p.id == switch.id)
                .expect("switch among platforms")
                .clone();
            let folders = app
                .db
                .get_scan_folders_for_platform(platform.id)
                .unwrap_or_default();
            app.modal_state = ModalState::ScanFolderForm {
                platform,
                folders,
                selected_index,
            };
        };

        // Start focused on item 0 (first folder row).
        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == switch.id)
            .expect("switch index");
        open_modal(&mut app, 0);

        // ▶ assigns the next configured emulator (Citron) and persists it.
        app.update(Action::CycleFolderEmulator(false)).await;
        let ModalState::ScanFolderForm { ref folders, .. } = app.modal_state else {
            panic!("modal still open");
        };
        let assigned = folders[0].assigned_emulator_id.expect("assigned");
        let persisted = app
            .db
            .get_scanned_folder(folders[0].id)
            .expect("row")
            .expect("folder exists");
        assert_eq!(persisted.assigned_emulator_id, Some(assigned));
        assert_eq!(
            app.db
                .get_runner_for_game(switch.id, Some(folders[0].id), None)
                .unwrap()
                .unwrap()
                .id,
            assigned
        );

        // ◀ cycles back to "Default" (None).
        app.update(Action::CycleFolderEmulator(true)).await;
        let ModalState::ScanFolderForm { ref folders, .. } = app.modal_state else {
            panic!("modal still open");
        };
        assert_eq!(folders[0].assigned_emulator_id, None);

        // Field navigation: items are [Folder1.Emu, AddNewFolder].
        // ModalNextField moves from item 0 to item 1 (AddNewFolder).
        app.update(Action::ModalNextField).await;
        let ModalState::ScanFolderForm {
            selected_index, ..
        } = app.modal_state
        else {
            panic!("modal still open");
        };
        assert_eq!(selected_index, 1);

        // Move to folder item (0) and trigger delete key.
        let folder_id = app
            .db
            .get_scan_folders_for_platform(switch.id)
            .unwrap()
            .pop()
            .expect("folder row")
            .id;
        open_modal(&mut app, 0);
        app.update(Action::OpenConfirmDeleteFolder).await;
        let ModalState::ConfirmDeleteFolder {
            platform_id,
            folder_ids,
            display,
            ..
        } = app.modal_state.clone()
        else {
            panic!("confirm modal open");
        };
        assert_eq!(platform_id, switch.id);
        assert_eq!(folder_ids, vec![folder_id]);
        assert_eq!(display, fake_path);
        assert!(app
            .db
            .get_scanned_folder(folder_id)
            .expect("row")
            .is_some());

        // Option 1 = YES: remove the folder and the games it scanned (ROM files
        // on disk are kept).
        app.update(Action::ToggleConfirmDeleteFolderOption).await;
        let ModalState::ConfirmDeleteFolder {
            selected_option, ..
        } = app.modal_state
        else {
            panic!("confirm modal open");
        };
        assert_eq!(selected_option, 1);
        app.update(Action::ConfirmDeleteFolderExecution).await;
        assert!(matches!(app.modal_state, ModalState::ScanFolderForm { .. }));
        assert!(app.db.get_scan_folders_for_platform(switch.id).unwrap().is_empty());

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Reproducción del bug "AÑADIR Y ESCANEAR rompe la vista": tras el
    /// escaneo la plataforma debe seguir visible y sus juegos (los previos y
    /// los nuevos) deben cargarse. Verifica el flujo completo real (escáner +
    /// refresco al recibir el evento `finished`).
    #[tokio::test]
    async fn add_folder_and_scan_keeps_platform_and_games_visible() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_addscan_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let fake_dir = tmp.join("roms");
        std::fs::create_dir_all(&fake_dir).expect("mkdir roms");
        std::fs::write(fake_dir.join("Super Mario Bros.nes"), "ROM").expect("write rom");
        let fake_path = fake_dir.to_string_lossy().into_owned();

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let nes = app
            .platforms
            .iter()
            .find(|p| p.slug == "nes")
            .expect("nes platform")
            .clone();

        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == nes.id)
            .expect("nes index");

        app.modal_state = ModalState::ScanFolderForm {
            platform: nes.clone(),
            folders: Vec::new(),
            selected_index: 0,
        };
        app.update(Action::OpenAddFolderModal).await;
        let ModalState::AddFolderScanForm {
            ref mut folder_path,
            ref mut extensions_input,
            ..
        } = app.modal_state
        else {
            panic!("Modal 2 open");
        };
        *folder_path = fake_path.clone();
        *extensions_input = ".nes".to_string();

        app.update(Action::AddFolder).await;
        let ModalState::ScanFolderForm { ref folders, .. } = app.modal_state else {
            panic!("modal still open after ADD FOLDER");
        };
        assert_eq!(folders.len(), 1, "folder registered via ADD FOLDER");

        app.update(Action::StartFolderScan).await;

        let mut waited = 0;
        while app.scan_rx.is_some() && waited < 500 {
            app.check_download_events().await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            waited += 1;
        }
        assert!(app.scan_rx.is_none(), "scan should finish, waited {waited}");

        let platforms = app
            .db
            .get_active_platforms(app.show_all_platforms)
            .expect("platforms");
        assert!(
            platforms.iter().any(|p| p.id == nes.id),
            "nes platform still visible after scan"
        );
        let games = app.db.get_games_for_platform(nes.id).expect("games");
        assert!(
            games.iter().any(|g| g.title.contains("Mario")),
            "scanned game present: {:?}",
            games.iter().map(|g| g.title.clone()).collect::<Vec<_>>()
        );

        assert!(
            app.games.iter().any(|g| g.title.contains("Mario")),
            "in-memory games refreshed"
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// TAREA 3 + Objetivos 1/2: el flujo A → "Escanear carpeta" abre el
    /// formulario simplificado (`AddFolderScanForm`) y su selector de núcleo
    /// se alimenta de los `.so` realmente presentes en disco (NDS + RetroArch
    /// Downloaded → `desmume`), no del catálogo a ciegas. El override de
    /// emulador/núcleo se persiste en la carpeta, el escaneo sigue
    /// funcionando y el folder manager ([e]) lo muestra después.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn add_scan_form_seeds_disk_core_for_retroarch_and_flows_to_scan() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_addscanflow_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        // Fake Downloaded RetroArch tree: AppImage + melondsds/melonds/desmume
        // `.so` en disco. Es el directorio que el runner resuelve
        // (`data_local_dir`).
        let managed = tmp
            .join("data")
            .join("tui_game_station")
            .join("runners")
            .join("emulators")
            .join("retroarch-data");
        let sub = managed.join("RetroArch-Linux-x86_64");
        std::fs::create_dir_all(&sub).expect("mkdir managed sub");
        let appimage = sub.join("RetroArch-Linux-x86_64.AppImage");
        std::fs::write(&appimage, "#!/bin/sh\n").expect("write AppImage");
        let cores_dir = PathBuf::from(format!("{}.home", appimage.to_string_lossy()))
            .join(".config")
            .join("retroarch")
            .join("cores");
        std::fs::create_dir_all(&cores_dir).expect("mkdir cores");
        std::fs::write(cores_dir.join("melondsds_libretro.so"), "").expect("write melondsds");
        std::fs::write(cores_dir.join("desmume_libretro.so"), "").expect("write desmume");

        let fake_dir = tmp.join("roms");
        std::fs::create_dir_all(&fake_dir).expect("mkdir roms");
        std::fs::write(fake_dir.join("New Super Mario Bros.nds"), "ROM").expect("write rom");
        let fake_path = fake_dir.to_string_lossy().into_owned();

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();

        let nds = app
            .platforms
            .iter()
            .find(|p| p.slug == "nds")
            .expect("nds platform")
            .clone();

        // Configura el runner RetroArch descargado como activo para NDS.
        let rid = app
            .db
            .insert_runner(nds.id, "RetroArch", "retroarch")
            .expect("runner");
        app.db
            .update_runner_config_with_source(rid, &appimage.to_string_lossy(), Some("Downloaded"))
            .expect("config runner");
        app.db.set_active_runner(nds.id, rid).expect("activate");
        let runners = app.db.get_runners_for_platform(nds.id).expect("runners");
        let active = runners.iter().find(|r| r.is_active).expect("active runner");
        assert_eq!(active.name, "RetroArch");
        assert_eq!(active.source.as_deref(), Some("Downloaded"));

        // El selector de núcleo filtra por `.so` reales en disco (no el catálogo).
        let disk_cores = add_scan_available_cores(&app.db, &nds, None);
        let disk_keys: Vec<&str> = disk_cores.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(disk_keys, vec!["desmume", "melondsds"]);

        // Flujo A → Scan Folder → confirmar plataforma → AddFolderScanForm.
        app.update(Action::OpenAddGameModal).await;
        app.update(Action::ModalConfirmStep1).await;
        let ModalState::ScanFolderStep1Platform { .. } = app.modal_state else {
            panic!("expected platform selection, got {:?}", app.modal_state);
        };
        let emulator_platforms = app.get_configured_emulator_platforms();
        let nds_idx = emulator_platforms
            .iter()
            .position(|p| p.id == nds.id)
            .expect("nds in configured emulator platforms");
        if let ModalState::ScanFolderStep1Platform { selected_platform_idx } = &mut app.modal_state {
            *selected_platform_idx = nds_idx;
        }
        app.update(Action::ScanModalConfirmPlatform).await;

        let ModalState::AddFolderScanForm {
            platform,
            add_emulator_id,
            add_core,
            ..
        } = &app.modal_state
        else {
            panic!("expected AddFolderScanForm, got {:?}", app.modal_state);
        };
        assert_eq!(platform.id, nds.id);
        assert_eq!(add_emulator_id, &None, "no explicit emulator override yet");
        assert_eq!(
            add_core.as_deref(),
            Some("desmume"),
            "seeded from .so on disk (catalog default)"
        );

        // Ciclo el emulador (único configurado → RetroArch), apunto la carpeta
        // de ROMs real y ejecuto Añadir y Escanear.
        app.cycle_add_folder_emulator(false);
        let ModalState::AddFolderScanForm {
            add_emulator_id,
            add_core,
            folder_path,
            extensions_input,
            ..
        } = &mut app.modal_state
        else {
            panic!("modal closed");
        };
        assert_eq!(add_emulator_id, &Some(rid));
        assert_eq!(add_core.as_deref(), Some("desmume"));
        *folder_path = fake_path.clone();
        *extensions_input = "nds".to_string();
        app.update(Action::StartAddAndScan).await;

        assert_eq!(app.modal_state, ModalState::None, "modal closed after scan start");
        let folders = app.db.get_scan_folders_for_platform(nds.id).expect("folders");
        let folder = folders.first().expect("saved folder");
        assert_eq!(folder.assigned_emulator_id, Some(rid));
        assert_eq!(folder.assigned_core.as_deref(), Some("desmume"));

        // Esperar a que termine el escaneo asíncrono real.
        let mut waited = 0;
        while app.scan_rx.is_some() && waited < 500 {
            app.check_download_events().await;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            waited += 1;
        }
        assert!(app.scan_rx.is_none(), "scan should finish, waited {waited}");

        let games = app.db.get_games_for_platform(nds.id).expect("games");
        assert!(
            games.iter().any(|g| g.title.contains("Mario")),
            "scanned game present: {:?}",
            games.iter().map(|g| g.title.clone()).collect::<Vec<_>>()
        );

        // El folder manager ([e] en una plataforma) muestra el override.
        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == nds.id)
            .expect("nds index");
        app.update(Action::OpenFolderManagerForPlatform).await;
        let ModalState::ScanFolderForm { ref folders, .. } = app.modal_state else {
            panic!("expected folder manager, got {:?}", app.modal_state);
        };
        let f = folders
            .iter()
            .find(|f| f.assigned_emulator_id == Some(rid))
            .expect("folder with override");
        assert_eq!(f.assigned_core.as_deref(), Some("desmume"));

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `[ Download Cores ]` in the folder manager must appear whenever a
    /// RetroArch runner is configured for the platform — even when RetroArch is
    /// NOT the active emulator (e.g. NDS active=melonDS). Platforms without a
    /// RetroArch runner (e.g. Switch) must not show it. The download modal must
    /// target the configured RetroArch runner in that case, not the active one.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn folder_manager_download_cores_visible_for_any_configured_retroarch() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_folder_dlc_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();

        // NDS: catalog seeds melonDS + RetroArch; melonDS is the active runner.
        let nds = app
            .platforms
            .iter()
            .find(|p| p.slug == "nds")
            .expect("nds platform")
            .clone();
        let nds_runners = app.db.get_runners_for_platform(nds.id).expect("nds runners");
        let melonds = nds_runners
            .iter()
            .find(|r| r.name == "melonDS")
            .expect("melonDS seeded");
        let retroarch = nds_runners
            .iter()
            .find(|r| r.name == "RetroArch")
            .expect("RetroArch seeded");
        assert!(melonds.is_active, "melonDS must be the active NDS runner");
        assert!(!retroarch.is_active);

        // Folder manager still offers core downloads: RetroArch is configured.
        assert!(scan_folder_platform_has_retroarch(&app.db, nds.id));
        let items = build_scan_folder_items(&app.db, nds.id, &[]);
        assert!(
            items.contains(&ScanFolderItem::DownloadCores),
            "NDS with RetroArch configured must show DownloadCores, got {items:?}"
        );

        // Rendering: the folder manager row is visible for NDS.
        app.modal_state = ModalState::ScanFolderForm {
            platform: nds.clone(),
            folders: Vec::new(),
            selected_index: items.len().saturating_sub(1),
        };
        let backend = ratatui::backend::TestBackend::new(100, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| crate::ui::render_ui(f, &mut app))
            .expect("draw");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            text.contains("Download Cores"),
            "folder manager must render Download Cores for NDS, got: {text}"
        );

        // The download modal targets the configured RetroArch runner, not the
        // active melonDS.
        app.update(Action::OpenDownloadCoreModal).await;
        let ModalState::DownloadCoreModal {
            platform,
            runner,
            cores,
            ..
        } = &app.modal_state
        else {
            panic!("expected DownloadCoreModal, got {:?}", app.modal_state);
        };
        assert_eq!(platform.id, nds.id);
        assert_eq!(runner.name, "RetroArch");
        assert!(
            cores.iter().any(|c| c.so_file == "melondsds_libretro.so"),
            "NDS cores listed: {:?}",
            cores.iter().map(|c| c.so_file.clone()).collect::<Vec<_>>()
        );

        // Switch: no RetroArch seeded -> no Download Cores anywhere.
        let switch = app
            .platforms
            .iter()
            .find(|p| p.slug == "switch")
            .expect("switch platform")
            .clone();
        assert!(!scan_folder_platform_has_retroarch(&app.db, switch.id));
        let switch_items = build_scan_folder_items(&app.db, switch.id, &[]);
        assert!(
            !switch_items.contains(&ScanFolderItem::DownloadCores),
            "Switch has no RetroArch runner, got {switch_items:?}"
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Folder manager Enter must NOT re-scan a folder whose games are already
    /// imported and must not spawn the task slider (scan_rx / progress) when
    /// there is nothing new. Enter only re-scans when the folder contains game
    /// files that are not in the DB yet (never scanned, or new files added).
    /// [R] (Action::RescanFolder) still forces a full rescan.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn folder_manager_enter_skips_already_scanned_folders_but_r_forces_rescan() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_folderscan_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        let nds = app
            .platforms
            .iter()
            .find(|p| p.slug == "nds")
            .expect("nds platform")
            .clone();

        // Real on-disk folders so RescanFolder passes its path check.
        let populated_dir = tmp.join("populated");
        let new_games_dir = tmp.join("new_games");
        std::fs::create_dir_all(&populated_dir).unwrap();
        std::fs::create_dir_all(&new_games_dir).unwrap();
        std::fs::write(populated_dir.join("rom.nds"), b"rom").unwrap();
        std::fs::write(new_games_dir.join("brand_new.nds"), b"new").unwrap();

        // Folder A: scanned before, one game already imported.
        let folder_a = app
            .db
            .save_scan_folder(nds.id, populated_dir.to_str().unwrap(), true)
            .unwrap();
        app.db
            .insert_game(&Game {
                id: 0,
                platform_id: nds.id,
                folder_id: Some(folder_a),
                emulator_override: None,
                core_override: None,
                title: "Already imported".to_string(),
                sort_title: None,
                game_type: "rom".to_string(),
                file_path: Some(populated_dir.join("rom.nds").to_string_lossy().to_string()),
                working_dir: None,
                custom_command: None,
                env_vars: None,
                wine_prefix: None,
                wine_runner_id: None,
                steam_appid: None,
                file_name: Some("rom.nds".to_string()),
                file_extension: Some(".nds".to_string()),
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
            })
            .unwrap();
        assert_eq!(app.db.get_game_count_for_folder(folder_a).unwrap(), 1);

        // Folder B: never scanned, but it already contains a new game file.
        let folder_b = app
            .db
            .save_scan_folder(nds.id, new_games_dir.to_str().unwrap(), true)
            .unwrap();
        assert_eq!(app.db.get_game_count_for_folder(folder_b).unwrap(), 0);
        assert!(
            game_core::scanner::Scanner::folder_has_new_games(
                &app.db,
                &nds,
                Path::new(new_games_dir.to_str().unwrap()),
                true,
                folder_b,
            )
            .unwrap()
        );
        assert!(
            !game_core::scanner::Scanner::folder_has_new_games(
                &app.db,
                &nds,
                Path::new(populated_dir.to_str().unwrap()),
                true,
                folder_a,
            )
            .unwrap()
        );

        let folders = app.db.get_scan_folders_for_platform(nds.id).unwrap();
        let items = build_scan_folder_items(&app.db, nds.id, &folders);
        assert_eq!(items[0], ScanFolderItem::FolderEmulator(0));
        let folder_b_row = items
            .iter()
            .position(|i| matches!(i, ScanFolderItem::FolderEmulator(1)))
            .expect("second folder row");

        // --- Enter on the populated folder: no rescan, no task slider. ---
        app.modal_state = ModalState::ScanFolderForm {
            platform: nds.clone(),
            folders: folders.clone(),
            selected_index: 0,
        };
        app.handle_scan_form_enter().await;
        assert!(
            app.scan_rx.is_none(),
            "Enter must not start a scan when all folder games are already imported"
        );
        assert!(
            app.download_progress.is_none(),
            "Enter must not spawn the task slider for an up-to-date folder"
        );
        assert!(
            app.status_msg.contains("up to date"),
            "expected an informational message, got: {}",
            app.status_msg
        );
        assert_eq!(app.db.get_game_count_for_folder(folder_a).unwrap(), 1);

        // --- Enter on a folder with new game files: scan runs normally. ---
        app.modal_state = ModalState::ScanFolderForm {
            platform: nds.clone(),
            folders: folders.clone(),
            selected_index: folder_b_row,
        };
        app.handle_scan_form_enter().await;
        assert!(
            app.scan_rx.is_some(),
            "Enter must scan when the folder has new game files: {}",
            app.status_msg
        );
        app.scan_rx = None;
        app.download_progress = None;

        // --- [R] forces a full rescan even for the populated folder. ---
        app.modal_state = ModalState::ScanFolderForm {
            platform: nds.clone(),
            folders: folders.clone(),
            selected_index: 0,
        };
        app.update(Action::RescanFolder).await;
        assert!(
            app.scan_rx.is_some(),
            "[R] must force a rescan of an already-scanned folder"
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// [E] sobre la plataforma Windows abre el manager de juegos Windows (lista
    /// de títulos con working dir), nunca el folder manager; sobre Steam no
    /// abre nada. Enter en el manager abre el formulario Wine directamente.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn folder_manager_bypasses_steam_and_lists_windows_games_with_wine_form() {
        let _xdg_guard = XDG_MUTEX.lock().unwrap();
        let old_data = std::env::var_os("XDG_DATA_HOME");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let tmp = std::env::temp_dir().join(format!(
            "tui_game_station_wine_mgr_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("XDG_DATA_HOME", tmp.join("data"));
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));

        let mut app = App::new().expect("App::new with isolated dirs");
        app.show_all_platforms = true;
        app.load_platforms();
        app.modal_state = ModalState::None;
        let windows = app
            .platforms
            .iter()
            .find(|p| p.slug == "windows")
            .expect("windows platform")
            .clone();
        let steam = app
            .platforms
            .iter()
            .find(|p| p.slug == "steam")
            .expect("steam platform")
            .clone();

        // --- Steam: [E] must not open the folder manager. ---
        app.selected_platform_idx = app.platforms.iter().position(|p| p.id == steam.id).unwrap();
        app.update(Action::OpenFolderManagerForPlatform).await;
        assert_eq!(
            app.modal_state,
            ModalState::None,
            "[E] must do nothing for the Steam platform"
        );

        // --- Windows: [E] opens the Windows Games manager (list, no folders). ---
        app.selected_platform_idx = app
            .platforms
            .iter()
            .position(|p| p.id == windows.id)
            .unwrap();
        app.db
            .insert_game(&Game {
                id: 0,
                platform_id: windows.id,
                folder_id: None,
                emulator_override: None,
                core_override: None,
                title: "Fallout 3".to_string(),
                sort_title: None,
                game_type: "wine".to_string(),
                file_path: Some("/games/windows/fallout3.exe".to_string()),
                working_dir: Some("/games/windows/fallout3".to_string()),
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
            })
            .unwrap();
        app.update(Action::OpenFolderManagerForPlatform).await;
        assert!(
            matches!(
                app.modal_state,
                ModalState::WindowsGamesManager { ref games, .. } if games.len() == 1
            ),
            "expected WindowsGamesManager with 1 game, got: {:?}",
            app.modal_state
        );

        // --- Enter on a game row → edit form with the manager as parent. ---
        app.handle_windows_games_enter().await;
        assert!(
            matches!(
                app.modal_state,
                ModalState::EditGameForm {
                    game_type: PlatformType::Wine,
                    ref parent_modal,
                    ..
                } if parent_modal.is_some()
            ),
            "Enter on a game row must open the edit form, got: {:?}",
            app.modal_state
        );

        // [Esc] returns to the manager list.
        app.update(Action::CloseModal).await;
        assert!(
            matches!(app.modal_state, ModalState::WindowsGamesManager { .. }),
            "[Esc] must return to the manager"
        );

        // --- Focus [ + Add New Game ] and Enter → Wine details form directly. ---
        if let ModalState::WindowsGamesManager { games, selected_idx } = &mut app.modal_state {
            *selected_idx = games.len();
        }
        app.handle_windows_games_enter().await;
        assert!(
            matches!(
                app.modal_state,
                ModalState::AddGameForm {
                    game_type: PlatformType::Wine,
                    ref parent_modal,
                    ..
                } if parent_modal.is_some()
            ),
            "Enter on [+ Add New Game] must open the Wine form with a parent, got: {:?}",
            app.modal_state
        );

        match old_data {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
