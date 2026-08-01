use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use ratatui_image::StatefulImage;

use crate::app::{App, BigPictureFocus, FocusedPane, ModalState, ViewMode};
use game_core::models::PlatformType;

pub fn render_ui(frame: &mut Frame, app: &mut App) {
    if app.is_big_picture {
        render_big_picture_mode(frame, app, frame.area());
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(8),    // Content
                Constraint::Length(3), // Activity & Status Bar (Log + Download Slider)
                Constraint::Length(3), // Shortcuts & Controls Footer
            ])
            .split(frame.area());

        render_header(frame, app, chunks[0]);
        render_main_content(frame, app, chunks[1]);
        render_activity_status_bar(frame, app, chunks[2]);
        render_controls_footer(frame, app, chunks[3]);
    }

    if app.modal_state != ModalState::None {
        render_modal(frame, app);
    }

    crate::toast::render_toasts(frame, &app.toasts, frame.area());
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let is_search_focused = app.focused_pane == FocusedPane::Search && app.modal_state == ModalState::None;
    let border_color = if is_search_focused {
        Color::Yellow
    } else if !app.search_query.is_empty() {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title_span = if is_search_focused {
        Span::styled(" Search (Typing...) ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" Search ", Style::default().fg(Color::DarkGray))
    };

    let search_content = if !app.search_query.is_empty() {
        let cursor = if is_search_focused { "█" } else { "" };
        vec![Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(&app.search_query, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(cursor, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  ({} games found across all platforms)", app.games.len()), Style::default().fg(Color::DarkGray)),
        ])]
    } else if is_search_focused {
        vec![Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" (Type to search across all platforms...)", Style::default().fg(Color::DarkGray)),
        ])]
    } else {
        vec![Line::from(vec![
            Span::styled("   [Type '/' or click to search games across all platforms...]", Style::default().fg(Color::DarkGray)),
        ])]
    };

    let search_p = Paragraph::new(search_content).block(
        Block::default()
            .title(title_span)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(search_p, area);
}

fn render_main_content(frame: &mut Frame, app: &mut App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(28), Constraint::Percentage(72)])
        .split(area);

    render_platforms_list(frame, app, main_chunks[0]);

    match app.view_mode {
        ViewMode::Table => render_games_table(frame, app, main_chunks[1]),
        ViewMode::CoverCard | ViewMode::BannerCard | ViewMode::IconCard => {
            render_games_grid(frame, app, main_chunks[1])
        }
    }
}

