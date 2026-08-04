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
    /// Config format; `"qt_ini"` today, extensible to other formats later.
    pub format: String,
    /// Section in the config file (e.g. "Renderer").
    pub section: String,
    /// Key in the section (e.g. "backend").
    pub key: String,
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
    config_target: Option<RawConfigTarget>,
}

/// A `choices` entry can be a plain string (label == value) or a table with a
/// distinct friendly label.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Simple(String),
    Labeled { value: String, label: Option<String> },
}

#[derive(Debug, Deserialize)]
struct RawConfigTarget {
    file: String,
    format: String,
    section: String,
    key: String,
    #[serde(default)]
    value_map: BTreeMap<String, String>,
}

/// Load the option definitions embedded for an emulator by its display name.
/// Unknown emulators return an empty list (no options popup section).
pub fn load_emulator_options(name: &str) -> anyhow::Result<Vec<EmulatorOption>> {
    let key = canonical_emulator_key(name);
    let source = match key.as_str() {
        "azahar" => include_str!("../../assets/emulators/azahar.toml"),
        "eden" => include_str!("../../assets/emulators/eden.toml"),
        _ => return Ok(Vec::new()),
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
        let config_target = opt.config_target.map(|ct| ConfigTarget {
            file: ct.file,
            format: ct.format,
            section: ct.section,
            key: ct.key,
            value_map: if ct.value_map.is_empty() {
                None
            } else {
                Some(ct.value_map)
            },
        });
        let choice_labels = match &kind {
            EmulatorOptionKind::Choice(_) => {
                choice_pairs.into_iter().collect::<BTreeMap<String, String>>()
            }
            EmulatorOptionKind::Toggle => BTreeMap::new(),
        };
        out.push(EmulatorOption {
            key: opt.key,
            name: opt.name,
            kind,
            default,
            flag_template: opt.flag_template,
            config_target,
            choice_labels,
        });
    }
    Ok(out)
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
pub fn merge_runner_options(
    options: &[EmulatorOption],
    stored: &RunnerOptions,
) -> RunnerOptions {
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

/// Read the REAL current value of a config-target option straight from the
/// emulator's config file, translated back to the logical option value.
///
/// Returns `None` when the option has no config target, the file is missing, or
/// the key/section is not present (callers fall back to the TOML `default`).
pub fn read_config_value(opt: &EmulatorOption) -> Option<String> {
    let target = opt.config_target.as_ref()?;
    let path = resolve_config_file(&target.file);
    let raw = crate::config_patch::qt_ini::read_qt_ini_value(&path, &target.section, &target.key)
        .ok()
        .flatten()?;
    match &target.value_map {
        Some(map) => {
            map.iter()
                .find(|(_, file_value)| *file_value == &raw)
                .map(|(logical, _)| logical.clone())
        }
        None => Some(raw),
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
        let path = resolve_config_file(&target.file);
        if let Err(err) = crate::config_patch::qt_ini::patch_qt_ini(
            &path,
            &target.section,
            &target.key,
            &translated,
        ) {
            failures.push(ConfigPatchFailure {
                option_key: opt.key.clone(),
                message: err.to_string(),
            });
        }
    }
    failures
}

/// Expand every non-default option into its CLI flags.
pub fn build_args(options: &[EmulatorOption], map: &RunnerOptions) -> Vec<String> {
    let mut out = Vec::new();
    for opt in options {
        let value = map
            .get(&opt.key)
            .cloned()
            .unwrap_or_else(|| opt.default.clone());
        if value == opt.default {
            continue;
        }
        let expanded = opt.flag_template.replace("{value}", &value);
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
        emulator_options: if filtered.is_empty() { None } else { Some(filtered) },
        custom_args: {
            let t = custom_args.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_patch::qt_ini::read_qt_ini_value;

    fn azahar() -> Vec<EmulatorOption> {
        load_emulator_options("Azahar").unwrap()
    }

    #[test]
    fn loads_toml_definitions_for_known_emulators() {
        let azahar = azahar();
        assert_eq!(azahar.len(), 3);
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
    fn resolve_flags_appends_custom_args_respecting_quotes() {
        let options = azahar();
        let map = default_map(&options);
        let flags = resolve_flags(&options, &map, "--noconsole \"un solo arg\"");
        assert_eq!(flags, vec!["--noconsole".to_string(), "un solo arg".to_string()]);
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
        let dir = std::env::temp_dir().join(format!(
            "tui_game_station_options_{}",
            std::process::id()
        ));
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
        options.iter().find(|o| o.key == "renderer_backend").unwrap()
    }

    #[test]
    fn eden_loads_config_target_definitions() {
        let options = load_emulator_options("Eden").unwrap();
        let backend = renderer_backend(&options);
        let target = backend.config_target.as_ref().unwrap();
        assert_eq!(target.section, "Renderer");
        assert_eq!(target.key, "backend");
        assert_eq!(target.format, "qt_ini");
        let map = target.value_map.as_ref().unwrap();
        assert_eq!(map.get("opengl"), Some(&"0".to_string()));
        assert_eq!(map.get("vulkan"), Some(&"1".to_string()));
        assert!(backend.flag_template.is_empty(), "config-only option must not emit CLI flags");

        let docked = options.iter().find(|o| o.key == "docked_mode").unwrap();
        let target = docked.config_target.as_ref().unwrap();
        assert_eq!((target.section.as_str(), target.key.as_str()), ("System", "use_docked_mode"));

        let res = options.iter().find(|o| o.key == "resolution_scale").unwrap();
        let target = res.config_target.as_ref().unwrap();
        assert_eq!((target.section.as_str(), target.key.as_str()), ("Renderer", "resolution_setup"));
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
            &renderer_backend(&options).config_target.as_ref().unwrap().file,
        );
        assert_eq!(
            read_qt_ini_value(&file, "Renderer", "backend").unwrap().as_deref(),
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
            read_qt_ini_value(&file, "Renderer", "backend").unwrap().as_deref(),
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
            &renderer_backend(&options).config_target.as_ref().unwrap().file,
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
            format: "qt_ini".to_string(),
            section: "NoSection".to_string(),
            key: "backend".to_string(),
            value_map: None,
        });
        assert_eq!(read_config_value(&missing), None);
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
}
