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
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Footer / Status bar
        ])
        .split(frame.area());

    render_header(frame, chunks[0]);
    render_main_content(frame, app, chunks[1]);
    render_footer(frame, app, chunks[2]);

    if app.modal_state != ModalState::None {
        render_modal(frame, app);
    }

    if let Some(ref progress) = app.download_progress {
        if !progress.is_finished {
            render_download_gauge(frame, progress);
        }
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

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = format!(
        " [v] View | [w] Media | [m] Runners | [s] Settings | [a] Add/Scan | [r] Rescan | [g] Fetch Media | [Space] Select | [Del/x] Delete | [Enter] Launch | {}",
        app.status_msg
    );

    let paragraph = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(paragraph, area);
}

/// Render sleek compact download progress popup overlay
fn render_download_gauge(frame: &mut Frame, progress: &crate::app::DownloadProgressState) {
    let popup_area = centered_rect(65, 12, frame.area());
    frame.render_widget(Clear, popup_area);

    let downloaded_mb = progress.downloaded_bytes as f64 / (1024.0 * 1024.0);
    let total_mb = progress.total_bytes as f64 / (1024.0 * 1024.0);
    let label = format!("{:.1}% ({:.1} MB / {:.1} MB)", progress.percentage, downloaded_mb, total_mb);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(Span::styled(
                    format!(" Downloading: {} ", progress.runner_name),
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

    frame.render_widget(gauge, popup_area);
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
            is_searching,
            ref candidates,
            selected_candidate_idx,
            ref covers,
            selected_cover_idx,
            ref banners,
            selected_banner_idx,
            ref icons,
            selected_icon_idx,
            ..
        } => {
            let tab_titles = vec![
                format!("1. Candidates ({})", candidates.len()),
                format!("2. Covers ({})", covers.len()),
                format!("3. Banners ({})", banners.len()),
                format!("4. Icons ({})", icons.len()),
            ];

            let tab_spans: Vec<Span> = tab_titles
                .iter()
                .enumerate()
                .map(|(idx, title)| {
                    if idx == active_tab {
                        Span::styled(format!(" [ {} ] ", title), Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled(format!("   {}   ", title), Style::default().fg(Color::Gray))
                    }
                })
                .collect();

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(8), Constraint::Length(2)])
                .split(popup_area);

            let tabs_line = Line::from(tab_spans);
            frame.render_widget(Paragraph::new(tabs_line), modal_chunks[0]);

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

                    let search_p = Paragraph::new(format!(" {}", search_query)).block(
                        Block::default()
                            .title(Span::styled(
                                " Search Query (Type name & press [s] or [Enter] to Search) ",
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    );
                    frame.render_widget(search_p, cand_chunks[0]);

                    let items: Vec<ListItem> = if is_searching {
                        vec![ListItem::new(" [ Searching SteamGridDB... ]").style(Style::default().fg(Color::Yellow))]
                    } else if candidates.is_empty() {
                        vec![ListItem::new(" No candidates found. Type a custom name above and press [s] / [Enter] to Search.").style(Style::default().fg(Color::Red))]
                    } else {
                        candidates
                            .iter()
                            .enumerate()
                            .map(|(idx, cand)| {
                                let is_selected = idx == selected_candidate_idx;
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {} (SGDB ID: {})", cand.name, cand.id)).style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                format!(" Candidates for '{}' ({}) ", game_title, candidates.len()),
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
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
                                let is_selected = idx == selected_cover_idx;
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  Cover #{} - ID: {}", idx + 1, c.id)).style(style)
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
                            .border_style(Style::default().fg(Color::Green)),
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
                                let is_selected = idx == selected_banner_idx;
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  Banner #{} - ID: {}", idx + 1, b.id)).style(style)
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
                            .border_style(Style::default().fg(Color::Green)),
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
                                let is_selected = idx == selected_icon_idx;
                                let style = if is_selected {
                                    Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  Icon #{} - ID: {}", idx + 1, ic.id)).style(style)
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
                            .border_style(Style::default().fg(Color::Green)),
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

            let help_str = match active_tab {
                0 => " [Type Query] Edit Name | [s] Search SteamGridDB | [Up/Down] Select Candidate | [Enter] Load Candidate Media | [Esc] Close",
                _ => " [Up/Down] Preview Image | [Enter] Apply Image | [Tab] Switch Tab | [Esc] Close",
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

            let help = Paragraph::new(" [Up/Down] Navigate | [Enter] Configure Emulator | [Esc] Back")
                .style(Style::default().fg(Color::DarkGray));
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

            match game_type {
                PlatformType::Emulator => {
                    let p_name = app.platforms.get(platform_idx).map(|p| p.name.as_str()).unwrap_or("Unknown");
                    lines.push(Line::from(vec![
                        Span::styled("1. Title: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Platform: < ", field_style(1)),
                        Span::styled(p_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(" > (Use Left/Right to change)", field_style(1)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. ROM Path: ", field_style(2)),
                        Span::raw(if file_path.is_empty() { "< Press [f] to select ROM >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(3)),
                    ]));
                }
                PlatformType::Native => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Title: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [f] to browse >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Working Dir: ", field_style(2)),
                        Span::raw(working_dir),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Custom Args: ", field_style(3)),
                        Span::raw(custom_command),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(4)),
                    ]));
                }
                PlatformType::Wine => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Title: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Executable .exe Path: ", field_style(1)),
                        Span::raw(if file_path.is_empty() { "< Press [f] to browse .exe >" } else { file_path }),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. WINEPREFIX: ", field_style(2)),
                        Span::raw(wine_prefix),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Working Dir: ", field_style(3)),
                        Span::raw(working_dir),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ SAVE GAME ]", field_style(4)),
                    ]));
                }
                PlatformType::Steam => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Title: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::raw(steam_appid),
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

            let help = Paragraph::new(" [f] File Picker | [Tab/Shift+Tab] Field | [Enter] Save | [Esc] Cancel")
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
