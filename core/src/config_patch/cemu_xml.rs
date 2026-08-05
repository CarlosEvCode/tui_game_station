//! Surgical patcher for Cemu's XML configuration (`settings.xml`).
//!
//! Format contract:
//! - XML with `<tag>value</tag>` leaf elements on a single line.
//! - The root element is `<content>`.
//! - Ambiguous tag names exist across branches (e.g. `<api>` appears in both
//!   `<Graphic>` and `<Audio>`), so elements MUST be located by their full
//!   path from the root (e.g. `["Graphic", "api"]`), not by tag name alone.
//! - Self-closing tags (`<Tag/>` or `<Tag .../>`) do NOT push to the path
//!   stack.
//! - Attributes on opening tags (`<Tag attr="val">`) are ignored for path
//!   tracking; only the tag name matters.
//!
//! This module does NOT parse the whole file into a DOM. It walks line-by-line,
//! maintaining a stack of open tags, and only touches the single matching line.
//! Everything else is rewritten byte-for-byte identical.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::qt_ini::PatchError;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of a successful Cemu XML patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CemuPatchResult {
    /// Full XML path that was patched (e.g. `["Graphic", "api"]`).
    pub xml_path: Vec<String>,
    /// Value that was on disk before the patch.
    pub old_value: Option<String>,
    /// Value written to disk.
    pub new_value: String,
}

/// Read the current text value of an XML element located by `xml_path`.
///
/// Returns `Ok(None)` when the path is not found — pure, non-destructive
/// lookup. Never modifies the file.
///
/// Convention: the document root element (e.g. `<content>`) is NOT part of
/// `xml_path`. Paths start from children of the root.
pub fn read_cemu_xml_value(path: &Path, xml_path: &[&str]) -> Result<Option<String>, PatchError> {
    let content = read_file(path)?;
    let lines = split_lines(&content);
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;

    for line in &lines {
        let trimmed = line.trim();

        // Self-closing tag: <Tag/> or <Tag .../>  — skip, no stack change.
        if is_self_closing(trimmed) {
            continue;
        }

        // Closing tag: </Tag> — pop from stack.
        if let Some(tag) = parse_closing_tag(trimmed) {
            if stack.last().map(|s| s.as_str()) == Some(tag) {
                stack.pop();
            }
            continue;
        }

        // Opening tag with value on same line: <Tag>value</Tag>
        if let Some((tag, value)) = parse_leaf_tag(trimmed) {
            // Check if the current stack + this tag matches the target path.
            if stack.len() + 1 == xml_path.len()
                && stack.iter().zip(xml_path.iter()).all(|(s, p)| s == p)
                && tag == *xml_path.last().unwrap_or(&"")
            {
                return Ok(Some(value));
            }
            // Leaf tag is both open and close — no stack change needed.
            continue;
        }

        // Opening tag (no value on same line): <Tag> or <Tag attr="...">
        if let Some(tag) = parse_opening_tag(trimmed) {
            // The first opening tag is the document root — skip pushing it.
            if !root_seen {
                root_seen = true;
                continue;
            }
            stack.push(tag.to_string());
        }
    }

    Ok(None)
}

