mod app;
mod cover_renderer;
mod panic_hook;
mod ui;

use anyhow::Result;
use app::{Action, App, FocusedPane, ModalState};
use game_core::models::PlatformType;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Install terminal recovery panic hook
    panic_hook::init_panic_hook();

    // Enable Crossterm raw mode & alternate screen
    enable_raw_mode()?;
    let mut stdout_handle = stdout();
    execute!(stdout_handle, EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend)?;

    // Initialize application state
    let mut app = App::new()?;

    // Main event loop
    loop {
        app.check_download_events().await;
        terminal.draw(|f| ui::render_ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Modal Input Handling
                    if app.modal_state != ModalState::None {
                        match key.code {
                            KeyCode::Esc => {
                                if let ModalState::ProtonDownloader { .. } = app.modal_state {
                                    app.update(Action::ProtonDownloaderBack).await;
                                } else {
                                    app.update(Action::CloseModal).await;
                                }
                            }
                            KeyCode::BackTab => {
                                app.update(Action::ModalPrevField).await;
                            }
                            KeyCode::Tab => {
                                if let ModalState::VisualMediaSelector { .. } = app.modal_state {
                                    app.update(Action::SwitchVisualMediaTab).await;
                                } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.update(Action::ModalPrevField).await;
                                } else {
                                    app.update(Action::ModalNextField).await;
                                }
                            }
                            KeyCode::Up => match app.modal_state {
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
                                | ModalState::WineToolsMenu { .. } => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                                _ => {
                                    app.update(Action::ModalPrevField).await;
                                }
                            },
                            KeyCode::Down => match app.modal_state {
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::VisualMediaNavDown).await;
                                }
                                ModalState::ProtonDownloader { .. } => {
                                    app.update(Action::ProtonDownloaderSelectNext).await;
                                }
                                ModalState::ConfirmDeleteGame { .. } => {
                                    app.update(Action::ToggleConfirmDeleteOption).await;
                                }
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ScanFolderStep1Platform { .. }
                                | ModalState::ManageRunnersStep1Platform { .. }
                                | ModalState::ManageWineRunners { .. }
                                | ModalState::SelectWineRunnerPicker { .. }
                                | ModalState::WineToolsMenu { .. } => {
                                    app.update(Action::ModalSelectNext).await;
                                }
                                _ => {
                                    app.update(Action::ModalNextField).await;
                                }
                            },
                            KeyCode::Left => match app.modal_state {
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::VisualMediaNavLeft).await;
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
                            KeyCode::Right => match app.modal_state {
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::VisualMediaNavRight).await;
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
                                ModalState::AppSettings { .. } => {
                                    app.update(Action::SaveAppSettings).await;
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
                                ModalState::ManageRunnersStep2Config { .. } => {
                                    app.update(Action::SaveRunnerConfig).await;
                                }
                                _ => {
                                    app.update(Action::SaveModalGame).await;
                                }
                            },
                            KeyCode::Backspace => {
                                app.update(Action::ModalBackspace).await;
                            }
                            KeyCode::Delete => {
                                if let ModalState::ManageWineRunners { .. } = app.modal_state {
                                    app.update(Action::DeleteInstalledWineRunner).await;
                                }
                            }
                            KeyCode::Char('d') => {
                                match app.modal_state {
                                    ModalState::ManageRunnersStep2Config { .. } => {
                                        app.update(Action::ResetRunnerConfig).await;
                                    }
                                    ModalState::ManageWineRunners { .. } => {
                                        app.update(Action::OpenProtonDownloader).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalInputChar('d')).await;
                                    }
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
                                if let ModalState::ManageRunnersStep2Config { .. } = app.modal_state {
                                    app.update(Action::StartRunnerDownload).await;
                                } else if let ModalState::ManageRunnersStep1Platform { .. } = app.modal_state {
                                    app.update(Action::OpenWineRunnerManager).await;
                                } else {
                                    app.update(Action::ModalInputChar('w')).await;
                                }
                            }
                            KeyCode::Char('x') => {
                                if let ModalState::ManageRunnersStep2Config { .. } = app.modal_state {
                                    app.update(Action::DeleteRunnerDownload).await;
                                } else {
                                    app.update(Action::ModalInputChar('x')).await;
                                }
                            }
                            KeyCode::Char('f') => {
                                match &app.modal_state {
                                    ModalState::ScanFolderForm { selected_field: 0, .. } => {
                                        app.update(Action::OpenFolderPicker).await;
                                    }
                                    ModalState::ManageRunnersStep2Config { .. } => {
                                        app.update(Action::OpenFilePicker).await;
                                    }
                                    _ => {
                                        app.update(Action::ModalInputChar('f')).await;
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                app.update(Action::ModalInputChar(c)).await;
                            }
                            _ => {}
                        }
                    } else {
                        // Main View Keyboard Shortcuts
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
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
                                app.update(Action::TogglePane).await;
                            }
                            KeyCode::Right => {
                                if app.focused_pane == FocusedPane::Platforms {
                                    app.focused_pane = FocusedPane::Games;
                                }
                            }
                            KeyCode::Left => {
                                if app.focused_pane == FocusedPane::Games {
                                    app.focused_pane = FocusedPane::Platforms;
                                }
                            }
                            KeyCode::Up => match app.focused_pane {
                                FocusedPane::Platforms => app.update(Action::PrevPlatform).await,
                                FocusedPane::Games => app.update(Action::PrevGame).await,
                            },
                            KeyCode::Down => match app.focused_pane {
                                FocusedPane::Platforms => app.update(Action::NextPlatform).await,
                                FocusedPane::Games => app.update(Action::NextGame).await,
                            },
                            KeyCode::Enter => {
                                // 1. Cleanly suspend TUI & leave alternate screen
                                disable_raw_mode()?;
                                execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;

                                // 2. Launch game (stdout & stderr isolated to log file)
                                app.update(Action::LaunchGame).await;

                                // 3. Cleanly restore TUI canvas & re-enter alternate screen
                                enable_raw_mode()?;
                                execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
                                terminal.clear()?;
                                terminal.draw(|f| ui::render_ui(f, &mut app))?;
                            }
                            KeyCode::Char('v') => {
                                app.update(Action::ToggleViewMode).await;
                            }
                            KeyCode::Char('s') => {
                                app.update(Action::OpenSettingsModal).await;
                            }
                            KeyCode::Char('p') => {
                                app.update(Action::ToggleShowAllPlatforms).await;
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

        if app.should_quit {
            break;
        }
    }

    // Cleanup terminal on normal exit
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;
    terminal.show_cursor()?;

    println!("TUI Game Station cerrado correctamente.");
    Ok(())
}
