use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{Context, Result};
use crate::models::Runner;

pub const RETROARCH_7Z_URL: &str =
    "https://buildbot.libretro.com/nightly/linux/x86_64/RetroArch.7z";

/// Managed directory for RetroArch downloaded data + AppImage.
pub fn get_retroarch_managed_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tui_game_station")
        .join("runners")
        .join("emulators")
        .join("retroarch-data")
}

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

/// Resolve the `retroarch.cfg` path for a given RetroArch runner depending on its `source`.
/// - `Downloaded` -> `<managed_dir>/retroarch.cfg`
/// - `Browsed` (or default) -> `~/.config/retroarch/retroarch.cfg`
pub fn resolve_retroarch_config_path(runner: &Runner) -> PathBuf {
    let is_downloaded = runner
        .source
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("Downloaded"))
        .unwrap_or_else(|| {
            runner
                .executable_path
                .as_deref()
                .map(|p| p.contains("retroarch-data"))
                .unwrap_or(false)
        });

    if is_downloaded {
        get_retroarch_managed_dir().join("retroarch.cfg")
    } else {
        get_retroarch_browsed_config_path()
    }
}

/// Resolve the `cores/` directory for a given RetroArch runner depending on its `source`.
/// - `Downloaded` -> `<managed_dir>/cores/`
/// - `Browsed` (or default) -> `~/.config/retroarch/cores/`
pub fn resolve_retroarch_cores_dir(runner: &Runner) -> PathBuf {
    let is_downloaded = runner
        .source
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("Downloaded"))
        .unwrap_or_else(|| {
            runner
                .executable_path
                .as_deref()
                .map(|p| p.contains("retroarch-data"))
                .unwrap_or(false)
        });

    if is_downloaded {
        get_retroarch_managed_dir().join("cores")
    } else {
        get_retroarch_browsed_cores_dir()
    }
}

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

    let status = Command::new(bin)
        .arg("x")
        .arg("-y")
        .arg(archive_path)
        .arg(format!("-o{}", output_dir.display()))
        .status()
        .with_context(|| format!("Failed to run '{}' tool", bin))?;

    if !status.success() {
        anyhow::bail!("Fallo la extracción del archivo 7z con el ejecutable '{}'", bin);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Runner;

    #[test]
    fn test_retroarch_config_path_resolution() {
        let downloaded_runner = Runner {
            id: 1,
            platform_id: Some(1),
            name: "RetroArch".to_string(),
            runner_type: "retroarch".to_string(),
            executable_path: Some("/some/path/retroarch-data/RetroArch.AppImage".to_string()),
            command_template: String::new(),
            default_env: None,
            download_url: None,
            download_filename: None,
            is_default: true,
            is_active: true,
            env_vars: None,
            source: Some("Downloaded".to_string()),
        };

        let browsed_runner = Runner {
            id: 2,
            platform_id: Some(1),
            name: "RetroArch".to_string(),
            runner_type: "retroarch".to_string(),
            executable_path: Some("/usr/bin/retroarch".to_string()),
            command_template: String::new(),
            default_env: None,
            download_url: None,
            download_filename: None,
            is_default: false,
            is_active: false,
            env_vars: None,
            source: Some("Browsed".to_string()),
        };

        let downloaded_cfg = resolve_retroarch_config_path(&downloaded_runner);
        assert!(downloaded_cfg.ends_with("retroarch-data/retroarch.cfg"));

        let browsed_cfg = resolve_retroarch_config_path(&browsed_runner);
        assert!(browsed_cfg.ends_with("retroarch/retroarch.cfg"));
    }

    #[test]
    fn test_retroarch_cores_dir_resolution() {
        let downloaded_runner = Runner {
            id: 1,
            platform_id: Some(1),
            name: "RetroArch".to_string(),
            runner_type: "retroarch".to_string(),
            executable_path: Some("/some/path/retroarch-data/RetroArch.AppImage".to_string()),
            command_template: String::new(),
            default_env: None,
            download_url: None,
            download_filename: None,
            is_default: true,
            is_active: true,
            env_vars: None,
            source: Some("Downloaded".to_string()),
        };

        let downloaded_cores = resolve_retroarch_cores_dir(&downloaded_runner);
        assert!(downloaded_cores.ends_with("retroarch-data/cores"));
    }
}
