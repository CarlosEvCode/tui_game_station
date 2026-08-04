//! Minimal, surgical patcher for Qt-style INI files (the format Eden and other
//! yuzu-family emulators use for `qt-config.ini`).
//!
//! Format contract this module understands:
//! - Sections look like `[Renderer]` and keys live until the next section.
//! - Keys may contain sub-groups separated by backslashes, e.g.
//!   `Shortcuts\Main Window\Fullscreen\KeySeq=F11`. The whole text before the
//!   first `=` is treated as ONE literal unique key per section; the backslash
//!   is NOT interpreted as nesting.
//! - A value is `key=value`; the value may be double quoted (`log_filter="*:Info"`).
//! - Many keys have a sibling `key\default=true|false` marking whether the value
//!   is still the factory default.
//!
//! The file is always handled line-by-line (raw `Vec<String>`), so an edit only
//! ever touches the target line and its `\default` sibling; everything else is
//! rewritten byte-for-byte identical. Writes are atomic (temp file + rename).
//!
//! This module never invents keys: if a section or key is missing it errors out
//! and leaves the file untouched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of a successful, applied patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    /// Section the key was patched in (as given by the caller).
    pub section: String,
    /// Full key path patched (as given by the caller).
    pub key: String,
    /// Value that was on disk before the patch (unquoted, unescaped).
    pub old_value: Option<String>,
    /// Value written to disk.
    pub new_value: String,
    /// Whether a sibling `key\default` line existed and was set to `false`.
    pub sibling_default_updated: bool,
}

/// Errors produced while reading or patching a Qt INI file.
#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("failed to read config file '{path}': {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write temporary file for '{path}': {source}")]
    TempWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace config file '{path}': {source}")]
    Replace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("config file '{path}' has no section [{section}]")]
    SectionNotFound { path: PathBuf, section: String },
    #[error("config file '{path}' has no key '{key}' in section [{section}]")]
    KeyNotFound {
        path: PathBuf,
        section: String,
        key: String,
    },
    #[error("failed to parse config file '{path}' as TOML: {message}")]
    TomlParse { path: PathBuf, message: String },
    #[error("config file '{path}' has no TOML key '{key}'")]
    TomlKeyNotFound { path: PathBuf, key: String },
    #[error("config file '{path}' key '{key}': cannot write value '{value}' (expected {expected})")]
    InvalidValue {
        path: PathBuf,
        key: String,
        value: String,
        expected: String,
    },
}

/// Read the current (unquoted, unescaped) value of `section`/`key`.
///
/// Returns `Ok(None)` when the section or key is absent — this is a pure,
/// non-destructive lookup. It never modifies the file.
pub fn read_qt_ini_value(
    path: &Path,
    section: &str,
    key: &str,
) -> Result<Option<String>, PatchError> {
    let content = read_file(path)?;
    let lines = split_lines(&content);
    let section = section.trim();
    let key = key.trim();

    let Some((start, end)) = find_section_range(&lines, section) else {
        return Ok(None);
    };
    let Some(idx) = find_key_line(&lines, start, end, key) else {
        return Ok(None);
    };
    Ok(Some(decode_value(&lines[idx]).0))
}

/// Patch the value of `section`/`key` in place, atomically.
///
/// - Only the target line and its sibling `key\default` (set to `false`) change.
/// - The existing quoting convention of the target line is preserved.
/// - Missing section/key is an error and the file is left completely untouched.
pub fn patch_qt_ini(
    path: &Path,
    section: &str,
    key: &str,
    new_value: &str,
) -> Result<PatchResult, PatchError> {
    let content = read_file(path)?;
    let mut lines = split_lines(&content);
    let eol = detect_eol(&content);
    let trailing_newline = content.ends_with('\n');
    let section = section.trim();
    let key = key.trim();

    let Some((start, end)) = find_section_range(&lines, section) else {
        return Err(PatchError::SectionNotFound {
            path: path.to_path_buf(),
            section: section.to_string(),
        });
    };
    let Some(target) = find_key_line(&lines, start, end, key) else {
        return Err(PatchError::KeyNotFound {
            path: path.to_path_buf(),
            section: section.to_string(),
            key: key.to_string(),
        });
    };

    let (old_value, quoted) = decode_value(&lines[target]);
    let replacement = build_line(&lines[target], new_value, quoted);
    lines[target] = replacement;

    let mut sibling_default_updated = false;
    let sibling_key = format!(r"{}\default", key);
    if let Some(sibling) = find_key_line(&lines, start, end, &sibling_key) {
        lines[sibling] = build_line(&lines[sibling], "false", false);
        sibling_default_updated = true;
    }

    let output = join_lines(&lines, eol, trailing_newline);
    write_atomic(path, output.as_bytes())?;

    Ok(PatchResult {
        section: section.to_string(),
        key: key.to_string(),
        old_value: Some(old_value),
        new_value: new_value.to_string(),
        sibling_default_updated,
    })
}

