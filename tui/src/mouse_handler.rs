use crate::app::{Action, App, BigPictureFocus, FocusedPane, ModalState};
use crate::ui::centered_rect_exact;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

/// Process incoming mouse events (scroll wheel, left clicks, pane focus, card selection)
pub async fn handle_mouse_event(app: &mut App, mouse: MouseEvent, area: Rect) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            if app.modal_state != ModalState::None {
                app.update(Action::ModalSelectPrev).await;
            } else if app.is_big_picture {
                if app.big_picture_focus == BigPictureFocus::PlatformBar {
                    app.update(Action::PrevPlatform).await;
                } else {
                    app.update(Action::PrevGame).await;
                }
            } else {
                match app.focused_pane {
                    FocusedPane::Platforms => app.update(Action::PrevPlatform).await,
                    FocusedPane::Games => app.update(Action::PrevGame).await,
                    FocusedPane::Search => {}
                }
            }
        }
        MouseEventKind::ScrollDown => {
            if app.modal_state != ModalState::None {
                app.update(Action::ModalSelectNext).await;
            } else if app.is_big_picture {
                if app.big_picture_focus == BigPictureFocus::PlatformBar {
                    app.update(Action::NextPlatform).await;
                } else {
                    app.update(Action::NextGame).await;
                }
            } else {
                match app.focused_pane {
                    FocusedPane::Platforms => app.update(Action::NextPlatform).await,
                    FocusedPane::Games => app.update(Action::NextGame).await,
                    FocusedPane::Search => {}
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let x = mouse.column;
            let y = mouse.row;

            if app.modal_state != ModalState::None {
                handle_modal_click(app, x, y, area).await;
            } else if app.is_big_picture {
                handle_big_picture_click(app, x, y, area).await;
            } else {
                handle_library_click(app, x, y, area).await;
            }
        }
        _ => {}
    }
}

async fn handle_modal_click(app: &mut App, x: u16, y: u16, area: Rect) {
    if let ModalState::WelcomeWizard {
        ref mut step,
        ref sgdb_api_key,
        ..
    } = app.modal_state
    {
        let footer_y = area.y + area.height.saturating_sub(4);
        if y >= footer_y {
            if x < area.x + area.width / 3 {
                if *step > 0 {
                    *step -= 1;
                }
            } else if x > area.x + (area.width * 2) / 3 {
                if *step < 3 {
                    *step += 1;
                } else {
                    let key = sgdb_api_key.clone();
                    app.finish_welcome_wizard(&key);
                }
            }
        } else if *step == 3 {
            let key = sgdb_api_key.clone();
            app.finish_welcome_wizard(&key);
        }
        return;
    }

    if let ModalState::ManageRunnersStep2Config {
        ref runner_info,
        ref exe_path_input,
        ref mut selected_row,
        ref mut selected_action_idx,
        ref mut option_values,
        ref options,
        ..
    } = app.modal_state
    {
        let popup_area = crate::ui::runner_step2_popup_area(options.len(), area);
        if x >= popup_area.x
            && x < popup_area.x + popup_area.width
            && y >= popup_area.y
            && y < popup_area.y + popup_area.height
        {
            let n = options.len();
            let rel_in = y.saturating_sub(popup_area.y + 1);
            let custom_line = if n == 0 { 5 } else { 6 + n };
            let buttons_line = custom_line + 2;

            if rel_in <= 3 {
                *selected_row = 0;
            } else if n > 0 && rel_in >= 6 && usize::from(rel_in) < 6 + n {
                let opt_idx = usize::from(rel_in) - 5;
                *selected_row = opt_idx + 1;
                crate::app::cycle_runner_option(options, option_values, opt_idx, false);
            } else if usize::from(rel_in) == custom_line {
                *selected_row = n + 1;
                app.update(Action::OpenCustomArgsEditor).await;
            } else if usize::from(rel_in) >= buttons_line {
                *selected_row = n + 2;
                let rel_x = x.saturating_sub(popup_area.x + 2);
                let w = popup_area.width.saturating_sub(4);
                let has_executable = !exe_path_input.trim().is_empty()
                    && (runner::is_executable_command(exe_path_input.trim())
                        || std::path::Path::new(exe_path_input.trim()).exists());
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

                let step_w = w / (actions.len() as u16).max(1);
                let clicked_action = ((rel_x / step_w.max(1)) as usize).min(actions.len() - 1);
                *selected_action_idx = clicked_action;

                let act = actions.get(clicked_action).copied().unwrap_or("save");
                match act {
                    "browse" => app.update(Action::OpenFilePicker).await,
                    "download" => app.update(Action::StartRunnerDownload).await,
                    "save" => app.update(Action::SaveRunnerConfig).await,
                    "open" => app.update(Action::OpenRunnerStandalone).await,
                    "toggle_active" => app.update(Action::ToggleRunnerActiveState).await,
                    "delete" => app.update(Action::OpenConfirmDeleteRunnerModal).await,
                    _ => {}
                }
            }
        }
        return;
    }

    if let ModalState::AppSettings {
        ref mut selected_field,
        ref mut is_editing_api_key,
        ref api_key_input,
        ref mut cursor_pos,
    } = app.modal_state
    {
        let popup_area = centered_rect_exact(60, 10, area);
        if x >= popup_area.x
            && x < popup_area.x + popup_area.width
            && y >= popup_area.y
            && y < popup_area.y + popup_area.height
        {
            let rel_y = y.saturating_sub(popup_area.y + 1);
            if rel_y <= 2 {
                *selected_field = 0;
                *is_editing_api_key = !*is_editing_api_key;
                if *is_editing_api_key {
                    *cursor_pos = api_key_input.len();
                }
            } else if rel_y == 3 || rel_y == 4 {
                *selected_field = 1;
                *is_editing_api_key = false;
                app.update(Action::OpenWelcomeWizardModal).await;
            } else if rel_y >= 5 {
                *selected_field = 2;
                *is_editing_api_key = false;
                app.update(Action::SaveAppSettings).await;
            }
        }
        return;
    }

    if let ModalState::PlatformSelector { selected_idx } = app.modal_state {
        let max_name_len = app
            .platforms
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(12);
        let needed_w = (max_name_len as u16 + 26)
            .clamp(42, 60)
            .min(area.width.saturating_sub(4));
        let needed_h = (app.platforms.len() as u16 + 2)
            .clamp(4, 16)
            .min(area.height.saturating_sub(2));
        let popup_area = centered_rect_exact(needed_w, needed_h, area);

        if x >= popup_area.x
            && x < popup_area.x + popup_area.width
            && y >= popup_area.y
            && y < popup_area.y + popup_area.height
        {
            let content_y = popup_area.y + 1; // skip top border
            if y >= content_y {
                let clicked_idx = (y - content_y) as usize;
                if clicked_idx < app.platforms.len() {
                    if clicked_idx == selected_idx {
                        app.update(Action::ConfirmPlatformSelectorModal).await;
                    } else {
                        app.modal_state = ModalState::PlatformSelector {
                            selected_idx: clicked_idx,
                        };
                    }
                }
            }
        } else {
            app.update(Action::CloseModal).await;
        }
    }
}

