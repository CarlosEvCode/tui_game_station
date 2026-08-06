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
        managed.join("RetroArch.AppImage.home/.config/retroarch/retroarch.cfg")
    } else {
        if let Some(ref path) = runner.executable_path {
            let p = path.to_lowercase();
            if p.contains("org.libretro.retroarch") || p.contains("flatpak run") {
                if let Some(home) = dirs::home_dir() {
                    let flatpak_cfg = home.join(".var/app/org.libretro.RetroArch/config/retroarch/retroarch.cfg");
                    if flatpak_cfg.parent().map(|p| p.exists()).unwrap_or(false) || p.contains("org.libretro.retroarch") {
                        return flatpak_cfg;
                    }
                }
            }
        }
        get_retroarch_browsed_config_path()
    }
}

/// Resolve the `cores/` directory for a given RetroArch runner.
///
/// - `Downloaded` → `<appimage>.home/.config/retroarch/cores/`
/// - `Flatpak` → `~/.var/app/org.libretro.RetroArch/config/retroarch/cores/`
/// - `Browsed` (or default) → `~/.config/retroarch/cores/`
pub fn resolve_retroarch_cores_dir(runner: &Runner) -> PathBuf {
    if is_downloaded_runner(runner) {
        let managed = get_retroarch_managed_dir();
        if let Some(appimage) = find_downloaded_appimage(&managed) {
            return appimage_cores_dir(&appimage);
        }
        managed.join("RetroArch.AppImage.home/.config/retroarch/cores")
    } else {
        if let Some(ref path) = runner.executable_path {
            let p = path.to_lowercase();
            if p.contains("org.libretro.retroarch") || p.contains("flatpak run") {
                if let Some(home) = dirs::home_dir() {
                    let flatpak_cores = home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores");
                    if flatpak_cores.parent().map(|p| p.exists()).unwrap_or(false) || p.contains("org.libretro.retroarch") {
                        return flatpak_cores;
                    }
                }
            }
        }
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

// ──────────────────────────────────────────────────────────────────────────────
// Core loadability (ELF GNU_STACK)
// ──────────────────────────────────────────────────────────────────────────────

/// ELF64 `PT_GNU_STACK` program-header type.
const PT_GNU_STACK: u32 = 0x6474e551;
/// ELF program-header flag for an executable segment (`PF_X`).
const PF_X: u32 = 0x1;

fn le_u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}
fn le_u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}
fn le_u64(data: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&data[off..off + 8]);
    u64::from_le_bytes(b)
}

/// Whether a libretro core `.so` requires an executable stack (`GNU_STACK`
/// marked RWE in the ELF header).
///
/// Hardened kernels (CachyOS/PaX-style hardening) refuse to map such segments,
/// so `dlopen()` fails with "cannot enable executable stack as shared object
/// requires: Invalid argument" and RetroArch exits with code 1 before loading
/// any game. On stock kernels the same core loads fine, so this is only used
/// to pick a safe fallback at launch time, never to ban cores in the UI.
pub fn core_requires_execstack(so_path: &Path) -> bool {
    let Ok(data) = std::fs::read(so_path) else {
        return false;
    };
    // ELF64 magic + class. Anything else (32-bit, non-ELF, empty stub used by
    // tests) is treated as loadable.
    if data.len() < 64 || &data[0..4] != b"\x7fELF" || data[4] != 2 {
        return false;
    }
    let phoff = le_u64(&data, 0x20) as usize;
    let phentsize = le_u16(&data, 0x36) as usize;
    let phnum = le_u16(&data, 0x38) as usize;
    if phoff == 0 || phentsize < 8 || phnum == 0 {
        return false;
    }
    for i in 0..phnum {
        let off = phoff.saturating_add(i.saturating_mul(phentsize));
        if off.saturating_add(8) > data.len() {
            break;
        }
        if le_u32(&data, off) == PT_GNU_STACK {
            return le_u32(&data, off + 4) & PF_X != 0;
        }
    }
    false
}

