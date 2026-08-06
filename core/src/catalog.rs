//! Explicit platform ↔ emulator catalog, defined as DATA in
//! `assets/emulators/platform_emulators.toml`. This is the single source of
//! truth for which emulators are compatible with which platform; `Database::seed_defaults`
//! reads it to create/refresh the `runners` rows, and new emulators (e.g.
//! Citron for Switch) ship as a TOML entry without touching Rust code.

use serde::Deserialize;

/// One compatible emulator entry for a platform.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogEmulator {
    pub name: String,
    /// `"appimage"` (or future variants); kept for backwards compatibility.
    #[serde(default)]
    pub runner_type: String,
    /// Direct download URL (GitHub release asset). `None` means the emulator
    /// has no built-in download (user browses for the executable manually).
    #[serde(default)]
    pub download_url: Option<String>,
    /// File name used for the download + for launching the AppImage.
    #[serde(default)]
    pub download_filename: Option<String>,
    /// Command template; defaults to `"{executable_path}" "{rom}"` when absent.
    #[serde(default)]
    pub command_template: Option<String>,
}

/// A platform and its compatible emulators, in catalog (preference) order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogPlatform {
    pub slug: String,
    #[serde(default)]
    pub emulators: Vec<CatalogEmulator>,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    #[serde(default)]
    platform: Vec<CatalogPlatform>,
}

/// Embedded TOML source of the catalog.
pub const CATALOG_SOURCE: &str =
    include_str!("../../assets/emulators/platform_emulators.toml");

/// Load every platform entry from the catalog.
pub fn load_catalog() -> Vec<CatalogPlatform> {
    toml::from_str::<CatalogFile>(CATALOG_SOURCE)
        .map(|c| c.platform)
        .unwrap_or_default()
}

/// The compatible emulators for a platform slug, in catalog order.
pub fn compatible_emulators(slug: &str) -> Vec<CatalogEmulator> {
    load_catalog()
        .into_iter()
        .find(|p| p.slug == slug)
        .map(|p| p.emulators)
        .unwrap_or_default()
}

/// Default command template used when a catalog entry has no override.
pub fn default_command_template() -> &'static str {
    "\"{executable_path}\" \"{rom}\""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_catalog_lists_ryujinx_eden_and_minimal_citron() {
        let switch = compatible_emulators("switch");
        let names: Vec<&str> = switch.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Ryujinx", "Eden", "Citron"],
            "Switch supports multiple emulators in preference order"
        );

        let citron = switch.iter().find(|e| e.name == "Citron").unwrap();
        assert_eq!(citron.runner_type, "appimage");
        assert!(
            citron.download_url.is_none() && citron.download_filename.is_none(),
            "Citron is a minimal entry: executable browsed manually, no download"
        );
        assert_eq!(
            citron.command_template, None,
            "Citron uses the default command template"
        );

        let eden = switch.iter().find(|e| e.name == "Eden").unwrap();
        assert!(eden.download_url.is_some(), "Eden keeps its download link");
    }

    #[test]
    fn catalog_covers_every_emulator_platform() {
        let platforms = load_catalog();
        let slugs: Vec<&str> = platforms.iter().map(|p| p.slug.as_str()).collect();
        for expected in [
            "3ds", "snes", "megadrive", "gba", "nes", "ps1", "ps2", "gamecube", "wii", "wii_u", "mame", "psp",
            "dreamcast", "switch", "nds", "vita",
        ] {
            assert!(
                slugs.contains(&expected),
                "catalog is missing platform {expected}"
            );
        }
        // Every emulator entry has a non-empty name and a valid runner_type.
        for p in &platforms {
            for e in &p.emulators {
                assert!(!e.name.is_empty());
                assert!(e.runner_type == "appimage" || e.runner_type == "retroarch");
            }
        }
    }

    #[test]
    fn known_emulators_keep_their_download_links() {
        let mame = compatible_emulators("mame");
        let mame = mame.iter().find(|e| e.name == "MAME").unwrap();
        assert_eq!(
            mame.download_url.as_deref(),
            Some("https://api.github.com/repos/pkgforge-dev/MAME-AppImage/releases/latest")
        );

        let cemu = compatible_emulators("wii_u");
        let cemu = cemu.iter().find(|e| e.name == "Cemu").unwrap();
        assert!(cemu.download_url.is_some());
    }
}
