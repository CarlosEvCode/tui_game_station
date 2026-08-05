//! Emulator launch options defined as DATA (TOML), never as per-emulator code.
//!
//! Each emulator ships a `{name}.toml` under `assets/emulators/`. Every option
//! has a stable `key`, a human-readable `name`, a `kind` (`toggle`/`choice`,
//! extensible), and a `default`.
//!
//! An option can reach the emulator through two INDEPENDENT channels (either,
//! both, or neither — this TOML decides):
//! - `flag_template`: expanded into CLI flags whenever the chosen value differs
//!   from the default (passed at launch time).
//! - `config_target`: a section/key inside the emulator's own config file that
//!   the launcher edits directly (patched at save time via `config_patch`).
//!
//! The selection is persisted as JSON inside the runner's `env_vars` column,
//! alongside optional custom launcher arguments.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Map of option `key` -> chosen value. Built from the TOML defaults and
/// persisted (filtered of defaults) as JSON on the runner row.
pub type RunnerOptions = BTreeMap<String, String>;

/// The kind of an option and the values it can hold.
#[derive(Debug, Clone, PartialEq)]
pub enum EmulatorOptionKind {
    /// A boolean flag. `default` is the "off" value ("0"), "1" means enabled.
    Toggle,
    /// Pick one value from a fixed list (e.g. renderer backend).
    Choice(Vec<String>),
}

/// Where (and how) an option is written into the emulator's own config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigTarget {
    /// Path to the emulator config file. A leading `~` expands to the user home.
    pub file: String,
    /// Optional alternative to `file`: candidate config paths, resolved at
    /// read/patch time. When exactly one exists it is used; when several exist
    /// the most recently modified one wins; when none exist the target is
    /// skipped (non-blocking). A leading `~` expands to the user home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_candidates: Option<Vec<String>>,
    /// Config format: `"qt_ini"` or `"cemu_xml"`.
    pub format: String,
    /// Section in the config file (qt_ini only, e.g. "Renderer").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Key in the section (qt_ini only, e.g. "backend").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Full XML path from root (cemu_xml only, e.g. ["Graphic", "api"]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml_path: Option<Vec<String>>,
    /// Full dotted path from root (melonds_toml only, e.g. ["3D", "Renderer"]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toml_path: Option<Vec<String>>,
    /// Translates the logical option value (e.g. "vulkan") to the value the
    /// config file expects (e.g. "1"). Absent/empty means identity mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_map: Option<BTreeMap<String, String>>,
}

/// A single emulator option loaded from its TOML definition.
#[derive(Debug, Clone, PartialEq)]
pub struct EmulatorOption {
    pub key: String,
    pub name: String,
    pub kind: EmulatorOptionKind,
    pub default: String,
    /// CLI flag expansion; empty string when the option is config-file only.
    pub flag_template: String,
    /// Optional CLI value -> token map. When set, `{value}` in `flag_template`
    /// is replaced with the token for the current value (e.g. a MAME toggle
    /// maps "1" -> "filter" / "0" -> "nofilter"). Unlike every other option,
    /// an option with this map ALWAYS emits a flag: both toggle states need an
    /// explicit token, so "off" is `-nofilter`, never a missing flag.
    pub value_map: Option<BTreeMap<String, String>>,
    /// Optional config-file target; when set, saving also patches that file.
    pub config_target: Option<ConfigTarget>,
    /// Friendly display labels for Choice values (value -> label).
    pub choice_labels: BTreeMap<String, String>,
}

/// A config-file patch that failed to apply (non-blocking by design).
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigPatchFailure {
    /// Option `key` whose config_target patch failed.
    pub option_key: String,
    /// Human-readable reason (section/key not found, file missing, ...).
    pub message: String,
}

/// Everything stored in the runner's `env_vars` column for emulator options.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunnerOptionEnv {
    /// Selected options, with default entries already removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emulator_options: Option<RunnerOptions>,
    /// Free-form extra CLI arguments (space separated, quotes respected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_args: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOptionsFile {
    #[serde(rename = "note")]
    _note: Option<String>,
    /// Real process name the emulator runs as once launched (e.g. "azahar",
    /// "dolphin-emu"). Used by the launcher to tell the real game process
    /// apart from AppImage mount/runtime helpers like `memfd:dwarfs`.
    #[serde(default)]
    process_name: Option<String>,
    #[serde(default)]
    options: Vec<RawOption>,
}

#[derive(Debug, Deserialize)]
struct RawOption {
    key: String,
    name: String,
    kind: String,
    #[serde(default)]
    choices: Vec<RawChoice>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    flag_template: String,
    #[serde(default)]
    value_map: BTreeMap<String, String>,
    #[serde(default)]
    config_target: Option<RawConfigTarget>,
}

/// A `choices` entry can be a plain string (label == value) or a table with a
/// distinct friendly label.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Simple(String),
    Labeled {
        value: String,
        label: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct RawConfigTarget {
    #[serde(default)]
    file: String,
    #[serde(default)]
    file_candidates: Option<Vec<String>>,
    format: String,
    #[serde(default)]
    section: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    xml_path: Option<Vec<String>>,
    #[serde(default)]
    toml_path: Option<Vec<String>>,
    #[serde(default)]
    value_map: BTreeMap<String, String>,
}

/// Embedded TOML source for an emulator, keyed by its canonical name.
fn emulator_toml_source(key: &str) -> Option<&'static str> {
    match key {
        "azahar" => Some(include_str!("../../assets/emulators/azahar.toml")),
        "eden" => Some(include_str!("../../assets/emulators/eden.toml")),
        "duckstation" => Some(include_str!("../../assets/emulators/duckstation.toml")),
        "pcsx2" => Some(include_str!("../../assets/emulators/pcsx2.toml")),
        "cemu" => Some(include_str!("../../assets/emulators/cemu.toml")),
        "ppsspp" => Some(include_str!("../../assets/emulators/ppsspp.toml")),
        "melonds" => Some(include_str!("../../assets/emulators/melonds.toml")),
        "dolphin" => Some(include_str!("../../assets/emulators/dolphin.toml")),
        "mame" => Some(include_str!("../../assets/emulators/mame.toml")),
        _ => None,
    }
}

/// Load the option definitions embedded for an emulator by its display name.
/// Unknown emulators return an empty list (no options popup section).
pub fn load_emulator_options(name: &str) -> anyhow::Result<Vec<EmulatorOption>> {
    let key = canonical_emulator_key(name);
    let Some(source) = emulator_toml_source(&key) else {
        return Ok(Vec::new());
    };

    let raw: RawOptionsFile = toml::from_str(source)?;
    let mut out = Vec::new();
    for opt in raw.options {
        let choice_pairs = opt
            .choices
            .iter()
            .map(|c| match c {
                RawChoice::Simple(value) => (value.clone(), value.clone()),
                RawChoice::Labeled { value, label } => (
                    value.clone(),
                    label.clone().unwrap_or_else(|| value.clone()),
                ),
            })
            .collect::<Vec<_>>();
        let kind = match opt.kind.as_str() {
            "choice" => {
                if choice_pairs.is_empty() {
                    anyhow::bail!(
                        "choice option '{}' in emulator '{}' has no choices",
                        opt.key,
                        name
                    );
                }
                EmulatorOptionKind::Choice(choice_pairs.iter().map(|(v, _)| v.clone()).collect())
            }
            _ => EmulatorOptionKind::Toggle,
        };
        let default = opt.default.unwrap_or_else(|| match kind {
            EmulatorOptionKind::Toggle => "0".to_string(),
            EmulatorOptionKind::Choice(_) => choice_pairs
                .first()
                .map(|(v, _)| v.clone())
                .unwrap_or_default(),
        });
        let value_map = if opt.value_map.is_empty() {
            None
        } else {
            Some(opt.value_map)
        };
        validate_cli_value_map(name, &opt.key, &value_map, &opt.flag_template)?;
        let config_target = opt.config_target.map(|ct| ConfigTarget {
            file: ct.file,
            file_candidates: ct.file_candidates,
            format: ct.format,
            section: ct.section,
            key: ct.key,
            xml_path: ct.xml_path,
            toml_path: ct.toml_path,
            value_map: if ct.value_map.is_empty() {
                None
            } else {
                Some(ct.value_map)
            },
        });
        let choice_labels = match &kind {
            EmulatorOptionKind::Choice(_) => choice_pairs
                .into_iter()
                .collect::<BTreeMap<String, String>>(),
            EmulatorOptionKind::Toggle => BTreeMap::new(),
        };
        out.push(EmulatorOption {
            key: opt.key,
            name: opt.name,
            kind,
            default,
            flag_template: opt.flag_template,
            value_map,
            config_target,
            choice_labels,
        });
    }
    Ok(out)
}