fn render_platforms_list(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Platforms && app.modal_state == ModalState::None;
    let border_color = if is_focused {
        Color::Yellow
    } else {
        Color::Gray
    };

    let items: Vec<ListItem> = app
        .platforms
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let is_selected = idx == app.selected_platform_idx;
            let style = if is_selected {
                if is_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Rgb(40, 44, 52))
                        .add_modifier(Modifier::BOLD)
                }
            } else {
                Style::default().fg(Color::White)
            };

            let pointer = if is_selected && is_focused { "▶ " } else if is_selected { "► " } else { "  " };

            let type_badge = match p.platform_type {
                PlatformType::Emulator => "[EMU]",
                PlatformType::Native => "[NAT]",
                PlatformType::Wine => "[WIN]",
                PlatformType::Steam => "[STM]",
            };

            let content = format!("{} {} {}", pointer, p.name, type_badge);
            ListItem::new(content).style(style)
        })
        .collect();

    let focus_badge = if is_focused { " [FOCUS] " } else { " " };
    let title = format!(" Platforms ({}){}", app.platforms.len(), focus_badge);
    let list_widget = List::new(items).block(
        Block::default()
            .title(Span::styled(
                title,
                if is_focused {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = ListState::default();
    state.select(Some(app.selected_platform_idx));

    frame.render_stateful_widget(list_widget, area, &mut state);
}

fn get_game_type_badge(game: &game_core::models::Game, platforms: &[game_core::models::Platform]) -> String {
    if game.game_type.to_lowercase() == "emulator" {
        if let Some(p) = platforms.iter().find(|p| p.id == game.platform_id) {
            match p.slug.to_lowercase().as_str() {
                "3ds" => "3DS".to_string(),
                "nds" | "ds" => "DS".to_string(),
                "ps1" | "psx" => "PS1".to_string(),
                "ps2" => "PS2".to_string(),
                "ps3" => "PS3".to_string(),
                "psp" => "PSP".to_string(),
                "psvita" | "vita" => "VITA".to_string(),
                "snes" => "SNES".to_string(),
                "nes" => "NES".to_string(),
                "gba" => "GBA".to_string(),
                "gbc" => "GBC".to_string(),
                "gb" => "GB".to_string(),
                "n64" => "N64".to_string(),
                "gc" | "gamecube" => "GC".to_string(),
                "wii" => "WII".to_string(),
                "wiiu" => "WIIU".to_string(),
                "switch" => "SWITCH".to_string(),
                "megadrive" | "genesis" => "GENESIS".to_string(),
                "dreamcast" | "dc" => "DC".to_string(),
                "saturn" => "SATURN".to_string(),
                _ => p.slug.to_uppercase(),
            }
        } else {
            "EMU".to_string()
        }
    } else {
        game.game_type.to_uppercase()
    }
}

fn render_games_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Games;
    let border_color = if is_focused { Color::Yellow } else { Color::DarkGray };

    let header = Row::new(vec!["Title", "Ext", "Size", "Type"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .games
        .iter()
        .enumerate()
        .map(|(idx, g)| {
            let is_selected = idx == app.selected_game_idx;
            let is_checked = app.selected_game_ids.contains(&g.id);

            let style = if is_selected {
                if is_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(40, 44, 52))
                        .add_modifier(Modifier::BOLD)
                }
            } else if is_checked {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let pointer = if is_selected && is_focused { "▶ " } else if is_selected { "► " } else { "" };
            let mark = if is_checked { "[x] " } else { "" };
            let title = format!("{}{}{}", pointer, mark, g.title);
            let ext = g.file_extension.clone().unwrap_or_else(|| "-".to_string());
            let size_mb = g.file_size.map(|s| format!("{:.1} MB", s as f64 / (1024.0 * 1024.0))).unwrap_or_else(|| "-".to_string());
            let gtype = get_game_type_badge(g, &app.platforms);

            Row::new(vec![title, ext, size_mb, gtype]).style(style)
        })
        .collect();

    let sel_count = app.selected_game_ids.len();
    let sel_title = if sel_count > 0 {
        format!(" ({}/{} selected)", sel_count, app.games.len())
    } else {
        format!(" ({})", app.games.len())
    };

    let focus_badge = if is_focused { " [FOCUS]" } else { "" };

    let title = if let Some(p) = app.platforms.get(app.selected_platform_idx) {
        format!(" Games Table - {}{}{} [v] Switch View ", p.name, sel_title, focus_badge)
    } else {
        format!(" Games (0){} ", focus_badge)
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(50),
            Constraint::Percentage(15),
            Constraint::Percentage(18),
            Constraint::Percentage(17),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(Span::styled(
                title,
                if is_focused {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = TableState::default();
    if !app.games.is_empty() {
        state.select(Some(app.selected_game_idx));
    }

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_games_grid(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Games && app.modal_state == ModalState::None;
    let border_color = if is_focused {
        Color::Yellow
    } else {
        Color::Gray
    };

    let grid_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Left Column: List of Game Cards
    let items: Vec<ListItem> = app
        .games
        .iter()
        .enumerate()
        .map(|(idx, g)| {
            let is_selected = idx == app.selected_game_idx;
            let is_checked = app.selected_game_ids.contains(&g.id);

            let style = if is_selected {
                if is_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(40, 44, 52))
                        .add_modifier(Modifier::BOLD)
                }
            } else if is_checked {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let pointer = if is_selected && is_focused { "▶ " } else if is_selected { "► " } else { "  " };
            let mark = if is_checked { "[x] " } else { "" };
            let appid_info = g.steam_appid.map(|id| format!(" (AppID: {})", id)).unwrap_or_default();
            let gtype = get_game_type_badge(g, &app.platforms);
            let content = format!("{}{}{}[{}] {}{}", pointer, mark, if is_checked { "" } else { "" }, gtype, g.title, appid_info);

            ListItem::new(content).style(style)
        })
        .collect();

    let sel_count = app.selected_game_ids.len();
    let sel_title = if sel_count > 0 {
        format!(" ({}/{} selected)", sel_count, app.games.len())
    } else {
        format!(" ({})", app.games.len())
    };

    let mode_name = match app.view_mode {
        ViewMode::CoverCard => "Cards (Cover)",
        ViewMode::BannerCard => "Hero Banners",
        ViewMode::IconCard => "Icons",
        ViewMode::Table => "Table",
    };

    let focus_badge = if is_focused { " [FOCUS]" } else { "" };

    let title = if let Some(p) = app.platforms.get(app.selected_platform_idx) {
        format!(" Mode: {} - {}{}{} [v] Cycle View ", mode_name, p.name, sel_title, focus_badge)
    } else {
        format!(" Mode: {} (0){} ", mode_name, focus_badge)
    };

    let list_widget = List::new(items).block(
        Block::default()
            .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = ListState::default();
    if !app.games.is_empty() {
        state.select(Some(app.selected_game_idx));
    }

    frame.render_stateful_widget(list_widget, grid_chunks[0], &mut state);

    // Right Column: Vertical Cover Image Card / Banner Card / Icon Card (Top) & Metadata Panel (Bottom)
    render_game_cover_card(frame, app, grid_chunks[1]);
}

fn render_cover_placeholder(frame: &mut Frame, app: &App, game_id: i64, media_type: &str, area: Rect) {
    frame.render_widget(Clear, area);
    let cover_status = app.db.get_media_status(game_id, media_type).ok().flatten();
    let label = match media_type {
        "banner" => "Banner",
        "icon" => "Icon",
        _ => "Cover",
    };
    let msg = if cover_status.as_deref() == Some("not_found") {
        format!("  [ No {} Found ]", label)
    } else {
        format!("  [ Fetching {}... ]", label)
    };
    let placeholder = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(msg, Style::default().fg(Color::Yellow))),
    ]);
    frame.render_widget(placeholder, area);
}

fn render_game_cover_card(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.games.is_empty() || app.selected_game_idx >= app.games.len() {
        let empty_p = Paragraph::new("No game selected").block(
            Block::default()
                .title(" Media & Details ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Gray)),
        );
        frame.render_widget(empty_p, area);
        return;
    }

    let game = &app.games[app.selected_game_idx];
    let game_id = game.id;
    let current_platform = app.platforms.get(app.selected_platform_idx);

    let (media_type, title_prefix, top_percentage) = match app.view_mode {
        ViewMode::CoverCard => ("cover", "Cover", 75),
        ViewMode::BannerCard => ("banner", "Hero Banner", 55),
        ViewMode::IconCard => ("icon", "Icon", 47),
        ViewMode::Table => ("cover", "Cover", 75),
    };

    let card_vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(top_percentage), Constraint::Percentage(100 - top_percentage)])
        .split(area);

    let cover_block = Block::default()
        .title(Span::styled(
            format!(" {} - {} ", title_prefix, game.title),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner_cover_area = cover_block.inner(card_vertical_chunks[0]);
    frame.render_widget(Clear, card_vertical_chunks[0]);
    frame.render_widget(cover_block, card_vertical_chunks[0]);

    let cell_ratio: f32 = match media_type {
        "banner" => 3.2,
        "icon" => 2.0,
        _ => 1.333,
    };

    let v_pad = 2u16;
    let max_h = inner_cover_area.height.saturating_sub(v_pad * 2).max(4);
    let target_w = ((max_h as f32) * cell_ratio) as u16;
    let (img_w, img_h) = if target_w <= inner_cover_area.width.saturating_sub(2) {
        (target_w.max(2), max_h)
    } else {
        let fit_w = inner_cover_area.width.saturating_sub(2).max(2);
        let fit_h = ((fit_w as f32) / cell_ratio) as u16;
        (fit_w, fit_h.min(max_h).max(2))
    };

    let offset_x = (inner_cover_area.width.saturating_sub(img_w)) / 2;
    let offset_y = (inner_cover_area.height.saturating_sub(img_h)) / 2 + 1;
    let centered_media_rect = Rect::new(
        inner_cover_area.x + offset_x,
        inner_cover_area.y + offset_y,
        img_w,
        img_h,
    );

    if let Some(protocol) = app.media_protocols.get_mut(&(game_id, media_type.to_string())) {
        let image_widget = StatefulImage::new(None);
        frame.render_stateful_widget(image_widget, centered_media_rect, protocol);
    } else {
        render_cover_placeholder(frame, app, game_id, media_type, centered_media_rect);
    }

    // 2. Render Game Details Panel
    let mut details_lines = Vec::new();

    details_lines.push(Line::from(vec![
        Span::styled("Title: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(&game.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]));

    if let Some(p) = current_platform {
        let gtype = get_game_type_badge(game, &app.platforms);
        details_lines.push(Line::from(vec![
            Span::styled("Platform: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&p.name, Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(gtype, Style::default().fg(Color::Green)),
        ]));
    }

    if let Some(appid) = game.steam_appid {
        details_lines.push(Line::from(vec![
            Span::styled("Steam AppID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(appid.to_string(), Style::default().fg(Color::Yellow)),
        ]));
    }

    let size_str = game
        .file_size
        .map(|s| format!("{:.2} GB", s as f64 / (1024.0 * 1024.0 * 1024.0)))
        .unwrap_or_else(|| "N/A".to_string());

    details_lines.push(Line::from(vec![
        Span::styled("Size on Disk: ", Style::default().fg(Color::DarkGray)),
        Span::raw(size_str),
    ]));

    details_lines.push(Line::from(vec![
        Span::styled("[ENTER] Launch Game", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
    ]));

    let details_p = Paragraph::new(details_lines).block(
        Block::default()
            .title(Span::styled(" Game Details ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(details_p, card_vertical_chunks[1]);
}

fn render_activity_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let bar_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let status_style = if app.status_msg.starts_with("[Error]") || app.status_msg.starts_with("Error") {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if app.status_msg.starts_with("[OK]") {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let status_paragraph = Paragraph::new(Line::from(vec![
        Span::styled(" LOG: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(&app.status_msg, status_style),
    ]))
    .block(
        Block::default()
            .title(Span::styled(" System Status & Log ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(status_paragraph, bar_chunks[0]);

    if let Some(ref progress) = app.download_progress {
        let is_scanning = progress.runner_name.contains("Scan") || progress.runner_name.contains("Identif");
        let (prefix, label) = if is_scanning {
            (
                " Task Progress: ",
                format!("{:.1}% ({}/{} items)", progress.percentage, progress.downloaded_bytes, progress.total_bytes)
            )
        } else {
            let downloaded_mb = progress.downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total_mb = progress.total_bytes as f64 / (1024.0 * 1024.0);
            let is_extracting = progress.percentage >= 99.9;
            let pfx = if is_extracting { " Extracting Archive: " } else { " Downloading Archive: " };
            (pfx, format!("{:.1}% ({:.1}/{:.1} MB)", progress.percentage, downloaded_mb, total_mb))
        };

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("{}{}", prefix, progress.runner_name),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .gauge_style(
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .percent((progress.percentage as u16).min(100))
            .label(label);

        frame.render_widget(gauge, bar_chunks[1]);
    } else {
        let idle_paragraph = Paragraph::new(" [ Download / Extraction Progress: Idle ]")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title(Span::styled(" Task Slider ", Style::default().fg(Color::DarkGray)))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(idle_paragraph, bar_chunks[1]);
    }
}

fn render_controls_footer(frame: &mut Frame, _app: &App, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" [/] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Search ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [?] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Help ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [v] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("View ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [m] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Emulators ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [c] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Wine ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [a] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Add Game ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [s] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Settings ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [Alt+O] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled("Big Picture ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [↵] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled("Launch Game", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]);

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(paragraph, area);
}

fn render_big_picture_mode(frame: &mut Frame, app: &mut App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Row (Platforms Navbar + Search Input)
            Constraint::Min(10),   // Stage
            Constraint::Length(3), // Floating Footer
        ])
        .split(area);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_chunks[0]);

    let is_bar_focused = app.big_picture_focus == BigPictureFocus::PlatformBar;
    let is_search_focused = app.big_picture_focus == BigPictureFocus::Search;

    // 1. LEFT TOP CHUNK: Dedicated Search Input Box for Big Picture
    let search_border_color = if is_search_focused {
        Color::Yellow
    } else if !app.search_query.is_empty() {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let search_content = if !app.search_query.is_empty() {
        let cursor = if is_search_focused { "█" } else { "" };
        vec![Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(&app.search_query, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(cursor, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" ({})", app.games.len()), Style::default().fg(Color::DarkGray)),
        ])]
    } else if is_search_focused {
        vec![Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" (Type...)", Style::default().fg(Color::DarkGray)),
        ])]
    } else {
        vec![Line::from(vec![
            Span::styled("   [Search...]", Style::default().fg(Color::DarkGray)),
        ])]
    };

    let search_p = Paragraph::new(search_content).block(
        Block::default()
            .title(Span::styled(
                if is_search_focused { " Search (Active) " } else { " Search " },
                Style::default().fg(search_border_color),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(search_border_color)),
    );
    frame.render_widget(search_p, top_chunks[0]);

    // 2. RIGHT TOP CHUNK: Platforms Carousel Navbar
    let mut platform_spans = Vec::new();
    for (idx, p) in app.platforms.iter().enumerate() {
        let is_current = idx == app.selected_platform_idx;
        let count = app.db.get_games_for_platform(p.id).map(|g| g.len()).unwrap_or(0);
        let label = format!(" {} ({}) ", p.name, count);

        if is_current {
            if is_bar_focused {
                platform_spans.push(Span::styled(
                    format!("▶{}◀", label),
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            } else {
                platform_spans.push(Span::styled(
                    format!("[{}]", label),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            platform_spans.push(Span::styled(
                format!(" {} ", p.name),
                Style::default().fg(Color::DarkGray),
            ));
        }
        platform_spans.push(Span::raw(" "));
    }

    let platform_bar_color = if is_bar_focused { Color::Yellow } else { Color::DarkGray };
    let platform_p = Paragraph::new(Line::from(platform_spans)).block(
        Block::default()
            .title(Span::styled(
                if is_bar_focused { " Consoles (Focused) " } else { " Consoles " },
                Style::default().fg(platform_bar_color),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(platform_bar_color)),
    );
    frame.render_widget(platform_p, top_chunks[1]);

    let stage_area = main_chunks[1];

    if app.games.is_empty() {
        let empty_p = Paragraph::new("\n  No games found in current platform.\n  Press [Alt+O] to return to Library Mode.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty_p, stage_area);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(23), // Left Side (Previous Game Preview)
                Constraint::Percentage(54), // Center Stage (FEATURED HD GAME)
                Constraint::Percentage(23), // Right Side (Next Game Preview)
            ])
            .split(stage_area);

        let sel_idx = app.selected_game_idx;

        // 1. LEFT SIDE: Previous Game Preview (Halfblocks cover, 2D dead-centered)
        if sel_idx > 0 {
            let prev_game = &app.games[sel_idx - 1];
            let left_stage = centered_rect(100, 85, cols[0]);
            let left_block = Block::default()
                .title(Span::styled(format!(" ◀ {} ", prev_game.title), Style::default().fg(Color::DarkGray)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray));
            let inner = left_block.inner(left_stage);
            frame.render_widget(left_block, left_stage);

            let max_h = inner.height.saturating_sub(2).max(4);
            let target_w = ((max_h as f32) * 1.33) as u16;
            let (img_w, img_h) = if target_w <= inner.width.saturating_sub(2) {
                (target_w, max_h)
            } else {
                let fit_w = inner.width.saturating_sub(2).max(4);
                let fit_h = ((fit_w as f32) / 1.33) as u16;
                (fit_w, fit_h.min(max_h))
            };
            let offset_x = (inner.width.saturating_sub(img_w)) / 2;
            let offset_y = (inner.height.saturating_sub(img_h)) / 2;
            let left_img_rect = Rect::new(inner.x + offset_x, inner.y + offset_y, img_w, img_h);

            let key = (prev_game.id, "cover_hb".to_string());
            if let Some(protocol) = app.media_protocols.get_mut(&key) {
                let image_widget = StatefulImage::new(None);
                frame.render_stateful_widget(image_widget, left_img_rect, protocol);
            } else {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("◀ PREV GAME", Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                    Line::from(Span::styled(&prev_game.title, Style::default().fg(Color::Gray))),
                ];
                let p = Paragraph::new(lines).alignment(Alignment::Center);
                frame.render_widget(p, left_img_rect);
            }
        }

        // 2. CENTER STAGE: Featured Focused Game in CRISP HD (Centered Horizontally & Vertically)!
        let active_game = &app.games[sel_idx];
        let center_block = Block::default()
            .title(Span::styled(
                format!(" FEATURED: {} ", active_game.title),
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow));

        let center_inner = center_block.inner(cols[1]);
        frame.render_widget(center_block, cols[1]);

        let inner_h = center_inner.height;
        let inner_w = center_inner.width;

        let text_h = 2u16;
        let gap_h = 1u16;
        let padding_h = 3u16;

        let avail_img_h = inner_h.saturating_sub(text_h + gap_h + padding_h).max(4);
        let target_img_w = ((avail_img_h as f32) * 1.33) as u16;

        let (img_w, img_h) = if target_img_w <= inner_w.saturating_sub(2) {
            (target_img_w, avail_img_h)
        } else {
            let fit_w = inner_w.saturating_sub(2).max(6);
            let fit_h = ((fit_w as f32) / 1.33) as u16;
            (fit_w, fit_h.min(avail_img_h))
        };

        let total_content_h = img_h + gap_h + text_h;
        let top_margin = (inner_h.saturating_sub(total_content_h)) / 2 + 2;
        let left_margin = (inner_w.saturating_sub(img_w)) / 2;

        let img_centered_rect = Rect::new(
            center_inner.x + left_margin,
            center_inner.y + top_margin,
            img_w,
            img_h,
        );

        let details_rect = Rect::new(
            center_inner.x,
            center_inner.y + top_margin + img_h,
            inner_w,
            text_h,
        );

        // Render Featured HD Native Cover Image
        let key = (active_game.id, "cover".to_string());
        if let Some(protocol) = app.media_protocols.get_mut(&key) {
            let image_widget = StatefulImage::new(None);
            frame.render_stateful_widget(image_widget, img_centered_rect, protocol);
        } else {
            let no_img = Paragraph::new("\n [ Loading HD Cover Artwork... ]\n Press [w] for Media Manager")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(no_img, img_centered_rect);
        }

        let badge = match active_game.game_type.as_str() {
            "wine" => "Windows / Wine / Proton",
            "native" => "Linux Native Executable",
            "steam" => "Steam Application",
            _ => "Emulator ROM",
        };

        let details_lines = vec![
            Line::from(vec![
                Span::styled("TITLE: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(&active_game.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("TYPE: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(badge, Style::default().fg(Color::Yellow)),
                Span::raw("  |  "),
                Span::styled("STATUS: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("Ready to Play", Style::default().fg(Color::Green)),
            ]),
        ];
        let details_p = Paragraph::new(details_lines).alignment(Alignment::Center);
        frame.render_widget(details_p, details_rect);

        // 3. RIGHT SIDE: Next Game Preview (Halfblocks cover, 2D dead-centered)
        if sel_idx + 1 < app.games.len() {
            let next_game = &app.games[sel_idx + 1];
            let right_stage = centered_rect(100, 85, cols[2]);
            let right_block = Block::default()
                .title(Span::styled(format!(" {} ▶ ", next_game.title), Style::default().fg(Color::DarkGray)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray));
            let inner = right_block.inner(right_stage);
            frame.render_widget(right_block, right_stage);

            let max_h = inner.height.saturating_sub(2).max(4);
            let target_w = ((max_h as f32) * 1.33) as u16;
            let (img_w, img_h) = if target_w <= inner.width.saturating_sub(2) {
                (target_w, max_h)
            } else {
                let fit_w = inner.width.saturating_sub(2).max(4);
                let fit_h = ((fit_w as f32) / 1.33) as u16;
                (fit_w, fit_h.min(max_h))
            };
            let offset_x = (inner.width.saturating_sub(img_w)) / 2;
            let offset_y = (inner.height.saturating_sub(img_h)) / 2;
            let right_img_rect = Rect::new(inner.x + offset_x, inner.y + offset_y, img_w, img_h);

            let key = (next_game.id, "cover_hb".to_string());
            if let Some(protocol) = app.media_protocols.get_mut(&key) {
                let image_widget = StatefulImage::new(None);
                frame.render_stateful_widget(image_widget, right_img_rect, protocol);
            } else {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("NEXT GAME ▶", Style::default().fg(Color::DarkGray))),
                    Line::from(""),
                    Line::from(Span::styled(&next_game.title, Style::default().fg(Color::Gray))),
                ];
                let p = Paragraph::new(lines).alignment(Alignment::Center);
                frame.render_widget(p, right_img_rect);
            }
        }
    }

    // Floating Footer (Transparent, Fine Brackets, Rounded Borders)
    let footer_text = Line::from(vec![
        Span::styled(" [/] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Search ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Focus ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [p] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("Consoles ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [Alt+O] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled("Normal Mode ", Style::default().fg(Color::Gray)),
        Span::raw("│ "),
        Span::styled(" [↵] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled("Launch Game", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]);

    let footer_p = Paragraph::new(footer_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    frame.render_widget(footer_p, main_chunks[2]);
}

fn extract_custom_flags(cmd: &str) -> String {
    let installed = game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
    for r in &installed {
        let runner_str = match r.kind {
            game_core::runner_detector::RunnerKind::Proton => format!("\"{}\" run \"{{file_path}}\"", r.binary_path.display()),
            game_core::runner_detector::RunnerKind::Wine => format!("\"{}\" \"{{file_path}}\"", r.binary_path.display()),
        };
        if cmd == runner_str {
            return String::new();
        }
    }
    if let Some(pos) = cmd.find("\"{file_path}\"") {
        let remainder = cmd[pos + "\"{file_path}\"".len()..].trim();
        if !remainder.is_empty() {
            return remainder.to_string();
        }
    }
    cmd.trim().to_string()
}

fn extract_runner_display_name(cmd: &str) -> String {
    if cmd.trim().is_empty() {
        return "System Wine / Default".to_string();
    }

    let installed = game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
    for r in &installed {
        if cmd.contains(&r.name) || cmd.contains(r.binary_path.to_str().unwrap_or("")) {
            return format!("{} ({})", r.name, r.location.display_name());
        }
    }

    if let Some(first_word) = cmd.split_whitespace().next() {
        let clean = first_word.trim_matches('"').trim_matches('\'');
        let path = std::path::Path::new(clean);
        if let Some(parent) = path.parent() {
            if let Some(fname) = parent.file_name() {
                let name = fname.to_string_lossy();
                if name != "bin" && name != "usr" {
                    return name.to_string();
                }
            }
        }
        if let Some(fname) = path.file_name() {
            return fname.to_string_lossy().to_string();
        }
    }

    "Custom Runner".to_string()
}

/// Render centered pop-up modal overlay dialog
fn render_modal(frame: &mut Frame, app: &mut App) {
    let popup_area = centered_rect(75, 70, frame.area());
    if !matches!(
        app.modal_state,
        ModalState::ConfirmDeleteGame { .. }
            | ModalState::EditCustomArgsInput { .. }
            | ModalState::PlatformSelector { .. }
    ) {
        frame.render_widget(Clear, popup_area);
    }

    match app.modal_state {
        ModalState::AddGameStep1Type { selected_type_idx } => {
            let options = vec![
                "[Folder Scan] Automated ROMs Directory Scanner",
                "[NAT] Linux Native Game (Binary, Script, AppImage)",
                "[WIN] Windows Game (Wine / Proton .exe)",
                "[STM] Steam Game (Launch via Steam AppID)",
            ];

            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(idx, opt)| {
                    let is_selected = idx == selected_type_idx;
                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(format!("  {} ", opt)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Add Games - Step 1: Select Import Method ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Select | [Enter] Next | [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ScanFolderStep1Platform { selected_platform_idx } => {
            let configured_emulators = app.get_configured_emulator_platforms();
            let active_ids: Vec<i64> = app.platforms.iter().map(|p| p.id).collect();

            if configured_emulators.is_empty() {
                let empty_p = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  [ No Configured Emulator Platforms ]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from("  First configure an emulator runner in [m] (e.g. Azahar for Nintendo 3DS)"),
                    Line::from("  to enable automated ROM scanning for that emulator platform."),
                ]).block(
                    Block::default()
                        .title(Span::styled(" Scan ROMs Folder - Select Platform ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                );

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(6), Constraint::Length(2)])
                    .split(popup_area);

                frame.render_widget(empty_p, chunks[0]);

                let help = Paragraph::new(" [Esc] Back")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(help, chunks[1]);
                return;
            }

            let items: Vec<ListItem> = configured_emulators
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let is_selected = idx == selected_platform_idx;
                    let is_active = active_ids.contains(&p.id);

                    let status_badge = if is_active {
                        " [Active / Configured]"
                    } else {
                        " [Runner Ready]"
                    };

                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Green)
                    };

                    ListItem::new(format!("  {} ({}){}", p.name, p.slug, status_badge)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Scan ROMs Folder - Step 2: Select Configured Emulator Platform ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Select Platform | [Enter] Configure Scan Form | [Esc] Back")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ScanFolderForm {
            ref platform,
            ref folder_path,
            ref extensions_input,
            recursive,
            use_dat_auto_id,
            selected_field,
        } => {
            let runner_info = app.db.get_runner_for_platform(platform.id).ok().flatten();
            let is_runner_ready = runner_info.as_ref().and_then(|r| r.executable_path.as_ref()).is_some() || platform.slug == "linux" || platform.slug == "windows";

            let field_style = |idx: usize| {
                if idx == selected_field {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Target Platform: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&platform.name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));

            let runner_status = if is_runner_ready {
                Span::styled("Status: [Runner Ready]", Style::default().fg(Color::Green))
            } else {
                Span::styled("Status: [Runner Not Configured - Press [c] to Configure]", Style::default().fg(Color::Red))
            };
            lines.push(Line::from(runner_status));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("1. Folder Path: ", field_style(0)),
                Span::raw(if folder_path.is_empty() { "< Press [Enter] to select folder directory >" } else { folder_path }),
            ]));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("2. File Extensions: ", field_style(1)),
                Span::raw(extensions_input),
            ]));
            lines.push(Line::from(""));

            let rec_check = if recursive { "[X] Yes" } else { "[ ] No" };
            lines.push(Line::from(vec![
                Span::styled("3. Scan Subfolders Recursively: ", field_style(2)),
                Span::styled(rec_check, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" (Press [Space] to toggle)"),
            ]));
            lines.push(Line::from(""));

            let dat_check = if use_dat_auto_id { "[X] Yes (DAT / Serial Auto-ID)" } else { "[ ] No (Use raw filenames)" };
            lines.push(Line::from(vec![
                Span::styled("4. Automatic DAT Identification: ", field_style(3)),
                Span::styled(dat_check, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" (Press [Space] to toggle)"),
            ]));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("[ START SCANNING ROMS ]", field_style(4)),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Scan ROMs Folder Options: {} ", platform.name),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Up/Down/Tab] Navigate Fields | [Enter] Confirm / Select Folder | [Space] Toggle Subfolders | [Esc] Back")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ConfigureApiKeyInput { ref input } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled("SteamGridDB API Key Required", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(""));
            lines.push(Line::from("Enter your personal SteamGridDB API key to download HD covers, banners, and icons."));
            lines.push(Line::from("You can get a free API key at: https://www.steamgriddb.com/profile/api"));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("API Key: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    if input.is_empty() { "< Type or Paste SteamGridDB API Key here >" } else { input },
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("[ SAVE API KEY & FETCH MEDIA ]", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " SteamGridDB Configuration ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Enter] Save API Key | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::AppSettings {
            ref api_key_input,
            selected_field,
            is_editing_api_key,
            cursor_pos,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled("Application Settings", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(""));

            let f0_label = if selected_field == 0 { "▶ 1. SteamGridDB API Key: " } else { "  1. SteamGridDB API Key: " };
            let f0_label_style = if selected_field == 0 {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(vec![
                Span::styled(f0_label, f0_label_style),
            ]));

            if is_editing_api_key {
                let (before, after) = api_key_input.split_at(cursor_pos.min(api_key_input.len()));
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(before, Style::default().fg(Color::White).bg(Color::DarkGray)),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]));
                lines.push(Line::from(Span::styled(
                    "   (Press [Enter] or [Esc] to Lock Key | [Ctrl+V] to Paste)",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                let masked_text = if api_key_input.is_empty() {
                    "< No API Key Set - Press [Enter] to Edit / Paste >".to_string()
                } else {
                    "●".repeat(api_key_input.len())
                };
                let display_style = if selected_field == 0 {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(masked_text, display_style),
                ]));
                lines.push(Line::from(Span::styled(
                    "   * Get key at: https://www.steamgriddb.com/profile/preferences/api (Press [Enter] to Reveal & Edit)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(""));

            let f1_style = if selected_field == 1 {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::styled(if selected_field == 1 { " ▶ " } else { "   " }, Style::default().fg(Color::Yellow)),
                Span::styled("[ Re-run Welcome & Setup Wizard ]", f1_style),
            ]));
            lines.push(Line::from(""));

            let f2_style = if selected_field == 2 {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::from(vec![
                Span::styled(if selected_field == 2 { " ▶ " } else { "   " }, Style::default().fg(Color::Yellow)),
                Span::styled("[ SAVE SETTINGS ]", f2_style),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " App Settings & Configuration ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Navigate Fields | [Enter] Reveal & Edit / Confirm | [Esc] Close Settings")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WelcomeWizard { step, ref sgdb_api_key, cursor_pos, .. } => {
            let wizard_area = frame.area();
            frame.render_widget(Clear, wizard_area);

            let main_block = Block::default()
                .title(Span::styled(
                    " GAME STATION - WELCOME & INITIAL SETUP ",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ))
                .title_bottom(Span::styled(
                    format!(" Step ({}/4) | [← / →] Switch Slide | [Tab] Cycle | [Esc] Skip Setup ", step + 1),
                    Style::default().fg(Color::DarkGray),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Yellow));

            let inner_area = main_block.inner(wizard_area);
            frame.render_widget(main_block, wizard_area);

            let outer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),     // Flexible Top Spacer (Centers content vertically)
                    Constraint::Length(18), // Main Centered Content Block (Banner + Spacer + Body)
                    Constraint::Min(0),     // Flexible Bottom Spacer
                    Constraint::Length(3),  // Fixed Footer Bar
                ])
                .split(inner_area);

            let content_area = outer_chunks[1];
            let content_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(7), // Massive ASCII Banner
                    Constraint::Length(1), // Spacer
                    Constraint::Min(10),   // Slide Body Lines
                ])
                .split(content_area);

            // 1. ANSI Shadow ASCII Art Header (Matching Chirp / Chirp Hub style with native terminal colors & pixel-perfect alignment)
            let ascii_banner = vec![
                Line::from(Span::styled("  ██████╗  █████╗ ███╗   ███╗███████╗    ███████╗████████╗█████╗ ████████╗██╗ ██████╗ ███╗   ██╗", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(" ██╔════╝ ██╔══██╗████╗ ████║██╔════╝    ██╔════╝╚══██╔══╝██╔══██╗╚══██╔══╝██║██╔═══██╗████╗  ██║", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(" ██║  ███╗███████║██╔████╔██║█████╗      ███████╗   ██║   ███████║   ██║   ██║██║   ██║██╔██╗ ██║", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(" ██║   ██║██╔══██║██║╚██╔╝██║██╔══╝      ╚════██║   ██║   ██╔══██║   ██║   ██║██║   ██║██║╚██╗██║", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(" ╚██████╔╝██║  ██║██║ ╚═╝ ██║███████╗    ███████║   ██║   ██║  ██║   ██║   ██║╚██████╔╝██║ ╚████║", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("  ╚═════╝ ╚═╝  ╚═╝╚═╝     ╚═╝╚══════╝    ╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝ ╚═════╝ ╚═╝   ╚═╝ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
                Line::from(""),
            ];
            let ascii_p = Paragraph::new(ascii_banner).alignment(Alignment::Center);
            frame.render_widget(ascii_p, content_chunks[0]);

            // 2. Slide Content Body (Horizontally and Vertically Centered)
            let mut body_lines = Vec::new();
            match step {
                0 => {
                    body_lines.push(Line::from(Span::styled("WELCOME TO GAME STATION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    body_lines.push(Line::from(Span::styled("Centralized Retro & Modern Gaming Dashboard for Linux", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("Game Station is your all-in-one terminal hub to organize, manage, and launch your"));
                    body_lines.push(Line::from("entire video game library seamlessly from a fast, hardware-accelerated TUI interface."));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![
                        Span::styled("Multi-Platform Consoles ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw("│ 3DS, DS, GameCube, Wii, Switch, PS1, PS2, PSP, SNES, GBA & more"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("Native & Wine / Proton ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw("│ Windows executables, custom runners, winetricks & Steam games"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("HD Artwork Scraper      ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw("│ Automatic cover art, hero banners & icons in seconds"),
                    ]));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(Span::styled("Press [ → / Right Arrow ] or [ Enter ] to continue setup tour...", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))));
                }
                1 => {
                    body_lines.push(Line::from(Span::styled("KEY FEATURES & NAVIGATION SHORTCUTS", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![
                        Span::styled("  [Alt+O]  ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                        Span::raw("Toggle Big Picture Mode (3D Cover Flow Stage with HD Media)"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("    [/]    ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw("Interactive Live Search bar to filter games across all platforms"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("    [w]    ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw("Visual Media Selector to fetch & customize covers, banners and icons"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("    [c]    ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw("Wine & Proton Runner Manager, winetricks and prefix tools"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled("    [?]    ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw("Keyboard & Mouse Controls Cheatsheet"),
                    ]));
                }
                2 => {
                    body_lines.push(Line::from(Span::styled("ARTWORK SCRAPER CONFIGURATION (OPTIONAL)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("Configure your SteamGridDB API key to enable instant high-definition cover artwork"));
                    body_lines.push(Line::from("and hero banner scraping for all your ROMs and executables."));
                    body_lines.push(Line::from(""));

                    let (before, after) = sgdb_api_key.split_at(cursor_pos.min(sgdb_api_key.len()));
                    body_lines.push(Line::from(vec![
                        Span::styled("SteamGridDB API Key: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(before, Style::default().fg(Color::White).bg(Color::DarkGray)),
                        Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                        Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                    ]));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("  * Obtain your free API key at: https://www.steamgriddb.com/profile/preferences/api"));
                    body_lines.push(Line::from("  * Use [Ctrl+V] to paste from clipboard"));
                }
                _ => {
                    body_lines.push(Line::from(Span::styled("INITIAL SETUP COMPLETE", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("Game Station is fully configured and ready for your game collection."));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![
                        Span::styled("[ GET STARTED ]", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]));
                }
            }

            let body_p = Paragraph::new(body_lines)
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::NONE),
                );
            frame.render_widget(body_p, content_chunks[2]);

            // 3. Footer Navigation Bar & Step Dots
            let prev_btn = if step > 0 {
                Span::styled(" [ ← Back ] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("            ", Style::default())
            };

            let next_btn = if step < 3 {
                Span::styled(" [ Next → ] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" [ Finish ] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            };

            let footer_content = Line::from(vec![
                prev_btn,
                Span::raw("       "),
                Span::styled("Slide ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}/4 ", step + 1), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                if step == 0 { Span::styled("● ", Style::default().fg(Color::Yellow)) } else { Span::styled("○ ", Style::default().fg(Color::DarkGray)) },
                if step == 1 { Span::styled("● ", Style::default().fg(Color::Yellow)) } else { Span::styled("○ ", Style::default().fg(Color::DarkGray)) },
                if step == 2 { Span::styled("● ", Style::default().fg(Color::Yellow)) } else { Span::styled("○ ", Style::default().fg(Color::DarkGray)) },
                if step == 3 { Span::styled("● ", Style::default().fg(Color::Yellow)) } else { Span::styled("○ ", Style::default().fg(Color::DarkGray)) },
                Span::raw("       "),
                next_btn,
            ]);

            let footer_p = Paragraph::new(footer_content)
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );

            frame.render_widget(footer_p, outer_chunks[3]);
        }
        ModalState::VisualMediaSelector {
            ref game_title,
            ref search_query,
            active_tab,
            focused_section,
            cursor_pos,
            is_searching,
            ref candidates,
            selected_candidate_idx,
            ref covers,
            selected_cover_idx,
            chosen_cover_idx,
            ref banners,
            selected_banner_idx,
            chosen_banner_idx,
            ref icons,
            selected_icon_idx,
            chosen_icon_idx,
            ..
        } => {
            let c_mark = if chosen_cover_idx.is_some() { "✓" } else { "-" };
            let b_mark = if chosen_banner_idx.is_some() { "✓" } else { "-" };
            let i_mark = if chosen_icon_idx.is_some() { "✓" } else { "-" };

            let tab_titles = vec![
                format!("1. Candidates ({})", candidates.len()),
                format!("2. Covers ({}) [{}]", covers.len(), c_mark),
                format!("3. Banners ({}) [{}]", banners.len(), b_mark),
                format!("4. Icons ({}) [{}]", icons.len(), i_mark),
            ];

            let tab_spans: Vec<Span> = tab_titles
                .iter()
                .enumerate()
                .map(|(idx, title)| {
                    if idx == active_tab {
                        let style = if focused_section == 0 {
                            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                        };
                        Span::styled(format!(" [ {} ] ", title), style)
                    } else {
                        Span::styled(format!("   {}   ", title), Style::default().fg(Color::Gray))
                    }
                })
                .collect();

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(8), Constraint::Length(2)])
                .split(popup_area);

            let tabs_title = if focused_section == 0 {
                " [FOCUS: TABS - Use Left/Right to Switch] "
            } else {
                " Tabs "
            };
            let tabs_p = Paragraph::new(Line::from(tab_spans)).block(
                Block::default()
                    .title(Span::styled(tabs_title, Style::default().fg(if focused_section == 0 { Color::Yellow } else { Color::DarkGray }).add_modifier(Modifier::BOLD)))
                    .borders(Borders::NONE)
            );
            frame.render_widget(tabs_p, modal_chunks[0]);

            let (list_area, preview_area) = if active_tab > 0 {
                let side_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                    .split(modal_chunks[1]);
                (side_chunks[0], Some(side_chunks[1]))
            } else {
                (modal_chunks[1], None)
            };

            match active_tab {
                0 => {
                    let cand_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(3), Constraint::Min(5)])
                        .split(list_area);

                    let search_border_style = if focused_section == 1 {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let search_title = if focused_section == 1 {
                        " Search Query [ACTIVE FOCUS] (Type & Press Left/Right: Cursor, Enter: Search, Down: Results) "
                    } else {
                        " Search Query (Press Up/Down to Focus) "
                    };

                    let query_line = if focused_section == 1 {
                        let cpos = cursor_pos.min(search_query.len());
                        let (before, after) = search_query.split_at(cpos);
                        Line::from(vec![
                            Span::raw(" "),
                            Span::raw(before),
                            Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                            Span::raw(after),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw(" "),
                            Span::raw(search_query),
                        ])
                    };

                    let search_p = Paragraph::new(query_line).block(
                        Block::default()
                            .title(Span::styled(
                                search_title,
                                Style::default().fg(if focused_section == 1 { Color::Yellow } else { Color::DarkGray }).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(search_border_style),
                    );
                    frame.render_widget(search_p, cand_chunks[0]);

                    let items: Vec<ListItem> = if is_searching {
                        vec![ListItem::new(" [ Searching SteamGridDB... ]").style(Style::default().fg(Color::Yellow))]
                    } else if candidates.is_empty() {
                        vec![ListItem::new(" No candidates found. Type a custom name above and press [Enter] / [s] to Search.").style(Style::default().fg(Color::Red))]
                    } else {
                        candidates
                            .iter()
                            .enumerate()
                            .map(|(idx, cand)| {
                                let is_selected = idx == selected_candidate_idx && focused_section == 2;
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                                } else if idx == selected_candidate_idx {
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {} (SGDB ID: {})", cand.name, cand.id)).style(style)
                            })
                            .collect()
                    };

                    let list_border_style = if focused_section == 2 {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let list_title = if focused_section == 2 {
                        format!(" Candidates for '{}' ({}) [ACTIVE FOCUS] ", game_title, candidates.len())
                    } else {
                        format!(" Candidates for '{}' ({}) ", game_title, candidates.len())
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                list_title,
                                Style::default().fg(if focused_section == 2 { Color::Cyan } else { Color::DarkGray }).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(list_border_style),
                    );
                    frame.render_widget(list, cand_chunks[1]);
                }
                1 => {
                    let items: Vec<ListItem> = if covers.is_empty() {
                        vec![ListItem::new(" No covers available for this candidate.").style(Style::default().fg(Color::Yellow))]
                    } else {
                        covers
                            .iter()
                            .enumerate()
                            .map(|(idx, c)| {
                                let is_selected = idx == selected_cover_idx && focused_section == 2;
                                let is_chosen = chosen_cover_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {}Cover #{} - ID: {}", check_str, idx + 1, c.id)).style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Covers ",
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 { Color::Green } else { Color::DarkGray })),
                    );
                    frame.render_widget(list, list_area);
                }
                2 => {
                    let items: Vec<ListItem> = if banners.is_empty() {
                        vec![ListItem::new(" No banners available for this candidate.").style(Style::default().fg(Color::Yellow))]
                    } else {
                        banners
                            .iter()
                            .enumerate()
                            .map(|(idx, b)| {
                                let is_selected = idx == selected_banner_idx && focused_section == 2;
                                let is_chosen = chosen_banner_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {}Banner #{} - ID: {}", check_str, idx + 1, b.id)).style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Banners ",
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 { Color::Green } else { Color::DarkGray })),
                    );
                    frame.render_widget(list, list_area);
                }
                3 => {
                    let items: Vec<ListItem> = if icons.is_empty() {
                        vec![ListItem::new(" No icons available for this candidate.").style(Style::default().fg(Color::Yellow))]
                    } else {
                        icons
                            .iter()
                            .enumerate()
                            .map(|(idx, ic)| {
                                let is_selected = idx == selected_icon_idx && focused_section == 2;
                                let is_chosen = chosen_icon_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {}Icon #{} - ID: {}", check_str, idx + 1, ic.id)).style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Icons ",
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 { Color::Green } else { Color::DarkGray })),
                    );
                    frame.render_widget(list, list_area);
                }
                _ => {}
            }

            // Render Live Preview on the right panel
            if let Some(preview_box) = preview_area {
                let preview_block = Block::default()
                    .title(Span::styled(
                        " Image Preview ",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow));

                let inner = preview_block.inner(preview_box);
                frame.render_widget(preview_block, preview_box);

                if app.visual_preview_loading {
                    let loading_txt = Paragraph::new("\n  [ Downloading Preview... ]")
                        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                    frame.render_widget(loading_txt, inner);
                } else if let Some(ref mut proto) = app.visual_preview_protocol {
                    let image_widget = StatefulImage::new(None);
                    frame.render_stateful_widget(image_widget, inner, proto);
                } else {
                    let no_preview = Paragraph::new("\n  No preview selected")
                        .style(Style::default().fg(Color::DarkGray));
                    frame.render_widget(no_preview, inner);
                }
            }

            let help_str = match (focused_section, active_tab) {
                (0, _) => " [FOCUS: TABS] [Left/Right] Switch Tab | [Down] Focus Query/List | [Esc] Close",
                (1, 0) => " [FOCUS: SEARCH QUERY] [Left/Right] Move Cursor | [Typing] Edit Text | [Enter] Search | [Up/Down] Change Section",
                _ => " [FOCUS: LIST] [Up/Down] Navigate Items | [Enter] Select/Apply Item | [Up] Back to Query | [Esc] Close",
            };
            let help = Paragraph::new(help_str).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, modal_chunks[2]);
        }
        ModalState::ManageRunnersStep1Platform { selected_platform_idx } => {
            let unique_runners = app.db.get_unique_runners().unwrap_or_default();

            let items: Vec<ListItem> = unique_runners
                .iter()
                .enumerate()
                .map(|(idx, r)| {
                    let is_selected = idx == selected_platform_idx;
                    let status_badge = if r.is_configured {
                        " [Configured]"
                    } else {
                        ""
                    };

                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else if r.is_configured {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    ListItem::new(format!("  {} ({}){}", r.name, r.console_initials, status_badge)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Emulator / Runner Management - Select Emulator ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Navigate | [Enter] Configure Emulator | [w] Wine/Proton Manager | [Esc] Back")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ManageWineRunners {
            ref installed_runners,
            selected_idx,
        } => {
            let items: Vec<ListItem> = if installed_runners.is_empty() {
                vec![ListItem::new("  [ No Wine / Proton runners detected on system ]").style(Style::default().fg(Color::Yellow))]
            } else {
                installed_runners
                    .iter()
                    .enumerate()
                    .map(|(idx, r)| {
                        let is_selected = idx == selected_idx;
                        let kind_badge = match r.kind {
                            game_core::runner_detector::RunnerKind::Proton => "[Proton]",
                            game_core::runner_detector::RunnerKind::Wine => "[Wine]",
                        };
                        let style = if is_selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let line = format!("  {:8} {:30}  {:25} ({})", kind_badge, r.name, r.location.display_name(), r.binary_path.display());
                        ListItem::new(line).style(style)
                    })
                    .collect()
            };

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Installed Wine & Proton Runners ({}) ", installed_runners.len()),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [d] Download GE-Proton / Proton-CachyOS | [Del] Delete Folder | [Esc] Close")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ProtonDownloader {
            step,
            selected_launcher_idx,
            selected_tool_idx,
            ref releases,
            selected_release_idx,
            is_loading,
            download_event: _,
        } => {
            let launchers = scraper::proton::TargetLauncher::all();
            let current_launcher = launchers.get(selected_launcher_idx).copied().unwrap_or(scraper::proton::TargetLauncher::TUIGameStation);
            let valid_tools = current_launcher.valid_repos();
            let current_tool = valid_tools.get(selected_tool_idx).copied().unwrap_or(scraper::proton::ProtonRepo::GEProton);

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            let breadcrumb_line = match step {
                0 => Line::from(vec![
                    Span::styled(" [ STEP 1/3 ] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(" Select Target Launcher", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                1 => Line::from(vec![
                    Span::styled(" [ STEP 2/3 ] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {} ", current_launcher.display_name()), Style::default().fg(Color::Cyan)),
                    Span::styled("➜  Select Tool / Runner", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                _ => Line::from(vec![
                    Span::styled(" [ STEP 3/3 ] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {} ➜ {} ", current_launcher.display_name(), current_tool.display_name()), Style::default().fg(Color::Cyan)),
                    Span::styled("➜  Select Version to Download", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ]),
            };
            frame.render_widget(Paragraph::new(breadcrumb_line), modal_chunks[0]);

            match step {
                0 => {
                    let items: Vec<ListItem> = launchers
                        .iter()
                        .enumerate()
                        .map(|(idx, l)| {
                            let is_selected = idx == selected_launcher_idx;
                            let style = if is_selected {
                                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            let sub_dir = l.installation_dir(scraper::proton::ProtonRepo::GEProton);
                            let path_str = sub_dir.to_str().unwrap_or("");
                            ListItem::new(format!("  {}. {:22}  ({})", idx + 1, l.display_name(), path_str)).style(style)
                        })
                        .collect();

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(" Target Launchers ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    );
                    frame.render_widget(list, modal_chunks[1]);
                }
                1 => {
                    let items: Vec<ListItem> = valid_tools
                        .iter()
                        .enumerate()
                        .map(|(idx, repo)| {
                            let is_selected = idx == selected_tool_idx;
                            let style = if is_selected {
                                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            let target_dir = current_launcher.installation_dir(*repo);
                            let folder_name = target_dir.file_name().and_then(|f| f.to_str()).unwrap_or("runners");
                            ListItem::new(format!("  {:32}  [Installs to: {}]", repo.display_name(), folder_name)).style(style)
                        })
                        .collect();

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                format!(" Compatible Tools for {} ({}) ", current_launcher.display_name(), valid_tools.len()),
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    );
                    frame.render_widget(list, modal_chunks[1]);
                }
                _ => {
                    if is_loading {
                        let loading_p = Paragraph::new("\n  [ Fetching release catalog from API... ]")
                            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                        frame.render_widget(loading_p, modal_chunks[1]);
                    } else if releases.is_empty() {
                        let empty_p = Paragraph::new("\n  No downloadable releases found for this repository.")
                            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
                        frame.render_widget(empty_p, modal_chunks[1]);
                    } else {
                        let items: Vec<ListItem> = releases
                            .iter()
                            .enumerate()
                            .map(|(idx, rel)| {
                                let is_selected = idx == selected_release_idx;
                                let size_mb = rel.asset.as_ref().map(|a| a.size as f64 / 1_048_576.0).unwrap_or(0.0);
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {:28}  ({:.1} MB)  Published: {}", rel.name, size_mb, rel.published_at.chars().take(10).collect::<String>())).style(style)
                            })
                            .collect();

                        let target_dir = current_launcher.installation_dir(current_tool);
                        let path_str = target_dir.to_str().unwrap_or("");
                        let list = List::new(items).block(
                            Block::default()
                                .title(Span::styled(
                                    format!(" Releases for {} -> [{}] ({}) ", current_tool.display_name(), path_str, releases.len()),
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                                ))
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(Color::Green)),
                        );
                        frame.render_widget(list, modal_chunks[1]);
                    }
                }
            }

            let help_text = match step {
                0 => " [Up/Down] Select Launcher | [Enter] Continue | [Esc] Close",
                1 => " [Up/Down] Select Tool / Runner | [Enter] Fetch Releases | [Esc] Back",
                _ => " [Up/Down] Select Version | [Enter] Download & Extract | [Esc] Back to Tools",
            };
            let help = Paragraph::new(help_text)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, modal_chunks[2]);
        }

        ModalState::SelectWineRunnerPicker {
            ref installed_runners,
            selected_idx,
            ..
        } => {
            let items: Vec<ListItem> = installed_runners
                .iter()
                .enumerate()
                .map(|(idx, r)| {
                    let is_selected = idx == selected_idx;
                    let kind_badge = match r.kind {
                        game_core::runner_detector::RunnerKind::Proton => "[Proton]",
                        game_core::runner_detector::RunnerKind::Wine => "[Wine]",
                    };
                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(format!("  {:8} {:25} ({})", kind_badge, r.name, r.location.display_name())).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Select Installed Wine / Proton Runner ",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Select Runner | [Enter] Apply to Game | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WineToolsMenu { selected_idx } => {
            let items = [
                ("Open winecfg", "Graphical Wine configuration"),
                ("Open winetricks", "Install Windows libraries"),
                ("Kill Wine processes", "Terminate wineserver"),
                ("Open Prefix folder", "Browse prefix in file manager"),
            ];

            let list_items: Vec<ListItem> = items.iter().enumerate().map(|(idx, (title, desc))| {
                let is_selected = idx == selected_idx;
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(format!("  {}: {} ({})", idx + 1, title, desc)).style(style)
            }).collect();

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(" Wine Tools ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);
            let help = Paragraph::new(" [Up/Down] Select Tool | [Enter] Execute | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::EditCustomArgsInput { ref input, cursor_pos, .. } => {
            let cpos = cursor_pos.min(input.len());
            let avail = 54usize;
            let scroll = if cpos > avail { cpos - avail } else { 0 };
            let end = (scroll + avail * 2).min(input.len());
            let visible = &input[scroll..end];
            let cursor_in_visible = cpos - scroll;
            let cursor_in_visible = cursor_in_visible.min(visible.len());
            let (before, after) = visible.split_at(cursor_in_visible);

            let p = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" Enter Custom Command / Launcher Arguments: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(before),
                    Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(after),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(" Examples: --fullscreen, -dx11, WINEFSYNC=1 ", Style::default().fg(Color::DarkGray)),
                ]),
            ])
            .block(
                Block::default()
                    .title(Span::styled(" Custom Launcher Arguments ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let popup_area = centered_rect(75, 30, frame.area());
            frame.render_widget(Clear, popup_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Enter] Save | [Esc] Cancel | [Left/Right] Move cursor | [Backspace] Delete")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ConfirmDeleteGame {
            ref display_title,
            selected_option,
            ref game_ids,
        } => {
            let popup_area = centered_rect_fixed(60, 8, frame.area());
            frame.render_widget(Clear, popup_area);

            let msg = if game_ids.len() > 1 {
                format!("Are you sure you want to remove {} selected games from your library?", game_ids.len())
            } else {
                format!("Are you sure you want to remove '{}' from your library?", display_title)
            };

            let no_style = if selected_option == 0 {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let yes_style = if selected_option == 1 {
                Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("   [ NO ]   ", no_style),
                    Span::raw("          "),
                    Span::styled("   [ YES, DELETE ]   ", yes_style),
                ]),
            ];

            let block = Paragraph::new(content)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .title(Span::styled(" Confirm Game Deletion ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(1)])
                .split(popup_area);

            frame.render_widget(block, chunks[0]);

            let help = Paragraph::new(" [Left/Right/Tab] Select Option | [Enter] Confirm | [Esc] Cancel")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ConfirmDeleteRunner {
            ref runner_info,
            selected_option,
        } => {
            let popup_area = centered_rect_fixed(60, 8, frame.area());
            frame.render_widget(Clear, popup_area);

            let msg = format!("Are you sure you want to delete the AppImage / executable file for '{}' from disk?", runner_info.name);

            let no_style = if selected_option == 0 {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let yes_style = if selected_option == 1 {
                Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("   [ NO ]   ", no_style),
                    Span::raw("          "),
                    Span::styled("   [ YES, DELETE ]   ", yes_style),
                ]),
            ];

            let block = Paragraph::new(content)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .title(Span::styled(" Confirm Runner Deletion ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(1)])
                .split(popup_area);

            frame.render_widget(block, chunks[0]);

            let help = Paragraph::new(" [Left/Right/Tab] Select Option | [Enter] Confirm | [Esc] Cancel")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ManageRunnersStep2Config {
            ref runner_info,
            ref exe_path_input,
            selected_row,
            selected_action_idx,
            cursor_pos,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Target Emulator: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} ({})", runner_info.name, runner_info.console_initials),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            let path_label_style = if selected_row == 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(vec![
                Span::styled(if selected_row == 0 { "▶ 1. Executable / AppImage Path: " } else { "  1. Executable / AppImage Path: " }, path_label_style),
            ]));

            let (before, after) = exe_path_input.split_at(cursor_pos.min(exe_path_input.len()));
            let path_span = if selected_row == 0 {
                vec![
                    Span::raw("   "),
                    Span::styled(before, Style::default().fg(Color::White).bg(Color::DarkGray)),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]
            } else {
                vec![
                    Span::raw("   "),
                    Span::styled(
                        if exe_path_input.is_empty() { "< No file selected >" } else { exe_path_input },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]
            };
            lines.push(Line::from(path_span));
            lines.push(Line::from(""));

            // Build action buttons list dynamically
            let has_executable = runner_info
                .executable_path
                .as_ref()
                .map(|p| !p.trim().is_empty() && std::path::Path::new(p).exists())
                .unwrap_or(false);

            struct ActionBtn {
                label: &'static str,
                fg: Color,
            }

            let mut btns = vec![
                ActionBtn { label: "[ Browse ]", fg: Color::Cyan },
            ];

            if runner_info.download_url.is_some() {
                btns.push(ActionBtn { label: "[ Download ]", fg: Color::LightBlue });
            }

            btns.push(ActionBtn { label: "[ Save ]", fg: Color::Green });

            if has_executable {
                btns.push(ActionBtn { label: "[ Delete ]", fg: Color::Red });
            }

            let mut actions_line = vec![Span::raw("  ")];

            for (idx, btn) in btns.iter().enumerate() {
                let is_selected = selected_row == 1 && idx == selected_action_idx;
                let btn_style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(btn.fg)
                };

                actions_line.push(Span::styled(btn.label, btn_style));
                actions_line.push(Span::raw("  "));
            }

            lines.push(Line::from(actions_line));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Emulator Options: {} ({}) ", runner_info.name, runner_info.console_initials),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Switch Row | [Left/Right] Select Action / Edit Cursor | [Enter] Confirm | [Esc] Back")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::AddGameForm {
            ref game_type,
            selected_field,
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
            cursor_pos,
        } => {
            let gtype_name = match game_type {
                PlatformType::Emulator => "EMULATOR",
                PlatformType::Native => "LINUX NATIVE",
                PlatformType::Wine => "WINDOWS (WINE)",
                PlatformType::Steam => "STEAM",
            };

            let block_title = format!(" Add Game Details ({}) ", gtype_name);
            let mut lines = Vec::new();

            let field_style = |idx: usize| {
                if idx == selected_field {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let mk_cb = |checked: bool, label: &str, idx: usize| -> Line {
                let mark = if checked { "[X]" } else { "[ ]" };
                let s = field_style(idx);
                Line::from(vec![
                    Span::styled(format!("{}. {} ", idx + 1, mark), s),
                    Span::styled(label.to_string(), field_style(idx)),
                ])
            };

            let title_line = if selected_field == 0 {
                let cpos = cursor_pos.min(title.len());
                let (before, after) = title.split_at(cpos);
                Line::from(vec![
                    Span::styled("1. Title: ", field_style(0)),
                    Span::raw(before),
                    Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(after),
                ])
            } else {
                Line::from(vec![
                    Span::styled("1. Title: ", field_style(0)),
                    Span::raw(title),
                ])
            };

            match game_type {
                PlatformType::Emulator => {
                    let p_name = app.platforms.get(platform_idx).map(|p| p.name.as_str()).unwrap_or("Unknown");
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Platform: ", field_style(1)),
                        Span::styled(format!("< {} >", p_name), field_style(1)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. ROM Path: ", field_style(2)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to select ROM >" } else { file_path }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(6)),
                    ]));
                }
                PlatformType::Native => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to browse >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Working Dir: ", field_style(2)),
                        Span::raw(if working_dir.is_empty() { "< Auto-populated >" } else { working_dir }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Custom Args: ", field_style(3)),
                        Span::raw(if custom_command.is_empty() { "< Optional >" } else { custom_command }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 4));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 5));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 6));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(7)),
                    ]));
                }
                PlatformType::Wine => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable .exe Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to browse .exe >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Prefix: ", field_style(2)),
                        Span::raw(if wine_prefix.is_empty() { "< Auto-created in working folder if empty >" } else { wine_prefix }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Working Dir: ", field_style(3)),
                        Span::raw(if working_dir.is_empty() { "< Auto-populated >" } else { working_dir }),
                    ]));

                    let runner_str = extract_runner_display_name(custom_command);

                    lines.push(Line::from(vec![
                        Span::styled("5. Wine / Proton Runner: ", field_style(4)),
                        Span::styled(format!("< {} >", runner_str), field_style(4)),
                    ]));
                    let flags = extract_custom_flags(custom_command);
                    let flags_display = if flags.is_empty() {
                        "< Optional >".to_string()
                    } else {
                        flags
                    };

                    lines.push(Line::from(vec![
                        Span::styled("6. Custom Args: ", field_style(5)),
                        Span::raw(flags_display),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("-- Wrappers & Toggles --", Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(mk_cb(gamemode, "GameMode", 6));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 7));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 8));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("-- Wine / Proton Options --", Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(mk_cb(esync, "Esync (eventfd sync)", 9));
                    lines.push(mk_cb(fsync, "Fsync (futex2 sync)", 10));
                    lines.push(mk_cb(dxvk, "DXVK Async", 11));
                    lines.push(mk_cb(vkd3d, "VKD3D-Proton Async", 12));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(13)),
                    ]));
                }
                PlatformType::Steam => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::raw(if steam_appid.is_empty() { "< Enter AppID >" } else { steam_appid }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Custom Args: ", field_style(2)),
                        Span::raw(if custom_command.is_empty() { "< Optional >" } else { custom_command }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(6)),
                    ]));
                }
            }

            let form_p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(block_title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(form_p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Navigate Fields | [Enter] Toggle / Select | [Space] Toggle Checkbox | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::EditGameForm {
            ref game_type,
            selected_field,
            ref title,
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
            cursor_pos,
            ..
        } => {
            let gtype_name = match game_type {
                PlatformType::Emulator => "EMULATOR",
                PlatformType::Native => "LINUX NATIVE",
                PlatformType::Wine => "WINDOWS (WINE)",
                PlatformType::Steam => "STEAM",
            };

            let block_title = format!(" Edit Game Details ({}) ", gtype_name);
            let mut lines = Vec::new();

            let field_style = |idx: usize| {
                if idx == selected_field {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let mk_cb = |checked: bool, label: &str, idx: usize| -> Line {
                let mark = if checked { "[X]" } else { "[ ]" };
                let s = field_style(idx);
                Line::from(vec![
                    Span::styled(format!("{}. {} ", idx + 1, mark), s),
                    Span::styled(label.to_string(), field_style(idx)),
                ])
            };

            let title_line = if selected_field == 0 {
                let cpos = cursor_pos.min(title.len());
                let (before, after) = title.split_at(cpos);
                Line::from(vec![
                    Span::styled("1. Title: ", field_style(0)),
                    Span::raw(before),
                    Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(after),
                ])
            } else {
                Line::from(vec![
                    Span::styled("1. Title: ", field_style(0)),
                    Span::raw(title),
                ])
            };

            match game_type {
                PlatformType::Emulator => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. ROM Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to select ROM >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Custom Command / Args: ", field_style(2)),
                        Span::raw(if custom_command.is_empty() { "< Optional >" } else { custom_command }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(6)),
                    ]));
                }
                PlatformType::Native => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to browse >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Working Directory: ", field_style(2)),
                        Span::raw(if working_dir.is_empty() { "< Optional >" } else { working_dir }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Custom Args / Command: ", field_style(3)),
                        Span::raw(if custom_command.is_empty() { "< Optional >" } else { custom_command }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 4));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 5));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 6));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(7)),
                    ]));
                }
                PlatformType::Wine => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable .exe Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [Enter] to browse .exe >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Prefix: ", field_style(2)),
                        Span::raw(if wine_prefix.is_empty() { "< Auto-created in working folder if empty >" } else { wine_prefix }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Working Directory: ", field_style(3)),
                        Span::raw(if working_dir.is_empty() { "< Optional >" } else { working_dir }),
                    ]));

                    let runner_str = extract_runner_display_name(custom_command);

                    lines.push(Line::from(vec![
                        Span::styled("5. Wine / Proton Runner: ", field_style(4)),
                        Span::styled(format!("< {} >", runner_str), field_style(4)),
                    ]));
                    let flags = extract_custom_flags(custom_command);
                    let flags_display = if flags.is_empty() {
                        "< Optional >".to_string()
                    } else {
                        flags
                    };

                    lines.push(Line::from(vec![
                        Span::styled("6. Custom Args: ", field_style(5)),
                        Span::raw(flags_display),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("-- Wrappers & Toggles --", Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(mk_cb(gamemode, "GameMode", 6));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 7));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 8));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("-- Wine / Proton Options --", Style::default().fg(Color::DarkGray)),
                    ]));
                    lines.push(mk_cb(esync, "Esync (eventfd sync)", 9));
                    lines.push(mk_cb(fsync, "Fsync (futex2 sync)", 10));
                    lines.push(mk_cb(dxvk, "DXVK Async", 11));
                    lines.push(mk_cb(vkd3d, "VKD3D-Proton Async", 12));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(13)),
                    ]));
                }
                PlatformType::Steam => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::raw(steam_appid),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Custom Args: ", field_style(2)),
                        Span::raw(if custom_command.is_empty() { "< Optional >" } else { custom_command }),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(6)),
                    ]));
                }
            }

            let form_p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(block_title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(form_p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Navigate Fields | [Enter] Toggle / Select | [Space] Toggle Checkbox | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::PlatformSelector { selected_idx } => {
            let max_name_len = app.platforms.iter().map(|p| p.name.len()).max().unwrap_or(12);
            let needed_w = (max_name_len as u16 + 26).clamp(42, 60).min(frame.area().width.saturating_sub(4));
            let needed_h = (app.platforms.len() as u16 + 2).clamp(4, 16).min(frame.area().height.saturating_sub(2));

            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let inner_width = popup_area.width.saturating_sub(4) as usize;

            let mut items = Vec::new();
            for (idx, p) in app.platforms.iter().enumerate() {
                let is_sel = idx == selected_idx;
                let count = app.db.get_games_for_platform(p.id).map(|g| g.len()).unwrap_or(0);
                let count_str = format!("({} juegos)", count);
                let name_str = format!(" {}", p.name);
                let pad_len = inner_width.saturating_sub(name_str.len() + count_str.len() + 3);
                let padding = " ".repeat(pad_len);

                let line = if is_sel {
                    Line::from(vec![
                        Span::styled(" ▶ ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(name_str, Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::styled(padding, Style::default().bg(Color::Yellow)),
                        Span::styled(count_str, Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw(" "),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled("   ", Style::default().fg(Color::DarkGray)),
                        Span::styled(name_str, Style::default().fg(Color::White)),
                        Span::styled(padding, Style::default().fg(Color::DarkGray)),
                        Span::styled(count_str, Style::default().fg(Color::Cyan)),
                        Span::raw(" "),
                    ])
                };
                items.push(ListItem::new(line));
            }

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(" SELECCIONAR PLATAFORMA ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .title_bottom(Span::styled(" ▲▼ Navegar | ↵ Seleccionar | Esc Cerrar ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            frame.render_widget(list, popup_area);
        }
        ModalState::CheatsheetModal { .. } => {
            let needed_w = 72u16.min(frame.area().width.saturating_sub(4));
            let needed_h = 16u16.min(frame.area().height.saturating_sub(2));
            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let lines = vec![
                Line::from(vec![
                    Span::styled(" [Navigation] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("Up/Down / Mouse Scroll ", Style::default().fg(Color::Cyan)),
                    Span::raw("Browse | "),
                    Span::styled("Tab / p ", Style::default().fg(Color::Cyan)),
                    Span::raw("Switch Consoles"),
                ]),
                Line::from(vec![
                    Span::styled(" [Views]      ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("v ", Style::default().fg(Color::Cyan)),
                    Span::raw("Cycle View (Cards/Banner/Table) | "),
                    Span::styled("Alt+O ", Style::default().fg(Color::Cyan)),
                    Span::raw("Big Picture Mode"),
                ]),
                Line::from(vec![
                    Span::styled(" [Search]     ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("/ ", Style::default().fg(Color::Cyan)),
                    Span::raw("Focus Search Bar | "),
                    Span::styled("Esc ", Style::default().fg(Color::Cyan)),
                    Span::raw("Clear Search Query"),
                ]),
                Line::from(vec![
                    Span::styled(" [Games]      ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("Enter / DblClick ", Style::default().fg(Color::Cyan)),
                    Span::raw("Launch | "),
                    Span::styled("Space ", Style::default().fg(Color::Cyan)),
                    Span::raw("Select | "),
                    Span::styled("Del ", Style::default().fg(Color::Cyan)),
                    Span::raw("Delete Game"),
                ]),
                Line::from(vec![
                    Span::styled(" [Media/ROMs] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("g ", Style::default().fg(Color::Cyan)),
                    Span::raw("Fetch Cover Artwork | "),
                    Span::styled("r ", Style::default().fg(Color::Cyan)),
                    Span::raw("Rescan Platform Folder"),
                ]),
                Line::from(vec![
                    Span::styled(" [System]     ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("m ", Style::default().fg(Color::Cyan)),
                    Span::raw("Emulators/Runners | "),
                    Span::styled("c ", Style::default().fg(Color::Cyan)),
                    Span::raw("Wine/Proton Tools | "),
                    Span::styled("s ", Style::default().fg(Color::Cyan)),
                    Span::raw("Settings"),
                ]),
            ];

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(" KEYBOARD & MOUSE CONTROLS CHEATSHEET ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
                    .title_bottom(Span::styled(" Press [Esc] or [?] to close ", Style::default().fg(Color::DarkGray)))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::FuzzySearchModal { ref query, cursor_pos } => {
            let needed_w = 60u16.min(frame.area().width.saturating_sub(4));
            let needed_h = 5u16;
            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let (before, after) = query.split_at(cursor_pos.min(query.len()));
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(" 🔍 Búsqueda: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(before),
                    Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(after),
                ]),
            ];

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(" 🔍 BÚSQUEDA DIFUSA EN VIVO 🔍 ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)))
                    .title_bottom(Span::styled(" [Enter] Filtrar | [Esc] Limpiar | Escriba para buscar ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::None => {}
    }
}

/// Helper function to center a pop-up dialog box with exact pixel dimensions relative to screen
pub fn centered_rect_exact(width: u16, height: u16, r: Rect) -> Rect {
    let w = width.min(r.width);
    let h = height.min(r.height);
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Helper function to center a pop-up dialog box in percentage relative to screen
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Helper function to center a pop-up dialog box with fixed height in lines and percentage width
fn centered_rect_fixed(percent_x: u16, height_lines: u16, r: Rect) -> Rect {
    let margin = r.height.saturating_sub(height_lines) / 2;
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(margin),
            Constraint::Length(height_lines),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
