use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    pub created_at: Instant,
    pub duration_secs: f32,
}

impl Toast {
    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Self {
            id,
            message: message.into(),
            kind,
            created_at: Instant::now(),
            duration_secs: 3.5,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs_f32() >= self.duration_secs
    }
}

/// Render floating toast notification badges in the top-right corner of the terminal
pub fn render_toasts(frame: &mut Frame, toasts: &[Toast], area: Rect) {
    if toasts.is_empty() {
        return;
    }

    let active_toasts: Vec<&Toast> = toasts.iter().filter(|t| !t.is_expired()).take(4).collect();
    if active_toasts.is_empty() {
        return;
    }

    let toast_w = 42u16.min(area.width.saturating_sub(4));
    let start_x = area.width.saturating_sub(toast_w + 2);
    let mut current_y = 1u16;

    for toast in active_toasts {
        let (border_color, icon, icon_color) = match toast.kind {
            ToastKind::Info => (Color::Cyan, "ℹ ", Color::Cyan),
            ToastKind::Success => (Color::Green, "✔ ", Color::Green),
            ToastKind::Warning => (Color::Yellow, "⚠ ", Color::Yellow),
            ToastKind::Error => (Color::Red, "✖ ", Color::Red),
        };

        let toast_h = 3u16;
        let rect = Rect::new(start_x, current_y, toast_w, toast_h);
        frame.render_widget(Clear, rect);

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled(icon, Style::default().fg(icon_color).add_modifier(Modifier::BOLD)),
            Span::styled(&toast.message, Style::default().fg(Color::White)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

        frame.render_widget(paragraph, rect);
        current_y += toast_h;
    }
}
