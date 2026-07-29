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
        ViewMode::Grid => render_games_grid(frame, app, main_chunks[1]),
    }
}

fn render_platforms_list(frame: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.focused_pane == FocusedPane::Platforms && app.modal_state == ModalState::None {
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
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let type_badge = match p.platform_type {
                PlatformType::Emulator => " [EMU]",
                PlatformType::Native => " [NAT]",
                PlatformType::Wine => " [WIN]",
                PlatformType::Steam => " [STM]",
            };

            let content = format!(" {} {}", p.name, type_badge);
            ListItem::new(content).style(style)
        })
        .collect();

    let title = format!(" Platforms ({}) ", app.platforms.len());
    let list_widget = List::new(items).block(
        Block::default()
            .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = ListState::default();
    state.select(Some(app.selected_platform_idx));

    frame.render_stateful_widget(list_widget, area, &mut state);
}

fn render_games_table(frame: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.focused_pane == FocusedPane::Games && app.modal_state == ModalState::None {
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
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let ext = g.file_extension.clone().unwrap_or_else(|| "-".to_string());
            let size_mb = g.file_size.map(|s| format!("{:.1} MB", s as f64 / (1024.0 * 1024.0))).unwrap_or_else(|| "-".to_string());
            let gtype = g.game_type.to_uppercase();

            Row::new(vec![g.title.clone(), ext, size_mb, gtype]).style(style)
        })
        .collect();

    let title = if let Some(p) = app.platforms.get(app.selected_platform_idx) {
        format!(" Games Table - {} ({}) [v] Switch View ", p.name, app.games.len())
    } else {
        " Games (0) ".to_string()
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
            .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    let mut state = TableState::default();
    if !app.games.is_empty() {
        state.select(Some(app.selected_game_idx));
    }

    frame.render_stateful_widget(table, area, &mut state);
}

/// Render Lutris-style Cards / Grid View with Vertical Cover & Metadata Layout
fn render_games_grid(frame: &mut Frame, app: &mut App, area: Rect) {
    let border_color = if app.focused_pane == FocusedPane::Games && app.modal_state == ModalState::None {
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
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let appid_info = g.steam_appid.map(|id| format!(" (AppID: {})", id)).unwrap_or_default();
            let content = format!(" [{}] {}{}", g.game_type.to_uppercase(), g.title, appid_info);

            ListItem::new(content).style(style)
        })
        .collect();

    let title = if let Some(p) = app.platforms.get(app.selected_platform_idx) {
        format!(" Cards View - {} ({}) [v] Switch View ", p.name, app.games.len())
    } else {
        " Cards View (0) ".to_string()
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

    // Right Column: Vertical Cover Image Card (Top) & Metadata Panel (Bottom)
    render_game_cover_card(frame, app, grid_chunks[1]);
}

fn render_game_cover_card(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.games.is_empty() || app.selected_game_idx >= app.games.len() {
        let empty_p = Paragraph::new("No game selected").block(
            Block::default()
                .title(" Cover & Details ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        );
        frame.render_widget(empty_p, area);
        return;
    }

    let game = &app.games[app.selected_game_idx];
    let game_id = game.id;
    let current_platform = app.platforms.get(app.selected_platform_idx);

    // Vertical layout inside the right card pane:
    let card_vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // 1. Render Cover Image Box
    let cover_block = Block::default()
        .title(Span::styled(
            format!(" Cover - {} ", game.title),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner_cover_area = cover_block.inner(card_vertical_chunks[0]);
    frame.render_widget(cover_block, card_vertical_chunks[0]);

    if let Some(protocol) = app.image_protocols.get_mut(&game_id) {
        let image_widget = StatefulImage::new(None);
        frame.render_stateful_widget(image_widget, inner_cover_area, protocol);
    } else {
        let placeholder = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  [ Fetching Cover Image... ]", Style::default().fg(Color::Yellow))),
        ]);
        frame.render_widget(placeholder, inner_cover_area);
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
    let filter_text = if app.show_all_platforms {
        "All"
    } else {
        "Active Only"
    };

    let help_text = format!(
        " [v] View (Grid/Table) | [m] Runners | [a] Add Game / Scan ROMs | [f] Folder Picker | [p] Filter ({}) | [Enter] Launch | {}",
        filter_text, app.status_msg
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
fn render_modal(frame: &mut Frame, app: &App) {
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
            let all_platforms = app.db.get_platforms().unwrap_or_default();
            let active_ids: Vec<i64> = app.platforms.iter().map(|p| p.id).collect();

            let items: Vec<ListItem> = all_platforms
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let is_selected = idx == selected_platform_idx;
                    let is_active = active_ids.contains(&p.id);

                    let status_badge = if is_active {
                        " [Active / Configured]"
                    } else {
                        " [Unconfigured]"
                    };

                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else if is_active {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    ListItem::new(format!("  {} ({}){}", p.name, p.slug, status_badge)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Scan ROMs Folder - Step 2: Select Target Platform ",
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
        ModalState::ManageRunnersStep1Platform { selected_platform_idx } => {
            let all_platforms = app.db.get_platforms().unwrap_or_default();
            let active_ids: Vec<i64> = app.platforms.iter().map(|p| p.id).collect();

            let items: Vec<ListItem> = all_platforms
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let is_selected = idx == selected_platform_idx;
                    let is_active = active_ids.contains(&p.id);

                    let status_badge = if is_active {
                        " [Active / Configured]"
                    } else {
                        " [Unconfigured]"
                    };

                    let style = if is_selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else if is_active {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    ListItem::new(format!("  {} ({}){}", p.name, p.slug, status_badge)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Runner Management - Select Platform ",
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

            let help = Paragraph::new(" [Up/Down] Navigate | [Enter] Configure Runner | [Esc] Back")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ManageRunnersStep2Config {
            ref platform,
            ref runners,
            selected_runner_idx,
            ref exe_path_input,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Target Platform: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&platform.name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("1. Available Runners (Use Left/Right to switch):", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));

            let mut current_runner_has_download = false;
            let mut is_downloaded = false;

            for (idx, r) in runners.iter().enumerate() {
                let is_sel = idx == selected_runner_idx;
                let mark = if is_sel { " -> [x] " } else { "    [ ] " };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let configured_info = if let Some(exe) = &r.executable_path {
                    format!(" (Path: {})", exe)
                } else {
                    String::new()
                };

                if is_sel {
                    if r.download_url.is_some() {
                        current_runner_has_download = true;
                    }
                    if let Some(exe) = &r.executable_path {
                        if std::path::Path::new(exe).exists() {
                            is_downloaded = true;
                        }
                    }
                }

                lines.push(Line::from(vec![
                    Span::styled(mark, style),
                    Span::styled(format!("{}{}", r.name, configured_info), style),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("2. Executable / .AppImage Path:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
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

            if current_runner_has_download {
                actions_line.push(Span::styled("[w] Download AppImage", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)));
                actions_line.push(Span::raw("  "));
            }

            if is_downloaded {
                actions_line.push(Span::styled("[x] Delete from Disk", Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)));
                actions_line.push(Span::raw("  "));
            }

            actions_line.push(Span::styled("[d] Deactivate", Style::default().fg(Color::Red)));

            lines.push(Line::from(actions_line));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Runner Options: {} ", platform.name),
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
