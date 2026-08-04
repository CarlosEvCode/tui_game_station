//! Safe, surgical patching of emulator configuration files.
//!
//! This module exists so launcher-level emulator options that have no CLI flag
//! (renderer backend, docked mode, resolution, vsync, ...) can be controlled by
//! editing the emulator's own config file without risking the rest of it.
//!
//! The only supported format today is Qt's INI variant (`qt_ini`), which is what
//! Eden (and most yuzu/forks) persist in `qt-config.ini`. See the `qt_ini`
//! submodule for the format contract.
//!
//! Nothing here is wired to the Emulator Options popup, the `eden.toml` assets,
//! or any launch flow yet — it is intentionally isolated so it can be proven
//! reliable first.

pub mod cemu_xml;
pub mod qt_ini;

pub use cemu_xml::{CemuPatchResult, patch_cemu_xml, read_cemu_xml_value};
pub use qt_ini::{PatchError, PatchResult, patch_qt_ini, read_qt_ini_value};
