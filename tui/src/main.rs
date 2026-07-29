mod app;
mod panic_hook;
mod ui;

use anyhow::Result;
use app::{Action, App, FocusedPane, ModalState};
use crossterm::{
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
    execute!(stdout_handle, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout_handle);
    let mut terminal = Terminal::new(backend)?;

    // Initialize application state
    let mut app = App::new()?;

    // Main event loop
    loop {
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
                                if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    app.update(Action::ModalPrevField).await;
                                } else {
                                    app.update(Action::ModalNextField).await;
                                }
                            }
                            KeyCode::Up => match app.modal_state {
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ManageRunnersStep1Platform { .. } => {
                                    app.update(Action::ModalSelectPrev).await;
                                }
                                _ => {
                                    app.update(Action::ModalPrevField).await;
                                }
                            },
                            KeyCode::Down => match app.modal_state {
                                ModalState::AddGameStep1Type { .. }
                                | ModalState::ManageRunnersStep1Platform { .. } => {
                                    app.update(Action::ModalSelectNext).await;
                                }
                                _ => {
                                    app.update(Action::ModalNextField).await;
                                }
                            },
                            KeyCode::Left => {
                                app.update(Action::ModalSelectPrev).await;
                            }
                            KeyCode::Right => {
                                app.update(Action::ModalSelectNext).await;
                            }
                            KeyCode::Enter => match app.modal_state {
                                ModalState::AddGameStep1Type { .. } => {
                                    app.update(Action::ModalConfirmStep1).await;
                                }
                                ModalState::ManageRunnersStep1Platform { .. } => {
                                    app.update(Action::RunnerModalConfirmPlatform).await;
                                }
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
                            KeyCode::Char('f') => {
                                app.update(Action::OpenFilePicker).await;
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
                            KeyCode::Char('m') => {
                                app.update(Action::OpenManageRunnersModal).await;
                            }
                            KeyCode::Tab => {
                                app.update(Action::TogglePane).await;
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
                                disable_raw_mode()?;
                                execute!(stdout(), LeaveAlternateScreen)?;

                                app.update(Action::LaunchGame).await;

                                enable_raw_mode()?;
                                execute!(stdout(), EnterAlternateScreen)?;
                                terminal.clear()?;
                            }
                            KeyCode::Char('s') => {
                                app.update(Action::ScanCurrentFolder).await;
                            }
                            KeyCode::Char('p') => {
                                app.update(Action::ToggleShowAllPlatforms).await;
                            }
                            KeyCode::Char('r') => {
                                app.update(Action::ScanSteamGames).await;
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!("TUI Game Station cerrado correctamente.");
    Ok(())
}
