use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use crate::app::{Action, App, BigPictureFocus, FocusedPane, ModalState};
use crate::ui::centered_rect_exact;

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
    if let ModalState::WelcomeWizard { ref mut step, ref sgdb_api_key, .. } = app.modal_state {
        let footer_y = area.y + area.height.saturating_sub(4);
        if y >= footer_y {
            if x < area.x + area.width / 3 {
                if *step > 0 { *step -= 1; }
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

    if let ModalState::PlatformSelector { selected_idx } = app.modal_state {
        let max_name_len = app.platforms.iter().map(|p| p.name.len()).max().unwrap_or(12);
        let needed_w = (max_name_len as u16 + 26).clamp(42, 60).min(area.width.saturating_sub(4));
        let needed_h = (app.platforms.len() as u16 + 2).clamp(4, 16).min(area.height.saturating_sub(2));
        let popup_area = centered_rect_exact(needed_w, needed_h, area);

        if x >= popup_area.x && x < popup_area.x + popup_area.width &&
           y >= popup_area.y && y < popup_area.y + popup_area.height {
            let content_y = popup_area.y + 1; // skip top border
            if y >= content_y {
                let clicked_idx = (y - content_y) as usize;
                if clicked_idx < app.platforms.len() {
                    if clicked_idx == selected_idx {
                        app.update(Action::ConfirmPlatformSelectorModal).await;
                    } else {
                        app.modal_state = ModalState::PlatformSelector { selected_idx: clicked_idx };
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