fn read_file(path: &Path) -> Result<String, PatchError> {
    fs::read_to_string(path).map_err(|source| PatchError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Split raw content into lines, dropping the trailing newline of each line.
/// Internal blank lines are preserved as empty strings.
fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(str::to_string).collect()
}

/// Reassemble lines back into content using the file's dominant EOL and its
/// original "ends with newline" state, so the on-disk form is preserved.
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

/// Write `bytes` to a temp file next to `path`, fsync it, then rename over the
/// original. The original is never left half-written.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PatchError> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(".{}.{}.{}.tmp", file_name, std::process::id(), counter));

    fs::write(&tmp_path, bytes).map_err(|source| PatchError::TempWrite {
        path: path.to_path_buf(),
        source,
    })?;
    if let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&tmp_path) {
        let _ = file.sync_all();
    }
    fs::rename(&tmp_path, path).map_err(|source| PatchError::Replace {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Find the `[body_start, body_end)` line range of a section by exact name.
fn find_section_range(lines: &[String], section: &str) -> Option<(usize, usize)> {
    for idx in 0..lines.len() {
        if parse_section_name(lines[idx].trim()).is_some_and(|name| name == section) {
            let mut end = idx + 1;
            while end < lines.len() && parse_section_name(lines[end].trim()).is_none() {
                end += 1;
            }
            return Some((idx + 1, end));
        }
    }
    None
}

/// Extract the section name from a trimmed line, or `None` if it is not a
/// section header. Lines that carry a value (contain `=`) are never sections.
fn parse_section_name(trimmed: &str) -> Option<&str> {
    if trimmed.len() >= 2
        && trimmed.starts_with('[')
        && trimmed.ends_with(']')
        && !trimmed.contains('=')
    {
        Some(&trimmed[1..trimmed.len() - 1])
    } else {
        None
    }
}

/// Find the line index whose key (text before the first `=`, trimmed) exactly
/// equals `key`, scanning only `[start, end)`. Comment-like keys (`;`/`#`) and
/// lines without `=` are skipped.
fn find_key_line(lines: &[String], start: usize, end: usize, key: &str) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate().take(end).skip(start) {
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key_part = line[..eq].trim();
        if key_part.is_empty()
            || key_part.starts_with(';')
            || key_part.starts_with('#')
            || key_part != key
        {
            continue;
        }
        return Some(idx);
    }
    None
}

/// Decode the current value of a key line. Returns `(logical_value, quoted)`.
/// A leading/trailing `"` means the line was written quoted; the logical value
/// is the unescaped text inside those quotes.
fn decode_value(line: &str) -> (String, bool) {
    let eq = line.find('=').expect("key line always contains '='");
    let raw = line[eq + 1..].trim();
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        (unescape_qt(&raw[1..raw.len() - 1]), true)
    } else {
        (raw.to_string(), false)
    }
}

/// Rebuild a key line replacing ONLY the value, keeping the key, the spacing
/// around `=`, and any trailing whitespace exactly as they were. When `quoted`
/// is true the new value is written inside double quotes (matching the original
/// convention).
fn build_line(line: &str, new_value: &str, quoted: bool) -> String {
    let eq = line.find('=').expect("key line always contains '='");
    let after_eq = &line[eq + 1..];
    let lead_len = after_eq.len() - after_eq.trim_start().len();
    let trimmed = after_eq.trim();
    let trail_start = lead_len + trimmed.len();
    let rendered = if quoted {
        format!("\"{}\"", escape_qt(new_value))
    } else {
        new_value.to_string()
    };
    format!(
        "{}{}{}",
        &line[..eq + 1 + lead_len],
        rendered,
        &after_eq[trail_start..]
    )
}

