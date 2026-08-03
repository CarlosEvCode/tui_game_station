//! Big-picture detail title, rendered with the TOIlet "Future" font via
//! `figlet-rs`. Long titles are wrapped by whole words (never mid-word) into at
//! most two fixed-height lines, then drawn with a plain ratatui `Paragraph` —
//! no pixel-font widgets, no image protocols.

use std::sync::OnceLock;

use figlet_rs::{Toilet, FIGure};

/// Maximum number of big-title lines we reserve and render. Kept FIXED so the
/// rest of the info panel never moves between games.
pub const MAX_LINES: usize = 2;

/// TOIlet "Future" font, parsed exactly once (parsing the .tlf is not free).
fn font() -> &'static Toilet {
    static FONT: OnceLock<Toilet> = OnceLock::new();
    FONT.get_or_init(|| Toilet::future().expect("bundled future.tlf must parse"))
}

fn render(text: &str) -> Option<FIGure<'static>> {
    font().convert(text)
}

/// Height in terminal rows of one Future line (constant for a given font).
pub fn line_height() -> u16 {
    static H: OnceLock<u16> = OnceLock::new();
    *H.get_or_init(|| render("A").map(|art| art.height as u16).unwrap_or(6))
}

/// Render width in columns of the widest row of `art` (trailing spaces are
/// ignored — they don't overflow the panel).
fn art_width(art: &FIGure<'_>) -> u16 {
    art.as_str()
        .lines()
        .map(|line| line.trim_end().chars().count() as u16)
        .max()
        .unwrap_or(0)
}

fn art_to_rows(art: &FIGure<'_>) -> Vec<String> {
    art.as_str()
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// Width of `text` rendered by the Future font (columns), or `u16::MAX` if the
/// font cannot render it at all.
fn rendered_width(text: &str) -> u16 {
    render(text).map(|art| art_width(&art)).unwrap_or(u16::MAX)
}

/// LAST-RESORT fallback. Only reached when a single word is wider than the
/// whole title panel (an absurdly long unbreakable token). The word is chopped
/// down char by char and an ellipsis is appended so the title still reads.
/// Tweak `MIN_BODY_CHARS` here if this ever needs tuning.
fn truncate_word_to_fit(word: &str, max_width: u16) -> String {
    const MIN_BODY_CHARS: usize = 1;
    let ellipsis = ellipsis();
    let body = word.trim();
    let total = body.chars().count();
    for keep in (MIN_BODY_CHARS..=total).rev() {
        let candidate: String = body
            .chars()
            .take(keep)
            .chain(ellipsis.chars())
            .collect();
        if rendered_width(&candidate) <= max_width {
            return candidate;
        }
    }
    // Nothing fit (not even a single char): return the bare ellipsis; the
    // Paragraph will clip whatever still overflows.
    ellipsis.to_string()
}

/// The unicode ellipsis is preferred, but if the font lacks that glyph
/// (figlet-rs drops unknown characters silently) fall back to ASCII dots so
/// the truncation marker is always visible.
fn ellipsis() -> &'static str {
    static E: OnceLock<&'static str> = OnceLock::new();
    *E.get_or_init(|| {
        if render("…").is_some() {
            "…"
        } else {
            "..."
        }
    })
}

/// Result of rendering a game title for the detail panel.
pub struct TitleArt {
    /// One entry per terminal row of the finished art (1 or 2 lines × line height).
    pub rows: Vec<String>,
    /// Terminal rows per big-title line (constant).
    pub line_height: u16,
    /// True when the title fell back to plain (wrapped) text — e.g. the font
    /// is missing a glyph the title needs.
    pub is_plain: bool,
}

/// Render `title` into at most [`MAX_LINES`] big lines, each no wider than
/// `max_width` columns. Falls back to plain text when the font can't render
/// some character (empty title or missing glyphs).
pub fn render_title(title: &str, max_width: u16) -> TitleArt {
    let line_height = line_height();

    // figlet-rs silently drops glyphs it doesn't have, so compare lengths to
    // detect titles the font can't fully render.
    let renderable = render(title).filter(|art| art.characters.len() == title.chars().count());

    match renderable {
        // Whole title fits on one line.
        Some(art) if art_width(&art) <= max_width => TitleArt {
            rows: art_to_rows(&art),
            line_height,
            is_plain: false,
        },
        // Too wide: wrap by whole words into at most two lines.
        Some(_) => {
            let mut rows = Vec::new();
            for text in wrap_words(title, max_width) {
                if let Some(art) = render(&text) {
                    rows.extend(art_to_rows(&art));
                }
            }
            if rows.is_empty() {
                TitleArt { rows: vec![title.to_string()], line_height, is_plain: true }
            } else {
                TitleArt { rows, line_height, is_plain: false }
            }
        }
        // Not fully renderable: plain wrapped text.
        None => TitleArt {
            rows: vec![title.to_string()],
            line_height,
            is_plain: true,
        },
    }
}