/// Real process name the emulator runs as once launched (defined per emulator
/// in its TOML as `process_name`), so the launcher can identify the actual
/// game process among AppImage mount/runtime helpers. Unknown emulators -> None.
pub fn emulator_process_name(name: &str) -> Option<String> {
    let key = canonical_emulator_key(name);
    let source = emulator_toml_source(&key)?;
    let raw: RawOptionsFile = toml::from_str(source).ok()?;
    raw.process_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

/// Build a full map with every option set to its default value.
pub fn default_map(options: &[EmulatorOption]) -> RunnerOptions {
    options
        .iter()
        .map(|o| (o.key.clone(), o.default.clone()))
        .collect()
}

/// Keep only entries whose value differs from the option default.
pub fn filter_default_map(options: &[EmulatorOption], map: &RunnerOptions) -> RunnerOptions {
    let mut out = map.clone();
    for opt in options {
        if out.get(&opt.key).map(|v| v == &opt.default).unwrap_or(true) {
            out.remove(&opt.key);
        }
    }
    out
}

/// Merge a stored (filtered) map over the defaults, validating every value so a
/// stale or corrupt value never produces an invalid flag.
pub fn merge_runner_options(options: &[EmulatorOption], stored: &RunnerOptions) -> RunnerOptions {
    let mut merged = default_map(options);
    for (key, value) in stored {
        if let Some(opt) = options.iter().find(|o| &o.key == key) {
            if value_is_valid(opt, value) {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

/// Whether a value is acceptable for an option (valid toggle / in choices).
pub fn value_is_valid(opt: &EmulatorOption, value: &str) -> bool {
    match &opt.kind {
        EmulatorOptionKind::Toggle => value == "0" || value == "1",
        EmulatorOptionKind::Choice(choices) => choices.iter().any(|c| c == value),
    }
}

/// Expand a `config_target.file` path. A leading `~`/`~/` is resolved against
/// the user's home directory; any other path is returned as-is.
///
/// Eden stores its Qt settings at `~/.config/eden/qt-config.ini` (QSettings
/// default location; confirmed on this system — see `assets/emulators/eden.toml`).
pub fn resolve_config_file(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path));
    }
    PathBuf::from(path)
}

/// Pick the config file to use among `candidates`: the only one when exactly one
/// exists, otherwise the most recently modified one. Returns `None` when no
/// candidate exists (the caller treats that as "no config file" and skips).
///
/// Used by emulators that ship as two variants with separate fixed config
/// directories (e.g. Azahar standard vs. Azahar Plus): which one is in use is
/// decided by what exists on disk at read/patch time, never cached.
pub fn resolve_config_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    let existing: Vec<&PathBuf> = candidates.iter().filter(|c| c.exists()).collect();
    match existing.len() {
        0 => None,
        1 => Some(existing[0].clone()),
        _ => {
            let mut best: Option<(&PathBuf, std::time::SystemTime)> = None;
            for path in existing {
                if let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) {
                    if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                        best = Some((path, mtime));
                    }
                }
            }
            best.map(|(path, _)| path.clone())
                .or_else(|| candidates.first().cloned())
        }
    }
}

/// Resolve the config file a target actually points at: `file_candidates` are
/// resolved by existence / most-recent mtime when present and non-empty,
/// otherwise `file` is used unchanged (backwards compatible). Returns `None`
/// when nothing usable exists (callers skip the target, non-blocking).
pub fn resolve_config_target_path(target: &ConfigTarget) -> Option<PathBuf> {
    if let Some(candidates) = &target.file_candidates {
        if !candidates.is_empty() {
            let paths: Vec<PathBuf> = candidates.iter().map(|c| resolve_config_file(c)).collect();
            return resolve_config_path(&paths);
        }
    }
    if target.file.is_empty() {
        return None;
    }
    Some(resolve_config_file(&target.file))
}

/// Read the REAL current value of a config-target option straight from the
/// emulator's config file, translated back to the logical option value.
///
/// Returns `None` when the option has no config target, the file is missing, or
/// the key/section is not present (callers fall back to the TOML `default`).
pub fn read_config_value(opt: &EmulatorOption) -> Option<String> {
    let target = opt.config_target.as_ref()?;
    let path = resolve_config_target_path(target)?;
    let raw = read_raw_value(&path, target).ok().flatten()?;
    match &target.value_map {
        Some(map) => map
            .iter()
            .find(|(_, file_value)| *file_value == &raw)
            .map(|(logical, _)| logical.clone()),
        None => Some(raw),
    }
}

/// Dispatch a raw read to the appropriate format-specific backend.
fn read_raw_value(
    path: &std::path::Path,
    target: &ConfigTarget,
) -> Result<Option<String>, crate::config_patch::qt_ini::PatchError> {
    match target.format.as_str() {
        "cemu_xml" => {
            let xml_path = target.xml_path.as_ref().ok_or_else(|| {
                crate::config_patch::qt_ini::PatchError::KeyNotFound {
                    path: path.to_path_buf(),
                    section: "cemu_xml".to_string(),
                    key: "missing xml_path in config_target".to_string(),
                }
            })?;
            crate::config_patch::cemu_xml::read_cemu_xml_value(
                path,
                &xml_path.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            )
        }
        "melonds_toml" => {
            let toml_path = target.toml_path.as_ref().ok_or_else(|| {
                crate::config_patch::qt_ini::PatchError::KeyNotFound {
                    path: path.to_path_buf(),
                    section: "melonds_toml".to_string(),
                    key: "missing toml_path in config_target".to_string(),
                }
            })?;
            crate::config_patch::melonds_toml::read_melonds_toml_value(
                path,
                &toml_path.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            )
            .map(|opt| opt.map(|v| v.to_file_string()))
        }
        _ => {
            // Default: qt_ini
            let section = target.section.as_deref().unwrap_or_default();
            let key = target.key.as_deref().unwrap_or_default();
            crate::config_patch::qt_ini::read_qt_ini_value(path, section, key)
        }
    }
}

/// Apply every option's `config_target` patch to the emulator config file.
///
/// The logical value is translated through the target's `value_map` (identity
/// when absent) and written to the config file. Failures are collected and
/// returned WITHOUT stopping — the caller already persisted the internal
/// selection, and a missing config file (e.g. emulator never launched) must not
/// break the rest of the save flow.
pub fn apply_config_patches(
    options: &[EmulatorOption],
    values: &RunnerOptions,
) -> Vec<ConfigPatchFailure> {
    let mut failures = Vec::new();
    for opt in options {
        let Some(target) = &opt.config_target else {
            continue;
        };
        let value = values
            .get(&opt.key)
            .cloned()
            .unwrap_or_else(|| opt.default.clone());
        let translated = target
            .value_map
            .as_ref()
            .and_then(|map| map.get(&value).cloned())
            .unwrap_or_else(|| value.clone());
        let Some(path) = resolve_config_target_path(target) else {
            // No candidate config file exists (e.g. emulator never launched):
            // skip silently, like a missing `file`.
            continue;
        };
        if let Err(err) = apply_single_patch(&path, target, &translated) {
            failures.push(ConfigPatchFailure {
                option_key: opt.key.clone(),
                message: err.to_string(),
            });
        }
    }
    failures
}

/// Dispatch a single patch to the appropriate format-specific backend.
fn apply_single_patch(
    path: &std::path::Path,
    target: &ConfigTarget,
    new_value: &str,
) -> Result<(), crate::config_patch::qt_ini::PatchError> {
    match target.format.as_str() {
        "cemu_xml" => {
            let xml_path = target.xml_path.as_ref().ok_or_else(|| {
                crate::config_patch::qt_ini::PatchError::KeyNotFound {
                    path: path.to_path_buf(),
                    section: "cemu_xml".to_string(),
                    key: "missing xml_path in config_target".to_string(),
                }
            })?;
            crate::config_patch::cemu_xml::patch_cemu_xml(
                path,
                &xml_path.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                new_value,
            )?;
            Ok(())
        }
        "melonds_toml" => {
            let toml_path = target.toml_path.as_ref().ok_or_else(|| {
                crate::config_patch::qt_ini::PatchError::KeyNotFound {
                    path: path.to_path_buf(),
                    section: "melonds_toml".to_string(),
                    key: "missing toml_path in config_target".to_string(),
                }
            })?;
            crate::config_patch::melonds_toml::patch_melonds_toml(
                path,
                &toml_path.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                new_value,
            )?;
            Ok(())
        }
        _ => {
            // Default: qt_ini
            let section = target.section.as_deref().unwrap_or_default();
            let key = target.key.as_deref().unwrap_or_default();
            crate::config_patch::qt_ini::patch_qt_ini(path, section, key, new_value)?;
            Ok(())
        }
    }
}

/// Expand every non-default option into its CLI flags.
///
/// Toggles with a CLI `value_map` (dual-token, e.g. MAME `-filter`/`-nofilter`)
/// are the exception: they ALWAYS emit a flag for the current state, because the
/// emulator needs an explicit token for both states and a missing flag is not
/// the same as "off". Every other option only emits when it differs from its
/// default, so plain toggles like `-f` keep working exactly as before.
pub fn build_args(options: &[EmulatorOption], map: &RunnerOptions) -> Vec<String> {
    let mut out = Vec::new();
    for opt in options {
        let value = map
            .get(&opt.key)
            .cloned()
            .unwrap_or_else(|| opt.default.clone());
        if opt.value_map.is_none() && value == opt.default {
            continue;
        }
        let token = opt
            .value_map
            .as_ref()
            .and_then(|vm| vm.get(&value).cloned())
            .unwrap_or_else(|| value.clone());
        let expanded = opt.flag_template.replace("{value}", &token);
        out.extend(shlex_split(&expanded));
    }
    out
}

/// Active option flags followed by the tokenized custom arguments.
pub fn resolve_flags(
    options: &[EmulatorOption],
    map: &RunnerOptions,
    custom_args: &str,
) -> Vec<String> {
    let mut flags = build_args(options, map);
    if !custom_args.trim().is_empty() {
        flags.extend(shlex_split(custom_args));
    }
    flags
}

/// Parse the runner's `env_vars` column back into its structured form.
pub fn from_env_json(json: &str) -> RunnerOptionEnv {
    if json.trim().is_empty() {
        return RunnerOptionEnv::default();
    }
    serde_json::from_str(json).unwrap_or_default()
}

/// Serialize the structured env into the JSON stored on the runner row.
pub fn to_env_json(env: &RunnerOptionEnv) -> String {
    serde_json::to_string(env).unwrap_or_default()
}

