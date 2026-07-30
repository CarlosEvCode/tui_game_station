mod app;
mod cli;
mod cover_renderer;
mod mouse_handler;
mod panic_hook;
mod toast;
mod ui;
mod window_helper;

use anyhow::Result;
use app::{Action, App, BigPictureFocus, FocusedPane, ModalState};
use clap::Parser;
use game_core::models::PlatformType;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, KeyEventKind, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli_args = cli::CliArgs::parse();

    // Install terminal recovery panic hook
    panic_hook::init_panic_hook();

    // Enable Crossterm raw mode, alternate screen & mouse capture
    enable_raw_mode()?;
    let mut stdout_handle = stdout();
    execute!(stdout_handle, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend)?;

    // Initialize application state
    let mut app = App::new()?;
    app.apply_cli_args(&cli_args);

    // Main event loop
    loop {
        app.check_download_events().await;
        terminal.draw(|f| ui::render_ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                    // Modal Input Handling
                    if app.modal_state != ModalState::None {
                        match key.code {
                            KeyCode::Esc => {
                                if let ModalState::AppSettings { ref mut is_editing_api_key, .. } = app.modal_state {
                                    if *is_editing_api_key {
                                        *is_editing_api_key = false;
                                        continue;
                                    }
                                }
                                if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                    app.update(Action::ProtonDownloaderBack).await;
                                } else if let ModalState::WelcomeWizard { ref sgdb_api_key, .. } = app.modal_state {
                                    let key = sgdb_api_key.clone();
                                    app.finish_welcome_wizard(&key);
                                } else {
                                    app.update(Action::CloseModal).await;
                                }
                            }
                            KeyCode::BackTab => {
                                if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.update(Action::ModalPrevField).await;
                                } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    app.update(Action::ModalPrevField).await;
                                } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.update(Action::ModalPrevField).await;
                                } else {
                                    app.update(Action::ModalNextField).await;
                                }
                            }
                            KeyCode::Tab => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
                                    app.update(Action::SwitchVisualMediaTab).await;
                                } else if let ModalState::WelcomeWizard { ref mut step, .. } = app.modal_state {
                                    *step = (*step + 1) % 4;
                                } else if let ModalState::AppSettings { ref mut selected_field, .. } = app.modal_state {
                                    *selected_field = (*selected_field + 1) % 3;
                                } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.update(Action::ModalPrevField).await;
                                } else {
                                    app.update(Action::ModalNextField).await;
                                }
                            }
                            KeyCode::Up => match &mut app.modal_state {
                                ModalState::AppSettings { ref mut selected_field, ref mut is_editing_api_key, .. } => {
                                    *is_editing_api_key = false;
                                    if *selected_field > 0 { *selected_field -= 1; }
                                }
                                ModalState::WelcomeWizard { step: 3, ref mut active_field, .. } => {
                                    if *active_field > 0 { *active_field -= 1; }
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
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ScanFolderStep1Platform { .. }
                                | ModalState::ManageRunnersStep1Platform { .. }
                                | ModalState::ManageWineRunners { .. }
                                | ModalState::SelectWineRunnerPicker { .. }
                                | ModalState::PlatformSelector { .. }
                                | ModalState::WineToolsMenu { .. } => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                                ModalState::ManageRunnersStep2Config { ref mut selected_row, .. } => {
                                    *selected_row = 0;
                                }
                                _ => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                            },
                            KeyCode::Down => match &mut app.modal_state {
                                ModalState::AppSettings { ref mut selected_field, ref mut is_editing_api_key, .. } => {
                                    *is_editing_api_key = false;
                                    if *selected_field < 2 { *selected_field += 1; }
                                }
                                ModalState::WelcomeWizard { step: 3, ref mut active_field, .. } => {
                                    if *active_field < 1 { *active_field += 1; }
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
                                ModalState::ManageRunnersStep2Config { ref mut selected_row, .. } => {
                                    *selected_row = 1;
                                }
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ScanFolderStep1Platform { .. }
                                | ModalState::ManageRunnersStep1Platform { .. }
                                | ModalState::ManageWineRunners { .. }
                                | ModalState::SelectWineRunnerPicker { .. }
                                | ModalState::PlatformSelector { .. }
                                | ModalState::WineToolsMenu { .. } => {
                                    app.update(Action::ModalSelectNext).await;
                                }
                                _ => {
                                    app.update(Action::ModalNextField).await;
                                }
                            },
                            KeyCode::Left => match &mut app.modal_state {
                                ModalState::WelcomeWizard { step, ref mut cursor_pos, .. } => {
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
                                ModalState::AppSettings { selected_field: 0, is_editing_api_key: true, ref mut cursor_pos, .. } => {
                                    if *cursor_pos > 0 {
                                        *cursor_pos -= 1;
                                    }
                                }
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::VisualMediaNavLeft).await;
                                }
                                ModalState::ManageRunnersStep2Config { ref selected_row, ref mut selected_action_idx, ref mut cursor_pos, .. } => {
                                    if *selected_row == 0 {
                                        if *cursor_pos > 0 { *cursor_pos -= 1; }
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
                                _ => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                            },
                            KeyCode::Right => match &mut app.modal_state {
                                ModalState::WelcomeWizard { step, ref sgdb_api_key, ref mut cursor_pos, .. } => {
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
                                ModalState::AppSettings { selected_field: 0, is_editing_api_key: true, ref api_key_input, ref mut cursor_pos, .. } => {
                                    if *cursor_pos < api_key_input.len() {
                                        *cursor_pos += 1;
                                    }
                                }
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::VisualMediaNavRight).await;
                                }
                                ModalState::ManageRunnersStep2Config { ref runner_info, ref exe_path_input, ref selected_row, ref mut selected_action_idx, ref mut cursor_pos } => {
                                    if *selected_row == 0 {
                                        if *cursor_pos < exe_path_input.len() { *cursor_pos += 1; }
                                    } else {
                                        let is_downloaded = runner_info.executable_path.as_ref().map(|p| std::path::Path::new(p).exists()).unwrap_or(false);
                                        let mut total_btns = 3;
                                        if runner_info.download_url.is_some() { total_btns += 1; }
                                        if is_downloaded { total_btns += 1; }
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
                                _ => {
                                    app.update(Action::ModalSelectNext).await;
                                }
                            },
                            KeyCode::Home => {
                                if let ModalState::WelcomeWizard { step: 2, ref mut cursor_pos, .. } = app.modal_state {
                                    *cursor_pos = 0;
                                } else if let ModalState::AppSettings { selected_field: 0, is_editing_api_key: true, ref mut cursor_pos, .. } = app.modal_state {
                                    *cursor_pos = 0;
                                }
                            }
                            KeyCode::End => {
                                if let ModalState::WelcomeWizard { step: 2, ref sgdb_api_key, ref mut cursor_pos, .. } = app.modal_state {
                                    *cursor_pos = sgdb_api_key.len();
                                } else if let ModalState::AppSettings { selected_field: 0, is_editing_api_key: true, ref api_key_input, ref mut cursor_pos, .. } = app.modal_state {
                                    *cursor_pos = api_key_input.len();
                                }
                            }
                            KeyCode::Char('1') => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
                                    app.update(Action::SetVisualMediaTab(0)).await;
                                } else {
                                    app.update(Action::ModalInputChar('1')).await;
                                }
                            }
                            KeyCode::Char('2') => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
                                    app.update(Action::SetVisualMediaTab(1)).await;
                                } else {
                                    app.update(Action::ModalInputChar('2')).await;
                                }
                            }
                            KeyCode::Char('3') => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
                                    app.update(Action::SetVisualMediaTab(2)).await;
                                } else {
                                    app.update(Action::ModalInputChar('3')).await;
                                }
                            }
                            KeyCode::Char('4') => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
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
                                let is_form = matches!(app.modal_state,
                                    ModalState::AddGameForm { .. } | ModalState::EditGameForm { .. });
                                if is_form {
                                    let selected_field = match &app.modal_state {
                                        ModalState::AddGameForm { selected_field, .. } | ModalState::EditGameForm { selected_field, .. } => *selected_field,
                                        _ => 0,
                                    };
                                    let game_type = match &app.modal_state {
                                        ModalState::AddGameForm { game_type, .. } | ModalState::EditGameForm { game_type, .. } => game_type,
                                        _ => &PlatformType::Native,
                                    };
                                    let on_checkbox = match game_type {
                                        PlatformType::Wine => selected_field >= 6 && selected_field <= 12,
                                        PlatformType::Native => selected_field >= 4 && selected_field <= 6,
                                        PlatformType::Emulator | PlatformType::Steam =>
                                            selected_field >= 3 && selected_field <= 5,
                                    };
                                    if on_checkbox {
                                        app.update(Action::ModalToggleCheckbox).await;
                                    } else {
                                        app.update(Action::ModalInputChar(' ')).await;
                                    }
                                } else if let ModalState::ScanFolderForm { .. } = app.modal_state {
                                    app.update(Action::ModalToggleCheckbox).await;
                                } else if let ModalState::VisualMediaSelector { active_tab, .. } = app.modal_state {
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
                                ModalState::AppSettings { selected_field, ref mut is_editing_api_key, ref api_key_input, ref mut cursor_pos } => {
                                    if selected_field == 0 {
                                        *is_editing_api_key = !*is_editing_api_key;
                                        if *is_editing_api_key {
                                            *cursor_pos = api_key_input.len();
                                        }
                                    } else if selected_field == 1 {
                                        app.update(Action::OpenWelcomeWizardModal).await;
                                    } else if selected_field == 2 {
                                        app.update(Action::SaveAppSettings).await;
                                    }
                                }
                                ModalState::WelcomeWizard { step, ref sgdb_api_key, .. } => {
                                    if step < 3 {
                                        if let ModalState::WelcomeWizard { ref mut step, .. } = app.modal_state {
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
                                    if focused_section == 1 || (focused_section == 0 && active_tab == 0) {
                                        app.update(Action::SearchVisualMedia).await;
                                    } else if active_tab == 0 {
                                        if candidates.is_empty() {
                                            app.update(Action::SearchVisualMedia).await;
                                        } else {
                                            app.update(Action::SelectVisualMediaCandidate).await;
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
                                                execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
                                                let mut child = std::process::Command::new(&cmd.exe)
                                                    .args(&cmd.args)
                                                    .envs(&cmd.envs)
                                                    .spawn()
                                                    .ok();
                                                if let Some(ref mut c) = child {
                                                    let _ = c.wait();
                                                }
                                                enable_raw_mode()?;
                                                execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
                                                terminal.clear()?;
                                                terminal.draw(|f| ui::render_ui(f, &mut app))?;
                                            }
                                            "wineserver" => {
                                                let _ = std::process::Command::new(&cmd.exe)
                                                    .args(&cmd.args)
                                                    .envs(&cmd.envs)
                                                    .output();
                                            }
                                            "xdg-open" => {
                                                let path = cmd.args.first().map(|s| s.as_str()).unwrap_or("");
                                                if std::path::Path::new(path).exists() {
                                                    let _ = std::process::Command::new(&cmd.exe)
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
                                ModalState::PlatformSelector { .. } => {
                                    app.update(Action::ConfirmPlatformSelectorModal).await;
                                }
                                ModalState::AddGameForm {
                                    ref game_type,
                                    selected_field,
                                    ..
                                } => {
                                    let on_checkbox = match game_type {
                                        PlatformType::Wine => selected_field >= 6 && selected_field <= 12,
                                        PlatformType::Native => selected_field >= 4 && selected_field <= 6,
                                        PlatformType::Emulator | PlatformType::Steam =>
                                            selected_field >= 3 && selected_field <= 5,
                                    };
                                    if on_checkbox {
                                        app.update(Action::ModalToggleCheckbox).await;
                                    } else {
                                        match game_type {
                                            PlatformType::Wine => match selected_field {
                                                1 | 2 | 3 => app.update(Action::OpenFilePicker).await,
                                                4 => app.update(Action::OpenWineRunnerPicker).await,
                                                5 => app.update(Action::OpenCustomArgsEditor).await,
                                                13 => app.update(Action::SaveModalGame).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Native => match selected_field {
                                                1 | 2 => app.update(Action::OpenFilePicker).await,
                                                3 => app.update(Action::OpenCustomArgsEditor).await,
                                                7 => app.update(Action::SaveModalGame).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Emulator => match selected_field {
                                                1 => app.update(Action::OpenFilePicker).await,
                                                2 => app.update(Action::OpenCustomArgsEditor).await,
                                                6 => app.update(Action::SaveModalGame).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Steam => match selected_field {
                                                2 => app.update(Action::OpenCustomArgsEditor).await,
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
                                        PlatformType::Wine => selected_field >= 6 && selected_field <= 12,
                                        PlatformType::Native => selected_field >= 4 && selected_field <= 6,
                                        PlatformType::Emulator | PlatformType::Steam =>
                                            selected_field >= 3 && selected_field <= 5,
                                    };
                                    if on_checkbox {
                                        app.update(Action::ModalToggleCheckbox).await;
                                    } else {
                                        match game_type {
                                            PlatformType::Wine => match selected_field {
                                                1 | 2 | 3 => app.update(Action::OpenFilePicker).await,
                                                4 => app.update(Action::OpenWineRunnerPicker).await,
                                                5 => app.update(Action::OpenCustomArgsEditor).await,
                                                13 => app.update(Action::SaveEditGameModal).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Native => match selected_field {
                                                1 | 2 => app.update(Action::OpenFilePicker).await,
                                                3 => app.update(Action::OpenCustomArgsEditor).await,
                                                7 => app.update(Action::SaveEditGameModal).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Emulator => match selected_field {
                                                1 => app.update(Action::OpenFilePicker).await,
                                                2 => app.update(Action::OpenCustomArgsEditor).await,
                                                6 => app.update(Action::SaveEditGameModal).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                            PlatformType::Steam => match selected_field {
                                                2 => app.update(Action::OpenCustomArgsEditor).await,
                                                6 => app.update(Action::SaveEditGameModal).await,
                                                _ => app.update(Action::ModalNextField).await,
                                            },
                                        }
                                    }
                                }
                                ModalState::ScanFolderForm { selected_field, .. } => match selected_field {
                                    0 => app.update(Action::OpenFilePicker).await,
                                    2 => app.update(Action::ModalToggleCheckbox).await,
                                    3 => app.update(Action::StartFolderScan).await,
                                    _ => app.update(Action::ModalNextField).await,
                                },
                                ModalState::ManageRunnersStep2Config { ref runner_info, ref exe_path_input, selected_row, selected_action_idx, .. } => {
                                    if selected_row == 0 {
                                        if exe_path_input.trim().is_empty() {
                                            app.update(Action::OpenFilePicker).await;
                                        } else if let ModalState::ManageRunnersStep2Config { ref mut selected_row, .. } = app.modal_state {
                                            *selected_row = 1;
                                        }
                                    } else {
                                        let is_downloaded = runner_info.executable_path.as_ref().map(|p| std::path::Path::new(p).exists()).unwrap_or(false);
                                        let mut actions = vec!["browse"];
                                        if runner_info.download_url.is_some() { actions.push("download"); }
                                        actions.push("save");
                                        if is_downloaded { actions.push("delete"); }
                                        actions.push("deactivate");

                                        let act = actions.get(selected_action_idx).copied().unwrap_or("save");
                                        match act {
                                            "browse" => app.update(Action::OpenFilePicker).await,
                                            "download" => app.update(Action::StartRunnerDownload).await,
                                            "save" => app.update(Action::SaveRunnerConfig).await,
                                            "delete" => app.update(Action::DeleteRunnerDownload).await,
                                            "deactivate" => app.update(Action::ResetRunnerConfig).await,
                                            _ => {}
                                        }
                                    }
                                },
                                _ => {
                                    app.update(Action::SaveModalGame).await;
                                }
                            },
                            KeyCode::Backspace => {
                                if let ModalState::WelcomeWizard { step: 2, ref mut sgdb_api_key, ref mut cursor_pos, .. } = app.modal_state {
                                    if *cursor_pos > 0 && !sgdb_api_key.is_empty() {
                                        sgdb_api_key.remove(*cursor_pos - 1);
                                        *cursor_pos -= 1;
                                    }
                                } else if let ModalState::FuzzySearchModal { ref mut query, ref mut cursor_pos } = app.modal_state {
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
                                if let ModalState::WelcomeWizard { step: 2, ref mut sgdb_api_key, cursor_pos, .. } = app.modal_state {
                                    if cursor_pos < sgdb_api_key.len() {
                                        sgdb_api_key.remove(cursor_pos);
                                    }
                                } else if let ModalState::AppSettings { selected_field: 0, is_editing_api_key: true, ref mut api_key_input, cursor_pos, .. } = app.modal_state {
                                    if cursor_pos < api_key_input.len() {
                                        api_key_input.remove(cursor_pos);
                                    }
                                } else if let ModalState::ManageWineRunners { .. } = app.modal_state {
                                    app.update(Action::DeleteInstalledWineRunner).await;
                                }
                            }
                            KeyCode::Char('v') | KeyCode::Char('V') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if let ModalState::WelcomeWizard { step: 2, ref mut sgdb_api_key, ref mut cursor_pos, .. } = app.modal_state {
                                    if let Some(pasted) = crate::app::get_clipboard_text() {
                                        sgdb_api_key.insert_str(*cursor_pos, &pasted);
                                        *cursor_pos += pasted.len();
                                    }
                                } else if let ModalState::AppSettings { selected_field: 0, ref mut is_editing_api_key, ref mut api_key_input, ref mut cursor_pos } = app.modal_state {
                                    if let Some(pasted) = crate::app::get_clipboard_text() {
                                        *is_editing_api_key = true;
                                        let pos = (*cursor_pos).min(api_key_input.len());
                                        api_key_input.insert_str(pos, &pasted);
                                        *cursor_pos = pos + pasted.len();
                                    }
                                } else if let ModalState::ConfigureApiKeyInput { ref mut input } = app.modal_state {
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
                            KeyCode::Char('p') => {
                                match app.modal_state {
                                    ModalState::AddGameForm { game_type: PlatformType::Wine, selected_field: 4, .. }
                                    | ModalState::EditGameForm { game_type: PlatformType::Wine, selected_field: 4, .. } => {
                                        app.update(Action::OpenWineRunnerPicker).await;
                                    }
                                    ModalState::AddGameForm { game_type: PlatformType::Wine, selected_field: 5, .. }
                                    | ModalState::EditGameForm { game_type: PlatformType::Wine, selected_field: 5, .. } => {
                                        app.update(Action::OpenCustomArgsEditor).await;
                                    }
                                    ModalState::ManageRunnersStep1Platform { .. } => {
                                        app.update(Action::OpenWineRunnerManager).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalInputChar('p')).await;
                                    }
                                }
                            }
                            KeyCode::Char('t') => {
                                if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                    app.update(Action::ProtonDownloaderSelectNext).await;
                                } else {
                                    app.update(Action::ModalInputChar('t')).await;
                                }
                            }
                            KeyCode::Char('w') => {
                                if let ModalState::ManageRunnersStep1Platform { .. } = app.modal_state {
                                    app.update(Action::OpenWineRunnerManager).await;
                                } else {
                                    app.update(Action::ModalInputChar('w')).await;
                                }
                            }
                            KeyCode::Char('x') => {
                                app.update(Action::ModalInputChar('x')).await;
                            }
                            KeyCode::Char('f') => {
                                app.update(Action::ModalInputChar('f')).await;
                            }
                            KeyCode::Char(c) => {
                                if let ModalState::WelcomeWizard { step: 2, ref mut sgdb_api_key, ref mut cursor_pos, .. } = app.modal_state {
                                    sgdb_api_key.insert(*cursor_pos, c);
                                    *cursor_pos += 1;
                                } else if let ModalState::FuzzySearchModal { ref mut query, ref mut cursor_pos } = app.modal_state {
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
                                KeyCode::Char('o') | KeyCode::Char('O') if key.modifiers.contains(KeyModifiers::ALT) => {
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
                                    app.update(Action::OpenEditGameModal).await;
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
                                KeyCode::Char('o') | KeyCode::Char('O') if key.modifiers.contains(KeyModifiers::ALT) => {
                                    app.update(Action::ToggleBigPictureMode).await;
                                }
                                KeyCode::Right => {
                                    if app.is_big_picture {
                                        if app.big_picture_focus == BigPictureFocus::PlatformBar {
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
                                        if app.big_picture_focus == BigPictureFocus::PlatformBar {
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
                                            FocusedPane::Platforms => app.update(Action::NextPlatform).await,
                                            FocusedPane::Games => app.update(Action::NextGame).await,
                                            _ => {}
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    window_helper::minimize_active_window();

                                    disable_raw_mode()?;
                                    execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture, cursor::Show)?;

                                    app.update(Action::LaunchGame).await;

                                    window_helper::restore_active_window();

                                    enable_raw_mode()?;
                                    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, cursor::Show)?;
    terminal.show_cursor()?;

    println!("TUI Game Station cerrado correctamente.");
    Ok(())
}