/// Greedy whole-word wrap into at most [`MAX_LINES`] lines. Every returned line
/// is a space-joined group of whole words; its real rendered width (respecting
/// the font's kerning/smushing) is what decides the fit, so words are never
/// split mid-word.
fn wrap_words(title: &str, max_width: u16) -> Vec<String> {
    // Chop down any single word that alone overflows the panel (extreme case,
    // e.g. a long unbreakable URL).
    let words: Vec<String> = title
        .split_whitespace()
        .map(|w| {
            if rendered_width(w) > max_width {
                truncate_word_to_fit(w, max_width)
            } else {
                w.to_string()
            }
        })
        .collect();

    let mut lines: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for w in words {
        let mut candidate = cur.clone();
        candidate.push(w.clone());
        if cur.is_empty() || rendered_width(&candidate.join(" ")) <= max_width {
            cur = candidate;
        } else if lines.len() + 1 >= MAX_LINES {
            break; // Only MAX_LINES big lines are reserved; drop the rest.
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push(w);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines.into_iter().map(|line| line.join(" ")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_title_stays_on_one_line() {
        let art = render_title("Tetris", 200);
        assert!(!art.is_plain);
        assert_eq!(art.rows.len(), line_height() as usize);
        assert!(art.rows.iter().all(|r| r.chars().count() <= 200));
    }

    #[test]
    fn medium_title_wraps_by_words_into_two_lines() {
        // Half the full width forces at least two lines.
        let full = rendered_width("New Super Mario Bros. 2");
        let lines = wrap_words("New Super Mario Bros. 2", full / 2 + 2);
        assert_eq!(lines.len(), 2, "expected 2 wrapped lines, got {lines:?}");
        // Every line is a subsequence of the original whole words.
        let words: Vec<&str> = "New Super Mario Bros. 2".split_whitespace().collect();
        for line in &lines {
            let line_words: Vec<&str> = line.split_whitespace().collect();
            let mut it = words.iter();
            for w in &line_words {
                assert!(it.any(|c| c == w), "word '{w}' not a whole word of the title");
            }
        }
        // And each line really fits when rendered.
        for line in &lines {
            assert!(rendered_width(line) <= full / 2 + 2);
        }
    }

    #[test]
    fn long_title_is_capped_at_two_lines() {
        let title = "Legend of Zelda, The - Majora's Mask 3D";
        let full = rendered_width(title);
        let lines = wrap_words(title, full / 2);
        assert!(lines.len() <= MAX_LINES, "hard cap of {MAX_LINES} lines");
        assert!(lines.iter().all(|l| rendered_width(l) <= full / 2));
        // Words are whole and in original order (never split mid-word); the cap
        // may only drop trailing words when the title is extremely long.
        let all: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        let orig: Vec<&str> = title.split_whitespace().collect();
        assert!(
            all.iter().zip(orig.iter()).all(|(a, b)| a == b),
            "words reordered or split: {all:?}"
        );
    }

    #[test]
    fn huge_single_word_is_truncated_with_ellipsis() {
        let word = "Supercalifragilisticexpialidocious";
        let narrow = 30u16;
        let lines = wrap_words(word, narrow);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Super"), "keep the word's beginning");
        assert!(rendered_width(&lines[0]) <= narrow, "truncated word must fit");
    }

    #[test]
    fn title_with_missing_glyph_falls_back_to_plain() {
        let art = render_title("Pokémon", 200);
        assert!(art.is_plain);
        assert_eq!(art.rows, vec!["Pokémon".to_string()]);
    }

    #[test]
    fn one_word_per_line_when_width_is_tiny() {
        let lines = wrap_words("Very Long Game Title", 10);
        assert!(lines.len() <= MAX_LINES);
    }
}