/// Build the env JSON to persist from live option values + custom args,
/// dropping every default entry and empty custom args.
pub fn build_env_json(
    options: &[EmulatorOption],
    values: &RunnerOptions,
    custom_args: &str,
) -> String {
    let filtered = filter_default_map(options, values);
    to_env_json(&RunnerOptionEnv {
        emulator_options: if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        },
        custom_args: {
            let t = custom_args.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        },
    })
}

/// Split a command line by spaces, keeping quoted segments as a single token.
/// `--renderer "vulkan llvm"` -> ["--renderer", "vulkan llvm"].
pub fn shlex_split(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in cmd.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn canonical_emulator_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// An option with a CLI `value_map` must expose the `{value}` placeholder in
/// its `flag_template`, otherwise the token would never be substituted and the
/// map would be silently inert. Pure data validation, run at load time.
fn validate_cli_value_map(
    emulator: &str,
    opt_key: &str,
    value_map: &Option<BTreeMap<String, String>>,
    flag_template: &str,
) -> anyhow::Result<()> {
    if value_map.is_some() && !flag_template.contains("{value}") {
        anyhow::bail!(
            "value_map option '{opt_key}' in emulator '{emulator}' needs '{{value}}' in flag_template"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_patch::melonds_toml::{read_melonds_toml_value, TomlValue};
    use crate::config_patch::qt_ini::read_qt_ini_value;

    fn azahar() -> Vec<EmulatorOption> {
        load_emulator_options("Azahar").unwrap()
    }

    #[test]
    fn emulator_process_names_resolve_from_toml() {
        assert_eq!(emulator_process_name("Azahar").as_deref(), Some("azahar"));
        assert_eq!(emulator_process_name("Eden").as_deref(), Some("eden"));
        assert_eq!(
            emulator_process_name("Dolphin").as_deref(),
            Some("dolphin-emu")
        );
        assert_eq!(emulator_process_name("melonDS").as_deref(), Some("melonDS"));
        assert_eq!(
            emulator_process_name("DuckStation").as_deref(),
            Some("duckstation")
        );
        assert_eq!(emulator_process_name("MAME").as_deref(), Some("mame"));
        assert_eq!(emulator_process_name("Ryujinx"), None);
        assert_eq!(emulator_process_name(""), None);
    }

    #[test]
    fn loads_toml_definitions_for_known_emulators() {
        let azahar = azahar();
        assert_eq!(azahar.len(), 6);
        assert!(azahar.iter().any(|o| o.key == "fullscreen"));
        let renderer = azahar.iter().find(|o| o.key == "renderer").unwrap();
        assert_eq!(
            renderer.kind,
            EmulatorOptionKind::Choice(vec![
                "software".to_string(),
                "opengl".to_string(),
                "vulkan".to_string()
            ])
        );
        assert_eq!(renderer.default, "opengl");

        let eden = load_emulator_options("Eden").unwrap();
        assert!(eden.iter().any(|o| o.key == "singlecore"));
        assert_eq!(load_emulator_options("Ryujinx").unwrap().len(), 0);

        let cemu = load_emulator_options("Cemu").unwrap();
        assert_eq!(
            cemu.len(),
            3,
            "Cemu should have 3 options (fullscreen, renderer_backend, vsync)"
        );
        assert!(cemu.iter().any(|o| o.key == "fullscreen"));
        assert!(cemu.iter().any(|o| o.key == "renderer_backend"));
        assert!(cemu.iter().any(|o| o.key == "vsync"));
        let renderer = cemu.iter().find(|o| o.key == "renderer_backend").unwrap();
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(target.format, "cemu_xml");
        let xml_path = target.xml_path.as_ref().unwrap();
        assert_eq!(xml_path, &vec!["Graphic".to_string(), "api".to_string()]);
    }

    #[test]
    fn choice_builds_renderer_flag_only_when_non_default() {
        let options = azahar();
        let mut map = default_map(&options);
        let args = build_args(&options, &map);
        assert!(args.is_empty(), "defaults must produce no flags");

        map.insert("renderer".to_string(), "vulkan".to_string());
        let args = build_args(&options, &map);
        assert_eq!(args, vec!["--renderer".to_string(), "vulkan".to_string()]);
    }

    #[test]
    fn toggle_appends_flag_when_enabled_and_absent_when_disabled() {
        let options = azahar();
        let mut map = default_map(&options);
        map.insert("fullscreen".to_string(), "1".to_string());
        let args = build_args(&options, &map);
        assert!(args.contains(&"-f".to_string()));
        assert!(!args.contains(&"--accurate-bus".to_string()));
    }

    #[test]
    fn dolphin_loads_flag_based_definitions() {
        let options = load_emulator_options("Dolphin").unwrap();
        assert_eq!(options.len(), 8);

        // Display options patch Dolphin.ini directly (like editing it by hand)
        // AND emit a per-launch -C override whenever non-default.
        let fullscreen = options.iter().find(|o| o.key == "fullscreen").unwrap();
        assert_eq!(
            fullscreen.flag_template,
            "-C Dolphin.Display.Fullscreen={value}"
        );
        let target = fullscreen.config_target.as_ref().unwrap();
        assert_eq!(
            (target.file.as_str(), target.format.as_str()),
            ("~/.config/dolphin-emu/Dolphin.ini", "qt_ini")
        );
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Display", "Fullscreen")
        );
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("0"), Some(&"False".to_string()));
        assert_eq!(map.get("1"), Some(&"True".to_string()));

        let render_to_main = options.iter().find(|o| o.key == "render_to_main").unwrap();
        assert_eq!(
            render_to_main.default, "1",
            "Render to Main Window is on by default"
        );
        assert_eq!(
            render_to_main.flag_template,
            "-C Dolphin.Display.RenderToMain={value}"
        );
        let target = render_to_main.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Display", "RenderToMain")
        );

        // Only the Display options patch Dolphin.ini; Graphics stay pure flags.
        for opt in &options {
            let has_target = opt.config_target.is_some();
            match opt.key.as_str() {
                "fullscreen" | "render_to_main" => {
                    assert!(has_target, "{} must patch Dolphin.ini", opt.key)
                }
                _ => assert!(!has_target, "{} must stay flag-only", opt.key),
            }
        }

        let vsync = options.iter().find(|o| o.key == "vsync").unwrap();
        assert_eq!(
            vsync.flag_template, "-C Graphics.Hardware.VSync=True",
            "VSync lives in Graphics.Hardware, not Graphics.Settings"
        );

        let res = options
            .iter()
            .find(|o| o.key == "internal_resolution")
            .unwrap();
        assert_eq!(res.default, "1");
        assert_eq!(
            res.kind,
            EmulatorOptionKind::Choice(vec![
                "0".to_string(),
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string(),
                "6".to_string(),
                "8".to_string(),
            ])
        );
        assert_eq!(
            res.choice_labels.get("0").map(String::as_str),
            Some("Auto (Multiple of 640x528)")
        );
        assert_eq!(
            res.choice_labels.get("1").map(String::as_str),
            Some("Native (640x528)")
        );
        assert_eq!(
            res.choice_labels.get("8").map(String::as_str),
            Some("8x Native (5120x4224)")
        );

        let aspect = options.iter().find(|o| o.key == "aspect_ratio").unwrap();
        assert_eq!(
            aspect.kind,
            EmulatorOptionKind::Choice(vec![
                "0".to_string(),
                "1".to_string(),
                "2".to_string(),
                "3".to_string()
            ])
        );
        assert_eq!(
            aspect.choice_labels.get("1").map(String::as_str),
            Some("Force 16:9")
        );

        let hack = options.iter().find(|o| o.key == "widescreen_hack").unwrap();
        assert_eq!(
            hack.flag_template, "-C Graphics.Settings.wideScreenHack=True",
            "Dolphin spells the key with a lowercase 'w'"
        );

        assert!(options.iter().any(|o| o.key == "crop_to_aspect"));
        assert!(options.iter().any(|o| o.key == "show_fps"));
    }

    #[test]
    fn dolphin_emits_config_overrides_only_when_non_default() {
        let options = load_emulator_options("Dolphin").unwrap();
        let mut map = default_map(&options);
        let args = build_args(&options, &map);
        assert!(args.is_empty(), "defaults must produce no flags");

        map.insert("fullscreen".to_string(), "1".to_string());
        map.insert("internal_resolution".to_string(), "3".to_string());
        map.insert("aspect_ratio".to_string(), "1".to_string());
        map.insert("widescreen_hack".to_string(), "1".to_string());
        let args = build_args(&options, &map);
        assert!(args.contains(&"-C".to_string()));
        assert!(args.contains(&"Dolphin.Display.Fullscreen=1".to_string()));
        assert!(args.contains(&"Graphics.Settings.InternalResolution=3".to_string()));
        assert!(args.contains(&"Graphics.Settings.AspectRatio=1".to_string()));
        assert!(args.contains(&"Graphics.Settings.wideScreenHack=True".to_string()));
        assert!(!args.contains(&"Graphics.Hardware.VSync=True".to_string()));
        assert!(!args.contains(&"Graphics.Settings.Crop=True".to_string()));
        // render_to_main is ON by default, so no flag is emitted.
        assert!(!args.contains(&"Dolphin.Display.RenderToMain=1".to_string()));

        // Disabling render_to_main forces it off both via flag and file patch.
        map.insert("render_to_main".to_string(), "0".to_string());
        let args = build_args(&options, &map);
        assert!(args.contains(&"Dolphin.Display.RenderToMain=0".to_string()));
    }

    /// A representative `Dolphin.ini` sample matching Dolphin's real format
    /// (`Key = Value` with spaces, True/False booleans in [Display]).
    const DOLPHIN_INI_SAMPLE: &str = "\
[Analytics]
ID = 491b10048e9cfefbaf70f98a154c6287
[DSP]
DSPThread = True
[Core]
GFXBackend = Vulkan
[Display]
RenderToMain = False
Fullscreen = False
[General]
ISOPath0 = /home/user/ROMs/Gamecube
[Interface]
ThemeName = Clean
";

    /// Load the real Dolphin TOML definitions with every config_target pointing
    /// at a temp file, so tests never touch the user's Dolphin.ini.
    fn dolphin_options_with_temp_target(name: &str) -> Vec<EmulatorOption> {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_options_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, DOLPHIN_INI_SAMPLE).unwrap();
        let mut options = load_emulator_options("Dolphin").unwrap();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = file.to_string_lossy().into_owned();
            }
        }
        options
    }

    fn dolphin_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    #[test]
    fn apply_config_patches_writes_dolphin_display_settings() {
        let options = dolphin_options_with_temp_target("apply_dolphin.ini");
        let file = resolve_config_file(
            &dolphin_option(&options, "fullscreen")
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );

        let mut values = default_map(&options);
        values.insert("fullscreen".to_string(), "1".to_string());
        // render_to_main stays at its default "1" (on): the file must still be
        // patched to True even though no CLI flag is emitted for it.
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        assert_eq!(
            read_qt_ini_value(&file, "Display", "Fullscreen")
                .unwrap()
                .as_deref(),
            Some("True")
        );
        assert_eq!(
            read_qt_ini_value(&file, "Display", "RenderToMain")
                .unwrap()
                .as_deref(),
            Some("True")
        );

        // Unrelated lines stay byte-identical (Dolphin uses `Key = Value`).
        let written = std::fs::read_to_string(&file).unwrap();
        let expected = DOLPHIN_INI_SAMPLE
            .replace("Fullscreen = False", "Fullscreen = True")
            .replace("RenderToMain = False", "RenderToMain = True");
        assert_eq!(written, expected);

        // Turning them off writes False back.
        values.insert("fullscreen".to_string(), "0".to_string());
        values.insert("render_to_main".to_string(), "0".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert_eq!(
            read_qt_ini_value(&file, "Display", "Fullscreen")
                .unwrap()
                .as_deref(),
            Some("False")
        );
        assert_eq!(
            read_qt_ini_value(&file, "Display", "RenderToMain")
                .unwrap()
                .as_deref(),
            Some("False")
        );
    }

    #[test]
    fn read_config_value_preloads_dolphin_display_settings() {
        let options = dolphin_options_with_temp_target("preload_dolphin.ini");

        assert_eq!(
            read_config_value(dolphin_option(&options, "fullscreen")).as_deref(),
            Some("0"),
            "file has Fullscreen = False -> logical off"
        );
        assert_eq!(
            read_config_value(dolphin_option(&options, "render_to_main")).as_deref(),
            Some("0"),
            "file has RenderToMain = False -> logical off"
        );
    }

    #[test]
    fn resolve_flags_appends_custom_args_respecting_quotes() {
        let options = azahar();
        let map = default_map(&options);
        let flags = resolve_flags(&options, &map, "--noconsole \"un solo arg\"");
        assert_eq!(
            flags,
            vec!["--noconsole".to_string(), "un solo arg".to_string()]
        );
    }

    #[test]
    fn filter_and_merge_roundtrip_drop_defaults() {
        let options = azahar();
        let mut map = default_map(&options);
        map.insert("fullscreen".to_string(), "1".to_string());
        map.insert("renderer".to_string(), "vulkan".to_string());

        let filtered = filter_default_map(&options, &map);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.get("fullscreen") == Some(&"1".to_string()));

        let merged = merge_runner_options(&options, &filtered);
        assert_eq!(merged.get("fullscreen"), Some(&"1".to_string()));
        assert_eq!(merged.get("renderer"), Some(&"vulkan".to_string()));
        assert_eq!(merged.get("accurate_bus"), Some(&"0".to_string()));

        let json = build_env_json(&options, &map, "  --noconsole  ");
        let env = from_env_json(&json);
        assert_eq!(
            env.emulator_options.as_ref().unwrap().get("renderer"),
            Some(&"vulkan".to_string())
        );
        assert_eq!(env.custom_args.as_deref(), Some("--noconsole"));

        let json_defaults = build_env_json(&options, &default_map(&options), "");
        assert_eq!(from_env_json(&json_defaults).emulator_options, None);
    }

    #[test]
    fn merge_validates_stored_values() {
        let options = azahar();
        let mut stored = RunnerOptions::new();
        stored.insert("renderer".to_string(), "bogus".to_string());
        stored.insert("fullscreen".to_string(), "1".to_string());
        let merged = merge_runner_options(&options, &stored);
        // Invalid choice falls back to default; valid toggle is kept.
        assert_eq!(merged.get("renderer"), Some(&"opengl".to_string()));
        assert_eq!(merged.get("fullscreen"), Some(&"1".to_string()));
    }

    // --- config_target ---------------------------------------------------

    /// A representative Eden `qt-config.ini` sample for the integration tests.
    const QT_INI_SAMPLE: &str = "\
[Renderer]
backend\\default=true
backend=1
resolution_setup\\default=true
resolution_setup=3

