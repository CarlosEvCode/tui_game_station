//! Emulator launch options defined as DATA (TOML), never as per-emulator code.
//!
//! Each emulator ships a `{name}.toml` under `assets/emulators/`. Every option
//! has a stable `key`, a human-readable `name`, a `kind` (`toggle`/`choice`,
//! extensible), a `default`, and a `flag_template` that is expanded into CLI
//! flags whenever the chosen value differs from the default. The selection is
//! persisted as JSON inside the runner's `env_vars` column, alongside optional
//! custom launcher arguments.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// A single emulator option loaded from its TOML definition.
#[derive(Debug, Clone, PartialEq)]
pub struct EmulatorOption {
    pub key: String,
    pub name: String,
    pub kind: EmulatorOptionKind,
    pub default: String,
    pub flag_template: String,
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
    choices: Vec<String>,
    #[serde(default)]
    default: Option<String>,
    flag_template: String,
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
        let kind = match opt.kind.as_str() {
            "choice" => {
                if opt.choices.is_empty() {
                    anyhow::bail!(
                        "choice option '{}' in emulator '{}' has no choices",
                        opt.key,
                        name
                    );
                }
                EmulatorOptionKind::Choice(opt.choices.clone())
            }
            _ => EmulatorOptionKind::Toggle,
        };
        let default = opt.default.unwrap_or_else(|| match kind {
            EmulatorOptionKind::Toggle => "0".to_string(),
            EmulatorOptionKind::Choice(_) => opt
                .choices
                .first()
                .cloned()
                .unwrap_or_default(),
        });
        out.push(EmulatorOption {
            key: opt.key,
            name: opt.name,
            kind,
            default,
            flag_template: opt.flag_template,
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

fn value_is_valid(opt: &EmulatorOption, value: &str) -> bool {
    match &opt.kind {
        EmulatorOptionKind::Toggle => value == "0" || value == "1",
        EmulatorOptionKind::Choice(choices) => choices.iter().any(|c| c == value),
    }
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
}
