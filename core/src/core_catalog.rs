use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CoreInfo {
    pub key: String,
    pub name: String,
    pub so_file: String,
    pub download_url: String,
}

#[derive(Debug, Deserialize)]
struct PlatformCores {
    #[serde(default)]
    cores: Vec<CoreInfo>,
}

#[derive(Debug, Deserialize)]
struct CoreCatalogFile {
    #[serde(default)]
    platform: HashMap<String, PlatformCores>,
}

pub const CORES_CATALOG_SOURCE: &str = include_str!("../../assets/retroarch_cores.toml");

/// Load the entire RetroArch core catalog from embedded TOML.
pub fn load_core_catalog() -> HashMap<String, Vec<CoreInfo>> {
    let raw: CoreCatalogFile =
        toml::from_str(CORES_CATALOG_SOURCE).unwrap_or_else(|_| CoreCatalogFile {
            platform: HashMap::new(),
        });
    raw.platform
        .into_iter()
        .map(|(k, v)| (k, v.cores))
        .collect()
}

/// Available cores for a platform slug (e.g. "snes", "megadrive", "nds", "ps1").
pub fn cores_for_platform(platform_slug: &str) -> Vec<CoreInfo> {
    let catalog = load_core_catalog();
    catalog.get(platform_slug).cloned().unwrap_or_default()
}

/// The default core for a platform slug (first entry in catalog).
pub fn default_core_for_platform(platform_slug: &str) -> Option<CoreInfo> {
    cores_for_platform(platform_slug).first().cloned()
}

/// Find a specific core info by its key for a platform slug.
pub fn core_by_key(platform_slug: &str, key: &str) -> Option<CoreInfo> {
    cores_for_platform(platform_slug)
        .into_iter()
        .find(|c| c.key == key)
}

/// Download an individual core `.so.zip` from buildbot if not already present in `target_cores_dir`.
/// Extracts `.so_file` into `target_cores_dir` and sets executable permissions.
pub async fn ensure_core_downloaded<P: AsRef<Path>>(
    core: &CoreInfo,
    target_cores_dir: P,
) -> Result<PathBuf> {
    let target_cores_dir = target_cores_dir.as_ref();
    std::fs::create_dir_all(target_cores_dir)
        .with_context(|| format!("Failed to create cores dir: {:?}", target_cores_dir))?;

    let so_path = target_cores_dir.join(&core.so_file);
    if so_path.is_file() {
        return Ok(so_path);
    }

    // Download ZIP archive into temp location
    let temp_zip =
        std::env::temp_dir().join(format!("core_{}_{}.zip", core.key, std::process::id()));
    let response = reqwest::get(&core.download_url)
        .await
        .with_context(|| format!("Failed to download core from {}", core.download_url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download core {}: HTTP status {}",
            core.name,
            response.status()
        );
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read core zip response for {}", core.name))?;

    std::fs::write(&temp_zip, &bytes)
        .with_context(|| format!("Failed to save temp core zip at {:?}", temp_zip))?;

    // Extract using system unzip or 7z
    let unzip_output = Command::new("unzip")
        .args(["-o", "-j"])
        .arg(&temp_zip)
        .arg(&core.so_file)
        .arg("-d")
        .arg(target_cores_dir)
        .output();

    let success = match unzip_output {
        Ok(out) if out.status.success() => true,
        _ => {
            // Fallback to 7z / 7zr
            Command::new("7z")
                .arg("x")
                .arg("-y")
                .arg(&temp_zip)
                .arg(format!("-o{}", target_cores_dir.display()))
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
    };

    let _ = std::fs::remove_file(&temp_zip);

    if !success || !so_path.is_file() {
        anyhow::bail!(
            "Could not extract '{}' from core archive for {}",
            core.so_file,
            core.name
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&so_path, std::fs::Permissions::from_mode(0o755));
    }

    Ok(so_path)
}