/// A core `.so` is usable on this system when it exists and does not require
/// an executable stack (hardened kernels refuse to grant it).
pub fn core_is_loadable(so_path: &Path) -> bool {
    so_path.is_file() && !core_requires_execstack(so_path)
}

/// First catalog core for `platform_slug`, in catalog order (default first),
/// whose `.so` is present and loadable in any of `dirs`. Used to pick a safe
/// fallback when the user-selected core cannot be loaded on this system.
pub fn first_loadable_core_in(dirs: &[&Path], platform_slug: &str) -> Option<crate::core_catalog::CoreInfo> {
    crate::core_catalog::cores_for_platform(platform_slug)
        .into_iter()
        .find(|core| dirs.iter().any(|dir| core_is_loadable(&dir.join(&core.so_file))))
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
        // Catalog order for nds is [melonds, desmume, noods, melondsds].
        std::fs::write(tmp.join("melondsds_libretro.so"), b"").unwrap();
        std::fs::write(tmp.join("melonds_libretro.so"), b"").unwrap();
        // desmume_libretro.so intentionally NOT written to disk.

        let available = super::available_cores_in(&tmp, "nds");
        assert_eq!(available.len(), 2);
        assert_eq!(available[0].key, "melonds");
        assert_eq!(available[0].so_file, "melonds_libretro.so");
        assert_eq!(available[1].key, "melondsds");

        // Empty / missing cores dir -> no cores at all.
        assert!(super::available_cores_in(&tmp.join("missing"), "nds").is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn fake_elf64_so(execstack: bool) -> Vec<u8> {
        let mut b = vec![0u8; 64 + 56];
        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 2; // ELFCLASS64
        b[5] = 1; // little endian
        b[6] = 1; // version
        b[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        b[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        b[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
        b[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff
        b[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        b[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        b[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
        let ph = 64usize;
        b[ph..ph + 4].copy_from_slice(&PT_GNU_STACK.to_le_bytes());
        let flags: u32 = if execstack { 0x7 } else { 0x6 };
        b[ph + 4..ph + 8].copy_from_slice(&flags.to_le_bytes());
        b
    }

    #[test]
    fn test_core_requires_execstack_parses_gnu_stack() {
        let tmp = std::env::temp_dir().join(format!("ra_execstack_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let rwe = tmp.join("rwe_libretro.so");
        std::fs::write(&rwe, fake_elf64_so(true)).unwrap();
        assert!(core_requires_execstack(&rwe));
        assert!(!core_is_loadable(&rwe));

        let rw = tmp.join("rw_libretro.so");
        std::fs::write(&rw, fake_elf64_so(false)).unwrap();
        assert!(!core_requires_execstack(&rw));
        assert!(core_is_loadable(&rw));

        // Empty / non-ELF stubs (as used by other tests) are treated as loadable.
        let stub = tmp.join("stub_libretro.so");
        std::fs::write(&stub, b"").unwrap();
        assert!(!core_requires_execstack(&stub));
        assert!(core_is_loadable(&stub));

        // Missing file is not loadable.
        assert!(!core_is_loadable(&tmp.join("missing_libretro.so")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_first_loadable_core_skips_execstack_and_missing() {
        let tmp = std::env::temp_dir().join(format!("ra_firstload_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // melondsds = RW (loadable); melonds = RWE (skipped); desmume missing.
        std::fs::write(tmp.join("melondsds_libretro.so"), fake_elf64_so(false)).unwrap();
        std::fs::write(tmp.join("melonds_libretro.so"), fake_elf64_so(true)).unwrap();

        let dirs = [tmp.as_path()];
        let core = first_loadable_core_in(&dirs, "nds").expect("fallback core");
        assert_eq!(core.key, "melondsds");

        // Missing dirs -> no fallback available.
        assert!(first_loadable_core_in(&[tmp.join("nope").as_path()], "nds").is_none());

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
