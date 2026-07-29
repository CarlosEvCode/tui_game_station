use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::{App, FocusedPane, ModalState};
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

    // Render modal overlay if active
    if app.modal_state != ModalState::None {
        render_modal(frame, app);
    }
}

fn render_header(frame: &mut Frame, area: Rect) {
    let header_text = vec![Line::from(vec![
        Span::styled(
            " 🎮 TUI GAME STATION ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " | Standalone Retro & PC Games Launcher ",
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
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(area);

    render_platforms_list(frame, app, main_chunks[0]);
    render_games_table(frame, app, main_chunks[1]);
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

    let title = format!(" Plataformas ({}) ", app.platforms.len());
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

    let header = Row::new(vec!["Título", "Ext", "Tamaño", "Tipo"])
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
        format!(" Juegos - {} ({}) ", p.name, app.games.len())
    } else {
        " Juegos (0) ".to_string()
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

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let filter_text = if app.show_all_platforms {
        "Todas"
    } else {
        "Solo Activas"
    };

    let help_text = format!(
        " [a] Agregar | [p] Plataformas ({}) | [r] Sync Steam | [Tab] Panel | [Enter] Jugar | [s] Escanear | [q] Salir | Info: {}",
        filter_text, app.status_msg
    );

    let paragraph = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    frame.render_widget(paragraph, area);
}

/// Render centered pop-up modal overlay dialog
fn render_modal(frame: &mut Frame, app: &App) {
    let popup_area = centered_rect(65, 60, frame.area());
    frame.render_widget(Clear, popup_area); // Clear background behind modal

    match app.modal_state {
        ModalState::AddGameStep1Type { selected_type_idx } => {
            let options = vec![
                "🕹️ Emulador (SNES, PS1, PS2, GBA, N64, 3DS...)",
                "🐧 Linux Nativo (Ejecutable binario, Script, AppImage)",
                "🪟 Windows (Wine / Proton .exe)",
                "🚂 Steam (Lanzamiento por Steam AppID)",
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
                        " ➕ Agregar Nuevo Juego - Paso 1: Selecciona el Tipo ",
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

            let help = Paragraph::new(" [↑/↓] Seleccionar | [Enter] Siguiente | [Esc] Cancelar")
                .style(Style::default().fg(Color::DarkGray));
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
                PlatformType::Emulator => "EMULADOR",
                PlatformType::Native => "LINUX NATIVO",
                PlatformType::Wine => "WINDOWS (WINE)",
                PlatformType::Steam => "STEAM",
            };

            let block_title = format!(" ➕ Detalles de Juego ({}) ", gtype_name);
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
                    let p_name = app.platforms.get(platform_idx).map(|p| p.name.as_str()).unwrap_or("Desconocida");
                    lines.push(Line::from(vec![
                        Span::styled("1. Título: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Plataforma: < ", field_style(1)),
                        Span::styled(p_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(" > (Usar ←/→ para cambiar)", field_style(1)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Ruta ROM: ", field_style(2)),
                        Span::raw(file_path),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ GUARDAR JUEGO ]", field_style(3)),
                    ]));
                }
                PlatformType::Native => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Título: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Ruta Ejecutable: ", field_style(1)),
                        Span::raw(file_path),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. Dir. Trabajo: ", field_style(2)),
                        Span::raw(working_dir),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Comando Extra: ", field_style(3)),
                        Span::raw(custom_command),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ GUARDAR JUEGO ]", field_style(4)),
                    ]));
                }
                PlatformType::Wine => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Título: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Ruta .exe: ", field_style(1)),
                        Span::raw(file_path),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("3. WINEPREFIX: ", field_style(2)),
                        Span::raw(wine_prefix),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("4. Dir. Trabajo: ", field_style(3)),
                        Span::raw(working_dir),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ GUARDAR JUEGO ]", field_style(4)),
                    ]));
                }
                PlatformType::Steam => {
                    lines.push(Line::from(vec![
                        Span::styled("1. Título: ", field_style(0)),
                        Span::raw(title),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::raw(steam_appid),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("[ GUARDAR JUEGO ]", field_style(2)),
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

            let help = Paragraph::new(" [Tab/Shift+Tab] Cambiar Campo | [Escribir] Texto | [Enter] Guardar | [Esc] Cancelar")
                .style(Style::default().fg(Color::DarkGray));
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