/// Patch the text value of an XML element located by `xml_path`.
///
/// Only the target line is rewritten; everything else stays byte-identical.
/// Missing path is an error and the file is left completely untouched.
///
/// Convention: the document root element (e.g. `<content>`) is NOT part of
/// `xml_path`. Paths start from children of the root.
pub fn patch_cemu_xml(
    path: &Path,
    xml_path: &[&str],
    new_value: &str,
) -> Result<CemuPatchResult, PatchError> {
    let content = read_file(path)?;
    let mut lines = split_lines(&content);
    let eol = detect_eol(&content);
    let trailing_newline = content.ends_with('\n');
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;
    let mut target_idx: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if is_self_closing(trimmed) {
            continue;
        }

        if let Some(tag) = parse_closing_tag(trimmed) {
            if stack.last().map(|s| s.as_str()) == Some(tag) {
                stack.pop();
            }
            continue;
        }

        if let Some((tag, _value)) = parse_leaf_tag(trimmed) {
            if stack.len() + 1 == xml_path.len()
                && stack.iter().zip(xml_path.iter()).all(|(s, p)| s == p)
                && tag == *xml_path.last().unwrap_or(&"")
            {
                if target_idx.is_some() {
                    // Duplicate path — ambiguous, refuse to patch.
                    return Err(PatchError::KeyNotFound {
                        path: path.to_path_buf(),
                        section: xml_path.join("/"),
                        key: "ambiguous path (duplicate match)".to_string(),
                    });
                }
                target_idx = Some(idx);
            }
            continue;
        }

        if let Some(tag) = parse_opening_tag(trimmed) {
            if !root_seen {
                root_seen = true;
                continue;
            }
            stack.push(tag.to_string());
        }
    }

    let Some(idx) = target_idx else {
        return Err(PatchError::KeyNotFound {
            path: path.to_path_buf(),
            section: xml_path.join("/"),
            key: xml_path.last().unwrap_or(&"").to_string(),
        });
    };

    let old_value = parse_leaf_tag(&lines[idx]).map(|(_, v)| v);

    // Rewrite ONLY the value between > and </ on this line.
    lines[idx] = replace_leaf_value(&lines[idx], new_value);

    let output = join_lines(&lines, eol, trailing_newline);
    write_atomic(path, output.as_bytes())?;

    Ok(CemuPatchResult {
        xml_path: xml_path.iter().map(|s| s.to_string()).collect(),
        old_value,
        new_value: new_value.to_string(),
    })
}

// ── helpers ────────────────────────────────────────────────────────────────

fn read_file(path: &Path) -> Result<String, PatchError> {
    fs::read_to_string(path).map_err(|source| PatchError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(str::to_string).collect()
}

fn join_lines(lines: &[String], eol: &str, trailing_newline: bool) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str(eol);
        }
        out.push_str(line);
    }
    if trailing_newline {
        out.push_str(eol);
    }
    out
}