[System]
use_docked_mode\\default=true
use_docked_mode=1

[UI]
Shortcuts\\Main Window\\Fullscreen\\KeySeq=F11
";

    fn temp_qt_ini(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_options_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, QT_INI_SAMPLE).unwrap();
        path
    }

    /// Load the real Eden TOML definitions with every config_target pointing at
    /// a temp file, so tests never touch the user's actual qt-config.ini.
    fn eden_options_with_temp_target(name: &str) -> Vec<EmulatorOption> {
        let file = temp_qt_ini(name);
        let mut options = load_emulator_options("Eden").unwrap();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = file.to_string_lossy().into_owned();
            }
        }
        options
    }

    fn renderer_backend(options: &[EmulatorOption]) -> &EmulatorOption {
        options
            .iter()
            .find(|o| o.key == "renderer_backend")
            .unwrap()
    }

    #[test]
    fn eden_loads_config_target_definitions() {
        let options = load_emulator_options("Eden").unwrap();
        let backend = renderer_backend(&options);
        let target = backend.config_target.as_ref().unwrap();
        assert_eq!(target.section.as_deref().unwrap(), "Renderer");
        assert_eq!(target.key.as_deref().unwrap(), "backend");
        assert_eq!(target.format, "qt_ini");
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("opengl"), Some(&"0".to_string()));
        assert_eq!(map.get("vulkan"), Some(&"1".to_string()));
        assert!(
            backend.flag_template.is_empty(),
            "config-only option must not emit CLI flags"
        );

        let docked = options.iter().find(|o| o.key == "docked_mode").unwrap();
        let target = docked.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("System", "use_docked_mode")
        );

        let res = options
            .iter()
            .find(|o| o.key == "resolution_scale")
            .unwrap();
        let target = res.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Renderer", "resolution_setup")
        );
        assert_eq!(
            res.choice_labels.get("3").map(String::as_str),
            Some("1X (Nativo)")
        );
        assert_eq!(res.choice_labels.get("12").map(String::as_str), Some("8X"));
        assert_eq!(
            res.kind,
            EmulatorOptionKind::Choice((0..=12).map(|n| n.to_string()).collect::<Vec<_>>())
        );
    }

    /// A representative DuckStation `settings.ini` sample (its own INI format:
    /// `Key = Value` with spaces around the `=` and true/false booleans).
    const DUCKSTATION_INI_SAMPLE: &str = "\
[Main]
StartFullscreen = false
RewindEnable = false

[GPU]
Renderer = Vulkan
ResolutionScale = 3
Multisamples = 1

[Display]
AspectRatio = 16:9
VSync = true

