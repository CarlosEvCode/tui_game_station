use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use crate::models::Runner;

pub const RETROARCH_7Z_URL: &str =
    "https://buildbot.libretro.com/nightly/linux/x86_64/RetroArch.7z";

/// Managed directory for RetroArch downloaded data + AppImage.
/// After extraction this will contain a single sub-directory like
/// `RetroArch-Linux-x86_64/` that holds the AppImage and its home.
pub fn get_retroarch_managed_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tui_game_station")
        .join("runners")
        .join("emulators")
        .join("retroarch-data")
}

/// Locate the first `*.AppImage` found recursively inside `managed_dir`,
/// searching at most 2 levels deep (managed_dir/<subdir>/<name>.AppImage).
///
/// The `.7z` archive from buildbot extracts into a sub-folder named after
/// the build (e.g. `RetroArch-Linux-x86_64/`), so the AppImage is at:
///   `<managed_dir>/RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage`
///
/// Returns `None` if no AppImage has been extracted yet.
pub fn find_downloaded_appimage(managed_dir: &Path) -> Option<PathBuf> {
    // First check direct children of managed_dir (unlikely but safe)
    if let Ok(entries) = std::fs::read_dir(managed_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("AppImage") && p.is_file() {
                return Some(p);
            }
        }
    }
    // Then check one level deeper (the extracted sub-folder)
    if let Ok(entries) = std::fs::read_dir(managed_dir) {
        for entry in entries.flatten() {
            let sub = entry.path();
            if sub.is_dir() {
                if let Ok(children) = std::fs::read_dir(&sub) {
                    for child in children.flatten() {
                        let p = child.path();
                        if p.extension().and_then(|e| e.to_str()) == Some("AppImage")
                            && p.is_file()
                        {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Given the path to a downloaded RetroArch AppImage, return the path to the
/// `retroarch.cfg` that the AppImage creates on first run inside its home dir.
///
/// The AppImage sets `$HOME` to `<appimage_path>.home`, so config ends up at:
///   `<appimage_path>.home/.config/retroarch/retroarch.cfg`
pub fn appimage_config_path(appimage_path: &Path) -> PathBuf {
    appimage_home_dir(appimage_path)
        .join(".config")
        .join("retroarch")
        .join("retroarch.cfg")
}

/// Given the path to a downloaded RetroArch AppImage, return the `cores/`
/// directory inside its AppImage home dir.
///
///   `<appimage_path>.home/.config/retroarch/cores/`
pub fn appimage_cores_dir(appimage_path: &Path) -> PathBuf {
    appimage_home_dir(appimage_path)
        .join(".config")
        .join("retroarch")
        .join("cores")
}

/// Returns `<appimage_path>.home` — the directory the AppImage runtime
/// uses as `$HOME` when launched.
fn appimage_home_dir(appimage_path: &Path) -> PathBuf {
    let mut s = appimage_path.as_os_str().to_owned();
    s.push(".home");
    PathBuf::from(s)
}

// ──────────────────────────────────────────────────────────────────────────────
// Browsed (system-wide / user-installed) paths
// ──────────────────────────────────────────────────────────────────────────────

/// Standard user config path for browsed / system RetroArch.
pub fn get_retroarch_browsed_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("retroarch")
        .join("retroarch.cfg")
}

/// Standard user cores dir for browsed / system RetroArch.
pub fn get_retroarch_browsed_cores_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("retroarch")
        .join("cores")
}

// ──────────────────────────────────────────────────────────────────────────────
// Runner-aware resolvers (used by runner/src/lib.rs and core_catalog)
// ──────────────────────────────────────────────────────────────────────────────

fn is_downloaded_runner(runner: &Runner) -> bool {
    runner
        .source
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("Downloaded"))
        .unwrap_or_else(|| {
            runner
                .executable_path
                .as_deref()
                .map(|p| p.contains("retroarch-data"))
                .unwrap_or(false)
        })
}

/// Resolve the `retroarch.cfg` path for a given RetroArch runner.
///
/// - `Downloaded` → looks up the AppImage dynamically and returns
///   `<appimage>.home/.config/retroarch/retroarch.cfg`
/// - `Browsed` (or default) → `~/.config/retroarch/retroarch.cfg`
pub fn resolve_retroarch_config_path(runner: &Runner) -> PathBuf {
    if is_downloaded_runner(runner) {
        let managed = get_retroarch_managed_dir();
        if let Some(appimage) = find_downloaded_appimage(&managed) {
            return appimage_config_path(&appimage);
        }
        // AppImage not yet extracted — return a deterministic placeholder that
        // will produce a clear "file not found" error rather than silently
        // using a wrong path.
        managed.join("RetroArch.AppImage.home/.config/retroarch/retroarch.cfg")
    } else {
        get_retroarch_browsed_config_path()
    }
}

/// Resolve the `cores/` directory for a given RetroArch runner.
///
/// - `Downloaded` → `<appimage>.home/.config/retroarch/cores/`
/// - `Browsed` (or default) → `~/.config/retroarch/cores/`
pub fn resolve_retroarch_cores_dir(runner: &Runner) -> PathBuf {
    if is_downloaded_runner(runner) {
        let managed = get_retroarch_managed_dir();
        if let Some(appimage) = find_downloaded_appimage(&managed) {
            return appimage_cores_dir(&appimage);
        }
        managed.join("RetroArch.AppImage.home/.config/retroarch/cores")
    } else {
        get_retroarch_browsed_cores_dir()
    }
}

/// Catalog cores whose libretro `.so` actually exists inside `cores_dir`.
///
/// The filesystem is the source of truth: the catalog may list cores that
/// have not been downloaded yet, and launching with those would fail with a
/// "core not found" error. Preserves catalog order (first = default).
pub fn available_cores_in(cores_dir: &Path, platform_slug: &str) -> Vec<crate::core_catalog::CoreInfo> {
    crate::core_catalog::cores_for_platform(platform_slug)
        .into_iter()
        .filter(|core| cores_dir.join(&core.so_file).is_file())
        .collect()
}

/// Cores from the catalog for `platform_slug` whose libretro `.so` actually
/// exists inside the cores dir resolved for `runner`.
pub fn available_retroarch_cores_for_platform(
    runner: &Runner,
    platform_slug: &str,
) -> Vec<crate::core_catalog::CoreInfo> {
    available_cores_in(&resolve_retroarch_cores_dir(runner), platform_slug)
}

/// Resolve the actual AppImage executable path for a downloaded runner.
///
/// Falls back to the stored `executable_path` on the runner row if the
/// dynamic search fails (e.g. browsed runner or not yet downloaded).
pub fn resolve_retroarch_executable(runner: &Runner) -> Option<PathBuf> {
    if is_downloaded_runner(runner) {
        let managed = get_retroarch_managed_dir();
        if let Some(appimage) = find_downloaded_appimage(&managed) {
            return Some(appimage);
        }
    }
    runner.executable_path.as_deref().map(PathBuf::from)
}

// ──────────────────────────────────────────────────────────────────────────────
// 7z extraction helper
// ──────────────────────────────────────────────────────────────────────────────

/// Extract a `.7z` archive into `output_dir` using system `7z`, `7zr`, or `7za`.
/// If no 7z binary is found on PATH, returns a clear user-facing error message.
pub fn extract_7z<P: AsRef<Path>, Q: AsRef<Path>>(archive_path: P, output_dir: Q) -> Result<()> {
    let archive_path = archive_path.as_ref();
    let output_dir = output_dir.as_ref();

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

    let binaries = ["7z", "7zr", "7za", "7z-full"];
    let mut found_bin = None;

    for bin in binaries {
        if let Ok(status) = Command::new(bin).arg("--help").output() {
            if status.status.success() || !status.stdout.is_empty() || !status.stderr.is_empty() {
                found_bin = Some(bin);
                break;
            }
        }
    }

    let bin = found_bin.ok_or_else(|| {
        anyhow::anyhow!(
            "No se encontró el ejecutable '7z' o '7zr' en el sistema. \
            Por favor instala el paquete 'p7zip' / '7-zip' en tu distribución \
            (ej. sudo apt install p7zip-full o sudo pacman -S p7zip)."
        )
    })?;

    let output = Command::new(bin)
        .arg("x")
        .arg("-y")
        .arg(archive_path)
        .arg(format!("-o{}", output_dir.display()))
        .output()
        .with_context(|| format!("Failed to run '{}' tool", bin))?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Fallo la extracción del archivo 7z con el ejecutable '{}': {}", bin, err_msg);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Runner;

    fn make_runner(source: &str, exe: &str) -> Runner {
        Runner {
            id: 1,
            platform_id: Some(1),
            name: "RetroArch".to_string(),
            runner_type: "retroarch".to_string(),
            executable_path: Some(exe.to_string()),
            command_template: String::new(),
            default_env: None,
            download_url: None,
            download_filename: None,
            is_default: true,
            is_active: true,
            env_vars: None,
            source: Some(source.to_string()),
        }
    }

    #[test]
    fn test_appimage_home_dir() {
        let appimage = PathBuf::from("/data/retroarch-data/RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage");
        let home = appimage_home_dir(&appimage);
        assert!(home.to_string_lossy().ends_with(".AppImage.home"));
    }

    #[test]
    fn test_appimage_config_path() {
        let appimage = PathBuf::from("/data/retroarch-data/RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage");
        let cfg = appimage_config_path(&appimage);
        assert!(cfg.to_string_lossy().ends_with("/.config/retroarch/retroarch.cfg"));
        assert!(cfg.to_string_lossy().contains(".AppImage.home"));
    }

    #[test]
    fn test_appimage_cores_dir() {
        let appimage = PathBuf::from("/data/retroarch-data/RetroArch-Linux-x86_64/RetroArch-Linux-x86_64.AppImage");
        let cores = appimage_cores_dir(&appimage);
        assert!(cores.to_string_lossy().ends_with("/.config/retroarch/cores"));
        assert!(cores.to_string_lossy().contains(".AppImage.home"));
    }

    #[test]
    fn test_browsed_runner_config_path() {
        let runner = make_runner("Browsed", "/usr/bin/retroarch");
        let cfg = resolve_retroarch_config_path(&runner);
        // Browsed always points to system retroarch config
        assert!(cfg.to_string_lossy().ends_with("retroarch/retroarch.cfg"));
        assert!(!cfg.to_string_lossy().contains("retroarch-data"));
    }

    #[test]
    fn test_browsed_runner_cores_dir() {
        let runner = make_runner("Browsed", "/usr/bin/retroarch");
        let cores = resolve_retroarch_cores_dir(&runner);
        assert!(cores.to_string_lossy().ends_with("retroarch/cores"));
        assert!(!cores.to_string_lossy().contains("retroarch-data"));
    }

    #[test]
    fn test_available_cores_filters_by_disk() {
        let tmp = std::env::temp_dir().join(format!("ra_cores_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("melonds_libretro.so"), b"").unwrap();
        // desmume_libretro.so intentionally NOT written to disk.

        let available = super::available_cores_in(&tmp, "nds");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].key, "melonds");
        assert_eq!(available[0].so_file, "melonds_libretro.so");

        // Empty / missing cores dir -> no cores at all.
        assert!(super::available_cores_in(&tmp.join("missing"), "nds").is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_downloaded_appimage_with_fake_tree() {
        let tmp = std::env::temp_dir().join(format!("ra_mgr_test_{}", std::process::id()));
        let sub = tmp.join("RetroArch-Linux-x86_64");
        std::fs::create_dir_all(&sub).unwrap();
        let appimage = sub.join("RetroArch-Linux-x86_64.AppImage");
        std::fs::write(&appimage, "fake").unwrap();

        let found = find_downloaded_appimage(&tmp);
        assert_eq!(found.as_deref(), Some(appimage.as_path()));

        let cfg = appimage_config_path(&appimage);
        assert!(cfg.to_string_lossy().contains(".AppImage.home/.config/retroarch/retroarch.cfg"));

        let cores = appimage_cores_dir(&appimage);
        assert!(cores.to_string_lossy().contains(".AppImage.home/.config/retroarch/cores"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
