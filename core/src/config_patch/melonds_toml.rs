//! Surgical patcher for emulator config files stored as real TOML documents
//! (the format melonDS persists in `~/.config/melonDS/melonDS.toml`).
//!
//! Format contract this module understands:
//! - The whole file is one TOML document with top-level keys (`LimitFPS = true`)
//!   and `[Section]` / `[Section.Sub]` tables.
//! - Keys are addressed by full path from the root, e.g. `["3D", "Renderer"]`
//!   for `[3D] Renderer = 0` or `["Instance0", "Window0", "ScreenLayout"]` for
//!   `[Instance0.Window0] ScreenLayout = 0`. Dotted headers like `[3D.Soft]`
//!   are traversed as separate path elements.
//! - The patched value KEEPS its existing TOML type: booleans stay unquoted
//!   `true`/`false`, integers stay numeric, floats keep a decimal point, and
//!   strings stay double quoted.
//!
//! The file is parsed with `toml_edit` (`DocumentMut`), which preserves the
//! formatting and comments of every untouched key, so a patch only rewrites the
//! single target key. Writes are atomic (temp file + rename).
//!
//! This module never invents keys: if a path is missing it errors out and
//! leaves the file untouched.

use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use super::qt_ini::PatchError;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A TOML value read from the document, keeping its original type.
#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl TomlValue {
    /// Render the value back to its canonical on-disk representation: unquoted
    /// booleans/numbers, raw (unescaped) string content.
    pub fn to_file_string(&self) -> String {
        match self {
            TomlValue::Bool(b) => b.to_string(),
            TomlValue::Int(i) => i.to_string(),
            TomlValue::Float(f) => toml_float_string(*f),
            TomlValue::Str(s) => s.clone(),
        }
    }
}

/// Outcome of a successful TOML patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlPatchResult {
    /// Full key path patched (e.g. `["3D", "Renderer"]`).
    pub key_path: Vec<String>,
    /// Value that was on disk before the patch.
    pub old_value: Option<String>,
    /// Value written to disk.
    pub new_value: String,
}

/// Read the current typed value of a key located by `key_path`.
///
/// Returns `Ok(None)` when the path is absent — this is a pure, non-destructive
/// lookup. It never modifies the file.
pub fn read_melonds_toml_value(
    path: &Path,
    key_path: &[&str],
) -> Result<Option<TomlValue>, PatchError> {
    let content = read_file(path)?;
    let doc = parse_document(&content, path)?;
    Ok(lookup(&doc, key_path).and_then(toml_value))
}

/// Patch the value of a key located by `key_path`, atomically.
///
/// - Only the target key is rewritten; every other key keeps its exact on-disk
///   formatting (including arrays like `RecentROM`).
/// - The value keeps the TOML type it already had: a boolean stays `true`/
///   `false` (unquoted), an integer stays numeric, a float keeps a decimal
///   point, a string stays quoted.
/// - Missing path is an error and the file is left completely untouched.
pub fn patch_melonds_toml(
    path: &Path,
    key_path: &[&str],
    new_value: &str,
) -> Result<TomlPatchResult, PatchError> {
    let content = read_file(path)?;
    let mut doc = parse_document(&content, path)?;

    let dotted = key_path.join(".");
    let Some(original) = lookup(&doc, key_path) else {
        return Err(PatchError::TomlKeyNotFound {
            path: path.to_path_buf(),
            key: dotted,
        });
    };
    let old_value = toml_value(original).map(|v| v.to_file_string());
    let replacement = build_typed_value(original, new_value, path, &dotted)?;

    // Locate the target mutably and replace only that item. Every intermediate
    // table is already known to exist from the immutable walk above, so this
    // never invents new keys.
    let mut target = doc.as_item_mut();
    for key in key_path {
        target = target
            .get_mut(*key)
            .ok_or_else(|| PatchError::TomlKeyNotFound {
                path: path.to_path_buf(),
                key: dotted.clone(),
            })?;
    }
    *target = toml_edit::value(replacement);

    write_atomic(path, doc.to_string().as_bytes())?;

    Ok(TomlPatchResult {
        key_path: key_path.iter().map(|s| s.to_string()).collect(),
        old_value,
        new_value: new_value.to_string(),
    })
}

