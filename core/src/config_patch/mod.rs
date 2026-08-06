//! Safe, surgical patching of emulator configuration files.
//!
//! This module exists so launcher-level emulator options that have no CLI flag
//! (renderer backend, docked mode, resolution, vsync, ...) can be controlled by
//! editing the emulator's own config file without risking the rest of it.
//!
//! Three formats are supported, each with its own submodule and format
//! contract:
//! - `qt_ini`: Qt's INI variant, used by Eden and most yuzu/forks
//!   (`qt-config.ini`) and by DuckStation/PCSX2/PPSSPP (`Key = Value` INI).
//! - `cemu_xml`: Cemu's `settings.xml`.
//! - `melonds_toml`: melonDS's real TOML document (`~/.config/melonDS/melonDS.toml`).
//!
//! Nothing here is wired to the Emulator Options popup, the `eden.toml` assets,
//! or any launch flow yet — it is intentionally isolated so it can be proven
//! reliable first.

pub mod cemu_xml;
pub mod melonds_toml;
pub mod qt_ini;
pub mod retroarch_cfg;

pub use cemu_xml::{patch_cemu_xml, read_cemu_xml_value, CemuPatchResult};
pub use melonds_toml::{patch_melonds_toml, read_melonds_toml_value, TomlPatchResult, TomlValue};
pub use qt_ini::{patch_qt_ini, read_qt_ini_value, PatchError, PatchResult};
pub use retroarch_cfg::{patch_retroarch_cfg, read_retroarch_cfg_value};