async fn handle_big_picture_click(app: &mut App, x: u16, y: u16, area: Rect) {
    let top_banner_h = 3u16;
    let footer_h = 3u16;

    if y < top_banner_h {
        let search_w = (area.width * 30) / 100;
        if x < search_w {
            app.big_picture_focus = BigPictureFocus::Search;
        } else {
            app.big_picture_focus = BigPictureFocus::PlatformBar;
            if x < search_w + (area.width - search_w) / 3 {
                app.update(Action::PrevPlatform).await;
            } else if x > search_w + ((area.width - search_w) * 2) / 3 {
                app.update(Action::NextPlatform).await;
            }
        }
        return;
    }

    if y >= area.height.saturating_sub(footer_h) {
        return;
    }

    app.big_picture_focus = BigPictureFocus::Carousel;
    let col_w = area.width / 3;

    if x < col_w {
        app.update(Action::PrevGame).await;
    } else if x > col_w * 2 {
        app.update(Action::NextGame).await;
    } else {
        app.trigger_async_cover_fetch();
    }
}

async fn handle_library_click(app: &mut App, x: u16, y: u16, area: Rect) {
    let header_h = 3u16;
    let status_h = 3u16;
    let footer_h = 3u16;

    if y < header_h {
        app.focused_pane = FocusedPane::Search;
        return;
    }

    if y >= area.height.saturating_sub(status_h + footer_h) {
        return;
    }

    let left_w = (area.width * 25) / 100;

    if x < left_w {
        app.focused_pane = FocusedPane::Platforms;
        let content_y = header_h + 1;
        if y >= content_y {
            let clicked_idx = (y - content_y) as usize;
            if clicked_idx < app.platforms.len() {
                app.selected_platform_idx = clicked_idx;
                app.load_games_for_selected_platform();
            }
        }
    } else {
        app.focused_pane = FocusedPane::Games;
        let content_y = header_h + 1;
        if y >= content_y {
            let clicked_idx = (y - content_y) as usize;
            if clicked_idx < app.games.len() {
                app.selected_game_idx = clicked_idx;
                app.trigger_async_cover_fetch();
            }
        }
    }
}