/// Walk `key_path` from the document root, returning the item found or `None`
/// if any element is missing. Never panics and never mutates the document.
fn lookup<'a>(doc: &'a toml_edit::DocumentMut, key_path: &[&str]) -> Option<&'a toml_edit::Item> {
    let mut item = doc.as_item();
    for key in key_path {
        item = item.get(*key)?;
    }
    if item.is_none() {
        return None;
    }
    Some(item)
}

/// Convert a TOML item into a typed [`TomlValue`]. Tables, arrays, inline
/// tables and datetimes are not representable and yield `None`.
fn toml_value(item: &toml_edit::Item) -> Option<TomlValue> {
    if let Some(b) = item.as_bool() {
        Some(TomlValue::Bool(b))
    } else if let Some(i) = item.as_integer() {
        Some(TomlValue::Int(i))
    } else if let Some(f) = item.as_float() {
        Some(TomlValue::Float(f))
    } else {
        item.as_str().map(|s| TomlValue::Str(s.to_string()))
    }
}

/// Build a TOML value for `new_value` typed to match the type the target key
/// already has on disk. `replacement` is the raw (non-decorated) value; the
/// assignment in [`patch_melonds_toml`] decides the on-disk quoting.
fn build_typed_value(
    original: &toml_edit::Item,
    new_value: &str,
    path: &Path,
    dotted: &str,
) -> Result<toml_edit::Value, PatchError> {
    if original.as_bool().is_some() {
        let b = match new_value {
            "true" => true,
            "false" => false,
            other => {
                return Err(invalid_value(path, dotted, other, "a boolean ('true' or 'false')"));
            }
        };
        Ok(toml_edit::Value::from(b))
    } else if original.as_integer().is_some() {
        let i: i64 = new_value.parse().map_err(|_| {
            invalid_value(path, dotted, new_value, "an integer")
        })?;
        Ok(toml_edit::Value::from(i))
    } else if original.as_float().is_some() {
        let f: f64 = new_value.parse().map_err(|_| {
            invalid_value(path, dotted, new_value, "a number")
        })?;
        Ok(toml_edit::Value::from(f))
    } else if original.as_str().is_some() {
        Ok(toml_edit::Value::from(new_value.to_string()))
    } else {
        Err(invalid_value(
            path,
            dotted,
            new_value,
            "a boolean, number or string",
        ))
    }
}

fn invalid_value(path: &Path, dotted: &str, value: &str, expected: &str) -> PatchError {
    PatchError::InvalidValue {
        path: path.to_path_buf(),
        key: dotted.to_string(),
        value: value.to_string(),
        expected: expected.to_string(),
    }
}

/// Format an `f64` the way the TOML encoder would, so a read value round-trips
/// byte-for-byte against the file (60.0 stays `60.0`, never `60`).
fn toml_float_string(f: f64) -> String {
    if f.is_nan() {
        if f.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        }
    } else if f == 0.0 {
        if f.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        }
    } else if f % 1.0 == 0.0 {
        format!("{f}.0")
    } else {
        f.to_string()
    }
}

