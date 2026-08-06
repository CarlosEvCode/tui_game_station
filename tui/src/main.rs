mod app;
mod cli;
mod cover_renderer;
mod figlet_title;
pub mod gamepad;
mod mouse_handler;
mod panic_hook;
mod single_instance;
mod toast;
mod ui;
pub mod updater;
pub mod edit_game_details;
mod window_helper;

use anyhow::Result;
use app::{
    Action, App, BigPictureFocus, FocusedPane, ModalState, scan_folder_add_core_idx,
    scan_folder_add_emu_idx, scan_folder_add_has_core, scan_folder_supports_dat,
};
use clap::Parser;
use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use game_core::models::PlatformType;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli_args = cli::CliArgs::parse();

    if cli_args.update {
        println!("--> Fetching and applying latest TUI Game Station update...");
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg("curl -fsSL https://raw.githubusercontent.com/CarlosEvCode/tui_game_station/main/install.sh | sh")
            .status()?;
        if !status.success() {
            eprintln!("Update failed.");
        }
        return Ok(());
    }

    if cli_args.uninstall {
        println!("--> Uninstalling TUI Game Station...");
        let exe_path = std::env::current_exe().ok();
        let home = std::env::var("HOME").unwrap_or_default();

        let target_local = format!("{}/.local/bin/tui-game-station", home);
        let target_global = "/usr/local/bin/tui-game-station";

        let mut removed = false;

        if std::path::Path::new(&target_local).exists() {
            println!("--> Removing {}...", target_local);
            if std::fs::remove_file(&target_local).is_ok() {
                removed = true;
            }
        }
        if std::path::Path::new(target_global).exists() {
            println!("--> Removing {}...", target_global);
            if std::fs::remove_file(target_global).is_ok() {
                removed = true;
            }
        }

        if let Some(ref path) = exe_path {
            if path.exists() && !removed && std::fs::remove_file(path).is_ok() {
                removed = true;
            }
        }

        if !removed {
            println!(
                "Notice: tui-game-station executable was not found in standard installation paths."
            );
        }

        if cli_args.purge {
            let config_dir = format!("{}/.config/tui_game_station", home);
            if std::path::Path::new(&config_dir).exists() {
                println!(
                    "--> Purging configuration and database at {}...",
                    config_dir
                );
                let _ = std::fs::remove_dir_all(&config_dir);
            }
        } else {
            let config_dir = format!("{}/.config/tui_game_station", home);
            if std::path::Path::new(&config_dir).exists() {
                println!(
                    "\nNote: User data and database remain saved at {}.",
                    config_dir
                );
                println!("To purge user data as well, run: tui-game-station --uninstall --purge");
            }
        }

        println!("\n[OK] TUI Game Station uninstalled successfully.");
        return Ok(());
    }

    // Install terminal recovery panic hook
    panic_hook::init_panic_hook();

    // File-based diagnostics log (timestamps + PIDs) for the launch/resume flow.
    init_file_logging();

    // Single-instance lock: refuse to start a second TUI while another one is
    // already running (two instances would compete for the same terminal and
    // gamepad and produce unpredictable input). Uses a real flock, so a crash
    // (kill -9) never leaves a blocking ghost lock. Done BEFORE raw mode / the
    // alternate screen, so a rejected instance never touches the terminal.
    let _single_instance_lock = match single_instance::acquire_single_instance_lock() {
        Ok(Some(file)) => Some(file),
        Ok(None) => {
            eprintln!("tui_game_station ya está en ejecución.");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::warn!(
                "No se pudo adquirir el lock de instancia única ({e}); se continúa sin él."
            );
            None
        }
    };

    // Enable Crossterm raw mode, alternate screen & mouse capture
    enable_raw_mode()?;
    let mut stdout_handle = stdout();
    execute!(
        stdout_handle,
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide
    )?;
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend)?;

    // Initialize application state
    let mut app = App::new()?;
    app.apply_cli_args(&cli_args);

    // Main event loop
    loop {
        app.check_download_events().await;
        app.check_game_exit().await;
        if app.needs_terminal_clear {
            app.needs_terminal_clear = false;
            terminal.clear()?;
        }
        terminal.draw(|f| ui::render_ui(f, &mut app))?;

        // Right after a game closes, discard any input still arriving while
        // the terminal focus settles, so stale gameplay input (held confirm
        // buttons, leftover key presses) can't trigger a phantom relaunch.
        if app.drain_stale_input() {
            continue;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if app.log_next_input {
                            app.log_next_input = false;
                            tracing::info!("[resume] first post-resume input: key event {key:?}");
                        }
                        app.active_input_source = crate::app::InputSource::Keyboard;
                        // Modal Input Handling
                        if app.modal_state != ModalState::None {
                            match key.code {
                                KeyCode::Esc => {
                                    if let ModalState::AppSettings {
                                        ref mut is_editing_api_key,
                                        ..
                                    } = app.modal_state
                                    {
                                        if *is_editing_api_key {
                                            *is_editing_api_key = false;
                                            continue;
                                        }
                                    }
                                    if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                        app.update(Action::ProtonDownloaderBack).await;
                                    } else if let ModalState::WelcomeWizard {
                                        ref sgdb_api_key,
                                        ..
                                    } = app.modal_state
                                    {
                                        let key = sgdb_api_key.clone();
                                        app.finish_welcome_wizard(&key);
                                    } else {
                                        app.update(Action::CloseModal).await;
                                    }
                                }
                                KeyCode::BackTab => {
                                    if key.modifiers.contains(KeyModifiers::SHIFT)
                                        || key.modifiers.contains(KeyModifiers::CONTROL)
                                    {
                                        app.update(Action::ModalPrevField).await;
                                    } else {
                                        app.update(Action::ModalNextField).await;
                                    }
                                }
                                KeyCode::Tab => {
                                    if let ModalState::VisualMediaSelector { .. } = app.modal_state
                                    {
                                        app.update(Action::SwitchVisualMediaTab).await;
                                    } else if let ModalState::WelcomeWizard {
                                        ref mut step, ..
                                    } = app.modal_state
                                    {
                                        *step = (*step + 1) % 4;
                                    } else if let ModalState::AppSettings {
                                        ref mut selected_field,
                                        ..
                                    } = app.modal_state
                                    {
                                        *selected_field = (*selected_field + 1) % 5;
                                    } else if let ModalState::ScanFolderForm { .. } =
                                        app.modal_state
                                    {
                                        app.update(Action::SwitchScanFolderPane).await;
                                    } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                                        app.update(Action::ModalPrevField).await;
                                    } else {
                                        app.update(Action::ModalNextField).await;
                                    }
                                }
                                KeyCode::Up => match &mut app.modal_state {
                                    ModalState::AppSettings {
                                        ref mut selected_field,
                                        ref mut is_editing_api_key,
                                        ..
                                    } => {
                                        *is_editing_api_key = false;
                                        if *selected_field > 0 {
                                            *selected_field -= 1;
                                        }
                                    }
                                    ModalState::WelcomeWizard {
                                        step: 3,
                                        ref mut active_field,
                                        ..
                                    } => {
                                        if *active_field > 0 {
                                            *active_field -= 1;
                                        }
                                    }
                                    ModalState::VisualMediaSelector { .. } => {
                                        app.update(Action::VisualMediaNavUp).await;
                                    }
                                    ModalState::ProtonDownloader { .. } => {
                                        app.update(Action::ProtonDownloaderSelectPrev).await;
                                    }
                                    ModalState::ConfirmDeleteGame { .. } => {
                                        app.update(Action::ToggleConfirmDeleteOption).await;
                                    }
                                    ModalState::ConfirmDeleteRunner { .. } => {
                                        app.update(Action::ToggleConfirmDeleteRunnerOption).await;
                                    }
                                    ModalState::AddGameStep1Type { .. }
                                    | ModalState::ScanFolderStep1Platform { .. }
                                    | ModalState::ManageRunnersStep1Platform { .. }
                                    | ModalState::ManageWineRunners { .. }
                                    | ModalState::SelectWineRunnerPicker { .. }
                                    | ModalState::PlatformSelector { .. }
                                    | ModalState::WineToolsMenu { .. } => {
                                        app.update(Action::ModalSelectPrev).await;
                                    }
                                    ModalState::ManageRunnersStep2Config {
                                        ref options,
                                        ref mut selected_row,
                                        ..
                                    } => {
                                        let total = options.len() + 3;
                                        *selected_row = if *selected_row == 0 {
                                            total - 1
                                        } else {
                                            *selected_row - 1
                                        };
                                    }
                                    ModalState::ScanFolderForm { .. }
                                    | ModalState::AddFolderScanForm { .. } => {
                                        app.update(Action::ModalPrevField).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalSelectPrev).await;
                                    }
                                },
                                KeyCode::Down => match &mut app.modal_state {
                                    ModalState::AppSettings {
                                        ref mut selected_field,
                                        ref mut is_editing_api_key,
                                        ..
                                    } => {
                                        *is_editing_api_key = false;
                                        if *selected_field < 4 {
                                            *selected_field += 1;
                                        }
                                    }
                                    ModalState::WelcomeWizard {
                                        step: 3,
                                        ref mut active_field,
                                        ..
                                    } => {
                                        if *active_field < 1 {
                                            *active_field += 1;
                                        }
                                    }
                                    ModalState::VisualMediaSelector { .. } => {
                                        app.update(Action::VisualMediaNavDown).await;
                                    }
                                    ModalState::ProtonDownloader { .. } => {
                                        app.update(Action::ProtonDownloaderSelectNext).await;
                                    }
                                    ModalState::ConfirmDeleteGame { .. } => {
                                        app.update(Action::ToggleConfirmDeleteOption).await;
                                    }
                                    ModalState::ConfirmDeleteRunner { .. } => {
                                        app.update(Action::ToggleConfirmDeleteRunnerOption).await;
                                    }
                                    ModalState::ManageRunnersStep2Config {
                                        ref options,
                                        ref mut selected_row,
                                        ..
                                    } => {
                                        let total = options.len() + 3;
                                        *selected_row = (*selected_row + 1) % total;
                                    }
                                    ModalState::AddGameStep1Type { .. }
                                    | ModalState::ScanFolderStep1Platform { .. }
                                    | ModalState::ManageRunnersStep1Platform { .. }
                                    | ModalState::ManageWineRunners { .. }
                                    | ModalState::SelectWineRunnerPicker { .. }
                                    | ModalState::PlatformSelector { .. }
                                    | ModalState::DownloadCoreModal { .. }
                                    | ModalState::WineToolsMenu { .. } => {
                                        app.update(Action::ModalSelectNext).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalNextField).await;
                                    }
                                },
                                KeyCode::Left => match &mut app.modal_state {
                                    ModalState::WelcomeWizard {
                                        step,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *step == 2 {
                                            if *cursor_pos > 0 {
                                                *cursor_pos -= 1;
                                            } else if *step > 0 {
                                                *step -= 1;
                                            }
                                        } else if *step > 0 {
                                            *step -= 1;
                                        }
                                    }
                                    ModalState::AppSettings {
                                        selected_field: 0,
                                        is_editing_api_key: true,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *cursor_pos > 0 {
                                            *cursor_pos -= 1;
                                        }
                                    }
                                    ModalState::VisualMediaSelector { .. } => {
                                        app.update(Action::VisualMediaNavLeft).await;
                                    }
                                    ModalState::ManageRunnersStep2Config {
                                        ref options,
                                        ref mut option_values,
                                        ref selected_row,
                                        ref mut selected_action_idx,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *selected_row == 0 {
                                            if *cursor_pos > 0 {
                                                *cursor_pos -= 1;
                                            }
                                        } else if *selected_row >= 1
                                            && *selected_row <= options.len()
                                        {
                                            crate::app::cycle_runner_option(
                                                options,
                                                option_values,
                                                *selected_row - 1,
                                                true,
                                            );
                                        } else if *selected_row == options.len() + 1 {
                                            if *cursor_pos > 0 {
                                                *cursor_pos -= 1;
                                            }
                                        } else if *selected_action_idx > 0 {
                                            *selected_action_idx -= 1;
                                        }
                                    }
                                    ModalState::EditCustomArgsInput { .. }
                                    | ModalState::AddGameForm { .. }
                                    | ModalState::EditGameForm { .. } => {
                                        app.update(Action::FormNavLeft).await;
                                    }
                                    ModalState::ProtonDownloader { .. } => {
                                        app.update(Action::ProtonDownloaderBack).await;
                                    }
                                    ModalState::ConfirmDeleteGame { .. } => {
                                        app.update(Action::ToggleConfirmDeleteOption).await;
                                    }
                                    ModalState::ConfirmDeleteRunner { .. } => {
                                        app.update(Action::ToggleConfirmDeleteRunnerOption).await;
                                    }
                                    ModalState::ConfirmDeleteFolder { .. } => {
                                        app.update(Action::ToggleConfirmDeleteFolderOption).await;
                                    }
                                    ModalState::ScanFolderForm {
                                        ref platform,
                                        add_emulator_id,
                                        focused_pane,
                                        selected_field,
                                        ..
                                    } => {
                                        if *focused_pane == 1 {
                                            // Right pane: cycle the new-folder
                                            // Emulador / Core selector with ◀.
                                            let dat = scan_folder_supports_dat(&platform.slug);
                                            let emu_idx = scan_folder_add_emu_idx(dat);
                                            let has_core = scan_folder_add_has_core(
                                                &app.db,
                                                platform.id,
                                                *add_emulator_id,
                                            );
                                            let core_idx = scan_folder_add_core_idx(dat);
                                            if *selected_field == emu_idx {
                                                app.cycle_add_folder_emulator(true);
                                            } else if has_core && *selected_field == core_idx {
                                                app.cycle_add_folder_core(true);
                                            }
                                        } else if *selected_field < app.scan_folder_num_rows() {
                                            // Left pane: folder rows re-assign
                                            // their emulator with ◀.
                                            app.update(Action::CycleFolderEmulator(true)).await;
                                        }
                                    }
                                    ModalState::AddFolderScanForm {
                                        ref platform,
                                        add_emulator_id,
                                        selected_field,
                                        ..
                                    } => {
                                        let dat = scan_folder_supports_dat(&platform.slug);
                                        let emu_idx = scan_folder_add_emu_idx(dat);
                                        let has_core = scan_folder_add_has_core(
                                            &app.db,
                                            platform.id,
                                            *add_emulator_id,
                                        );
                                        let core_idx = scan_folder_add_core_idx(dat);
                                        if *selected_field == emu_idx {
                                            app.cycle_add_folder_emulator(true);
                                        } else if has_core && *selected_field == core_idx {
                                            app.cycle_add_folder_core(true);
                                        }
                                    }
                                    ModalState::None if app.focused_pane == FocusedPane::Platforms => {
                                        // ◀ Emulador (o Núcleo) anterior.
                                        app.update(Action::CycleActiveEmulatorPrev).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalSelectPrev).await;
                                    }
                                },
                                KeyCode::Right => match &mut app.modal_state {
                                    ModalState::WelcomeWizard {
                                        step,
                                        ref sgdb_api_key,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *step == 2 {
                                            if *cursor_pos < sgdb_api_key.len() {
                                                *cursor_pos += 1;
                                            } else if *step < 3 {
                                                *step += 1;
                                            }
                                        } else if *step < 3 {
                                            *step += 1;
                                        }
                                    }
                                    ModalState::AppSettings {
                                        selected_field: 0,
                                        is_editing_api_key: true,
                                        ref api_key_input,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *cursor_pos < api_key_input.len() {
                                            *cursor_pos += 1;
                                        }
                                    }
                                    ModalState::VisualMediaSelector { .. } => {
                                        app.update(Action::VisualMediaNavRight).await;
                                    }
                                    ModalState::ManageRunnersStep2Config {
                                        ref runner_info,
                                        ref exe_path_input,
                                        ref options,
                                        ref mut option_values,
                                        ref custom_args,
                                        ref selected_row,
                                        ref mut selected_action_idx,
                                        ref mut cursor_pos,
                                        ..
                                    } => {
                                        if *selected_row == 0 {
                                            if *cursor_pos < exe_path_input.len() {
                                                *cursor_pos += 1;
                                            }
                                        } else if *selected_row >= 1
                                            && *selected_row <= options.len()
                                        {
                                            crate::app::cycle_runner_option(
                                                options,
                                                option_values,
                                                *selected_row - 1,
                                                false,
                                            );
                                        } else if *selected_row == options.len() + 1 {
                                            if *cursor_pos < custom_args.len() {
                                                *cursor_pos += 1;
                                            }
                                        } else {
                                            let has_executable = runner_info
                                                .executable_path
                                                .as_ref()
                                                .map(|p| {
                                                    !p.trim().is_empty()
                                                        && std::path::Path::new(p).exists()
                                                })
                                                .unwrap_or(false);
                                            let mut total_btns = 2;
                                            if runner_info.download_url.is_some() {
                                                total_btns += 1;
                                            }
                                            if has_executable {
                                                total_btns += 2;
                                            }
                                            if *selected_action_idx + 1 < total_btns {
                                                *selected_action_idx += 1;
                                            }
                                        }
                                    }
                                    ModalState::EditCustomArgsInput { .. }
                                    | ModalState::AddGameForm { .. }
                                    | ModalState::EditGameForm { .. } => {
                                        app.update(Action::FormNavRight).await;
                                    }
                                    ModalState::ProtonDownloader { .. } => {
                                        app.update(Action::ProtonDownloaderConfirm).await;
                                    }
                                    ModalState::ConfirmDeleteGame { .. } => {
                                        app.update(Action::ToggleConfirmDeleteOption).await;
                                    }
                                    ModalState::ConfirmDeleteRunner { .. } => {
                                        app.update(Action::ToggleConfirmDeleteRunnerOption).await;
                                    }
                                    ModalState::ConfirmDeleteFolder { .. } => {
                                        app.update(Action::ToggleConfirmDeleteFolderOption).await;
                                    }
                                    ModalState::ScanFolderForm {
                                        ref platform,
                                        add_emulator_id,
                                        focused_pane,
                                        selected_field,
                                        ..
                                    } => {
                                        if *focused_pane == 1 {
                                            // Right pane: cycle the new-folder
                                            // Emulador / Core selector with ▶.
                                            let dat = scan_folder_supports_dat(&platform.slug);
                                            let emu_idx = scan_folder_add_emu_idx(dat);
                                            let has_core = scan_folder_add_has_core(
                                                &app.db,
                                                platform.id,
                                                *add_emulator_id,
                                            );
                                            let core_idx = scan_folder_add_core_idx(dat);
                                            if *selected_field == emu_idx {
                                                app.cycle_add_folder_emulator(false);
                                            } else if has_core && *selected_field == core_idx {
                                                app.cycle_add_folder_core(false);
                                            }
                                        } else if *selected_field < app.scan_folder_num_rows() {
                                            app.update(Action::CycleFolderEmulator(false)).await;
                                        }
                                    }
                                    ModalState::AddFolderScanForm {
                                        ref platform,
                                        add_emulator_id,
                                        selected_field,
                                        ..
                                    } => {
                                        let dat = scan_folder_supports_dat(&platform.slug);
                                        let emu_idx = scan_folder_add_emu_idx(dat);
                                        let has_core = scan_folder_add_has_core(
                                            &app.db,
                                            platform.id,
                                            *add_emulator_id,
                                        );
                                        let core_idx = scan_folder_add_core_idx(dat);
                                        if *selected_field == emu_idx {
                                            app.cycle_add_folder_emulator(false);
                                        } else if has_core && *selected_field == core_idx {
                                            app.cycle_add_folder_core(false);
                                        }
                                    }
                                    ModalState::None if app.focused_pane == FocusedPane::Platforms => {
                                        // Emulador (o Núcleo) siguiente ▶.
                                        app.update(Action::CycleActiveEmulatorNext).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalSelectNext).await;
                                    }
                                },
                                KeyCode::Home => {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        *cursor_pos = 0;
                                    } else if let ModalState::AppSettings {
                                        selected_field: 0,
                                        is_editing_api_key: true,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        *cursor_pos = 0;
                                    }
                                }
                                KeyCode::End => {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref sgdb_api_key,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        *cursor_pos = sgdb_api_key.len();
                                    } else if let ModalState::AppSettings {
                                        selected_field: 0,
                                        is_editing_api_key: true,
                                        ref api_key_input,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        *cursor_pos = api_key_input.len();
                                    }
                                }
                                KeyCode::Char('1') => {
                                    if let ModalState::VisualMediaSelector { .. } = app.modal_state
                                    {
                                        app.update(Action::SetVisualMediaTab(0)).await;
                                    } else {
                                        app.update(Action::ModalInputChar('1')).await;
                                    }
                                }
                                KeyCode::Char('2') => {
                                    if let ModalState::VisualMediaSelector { .. } = app.modal_state
                                    {
                                        app.update(Action::SetVisualMediaTab(1)).await;
                                    } else {
                                        app.update(Action::ModalInputChar('2')).await;
                                    }
                                }
                                KeyCode::Char('3') => {
                                    if let ModalState::VisualMediaSelector { .. } = app.modal_state
                                    {
                                        app.update(Action::SetVisualMediaTab(2)).await;
                                    } else {
                                        app.update(Action::ModalInputChar('3')).await;
                                    }
                                }
                                KeyCode::Char('4') => {
                                    if let ModalState::VisualMediaSelector { .. } = app.modal_state
                                    {
                                        app.update(Action::SetVisualMediaTab(3)).await;
                                    } else {
                                        app.update(Action::ModalInputChar('4')).await;
                                    }
                                }
                                KeyCode::Char('r') => {
                                    if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                        app.update(Action::ProtonDownloaderSelectNext).await;
                                    } else {
                                        app.update(Action::ModalInputChar('r')).await;
                                    }
                                }
                                KeyCode::Char(' ') => {
                                    let is_form = matches!(
                                        app.modal_state,
                                        ModalState::AddGameForm { .. }
                                            | ModalState::EditGameForm { .. }
                                    );
                                    if is_form {
                                        let selected_field = match &app.modal_state {
                                            ModalState::AddGameForm { selected_field, .. }
                                            | ModalState::EditGameForm { selected_field, .. } => {
                                                *selected_field
                                            }
                                            _ => 0,
                                        };
                                        let game_type = match &app.modal_state {
                                            ModalState::AddGameForm { game_type, .. }
                                            | ModalState::EditGameForm { game_type, .. } => {
                                                game_type
                                            }
                                            _ => &PlatformType::Native,
                                        };
                                        let on_checkbox = match game_type {
                                            PlatformType::Wine => {
                                                (6..=12).contains(&selected_field)
                                            }
                                            PlatformType::Native => {
                                                (4..=6).contains(&selected_field)
                                            }
                                            PlatformType::Emulator | PlatformType::Steam => {
                                                (3..=5).contains(&selected_field)
                                            }
                                        };
                                        if on_checkbox {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            app.update(Action::ModalInputChar(' ')).await;
                                        }
                                    } else if let ModalState::ScanFolderForm {
                                        focused_pane,
                                        selected_field,
                                        ..
                                    } = app.modal_state
                                    {
                                        // Left pane: Space toggles a folder row
                                        // in/out of the multi-selection. Right
                                        // pane: toggles checkboxes or types a
                                        // space into text fields.
                                        if focused_pane == 0 {
                                            app.update(Action::ToggleSelectFolder).await;
                                        } else if selected_field == 2
                                            || selected_field == 3
                                        {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            app.update(Action::ModalInputChar(' ')).await;
                                        }
                                    } else if let ModalState::AddFolderScanForm {
                                        selected_field,
                                        ..
                                    } = app.modal_state
                                    {
                                        if selected_field == 2 || selected_field == 3 {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            app.update(Action::ModalInputChar(' ')).await;
                                        }
                                    } else if let ModalState::ManageRunnersStep2Config {
                                        ref options,
                                        ref mut option_values,
                                        ref selected_row,
                                        ..
                                    } = app.modal_state
                                    {
                                        if *selected_row >= 1 && *selected_row <= options.len() {
                                            crate::app::cycle_runner_option(
                                                options,
                                                option_values,
                                                *selected_row - 1,
                                                false,
                                            );
                                        } else {
                                            app.update(Action::ModalInputChar(' ')).await;
                                        }
                                    } else if let ModalState::VisualMediaSelector {
                                        active_tab,
                                        ..
                                    } = app.modal_state
                                    {
                                        if active_tab > 0 {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            app.update(Action::ModalInputChar(' ')).await;
                                        }
                                    } else {
                                        app.update(Action::ModalInputChar(' ')).await;
                                    }
                                }
                                KeyCode::Enter => match app.modal_state {
                                    ModalState::AddGameStep1Type { .. } => {
                                        app.update(Action::ModalConfirmStep1).await;
                                    }
                                    ModalState::ScanFolderStep1Platform { .. } => {
                                        app.update(Action::ScanModalConfirmPlatform).await;
                                    }
                                    ModalState::ConfigureApiKeyInput { .. } => {
                                        app.update(Action::SaveApiKey).await;
                                    }
                                    ModalState::AppSettings {
                                        selected_field,
                                        ref mut is_editing_api_key,
                                        ref api_key_input,
                                        ref mut cursor_pos,
                                    } => {
                                        if selected_field == 0 {
                                            *is_editing_api_key = !*is_editing_api_key;
                                            if *is_editing_api_key {
                                                *cursor_pos = api_key_input.len();
                                            }
                                        } else if selected_field == 1 {
                                            app.update(Action::OpenWelcomeWizardModal).await;
                                        } else if selected_field == 2 {
                                            app.update(Action::OpenAboutModal).await;
                                        } else if selected_field == 3 {
                                            app.update(Action::CheckForUpdates { silent: false })
                                                .await;
                                        } else if selected_field == 4 {
                                            app.update(Action::SaveAppSettings).await;
                                        }
                                    }
                                    ModalState::UpdateAvailable {
                                        ref download_url,
                                        ref new_version,
                                        ..
                                    } => {
                                        let url = download_url.clone();
                                        let ver = new_version.clone();
                                        app.update(Action::StartAppUpdate {
                                            download_url: url,
                                            new_version: ver,
                                        })
                                        .await;
                                    }
                                    ModalState::WelcomeWizard {
                                        step,
                                        ref sgdb_api_key,
                                        ..
                                    } => {
                                        if step < 3 {
                                            if let ModalState::WelcomeWizard {
                                                ref mut step, ..
                                            } = app.modal_state
                                            {
                                                *step += 1;
                                            }
                                        } else {
                                            let key = sgdb_api_key.clone();
                                            app.finish_welcome_wizard(&key);
                                        }
                                    }
                                    ModalState::VisualMediaSelector {
                                        focused_section,
                                        active_tab,
                                        ref candidates,
                                        ..
                                    } => {
                                        if focused_section == 1
                                            || (focused_section == 0 && active_tab == 0)
                                        {
                                            app.update(Action::SearchVisualMedia).await;
                                        } else if active_tab == 0 {
                                            if candidates.is_empty() {
                                                app.update(Action::SearchVisualMedia).await;
                                            } else {
                                                app.update(Action::SelectVisualMediaCandidate)
                                                    .await;
                                            }
                                        } else {
                                            app.update(Action::ApplyVisualMediaSelection).await;
                                        }
                                    }
                                    ModalState::ManageRunnersStep1Platform { .. } => {
                                        app.update(Action::RunnerModalConfirmPlatform).await;
                                    }
                                    ModalState::ManageWineRunners { .. } => {
                                        app.update(Action::OpenProtonDownloader).await;
                                    }
                                    ModalState::ProtonDownloader { .. } => {
                                        app.update(Action::ProtonDownloaderConfirm).await;
                                    }
                                    ModalState::SelectWineRunnerPicker { .. } => {
                                        app.update(Action::SelectWineRunnerFromPicker).await;
                                    }
                                    ModalState::WineToolsMenu { .. } => {
                                        app.update(Action::SelectWineTool).await;
                                        if let Some(cmd) = app.pending_wine_tool.take() {
                                            match cmd.exe.as_str() {
                                                "winecfg" | "winetricks" => {
                                                    disable_raw_mode()?;
                                                    execute!(
                                                        stdout(),
                                                        LeaveAlternateScreen,
                                                        cursor::Show
                                                    )?;
                                                    let mut child =
                                                        std::process::Command::new(&cmd.exe)
                                                            .args(&cmd.args)
                                                            .envs(&cmd.envs)
                                                            .spawn()
                                                            .ok();
                                                    if let Some(ref mut c) = child {
                                                        let _ = c.wait();
                                                    }
                                                    enable_raw_mode()?;
                                                    execute!(
                                                        stdout(),
                                                        EnterAlternateScreen,
                                                        cursor::Hide
                                                    )?;
                                                    terminal.clear()?;
                                                    terminal
                                                        .draw(|f| ui::render_ui(f, &mut app))?;
                                                }
                                                "wineserver" => {
                                                    let _ = std::process::Command::new(&cmd.exe)
                                                        .args(&cmd.args)
                                                        .envs(&cmd.envs)
                                                        .output();
                                                }
                                                "xdg-open" => {
                                                    let path = cmd
                                                        .args
                                                        .first()
                                                        .map(|s| s.as_str())
                                                        .unwrap_or("");
                                                    if std::path::Path::new(path).exists() {
                                                        let _ =
                                                            std::process::Command::new(&cmd.exe)
                                                                .args(&cmd.args)
                                                                .spawn();
                                                    } else {
                                                        app.status_msg = format!("[Warning] Prefix folder does not exist: {}", path);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    ModalState::EditCustomArgsInput { .. } => {
                                        app.update(Action::SaveCustomArgsInput).await;
                                    }
                                    ModalState::ConfirmDeleteGame { .. } => {
                                        app.update(Action::ConfirmDeleteGameExecution).await;
                                    }
                                    ModalState::ConfirmDeleteRunner { .. } => {
                                        app.update(Action::ConfirmDeleteRunnerExecution).await;
                                    }
                                    ModalState::ConfirmDeleteFolder { .. } => {
                                        app.update(Action::ConfirmDeleteFolderExecution).await;
                                    }
                                    ModalState::PlatformSelector { .. } => {
                                        app.update(Action::ConfirmPlatformSelectorModal).await;
                                    }
                                    ModalState::AddGameForm {
                                        ref game_type,
                                        selected_field,
                                        ..
                                    } => {
                                        let on_checkbox = match game_type {
                                            PlatformType::Wine => {
                                                (6..=12).contains(&selected_field)
                                            }
                                            PlatformType::Native => {
                                                (4..=6).contains(&selected_field)
                                            }
                                            PlatformType::Emulator | PlatformType::Steam => {
                                                (3..=5).contains(&selected_field)
                                            }
                                        };
                                        if on_checkbox {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            match game_type {
                                                PlatformType::Wine => match selected_field {
                                                    1..=3 => {
                                                        app.update(Action::OpenFilePicker).await
                                                    }
                                                    4 => {
                                                        app.update(Action::OpenWineRunnerPicker)
                                                            .await
                                                    }
                                                    5 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    13 => app.update(Action::SaveModalGame).await,
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Native => match selected_field {
                                                    1 | 2 => {
                                                        app.update(Action::OpenFilePicker).await
                                                    }
                                                    3 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    7 => app.update(Action::SaveModalGame).await,
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Emulator => match selected_field {
                                                    1 => app.update(Action::OpenFilePicker).await,
                                                    2 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    6 => app.update(Action::SaveModalGame).await,
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Steam => match selected_field {
                                                    2 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    6 => app.update(Action::SaveModalGame).await,
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                            }
                                        }
                                    }
                                    ModalState::EditGameForm {
                                        ref game_type,
                                        selected_field,
                                        ..
                                    } => {
                                        let on_checkbox = match game_type {
                                            PlatformType::Wine => {
                                                (6..=12).contains(&selected_field)
                                            }
                                            PlatformType::Native => {
                                                (4..=6).contains(&selected_field)
                                            }
                                            PlatformType::Emulator | PlatformType::Steam => {
                                                (3..=5).contains(&selected_field)
                                            }
                                        };
                                        if on_checkbox {
                                            app.update(Action::ModalToggleCheckbox).await;
                                        } else {
                                            match game_type {
                                                PlatformType::Wine => match selected_field {
                                                    1..=3 => {
                                                        app.update(Action::OpenFilePicker).await
                                                    }
                                                    4 => {
                                                        app.update(Action::OpenWineRunnerPicker)
                                                            .await
                                                    }
                                                    5 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    13 => {
                                                        app.update(Action::SaveEditGameModal).await
                                                    }
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Native => match selected_field {
                                                    1 | 2 => {
                                                        app.update(Action::OpenFilePicker).await
                                                    }
                                                    3 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    7 => {
                                                        app.update(Action::SaveEditGameModal).await
                                                    }
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Emulator => match selected_field {
                                                    1 => app.update(Action::OpenFilePicker).await,
                                                    2 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    6 => {
                                                        app.update(Action::SaveEditGameModal).await
                                                    }
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                                PlatformType::Steam => match selected_field {
                                                    2 => {
                                                        app.update(Action::OpenCustomArgsEditor)
                                                            .await
                                                    }
                                                    6 => {
                                                        app.update(Action::SaveEditGameModal).await
                                                    }
                                                    _ => app.update(Action::ModalNextField).await,
                                                },
                                            }
                                        }
                                    }
                                    ModalState::ScanFolderForm { .. } => {
                                        app.handle_scan_form_enter().await;
                                    }
                                    ModalState::AddFolderScanForm { .. } => {
                                        app.handle_add_scan_form_enter().await;
                                    }
                                    ModalState::DownloadCoreModal { .. } => {
                                        app.update(Action::TriggerDownloadCore).await;
                                    }
                                    ModalState::ManageRunnersStep2Config {
                                        ref runner_info,
                                        ref exe_path_input,
                                        ref options,
                                        ref mut option_values,
                                        ref selected_row,
                                        selected_action_idx,
                                        ..
                                    } => {
                                        if *selected_row == 0 {
                                            if exe_path_input.trim().is_empty() {
                                                app.update(Action::OpenFilePicker).await;
                                            } else if let ModalState::ManageRunnersStep2Config {
                                                ref mut selected_row,
                                                ref options,
                                                ..
                                            } = app.modal_state
                                            {
                                                *selected_row = options.len() + 2;
                                            }
                                        } else if *selected_row >= 1
                                            && *selected_row <= options.len()
                                        {
                                            crate::app::cycle_runner_option(
                                                options,
                                                option_values,
                                                *selected_row - 1,
                                                false,
                                            );
                                        } else if *selected_row == options.len() + 1 {
                                            app.update(Action::OpenCustomArgsEditor).await;
                                        } else {
                                            let has_executable = !exe_path_input.trim().is_empty()
                                                && std::path::Path::new(exe_path_input.trim())
                                                    .exists();
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

                                            let act = actions
                                                .get(selected_action_idx)
                                                .copied()
                                                .unwrap_or("save");
                                            match act {
                                                "browse" => {
                                                    app.update(Action::OpenFilePicker).await
                                                }
                                                "download" => {
                                                    app.update(Action::StartRunnerDownload).await
                                                }
                                                "save" => {
                                                    app.update(Action::SaveRunnerConfig).await
                                                }
                                                "open" => {
                                                    app.update(Action::OpenRunnerStandalone).await
                                                }
                                                "toggle_active" => {
                                                    app.update(Action::ToggleRunnerActiveState)
                                                        .await
                                                }
                                                "delete" => {
                                                    app.update(Action::OpenConfirmDeleteRunnerModal)
                                                        .await
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    _ => {
                                        app.update(Action::SaveModalGame).await;
                                    }
                                },
                                KeyCode::Backspace => {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref mut sgdb_api_key,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        if *cursor_pos > 0 && !sgdb_api_key.is_empty() {
                                            sgdb_api_key.remove(*cursor_pos - 1);
                                            *cursor_pos -= 1;
                                        }
                                    } else if let ModalState::FuzzySearchModal {
                                        ref mut query,
                                        ref mut cursor_pos,
                                    } = app.modal_state
                                    {
                                        if *cursor_pos > 0 && !query.is_empty() {
                                            query.remove(*cursor_pos - 1);
                                            *cursor_pos -= 1;
                                            let q = query.clone();
                                            app.update(Action::UpdateFuzzySearchQuery(q)).await;
                                        }
                                    } else {
                                        app.update(Action::ModalBackspace).await;
                                    }
                                }
                                KeyCode::Delete => {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref mut sgdb_api_key,
                                        cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        if cursor_pos < sgdb_api_key.len() {
                                            sgdb_api_key.remove(cursor_pos);
                                        }
                                    } else if let ModalState::AppSettings {
                                        selected_field: 0,
                                        is_editing_api_key: true,
                                        ref mut api_key_input,
                                        cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        if cursor_pos < api_key_input.len() {
                                            api_key_input.remove(cursor_pos);
                                        }
                                    } else if let ModalState::ManageWineRunners { .. } =
                                        app.modal_state
                                    {
                                        app.update(Action::DeleteInstalledWineRunner).await;
                                    } else if let ModalState::ScanFolderForm {
                                        focused_pane,
                                        ..
                                    } = app.modal_state
                                    {
                                        // Left pane: Delete opens the removal
                                        // confirmation for the selected rows.
                                        if focused_pane == 0 {
                                            app.update(Action::OpenConfirmDeleteFolder).await;
                                        }
                                    }
                                }
                                KeyCode::Char('v') | KeyCode::Char('V')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref mut sgdb_api_key,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        if let Some(pasted) = crate::app::get_clipboard_text() {
                                            sgdb_api_key.insert_str(*cursor_pos, &pasted);
                                            *cursor_pos += pasted.len();
                                        }
                                    } else if let ModalState::AppSettings {
                                        selected_field: 0,
                                        ref mut is_editing_api_key,
                                        ref mut api_key_input,
                                        ref mut cursor_pos,
                                    } = app.modal_state
                                    {
                                        if let Some(pasted) = crate::app::get_clipboard_text() {
                                            *is_editing_api_key = true;
                                            let pos = (*cursor_pos).min(api_key_input.len());
                                            api_key_input.insert_str(pos, &pasted);
                                            *cursor_pos = pos + pasted.len();
                                        }
                                    } else if let ModalState::ConfigureApiKeyInput {
                                        ref mut input,
                                    } = app.modal_state
                                    {
                                        if let Some(pasted) = crate::app::get_clipboard_text() {
                                            input.push_str(&pasted);
                                        }
                                    }
                                }
                                KeyCode::Char('d') => {
                                    if let ModalState::ManageWineRunners { .. } = app.modal_state {
                                        app.update(Action::OpenProtonDownloader).await;
                                    } else {
                                        app.update(Action::ModalInputChar('d')).await;
                                    }
                                }
                                KeyCode::Char('p') => match app.modal_state {
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
                                        app.update(Action::OpenWineRunnerPicker).await;
                                    }
                                    ModalState::AddGameForm {
                                        game_type: PlatformType::Wine,
                                        selected_field: 5,
                                        ..
                                    }
                                    | ModalState::EditGameForm {
                                        game_type: PlatformType::Wine,
                                        selected_field: 5,
                                        ..
                                    } => {
                                        app.update(Action::OpenCustomArgsEditor).await;
                                    }
                                    ModalState::ManageRunnersStep1Platform { .. } => {
                                        app.update(Action::OpenWineRunnerManager).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalInputChar('p')).await;
                                    }
                                },
                                KeyCode::Char('t') => {
                                    if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                        app.update(Action::ProtonDownloaderSelectNext).await;
                                    } else {
                                        app.update(Action::ModalInputChar('t')).await;
                                    }
                                }
                                KeyCode::Char('w') => {
                                    if let ModalState::ManageRunnersStep1Platform { .. } =
                                        app.modal_state
                                    {
                                        app.update(Action::OpenWineRunnerManager).await;
                                    } else {
                                        app.update(Action::ModalInputChar('w')).await;
                                    }
                                }
                                KeyCode::Char('x') => {
                                    app.update(Action::ModalInputChar('x')).await;
                                }
                                KeyCode::Char('u') | KeyCode::Char('U') => {
                                    if let ModalState::About = app.modal_state {
                                        app.update(Action::CheckForUpdates { silent: false }).await;
                                    } else {
                                        app.update(Action::ModalInputChar('u')).await;
                                    }
                                }
                                KeyCode::Char('f') => {
                                    app.update(Action::ModalInputChar('f')).await;
                                }
                                KeyCode::Char(c) => {
                                    if let ModalState::WelcomeWizard {
                                        step: 2,
                                        ref mut sgdb_api_key,
                                        ref mut cursor_pos,
                                        ..
                                    } = app.modal_state
                                    {
                                        sgdb_api_key.insert(*cursor_pos, c);
                                        *cursor_pos += 1;
                                    } else if let ModalState::FuzzySearchModal {
                                        ref mut query,
                                        ref mut cursor_pos,
                                    } = app.modal_state
                                    {
                                        query.insert(*cursor_pos, c);
                                        *cursor_pos += 1;
                                        let q = query.clone();
                                        app.update(Action::UpdateFuzzySearchQuery(q)).await;
                                    } else {
                                        app.update(Action::ModalInputChar(c)).await;
                                    }
                                }
                                _ => {}
                            }
                        } else if app.is_big_picture && app.big_picture_in_detail {
                            // Game Detail View: dedicated controls
                            match key.code {
                                KeyCode::Enter => {
                                    if app.detail_action_idx == 0 {
                                        app.update(Action::LaunchGame).await;
                                    } else {
                                        app.show_toast(
                                            "This action will be available soon.",
                                            crate::toast::ToastKind::Info,
                                        );
                                    }
                                    terminal.clear()?;
                                    terminal.draw(|f| ui::render_ui(f, &mut app))?;
                                }
                                KeyCode::Esc => {
                                    app.update(Action::CloseGameDetail).await;
                                }
                                KeyCode::Left => {
                                    app.update(Action::DetailPrevAction).await;
                                }
                                KeyCode::Right => {
                                    app.update(Action::DetailNextAction).await;
                                }
                                _ => {}
                            }
                        } else {
                            // Main View Keyboard Shortcuts & Interactive Search Input
                            let is_search_active_focus = if app.is_big_picture {
                                app.big_picture_focus == BigPictureFocus::Search
                            } else {
                                app.focused_pane == FocusedPane::Search
                            };

                            if is_search_active_focus {
                                match key.code {
                                    KeyCode::Esc => {
                                        app.search_query.clear();
                                        app.filter_games_by_search();
                                        if app.is_big_picture {
                                            app.big_picture_focus = BigPictureFocus::Carousel;
                                        } else {
                                            app.focused_pane = FocusedPane::Platforms;
                                        }
                                    }
                                    KeyCode::Enter | KeyCode::Down => {
                                        if app.is_big_picture {
                                            app.big_picture_focus = BigPictureFocus::Carousel;
                                        } else {
                                            app.focused_pane = FocusedPane::Platforms;
                                        }
                                    }
                                    KeyCode::Backspace => {
                                        if !app.search_query.is_empty() {
                                            app.search_query.pop();
                                            app.filter_games_by_search();
                                        }
                                    }
                                    KeyCode::Tab => {
                                        if app.is_big_picture {
                                            app.update(Action::ToggleBigPictureFocus).await;
                                        } else {
                                            app.update(Action::TogglePane).await;
                                        }
                                    }
                                    KeyCode::Char('o') | KeyCode::Char('O')
                                        if key.modifiers.contains(KeyModifiers::ALT) =>
                                    {
                                        app.update(Action::ToggleBigPictureMode).await;
                                    }
                                    KeyCode::Char(c) => {
                                        app.search_query.push(c);
                                        app.filter_games_by_search();
                                    }
                                    _ => {}
                                }
                            } else {
                                match key.code {
                                    KeyCode::Char('/') => {
                                        if app.is_big_picture {
                                            app.big_picture_focus = BigPictureFocus::Search;
                                        } else {
                                            app.focused_pane = FocusedPane::Search;
                                        }
                                    }
                                    KeyCode::Esc if !app.search_query.is_empty() => {
                                        app.search_query.clear();
                                        app.filter_games_by_search();
                                    }
                                    KeyCode::Char('?') => {
                                        app.update(Action::OpenCheatsheetModal).await;
                                    }
                                    KeyCode::Char('q') => {
                                        app.update(Action::Quit).await;
                                    }
                                    KeyCode::Char('a') => {
                                        app.update(Action::OpenAddGameModal).await;
                                    }
                                    KeyCode::Char('c') => {
                                        app.update(Action::OpenWineToolsMenu).await;
                                    }
                                    KeyCode::Char('e') => {
                                        if app.focused_pane == FocusedPane::Platforms {
                                            app.update(Action::OpenFolderManagerForPlatform)
                                                .await;
                                        } else {
                                            app.update(Action::OpenEditGameModal).await;
                                        }
                                    }
                                    KeyCode::Char('m') => {
                                        app.update(Action::OpenManageRunnersModal).await;
                                    }
                                    KeyCode::Char('w') => {
                                        app.update(Action::OpenVisualMediaModal).await;
                                    }
                                    KeyCode::Tab => {
                                        if app.is_big_picture {
                                            app.update(Action::ToggleBigPictureFocus).await;
                                        } else {
                                            app.update(Action::TogglePane).await;
                                        }
                                    }
                                    KeyCode::BackTab => {
                                        if app.is_big_picture {
                                            app.update(Action::PrevPlatform).await;
                                        }
                                    }
                                    KeyCode::Char('o') | KeyCode::Char('O')
                                        if key.modifiers.contains(KeyModifiers::ALT) =>
                                    {
                                        app.update(Action::ToggleBigPictureMode).await;
                                    }
                                    KeyCode::Right => {
                                        if app.is_big_picture {
                                            if app.big_picture_focus == BigPictureFocus::PlatformBar
                                            {
                                                app.update(Action::NextPlatform).await;
                                            } else if app.selected_game_idx + 1 < app.games.len() {
                                                app.selected_game_idx += 1;
                                                app.sync_platform_selection_with_game();
                                                app.trigger_async_cover_fetch();
                                            }
                                        } else if app.focused_pane == FocusedPane::Platforms {
                                            app.focused_pane = FocusedPane::Games;
                                        }
                                    }
                                    KeyCode::Left => {
                                        if app.is_big_picture {
                                            if app.big_picture_focus == BigPictureFocus::PlatformBar
                                            {
                                                app.update(Action::PrevPlatform).await;
                                            } else if app.selected_game_idx > 0 {
                                                app.selected_game_idx -= 1;
                                                app.sync_platform_selection_with_game();
                                                app.trigger_async_cover_fetch();
                                            }
                                        } else if app.focused_pane == FocusedPane::Games {
                                            app.focused_pane = FocusedPane::Platforms;
                                        }
                                    }
                                    KeyCode::Up => {
                                        if app.is_big_picture {
                                            app.big_picture_focus = BigPictureFocus::PlatformBar;
                                        } else {
                                            match app.focused_pane {
                                                FocusedPane::Platforms => {
                                                    if app.selected_platform_idx == 0 {
                                                        app.focused_pane = FocusedPane::Search;
                                                    } else {
                                                        app.update(Action::PrevPlatform).await;
                                                    }
                                                }
                                                FocusedPane::Games => {
                                                    if app.selected_game_idx == 0 {
                                                        app.focused_pane = FocusedPane::Search;
                                                    } else {
                                                        app.update(Action::PrevGame).await;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    KeyCode::Down => {
                                        if app.is_big_picture {
                                            app.big_picture_focus = BigPictureFocus::Carousel;
                                        } else {
                                            match app.focused_pane {
                                                FocusedPane::Platforms => {
                                                    app.update(Action::NextPlatform).await
                                                }
                                                FocusedPane::Games => {
                                                    app.update(Action::NextGame).await
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    KeyCode::Enter => {
                                        if app.is_big_picture {
                                            app.update(Action::OpenGameDetail).await;
                                        } else {
                                            app.update(Action::LaunchGame).await;
                                        }
                                        terminal.clear()?;
                                        terminal.draw(|f| ui::render_ui(f, &mut app))?;
                                    }
                                    KeyCode::Char('v') => {
                                        app.update(Action::ToggleViewMode).await;
                                    }
                                    KeyCode::Char('s') => {
                                        app.update(Action::OpenSettingsModal).await;
                                    }
                                    KeyCode::Char('p') | KeyCode::Char('P') => {
                                        if app.is_big_picture {
                                            app.update(Action::OpenPlatformSelectorModal).await;
                                        } else {
                                            app.update(Action::ToggleShowAllPlatforms).await;
                                        }
                                    }
                                    KeyCode::Char('r') => {
                                        app.update(Action::QuickRescanPlatform).await;
                                    }
                                    KeyCode::Char('g') => {
                                        app.update(Action::FetchGameMedia).await;
                                    }
                                    KeyCode::Char('f') => {
                                        app.update(Action::ForceCloseGame).await;
                                    }
                                    KeyCode::Char(' ') => {
                                        app.update(Action::ToggleSelectGame).await;
                                    }
                                    KeyCode::Delete => {
                                        app.update(Action::OpenConfirmDeleteModal).await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                    mouse_handler::handle_mouse_event(&mut app, mouse, area).await;
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup terminal on normal exit
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        cursor::Show
    )?;
    terminal.show_cursor()?;

    println!("TUI Game Station cerrado correctamente.");
    Ok(())
}

/// Initialize a file-based tracing subscriber so the game launch / TUI resume
/// flow (spawn PID, input flush, grace window, AppImage process tree) can be
/// diagnosed from `~/.cache/tui_game_station/logs/app.log`.
fn init_file_logging() {
    let logs_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("tui_game_station")
        .join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);

    let appender = tracing_appender::rolling::daily(&logs_dir, "app.log");
    let subscriber = tracing_subscriber::fmt()
        .with_writer(appender)
        .with_timer(tracing_subscriber::fmt::time::SystemTime)
        .with_target(true)
        .with_level(true)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}
