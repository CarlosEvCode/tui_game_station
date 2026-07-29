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
                                app.update(Action::CloseModal).await;
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
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ScanFolderStep1Platform { .. }
                                | ModalState::ManageRunnersStep1Platform { .. }
                                | ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                                _ => {
                                    app.update(Action::ModalPrevField).await;
                                }
                            },
                            KeyCode::Down => match app.modal_state {
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ScanFolderStep1Platform { .. }
                                | ModalState::ManageRunnersStep1Platform { .. }
                                | ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::ModalSelectNext).await;
                                }
                                _ => {
                                    app.update(Action::ModalNextField).await;
                                }
                            },
                            KeyCode::Left => match app.modal_state {
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::SwitchVisualMediaTab).await;
                                }
                                _ => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                            },
                            KeyCode::Right => match app.modal_state {
                                ModalState::VisualMediaSelector { .. } => {
                                    app.update(Action::SwitchVisualMediaTab).await;
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
                            KeyCode::Char(' ') => {
                                if let ModalState::ScanFolderForm { .. } = app.modal_state {
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
                                ModalState::ScanFolderForm { .. } => {
                                    app.update(Action::StartFolderScan).await;
                                }
                                ModalState::ConfigureApiKeyInput { .. } => {
                                    app.update(Action::SaveApiKey).await;
                                }
                                ModalState::AppSettings { .. } => {
                                    app.update(Action::SaveAppSettings).await;
                                }
                                ModalState::VisualMediaSelector {
                                    active_tab,
                                    ref candidates,
                                    ..
                                } => {
                                    if active_tab == 0 {
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
                                ModalState::ManageRunnersStep2Config { .. } => {
                                    app.update(Action::SaveRunnerConfig).await;
                                }
                                ModalState::EditGameForm { .. } => {
                                    app.update(Action::SaveEditGameModal).await;
                                }
                                _ => {
                                    app.update(Action::SaveModalGame).await;
                                }
                            },
                            KeyCode::Backspace => {
                                app.update(Action::ModalBackspace).await;
                            }
                            KeyCode::Char('d') => {
                                if let ModalState::ManageRunnersStep2Config { .. } = app.modal_state {
                                    app.update(Action::ResetRunnerConfig).await;
                                } else {
                                    app.update(Action::ModalInputChar('d')).await;
                                }
                            }
                            KeyCode::Char('w') => {
                                if let ModalState::ManageRunnersStep2Config { .. } = app.modal_state {
                                    app.update(Action::StartRunnerDownload).await;
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
                                    ModalState::AddGameForm { selected_field, game_type, .. } => {
                                        match (game_type, *selected_field) {
                                            (PlatformType::Emulator, 2) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Native, 1) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Native, 2) => app.update(Action::OpenFolderPicker).await,
                                            (PlatformType::Wine, 1) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Wine, 2) => app.update(Action::OpenFolderPicker).await,
                                            (PlatformType::Wine, 3) => app.update(Action::OpenFolderPicker).await,
                                            _ => app.update(Action::ModalInputChar('f')).await,
                                        }
                                    }
                                    ModalState::EditGameForm { selected_field, game_type, .. } => {
                                        match (game_type, *selected_field) {
                                            (PlatformType::Emulator, 1) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Native, 1) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Native, 2) => app.update(Action::OpenFolderPicker).await,
                                            (PlatformType::Wine, 1) => app.update(Action::OpenFilePicker).await,
                                            (PlatformType::Wine, 2) => app.update(Action::OpenFolderPicker).await,
                                            (PlatformType::Wine, 3) => app.update(Action::OpenFolderPicker).await,
                                            _ => app.update(Action::ModalInputChar('f')).await,
                                        }
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
                            KeyCode::Delete | KeyCode::Char('x') => {
                                app.update(Action::DeleteSelectedGames).await;
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
