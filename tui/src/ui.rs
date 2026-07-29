use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame,
};
use ratatui_image::StatefulImage;

use crate::app::{App, FocusedPane, ModalState, ViewMode};
use game_core::models::PlatformType;

pub fn render_ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(8),    // Content
            Constraint::Length(3), // Activity & Status Bar (Log + Download Slider)
            Constraint::Length(3), // Shortcuts & Controls Footer
        ])
        .split(frame.area());

    render_header(frame, chunks[0]);
    render_main_content(frame, app, chunks[1]);
    render_activity_status_bar(frame, app, chunks[2]);
    render_controls_footer(frame, app, chunks[3]);

    if app.modal_state != ModalState::None {
        render_modal(frame, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect) {
    let header_text = vec![Line::from(vec![
        Span::styled(
            " TUI GAME STATION ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "| Standalone Games Launcher ",
            Style::default().fg(Color::DarkGray),
        ),
    ])];

    let header_paragraph = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(header_paragraph, area);
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
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = ListState::default();
    state.select(Some(app.selected_platform_idx));

    frame.render_stateful_widget(list_widget, area, &mut state);
}

fn render_games_table(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Games && app.modal_state == ModalState::None;
    let border_color = if is_focused {
        Color::Yellow
    } else {
        Color::Gray
    };

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
            let gtype = g.game_type.to_uppercase();

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
            let content = format!("{}{}{}[{}] {}{}", pointer, mark, if is_checked { "" } else { "" }, g.game_type.to_uppercase(), g.title, appid_info);

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
                .border_style(Style::default().fg(Color::Gray)),
        );
        frame.render_widget(empty_p, area);
        return;
    }

    let game = &app.games[app.selected_game_idx];
    let game_id = game.id;
    let current_platform = app.platforms.get(app.selected_platform_idx);

    let (media_type, title_prefix, top_percentage) = match app.view_mode {
        ViewMode::CoverCard => ("cover", "Cover", 55),
        ViewMode::BannerCard => ("banner", "Hero Banner", 35),
        ViewMode::IconCard => ("icon", "Icon", 25),
        ViewMode::Table => ("cover", "Cover", 55),
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
        .border_style(Style::default().fg(Color::Cyan));

    let inner_cover_area = cover_block.inner(card_vertical_chunks[0]);
    frame.render_widget(Clear, card_vertical_chunks[0]);
    frame.render_widget(cover_block, card_vertical_chunks[0]);

    if let Some(protocol) = app.media_protocols.get_mut(&(game_id, media_type.to_string())) {
        let image_widget = StatefulImage::new(None);
        frame.render_stateful_widget(image_widget, inner_cover_area, protocol);
    } else {
        render_cover_placeholder(frame, app, game_id, media_type, inner_cover_area);
    }

    // 2. Render Game Details Panel
    let mut details_lines = Vec::new();

    details_lines.push(Line::from(vec![
        Span::styled("Title: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(&game.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]));

    if let Some(p) = current_platform {
        details_lines.push(Line::from(vec![
            Span::styled("Platform: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&p.name, Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
            Span::styled(game.game_type.to_uppercase(), Style::default().fg(Color::Green)),
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
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(status_paragraph, bar_chunks[0]);

    if let Some(ref progress) = app.download_progress {
        let downloaded_mb = progress.downloaded_bytes as f64 / (1024.0 * 1024.0);
        let total_mb = progress.total_bytes as f64 / (1024.0 * 1024.0);
        let is_extracting = progress.percentage >= 99.9;
        let prefix = if is_extracting {
            " Extracting Archive: "
        } else {
            " Downloading Archive: "
        };
        let label = format!("{:.1}% ({:.1}/{:.1} MB)", progress.percentage, downloaded_mb, total_mb);

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("{}{}", prefix, progress.runner_name),
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .gauge_style(
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(progress.percentage as u16)
            .label(label);

        frame.render_widget(gauge, bar_chunks[1]);
    } else {
        let idle_paragraph = Paragraph::new(" [ Download / Extraction Progress: Idle ]")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .title(Span::styled(" Task Slider ", Style::default().fg(Color::DarkGray)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(idle_paragraph, bar_chunks[1]);
    }
}

fn render_controls_footer(frame: &mut Frame, _app: &App, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" [v] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("View "),
        Span::styled(" [w] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Media "),
        Span::styled(" [e] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Edit "),
        Span::styled(" [m] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Runners "),
        Span::styled(" [a] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Add/Scan "),
        Span::styled(" [s] ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Settings "),
        Span::styled(" [Space] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("Select "),
        Span::styled(" [Del] ", Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("Delete "),
        Span::styled(" [Enter] ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("Launch Game"),
    ]);

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(paragraph, area);
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
    frame.render_widget(Clear, popup_area);

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
                Span::styled("Status: [Runner Not Configured - Configure in [m]]", Style::default().fg(Color::Red))
            };
            lines.push(Line::from(runner_status));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("1. Folder Path: ", field_style(0)),
                Span::raw(if folder_path.is_empty() { "< Press [f] to select folder >" } else { folder_path }),
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

            lines.push(Line::from(vec![
                Span::styled("[ START SCANNING ROMS ]", field_style(3)),
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

            let help = Paragraph::new(" [f] System Folder Picker | [Space] Toggle Subfolders | [Enter] Start Scan | [Esc] Back")
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
        ModalState::AppSettings { ref api_key_input, .. } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled("Application Settings", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("SteamGridDB API Key: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    if api_key_input.is_empty() { "< No API Key Configured >" } else { api_key_input },
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from("  * You can get your SteamGridDB key at: https://www.steamgriddb.com/profile/api"));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("[ SAVE SETTINGS ]", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " App Settings & Configuration ",
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

            let help = Paragraph::new(" [Typing] Edit API Key | [Enter] Save | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
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
        ModalState::ManageRunnersStep1Platform { selected_runner_idx } => {
            let unique_runners = app.db.get_unique_runners().unwrap_or_default();

            let items: Vec<ListItem> = unique_runners
                .iter()
                .enumerate()
                .map(|(idx, r)| {
                    let is_selected = idx == selected_runner_idx;
                    let status_badge = if r.is_configured {
                        " [Active / Configured]"
                    } else {
                        " [Unconfigured]"
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
            selected_repo,
            ref releases,
            selected_release_idx,
            selected_target_idx,
            is_loading,
            download_event: _,
        } => {
            let repo_spans = vec![
                if selected_repo == scraper::proton::ProtonRepo::GEProton {
                    Span::styled(" [ 1. GE-Proton (GloriousEggroll) ] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("   1. GE-Proton (GloriousEggroll)   ", Style::default().fg(Color::Gray))
                },
                if selected_repo == scraper::proton::ProtonRepo::ProtonCachyOS {
                    Span::styled(" [ 2. Proton-CachyOS (CachyOS) ] ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("   2. Proton-CachyOS (CachyOS)   ", Style::default().fg(Color::Gray))
                },
            ];

            let targets = [
                "1. TUI Game Station",
                "2. Steam (compatibilitytools.d)",
                "3. Heroic Proton",
                "4. Heroic Wine",
                "5. Lutris Wine",
            ];
            let target_spans: Vec<Span> = targets
                .iter()
                .enumerate()
                .map(|(idx, t)| {
                    if idx == selected_target_idx {
                        Span::styled(format!(" [{}] ", t), Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled(format!("  {}  ", t), Style::default().fg(Color::Gray))
                    }
                })
                .collect();

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Length(2), Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(Paragraph::new(Line::from(repo_spans)), modal_chunks[0]);
            frame.render_widget(Paragraph::new(Line::from(target_spans)), modal_chunks[1]);

            if is_loading {
                let loading_p = Paragraph::new("\n  [ Fetching releases from GitHub API... ]")
                    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                frame.render_widget(loading_p, modal_chunks[2]);
            } else {
                let items: Vec<ListItem> = if releases.is_empty() {
                    vec![ListItem::new(" No downloadable releases found for this repository.").style(Style::default().fg(Color::Red))]
                } else {
                    releases
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
                            ListItem::new(format!("  {:25}  ({:.1} MB)  Published: {}", rel.name, size_mb, rel.published_at.chars().take(10).collect::<String>())).style(style)
                        })
                        .collect()
                };

                let list = List::new(items).block(
                    Block::default()
                        .title(Span::styled(
                            format!(" Available Releases for {} ({}) ", selected_repo.display_name(), releases.len()),
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Green)),
                );
                frame.render_widget(list, modal_chunks[2]);
            }

            let help = Paragraph::new(" [Tab] Switch Repo | [Left/Right] Select Target Location | [Up/Down] Select Release | [Enter] Download & Extract | [Esc] Close")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, modal_chunks[3]);
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
        ModalState::EditCustomArgsInput { ref input, .. } => {
            let p = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" Enter Custom Command / Launcher Arguments: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(input),
                    Span::styled("█", Style::default().fg(Color::Yellow)),
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

            let popup_area = centered_rect(65, 30, frame.area());
            frame.render_widget(Clear, popup_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Enter] Save Arguments | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ManageRunnersStep2Config {
            ref runner_info,
            ref exe_path_input,
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
            lines.push(Line::from(Span::styled(
                "1. Executable / .AppImage Path:",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    if exe_path_input.is_empty() { "< Press [f] to browse file or [w] to download >" } else { exe_path_input },
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(""));

            let mut actions_line = vec![
                Span::styled("[ SAVE RUNNER ]", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
            ];

            if runner_info.download_url.is_some() {
                actions_line.push(Span::styled("[w] Download AppImage", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)));
                actions_line.push(Span::raw("  "));
            }

            let is_downloaded = runner_info
                .executable_path
                .as_ref()
                .map(|p| std::path::Path::new(p).exists())
                .unwrap_or(false);

            if is_downloaded {
                actions_line.push(Span::styled("[x] Delete from Disk", Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)));
                actions_line.push(Span::raw("  "));
            }

            actions_line.push(Span::styled("[d] Deactivate", Style::default().fg(Color::Red)));

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

            let help = Paragraph::new(" [w] Download | [x] Delete | [f] File Picker | [Enter] Save | [Esc] Back")
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
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(3)),
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
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(4)),
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
                        Span::styled("[ SAVE GAME ]", field_style(6)),
                    ]));
                }
                PlatformType::Steam => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::raw(if steam_appid.is_empty() { "< Enter AppID >" } else { steam_appid }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(2)),
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

            let help = Paragraph::new(" [Up/Down] Navigate Fields | [Left/Right] Move Cursor / Switch Runner | [Enter] Select / Save | [Esc] Cancel")
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
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(3)),
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
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(4)),
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
                        Span::styled("[ SAVE CHANGES ]", field_style(6)),
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
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE CHANGES ]", field_style(3)),
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

            let help = Paragraph::new(" [Up/Down] Navigate Fields | [Left/Right] Move Cursor / Switch Runner | [Enter] Select / Save | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::None => {}
    }
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