[UI]
Theme = Dark
";

    /// Load the real DuckStation TOML definitions with every config_target
    /// pointing at a temp file, so tests never touch the user's settings.ini.
    fn duckstation_options_with_temp_target(name: &str) -> Vec<EmulatorOption> {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_options_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, DUCKSTATION_INI_SAMPLE).unwrap();
        let mut options = load_emulator_options("DuckStation").unwrap();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = file.to_string_lossy().into_owned();
            }
        }
        options
    }

    fn duckstation_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    #[test]
    fn duckstation_loads_config_target_definitions() {
        let options = load_emulator_options("DuckStation").unwrap();
        assert_eq!(options.len(), 4);

        let fullscreen = duckstation_option(&options, "fullscreen");
        let target = fullscreen.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Main", "StartFullscreen")
        );
        assert_eq!(target.format, "qt_ini");
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("0"), Some(&"false".to_string()));
        assert_eq!(map.get("1"), Some(&"true".to_string()));
        assert!(
            fullscreen.flag_template.is_empty(),
            "config-only option must not emit CLI flags"
        );

        let renderer = duckstation_option(&options, "renderer");
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("GPU", "Renderer")
        );
        assert_eq!(
            renderer.kind,
            EmulatorOptionKind::Choice(vec![
                "Vulkan".to_string(),
                "OpenGL".to_string(),
                "Software".to_string()
            ])
        );

        let aspect = duckstation_option(&options, "aspect_ratio");
        let target = aspect.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Display", "AspectRatio")
        );
        assert_eq!(aspect.default, "16:9");

        let res = duckstation_option(&options, "resolution_scale");
        let target = res.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("GPU", "ResolutionScale")
        );
        assert_eq!(
            res.choice_labels.get("1").map(String::as_str),
            Some("1X (Nativo)")
        );
        assert_eq!(res.choice_labels.get("8").map(String::as_str), Some("8X"));
        assert_eq!(
            res.kind,
            EmulatorOptionKind::Choice((1..=8).map(|n| n.to_string()).collect::<Vec<_>>())
        );
    }

    #[test]
    fn apply_config_patches_writes_duckstation_settings() {
        let options = duckstation_options_with_temp_target("apply_duckstation.ini");
        let file = resolve_config_file(
            &duckstation_option(&options, "fullscreen")
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );

        let mut values = default_map(&options);
        values.insert("fullscreen".to_string(), "1".to_string());
        values.insert("renderer".to_string(), "OpenGL".to_string());
        values.insert("aspect_ratio".to_string(), "4:3".to_string());
        values.insert("resolution_scale".to_string(), "5".to_string());

        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        // Booleans must be translated to DuckStation's true/false words.
        assert_eq!(
            read_qt_ini_value(&file, "Main", "StartFullscreen")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        // Identity choices written verbatim, spacing around `=` preserved.
        assert_eq!(
            read_qt_ini_value(&file, "GPU", "Renderer")
                .unwrap()
                .as_deref(),
            Some("OpenGL")
        );
        assert_eq!(
            read_qt_ini_value(&file, "Display", "AspectRatio")
                .unwrap()
                .as_deref(),
            Some("4:3")
        );
        assert_eq!(
            read_qt_ini_value(&file, "GPU", "ResolutionScale")
                .unwrap()
                .as_deref(),
            Some("5")
        );

        // Unrelated lines stay byte-identical (DuckStation uses `Key = Value`).
        let written = std::fs::read_to_string(&file).unwrap();
        let expected = DUCKSTATION_INI_SAMPLE
            .replace("StartFullscreen = false", "StartFullscreen = true")
            .replace("Renderer = Vulkan", "Renderer = OpenGL")
            .replace("AspectRatio = 16:9", "AspectRatio = 4:3")
            .replace("ResolutionScale = 3", "ResolutionScale = 5");
        assert_eq!(written, expected);
        assert!(written.contains("VSync = true"));
    }

    #[test]
    fn read_config_value_preloads_duckstation_settings() {
        let options = duckstation_options_with_temp_target("preload_duckstation.ini");

        assert_eq!(
            read_config_value(duckstation_option(&options, "fullscreen")).as_deref(),
            Some("0"),
            "file has StartFullscreen = false -> logical off"
        );
        assert_eq!(
            read_config_value(duckstation_option(&options, "renderer")).as_deref(),
            Some("Vulkan")
        );
        assert_eq!(
            read_config_value(duckstation_option(&options, "aspect_ratio")).as_deref(),
            Some("16:9")
        );
        assert_eq!(
            read_config_value(duckstation_option(&options, "resolution_scale")).as_deref(),
            Some("3")
        );
    }

    /// A representative PCSX2 `PCSX2.ini` sample (same `Key = Value` format as
    /// DuckStation, with true/false booleans and numeric enum codes).
    const PCSX2_INI_SAMPLE: &str = "\
[UI]
StartFullscreen = false
ConfirmShutdown = true

[EmuCore/GS]
AspectRatio = Auto 4:3/3:2
Renderer = -1
upscale_multiplier = 1
VsyncEnable = false

[EmuCore/Speedhacks]
EECycleRate = 0
";

    /// Load the real PCSX2 TOML definitions with every config_target pointing at
    /// a temp file, so tests never touch the user's PCSX2.ini.
    fn pcsx2_options_with_temp_target(name: &str) -> Vec<EmulatorOption> {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_options_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, PCSX2_INI_SAMPLE).unwrap();
        let mut options = load_emulator_options("PCSX2").unwrap();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = file.to_string_lossy().into_owned();
            }
        }
        options
    }

    fn pcsx2_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    #[test]
    fn pcsx2_loads_config_target_definitions() {
        let options = load_emulator_options("PCSX2").unwrap();
        assert_eq!(options.len(), 4);

        let fullscreen = pcsx2_option(&options, "fullscreen");
        let target = fullscreen.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("UI", "StartFullscreen")
        );
        assert_eq!(target.format, "qt_ini");
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("0"), Some(&"false".to_string()));
        assert_eq!(map.get("1"), Some(&"true".to_string()));
        assert!(
            fullscreen.flag_template.is_empty(),
            "config-only option must not emit CLI flags"
        );

        let renderer = pcsx2_option(&options, "renderer");
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("EmuCore/GS", "Renderer")
        );
        // Logical names map to PCSX2 v2.6.3 numeric enum codes.
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("Auto"), Some(&"-1".to_string()));
        assert_eq!(map.get("Vulkan"), Some(&"14".to_string()));
        assert_eq!(map.get("OpenGL"), Some(&"12".to_string()));
        assert_eq!(map.get("Software"), Some(&"13".to_string()));
        assert_eq!(
            renderer.kind,
            EmulatorOptionKind::Choice(vec![
                "Auto".to_string(),
                "Vulkan".to_string(),
                "OpenGL".to_string(),
                "Software".to_string()
            ])
        );

        let aspect = pcsx2_option(&options, "aspect_ratio");
        let target = aspect.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("EmuCore/GS", "AspectRatio")
        );
        assert_eq!(aspect.default, "Auto 4:3/3:2");
        assert!(
            aspect.config_target.as_ref().unwrap().value_map.is_none(),
            "identity strings"
        );
        assert_eq!(
            aspect.kind,
            EmulatorOptionKind::Choice(vec![
                "Auto 4:3/3:2".to_string(),
                "4:3".to_string(),
                "16:9".to_string(),
                "10:7".to_string(),
                "Stretch".to_string()
            ])
        );

        let res = pcsx2_option(&options, "resolution_scale");
        let target = res.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("EmuCore/GS", "upscale_multiplier")
        );
        assert_eq!(
            res.choice_labels.get("1").map(String::as_str),
            Some("1X (Nativo)")
        );
        assert_eq!(res.choice_labels.get("8").map(String::as_str), Some("8X"));
        assert_eq!(
            res.kind,
            EmulatorOptionKind::Choice((1..=8).map(|n| n.to_string()).collect::<Vec<_>>())
        );
    }

    #[test]
    fn apply_config_patches_writes_pcsx2_settings() {
        let options = pcsx2_options_with_temp_target("apply_pcsx2.ini");
        let file = resolve_config_file(
            &pcsx2_option(&options, "fullscreen")
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );

        let mut values = default_map(&options);
        values.insert("fullscreen".to_string(), "1".to_string());
        values.insert("renderer".to_string(), "Vulkan".to_string());
        values.insert("aspect_ratio".to_string(), "16:9".to_string());
        values.insert("resolution_scale".to_string(), "4".to_string());

        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        assert_eq!(
            read_qt_ini_value(&file, "UI", "StartFullscreen")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        // Logical Vulkan -> numeric code 14, spacing around `=` preserved.
        assert_eq!(
            read_qt_ini_value(&file, "EmuCore/GS", "Renderer")
                .unwrap()
                .as_deref(),
            Some("14")
        );
        assert_eq!(
            read_qt_ini_value(&file, "EmuCore/GS", "AspectRatio")
                .unwrap()
                .as_deref(),
            Some("16:9")
        );
        assert_eq!(
            read_qt_ini_value(&file, "EmuCore/GS", "upscale_multiplier")
                .unwrap()
                .as_deref(),
            Some("4")
        );

        // Unrelated lines stay byte-identical.
        let written = std::fs::read_to_string(&file).unwrap();
        let expected = PCSX2_INI_SAMPLE
            .replace("StartFullscreen = false", "StartFullscreen = true")
            .replace("Renderer = -1", "Renderer = 14")
            .replace("AspectRatio = Auto 4:3/3:2", "AspectRatio = 16:9")
            .replace("upscale_multiplier = 1", "upscale_multiplier = 4");
        assert_eq!(written, expected);
        assert!(written.contains("EECycleRate = 0"));
    }

    #[test]
    fn read_config_value_preloads_pcsx2_settings() {
        let options = pcsx2_options_with_temp_target("preload_pcsx2.ini");

        assert_eq!(
            read_config_value(pcsx2_option(&options, "fullscreen")).as_deref(),
            Some("0"),
            "file has StartFullscreen = false -> logical off"
        );
        assert_eq!(
            read_config_value(pcsx2_option(&options, "renderer")).as_deref(),
            Some("Auto"),
            "file has Renderer = -1 -> logical Auto"
        );
        assert_eq!(
            read_config_value(pcsx2_option(&options, "aspect_ratio")).as_deref(),
            Some("Auto 4:3/3:2")
        );
        assert_eq!(
            read_config_value(pcsx2_option(&options, "resolution_scale")).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn flag_template_options_still_work_unchanged() {
        let options = load_emulator_options("Eden").unwrap();
        let fullscreen = options.iter().find(|o| o.key == "fullscreen").unwrap();
        assert_eq!(fullscreen.flag_template, "-f");
        assert!(fullscreen.config_target.is_none());
    }

    #[test]
    fn apply_config_patches_writes_renderer_backend_to_file() {
        let options = eden_options_with_temp_target("apply_backend.ini");
        let mut values = default_map(&options);

        values.insert("renderer_backend".to_string(), "opengl".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        let file = resolve_config_file(
            &renderer_backend(&options)
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );
        assert_eq!(
            read_qt_ini_value(&file, "Renderer", "backend")
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            read_qt_ini_value(&file, "Renderer", "backend\\default")
                .unwrap()
                .as_deref(),
            Some("false")
        );

        values.insert("renderer_backend".to_string(), "vulkan".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert_eq!(
            read_qt_ini_value(&file, "Renderer", "backend")
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn apply_config_patches_writes_docked_mode_to_file() {
        let options = eden_options_with_temp_target("apply_docked.ini");
        let mut values = default_map(&options);
        let file = resolve_config_file(
            &options
                .iter()
                .find(|o| o.key == "docked_mode")
                .unwrap()
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );

        // Eden inverts use_docked_mode: 0 = docked, 1 = handheld.
        // Logical Docked ON ("1") must write a 0 to the file.
        values.insert("docked_mode".to_string(), "1".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert_eq!(
            read_qt_ini_value(&file, "System", "use_docked_mode")
                .unwrap()
                .as_deref(),
            Some("0")
        );

        // Logical Docked OFF ("0") must write a 1.
        values.insert("docked_mode".to_string(), "0".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert_eq!(
            read_qt_ini_value(&file, "System", "use_docked_mode")
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn apply_config_patches_never_touches_other_lines() {
        let options = eden_options_with_temp_target("apply_surgical.ini");
        let backend_only: Vec<EmulatorOption> = options
            .iter()
            .filter(|o| o.key == "renderer_backend")
            .cloned()
            .collect();
        let file = resolve_config_file(
            &renderer_backend(&options)
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );
        let before = std::fs::read_to_string(&file).unwrap();

        let mut values = default_map(&backend_only);
        values.insert("renderer_backend".to_string(), "opengl".to_string());
        let failures = apply_config_patches(&backend_only, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        let after = std::fs::read_to_string(&file).unwrap();
        let expected = before
            .replace("backend=1", "backend=0")
            .replace("backend\\default=true", "backend\\default=false");
        assert_eq!(after, expected);
        assert!(after.contains(r"Shortcuts\Main Window\Fullscreen\KeySeq=F11"));
    }

    #[test]
    fn apply_config_patches_reports_failures_without_panicking() {
        // Point the target at a file that does not exist.
        let mut options = eden_options_with_temp_target("apply_missing.ini");
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = "/nonexistent/eden/qt-config.ini".to_string();
            }
        }
        let values = default_map(&options);
        let failures = apply_config_patches(&options, &values);
        assert!(!failures.is_empty());
        assert!(
            failures.iter().any(|f| f.option_key == "renderer_backend"),
            "expected renderer_backend among failures: {failures:?}"
        );
    }

    #[test]
    fn read_config_value_preloads_from_real_file() {
        let options = eden_options_with_temp_target("preload.ini");
        // Sample file has backend=1 (Vulkan) -> logical value must be "vulkan",
        // not the TOML default.
        assert_eq!(
            read_config_value(renderer_backend(&options)).as_deref(),
            Some("vulkan")
        );

        // Docked mode sample is use_docked_mode=1, which Eden treats as
        // HANDHELD (0 = docked) -> logical value must be "0".
        let docked = options.iter().find(|o| o.key == "docked_mode").unwrap();
        assert_eq!(read_config_value(docked).as_deref(), Some("0"));

        // Missing key/section -> None, so the caller falls back to the default.
        let mut missing = renderer_backend(&options).clone();
        missing.config_target = Some(ConfigTarget {
            file: missing.config_target.unwrap().file,
            file_candidates: None,
            format: "qt_ini".to_string(),
            section: Some("NoSection".to_string()),
            key: Some("backend".to_string()),
            xml_path: None,
            toml_path: None,
            value_map: None,
        });
        assert_eq!(read_config_value(&missing), None);
    }

    /// A representative melonDS `melonDS.toml` sample (real TOML format with
    /// nested tables, kept close to the real file so path depth matters).
    /// Deliberately lacks `[3D.GL]` and `Screen.VSync` — like a real config
    /// melonDS has only ever run with the software renderer — so the tests also
    /// cover the patcher creating those tables/keys on save.
    const MELONDS_TOML_SAMPLE: &str = "\
TargetFPS = 60.0
RecentROM = [\"/home/x/rom.nds\"]

[3D]
Renderer = 0

[JIT]
Enable = false

[Screen]
UseGL = false

[Instance0.Window0]
ScreenLayout = 0
";

    /// Load the real melonDS TOML definitions with every config_target pointing
    /// at a temp file, so tests never touch the user's melonDS.toml.
    fn melonds_options_with_temp_target(name: &str) -> Vec<EmulatorOption> {
        let dir =
            std::env::temp_dir().join(format!("tui_game_station_options_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, MELONDS_TOML_SAMPLE).unwrap();
        let mut options = load_emulator_options("melonDS").unwrap();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = file.to_string_lossy().into_owned();
            }
        }
        options
    }

    fn melonds_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    #[test]
    fn melonds_loads_config_target_definitions() {
        let options = load_emulator_options("melonDS").unwrap();
        assert_eq!(options.len(), 5);

        // Fullscreen is CLI-only, no config file touch.
        let fullscreen = melonds_option(&options, "fullscreen");
        assert_eq!(fullscreen.flag_template, "-f");
        assert!(fullscreen.config_target.is_none());

        let renderer = melonds_option(&options, "renderer_3d");
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(target.format, "melonds_toml");
        assert_eq!(target.toml_path.as_deref().unwrap(), ["3D", "Renderer"]);

        let layout = melonds_option(&options, "screen_layout");
        let target = layout.config_target.as_ref().unwrap();
        assert_eq!(
            target.toml_path.as_deref().unwrap(),
            ["Instance0", "Window0", "ScreenLayout"]
        );

        let scale = melonds_option(&options, "scale_factor");
        let target = scale.config_target.as_ref().unwrap();
        assert_eq!(
            target.toml_path.as_deref().unwrap(),
            ["3D", "GL", "ScaleFactor"]
        );
        assert!(target.value_map.is_none(), "scale factor maps identically");
        assert_eq!(scale.default, "2");
        assert_eq!(
            scale.kind,
            EmulatorOptionKind::Choice((1..=16).map(|n| n.to_string()).collect::<Vec<_>>())
        );
        assert_eq!(
            scale.choice_labels.get("1").map(String::as_str),
            Some("1X (Native)")
        );

        let vsync = melonds_option(&options, "vsync");
        let target = vsync.config_target.as_ref().unwrap();
        assert_eq!(
            target.toml_path.as_deref().unwrap(),
            ["Screen", "VSync"],
            "VSync lives in [Screen], not [3D.GL]"
        );
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("0"), Some(&"false".to_string()));
        assert_eq!(map.get("1"), Some(&"true".to_string()));
        assert!(
            vsync.flag_template.is_empty(),
            "config-only option must not emit CLI flags"
        );
    }

    #[test]
    fn apply_config_patches_writes_melonds_settings() {
        let options = melonds_options_with_temp_target("apply_melonds.toml");
        let file = resolve_config_file(
            &melonds_option(&options, "renderer_3d")
                .config_target
                .as_ref()
                .unwrap()
                .file,
        );

        let mut values = default_map(&options);
        values.insert("renderer_3d".to_string(), "OpenGL".to_string());
        values.insert("screen_layout".to_string(), "Vertical".to_string());
        values.insert("scale_factor".to_string(), "4".to_string());
        values.insert("vsync".to_string(), "1".to_string());

        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        // Existing nested integer codes keep their numeric type.
        assert_eq!(
            read_melonds_toml_value(&file, &["3D", "Renderer"]).unwrap(),
            Some(TomlValue::Int(1))
        );
        assert_eq!(
            read_melonds_toml_value(&file, &["Instance0", "Window0", "ScreenLayout"]).unwrap(),
            Some(TomlValue::Int(1))
        );

        // [3D.GL] did not exist -> created with the integer scale factor.
        assert_eq!(
            read_melonds_toml_value(&file, &["3D", "GL", "ScaleFactor"]).unwrap(),
            Some(TomlValue::Int(4))
        );

        // Screen.VSync did not exist -> created as a boolean.
        assert_eq!(
            read_melonds_toml_value(&file, &["Screen", "VSync"]).unwrap(),
            Some(TomlValue::Bool(true))
        );

        // Unrelated lines (RecentROM array, JIT block) stay byte-identical.
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("TargetFPS = 60.0"));
        assert!(written.contains("RecentROM = [\"/home/x/rom.nds\"]"));
        assert!(written.contains("[JIT]\nEnable = false"));
        // [3D.GL] is created (with the existing [3D] table updated in place)
        // and Screen.VSync is added to the existing [Screen] table.
        assert!(written.contains("Renderer = 1"));
        assert!(written.contains("[3D.GL]\nScaleFactor = 4"));
        assert!(written.contains("[Screen]\nUseGL = false\nVSync = true"));
        assert!(written.contains("ScreenLayout = 1"));
    }

    #[test]
    fn read_config_value_preloads_melonds_settings() {
        let options = melonds_options_with_temp_target("preload_melonds.toml");

        assert_eq!(
            read_config_value(melonds_option(&options, "renderer_3d")).as_deref(),
            Some("Software"),
            "file has 3D.Renderer = 0 -> logical Software"
        );
        assert_eq!(
            read_config_value(melonds_option(&options, "screen_layout")).as_deref(),
            Some("Natural"),
            "file has ScreenLayout = 0 -> logical Natural"
        );
        // Keys melonDS has not written yet read as None (caller falls back to
        // the TOML default).
        assert_eq!(read_config_value(melonds_option(&options, "vsync")), None);
    }

    #[test]
    fn resolve_config_file_expands_home() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(
            resolve_config_file("~/.config/eden/qt-config.ini"),
            home.join(".config/eden/qt-config.ini")
        );
        assert_eq!(resolve_config_file("~"), home);
        assert_eq!(
            resolve_config_file("/abs/path.ini"),
            PathBuf::from("/abs/path.ini")
        );
    }

    // --- Azahar `file_candidates` multi-path resolution -------------------

    /// Realistic qt-config.ini fragments shared by both Azahar variants. The
    /// differing values let tests tell which file was patched (A standard,
    /// B Plus) and exercise value_map reverse-lookup on preload.
    const AZAHAR_QT_INI_A: &str = r#"[Renderer]
graphics_api=1
graphics_api\default=false
resolution_factor=2
resolution_factor\default=false
use_vsync_new=true
use_vsync_new\default=true

[Layout]
layout_option=0
layout_option\default=true
"#;
    const AZAHAR_QT_INI_B: &str = r#"[Renderer]
graphics_api=2
graphics_api\default=false
resolution_factor=1
resolution_factor\default=false
use_vsync_new=true
use_vsync_new\default=true

[Layout]
layout_option=3
layout_option\default=true
"#;

    fn azahar_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tui_game_station_azahar_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Override the mtime of `path` to `age_secs` in the past, so tests can
    /// control which candidate is "most recently modified".
    fn set_mtime(path: &std::path::Path, age_secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs))
            .unwrap();
    }

    /// Load the real Azahar TOML definitions with every config_target's
    /// `file_candidates` pointing at the two given temp files (standard first,
    /// Plus second — matching the TOML order). Does NOT create the files;
    /// callers decide. Tests never touch the user's real qt-config.ini.
    fn azahar_options_with_candidates(
        standard: &std::path::Path,
        plus: &std::path::Path,
    ) -> Vec<EmulatorOption> {
        let mut options = azahar();
        for opt in &mut options {
            if let Some(target) = &mut opt.config_target {
                target.file = String::new();
                target.file_candidates = Some(vec![
                    standard.to_string_lossy().into_owned(),
                    plus.to_string_lossy().into_owned(),
                ]);
            }
        }
        options
    }

    fn azahar_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    #[test]
    fn azahar_loads_real_config_targets() {
        let options = azahar();

        // Fullscreen and accurate_bus stay pure CLI flags.
        let fullscreen = azahar_option(&options, "fullscreen");
        assert_eq!(fullscreen.flag_template, "-f");
        assert!(fullscreen.config_target.is_none());
        assert!(azahar_option(&options, "accurate_bus")
            .config_target
            .is_none());

        // Renderer patches [Renderer] graphics_api with a value_map.
        let renderer = azahar_option(&options, "renderer");
        assert_eq!(renderer.flag_template, "--renderer {value}");
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(target.format, "qt_ini");
        assert_eq!(target.file, "", "file_candidates replaces file for Azahar");
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Renderer", "graphics_api")
        );
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("software"), Some(&"0".to_string()));
        assert_eq!(map.get("opengl"), Some(&"1".to_string()));
        assert_eq!(map.get("vulkan"), Some(&"2".to_string()));

        // Screen layout is config-only ([Layout] layout_option, no CLI flag).
        let layout = azahar_option(&options, "screen_layout");
        assert!(
            layout.flag_template.is_empty(),
            "layout must be config-only"
        );
        let target = layout.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Layout", "layout_option")
        );
        assert_eq!(layout.default, "Default");
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("Default"), Some(&"0".to_string()));
        assert_eq!(map.get("SideScreen"), Some(&"3".to_string()));
        assert_eq!(map.get("CustomLayout"), Some(&"6".to_string()));
        assert_eq!(
            layout.choice_labels.get("SingleScreen").map(String::as_str),
            Some("Single Screen")
        );

        // Resolution scale maps identically (no value_map), range 1x-10x.
        let scale = azahar_option(&options, "resolution_factor");
        let target = scale.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Renderer", "resolution_factor")
        );
        assert!(target.value_map.is_none(), "scale factor maps identically");
        assert_eq!(scale.default, "1");
        assert_eq!(
            scale.choice_labels.get("1").map(String::as_str),
            Some("1X (Native)")
        );

        // VSync writes the modern Azahar key under [Renderer].
        let vsync = azahar_option(&options, "vsync");
        assert!(vsync.flag_template.is_empty(), "vsync must be config-only");
        let target = vsync.config_target.as_ref().unwrap();
        assert_eq!(
            (
                target.section.as_deref().unwrap(),
                target.key.as_deref().unwrap()
            ),
            ("Renderer", "use_vsync_new")
        );
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("0"), Some(&"false".to_string()));
        assert_eq!(map.get("1"), Some(&"true".to_string()));
    }

    #[test]
    fn azahar_loads_file_candidates_from_toml() {
        let options = azahar();
        let renderer = azahar_option(&options, "renderer");
        let target = renderer.config_target.as_ref().unwrap();
        assert_eq!(
            target.file_candidates.as_deref(),
            Some(
                &[
                    "~/.config/azahar-emu/qt-config.ini".to_string(),
                    "~/.config/azaharplus-emu/qt-config.ini".to_string(),
                ][..]
            )
        );
    }

    #[test]
    fn resolve_config_path_uses_whichever_variant_exists() {
        let dir = azahar_temp_dir("single");
        let standard = dir.join("qt-config.ini");
        let plus = dir.join("qt-config-plus.ini");

        std::fs::write(&standard, AZAHAR_QT_INI_A).unwrap();
        assert_eq!(
            resolve_config_path(&[standard.clone(), plus.clone()]).as_deref(),
            Some(standard.as_path()),
            "only the standard config exists -> standard"
        );

        std::fs::remove_file(&standard).unwrap();
        std::fs::write(&plus, AZAHAR_QT_INI_B).unwrap();
        assert_eq!(
            resolve_config_path(&[standard.clone(), plus.clone()]).as_deref(),
            Some(plus.as_path()),
            "only the Plus config exists -> Plus"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_config_path_prefers_most_recent_without_first_bias() {
        let dir = azahar_temp_dir("mtime");
        let standard = dir.join("qt-config.ini");
        let plus = dir.join("qt-config-plus.ini");
        std::fs::write(&standard, AZAHAR_QT_INI_A).unwrap();
        std::fs::write(&plus, AZAHAR_QT_INI_B).unwrap();

        set_mtime(&standard, 100);
        set_mtime(&plus, 10);
        assert_eq!(
            resolve_config_path(&[standard.clone(), plus.clone()]).as_deref(),
            Some(plus.as_path()),
            "Plus is more recent -> Plus, despite standard being first in the list"
        );

        set_mtime(&standard, 10);
        set_mtime(&plus, 100);
        assert_eq!(
            resolve_config_path(&[standard.clone(), plus.clone()]).as_deref(),
            Some(standard.as_path()),
            "standard is now more recent -> standard"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_config_path_with_no_existing_candidate_is_non_blocking() {
        let dir = azahar_temp_dir("none");
        let standard = dir.join("qt-config.ini");
        let plus = dir.join("qt-config-plus.ini");
        assert_eq!(
            resolve_config_path(&[standard.clone(), plus.clone()]),
            None,
            "no existing candidate -> None"
        );

        // read_config_value falls back (None) and apply_config_patches writes
        // nothing, reporting no failures — like the Cemu settings.xml case.
        let options = azahar_options_with_candidates(&standard, &plus);
        assert_eq!(read_config_value(azahar_option(&options, "renderer")), None);
        let failures = apply_config_patches(&options, &default_map(&options));
        assert!(
            failures.is_empty(),
            "missing candidates must not report failures"
        );
        assert!(
            !standard.exists() && !plus.exists(),
            "nothing may be created when no candidate exists"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_config_patches_writes_only_the_most_recent_candidate() {
        let dir = azahar_temp_dir("e2e");
        let standard = dir.join("qt-config.ini");
        let plus = dir.join("qt-config-plus.ini");
        std::fs::write(&standard, AZAHAR_QT_INI_A).unwrap();
        std::fs::write(&plus, AZAHAR_QT_INI_B).unwrap();
        set_mtime(&standard, 100);
        set_mtime(&plus, 10);

        let options = azahar_options_with_candidates(&standard, &plus);
        let mut values = default_map(&options);
        values.insert("renderer".to_string(), "vulkan".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        let patched = std::fs::read_to_string(&plus).unwrap();
        assert!(
            patched.contains("graphics_api=2"),
            "the most-recent candidate must be patched: {patched:?}"
        );
        assert!(
            patched.contains("graphics_api\\default=false"),
            "patch must clear the \\default sibling: {patched:?}"
        );
        let untouched = std::fs::read_to_string(&standard).unwrap();
        assert_eq!(
            untouched, AZAHAR_QT_INI_A,
            "the older candidate must stay byte-identical"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn azahar_reads_and_patches_real_targets_e2e() {
        let dir = azahar_temp_dir("real");
        let standard = dir.join("qt-config.ini");
        let plus = dir.join("qt-config-plus.ini");
        std::fs::write(&standard, AZAHAR_QT_INI_A).unwrap();
        std::fs::write(&plus, AZAHAR_QT_INI_B).unwrap();
        set_mtime(&standard, 100);
        set_mtime(&plus, 10);

        let options = azahar_options_with_candidates(&standard, &plus);

        // Preload translates raw config values back to logical option values.
        assert_eq!(
            read_config_value(azahar_option(&options, "renderer")).as_deref(),
            Some("vulkan"),
            "graphics_api=2 -> vulkan"
        );
        assert_eq!(
            read_config_value(azahar_option(&options, "screen_layout")).as_deref(),
            Some("SideScreen"),
            "layout_option=3 -> SideScreen"
        );
        assert_eq!(
            read_config_value(azahar_option(&options, "resolution_factor")).as_deref(),
            Some("1")
        );
        assert_eq!(
            read_config_value(azahar_option(&options, "vsync")).as_deref(),
            Some("1"),
            "use_vsync_new=true -> 1"
        );

        // Patching a full save writes every option to the active (Plus) file.
        let mut values = default_map(&options);
        values.insert("renderer".to_string(), "software".to_string());
        values.insert("screen_layout".to_string(), "LargeScreen".to_string());
        values.insert("resolution_factor".to_string(), "4".to_string());
        values.insert("vsync".to_string(), "0".to_string());
        let failures = apply_config_patches(&options, &values);
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");

        let patched = std::fs::read_to_string(&plus).unwrap();
        assert!(patched.contains("graphics_api=0\n"), "{patched:?}");
        assert!(patched.contains("layout_option=2\n"), "{patched:?}");
        assert!(patched.contains("resolution_factor=4\n"), "{patched:?}");
        assert!(patched.contains("use_vsync_new=false\n"), "{patched:?}");
        for sibling in [
            "graphics_api\\default=false",
            "layout_option\\default=false",
            "resolution_factor\\default=false",
            "use_vsync_new\\default=false",
        ] {
            assert!(patched.contains(sibling), "missing {sibling}: {patched:?}");
        }
        assert_eq!(
            std::fs::read_to_string(&standard).unwrap(),
            AZAHAR_QT_INI_A,
            "the older candidate must stay byte-identical"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- MAME CLI dual-token toggles (no config_target at all) -----------

    fn mame() -> Vec<EmulatorOption> {
        load_emulator_options("MAME").unwrap()
    }

    fn mame_option<'a>(options: &'a [EmulatorOption], key: &str) -> &'a EmulatorOption {
        options.iter().find(|o| o.key == key).unwrap()
    }

    /// Hand-built CLI options to unit test `build_args` in isolation.
    fn cli_toggle(
        key: &str,
        default: &str,
        flag: &str,
        value_map: Option<&[(&str, &str)]>,
    ) -> EmulatorOption {
        EmulatorOption {
            key: key.to_string(),
            name: key.to_string(),
            kind: EmulatorOptionKind::Toggle,
            default: default.to_string(),
            flag_template: flag.to_string(),
            value_map: value_map.map(|pairs| {
                pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            }),
            config_target: None,
            choice_labels: BTreeMap::new(),
        }
    }

    fn cli_choice(key: &str, default: &str, flag: &str, choices: &[&str]) -> EmulatorOption {
        EmulatorOption {
            key: key.to_string(),
            name: key.to_string(),
            kind: EmulatorOptionKind::Choice(choices.iter().map(|c| c.to_string()).collect()),
            default: default.to_string(),
            flag_template: flag.to_string(),
            value_map: None,
            config_target: None,
            choice_labels: BTreeMap::new(),
        }
    }

    #[test]
    fn mame_loads_flag_based_definitions() {
        let options = mame();
        assert_eq!(options.len(), 9);
        for opt in &options {
            assert!(
                opt.config_target.is_none(),
                "MAME never touches config files: {}",
                opt.key
            );
            assert!(
                !opt.flag_template.is_empty(),
                "every MAME option maps to a CLI flag: {}",
                opt.key
            );
        }

        let filter = mame_option(&options, "filter");
        assert_eq!(filter.kind, EmulatorOptionKind::Toggle);
        assert_eq!(filter.default, "1");
        assert_eq!(filter.flag_template, "-{value}");
        let map = filter.value_map.as_ref().unwrap();
        assert_eq!(map.get("1"), Some(&"filter".to_string()));
        assert_eq!(map.get("0"), Some(&"nofilter".to_string()));

        let keepaspect = mame_option(&options, "keepaspect");
        assert_eq!(
            keepaspect.value_map.as_ref().unwrap().get("0"),
            Some(&"nokeepaspect".to_string())
        );

        let autoframeskip = mame_option(&options, "autoframeskip");
        assert_eq!(autoframeskip.default, "0");
        assert_eq!(
            autoframeskip.value_map.as_ref().unwrap().get("1"),
            Some(&"autoframeskip".to_string())
        );

        let frameskip = mame_option(&options, "frameskip");
        assert!(matches!(&frameskip.kind, EmulatorOptionKind::Choice(c) if c.len() == 11));
        assert_eq!(frameskip.flag_template, "-frameskip {value}");
        assert!(frameskip.value_map.is_none(), "choices have no value_map");

        let video = mame_option(&options, "video");
        assert!(
            matches!(&video.kind, EmulatorOptionKind::Choice(c) if c.iter().any(|v| v == "bgfx"))
        );
        assert_eq!(video.default, "auto");
        assert_eq!(video.flag_template, "-video {value}");
        assert!(video.value_map.is_none());
    }

    #[test]
    fn dual_token_toggles_always_emit_even_at_default() {
        let options = mame();
        // MAME needs an explicit token for BOTH toggle states, so the default
        // state is still emitted: on-defaults -> -flag, off-defaults -> -noflag.
        let args = build_args(&options, &default_map(&options));
        assert!(
            args.contains(&"-filter".to_string()),
            "on default explicit: {args:?}"
        );
        assert!(args.contains(&"-keepaspect".to_string()));
        assert!(args.contains(&"-throttle".to_string()));
        assert!(args.contains(&"-skip_gameinfo".to_string()));
        assert!(
            args.contains(&"-noautoframeskip".to_string()),
            "off default explicit: {args:?}"
        );
        assert!(args.contains(&"-nocheat".to_string()));
        assert!(args.contains(&"-noconfirm_quit".to_string()));
        // The opposite token of each toggle is absent for the default state.
        assert!(!args.contains(&"-nofilter".to_string()));
        assert!(!args.contains(&"-nokeepaspect".to_string()));
        assert!(!args.contains(&"-autoframeskip".to_string()));
        assert!(!args.contains(&"-cheat".to_string()));
        // Choices at their default are still skipped entirely.
        assert!(
            !args.iter().any(|a| a == "-video" || a == "-frameskip"),
            "choices at default must not emit: {args:?}"
        );
    }

    #[test]
    fn mame_builds_expected_command_for_mixed_states() {
        let options = mame();
        let mut map = default_map(&options);
        map.insert("filter".to_string(), "0".to_string()); // off -> -nofilter
        map.insert("keepaspect".to_string(), "1".to_string()); // on (default) -> -keepaspect
        map.insert("video".to_string(), "opengl".to_string()); // choice -> -video opengl

        let args = build_args(&options, &map);
        assert_eq!(
            args,
            vec![
                "-nofilter",
                "-keepaspect",
                "-noautoframeskip",
                "-throttle",
                "-nocheat",
                "-skip_gameinfo",
                "-noconfirm_quit",
                "-video",
                "opengl",
            ],
            "the command must include exactly -nofilter -keepaspect -video opengl \
             plus the rest of explicit defaults, well formed"
        );
    }

    #[test]
    fn simple_single_flag_toggle_unchanged_by_value_map_support() {
        // Regression: classic toggles (no value_map) keep the old semantics —
        // the whole flag is only added when it differs from the default.
        let options = azahar();
        let map = default_map(&options);
        assert!(
            build_args(&options, &map).is_empty(),
            "defaults must produce no flags"
        );

        let mut map = map;
        map.insert("fullscreen".to_string(), "1".to_string());
        assert_eq!(build_args(&options, &map), vec!["-f".to_string()]);

        map.insert("fullscreen".to_string(), "0".to_string());
        assert!(
            build_args(&options, &map).is_empty(),
            "off (default) must omit -f entirely"
        );
    }

    #[test]
    fn choice_with_parameterized_flag_template() {
        let options = mame();
        let mut map = default_map(&options);
        map.insert("video".to_string(), "bgfx".to_string());
        let args = build_args(&options, &map);
        assert!(
            args.windows(2).any(|w| w == ["-video", "bgfx"]),
            "video choice must expand to '-video bgfx': {args:?}"
        );

        map.insert("frameskip".to_string(), "3".to_string());
        let args = build_args(&options, &map);
        assert!(
            args.windows(2).any(|w| w == ["-frameskip", "3"]),
            "{args:?}"
        );

        // Existing choice substitution keeps working for non-MAME emulators.
        let az = azahar();
        let mut am = default_map(&az);
        am.insert("renderer".to_string(), "vulkan".to_string());
        assert_eq!(
            build_args(&az, &am),
            vec!["--renderer".to_string(), "vulkan".to_string()]
        );
    }

    #[test]
    fn build_args_covers_all_three_flag_kinds_together() {
        let options = vec![
            cli_toggle("fullscreen", "0", "-f", None),
            cli_toggle(
                "filter",
                "1",
                "-{value}",
                Some(&[("1", "filter"), ("0", "nofilter")]),
            ),
            cli_choice("video", "auto", "-video {value}", &["auto", "opengl"]),
        ];

        // Everything at default: the dual-token toggle still emits.
        assert_eq!(
            build_args(&options, &default_map(&options)),
            vec!["-filter".to_string()]
        );

        let mut map = default_map(&options);
        map.insert("fullscreen".to_string(), "1".to_string());
        map.insert("filter".to_string(), "0".to_string());
        map.insert("video".to_string(), "opengl".to_string());
        assert_eq!(
            build_args(&options, &map),
            vec!["-f", "-nofilter", "-video", "opengl"]
        );
    }

    #[test]
    fn value_map_requires_placeholder_in_flag_template() {
        assert!(validate_cli_value_map("mame", "filter", &None, "-{value}").is_ok());
        assert!(validate_cli_value_map(
            "mame",
            "filter",
            &Some(BTreeMap::from([("0".to_string(), "nofilter".to_string())])),
            "-{value}"
        )
        .is_ok());
        let err = validate_cli_value_map(
            "mame",
            "filter",
            &Some(BTreeMap::from([("0".to_string(), "nofilter".to_string())])),
            "-f",
        )
        .unwrap_err();
        assert!(err.to_string().contains("needs '{value}'"));
    }
}
