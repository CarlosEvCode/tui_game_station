use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table,
        TableState, Wrap,
    },
    Frame,
};
use ratatui_image::{Resize, StatefulImage};

use crate::app::{
    add_scan_available_cores, build_scan_folder_items, folder_has_core,
    scan_folder_add_action_index, scan_folder_add_core_idx, scan_folder_add_emu_idx,
    scan_folder_add_has_core, scan_folder_platform_has_retroarch, scan_folder_supports_dat, App,
    BigPictureFocus, FocusedPane, MediaFocus, ModalState, QuotaInfo, ScanFolderItem, ViewMode,
};
use game_core::models::PlatformType;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const DETAIL_ACTIONS: [&str; 5] = ["Play", "Favorite", "Scraper", "Options", "Delete"];

/// Byte offset in `s` where the display column reaches `col`, snapping to the
/// nearest char boundary (never cutting a multi-byte / wide char in half).
fn byte_off_at_col(s: &str, col: usize) -> usize {
    let mut w = 0usize;
    for (i, ch) in s.char_indices() {
        if w >= col {
            return i;
        }
        w += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    s.len()
}

/// Truncate `path` so it fits in `max_cols` columns, keeping the tail (the most
/// useful part of a path: the file/folder name) and a leading "…" marker when
/// something was cut off. Returns `path` unchanged when it already fits.
pub fn ellipsize_path_tail(path: &str, max_cols: usize) -> String {
    if path.width() <= max_cols {
        return path.to_string();
    }
    let budget = max_cols.saturating_sub(1);
    let start = path.width().saturating_sub(budget);
    let start_byte = byte_off_at_col(path, start);
    format!("…{}", &path[start_byte..])
}

/// Compute the visible window of an editable single-line input that must be
/// rendered inside `max_cols` columns. Returns `(before, after, show_left, show_right)`
/// where `before`/`after` are the text slices around the cursor and the booleans
/// tell the caller to prepend/append a "…" marker because there is more content
/// off-screen. The window follows the cursor so it is always visible.
pub fn editable_input_window(
    input: &str,
    cursor: usize,
    max_cols: usize,
) -> (String, String, bool, bool) {
    let cursor = cursor.min(input.len());
    let total = input.width();
    if total <= max_cols {
        return (
            input[..cursor].to_string(),
            input[cursor..].to_string(),
            false,
            false,
        );
    }
    let ccol = input[..cursor].width();
    let ideal_start = ccol.saturating_sub(max_cols / 3);
    let start_col = ideal_start.min(total.saturating_sub(max_cols));
    let end_col = start_col + max_cols;
    let start_byte = byte_off_at_col(input, start_col);
    let end_byte = byte_off_at_col(input, end_col.min(total));
    (
        input[start_byte..cursor].to_string(),
        input[cursor..end_byte].to_string(),
        start_col > 0,
        end_col < total,
    )
}

/// Keep `budget` columns available for a path inside a row that also uses
/// `other_cols` columns; never lets the path shrink below a readable minimum.
fn path_budget(area_cols: usize, other_cols: usize) -> usize {
    area_cols.saturating_sub(other_cols).max(12)
}

/// Build the spans of an editable single-line input rendered inside a
/// `max_cols`-wide area. When `active` the █ cursor is shown and the window
/// follows it (with "…" markers when content is cut off); otherwise the value
/// is shown tail-ellipsized in a muted style.
fn field_input_spans(
    text: &str,
    cursor: usize,
    max_cols: usize,
    active: bool,
) -> Vec<Span<'static>> {
    if active {
        let (before, after, l, r) = editable_input_window(text, cursor, max_cols);
        vec![
            Span::styled(
                format!("{}{}", if l { "…" } else { "" }, before),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
            Span::styled(
                format!("{}{}", after, if r { "…" } else { "" }),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
        ]
    } else {
        vec![Span::styled(
            ellipsize_path_tail(text, max_cols),
            Style::default().fg(Color::DarkGray),
        )]
    }
}

pub struct ShortcutItem<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub key_color: Color,
}

pub fn render_shortcut_footer(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    items: &[ShortcutItem],
    alignment: Alignment,
) {
    let mut spans = Vec::new();

    if let crate::app::InputSource::Gamepad(ref pad_name) = app.active_input_source {
        spans.push(Span::styled(" 🎮 ", Style::default().fg(Color::Yellow)));
        spans.push(Span::styled(
            format!("{} ", pad_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("│ "));
    }

    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("│ "));
        }
        spans.push(Span::styled(
            format!(" [{}] ", item.key),
            Style::default()
                .fg(item.key_color)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("{} ", item.label),
            Style::default().fg(Color::Gray),
        ));
    }

    let paragraph = Paragraph::new(Line::from(spans))
        .alignment(alignment)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    frame.render_widget(paragraph, area);
}