/// Escape a value the way QSettings would inside double quotes.
fn escape_qt(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Undo QSettings escaping of a quoted value.
fn unescape_qt(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Representative sample of a real Eden `qt-config.ini`: full `[Renderer]`
    /// block with `backend`, `[System]` with `use_docked_mode`, a quoted value
    /// with spaces (`log_filter`), and `[UI]` with backslash sub-group keys
    /// (shortcuts) that must stay byte-identical after unrelated patches.
    const FIXTURE: &str = "\
[Renderer]
backend=1
backend\\default=true
async=1
async\\default=true
cpu_debug_mode=false
debug_flags=\"\"
disable_fps_limiter=false
disable_ring_buffer=false
enable_sanitizers=false
enable_shader_feedback=false
extended_logging=false
fullscreen_mode=0
fullscreen_mode\\default=true
gamma=1.0000000000
gpu_debugging=false
perf_stats=false
renderer=opengl
renderer\\default=true
res_info=false
resolution_factor=1
resolution_factor\\default=true
use_disk_shader_cache=true
use_disk_shader_cache\\default=true
use_driver_cache_manager=true
use_speed_limiter=true
use_speed_limiter\\default=true
use_vsync=true
use_vsync\\default=true

[System]
use_docked_mode=true
use_docked_mode\\default=true
language=0
region_index=1

[Logging]
log_filter\\default=true
log_filter=\"*:Info\"

[UI]
UIGameList\\game_icon_size=64
UIGameList\\game_icon_size\\default=true
UIGameList\\row_height=80
UIGameList\\row_height\\default=true
Shortcuts\\Main Window\\Fullscreen\\KeySeq=F11
Shortcuts\\Main Window\\Fullscreen\\KeySeq\\default=true
Shortcuts\\Main Window\\Fullscreen\\Type=Shortcut
Shortcuts\\Main Window\\Game List\\Clear Game List\\KeySeq=
Shortcuts\\Main Window\\Game List\\Clear Game List\\KeySeq\\default=true
Shortcuts\\Main Window\\Game List\\Clear Game List\\Type=Shortcut
";

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tui_game_station_qt_ini_{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn patch_changes_only_target_line_and_sibling() {
        let path = temp_file("patch_only_target.ini", FIXTURE);
        let expected = FIXTURE
            .replace("backend=1", "backend=0")
            .replace("backend\\default=true", "backend\\default=false");

        let res = patch_qt_ini(&path, "Renderer", "backend", "0").unwrap();

        assert_eq!(res.section, "Renderer");
        assert_eq!(res.key, "backend");
        assert_eq!(res.old_value.as_deref(), Some("1"));
        assert_eq!(res.new_value, "0");
        assert!(res.sibling_default_updated);

        let written = fs::read_to_string(&path).unwrap();
        // Whole-file equality with only the two expected lines rewritten proves
        // the diff is line-for-line minimal (Shortcuts block included).
        assert_eq!(written, expected);

        let orig_lines: Vec<&str> = FIXTURE.lines().collect();
        let new_lines: Vec<&str> = written.lines().collect();
        assert_eq!(orig_lines.len(), new_lines.len());
        let diffs = orig_lines
            .iter()
            .zip(new_lines.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diffs, 2);
    }

    #[test]
    fn sibling_default_becomes_false() {
        let path = temp_file("sibling_default.ini", FIXTURE);
        patch_qt_ini(&path, "System", "use_docked_mode", "false").unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("use_docked_mode=false\n"));
        assert!(written.contains("use_docked_mode\\default=false\n"));
        assert_eq!(
            read_qt_ini_value(&path, "System", "use_docked_mode\\default")
                .unwrap()
                .as_deref(),
            Some("false")
        );
    }

    #[test]
    fn key_without_sibling_does_not_fabricate_one() {
        let path = temp_file("no_sibling.ini", FIXTURE);
        let res = patch_qt_ini(&path, "System", "language", "1").unwrap();
        assert!(!res.sibling_default_updated);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("language=1\n"));
        assert!(!written.contains("language\\default"));
    }

    #[test]
    fn missing_key_errors_and_leaves_file_untouched() {
        let path = temp_file("patch_missing_key.ini", FIXTURE);
        let original = fs::read_to_string(&path).unwrap();

        let err = patch_qt_ini(&path, "Renderer", "no_such_key", "1").unwrap_err();
        assert!(
            matches!(err, PatchError::KeyNotFound { .. }),
            "unexpected error: {err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn missing_section_errors_and_leaves_file_untouched() {
        let path = temp_file("patch_missing_section.ini", FIXTURE);
        let original = fs::read_to_string(&path).unwrap();

        let err = patch_qt_ini(&path, "NoSection", "backend", "1").unwrap_err();
        assert!(
            matches!(err, PatchError::SectionNotFound { .. }),
            "unexpected error: {err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn patch_quoted_value_preserves_quotes_and_stays_valid() {
        let path = temp_file("patch_quoted.ini", FIXTURE);

        let res = patch_qt_ini(&path, "Logging", "log_filter", "*:Debug").unwrap();
        assert_eq!(res.old_value.as_deref(), Some("*:Info"));
        assert!(res.sibling_default_updated);

        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("log_filter=\"*:Debug\"\n"));
        assert!(written.contains("log_filter\\default=false\n"));

        // The result must still be parseable by the module itself.
        assert_eq!(
            read_qt_ini_value(&path, "Logging", "log_filter")
                .unwrap()
                .as_deref(),
            Some("*:Debug")
        );
    }

    #[test]
    fn unquoted_key_patch_stays_unquoted() {
        let path = temp_file("patch_unquoted.ini", FIXTURE);
        patch_qt_ini(&path, "System", "region_index", "2").unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("region_index=2\n"));
        assert!(!written.contains("region_index=\"2\""));
    }

    #[test]
    fn read_value_before_and_after_patch() {
        let path = temp_file("read_before_after.ini", FIXTURE);

        assert_eq!(
            read_qt_ini_value(&path, "Renderer", "backend").unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(
            read_qt_ini_value(&path, "System", "use_docked_mode")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        // Missing key and missing section both read as None (never an error).
        assert_eq!(read_qt_ini_value(&path, "Renderer", "missing").unwrap(), None);
        assert_eq!(
            read_qt_ini_value(&path, "MissingSection", "backend").unwrap(),
            None
        );

        patch_qt_ini(&path, "System", "use_docked_mode", "false").unwrap();
        assert_eq!(
            read_qt_ini_value(&path, "System", "use_docked_mode")
                .unwrap()
                .as_deref(),
            Some("false")
        );
    }

    #[test]
    fn consecutive_patches_never_duplicate_or_lose_lines() {
        let path = temp_file("consecutive_patches.ini", FIXTURE);
        let original_count = FIXTURE.lines().count();

        patch_qt_ini(&path, "Renderer", "backend", "0").unwrap();
        patch_qt_ini(&path, "System", "use_docked_mode", "false").unwrap();
        patch_qt_ini(&path, "UI", r"Shortcuts\Main Window\Fullscreen\KeySeq", "F10").unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written.lines().count(), original_count);
        assert!(written.contains("backend=0\n"));
        assert!(written.contains("use_docked_mode=false\n"));
        assert!(written.contains(r"Shortcuts\Main Window\Fullscreen\KeySeq=F10"));
        assert_eq!(
            read_qt_ini_value(&path, "UI", r"Shortcuts\Main Window\Fullscreen\KeySeq")
                .unwrap()
                .as_deref(),
            Some("F10")
        );
    }

    #[test]
    fn patches_sub_group_key_with_backslashes() {
        let path = temp_file("sub_group_key.ini", FIXTURE);
        let res = patch_qt_ini(
            &path,
            "UI",
            r"Shortcuts\Main Window\Game List\Clear Game List\KeySeq",
            "Ctrl+L",
        )
        .unwrap();
        assert_eq!(
            res.old_value.as_deref(),
            Some(""),
            "empty value should be read as empty string, not None"
        );
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains(
            "Shortcuts\\Main Window\\Game List\\Clear Game List\\KeySeq=Ctrl+L\n"
        ));
        assert!(written.contains(
            "Shortcuts\\Main Window\\Game List\\Clear Game List\\KeySeq\\default=false\n"
        ));
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let content = FIXTURE.replace('\n', "\r\n");
        let path = temp_file("crlf.ini", &content);

        patch_qt_ini(&path, "Renderer", "backend", "0").unwrap();

        let expected = content
            .replace("backend=1", "backend=0")
            .replace("backend\\default=true", "backend\\default=false");
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, expected);
        // No bare LF survives: every line still ends with CRLF.
        assert!(!written.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn missing_file_returns_read_error() {
        let path = std::env::temp_dir().join("tui_game_station_definitely_missing.ini");
        let _ = fs::remove_file(&path);
        let err = patch_qt_ini(&path, "Renderer", "backend", "0").unwrap_err();
        assert!(matches!(err, PatchError::Read { .. }));
    }
}