fn detect_eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else if content.contains('\n') {
        "\n"
    } else if content.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PatchError> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        counter
    ));

    fs::write(&tmp_path, bytes).map_err(|source| PatchError::TempWrite {
        path: path.to_path_buf(),
        source,
    })?;
    if let Ok(file) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&tmp_path)
    {
        let _ = file.sync_all();
    }
    fs::rename(&tmp_path, path).map_err(|source| PatchError::Replace {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Returns true if the trimmed line is a self-closing tag like `<Tag/>` or
/// `<Tag attr="val"/>`.
fn is_self_closing(trimmed: &str) -> bool {
    if !trimmed.starts_with('<') || trimmed.len() < 3 {
        return false;
    }
    // Must not be a closing tag.
    if trimmed.starts_with("</") {
        return false;
    }
    trimmed.ends_with("/>")
}

/// Parse a closing tag `</Tag>` and return the tag name, or None.
fn parse_closing_tag(trimmed: &str) -> Option<&str> {
    if trimmed.starts_with("</") && trimmed.ends_with('>') && trimmed.len() >= 4 {
        Some(&trimmed[2..trimmed.len() - 1])
    } else {
        None
    }
}

/// Parse a leaf tag `<Tag>value</Tag>` and return (tag, value), or None.
fn parse_leaf_tag(trimmed: &str) -> Option<(&str, String)> {
    if !trimmed.starts_with('<') || trimmed.starts_with("</") || trimmed.starts_with("<?") {
        return None;
    }
    // Find the first '>' — end of opening tag.
    let open_end = trimmed.find('>')?;
    let tag = &trimmed[1..open_end];
    // Tag must not contain spaces (that would mean attributes, which we skip).
    if tag.contains(char::is_whitespace) || tag.ends_with('/') {
        return None;
    }
    let rest = &trimmed[open_end + 1..];
    // Find the closing tag `</tag>`.
    let close_pattern = format!("</{}>", tag);
    let close_pos = rest.find(&close_pattern)?;
    let value = rest[..close_pos].to_string();
    Some((tag, value))
}

/// Parse an opening tag `<Tag>` or `<Tag attr="...">` and return the tag name.
/// Returns None for closing tags, self-closing tags, or XML declarations.
fn parse_opening_tag(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('<')
        || trimmed.starts_with("</")
        || trimmed.starts_with("<?")
        || trimmed.ends_with("/>")
    {
        return None;
    }
    // Skip the '<', find the first space or '>'.
    let inner = &trimmed[1..];
    // The tag name is the first word before any space or closing '>'.
    let tag_end = inner.find(['>', ' ']).unwrap_or(inner.len());
    let tag = &inner[..tag_end];
    if tag.is_empty() {
        return None;
    }
    Some(tag)
}

/// Replace only the value text in a leaf line `<Tag>old</Tag>` → `<Tag>new</Tag>`.
/// Preserves indentation and the rest of the line exactly.
fn replace_leaf_value(line: &str, new_value: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];

    let open_end = trimmed.find('>').unwrap_or(0);
    let tag = &trimmed[1..open_end];
    let rest = &trimmed[open_end + 1..];
    let close_pattern = format!("</{}>", tag);
    let close_pos = rest.find(&close_pattern).unwrap_or(0);
    let after_close = &rest[close_pos + close_pattern.len()..];

    format!("{}<{}>{}</{}>{}", indent, tag, new_value, tag, after_close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Trimmed version of the real Cemu settings.xml, preserving the full
    /// `<Graphic>` and `<Audio>` sections (where the ambiguous `<api>` tag
    /// lives) plus enough surrounding context to exercise the stack.
    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<content>
    <logflag>0</logflag>
    <fullscreen>false</fullscreen>
    <console_language>5</console_language>
    <GameCache/>
    <Graphic>
        <api>1</api>
        <VSync>0</VSync>
        <UpscaleFilter>0</UpscaleFilter>
        <Overlay>
            <Position>1</Position>
            <FPS>true</FPS>
        </Overlay>
        <Notification>
            <Position>1</Position>
            <ShaderCompiling>true</ShaderCompiling>
        </Notification>
    </Graphic>
    <Audio>
        <api>3</api>
        <delay>2</delay>
    </Audio>
</content>
"#;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_cemu_xml_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn read_leaf_value_simple() {
        let path = temp_file("read_simple.xml", FIXTURE);
        let val = read_cemu_xml_value(&path, &["fullscreen"]).unwrap();
        assert_eq!(val.as_deref(), Some("false"));
    }

    #[test]
    fn read_leaf_value_nested() {
        let path = temp_file("read_nested.xml", FIXTURE);
        let val = read_cemu_xml_value(&path, &["Graphic", "api"]).unwrap();
        assert_eq!(val.as_deref(), Some("1"));
    }

    #[test]
    fn disambiguate_graphic_api_vs_audio_api() {
        let path = temp_file("disambig_api.xml", FIXTURE);
        let graphic = read_cemu_xml_value(&path, &["Graphic", "api"]).unwrap();
        let audio = read_cemu_xml_value(&path, &["Audio", "api"]).unwrap();
        assert_eq!(graphic.as_deref(), Some("1"));
        assert_eq!(audio.as_deref(), Some("3"));
    }

    #[test]
    fn disambiguate_overlay_position_vs_notification_position() {
        let path = temp_file("disambig_pos.xml", FIXTURE);
        let overlay = read_cemu_xml_value(&path, &["Graphic", "Overlay", "Position"]).unwrap();
        let notif = read_cemu_xml_value(&path, &["Graphic", "Notification", "Position"]).unwrap();
        assert_eq!(overlay.as_deref(), Some("1"));
        assert_eq!(notif.as_deref(), Some("1"));
    }

    #[test]
    fn read_nonexistent_path_returns_none() {
        let path = temp_file("read_none.xml", FIXTURE);
        let val = read_cemu_xml_value(&path, &["Graphic", "NoExiste"]).unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn patch_graphic_api_does_not_touch_audio_api() {
        let path = temp_file("patch_api.xml", FIXTURE);
        let before = fs::read_to_string(&path).unwrap();

        patch_cemu_xml(&path, &["Graphic", "api"], "0").unwrap();

        let after = fs::read_to_string(&path).unwrap();
        let expected = before.replace("<api>1</api>", "<api>0</api>");
        // Only ONE <api> line changes; the other stays identical.
        assert_eq!(after, expected);
        // Confirm Audio/api is still 3.
        assert_eq!(
            read_cemu_xml_value(&path, &["Audio", "api"])
                .unwrap()
                .as_deref(),
            Some("3")
        );
    }

    #[test]
    fn patch_overlay_position_does_not_touch_notification_position() {
        let path = temp_file("patch_pos.xml", FIXTURE);
        let before = fs::read_to_string(&path).unwrap();

        patch_cemu_xml(&path, &["Graphic", "Overlay", "Position"], "3").unwrap();

        let after = fs::read_to_string(&path).unwrap();
        // Only Overlay/Position changes; Notification/Position stays "1".
        assert!(
            after.contains("<Position>3</Position>"),
            "Overlay should be patched"
        );
        let notif_val =
            read_cemu_xml_value(&path, &["Graphic", "Notification", "Position"]).unwrap();
        assert_eq!(notif_val.as_deref(), Some("1"));
        // Line count unchanged.
        assert_eq!(before.lines().count(), after.lines().count());
    }

    #[test]
    fn patch_nonexistent_path_errors_and_leaves_file_intact() {
        let path = temp_file("patch_missing.xml", FIXTURE);
        let before = fs::read_to_string(&path).unwrap();

        let err = patch_cemu_xml(&path, &["Graphic", "NoExiste"], "99").unwrap_err();
        assert!(
            matches!(err, PatchError::KeyNotFound { .. }),
            "unexpected: {err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn patch_root_level_tag() {
        let path = temp_file("patch_root.xml", FIXTURE);
        patch_cemu_xml(&path, &["fullscreen"], "true").unwrap();
        assert_eq!(
            read_cemu_xml_value(&path, &["fullscreen"])
                .unwrap()
                .as_deref(),
            Some("true")
        );
    }

    #[test]
    fn three_consecutive_patches_preserve_file_integrity() {
        let path = temp_file("patch_consec.xml", FIXTURE);
        let before = fs::read_to_string(&path).unwrap();
        let original_line_count = before.lines().count();

        patch_cemu_xml(&path, &["Graphic", "api"], "0").unwrap();
        patch_cemu_xml(&path, &["Audio", "api"], "2").unwrap();
        patch_cemu_xml(&path, &["fullscreen"], "true").unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after.lines().count(), original_line_count);

        // Verify all three patches took effect.
        assert_eq!(
            read_cemu_xml_value(&path, &["Graphic", "api"])
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            read_cemu_xml_value(&path, &["Audio", "api"])
                .unwrap()
                .as_deref(),
            Some("2"),
            "Audio/api was patched to 2 in the second call"
        );

        // GraphicPack with Preset nesting stays byte-identical (not present in
        // trimmed fixture, but the general structure is preserved).
        assert!(after.contains("<VSync>0</VSync>"));
        assert!(after.contains("<console_language>5</console_language>"));
    }

    #[test]
    fn read_before_and_after_patch() {
        let path = temp_file("read_before_after.xml", FIXTURE);
        assert_eq!(
            read_cemu_xml_value(&path, &["Graphic", "VSync"])
                .unwrap()
                .as_deref(),
            Some("0")
        );
        patch_cemu_xml(&path, &["Graphic", "VSync"], "1").unwrap();
        assert_eq!(
            read_cemu_xml_value(&path, &["Graphic", "VSync"])
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn preserves_self_closing_tags_and_comments() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<content>
    <GameCache/>
    <fullscreen>false</fullscreen>
</content>
"#;
        let path = temp_file("preserve_selfclose.xml", xml);
        patch_cemu_xml(&path, &["fullscreen"], "true").unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("<GameCache/>"),
            "self-closing tag must survive"
        );
        assert!(after.contains("<fullscreen>true</fullscreen>"));
    }
}