fn read_file(path: &Path) -> Result<String, PatchError> {
    fs::read_to_string(path).map_err(|source| PatchError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_document(content: &str, path: &Path) -> Result<toml_edit::DocumentMut, PatchError> {
    toml_edit::DocumentMut::from_str(content).map_err(|err| PatchError::TomlParse {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Representative sample trimmed from a real `~/.config/melonDS/melonDS.toml`.
    /// Keeps the top-level scalars, `[3D]` with a dotted `[3D.Soft]` child, a
    /// `[JIT]` block, `[Screen]`, and `[Instance0.Window0]`, plus the
    /// `RecentROM` array that must survive patches byte-for-byte.
    const FIXTURE: &str = "\
LimitFPS = true
TargetFPS = 60.0
AudioSync = false
PauseLostFocus = false
UITheme = \"\"
RecentROM = [\"/home/carlos/Descargas/ROMs/nds/rom.nds\", \"/home/carlos/Descargas/ROMs/nds/rom2.nds\"]

[3D]
Renderer = 0

[3D.Soft]
Threaded = true

[JIT]
MaxBlockSize = 32
FastMemory = true
LiteralOptimisations = true
BranchOptimisations = true
Enable = false

[Screen]
UseGL = false

[Instance0.Window0]
Enabled = true
ScreenFilter = false
ScreenSizing = 0
ScreenLayout = 0
ShowOSD = true
";

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tui_game_station_melonds_toml_{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn patch_root_bool_key_keeps_boolean_type() {
        let path = temp_file("patch_root_bool.toml", FIXTURE);

        let res = patch_melonds_toml(&path, &["LimitFPS"], "false").unwrap();
        assert_eq!(res.key_path, vec!["LimitFPS".to_string()]);
        assert_eq!(res.old_value.as_deref(), Some("true"));
        assert_eq!(res.new_value, "false");

        let written = fs::read_to_string(&path).unwrap();
        let expected = FIXTURE.replace("LimitFPS = true", "LimitFPS = false");
        assert_eq!(written, expected, "only the target line may change");
        assert!(written.contains("LimitFPS = false"), "boolean stays unquoted");
        assert!(!written.contains("LimitFPS = \"false\""));
    }

    #[test]
    fn patch_one_level_nested_int_key_keeps_numeric_type() {
        let path = temp_file("patch_nested_int.toml", FIXTURE);

        let res = patch_melonds_toml(&path, &["3D", "Renderer"], "1").unwrap();
        assert_eq!(res.old_value.as_deref(), Some("0"));
        assert_eq!(res.new_value, "1");

        let written = fs::read_to_string(&path).unwrap();
        let expected = FIXTURE.replace("Renderer = 0", "Renderer = 1");
        assert_eq!(written, expected, "only the Renderer line may change");
        assert!(written.contains("\nRenderer = 1\n"));
        assert!(!written.contains("Renderer = \"1\""));
    }

    #[test]
    fn patch_two_level_nested_key_inside_instance_window() {
        let path = temp_file("patch_two_level.toml", FIXTURE);

        let res =
            patch_melonds_toml(&path, &["Instance0", "Window0", "ScreenLayout"], "2").unwrap();
        assert_eq!(res.old_value.as_deref(), Some("0"));
        assert_eq!(res.new_value, "2");

        let written = fs::read_to_string(&path).unwrap();
        let expected = FIXTURE.replace("ScreenLayout = 0", "ScreenLayout = 2");
        assert_eq!(written, expected, "only the ScreenLayout line may change");
        assert!(written.contains("ScreenLayout = 2"));
    }

    #[test]
    fn patch_float_and_string_preserve_their_types() {
        let path = temp_file("patch_float_string.toml", FIXTURE);

        // Float keeps a decimal point: 30.0, not 30.
        let res = patch_melonds_toml(&path, &["TargetFPS"], "30.0").unwrap();
        assert_eq!(res.old_value.as_deref(), Some("60.0"));
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("TargetFPS = 30.0"), "float keeps decimal point");
        assert!(!written.contains("TargetFPS = 30\n"));

        // String stays double quoted.
        let res = patch_melonds_toml(&path, &["UITheme"], "dark").unwrap();
        assert_eq!(res.old_value.as_deref(), Some(""));
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("UITheme = \"dark\""));
        assert!(!written.contains("UITheme = dark"));
    }

    #[test]
    fn missing_path_errors_and_leaves_file_untouched() {
        let path = temp_file("patch_missing_path.toml", FIXTURE);
        let original = fs::read_to_string(&path).unwrap();

        // Missing leaf in an existing table.
        let err = patch_melonds_toml(&path, &["3D", "NoSuchKey"], "1").unwrap_err();
        assert!(
            matches!(err, PatchError::TomlKeyNotFound { .. }),
            "unexpected error: {err}"
        );

        // Missing intermediate table.
        let err = patch_melonds_toml(&path, &["NoTable", "Renderer"], "1").unwrap_err();
        assert!(matches!(err, PatchError::TomlKeyNotFound { .. }));

        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn invalid_value_for_type_errors_and_leaves_file_untouched() {
        let path = temp_file("patch_bad_value.toml", FIXTURE);
        let original = fs::read_to_string(&path).unwrap();

        let err = patch_melonds_toml(&path, &["3D", "Renderer"], "not-a-number").unwrap_err();
        assert!(
            matches!(err, PatchError::InvalidValue { .. }),
            "unexpected error: {err}"
        );

        let err = patch_melonds_toml(&path, &["LimitFPS"], "1").unwrap_err();
        assert!(
            matches!(err, PatchError::InvalidValue { .. }),
            "boolean key must receive 'true'/'false', got '1': {err}"
        );

        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn patches_never_corrupt_recent_rom_array_or_other_sections() {
        let path = temp_file("patch_preserves_rest.toml", FIXTURE);

        patch_melonds_toml(&path, &["LimitFPS"], "false").unwrap();
        patch_melonds_toml(&path, &["3D", "Renderer"], "1").unwrap();
        patch_melonds_toml(&path, &["Instance0", "Window0", "ScreenLayout"], "3").unwrap();

        let written = fs::read_to_string(&path).unwrap();
        // The RecentROM array (a real TOML list) stays byte-identical.
        assert!(written.contains(
            "RecentROM = [\"/home/carlos/Descargas/ROMs/nds/rom.nds\", \"/home/carlos/Descargas/ROMs/nds/rom2.nds\"]"
        ));
        // Dotted child table and untouched blocks stay byte-identical.
        assert!(written.contains("[3D.Soft]\nThreaded = true"));
        assert!(written.contains("[JIT]\nMaxBlockSize = 32\nFastMemory = true"));
        assert!(written.contains("[Screen]\nUseGL = false"));

        // Exactly three lines differ from the original.
        let orig: Vec<&str> = FIXTURE.lines().collect();
        let new_lines: Vec<&str> = written.lines().collect();
        assert_eq!(orig.len(), new_lines.len());
        let diffs = orig
            .iter()
            .zip(new_lines.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diffs, 3);
    }

    #[test]
    fn read_before_and_after_for_bool_int_float_string() {
        let path = temp_file("read_before_after.toml", FIXTURE);

        assert_eq!(
            read_melonds_toml_value(&path, &["LimitFPS"]).unwrap(),
            Some(TomlValue::Bool(true))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["3D", "Renderer"]).unwrap(),
            Some(TomlValue::Int(0))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["TargetFPS"]).unwrap(),
            Some(TomlValue::Float(60.0))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["UITheme"]).unwrap(),
            Some(TomlValue::Str(String::new()))
        );
        // Missing path reads as None, never an error.
        assert_eq!(read_melonds_toml_value(&path, &["Missing"]).unwrap(), None);
        assert_eq!(
            read_melonds_toml_value(&path, &["NoTable", "Renderer"]).unwrap(),
            None
        );

        // File-string round-trips must match the on-disk representation.
        assert_eq!(
            read_melonds_toml_value(&path, &["TargetFPS"])
                .unwrap()
                .map(|v| v.to_file_string())
                .as_deref(),
            Some("60.0")
        );

        patch_melonds_toml(&path, &["LimitFPS"], "false").unwrap();
        patch_melonds_toml(&path, &["3D", "Renderer"], "2").unwrap();
        patch_melonds_toml(&path, &["TargetFPS"], "59.94").unwrap();
        patch_melonds_toml(&path, &["UITheme"], "dark").unwrap();

        assert_eq!(
            read_melonds_toml_value(&path, &["LimitFPS"]).unwrap(),
            Some(TomlValue::Bool(false))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["3D", "Renderer"]).unwrap(),
            Some(TomlValue::Int(2))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["TargetFPS"]).unwrap(),
            Some(TomlValue::Float(59.94))
        );
        assert_eq!(
            read_melonds_toml_value(&path, &["UITheme"]).unwrap(),
            Some(TomlValue::Str("dark".to_string()))
        );
    }

    #[test]
    fn missing_file_returns_read_error() {
        let path = std::env::temp_dir().join("tui_game_station_melonds_definitely_missing.toml");
        let _ = fs::remove_file(&path);
        let err = patch_melonds_toml(&path, &["LimitFPS"], "false").unwrap_err();
        assert!(matches!(err, PatchError::Read { .. }));
    }
}