/// Render a game-form path/text row: `label` followed by the editable value
/// limited to the width remaining in the modal. `placeholder` is shown while
/// the field is empty.
fn form_text_row(
    label: &str,
    text: &str,
    placeholder: &str,
    cursor: usize,
    max_cols: usize,
    selected: bool,
    style: Style,
) -> Line<'static> {
    let label_w = label.width();
    let budget = max_cols.saturating_sub(label_w).max(12);
    let mut spans = vec![Span::styled(label.to_string(), style)];
    if text.is_empty() {
        spans.push(Span::styled(
            placeholder.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.extend(field_input_spans(text, cursor, budget, selected));
    }
    Line::from(spans)
}

pub fn render_ui(frame: &mut Frame, app: &mut App) {
    if app.is_big_picture {
        if app.big_picture_in_detail {
            render_game_detail_view(frame, app, frame.area());
        } else {
            render_big_picture_mode(frame, app, frame.area());
        }
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
    let is_search_focused =
        app.focused_pane == FocusedPane::Search && app.modal_state == ModalState::None;
    let border_color = if is_search_focused {
        Color::Yellow
    } else if !app.search_query.is_empty() {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title_span = if is_search_focused {
        Span::styled(
            " Search (Typing...) ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" Search ", Style::default().fg(Color::DarkGray))
    };

    let search_content = if !app.search_query.is_empty() {
        let cursor = if is_search_focused { "█" } else { "" };
        vec![Line::from(vec![
            Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &app.search_query,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                cursor,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} games found across all platforms)", app.games.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])]
    } else if is_search_focused {
        vec![Line::from(vec![
            Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " (Type to search across all platforms...)",
                Style::default().fg(Color::DarkGray),
            ),
        ])]
    } else {
        vec![Line::from(vec![Span::styled(
            "   [Type '/' or click to search games across all platforms...]",
            Style::default().fg(Color::DarkGray),
        )])]
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
    let is_focused =
        app.focused_pane == FocusedPane::Platforms && app.modal_state == ModalState::None;
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

            let pointer = if is_selected && is_focused {
                "▶ "
            } else if is_selected {
                "► "
            } else {
                "  "
            };

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
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );

    // Leave room at the bottom for the "Emulador Activo" selector box.
    let has_core = app
        .active_emulator_selector_info()
        .as_ref()
        .is_some_and(|(_, core)| core.is_some());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(if has_core { 4 } else { 3 }),
        ])
        .split(area);

    let mut state = ListState::default();
    state.select(Some(app.selected_platform_idx));

    frame.render_stateful_widget(list_widget, chunks[0], &mut state);
    render_active_emulator_selector(frame, app, chunks[1]);
}

/// The "Emulador Activo (◀ ▶)" box under the platform list. Cycles through the
/// configured emulators of the selected platform; when the active emulator is
/// core-based it shows the nested "Núcleo" row instead.
fn render_active_emulator_selector(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused =
        app.focused_pane == FocusedPane::Platforms && app.modal_state == ModalState::None;
    let border_color = if is_focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let mut lines = Vec::new();
    match app.active_emulator_selector_info() {
        Some((name, core)) => {
            lines.push(Line::from(vec![
                Span::styled("Emulador: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "◀ ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " ▶",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            if let Some(core_label) = core {
                lines.push(Line::from(vec![
                    Span::styled("Núcleo:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "◀ ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        core_label,
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " ▶",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                "Sin emulador para esta plataforma [m]",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                " Emulador ◀ ▶ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(paragraph, area);
}

fn get_game_type_badge(
    game: &game_core::models::Game,
    platforms: &[game_core::models::Platform],
) -> String {
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
    let border_color = if is_focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let header = Row::new(vec!["Title", "Ext", "Size", "Type"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

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
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let pointer = if is_selected && is_focused {
                "▶ "
            } else if is_selected {
                "► "
            } else {
                ""
            };
            let mark = if is_checked { "[x] " } else { "" };
            let override_tag = if g.emulator_override.is_some() {
                " ⚙"
            } else {
                ""
            };
            let title = format!("{}{}{}{}", pointer, mark, g.title, override_tag);
            let ext = g.file_extension.clone().unwrap_or_else(|| "-".to_string());
            let size_mb = g
                .file_size
                .map(|s| format!("{:.1} MB", s as f64 / (1024.0 * 1024.0)))
                .unwrap_or_else(|| "-".to_string());
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
        format!(
            " Games Table - {}{}{} [v] Switch View ",
            p.name, sel_title, focus_badge
        )
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
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let pointer = if is_selected && is_focused {
                "▶ "
            } else if is_selected {
                "► "
            } else {
                "  "
            };
            let mark = if is_checked { "[x] " } else { "" };
            let appid_info = g
                .steam_appid
                .map(|id| format!(" (AppID: {})", id))
                .unwrap_or_default();
            let gtype = get_game_type_badge(g, &app.platforms);
            let override_tag = if g.emulator_override.is_some() {
                " ⚙"
            } else {
                ""
            };
            let content = format!(
                "{}{}[{}] {}{}{}",
                pointer, mark, gtype, g.title, override_tag, appid_info
            );

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
        format!(
            " Mode: {} - {}{}{} [v] Cycle View ",
            mode_name, p.name, sel_title, focus_badge
        )
    } else {
        format!(" Mode: {} (0){} ", mode_name, focus_badge)
    };

    let list_widget = List::new(items).block(
        Block::default()
            .title(Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            ))
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

fn render_cover_placeholder(
    frame: &mut Frame,
    app: &App,
    game_id: i64,
    media_type: &str,
    area: Rect,
) {
    frame.render_widget(Clear, area);
    let cover_status = app.db.get_media_status(game_id, media_type).ok().flatten();
    let label = match media_type {
        "banner" => "Banner",
        "icon" => "Icon",
        _ => "Cover",
    };
    let is_fetching = app.download_progress.is_some();
    let msg = match (cover_status.as_deref(), is_fetching) {
        (Some("not_found"), _) => format!("  [ No {} Found ]", label),
        (_, true) => format!("  [ Fetching {}... ]", label),
        _ => format!("  [ No {} Found ]", label),
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
        .constraints([
            Constraint::Percentage(top_percentage),
            Constraint::Percentage(100 - top_percentage),
        ])
        .split(area);

    let cover_block = Block::default()
        .title(Span::styled(
            format!(" {} - {} ", title_prefix, game.title),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
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

    if let Some(protocol) = app
        .media_protocols
        .get_mut(&(game_id, media_type.to_string()))
    {
        let image_widget = StatefulImage::new(Some(image::Rgb([18, 20, 26])));
        frame.render_stateful_widget(image_widget, centered_media_rect, protocol);
    } else {
        render_cover_placeholder(frame, app, game_id, media_type, centered_media_rect);
    }

    // 2. Render Game Details Panel
    let mut details_lines = Vec::new();

    // Title line
    details_lines.push(Line::from(vec![
        Span::styled(
            "Title: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            &game.title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Platform & Game Type line
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

    // Release Year, Rating, Genre
    let mut meta_spans = Vec::new();

    if let Some(year) = game.release_year {
        meta_spans.push(Span::styled("Year: ", Style::default().fg(Color::DarkGray)));
        meta_spans.push(Span::styled(
            year.to_string(),
            Style::default().fg(Color::Magenta),
        ));
        meta_spans.push(Span::raw(" | "));
    }

    if let Some(rating) = game.rating {
        let stars_count = (rating.round() as usize).min(5);
        let stars = format!("{}{}", "★".repeat(stars_count), "☆".repeat(5 - stars_count));
        meta_spans.push(Span::styled(
            "Rating: ",
            Style::default().fg(Color::DarkGray),
        ));
        meta_spans.push(Span::styled(
            format!("{} ({:.1})", stars, rating),
            Style::default().fg(Color::Yellow),
        ));
        meta_spans.push(Span::raw(" | "));
    }

    if let Some(ref genre) = game.genre {
        if !genre.trim().is_empty() {
            meta_spans.push(Span::styled(
                "Genre: ",
                Style::default().fg(Color::DarkGray),
            ));
            meta_spans.push(Span::styled(genre, Style::default().fg(Color::Cyan)));
        }
    }

    // Strip trailing separator if needed
    if meta_spans.last().map(|s| s.content.as_ref()) == Some(" | ") {
        meta_spans.pop();
    }

    if !meta_spans.is_empty() {
        details_lines.push(Line::from(meta_spans));
    }

    // Developer & Publisher
    let mut dev_pub_spans = Vec::new();
    if let Some(ref dev) = game.developer {
        if !dev.trim().is_empty() {
            dev_pub_spans.push(Span::styled(
                "Developer: ",
                Style::default().fg(Color::DarkGray),
            ));
            dev_pub_spans.push(Span::styled(dev, Style::default().fg(Color::White)));
        }
    }

    if let Some(ref publ) = game.publisher {
        if !publ.trim().is_empty() {
            if !dev_pub_spans.is_empty() {
                dev_pub_spans.push(Span::raw(" | "));
            }
            dev_pub_spans.push(Span::styled(
                "Publisher: ",
                Style::default().fg(Color::DarkGray),
            ));
            dev_pub_spans.push(Span::styled(publ, Style::default().fg(Color::White)));
        }
    }

    if !dev_pub_spans.is_empty() {
        details_lines.push(Line::from(dev_pub_spans));
    }

    // Steam AppID or Disk Size
    if let Some(appid) = game.steam_appid {
        details_lines.push(Line::from(vec![
            Span::styled("Steam AppID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(appid.to_string(), Style::default().fg(Color::Yellow)),
        ]));
    } else {
        let size_str = game
            .file_size
            .map(|s| format!("{:.2} GB", s as f64 / (1024.0 * 1024.0 * 1024.0)))
            .unwrap_or_else(|| "N/A".to_string());

        details_lines.push(Line::from(vec![
            Span::styled("Size on Disk: ", Style::default().fg(Color::DarkGray)),
            Span::raw(size_str),
        ]));
    }

    details_lines.push(Line::from(vec![Span::styled(
        "[ENTER] Launch Game",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )]));

    let details_p = Paragraph::new(details_lines).block(
        Block::default()
            .title(Span::styled(
                " Game Details ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
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

    let status_style =
        if app.status_msg.starts_with("[Error]") || app.status_msg.starts_with("Error") {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if app.status_msg.starts_with("[OK]") {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };

    let status_paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            " LOG: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&app.status_msg, status_style),
    ]))
    .block(
        Block::default()
            .title(Span::styled(
                " System Status & Log ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(status_paragraph, bar_chunks[0]);

    if let Some(ref running) = app.running_game {
        let elapsed = running.started_at.elapsed().as_secs();
        let mut spans = vec![
            Span::styled(
                " RUNNING: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &running.title,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({:02}:{:02})", elapsed / 60, elapsed % 60),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        if let Some(ref runner_name) = running.runner_name {
            spans.push(Span::styled(
                format!("  via {}", runner_name),
                Style::default().fg(Color::DarkGray),
            ));
        }
        spans.push(Span::styled(
            "   [F] Forzar cierre",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        let running_paragraph = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .title(Span::styled(
                    " Running Game ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(running_paragraph, bar_chunks[1]);
    } else if let Some(ref progress) = app.download_progress {
        let is_item_progress = progress.runner_name.contains("Scan")
            || progress.runner_name.contains("Identif")
            || progress.runner_name.contains("SteamGridDB")
            || progress.runner_name.contains("Media")
            || (progress.total_bytes > 0 && progress.total_bytes < 50_000);
        let (prefix, label) = if is_item_progress {
            (
                " Task Progress: ",
                format!(
                    "{:.1}% ({}/{} items)",
                    progress.percentage, progress.downloaded_bytes, progress.total_bytes
                ),
            )
        } else {
            let downloaded_mb = progress.downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total_mb = progress.total_bytes as f64 / (1024.0 * 1024.0);
            let is_extracting = progress.percentage >= 99.9;
            let pfx = if is_extracting {
                " Extracting Archive: "
            } else {
                " Downloading Archive: "
            };
            (
                pfx,
                format!(
                    "{:.1}% ({:.1}/{:.1} MB)",
                    progress.percentage, downloaded_mb, total_mb
                ),
            )
        };

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("{}{}", prefix, progress.runner_name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
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
                    .title(Span::styled(
                        " Task Slider ",
                        Style::default().fg(Color::DarkGray),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        frame.render_widget(idle_paragraph, bar_chunks[1]);
    }
}

fn render_controls_footer(frame: &mut Frame, app: &App, area: Rect) {
    let items = match &app.active_input_source {
        crate::app::InputSource::Gamepad(_) => vec![
            ShortcutItem {
                key: "Ⓐ",
                label: "Launch",
                key_color: Color::Green,
            },
            ShortcutItem {
                key: "Ⓑ",
                label: "Back",
                key_color: Color::Red,
            },
            ShortcutItem {
                key: "ⓧ",
                label: "View",
                key_color: Color::Blue,
            },
            ShortcutItem {
                key: "Ⓨ",
                label: "Select",
                key_color: Color::Yellow,
            },
            ShortcutItem {
                key: "LB/RB",
                label: "Tab",
                key_color: Color::Magenta,
            },
            ShortcutItem {
                key: "Select",
                label: "BigPic",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "Start",
                label: "Settings",
                key_color: Color::Yellow,
            },
            ShortcutItem {
                key: "R3",
                label: "Delete",
                key_color: Color::Magenta,
            },
        ],
        crate::app::InputSource::Keyboard => vec![
            ShortcutItem {
                key: "/",
                label: "Search",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "?",
                label: "Help",
                key_color: Color::Yellow,
            },
            ShortcutItem {
                key: "v",
                label: "View",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "m",
                label: "Emulators",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "c",
                label: "Wine",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "Ctrl+S",
                label: "Scraper Menu",
                key_color: Color::Yellow,
            },
            ShortcutItem {
                key: "Shift+S",
                label: "Search Game",
                key_color: Color::Yellow,
            },
            ShortcutItem {
                key: "a",
                label: "Add Game",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "s",
                label: "Software Hub",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "Alt+O",
                label: "Big Picture",
                key_color: Color::Magenta,
            },
            ShortcutItem {
                key: "↵",
                label: "Launch Game",
                key_color: Color::Green,
            },
        ],
    };

    render_shortcut_footer(frame, app, area, &items, Alignment::Left);
}

/// Return the display aspect ratio (width/height) to use for a cover image
/// in the Big Picture carousel, based on the actual pixel dimensions stored in
/// `app.media_dimensions`. Falls back to 0.75 (portrait 3:4) when unknown.
fn aspect_ratio_for_cover(game_id: i64, app: &App) -> f32 {
    app.media_dimensions
        .get(&(game_id, "cover".to_string()))
        .map(|&(w, h)| {
            if h == 0 {
                0.75
            } else {
                (w as f32 / h as f32).clamp(0.2, 5.0)
            }
        })
        .unwrap_or(0.75)
}

/// Calculate the largest `Rect` (in character cells) that fits inside `viewport`,
/// preserving the image's pixel aspect ratio while accounting for terminal cell geometry,
/// and dead-centers it symmetrically horizontally and vertically inside `viewport`.
fn fit_and_center_media_viewport(viewport: Rect, pixel_ratio: f32, font_size: (u16, u16)) -> Rect {
    let (fw, fh) = font_size;
    let cell_pixel_ratio = if fh == 0 { 0.5 } else { fw as f32 / fh as f32 };
    let display_ratio_cells = pixel_ratio / cell_pixel_ratio;

    let max_w = viewport.width.saturating_sub(2).max(1);
    let max_h = viewport.height.saturating_sub(2).max(1);

    let target_w = ((max_h as f32) * display_ratio_cells).round() as u16;
    let (img_w, img_h) = if target_w <= max_w {
        (target_w.max(1), max_h)
    } else {
        let fit_w = max_w;
        let fit_h = ((fit_w as f32) / display_ratio_cells).round() as u16;
        (fit_w, fit_h.clamp(1, max_h))
    };

    let offset_x = (viewport.width.saturating_sub(img_w)) / 2;
    let offset_y = (viewport.height.saturating_sub(img_h)) / 2;

    Rect::new(viewport.x + offset_x, viewport.y + offset_y, img_w, img_h)
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
            Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &app.search_query,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                cursor,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({})", app.games.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])]
    } else if is_search_focused {
        vec![Line::from(vec![
            Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (Type...)", Style::default().fg(Color::DarkGray)),
        ])]
    } else {
        vec![Line::from(vec![Span::styled(
            "   [Search...]",
            Style::default().fg(Color::DarkGray),
        )])]
    };

    let search_p = Paragraph::new(search_content).block(
        Block::default()
            .title(Span::styled(
                if is_search_focused {
                    " Search (Active) "
                } else {
                    " Search "
                },
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
        let count = app
            .db
            .get_games_for_platform(p.id)
            .map(|g| g.len())
            .unwrap_or(0);
        let label = format!(" {} ({}) ", p.name, count);

        if is_current {
            if is_bar_focused {
                platform_spans.push(Span::styled(
                    format!("▶{}◀", label),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                platform_spans.push(Span::styled(
                    format!("[{}]", label),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
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

    let platform_bar_color = if is_bar_focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let platform_p = Paragraph::new(Line::from(platform_spans)).block(
        Block::default()
            .title(Span::styled(
                if is_bar_focused {
                    " Consoles (Focused) "
                } else {
                    " Consoles "
                },
                Style::default().fg(platform_bar_color),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(platform_bar_color)),
    );
    frame.render_widget(platform_p, top_chunks[1]);

    let stage_area = main_chunks[1];

    if app.games.is_empty() {
        let empty_p = Paragraph::new(
            "\n  No games found in current platform.\n  Press [Alt+O] to return to Library Mode.",
        )
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
            let left_stage = cols[0];
            let left_block = Block::default()
                .title(Span::styled(
                    format!(" ◀ {} ", prev_game.title),
                    Style::default().fg(Color::DarkGray),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray));
            let inner = left_block.inner(left_stage);
            frame.render_widget(left_block, left_stage);

            let pixel_ratio = aspect_ratio_for_cover(prev_game.id, app);
            let font_size = app.cover_manager.picker.font_size();
            let left_img_rect = fit_and_center_media_viewport(inner, pixel_ratio, font_size);

            let key_hb = (prev_game.id, "cover_hb".to_string());
            let key_cover = (prev_game.id, "cover".to_string());
            let protocol = if app.media_protocols.contains_key(&key_hb) {
                app.media_protocols.get_mut(&key_hb)
            } else {
                app.media_protocols.get_mut(&key_cover)
            };
            if let Some(protocol) = protocol {
                let image_widget =
                    StatefulImage::new(Some(image::Rgb([18, 20, 26]))).resize(Resize::Fit(None));
                frame.render_stateful_widget(image_widget, left_img_rect, protocol);
            } else {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "◀ PREV GAME",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        &prev_game.title,
                        Style::default().fg(Color::Gray),
                    )),
                ];
                let p = Paragraph::new(lines).alignment(Alignment::Center);
                frame.render_widget(p, left_img_rect);
            }
        }

        // 2. CENTER STAGE: Featured Focused Game in CRISP HD (Centered Horizontally & Vertically)!
        let active_game = &app.games[sel_idx];
        let override_tag = if active_game.emulator_override.is_some() {
            " ⚙"
        } else {
            ""
        };
        let center_block = Block::default()
            .title(Span::styled(
                format!(" FEATURED: {}{} ", active_game.title, override_tag),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow));

        let center_inner = center_block.inner(cols[1]);
        frame.render_widget(center_block, cols[1]);

        // Explicitly partition center_inner into media_viewport and details_rect
        let center_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),    // media_viewport exclusively for cover
                Constraint::Length(3), // details_rect strictly for metadata
            ])
            .split(center_inner);

        let media_viewport = center_chunks[0];
        let details_rect = center_chunks[1];

        let pixel_ratio = aspect_ratio_for_cover(active_game.id, app);
        let font_size = app.cover_manager.picker.font_size();
        let img_centered_rect =
            fit_and_center_media_viewport(media_viewport, pixel_ratio, font_size);

        // Render Featured HD Native Cover Image
        let key = (active_game.id, "cover".to_string());
        if let Some(protocol) = app.media_protocols.get_mut(&key) {
            let image_widget =
                StatefulImage::new(Some(image::Rgb([18, 20, 26]))).resize(Resize::Fit(None));
            frame.render_stateful_widget(image_widget, img_centered_rect, protocol);
        } else {
            let no_img =
                Paragraph::new("\n [ Loading HD Cover Artwork... ]\n Press [w] for Media Manager")
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
                Span::styled(
                    "TITLE: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &active_game.title,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "TYPE: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(badge, Style::default().fg(Color::Yellow)),
                Span::raw("  |  "),
                Span::styled(
                    "STATUS: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("Ready to Play", Style::default().fg(Color::Green)),
            ]),
        ];

        let details_p = Paragraph::new(details_lines).alignment(Alignment::Center);
        frame.render_widget(details_p, details_rect);

        // 3. RIGHT SIDE: Next Game Preview (Halfblocks cover, 2D dead-centered)
        if sel_idx + 1 < app.games.len() {
            let next_game = &app.games[sel_idx + 1];
            let right_stage = cols[2];
            let right_block = Block::default()
                .title(Span::styled(
                    format!(" {} ▶ ", next_game.title),
                    Style::default().fg(Color::DarkGray),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray));
            let inner = right_block.inner(right_stage);
            frame.render_widget(right_block, right_stage);

            let pixel_ratio = aspect_ratio_for_cover(next_game.id, app);
            let font_size = app.cover_manager.picker.font_size();
            let right_img_rect = fit_and_center_media_viewport(inner, pixel_ratio, font_size);

            let key_hb = (next_game.id, "cover_hb".to_string());
            let key_cover = (next_game.id, "cover".to_string());
            let protocol = if app.media_protocols.contains_key(&key_hb) {
                app.media_protocols.get_mut(&key_hb)
            } else {
                app.media_protocols.get_mut(&key_cover)
            };
            if let Some(protocol) = protocol {
                let image_widget =
                    StatefulImage::new(Some(image::Rgb([18, 20, 26]))).resize(Resize::Fit(None));
                frame.render_stateful_widget(image_widget, right_img_rect, protocol);
            } else {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "NEXT GAME ▶",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        &next_game.title,
                        Style::default().fg(Color::Gray),
                    )),
                ];
                let p = Paragraph::new(lines).alignment(Alignment::Center);
                frame.render_widget(p, right_img_rect);
            }
        }
    }

    let items = match &app.active_input_source {
        crate::app::InputSource::Gamepad(_) => vec![
            ShortcutItem {
                key: "Ⓐ",
                label: "Details",
                key_color: Color::Green,
            },
            ShortcutItem {
                key: "Ⓑ",
                label: "Normal Mode",
                key_color: Color::Red,
            },
            ShortcutItem {
                key: "ⓧ",
                label: "Consoles",
                key_color: Color::Blue,
            },
            ShortcutItem {
                key: "LB/RB",
                label: "Console",
                key_color: Color::Magenta,
            },
        ],
        crate::app::InputSource::Keyboard => vec![
            ShortcutItem {
                key: "/",
                label: "Search",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "Tab",
                label: "Focus",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "p",
                label: "Consoles",
                key_color: Color::Cyan,
            },
            ShortcutItem {
                key: "Alt+O",
                label: "Normal Mode",
                key_color: Color::Magenta,
            },
            ShortcutItem {
                key: "↵",
                label: "Details",
                key_color: Color::Green,
            },
        ],
    };

    render_shortcut_footer(frame, app, main_chunks[2], &items, Alignment::Center);
}

fn render_game_detail_view(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.games.is_empty() || app.selected_game_idx >= app.games.len() {
        return;
    }
    let game = app.games[app.selected_game_idx].clone();

    let platform_name = app
        .platforms
        .iter()
        .find(|p| p.id == game.platform_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let badge = match game.game_type.as_str() {
        "wine" => "Windows / Wine / Proton",
        "native" => "Linux Native",
        "steam" => "Steam",
        _ => "Emulator",
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40), // Hero (full width)
            Constraint::Min(14),        // Content: info panel, right of the cover column
            Constraint::Length(3),      // Footer
        ])
        .split(area);

    // 1. HERO (full width): the banner is cropped to cover the whole hero. The
    //    tall poster column (cover) overlays its bottom-left corner and runs
    //    down into the content zone, drawn over both hero and info sections.
    let hero_area = chunks[0];
    frame.render_widget(Clear, hero_area);

    // Banner: halfblocks (crop) hero background
    let banner_key = (game.id, "banner_hb".to_string());
    if let Some(proto) = app.media_protocols.get_mut(&banner_key) {
        let img = StatefulImage::new(None).resize(Resize::Crop(None));
        frame.render_stateful_widget(img, hero_area, proto);
    } else {
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::DarkGray)),
            hero_area,
        );
    }

    // Cover: native poster image overlaying hero + content section
    let hero_bottom = hero_area.y + hero_area.height;
    let old_cover_h = 16u16.min(hero_area.height.saturating_sub(3));
    let cover_top = hero_bottom.saturating_sub(old_cover_h);
    let info_bottom = chunks[1].y + chunks[1].height;
    let cover_h = info_bottom.saturating_sub(cover_top).max(1);
    let cover_w = (cover_h as f32 * 4.0 / 3.0).round() as u16;
    let cover_w = cover_w.min(area.width.saturating_sub(50).max(18));
    let cover_rect = Rect::new(hero_area.x + 2, cover_top, cover_w.max(1), cover_h);
    let cover_key = (game.id, "cover".to_string());
    if let Some(proto) = app.media_protocols.get_mut(&cover_key) {
        let img = StatefulImage::new(Some(image::Rgb([18, 20, 26])));
        frame.render_stateful_widget(img, cover_rect, proto);
    }

    // 2. INFO panel
    let info_x = cover_rect.right() + 2;
    let info_w = area.right().saturating_sub(info_x + 2).max(1);
    let info_area = Rect::new(info_x, chunks[1].y, info_w, chunks[1].height);
    let info_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " INFORMATION ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let info_inner = info_block.inner(info_area);
    frame.render_widget(info_block, info_area);

    let info_inner_w = info_inner.width;

    const ICON_H: u16 = 12;
    let icon_w = (ICON_H.saturating_mul(2))
        .min(info_inner_w.saturating_sub(6))
        .max(2);
    let title_avail_w = info_inner_w.saturating_sub(icon_w + 2);

    let title_art = crate::figlet_title::render_title(&game.title, title_avail_w);
    let top_region_h = ICON_H.max(title_art.line_height.saturating_mul(2));

    let info_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_region_h),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(info_inner);
    let title_region = info_chunks[0];

    // Icon beside the title
    let icon_key = (game.id, "icon_hb".to_string());
    if let Some(icon_proto) = app.media_protocols.get_mut(&icon_key) {
        let icon_rect = Rect::new(title_region.x, title_region.y, icon_w, ICON_H);
        let icon_img = StatefulImage::new(Some(image::Rgb([18, 20, 26])));
        frame.render_stateful_widget(icon_img, icon_rect, icon_proto);
    }

    // Title art, vertically centered in the fixed two-line region (a single
    // line stays centered instead of the panel shifting between games).
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    if title_art.is_plain {
        // Fallback when the font can't render some character (e.g. accented
        // names): normal wrapped text.
        let fallback = Paragraph::new(Line::from(Span::styled(&game.title, title_style)))
            .wrap(Wrap { trim: true });
        let title_rect = Rect::new(
            title_region.x + icon_w + 2,
            title_region.y,
            title_region.width.saturating_sub(icon_w + 2),
            title_region.height,
        );
        frame.render_widget(fallback, title_rect);
    } else {
        let used_h = title_art.rows.len() as u16;
        let title_top = title_region.y + (title_region.height.saturating_sub(used_h)) / 2;
        let title_rect = Rect::new(
            title_region.x + icon_w + 2,
            title_top,
            title_region.width.saturating_sub(icon_w + 2),
            used_h,
        );
        let text: Vec<Line> = title_art
            .rows
            .iter()
            .map(|row| Line::from(Span::styled(row.clone(), title_style)))
            .collect();
        let title_p = Paragraph::new(text).left_aligned();
        frame.render_widget(title_p, title_rect);
    }

    let year_str = game
        .release_year
        .map(|y| y.to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let dev_str = game.developer.as_deref().unwrap_or("Unknown");
    let pub_str = game.publisher.as_deref().unwrap_or("Unknown");
    let genre_str = game.genre.as_deref().unwrap_or("Unknown");

    let rating_str = if let Some(r) = game.rating {
        let stars = (r / 2.0).round().clamp(1.0, 5.0) as usize;
        format!(
            "{} ({:.1}/10)",
            "★".repeat(stars) + &"☆".repeat(5 - stars),
            r
        )
    } else {
        "N/A".to_string()
    };

    // ── HD Focus indicator ──────────────────────────────────────────────
    let focus_indicator = {
        let mk = |label: &'static str, active: bool| -> Vec<Span<'static>> {
            let dot = if active { "◆ " } else { "◇ " };
            let col = if active {
                Color::Yellow
            } else {
                Color::DarkGray
            };
            vec![Span::styled(
                format!("{}{}", dot, label),
                Style::default().fg(col).add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            )]
        };
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.extend(mk(
            "Cover",
            app.big_picture_media_focus == MediaFocus::Cover,
        ));
        spans.push(Span::raw("   "));
        spans.extend(mk(
            "Banner",
            app.big_picture_media_focus == MediaFocus::Banner,
        ));
        spans.push(Span::raw("   "));
        spans.extend(mk("Icon", app.big_picture_media_focus == MediaFocus::Icon));
        spans.push(Span::styled(
            "    [Tab] foco HD",
            Style::default().fg(Color::DarkGray),
        ));
        Line::from(spans)
    };

    let mut info_lines = vec![
        focus_indicator,
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "PLATFORM: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&platform_name, Style::default().fg(Color::White)),
            Span::raw("   |   "),
            Span::styled(
                "RELEASE YEAR: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(year_str, Style::default().fg(Color::Yellow)),
            Span::raw("   |   "),
            Span::styled(
                "RATING: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(rating_str, Style::default().fg(Color::LightYellow)),
        ]),
        Line::from(vec![
            Span::styled(
                "DEVELOPER: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(dev_str, Style::default().fg(Color::White)),
            Span::raw("   |   "),
            Span::styled(
                "PUBLISHER: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(pub_str, Style::default().fg(Color::White)),
            Span::raw("   |   "),
            Span::styled(
                "GENRE: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(genre_str, Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled(
                "EXECUTABLE TYPE: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(badge, Style::default().fg(Color::Yellow)),
            Span::raw("   |   "),
            Span::styled(
                "STATUS: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Ready to Play", Style::default().fg(Color::Green)),
        ]),
    ];

    if let Some(ref desc) = game.description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            info_lines.push(Line::from(""));
            info_lines.push(Line::from(vec![Span::styled(
                "DESCRIPTION:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            for desc_line in trimmed.lines() {
                let clean = desc_line.trim();
                if !clean.is_empty() {
                    info_lines.push(Line::from(Span::styled(
                        clean,
                        Style::default().fg(Color::Gray),
                    )));
                }
            }
        }
    }

    let info_p = Paragraph::new(info_lines).wrap(Wrap { trim: true });
    frame.render_widget(info_p, info_chunks[1]);

    // 3. ACTIONS: integrated inside the info panel, one horizontal row at its bottom.
    let is_fav = game.favorite;
    let mut action_spans = Vec::new();
    for (i, raw_label) in DETAIL_ACTIONS.iter().enumerate() {
        if i > 0 {
            action_spans.push(Span::raw("     "));
        }
        let label = if i == 1 {
            if is_fav {
                "Unfavorite"
            } else {
                "Favorite"
            }
        } else {
            *raw_label
        };
        let is_focused = app.detail_action_idx == i;
        let text = if i == 0 || is_focused {
            format!("▶ {} ◀", label)
        } else {
            format!("[ {} ]", label)
        };
        let style = if i == 0 {
            if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            }
        } else if is_focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        action_spans.push(Span::styled(text, style));
    }
    let actions_p = Paragraph::new(Line::from(action_spans)).alignment(Alignment::Right);
    frame.render_widget(actions_p, info_chunks[2]);

    // 4. DETAIL FOOTER
    let items = match &app.active_input_source {
        crate::app::InputSource::Gamepad(_) => vec![
            ShortcutItem {
                key: "Ⓐ",
                label: "Play",
                key_color: Color::Green,
            },
            ShortcutItem {
                key: "Ⓑ",
                label: "Back",
                key_color: Color::Red,
            },
            ShortcutItem {
                key: "◀ ▶",
                label: "Action",
                key_color: Color::Cyan,
            },
        ],
        crate::app::InputSource::Keyboard => vec![
            ShortcutItem {
                key: "↵",
                label: "Play",
                key_color: Color::Green,
            },
            ShortcutItem {
                key: "Esc",
                label: "Back",
                key_color: Color::Red,
            },
            ShortcutItem {
                key: "← →",
                label: "Action",
                key_color: Color::Cyan,
            },
        ],
    };

    render_shortcut_footer(frame, app, chunks[2], &items, Alignment::Center);
}

fn extract_custom_flags(cmd: &str) -> String {
    let installed = game_core::runner_detector::RunnerDetector::detect_installed_wine_runners();
    for r in &installed {
        let runner_str = match r.kind {
            game_core::runner_detector::RunnerKind::Proton => {
                format!("\"{}\" run \"{{file_path}}\"", r.binary_path.display())
            }
            game_core::runner_detector::RunnerKind::Wine => {
                format!("\"{}\" \"{{file_path}}\"", r.binary_path.display())
            }
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
    let popup_area = match app.modal_state {
        ModalState::ManageRunnersStep2Config { ref options, .. } => {
            runner_step2_popup_area(options.len(), frame.area())
        }
        _ => centered_rect(75, 70, frame.area()),
    };
    if !matches!(
        app.modal_state,
        ModalState::ConfirmDeleteGame { .. }
            | ModalState::ConfirmDeleteRunner { .. }
            | ModalState::EditCustomArgsInput { .. }
            | ModalState::PlatformSelector { .. }
            | ModalState::About
            | ModalState::UpdateAvailable { .. }
            | ModalState::FuzzySearchModal { .. }
    ) {
        frame.render_widget(Clear, popup_area);
    }

    match app.modal_state {
        ModalState::AddGameStep1Type { selected_type_idx } => {
            let options = [
                "[ROM] Automated ROMs Directory Scanner",
                "[LIN] Native Linux Executable / AppImage",
                "[WIN] Windows Executable (Wine / Proton .exe)",
                "[STM] Steam Application / Game",
            ];

            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(idx, opt)| {
                    let is_selected = idx == selected_type_idx;
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(format!("  {} ", opt)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Add Entry - Step 1: Select Type ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
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
        ModalState::WindowsStep1ActionMode {
            selected_action_idx,
        } => {
            let options = vec![
                "Registered Executable (Select existing .exe binary)",
                "Run Installer         (Execute setup.exe / installer)",
            ];
            let mut list_items = Vec::new();
            for (idx, opt) in options.iter().enumerate() {
                let is_selected = idx == selected_action_idx;
                let dot = if is_selected { "(•) " } else { "( ) " };
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let prefix = if is_selected { "▶ " } else { "  " };
                list_items.push(ListItem::new(Span::styled(
                    format!("{}{}{}", prefix, dot, opt),
                    style,
                )));
            }

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " Windows Setup - Step 1: Select Action ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Select Option | [Enter] Confirm | [Esc] Cancel")
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WindowsStep2CategoryMode {
            selected_category_idx,
            ..
        } => {
            let options = vec![
                "Game                (Add to Main Library & CoverFlow)",
                "Software / Utility  (Add to Software Hub with Icon Extraction)",
            ];
            let mut list_items = Vec::new();
            for (idx, opt) in options.iter().enumerate() {
                let is_selected = idx == selected_category_idx;
                let dot = if is_selected { "(•) " } else { "( ) " };
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let prefix = if is_selected { "▶ " } else { "  " };
                list_items.push(ListItem::new(Span::styled(
                    format!("{}{}{}", prefix, dot, opt),
                    style,
                )));
            }

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " Windows Setup - Step 2: Select Target Category ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Select Option | [Enter] Confirm | [Esc] Back")
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WindowsRunInstallerForm {
            selected_field,
            ref installer_path,
            ref wine_prefix,
            ..
        } => {
            let mut lines = Vec::new();

            // Field 0: Installer Executable Path
            let f0_style = if selected_field == 0 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 0 {
                        "▶ 1. Installer Path (.exe): "
                    } else {
                        "  1. Installer Path (.exe): "
                    },
                    f0_style,
                ),
                Span::styled(
                    if installer_path.is_empty() {
                        "<Select .exe installer file>"
                    } else {
                        installer_path.as_str()
                    },
                    if installer_path.is_empty() {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ]));
            lines.push(Line::from(""));

            // Field 1: Wine Prefix Path
            let f1_style = if selected_field == 1 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 1 {
                        "▶ 2. Wine Prefix: "
                    } else {
                        "  2. Wine Prefix: "
                    },
                    f1_style,
                ),
                Span::styled(
                    if wine_prefix.is_empty() {
                        "~/.wine"
                    } else {
                        wine_prefix.as_str()
                    },
                    Style::default().fg(Color::Yellow),
                ),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " Run Windows Installer ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help =
                Paragraph::new(" [Up/Down] Select Field | [Enter] Run Installer | [Esc] Back")
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::SelectDetectedExePicker {
            ref executables,
            selected_idx,
            ..
        } => {
            let mut list_items = Vec::new();
            for (idx, exe) in executables.iter().enumerate() {
                let is_selected = idx == selected_idx;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_selected { "▶ " } else { "  " };
                list_items.push(ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("{}{}", prefix, exe.display_name),
                        style,
                    )),
                    Line::from(Span::styled(
                        format!("    {}", exe.relative_path),
                        Style::default().fg(Color::DarkGray),
                    )),
                ]));
            }

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            format!(
                                " Select Installed Wine Application (.exe) [{}/{}] ",
                                selected_idx + 1,
                                executables.len()
                            ),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(2)])
                .split(popup_area);

            let mut list_state = ListState::default();
            list_state.select(Some(selected_idx));

            frame.render_stateful_widget(list, chunks[0], &mut list_state);

            let help =
                Paragraph::new(" [Up/Down] Navigate Apps | [Enter] Select App | [Esc] Cancel")
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::SoftwareHubModal {
            ref apps,
            selected_idx,
        } => {
            if apps.is_empty() {
                let empty_p = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  [ No installed software utilities registered yet ]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(
                        "  Register an .exe app using [a] or [WIN] -> Registered Executable",
                    ),
                    Line::from("  and select category 'Software / Utility' to display it here."),
                ])
                .block(
                    Block::default()
                        .title(Span::styled(
                            " Software Hub - Applications & Utilities ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                );

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(6), Constraint::Length(2)])
                    .split(popup_area);

                frame.render_widget(empty_p, chunks[0]);

                let help =
                    Paragraph::new(" [Esc] Close").style(Style::default().fg(Color::DarkGray));
                frame.render_widget(help, chunks[1]);
                return;
            }

            // Split into Header/Main Grid & Footer
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(2),
                ])
                .split(popup_area);

            let header_p = Paragraph::new(Line::from(vec![
                Span::styled(
                    " SOFTWARE HUB ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  Selected App [{}/{}]", selected_idx + 1, apps.len()),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(header_p, main_chunks[0]);

            // Grid of App Cards (3 columns x N rows)
            let grid_area = main_chunks[1];
            let cols_count = 3;
            let card_width = grid_area.width / cols_count;
            let card_height = 8; // height of each app card

            let rows_count = (grid_area.height / card_height).max(1) as usize;
            let visible_capacity = (cols_count as usize) * rows_count;

            let start_page_idx = (selected_idx / visible_capacity) * visible_capacity;
            let end_page_idx = (start_page_idx + visible_capacity).min(apps.len());

            for (i, app_game) in apps[start_page_idx..end_page_idx].iter().enumerate() {
                let real_idx = start_page_idx + i;
                let is_selected = real_idx == selected_idx;

                let row_i = i / (cols_count as usize);
                let col_i = i % (cols_count as usize);

                let card_x = grid_area.x + (col_i as u16) * card_width;
                let card_y = grid_area.y + (row_i as u16) * card_height;
                let card_rect = Rect::new(
                    card_x,
                    card_y,
                    card_width.min(grid_area.width - (col_i as u16) * card_width),
                    card_height,
                );

                let border_color = if is_selected {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(if is_selected {
                        BorderType::Double
                    } else {
                        BorderType::Rounded
                    })
                    .border_style(Style::default().fg(border_color));

                frame.render_widget(block, card_rect);

                let card_inner = card_rect.inner(ratatui::layout::Margin {
                    vertical: 1,
                    horizontal: 1,
                });
                if card_inner.width >= 4 && card_inner.height >= 4 {
                    // Split into Icon (Left: 8 cols x 4 rows) and Info (Right)
                    let card_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(8), Constraint::Min(4)])
                        .split(card_inner);

                    let icon_area = card_chunks[0];
                    let info_area = card_chunks[1];

                    // Fixed Icon Container Area (6 cols x 4 rows)
                    let icon_w = 6.min(icon_area.width);
                    let icon_h = 4.min(icon_area.height);
                    let offset_x = (icon_area.width.saturating_sub(icon_w)) / 2;
                    let offset_y = (icon_area.height.saturating_sub(icon_h)) / 2;
                    let fixed_icon_rect = Rect::new(
                        icon_area.x + offset_x,
                        icon_area.y + offset_y,
                        icon_w,
                        icon_h,
                    );

                    // Render Icon if protocol exists
                    let key = (app_game.id, "app_icon_hb".to_string());
                    if let Some(protocol) = app.media_protocols.get_mut(&key) {
                        let icon_img = StatefulImage::new(Some(image::Rgb([18, 20, 26])));
                        frame.render_stateful_widget(icon_img, fixed_icon_rect, protocol);
                    } else {
                        let fallback_p = Paragraph::new("[APP]")
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(Color::Cyan));
                        frame.render_widget(fallback_p, fixed_icon_rect);
                    }

                    // Render Title (Clean Icon + Name layout)
                    let title_style = if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let lines = vec![
                        Line::from(""),
                        Line::from(Span::styled(format!("{}", app_game.title), title_style)),
                    ];

                    let info_p = Paragraph::new(lines);
                    frame.render_widget(info_p, info_area);
                }
            }

            let help = Paragraph::new(" [Up/Down/Left/Right] Navigate Grid | [Enter] Launch | [Delete] Remove | [Esc] Close")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, main_chunks[2]);
        }
        ModalState::ScanFolderStep1Platform {
            selected_platform_idx,
        } => {
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

                let help =
                    Paragraph::new(" [Esc] Back").style(Style::default().fg(Color::DarkGray));
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
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
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
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(
                " [Up/Down] Select Platform | [Enter] Configure Scan Form | [Esc] Back",
            )
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WindowsGamesManager {
            ref games,
            selected_idx,
        } => {
            let popup_area = centered_rect(80, 75, frame.area());
            frame.render_widget(Clear, popup_area);

            let outer = Block::default()
                .title(Span::styled(
                    " Manage Windows Games ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let inner = outer.inner(popup_area);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(1)])
                .split(inner);

            let mut lines = Vec::new();

            if games.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No Windows games registered yet. Use [ + Add New Game ] to register one.",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
            } else {
                for (idx, game) in games.iter().enumerate() {
                    let is_selected = idx == selected_idx;
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let workdir = game.working_dir.as_deref().unwrap_or("?");
                    let wd_budget = path_budget(popup_area.width.saturating_sub(6) as usize, 16);
                    lines.push(Line::from(vec![
                        Span::styled(if is_selected { " ▶ " } else { "   " }, style),
                        Span::styled(&game.title, style),
                        Span::styled(
                            format!("   ({})", ellipsize_path_tail(workdir, wd_budget)),
                            if is_selected {
                                Style::default().fg(Color::Yellow)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        ),
                    ]));
                }
                lines.push(Line::from(""));
            }

            let is_add_focused = selected_idx == games.len();
            let add_style = if is_add_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{} [ + Add New Game ]",
                    if is_add_focused { "▶" } else { " " }
                ),
                add_style,
            )));

            let list_paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            frame.render_widget(outer, popup_area);
            frame.render_widget(list_paragraph, chunks[0]);

            let footer = Paragraph::new(" [↑↓] Navigate  [Enter] Edit / Add New Game  [Esc] Back")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(footer, chunks[1]);
        }
        ModalState::ScanFolderForm {
            ref platform,
            ref folders,
            selected_index,
        } => {
            let items = build_scan_folder_items(&app.db, platform.id, folders);
            let runners = app
                .db
                .get_runners_for_platform(platform.id)
                .unwrap_or_default();

            let folder_emu_label = |assigned: Option<i64>| -> String {
                match assigned {
                    Some(id) => runners
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| "?".to_string()),
                    None => "Default".to_string(),
                }
            };

            let popup_area = centered_rect(75, 70, frame.area());
            frame.render_widget(Clear, popup_area);

            let outer = Block::default()
                .title(Span::styled(
                    format!(" Manage Folders — {} ", platform.name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let inner = outer.inner(popup_area);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(1)])
                .split(inner);

            let mut lines = Vec::new();

            if folders.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No scan folders registered for this platform.",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
            } else {
                for (f_idx, folder) in folders.iter().enumerate() {
                    let count = app.db.get_game_count_for_folder(folder.id).unwrap_or(0);
                    let path_budget_f = path_budget(
                        popup_area.width.saturating_sub(6) as usize,
                        12 + count.to_string().len() + 10,
                    );
                    // Header row for folder: non-focusable metadata
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("Folder {}: ", f_idx + 1),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            ellipsize_path_tail(&folder.path, path_budget_f),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!(" ({} game{})", count, if count == 1 { "" } else { "s" }),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));

                    // Sub-row 1: Emulator
                    let is_emu_focused =
                        items.get(selected_index) == Some(&ScanFolderItem::FolderEmulator(f_idx));
                    let emu_name = folder_emu_label(folder.assigned_emulator_id);
                    let emu_style = if is_emu_focused {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(if is_emu_focused { " ▶ " } else { "   " }, emu_style),
                        Span::styled("Emulator: ", emu_style),
                        Span::styled(format!("◀ {} ▶", emu_name), emu_style),
                    ]));

                    // Sub-row 2: Core (if applicable)
                    if folder_has_core(&app.db, platform.id, folder) {
                        let is_core_focused =
                            items.get(selected_index) == Some(&ScanFolderItem::FolderCore(f_idx));
                        let available = add_scan_available_cores(
                            &app.db,
                            platform,
                            folder.assigned_emulator_id,
                        );
                        let core_name = if available.is_empty() {
                            "No downloaded cores".to_string()
                        } else {
                            folder
                                .assigned_core
                                .as_deref()
                                .and_then(|k| available.iter().find(|(ck, _)| ck == k))
                                .map(|(_, label)| label.clone())
                                .unwrap_or_else(|| "Default".to_string())
                        };
                        let core_style = if is_core_focused {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        lines.push(Line::from(vec![
                            Span::styled(if is_core_focused { " ▶ " } else { "   " }, core_style),
                            Span::styled("Core: ", core_style),
                            Span::styled(format!("◀ {} ▶", core_name), core_style),
                        ]));
                    }
                    lines.push(Line::from(""));
                }
            }

            // Bottom focusable item: [ + Add New Folder ]
            let is_add_focused = items.get(selected_index) == Some(&ScanFolderItem::AddNewFolder);
            let add_style = if is_add_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{} [ + Add New Folder ]",
                    if is_add_focused { "▶" } else { " " }
                ),
                add_style,
            )));

            // Bottom focusable item: [ Download Cores ] (only for core-based emulators)
            let platform_has_core = scan_folder_platform_has_retroarch(&app.db, platform.id);
            if platform_has_core {
                let is_dl_core_focused =
                    items.get(selected_index) == Some(&ScanFolderItem::DownloadCores);
                let dl_style = if is_dl_core_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} [ Download Cores ]",
                        if is_dl_core_focused { "▶" } else { " " }
                    ),
                    dl_style,
                )));
            }

            let list_paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            frame.render_widget(outer, popup_area);
            frame.render_widget(list_paragraph, chunks[0]);

            let footer = Paragraph::new(
                " [↑↓] Move focus  [←→] Change value  [Del] Delete  [R] Force rescan  [Enter] Confirm  [Esc] Back"
            )
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(footer, chunks[1]);
        }
        ModalState::AddFolderScanForm {
            ref platform,
            ref folder_path,
            ref extensions_input,
            recursive,
            use_dat_auto_id,
            add_emulator_id,
            ref add_core,
            selected_field,
            ..
        } => {
            let field_style = |idx: usize| {
                if idx == selected_field {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let supports_dat = scan_folder_supports_dat(&platform.slug);
            let has_core = scan_folder_add_has_core(&app.db, platform.id, add_emulator_id);
            let emu_idx = scan_folder_add_emu_idx(supports_dat);
            let core_idx = scan_folder_add_core_idx(supports_dat);
            let action_idx = scan_folder_add_action_index(supports_dat, has_core);

            let runners = app
                .db
                .get_runners_for_platform(platform.id)
                .unwrap_or_default();

            let mut lines = Vec::new();

            let path_selected = selected_field == 0;
            let path_budget_f = path_budget(popup_area.width.saturating_sub(4) as usize, 2 + 6 + 8);
            lines.push(Line::from(vec![
                Span::styled(if path_selected { "▶ " } else { "  " }, field_style(0)),
                Span::styled("Path: ", field_style(0)),
                Span::raw(if folder_path.is_empty() {
                    "< [Enter] to pick a folder >".to_string()
                } else {
                    ellipsize_path_tail(&folder_path, path_budget_f)
                }),
                Span::styled(
                    "  Browse",
                    if path_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ]));
            let ext_selected = selected_field == 1;
            lines.push(Line::from(vec![
                Span::styled(if ext_selected { "▶ " } else { "  " }, field_style(1)),
                Span::styled("Extensions: ", field_style(1)),
                Span::raw(extensions_input),
            ]));

            let rec_selected = selected_field == 2;
            let rec_check = if recursive { "[x] Yes" } else { "[ ] No" };
            lines.push(Line::from(vec![
                Span::styled(if rec_selected { "▶ " } else { "  " }, field_style(2)),
                Span::styled("Scan subfolders recursively: ", field_style(2)),
                Span::styled(
                    rec_check,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            if supports_dat {
                let dat_selected = selected_field == 3;
                let dat_check = if use_dat_auto_id {
                    "[x] Yes (DAT / Serial Auto-ID)"
                } else {
                    "[ ] No (filename matching)"
                };
                lines.push(Line::from(vec![
                    Span::styled(if dat_selected { "▶ " } else { "  " }, field_style(3)),
                    Span::styled("Automatic DAT identification: ", field_style(3)),
                    Span::styled(
                        dat_check,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }

            let emu_selected = selected_field == emu_idx;
            let emu_label =
                crate::app::add_scan_emulator_label(&app.db, platform, add_emulator_id, &runners);
            lines.push(Line::from(vec![
                Span::styled(if emu_selected { "▶ " } else { "  " }, field_style(emu_idx)),
                Span::styled("Emulator: ", field_style(emu_idx)),
                Span::styled(format!("◀ {} ▶", emu_label), field_style(emu_idx)),
            ]));

            if has_core {
                let core_selected = selected_field == core_idx;
                let available = add_scan_available_cores(&app.db, platform, add_emulator_id);
                if available.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            if core_selected { "▶ " } else { "  " },
                            field_style(core_idx),
                        ),
                        Span::styled(
                            "Core: No downloaded cores",
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                } else {
                    let core_name = add_core
                        .as_deref()
                        .and_then(|k| available.iter().find(|(ck, _)| ck == k))
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| "Default".to_string());
                    lines.push(Line::from(vec![
                        Span::styled(
                            if core_selected { "▶ " } else { "  " },
                            field_style(core_idx),
                        ),
                        Span::styled("Core: ", field_style(core_idx)),
                        Span::styled(format!("◀ {} ▶", core_name), field_style(core_idx)),
                    ]));
                }

                let dl_core_idx = crate::app::scan_folder_add_dl_core_idx(supports_dat);
                let dl_selected = selected_field == dl_core_idx;
                lines.push(Line::from(Span::styled(
                    format!("{} [ Download Cores ]", if dl_selected { "▶" } else { " " }),
                    Style::default()
                        .fg(if dl_selected {
                            Color::Black
                        } else {
                            Color::Yellow
                        })
                        .bg(if dl_selected {
                            Color::Yellow
                        } else {
                            Color::Reset
                        })
                        .add_modifier(Modifier::BOLD),
                )));
            }

            let add_selected = selected_field == action_idx;
            lines.push(Line::from(Span::styled(
                format!("{} [ Add & Scan ]", if add_selected { "▶" } else { " " }),
                Style::default()
                    .fg(if add_selected {
                        Color::Black
                    } else {
                        Color::Green
                    })
                    .bg(if add_selected {
                        Color::Green
                    } else {
                        Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            )));

            let widget = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Add Game · {} ", platform.name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            frame.render_widget(widget, popup_area);
        }
        ModalState::ConfigureApiKeyInput { ref input } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                "SteamGridDB API Key Required",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from("Enter your personal SteamGridDB API key to download HD covers, banners, and icons."));
            lines.push(Line::from(
                "You can get a free API key at: https://www.steamgriddb.com/profile/api",
            ));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "API Key: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if input.is_empty() {
                        "< Type or Paste SteamGridDB API Key here >"
                    } else {
                        input
                    },
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "[ SAVE API KEY & FETCH MEDIA ]",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " SteamGridDB Configuration ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Enter] Save API Key | [Esc] Cancel").style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::UnifiedSearchTitleInput {
            ref search_query,
            cursor_pos,
            ..
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                "Unified Media & Metadata Search (w)",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from("Enter game title to search:"));

            let (before, after) = search_query.split_at(cursor_pos.min(search_query.len()));
            lines.push(Line::from(vec![
                Span::raw(" > "),
                Span::styled(
                    before,
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
                Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[Enter]: Continue to Select Provider  ·  [Esc]: Cancel",
                Style::default().fg(Color::DarkGray),
            )));

            let popup_area = centered_rect(55, 30, frame.area());
            frame.render_widget(Clear, popup_area);
            let block = Block::default()
                .title(Span::styled(
                    " 1. Edit Search Title ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            frame.render_widget(Paragraph::new(lines).block(block), popup_area);
        }
        ModalState::UnifiedSearchProviderPicker {
            ref search_query,
            selected_provider_idx,
            ..
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                format!("Query: '{}'", search_query),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from("Select Search Provider:"));
            lines.push(Line::from(""));

            let providers = [
                (
                    "1. SteamGridDB",
                    "Visual Media (Grids, Covers, Banners, Icons)",
                ),
                ("2. ScreenScraper", "Full Metadata & Official Game Artwork"),
            ];

            for (idx, (name, desc)) in providers.iter().enumerate() {
                let is_sel = idx == selected_provider_idx;
                let prefix = if is_sel { "▶ " } else { "  " };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Yellow)),
                    Span::styled(*name, style),
                    Span::raw(" — "),
                    Span::styled(*desc, Style::default().fg(Color::DarkGray)),
                ]));
                lines.push(Line::from(""));
            }

            lines.push(Line::from(Span::styled(
                "[↑/↓]: Navigate  ·  [Enter]: Search with selected Provider  ·  [Esc]: Back",
                Style::default().fg(Color::DarkGray),
            )));

            let popup_area = centered_rect(65, 40, frame.area());
            frame.render_widget(Clear, popup_area);
            let block = Block::default()
                .title(Span::styled(
                    " 2. Select Provider ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            frame.render_widget(Paragraph::new(lines).block(block), popup_area);
        }
        ModalState::ScreenScraperAccount {
            ref user_input,
            ref pass_input,
            selected_field,
            is_editing_field,
            cursor_pos,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                "ScreenScraper Account Credentials",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            let f0_label = if selected_field == 0 {
                "▶ 1. Username: "
            } else {
                "  1. Username: "
            };
            let f0_style = if selected_field == 0 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![Span::styled(f0_label, f0_style)]));

            if is_editing_field && selected_field == 0 {
                let (before, after) = user_input.split_at(cursor_pos.min(user_input.len()));
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        before,
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]));
            } else {
                let u_text = if user_input.is_empty() {
                    "< Anonymous / Default IP Quota >".to_string()
                } else {
                    user_input.clone()
                };
                let u_style = if selected_field == 0 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(u_text, u_style),
                ]));
            }
            lines.push(Line::from(""));

            let f1_label = if selected_field == 1 {
                "▶ 2. Password: "
            } else {
                "  2. Password: "
            };
            let f1_style = if selected_field == 1 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![Span::styled(f1_label, f1_style)]));

            if is_editing_field && selected_field == 1 {
                let (before, after) = pass_input.split_at(cursor_pos.min(pass_input.len()));
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        before,
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]));
            } else {
                let p_text = if pass_input.is_empty() {
                    "< No Password Set >".to_string()
                } else {
                    "●".repeat(pass_input.len())
                };
                let p_style = if selected_field == 1 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(p_text, p_style),
                ]));
            }
            lines.push(Line::from(""));

            let f2_style = if selected_field == 2 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 2 { " ▶ " } else { "   " },
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("[ ✓ Save Credentials & Return ]", f2_style),
            ]));

            let help_text = if is_editing_field {
                "[Enter]: Save & Lock field  ·  [Esc]: Cancel editing  ·  [Ctrl+V]: Paste"
            } else {
                "[↑/↓]: Navigate  ·  [Enter]: Edit / Save  ·  [Esc]: Return to Settings"
            };

            let popup_area = centered_rect(60, 45, frame.area());
            frame.render_widget(Clear, popup_area);
            let block = Block::default()
                .title(Span::styled(
                    " ScreenScraper Account ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, popup_area);

            let help_area = Rect::new(
                popup_area.x,
                popup_area.y + popup_area.height,
                popup_area.width,
                1,
            );
            frame.render_widget(
                Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray)),
                help_area,
            );
        }
        ModalState::AppSettings {
            ref api_key_input,
            ref screenscraper_user_input,
            ref screenscraper_pass_input,
            ref thegamesdb_api_key_input,
            selected_field,
            is_editing_field,
            cursor_pos,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(Span::styled(
                "Application Settings & Integrations",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            // Row 0: 1. SteamGridDB API Key
            let f0_label = if selected_field == 0 {
                "▶ 1. SteamGridDB API Key: "
            } else {
                "  1. SteamGridDB API Key: "
            };
            let f0_label_style = if selected_field == 0 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![Span::styled(f0_label, f0_label_style)]));

            if is_editing_field && selected_field == 0 {
                let (before, after) = api_key_input.split_at(cursor_pos.min(api_key_input.len()));
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        before,
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]));
                lines.push(Line::from(Span::styled(
                    "   [Enter] Lock Field | [Ctrl+V] Paste",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                let masked_text = if api_key_input.is_empty() {
                    "< No API Key Set >".to_string()
                } else {
                    "●".repeat(api_key_input.len())
                };
                let display_style = if selected_field == 0 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(masked_text, display_style),
                ]));
            }
            lines.push(Line::from(""));

            // Row 1: 2. ScreenScraper Integration (Sub-modal trigger)
            let f1_label = if selected_field == 1 {
                "▶ 2. ScreenScraper Integration: "
            } else {
                "  2. ScreenScraper Integration: "
            };
            let f1_label_style = if selected_field == 1 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![Span::styled(f1_label, f1_label_style)]));
            let ss_status = if screenscraper_user_input.is_empty() {
                "[ User & Password: <Anonymous / IP Limit> ↵ Press Enter to Edit ]".to_string()
            } else {
                format!(
                    "[ User: {} · Pass: ●●●●●● ↵ Press Enter to Edit ]",
                    screenscraper_user_input
                )
            };
            let ss_style = if selected_field == 1 {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(ss_status, ss_style),
            ]));
            lines.push(Line::from(""));

            // Row 2: 3. TheGamesDB API Key
            let f2_label = if selected_field == 2 {
                "▶ 3. TheGamesDB API Key: "
            } else {
                "  3. TheGamesDB API Key: "
            };
            let f2_label_style = if selected_field == 2 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![Span::styled(f2_label, f2_label_style)]));

            if is_editing_field && selected_field == 2 {
                let (before, after) = thegamesdb_api_key_input
                    .split_at(cursor_pos.min(thegamesdb_api_key_input.len()));
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        before,
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                ]));
            } else {
                let masked_tgdb = if thegamesdb_api_key_input.is_empty() {
                    "< No API Key Set >".to_string()
                } else {
                    "●".repeat(thegamesdb_api_key_input.len())
                };
                let display_style = if selected_field == 2 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(masked_tgdb, display_style),
                ]));
            }
            lines.push(Line::from(""));

            // Row 3: Re-run Welcome Wizard
            let f3_style = if selected_field == 3 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 3 { " ▶ " } else { "   " },
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("[ Re-run Welcome & Setup Wizard ]", f3_style),
            ]));
            lines.push(Line::from(""));

            // Row 4: About TUI Game Station
            let f4_style = if selected_field == 4 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 4 { " ▶ " } else { "   " },
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("[ About TUI Game Station ]", f4_style),
            ]));
            lines.push(Line::from(""));

            // Field 7: Save Settings
            let f7_style = if selected_field == 7 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected_field == 7 { " ▶ " } else { "   " },
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("[ SAVE SETTINGS ]", f7_style),
            ]));

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " App Settings & Configuration ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
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
        ModalState::WelcomeWizard {
            step,
            ref sgdb_api_key,
            cursor_pos,
            ..
        } => {
            let wizard_area = frame.area();
            frame.render_widget(Clear, wizard_area);

            let main_block = Block::default()
                .title(Span::styled(
                    " GAME STATION - WELCOME & INITIAL SETUP ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
                .title_bottom(Span::styled(
                    format!(
                        " Step ({}/4) | [← / →] Switch Slide | [Tab] Cycle | [Esc] Skip Setup ",
                        step + 1
                    ),
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
                    body_lines.push(Line::from(Span::styled(
                        "WELCOME TO GAME STATION",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    body_lines.push(Line::from(Span::styled(
                        "Centralized Retro & Modern Gaming Dashboard for Linux",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("Game Station is your all-in-one terminal hub to organize, manage, and launch your"));
                    body_lines.push(Line::from("entire video game library seamlessly from a fast, hardware-accelerated TUI interface."));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "Multi-Platform Consoles ",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(
                            "│ 3DS, DS, GameCube, Wii, Switch, PS1, PS2, PSP, SNES, GBA & more",
                        ),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "Native & Wine / Proton ",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(
                            "│ Windows executables, custom runners, winetricks & Steam games",
                        ),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "HD Artwork Scraper      ",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("│ Automatic cover art, hero banners & icons in seconds"),
                    ]));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(Span::styled(
                        "Press [ → / Right Arrow ] or [ Enter ] to continue setup tour...",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                1 => {
                    body_lines.push(Line::from(Span::styled(
                        "KEY FEATURES & NAVIGATION SHORTCUTS",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "  [Alt+O]  ",
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("Toggle Big Picture Mode (3D Cover Flow Stage with HD Media)"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "    [/]    ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(
                            "Interactive Live Search bar to filter games across all platforms",
                        ),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "    [w]    ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(
                            "Visual Media Selector to fetch & customize covers, banners and icons",
                        ),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "    [c]    ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("Wine & Proton Runner Manager, winetricks and prefix tools"),
                    ]));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "    [?]    ",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("Keyboard & Mouse Controls Cheatsheet"),
                    ]));
                }
                2 => {
                    body_lines.push(Line::from(Span::styled(
                        "ARTWORK SCRAPER CONFIGURATION (OPTIONAL)",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("Configure your SteamGridDB API key to enable instant high-definition cover artwork"));
                    body_lines.push(Line::from(
                        "and hero banner scraping for all your ROMs and executables.",
                    ));
                    body_lines.push(Line::from(""));

                    let (before, after) = sgdb_api_key.split_at(cursor_pos.min(sgdb_api_key.len()));
                    body_lines.push(Line::from(vec![
                        Span::styled(
                            "SteamGridDB API Key: ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            before,
                            Style::default().fg(Color::White).bg(Color::DarkGray),
                        ),
                        Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                        Span::styled(after, Style::default().fg(Color::White).bg(Color::DarkGray)),
                    ]));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from("  * Obtain your free API key at: https://www.steamgriddb.com/profile/preferences/api"));
                    body_lines.push(Line::from("  * Use [Ctrl+V] to paste from clipboard"));
                }
                _ => {
                    body_lines.push(Line::from(Span::styled(
                        "INITIAL SETUP COMPLETE",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(
                        "Game Station is fully configured and ready for your game collection.",
                    ));
                    body_lines.push(Line::from(""));
                    body_lines.push(Line::from(vec![Span::styled(
                        "[ GET STARTED ]",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )]));
                }
            }

            let body_p = Paragraph::new(body_lines)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::NONE));
            frame.render_widget(body_p, content_chunks[2]);

            // 3. Footer Navigation Bar & Step Dots
            let prev_btn = if step > 0 {
                Span::styled(
                    " [ ← Back ] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("            ", Style::default())
            };

            let next_btn = if step < 3 {
                Span::styled(
                    " [ Next → ] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    " [ Finish ] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            };

            let footer_content = Line::from(vec![
                prev_btn,
                Span::raw("       "),
                Span::styled("Slide ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}/4 ", step + 1),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                if step == 0 {
                    Span::styled("● ", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("○ ", Style::default().fg(Color::DarkGray))
                },
                if step == 1 {
                    Span::styled("● ", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("○ ", Style::default().fg(Color::DarkGray))
                },
                if step == 2 {
                    Span::styled("● ", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("○ ", Style::default().fg(Color::DarkGray))
                },
                if step == 3 {
                    Span::styled("● ", Style::default().fg(Color::Yellow))
                } else {
                    Span::styled("○ ", Style::default().fg(Color::DarkGray))
                },
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
            let c_mark = if chosen_cover_idx.is_some() {
                "✓"
            } else {
                "-"
            };
            let b_mark = if chosen_banner_idx.is_some() {
                "✓"
            } else {
                "-"
            };
            let i_mark = if chosen_icon_idx.is_some() {
                "✓"
            } else {
                "-"
            };

            let tab_titles = [
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
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        };
                        Span::styled(format!(" [ {} ] ", title), style)
                    } else {
                        Span::styled(format!("   {}   ", title), Style::default().fg(Color::Gray))
                    }
                })
                .collect();

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(8),
                    Constraint::Length(2),
                ])
                .split(popup_area);

            let tabs_title = if focused_section == 0 {
                " [FOCUS: TABS - Use Left/Right to Switch] "
            } else {
                " Tabs "
            };
            let tabs_p = Paragraph::new(Line::from(tab_spans)).block(
                Block::default()
                    .title(Span::styled(
                        tabs_title,
                        Style::default()
                            .fg(if focused_section == 0 {
                                Color::Yellow
                            } else {
                                Color::DarkGray
                            })
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::NONE),
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
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
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
                            Span::styled(
                                "█",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(after),
                        ])
                    } else {
                        Line::from(vec![Span::raw(" "), Span::raw(search_query)])
                    };

                    let search_p = Paragraph::new(query_line).block(
                        Block::default()
                            .title(Span::styled(
                                search_title,
                                Style::default()
                                    .fg(if focused_section == 1 {
                                        Color::Yellow
                                    } else {
                                        Color::DarkGray
                                    })
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(search_border_style),
                    );
                    frame.render_widget(search_p, cand_chunks[0]);

                    let items: Vec<ListItem> = if is_searching {
                        vec![ListItem::new(" [ Searching SteamGridDB... ]")
                            .style(Style::default().fg(Color::Yellow))]
                    } else if candidates.is_empty() {
                        vec![ListItem::new(" No candidates found. Type a custom name above and press [Enter] / [s] to Search.").style(Style::default().fg(Color::Red))]
                    } else {
                        candidates
                            .iter()
                            .enumerate()
                            .map(|(idx, cand)| {
                                let is_selected =
                                    idx == selected_candidate_idx && focused_section == 2;
                                let style = if is_selected {
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Cyan)
                                        .add_modifier(Modifier::BOLD)
                                } else if idx == selected_candidate_idx {
                                    Style::default()
                                        .fg(Color::Cyan)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!("  {} (SGDB ID: {})", cand.name, cand.id))
                                    .style(style)
                            })
                            .collect()
                    };

                    let list_border_style = if focused_section == 2 {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let list_title = if focused_section == 2 {
                        format!(
                            " Candidates for '{}' ({}) [ACTIVE FOCUS] ",
                            game_title,
                            candidates.len()
                        )
                    } else {
                        format!(" Candidates for '{}' ({}) ", game_title, candidates.len())
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                list_title,
                                Style::default()
                                    .fg(if focused_section == 2 {
                                        Color::Cyan
                                    } else {
                                        Color::DarkGray
                                    })
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(list_border_style),
                    );
                    frame.render_widget(list, cand_chunks[1]);
                }
                1 => {
                    let items: Vec<ListItem> = if covers.is_empty() {
                        vec![ListItem::new(" No covers available for this candidate.")
                            .style(Style::default().fg(Color::Yellow))]
                    } else {
                        covers
                            .iter()
                            .enumerate()
                            .map(|(idx, c)| {
                                let is_selected = idx == selected_cover_idx && focused_section == 2;
                                let is_chosen = chosen_cover_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!(
                                    "  {}Cover #{} - ID: {}",
                                    check_str,
                                    idx + 1,
                                    c.id
                                ))
                                .style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Covers ",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 {
                                Color::Green
                            } else {
                                Color::DarkGray
                            })),
                    );
                    frame.render_widget(list, list_area);
                }
                2 => {
                    let items: Vec<ListItem> = if banners.is_empty() {
                        vec![ListItem::new(" No banners available for this candidate.")
                            .style(Style::default().fg(Color::Yellow))]
                    } else {
                        banners
                            .iter()
                            .enumerate()
                            .map(|(idx, b)| {
                                let is_selected =
                                    idx == selected_banner_idx && focused_section == 2;
                                let is_chosen = chosen_banner_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!(
                                    "  {}Banner #{} - ID: {}",
                                    check_str,
                                    idx + 1,
                                    b.id
                                ))
                                .style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Banners ",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 {
                                Color::Green
                            } else {
                                Color::DarkGray
                            })),
                    );
                    frame.render_widget(list, list_area);
                }
                3 => {
                    let items: Vec<ListItem> = if icons.is_empty() {
                        vec![ListItem::new(" No icons available for this candidate.")
                            .style(Style::default().fg(Color::Yellow))]
                    } else {
                        icons
                            .iter()
                            .enumerate()
                            .map(|(idx, ic)| {
                                let is_selected = idx == selected_icon_idx && focused_section == 2;
                                let is_chosen = chosen_icon_idx == Some(idx);
                                let check_str = if is_chosen { "[X] " } else { "[ ] " };

                                let style = if is_selected {
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else if is_chosen {
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!(
                                    "  {}Icon #{} - ID: {}",
                                    check_str,
                                    idx + 1,
                                    ic.id
                                ))
                                .style(style)
                            })
                            .collect()
                    };

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Available Icons ",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(if focused_section == 2 {
                                Color::Green
                            } else {
                                Color::DarkGray
                            })),
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
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow));

                let inner = preview_block.inner(preview_box);
                frame.render_widget(preview_block, preview_box);

                if app.visual_preview_loading {
                    let loading_txt = Paragraph::new("\n  [ Downloading Preview... ]").style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
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
            let help = Paragraph::new(help_str).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(help, modal_chunks[2]);
        }
        ModalState::AdvancedScraperSearch {
            ref original_title,
            ref search_query,
            cursor_pos,
            is_searching,
            ref results,
            selected_result_idx,
            ref error_msg,
            ..
        } => {
            let mut top_lines = Vec::new();
            top_lines.push(Line::from(vec![
                Span::styled("Original Game: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    original_title,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            top_lines.push(Line::from(""));

            let mut query_spans = vec![Span::styled(
                "Search Query: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )];
            if search_query.is_empty() {
                query_spans.push(Span::styled(
                    "< Type custom search title >",
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                let (left, right) = search_query.split_at(cursor_pos.min(search_query.len()));
                query_spans.push(Span::raw(left.to_string()));
                if let Some(ch) = right.chars().next() {
                    query_spans.push(Span::styled(
                        ch.to_string(),
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    ));
                    query_spans.push(Span::raw(right[ch.len_utf8()..].to_string()));
                } else {
                    query_spans.push(Span::styled(
                        " ",
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    ));
                }
            }
            top_lines.push(Line::from(query_spans));

            if is_searching {
                top_lines.push(Line::from(Span::styled(
                    "  ⚡ Searching ScreenScraper / TheGamesDB...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
            } else if let Some(ref err) = error_msg {
                top_lines.push(Line::from(Span::styled(
                    format!("  ❌ {}", err),
                    Style::default().fg(Color::Red),
                )));
            } else {
                top_lines.push(Line::from(Span::styled(
                    format!("  Found {} candidate result(s)", results.len()),
                    Style::default().fg(Color::Green),
                )));
            }

            // Quota status
            let ss_quota = app
                .scraper_quota
                .get(&scraper::pipeline::ScraperSource::ScreenScraper);
            let quota_color = match ss_quota.and_then(|q| q.fraction()) {
                Some(f) if f > 0.95 => Color::Red,
                Some(f) if f > 0.80 => Color::Yellow,
                Some(_) => Color::Green,
                None => Color::DarkGray,
            };
            let quota_label = ss_quota
                .map(|q| q.display_label())
                .unwrap_or_else(|| "--".to_string());
            top_lines.push(Line::from(vec![
                Span::styled("  Cuota SS: ", Style::default().fg(Color::DarkGray)),
                Span::styled(quota_label, Style::default().fg(quota_color)),
            ]));

            let top_widget = Paragraph::new(top_lines).block(
                Block::default()
                    .title(Span::styled(
                        " Refine Game Title / Advanced Search ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(8),
                    Constraint::Min(8),
                    Constraint::Length(2),
                ])
                .split(popup_area);

            frame.render_widget(top_widget, main_chunks[0]);

            let list_detail_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(main_chunks[1]);

            // Left List: Candidates
            let items: Vec<ListItem> = if results.is_empty() {
                vec![
                    ListItem::new(" No results. Type a refined title and press [Enter].")
                        .style(Style::default().fg(Color::DarkGray)),
                ]
            } else {
                results
                    .iter()
                    .enumerate()
                    .map(|(idx, res)| {
                        let is_selected = idx == selected_result_idx;
                        let style = if is_selected {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let year_str = res
                            .release_year
                            .map(|y| format!(" ({})", y))
                            .unwrap_or_default();
                        ListItem::new(format!("{}. {}{}", idx + 1, res.title, year_str))
                            .style(style)
                    })
                    .collect()
            };

            let results_list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Candidates List ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            frame.render_widget(results_list, list_detail_chunks[0]);

            // Right Detail: Selected Candidate Details
            let mut detail_lines = Vec::new();
            if let Some(res) = results.get(selected_result_idx) {
                detail_lines.push(Line::from(Span::styled(
                    &res.title,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                if let Some(y) = res.release_year {
                    detail_lines.push(Line::from(format!("Released: {}", y)));
                }
                if let Some(ref dev) = res.developer {
                    detail_lines.push(Line::from(format!("Developer: {}", dev)));
                }
                if let Some(ref publ) = res.publisher {
                    detail_lines.push(Line::from(format!("Publisher: {}", publ)));
                }
                if let Some(ref g) = res.genre {
                    detail_lines.push(Line::from(format!("Genre: {}", g)));
                }
                if let Some(r) = res.rating {
                    detail_lines.push(Line::from(format!("Rating: {:.1} / 5.0", r)));
                }
                detail_lines.push(Line::from(""));
                if let Some(ref desc) = res.description {
                    detail_lines.push(Line::from(Span::styled(
                        "Description:",
                        Style::default().fg(Color::Cyan),
                    )));
                    detail_lines.push(Line::from(desc.as_str()));
                }
            } else {
                detail_lines.push(Line::from(Span::styled(
                    "Select a game result from the left list to preview details.",
                    Style::default().fg(Color::DarkGray),
                )));
            }

            let detail_widget = Paragraph::new(detail_lines).block(
                Block::default()
                    .title(Span::styled(
                        " Result Details ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            frame.render_widget(detail_widget, list_detail_chunks[1]);

            let help = Paragraph::new(" [Enter] Search/Select Result | [Up/Down] Navigate Results | [Typing] Edit Query | [Esc] Cancel").style(
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            );
            frame.render_widget(help, main_chunks[2]);
        }
        ModalState::ManageRunnersStep1Platform {
            selected_platform_idx,
        } => {
            let unique_runners = app.db.get_unique_runners().unwrap_or_default();

            let items: Vec<ListItem> = unique_runners
                .iter()
                .enumerate()
                .map(|(idx, r)| {
                    let is_selected = idx == selected_platform_idx;
                    let status_badge = if r.is_configured { " [Configured]" } else { "" };

                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else if r.is_configured {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    ListItem::new(format!(
                        "  {} ({}){}",
                        r.name, r.console_initials, status_badge
                    ))
                    .style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Emulator / Runner Management - Select Emulator ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
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
                vec![
                    ListItem::new("  [ No Wine / Proton runners detected on system ]")
                        .style(Style::default().fg(Color::Yellow)),
                ]
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
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let line = format!(
                            "  {:8} {:30}  {:25} ({})",
                            kind_badge,
                            r.name,
                            r.location.display_name(),
                            r.binary_path.display()
                        );
                        ListItem::new(line).style(style)
                    })
                    .collect()
            };

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        format!(
                            " Installed Wine & Proton Runners ({}) ",
                            installed_runners.len()
                        ),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(
                " [d] Download GE-Proton / Proton-CachyOS | [Del] Delete Folder | [Esc] Close",
            )
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
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
            let current_launcher = launchers
                .get(selected_launcher_idx)
                .copied()
                .unwrap_or(scraper::proton::TargetLauncher::TUIGameStation);
            let valid_tools = current_launcher.valid_repos();
            let current_tool = valid_tools
                .get(selected_tool_idx)
                .copied()
                .unwrap_or(scraper::proton::ProtonRepo::GEProton);

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(6),
                    Constraint::Length(2),
                ])
                .split(popup_area);

            let breadcrumb_line = match step {
                0 => Line::from(vec![
                    Span::styled(
                        " [ STEP 1/3 ] ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " Select Target Launcher",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                1 => Line::from(vec![
                    Span::styled(
                        " [ STEP 2/3 ] ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {} ", current_launcher.display_name()),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        "➜  Select Tool / Runner",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                _ => Line::from(vec![
                    Span::styled(
                        " [ STEP 3/3 ] ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            " {} ➜ {} ",
                            current_launcher.display_name(),
                            current_tool.display_name()
                        ),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        "➜  Select Version to Download",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
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
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            let sub_dir = l.installation_dir(scraper::proton::ProtonRepo::GEProton);
                            let path_str = sub_dir.to_str().unwrap_or("");
                            ListItem::new(format!(
                                "  {}. {:22}  ({})",
                                idx + 1,
                                l.display_name(),
                                path_str
                            ))
                            .style(style)
                        })
                        .collect();

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                " Target Launchers ",
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ))
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
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            let target_dir = current_launcher.installation_dir(*repo);
                            let folder_name = target_dir
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or("runners");
                            ListItem::new(format!(
                                "  {:32}  [Installs to: {}]",
                                repo.display_name(),
                                folder_name
                            ))
                            .style(style)
                        })
                        .collect();

                    let list = List::new(items).block(
                        Block::default()
                            .title(Span::styled(
                                format!(
                                    " Compatible Tools for {} ({}) ",
                                    current_launcher.display_name(),
                                    valid_tools.len()
                                ),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    );
                    frame.render_widget(list, modal_chunks[1]);
                }
                _ => {
                    if is_loading {
                        let loading_p =
                            Paragraph::new("\n  [ Fetching release catalog from API... ]").style(
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            );
                        frame.render_widget(loading_p, modal_chunks[1]);
                    } else if releases.is_empty() {
                        let empty_p = Paragraph::new(
                            "\n  No downloadable releases found for this repository.",
                        )
                        .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
                        frame.render_widget(empty_p, modal_chunks[1]);
                    } else {
                        let items: Vec<ListItem> = releases
                            .iter()
                            .enumerate()
                            .map(|(idx, rel)| {
                                let is_selected = idx == selected_release_idx;
                                let size_mb = rel
                                    .asset
                                    .as_ref()
                                    .map(|a| a.size as f64 / 1_048_576.0)
                                    .unwrap_or(0.0);
                                let style = if is_selected {
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                };
                                ListItem::new(format!(
                                    "  {:28}  ({:.1} MB)  Published: {}",
                                    rel.name,
                                    size_mb,
                                    rel.published_at.chars().take(10).collect::<String>()
                                ))
                                .style(style)
                            })
                            .collect();

                        let target_dir = current_launcher.installation_dir(current_tool);
                        let path_str = target_dir.to_str().unwrap_or("");
                        let list = List::new(items).block(
                            Block::default()
                                .title(Span::styled(
                                    format!(
                                        " Releases for {} -> [{}] ({}) ",
                                        current_tool.display_name(),
                                        path_str,
                                        releases.len()
                                    ),
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
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
            let help = Paragraph::new(help_text).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(help, modal_chunks[2]);
        }

        ModalState::DownloadCoreModal {
            ref platform,
            ref runner,
            ref cores,
            ref downloaded_keys,
            selected_idx,
            ..
        } => {
            let cores_dir = game_core::retroarch_manager::resolve_retroarch_cores_dir(runner);
            let items: Vec<ListItem> = cores
                .iter()
                .enumerate()
                .map(|(idx, core)| {
                    let is_selected = idx == selected_idx;
                    let is_downloaded = downloaded_keys.contains(&core.key)
                        || cores_dir.join(&core.so_file).is_file();
                    let (badge, badge_style) = if is_downloaded {
                        ("[Downloaded]", Style::default().fg(Color::Green))
                    } else {
                        ("[Available]", Style::default().fg(Color::Yellow))
                    };
                    let bg = if is_selected {
                        Color::Yellow
                    } else {
                        Color::Reset
                    };
                    let fg = if is_selected {
                        Color::Black
                    } else {
                        Color::White
                    };
                    let style = Style::default().fg(fg).bg(bg).add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    });
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {} ", if is_selected { "▶" } else { " " }), style),
                        Span::styled(format!("{:20} ", core.name), style),
                        Span::styled(
                            format!("{:30} ", core.so_file),
                            Style::default()
                                .fg(if is_selected {
                                    Color::Black
                                } else {
                                    Color::DarkGray
                                })
                                .bg(bg),
                        ),
                        Span::styled(
                            badge,
                            if is_selected {
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(bg)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                badge_style
                            },
                        ),
                    ]))
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Download RetroArch Cores for {} ", platform.name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help = Paragraph::new(
                " [▲/▼] Select Core | [Enter] Download Selected Core | [Esc] Return to Form",
            )
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(help, chunks[1]);
        }

        ModalState::SelectDetectedEmulatorModal {
            ref runner_name,
            ref candidates,
            selected_idx,
            ..
        } => {
            let items: Vec<ListItem> = candidates
                .iter()
                .enumerate()
                .map(|(idx, cand)| {
                    let is_selected = idx == selected_idx;
                    let source_label = cand
                        .sources
                        .first()
                        .map(|s| s.display_label())
                        .unwrap_or("System");
                    let cmd = cand.launch_command();
                    let bg = if is_selected {
                        Color::Yellow
                    } else {
                        Color::Reset
                    };
                    let fg = if is_selected {
                        Color::Black
                    } else {
                        Color::White
                    };
                    let style = Style::default().fg(fg).bg(bg).add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    });

                    ListItem::new(Line::from(vec![
                        Span::styled(format!("  {} ", if is_selected { "▶" } else { " " }), style),
                        Span::styled(
                            format!("[{:20}] ", source_label),
                            Style::default()
                                .fg(if is_selected {
                                    Color::Black
                                } else {
                                    Color::Cyan
                                })
                                .bg(bg),
                        ),
                        Span::styled(cmd, style),
                    ]))
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        format!(" Detected Executables for {} ", runner_name),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help =
                Paragraph::new(" [▲/▼] Select Executable | [Enter] Use Executable | [Esc] Cancel")
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
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
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(format!(
                        "  {:8} {:25} ({})",
                        kind_badge,
                        r.name,
                        r.location.display_name()
                    ))
                    .style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(
                        " Select Installed Wine / Proton Runner ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);

            let help =
                Paragraph::new(" [Up/Down] Select Runner | [Enter] Apply to Game | [Esc] Cancel")
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::WineToolsMenu { selected_idx } => {
            let items = [
                ("Open winecfg", "Graphical Wine configuration"),
                ("Open winetricks", "Install Windows libraries"),
                ("Kill Wine processes", "Terminate wineserver"),
                ("Open Prefix folder", "Browse prefix in file manager"),
            ];

            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(idx, (title, desc))| {
                    let is_selected = idx == selected_idx;
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(format!("  {}: {} ({})", idx + 1, title, desc)).style(style)
                })
                .collect();

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " Wine Tools ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(list, chunks[0]);
            let help = Paragraph::new(" [Up/Down] Select Tool | [Enter] Execute | [Esc] Cancel")
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::EditCustomArgsInput {
            ref input,
            cursor_pos,
            ..
        } => {
            let cpos = cursor_pos.min(input.len());
            let avail = 54usize;
            let scroll = cpos.saturating_sub(avail);
            let end = (scroll + avail * 2).min(input.len());
            let visible = &input[scroll..end];
            let cursor_in_visible = cpos - scroll;
            let cursor_in_visible = cursor_in_visible.min(visible.len());
            let (before, after) = visible.split_at(cursor_in_visible);

            let p = Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    " Enter Custom Command / Launcher Arguments: ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        " > ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(before),
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(after),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    " Examples: --fullscreen, -dx11, WINEFSYNC=1 ",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(
                Block::default()
                    .title(Span::styled(
                        " Custom Launcher Arguments ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
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

            let help = Paragraph::new(
                " [Enter] Save | [Esc] Cancel | [Left/Right] Move cursor | [Backspace] Delete",
            )
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
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
                format!(
                    "Are you sure you want to remove {} selected games from your library?",
                    game_ids.len()
                )
            } else {
                format!(
                    "Are you sure you want to remove '{}' from your library?",
                    display_title
                )
            };

            let no_style = if selected_option == 0 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let yes_style = if selected_option == 1 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(
                    msg,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
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
                        .title(Span::styled(
                            " Confirm Game Deletion ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(1)])
                .split(popup_area);

            frame.render_widget(block, chunks[0]);

            let help =
                Paragraph::new(" [Left/Right/Tab] Select Option | [Enter] Confirm | [Esc] Cancel")
                    .alignment(Alignment::Center)
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ConfirmDeleteFolder {
            ref display,
            selected_option,
            ..
        } => {
            let popup_area = centered_rect_fixed(64, 9, frame.area());
            frame.render_widget(Clear, popup_area);

            let msg = format!(
                "Are you sure you want to remove '{}'?\nThe games it scanned are removed from your library.\nROM files are kept on disk.",
                display
            );

            let no_style = if selected_option == 0 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let yes_style = if selected_option == 1 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(
                    msg,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("   [ NO ]   ", no_style),
                    Span::raw("          "),
                    Span::styled("   [ YES ]   ", yes_style),
                ]),
            ];

            let block = Paragraph::new(content)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .title(Span::styled(
                            " Confirm Folder Deletion ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(1)])
                .split(popup_area);

            frame.render_widget(block, chunks[0]);

            let help =
                Paragraph::new(" [Left/Right/Tab] Select Option | [Enter] Confirm | [Esc] Cancel")
                    .alignment(Alignment::Center)
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
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
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let yes_style = if selected_option == 1 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(
                    msg,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
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
                        .title(Span::styled(
                            " Confirm Runner Deletion ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(1)])
                .split(popup_area);

            frame.render_widget(block, chunks[0]);

            let help =
                Paragraph::new(" [Left/Right/Tab] Select Option | [Enter] Confirm | [Esc] Cancel")
                    .alignment(Alignment::Center)
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
            frame.render_widget(help, chunks[1]);
        }
        ModalState::ManageRunnersStep2Config {
            ref runner_info,
            ref exe_path_input,
            ref options,
            ref option_values,
            ref custom_args,
            selected_row,
            selected_action_idx,
            cursor_pos,
        } => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Target Emulator: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} ({})", runner_info.name, runner_info.console_initials),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            let field_style = |idx: usize| {
                if idx == selected_row {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }
            };

            let path_label = if selected_row == 0 {
                "▶ 1. Executable Path: "
            } else {
                "  1. Executable Path: "
            };
            lines.push(Line::from(vec![Span::styled(path_label, field_style(0))]));

            let path_input_max = (popup_area.width.saturating_sub(7)) as usize;
            let (before, after, show_left, show_right) =
                editable_input_window(exe_path_input, cursor_pos, path_input_max);
            let path_span = if selected_row == 0 {
                vec![
                    Span::raw("   "),
                    Span::styled(
                        format!("{}{}", if show_left { "…" } else { "" }, before),
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled("█", Style::default().fg(Color::Yellow).bg(Color::DarkGray)),
                    Span::styled(
                        format!("{}{}", after, if show_right { "…" } else { "" }),
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                ]
            } else {
                vec![
                    Span::raw("   "),
                    Span::styled(
                        if exe_path_input.is_empty() {
                            "< No file selected >".to_string()
                        } else {
                            ellipsize_path_tail(exe_path_input, path_input_max)
                        },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]
            };
            lines.push(Line::from(path_span));
            lines.push(Line::from(""));

            if !options.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    "  Emulator Options:",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]));
            }

            for (i, opt) in options.iter().enumerate() {
                let row = i + 1;
                let value = option_values
                    .get(&opt.key)
                    .cloned()
                    .unwrap_or_else(|| opt.default.clone());
                let is_sel = selected_row == row;
                let arrow = if is_sel { "▶" } else { " " };
                match &opt.kind {
                    game_core::options::EmulatorOptionKind::Toggle => {
                        let on = value != opt.default;
                        let mark = if on { "[X]" } else { "[ ]" };
                        lines.push(Line::from(vec![
                            Span::styled(format!("{} {}. ", arrow, i + 2), field_style(row)),
                            Span::styled(mark, field_style(row)),
                            Span::raw(" "),
                            Span::styled(&opt.name, field_style(row)),
                            Span::raw("  "),
                            Span::styled(
                                if on {
                                    format!("(flag: {})", opt.flag_template)
                                } else {
                                    "(off)".to_string()
                                },
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                    game_core::options::EmulatorOptionKind::Choice(choices) => {
                        let pos = choices.iter().position(|c| *c == value).unwrap_or(0);
                        let label = |v: &str| {
                            opt.choice_labels
                                .get(v)
                                .cloned()
                                .unwrap_or_else(|| v.to_string())
                        };
                        lines.push(Line::from(vec![
                            Span::styled(format!("{} {}. ", arrow, i + 2), field_style(row)),
                            Span::styled(&opt.name, field_style(row)),
                            Span::raw(": "),
                            Span::styled(label(&value), field_style(row)),
                            Span::raw("  ["),
                            Span::styled(
                                format!("{}/{}", pos + 1, choices.len()),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled("]", Style::default().fg(Color::DarkGray)),
                            Span::raw("  "),
                            Span::styled(
                                choices
                                    .iter()
                                    .map(|c| label(c))
                                    .collect::<Vec<_>>()
                                    .join(" | "),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }
            }

            let custom_row = options.len() + 1;
            let custom_label = if selected_row == custom_row {
                "▶ Custom Args: "
            } else {
                "  Custom Args: "
            };
            let mut custom_line = vec![Span::styled(custom_label, field_style(custom_row))];
            let custom_label_w = custom_label.width();
            let custom_max = (popup_area.width.saturating_sub(4) as usize)
                .saturating_sub(custom_label_w)
                .max(12);
            if selected_row == custom_row {
                let (cbefore, cafter, c_left, c_right) =
                    editable_input_window(&custom_args, cursor_pos, custom_max);
                custom_line.push(Span::styled(
                    format!("{}{}", if c_left { "…" } else { "" }, cbefore),
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ));
                custom_line.push(Span::styled(
                    "█",
                    Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                ));
                custom_line.push(Span::styled(
                    format!("{}{}", cafter, if c_right { "…" } else { "" }),
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ));
            } else {
                custom_line.push(Span::styled(
                    if custom_args.is_empty() {
                        "< no extra flags >".to_string()
                    } else {
                        ellipsize_path_tail(&custom_args, custom_max)
                    },
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(custom_line));
            lines.push(Line::from(""));

            // Build action buttons list dynamically (based on the live form path)
            let exe_trimmed = exe_path_input.trim();
            let has_executable = !exe_trimmed.is_empty()
                && (exe_trimmed.contains(' ')
                    || exe_trimmed.starts_with("flatpak run")
                    || std::path::Path::new(exe_trimmed).exists());

            let is_flatpak_cmd = exe_trimmed.starts_with("flatpak run")
                || (exe_trimmed.contains(' ') && !std::path::Path::new(exe_trimmed).exists());

            struct ActionBtn {
                label: &'static str,
                fg: Color,
            }

            let mut btns = vec![
                ActionBtn {
                    label: "[ Browse ]",
                    fg: Color::Cyan,
                },
                ActionBtn {
                    label: "[ Detect ]",
                    fg: Color::Yellow,
                },
            ];

            if runner_info.download_url.is_some() {
                btns.push(ActionBtn {
                    label: "[ Download ]",
                    fg: Color::LightBlue,
                });
            }

            btns.push(ActionBtn {
                label: "[ Save ]",
                fg: Color::Green,
            });

            if has_executable {
                btns.push(ActionBtn {
                    label: "[ Open ]",
                    fg: Color::Magenta,
                });
                if !is_flatpak_cmd {
                    btns.push(ActionBtn {
                        label: "[ Delete ]",
                        fg: Color::Red,
                    });
                }
            }

            let buttons_row = options.len() + 2;
            let mut actions_line = vec![Span::raw("  ")];

            for (idx, btn) in btns.iter().enumerate() {
                let is_selected = selected_row == buttons_row && idx == selected_action_idx;
                let btn_style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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
                        format!(
                            " Emulator Options: {} ({}) ",
                            runner_info.name, runner_info.console_initials
                        ),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Switch Row | [Left/Right] Cycle Option / Select Action | [Enter] Confirm | [Esc] Back")
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
            path_cursor,
            workdir_cursor,
            prefix_cursor,
            cmd_cursor,
            ..
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
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(after),
                ])
            } else {
                Line::from(vec![
                    Span::styled("1. Title: ", field_style(0)),
                    Span::raw(title),
                ])
            };

            let form_max = (popup_area.width as usize).saturating_sub(4);

            match game_type {
                PlatformType::Emulator => {
                    let p_name = app
                        .platforms
                        .get(platform_idx)
                        .map(|p| p.name.as_str())
                        .unwrap_or("Unknown");
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Platform: ", field_style(1)),
                        Span::styled(format!("< {} >", p_name), field_style(1)),
                    ]));
                    lines.push(form_text_row(
                        "3. ROM Path: ",
                        file_path,
                        "< Press [Enter] to select ROM >",
                        path_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE GAME ]",
                        field_style(6),
                    )]));
                }
                PlatformType::Native => {
                    lines.push(title_line);
                    lines.push(form_text_row(
                        "2. Executable Path: ",
                        file_path,
                        "< Press [Enter] to browse >",
                        path_cursor,
                        form_max,
                        selected_field == 1,
                        field_style(1),
                    ));
                    lines.push(form_text_row(
                        "3. Working Dir: ",
                        working_dir,
                        "< Auto-populated >",
                        workdir_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(form_text_row(
                        "4. Custom Args: ",
                        custom_command,
                        "< Optional >",
                        cmd_cursor,
                        form_max,
                        selected_field == 3,
                        field_style(3),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 4));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 5));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 6));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE GAME ]",
                        field_style(7),
                    )]));
                }
                PlatformType::Wine => {
                    lines.push(title_line);
                    lines.push(form_text_row(
                        "2. Executable .exe Path: ",
                        file_path,
                        "< Press [Enter] to browse .exe >",
                        path_cursor,
                        form_max,
                        selected_field == 1,
                        field_style(1),
                    ));
                    lines.push(form_text_row(
                        "3. Prefix: ",
                        wine_prefix,
                        "< Auto-created in working folder if empty >",
                        prefix_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(form_text_row(
                        "4. Working Dir: ",
                        working_dir,
                        "< Auto-populated >",
                        workdir_cursor,
                        form_max,
                        selected_field == 3,
                        field_style(3),
                    ));

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
                        Span::styled(
                            ellipsize_path_tail(&flags_display, form_max.saturating_sub(15)),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "-- Wrappers & Toggles --",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    lines.push(mk_cb(gamemode, "GameMode", 6));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 7));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 8));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "-- Wine / Proton Options --",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    lines.push(mk_cb(esync, "Esync (eventfd sync)", 9));
                    lines.push(mk_cb(fsync, "Fsync (futex2 sync)", 10));
                    lines.push(mk_cb(dxvk, "DXVK Async", 11));
                    lines.push(mk_cb(vkd3d, "VKD3D-Proton Async", 12));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE GAME ]",
                        field_style(13),
                    )]));
                }
                PlatformType::Steam => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::styled(
                            if steam_appid.is_empty() {
                                "< Enter AppID >".to_string()
                            } else {
                                ellipsize_path_tail(&steam_appid, form_max.saturating_sub(16))
                            },
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    lines.push(form_text_row(
                        "3. Custom Args: ",
                        custom_command,
                        "< Optional >",
                        cmd_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE GAME ]",
                        field_style(6),
                    )]));
                }
            }

            let form_p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        block_title,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
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
            game_id,
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
            path_cursor,
            workdir_cursor,
            prefix_cursor,
            cmd_cursor,
            emulator_override,
            ref core_override,
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
            let form_max = (popup_area.width as usize).saturating_sub(4);

            let field_style = |idx: usize| {
                if idx == selected_field {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
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
                    let (emu_label, has_core, core_label) =
                        if let Some(game) = app.games.iter().find(|g| g.id == game_id) {
                            let platform_id = game.platform_id;
                            let platform_slug = app
                                .db
                                .get_platforms()
                                .unwrap_or_default()
                                .into_iter()
                                .find(|p| p.id == platform_id)
                                .map(|p| p.slug)
                                .unwrap_or_default();
                            let choices =
                                crate::edit_game_details::EditGameFormHelper::get_emulator_choices(
                                    &app.db, game,
                                );
                            let idx =
                            crate::edit_game_details::EditGameFormHelper::get_current_choice_idx(
                                &choices,
                                emulator_override,
                            );
                            let elabel = choices
                                .get(idx)
                                .map(|c| c.display_label.clone())
                                .unwrap_or_else(|| "Default".to_string());

                            let has_core =
                                scan_folder_add_has_core(&app.db, platform_id, emulator_override);

                            let emu_name = if let Some(id) = emulator_override {
                                app.db
                                    .get_runners_for_platform(platform_id)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .find(|r| r.id == id)
                                    .map(|r| r.name)
                                    .unwrap_or_else(|| "RetroArch".to_string())
                            } else {
                                app.db
                                    .get_runner_for_game(game.platform_id, game.folder_id, None)
                                    .ok()
                                    .flatten()
                                    .map(|r| r.name)
                                    .unwrap_or_else(|| "RetroArch".to_string())
                            };

                            let core_choices =
                                crate::edit_game_details::EditGameFormHelper::get_core_choices(
                                    &platform_slug,
                                    &emu_name,
                                );
                            let clabel = core_choices
                                .iter()
                                .find(|c| c.core_key == *core_override)
                                .map(|c| c.display_label.clone())
                                .unwrap_or_else(|| "Default".to_string());

                            (elabel, has_core, clabel)
                        } else {
                            ("Default".to_string(), false, "Default".to_string())
                        };

                    lines.push(title_line);
                    lines.push(form_text_row(
                        "2. ROM Path: ",
                        file_path,
                        "< Press [Enter] to select ROM >",
                        path_cursor,
                        form_max,
                        selected_field == 1,
                        field_style(1),
                    ));
                    lines.push(Line::from(vec![
                        Span::styled("3. Emulador: ", field_style(2)),
                        Span::styled(format!("◀ {} ▶", emu_label), field_style(2)),
                    ]));

                    let mut f_idx = 3;
                    if has_core {
                        lines.push(Line::from(vec![
                            Span::styled("4. Core: ", field_style(3)),
                            Span::styled(format!("◀ {} ▶", core_label), field_style(3)),
                        ]));
                        f_idx += 1;
                    }

                    lines.push(form_text_row(
                        &format!("{}. Custom Command / Args: ", f_idx + 1),
                        custom_command,
                        "< Optional >",
                        cmd_cursor,
                        form_max,
                        selected_field == f_idx,
                        field_style(f_idx),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", f_idx + 1));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", f_idx + 2));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", f_idx + 3));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE CHANGES ]",
                        field_style(f_idx + 4),
                    )]));
                }
                PlatformType::Native => {
                    lines.push(title_line);
                    lines.push(form_text_row(
                        "2. Executable Path: ",
                        file_path,
                        "< Press [Enter] to browse >",
                        path_cursor,
                        form_max,
                        selected_field == 1,
                        field_style(1),
                    ));
                    lines.push(form_text_row(
                        "3. Working Directory: ",
                        working_dir,
                        "< Optional >",
                        workdir_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(form_text_row(
                        "4. Custom Args / Command: ",
                        custom_command,
                        "< Optional >",
                        cmd_cursor,
                        form_max,
                        selected_field == 3,
                        field_style(3),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 4));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 5));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 6));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE CHANGES ]",
                        field_style(7),
                    )]));
                }
                PlatformType::Wine => {
                    lines.push(title_line);
                    lines.push(form_text_row(
                        "2. Executable .exe Path: ",
                        file_path,
                        "< Press [Enter] to browse .exe >",
                        path_cursor,
                        form_max,
                        selected_field == 1,
                        field_style(1),
                    ));
                    lines.push(form_text_row(
                        "3. Prefix: ",
                        wine_prefix,
                        "< Auto-created in working folder if empty >",
                        prefix_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(form_text_row(
                        "4. Working Directory: ",
                        working_dir,
                        "< Optional >",
                        workdir_cursor,
                        form_max,
                        selected_field == 3,
                        field_style(3),
                    ));

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
                        Span::styled(
                            ellipsize_path_tail(&flags_display, form_max.saturating_sub(15)),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "-- Wrappers & Toggles --",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    lines.push(mk_cb(gamemode, "GameMode", 6));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 7));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 8));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "-- Wine / Proton Options --",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    lines.push(mk_cb(esync, "Esync (eventfd sync)", 9));
                    lines.push(mk_cb(fsync, "Fsync (futex2 sync)", 10));
                    lines.push(mk_cb(dxvk, "DXVK Async", 11));
                    lines.push(mk_cb(vkd3d, "VKD3D-Proton Async", 12));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE CHANGES ]",
                        field_style(13),
                    )]));
                }
                PlatformType::Steam => {
                    lines.push(title_line);
                    lines.push(Line::from(vec![
                        Span::styled("2. Steam AppID: ", field_style(1)),
                        Span::styled(
                            ellipsize_path_tail(steam_appid, form_max.saturating_sub(16)),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    lines.push(form_text_row(
                        "3. Custom Args: ",
                        custom_command,
                        "< Optional >",
                        cmd_cursor,
                        form_max,
                        selected_field == 2,
                        field_style(2),
                    ));
                    lines.push(Line::from(""));
                    lines.push(mk_cb(gamemode, "GameMode", 3));
                    lines.push(mk_cb(mangohud, "MangoHud OSD", 4));
                    lines.push(mk_cb(gamescope, "Gamescope (Micro-compositor)", 5));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![Span::styled(
                        "[ SAVE CHANGES ]",
                        field_style(6),
                    )]));
                }
            }

            let form_p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        block_title,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(6), Constraint::Length(2)])
                .split(popup_area);

            frame.render_widget(form_p, chunks[0]);

            let help = Paragraph::new(" [Up/Down] Navigate | [Enter] Select | [Space] Toggle | [m] Local Media | [Esc] Cancel")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(help, chunks[1]);
        }
        ModalState::PlatformSelector { selected_idx } => {
            let max_name_len = app
                .platforms
                .iter()
                .map(|p| p.name.len())
                .max()
                .unwrap_or(12);
            let needed_w = (max_name_len as u16 + 26)
                .clamp(42, 60)
                .min(frame.area().width.saturating_sub(4));
            let needed_h = (app.platforms.len() as u16 + 2)
                .clamp(4, 16)
                .min(frame.area().height.saturating_sub(2));

            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let inner_width = popup_area.width.saturating_sub(4) as usize;

            let mut items = Vec::new();
            for (idx, p) in app.platforms.iter().enumerate() {
                let is_sel = idx == selected_idx;
                let count = app
                    .db
                    .get_games_for_platform(p.id)
                    .map(|g| g.len())
                    .unwrap_or(0);
                let count_str = format!("({} juegos)", count);
                let name_str = format!(" {}", p.name);
                let pad_len = inner_width.saturating_sub(name_str.len() + count_str.len() + 3);
                let padding = " ".repeat(pad_len);

                let line = if is_sel {
                    Line::from(vec![
                        Span::styled(
                            " ▶ ",
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            name_str,
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(padding, Style::default().bg(Color::Yellow)),
                        Span::styled(
                            count_str,
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
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
                    .title(Span::styled(
                        " SELECCIONAR PLATAFORMA ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " ▲▼ Navegar | ↵ Seleccionar | Esc Cerrar ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
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
                    Span::styled(
                        " [Navigation] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Up/Down / Mouse Scroll ", Style::default().fg(Color::Cyan)),
                    Span::raw("Browse | "),
                    Span::styled("Tab ", Style::default().fg(Color::Cyan)),
                    Span::raw("Cycle Pane | "),
                    Span::styled("p ", Style::default().fg(Color::Cyan)),
                    Span::raw("Toggle All Platforms"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [Views]      ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("v ", Style::default().fg(Color::Cyan)),
                    Span::raw("Cycle View (Cards/Banner/Table) | "),
                    Span::styled("Alt+O ", Style::default().fg(Color::Cyan)),
                    Span::raw("Big Picture Mode"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [Search]     ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("/ ", Style::default().fg(Color::Cyan)),
                    Span::raw("Focus Search Bar | "),
                    Span::styled("Esc ", Style::default().fg(Color::Cyan)),
                    Span::raw("Clear Search Query"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [Games]      ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("Enter / DblClick ", Style::default().fg(Color::Cyan)),
                    Span::raw("Launch | "),
                    Span::styled("Space ", Style::default().fg(Color::Cyan)),
                    Span::raw("Select | "),
                    Span::styled("Del ", Style::default().fg(Color::Cyan)),
                    Span::raw("Delete Game | "),
                    Span::styled("F ", Style::default().fg(Color::Red)),
                    Span::raw("Force Close Running Game"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [Media/ROMs] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("g ", Style::default().fg(Color::Cyan)),
                    Span::raw("Fetch Cover Artwork | "),
                    Span::styled("r ", Style::default().fg(Color::Cyan)),
                    Span::raw("Rescan Platform Folder"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [System]     ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
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
                    .title(Span::styled(
                        " KEYBOARD & MOUSE CONTROLS CHEATSHEET ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " Press [Esc] or [?] to close ",
                        Style::default().fg(Color::DarkGray),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::FuzzySearchModal {
            ref query,
            cursor_pos,
        } => {
            let needed_w = 60u16.min(frame.area().width.saturating_sub(4));
            let needed_h = 5u16;
            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let (before, after) = query.split_at(cursor_pos.min(query.len()));
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        " 🔍 Búsqueda: ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(before),
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(after),
                ]),
            ];

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " 🔍 BÚSQUEDA DIFUSA EN VIVO 🔍 ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " [Enter] Filtrar | [Esc] Limpiar | Escriba para buscar ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::About => {
            let popup_area = centered_rect_fixed(60, 10, frame.area());
            frame.render_widget(Clear, popup_area);

            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        " TUI Game Station ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("v{}", env!("CARGO_PKG_VERSION")),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(
                    " Sleek terminal gaming launcher & emulator dashboard",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(vec![
                    Span::styled(
                        " Author:  ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("CarlosEvCode"),
                ]),
                Line::from(vec![
                    Span::styled(
                        " Repo:    ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "https://github.com/CarlosEvCode/tui_game_station",
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        " License: ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("MIT License"),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        " [u] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Check Updates"),
                    Span::raw("     "),
                    Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)),
                    Span::raw("Close"),
                ]),
            ];

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        " ABOUT ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::UpdateAvailable {
            ref new_version,
            ref release_notes,
            ..
        } => {
            let needed_w = 58u16.min(frame.area().width.saturating_sub(4));
            let needed_h = 9u16;
            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let notes_first = release_notes
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("New release available on GitHub.")
                .trim();

            let lines = vec![
                Line::from(vec![
                    Span::styled("Current: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("v{}", env!("CARGO_PKG_VERSION")),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw("   ➔   "),
                    Span::styled("New: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("v{}", new_version),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Changelog: ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(notes_first, Style::default().fg(Color::White)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "[Enter] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Update Now"),
                    Span::raw("     "),
                    Span::styled("[Esc] ", Style::default().fg(Color::DarkGray)),
                    Span::raw("Dismiss"),
                ]),
            ];

            let p = Paragraph::new(lines).block(
                Block::default()
                    .title(Span::styled(
                        format!(" UPDATE AVAILABLE: v{} ", new_version),
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Green)),
            );

            frame.render_widget(p, popup_area);
        }
        ModalState::FavoritesModal {
            ref games,
            selected_idx,
        } => {
            let popup_area = centered_rect(60, 60, frame.area());
            frame.render_widget(Clear, popup_area);

            let mut list_items = Vec::new();
            if games.is_empty() {
                list_items.push(ListItem::new(Span::styled(
                    "  No favorites added yet. Press [f] on any game or detail view.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for (idx, game) in games.iter().enumerate() {
                    let is_sel = idx == selected_idx;
                    let style = if is_sel {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let prefix = if is_sel { " ▶ " } else { "   " };
                    let title_text = format!("{}* {}", prefix, game.title);
                    list_items.push(ListItem::new(Span::styled(title_text, style)));
                }
            }

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " FAVORITES LIBRARY ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " [Up/Down] Navigate | [Enter] Go to game | [Esc] Close ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(list, popup_area);
        }
        ModalState::LocalMediaPicker { selected_row, .. } => {
            let needed_w = 56u16.min(frame.area().width.saturating_sub(4));
            let needed_h = 9u16;
            let popup_area = centered_rect_exact(needed_w, needed_h, frame.area());
            frame.render_widget(Clear, popup_area);

            let rows = ["Cover Image Path", "Banner Image Path", "Icon Image Path"];
            let mut list_items = Vec::new();
            for (idx, name) in rows.iter().enumerate() {
                let is_sel = idx == selected_row;
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_sel { " ▶ " } else { "   " };
                list_items.push(ListItem::new(Span::styled(
                    format!("{}{}", prefix, name),
                    style,
                )));
            }

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " LOCAL MEDIA SELECTOR ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " [Up/Down] Navigate | [Enter] Browse File | [Esc] Back ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(list, popup_area);
        }
        ModalState::ScraperMenu {
            selected_source,
            ref enabled_platform_ids,
            region_idx,
            cover_pref_idx,
            banner_pref_idx,
            icon_pref_idx,
            selected_field,
        } => {
            let popup_area = centered_rect(70, 75, frame.area());
            frame.render_widget(Clear, popup_area);

            let region_labels = [
                "USA / North America (us)",
                "World (wor)",
                "Europe (eu)",
                "Japan (jp)",
            ];
            let region_name = region_labels
                .get(region_idx)
                .unwrap_or(&"USA / North America (us)");

            let cover_labels = [
                "2D Boxart",
                "3D Boxart",
                "2D Cartridge/Disk",
                "3D Cartridge/Disk",
                "Recalbox Mix V1",
                "Recalbox Mix V2",
            ];
            let cover_name = cover_labels.get(cover_pref_idx).unwrap_or(&"2D Boxart");

            let banner_labels = [
                "Fanart Background",
                "ScreenMarquee",
                "Gameplay Screenshot",
                "Title Screen",
                "Wheel Logo",
            ];
            let banner_name = banner_labels
                .get(banner_pref_idx)
                .unwrap_or(&"Fanart Background");

            let icon_labels = [
                "Wheel Logo",
                "Steel Wheel",
                "Carbon Wheel",
                "2D Cartridge/Disk",
                "3D Cartridge/Disk",
            ];
            let icon_name = icon_labels.get(icon_pref_idx).unwrap_or(&"Wheel Logo");

            let systems_summary = if enabled_platform_ids.is_empty() {
                "ALL SYSTEMS".to_string()
            } else {
                format!("{} SYSTEM(S) SELECTED", enabled_platform_ids.len())
            };

            let mut list_items = Vec::new();

            // Field 0: SCRAPE FROM
            let style_0 = if selected_field == 0 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_0 = if selected_field == 0 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!(
                    "{}SCRAPE FROM: ◄ {} ►",
                    prefix_0,
                    selected_source.as_str().to_uppercase()
                ),
                style_0,
            )));

            // Field 1: SCRAPE THESE SYSTEMS
            let style_1 = if selected_field == 1 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_1 = if selected_field == 1 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!(
                    "{}SCRAPE THESE SYSTEMS: [ {} ] ↵",
                    prefix_1, systems_summary
                ),
                style_1,
            )));

            // Field 2: PREFERRED REGION
            let style_2 = if selected_field == 2 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_2 = if selected_field == 2 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!("{}PREFERRED REGION: ◄ {} ►", prefix_2, region_name),
                style_2,
            )));

            // Field 3: COVER MEDIA PREFERENCE
            let style_3 = if selected_field == 3 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_3 = if selected_field == 3 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!("{}COVER MEDIA TYPE: ◄ {} ►", prefix_3, cover_name),
                style_3,
            )));

            // Field 4: BANNER MEDIA PREFERENCE
            let style_4 = if selected_field == 4 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_4 = if selected_field == 4 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!("{}BANNER MEDIA TYPE: ◄ {} ►", prefix_4, banner_name),
                style_4,
            )));

            // Field 5: ICON MEDIA PREFERENCE
            let style_5 = if selected_field == 5 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix_5 = if selected_field == 5 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!("{}ICON MEDIA TYPE: ◄ {} ►", prefix_5, icon_name),
                style_5,
            )));

            // Blank separator line
            list_items.push(ListItem::new(Span::raw("")));

            // Field 6: START SCRAPER BUTTON
            let style_6 = if selected_field == 6 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let prefix_6 = if selected_field == 6 { " ▶ " } else { "   " };
            list_items.push(ListItem::new(Span::styled(
                format!("{}[ START SCRAPER ]", prefix_6),
                style_6,
            )));

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " SCRAPER CONFIGURATION ",
                        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " [Up/Down] Navigate | [Left/Right] Option | [Enter] Select/Start | [Esc] Close ",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(list, popup_area);

            // ── Quota status line below the list ────────────────────────────
            let quota_area = Rect::new(
                popup_area.x + 2,
                popup_area.y + popup_area.height,
                popup_area.width.saturating_sub(4),
                1,
            );
            if quota_area.y < frame.area().height {
                let quota_color = |info: Option<&QuotaInfo>| -> Color {
                    match info.and_then(|q| q.fraction()) {
                        Some(f) if f > 0.95 => Color::Red,
                        Some(f) if f > 0.80 => Color::Yellow,
                        Some(_) => Color::Green,
                        None => Color::DarkGray,
                    }
                };
                let ss_info = app
                    .scraper_quota
                    .get(&scraper::pipeline::ScraperSource::ScreenScraper);
                let tgdb_info = app
                    .scraper_quota
                    .get(&scraper::pipeline::ScraperSource::TheGamesDB);
                let ss_label = ss_info
                    .map(|q| q.display_label())
                    .unwrap_or_else(|| "--".to_string());
                let tgdb_label = tgdb_info
                    .map(|q| q.display_label())
                    .unwrap_or_else(|| "--".to_string());
                let quota_line = Line::from(vec![
                    Span::styled(" SS: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(ss_label, Style::default().fg(quota_color(ss_info))),
                    Span::styled("   TGDB: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(tgdb_label, Style::default().fg(quota_color(tgdb_info))),
                ]);
                frame.render_widget(Paragraph::new(quota_line), quota_area);
            }
        }
        ModalState::ScraperSystemSelector {
            ref platforms,
            ref enabled_platform_ids,
            selected_idx,
            ..
        } => {
            let popup_area = centered_rect(65, 60, frame.area());
            frame.render_widget(Clear, popup_area);

            let mut list_items = Vec::new();
            for (idx, p) in platforms.iter().enumerate() {
                let is_sel = idx == selected_idx;
                let is_enabled =
                    enabled_platform_ids.is_empty() || enabled_platform_ids.contains(&p.id);
                let check_mark = if is_enabled { "[X]" } else { "[ ]" };

                let style = if is_sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if is_sel { " ▶ " } else { "   " };

                list_items.push(ListItem::new(Span::styled(
                    format!(
                        "{}{} {} ({})",
                        prefix,
                        check_mark,
                        p.name.to_uppercase(),
                        p.slug
                    ),
                    style,
                )));
            }

            let list = List::new(list_items).block(
                Block::default()
                    .title(Span::styled(
                        " SELECT SYSTEMS TO SCRAPE ",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(
                        " [Up/Down] Navigate | [Enter/Space] Toggle System | [Esc] Back ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(list, popup_area);
        }
        ModalState::None => {}
    }
}

/// Popup area for the Emulator Options (Step 2) modal. Shared with the mouse
/// handler so click coordinates always match the drawn popup, no matter the
/// terminal width. Height grows with the number of dynamic options.
pub fn runner_step2_popup_area(options_len: usize, r: Rect) -> Rect {
    let rows = (13 + options_len).min(40) as u16;
    centered_rect_fixed(78, rows, r)
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
