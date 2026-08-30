use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RunnerKind {
    Proton,
    Wine,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RunnerLocation {
    TUIGameStation,
    Steam,
    LutrisWine,
    HeroicWine,
    HeroicProton,
    System,
    Custom,
}

impl RunnerLocation {
    pub fn display_name(&self) -> &'static str {
        match self {
            RunnerLocation::TUIGameStation => "TUI Game Station",
            RunnerLocation::Steam => "Steam (compatibilitytools.d)",
            RunnerLocation::LutrisWine => "Lutris (runners/wine)",
            RunnerLocation::HeroicWine => "Heroic (tools/wine)",
            RunnerLocation::HeroicProton => "Heroic (tools/proton)",
            RunnerLocation::System => "System Wine",
            RunnerLocation::Custom => "Custom Location",
        }
    }

    pub fn default_dir(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        match self {
            RunnerLocation::TUIGameStation => {
                Some(home.join(".local/share/tui_game_station/runners/wine"))
            }
            RunnerLocation::Steam => Some(home.join(".local/share/Steam/compatibilitytools.d")),
            RunnerLocation::HeroicProton => Some(home.join(".config/heroic/tools/proton")),
            RunnerLocation::HeroicWine => Some(home.join(".config/heroic/tools/wine")),
            RunnerLocation::LutrisWine => Some(home.join(".local/share/lutris/runners/wine")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledWineRunner {
    pub name: String,
    pub kind: RunnerKind,
    pub location: RunnerLocation,
    pub base_path: PathBuf,
    pub binary_path: PathBuf,
}

pub struct RunnerDetector;

impl RunnerDetector {
    /// Detect all installed Proton and Wine runners across TUI Game Station, Steam, Heroic, Lutris, and System paths
    pub fn detect_installed_wine_runners() -> Vec<InstalledWineRunner> {
        let mut list = Vec::new();
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return list,
        };

        let scan_locations = [
            (
                RunnerLocation::TUIGameStation,
                home.join(".local/share/tui_game_station/runners/wine"),
            ),
            (
                RunnerLocation::Steam,
                home.join(".local/share/Steam/compatibilitytools.d"),
            ),
            (
                RunnerLocation::Steam,
                home.join(".steam/root/compatibilitytools.d"),
            ),
            (
                RunnerLocation::Steam,
                home.join(".steam/steam/compatibilitytools.d"),
            ),
            (
                RunnerLocation::Steam,
                home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"),
            ),
            (
                RunnerLocation::LutrisWine,
                home.join(".local/share/lutris/runners/wine"),
            ),
            (
                RunnerLocation::LutrisWine,
                home.join(".var/app/net.lutris.Lutris/data/lutris/runners/wine"),
            ),
            (
                RunnerLocation::HeroicWine,
                home.join(".config/heroic/tools/wine"),
            ),
            (
                RunnerLocation::HeroicWine,
                home.join(".local/share/heroic/tools/wine"),
            ),
            (
                RunnerLocation::HeroicWine,
                home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/tools/wine"),
            ),
            (
                RunnerLocation::HeroicProton,
                home.join(".config/heroic/tools/proton"),
            ),
            (
                RunnerLocation::HeroicProton,
                home.join(".local/share/heroic/tools/proton"),
            ),
            (
                RunnerLocation::HeroicProton,
                home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/tools/proton"),
            ),
        ];

        let mut seen_paths = std::collections::HashSet::new();

        for (location, dir) in scan_locations {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if seen_paths.contains(&canonical) {
                        continue;
                    }
                    seen_paths.insert(canonical);

                    let folder_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    if folder_name.is_empty() {
                        continue;
                    }

                    let proton_bin = path.join("proton");
                    let proton_sh = path.join("proton.sh");

                    if proton_bin.exists() {
                        list.push(InstalledWineRunner {
                            name: folder_name,
                            kind: RunnerKind::Proton,
                            location: location.clone(),
                            base_path: path.clone(),
                            binary_path: proton_bin,
                        });
                    } else if proton_sh.exists() {
                        list.push(InstalledWineRunner {
                            name: folder_name,
                            kind: RunnerKind::Proton,
                            location: location.clone(),
                            base_path: path.clone(),
                            binary_path: proton_sh,
                        });
                    } else {
                        let wine_bin = path.join("bin").join("wine");
                        let wine64_bin = path.join("bin").join("wine64");

                        if wine_bin.exists() {
                            list.push(InstalledWineRunner {
                                name: folder_name,
                                kind: RunnerKind::Wine,
                                location: location.clone(),
                                base_path: path.clone(),
                                binary_path: wine_bin,
                            });
                        } else if wine64_bin.exists() {
                            list.push(InstalledWineRunner {
                                name: folder_name,
                                kind: RunnerKind::Wine,
                                location: location.clone(),
                                base_path: path.clone(),
                                binary_path: wine64_bin,
                            });
                        }
                    }
                }
            }
        }

        let system_wine_paths = ["/usr/bin/wine", "/usr/local/bin/wine"];
        for sys_path in system_wine_paths {
            let p = PathBuf::from(sys_path);
            if p.exists() && !seen_paths.contains(&p) {
                seen_paths.insert(p.clone());
                list.push(InstalledWineRunner {
                    name: "System Wine".to_string(),
                    kind: RunnerKind::Wine,
                    location: RunnerLocation::System,
                    base_path: p.clone(),
                    binary_path: p,
                });
            }
        }

        list
    }

    /// Check if a binary or runner is installed in PATH or standard directories
    pub fn is_runner_installed(runner_cmd: &str) -> bool {
        let binary_name = runner_cmd
            .split_whitespace()
            .next()
            .unwrap_or(runner_cmd)
            .trim_matches('"');

        if binary_name.is_empty() {
            return false;
        }

        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(':') {
                let bin_path = PathBuf::from(dir).join(binary_name);
                if bin_path.exists() && bin_path.is_file() {
                    return true;
                }
            }
        }

        let standard_paths = [
            format!("/usr/bin/{}", binary_name),
            format!("/usr/local/bin/{}", binary_name),
            format!("/opt/{}", binary_name),
        ];

        for path_str in standard_paths {
            if PathBuf::from(&path_str).exists() {
                return true;
            }
        }

        if binary_name == "steam"
            && dirs::home_dir()
                .map(|h| h.join(".var/app/com.valvesoftware.Steam").exists())
                .unwrap_or(false)
        {
            return true;
        }

        false
    }

    /// Scan a Wine prefix directory for installed .exe executables, excluding system Wine binaries
    pub fn scan_prefix_executables(wine_prefix: &str) -> Vec<PrefixExecutable> {
        let mut results = Vec::new();
        let prefix_path = PathBuf::from(wine_prefix);
        let drive_c = prefix_path.join("drive_c");

        if !drive_c.exists() {
            return results;
        }

        let system_folder_names = ["windows", "system32", "syswow64", "winsxs"];

        // (directory_path, current_depth)
        let mut dirs_to_walk = vec![(drive_c, 0)];
        while let Some((current_dir, depth)) = dirs_to_walk.pop() {
            if depth > 6 || results.len() >= 500 {
                continue;
            }

            if let Ok(entries) = std::fs::read_dir(&current_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();

                    // Skip symlinks (e.g. z: -> / or dosdevices symlinks) to prevent infinite loops!
                    if entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) {
                        continue;
                    }

                    let name = entry.file_name().to_string_lossy().to_string();

                    if path.is_dir() {
                        let name_lower = name.to_lowercase();
                        if system_folder_names.contains(&name_lower.as_str()) {
                            continue;
                        }
                        dirs_to_walk.push((path, depth + 1));
                    } else if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext.to_string_lossy().to_lowercase() == "exe" {
                                let name_lower = name.to_lowercase();
                                if name_lower.contains("unins")
                                    || name_lower.contains("uninstall")
                                    || name_lower.contains("setup")
                                    || name_lower.contains("installer")
                                {
                                    continue;
                                }

                                let rel_path = path
                                    .strip_prefix(&prefix_path)
                                    .unwrap_or(&path)
                                    .to_string_lossy()
                                    .to_string();

                                let display_name = path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or(name);

                                results.push(PrefixExecutable {
                                    display_name,
                                    full_path: path.to_string_lossy().to_string(),
                                    relative_path: rel_path,
                                });
                            }
                        }
                    }
                }
            }
        }

        results.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });
        results
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrefixExecutable {
    pub display_name: String,
    pub full_path: String,
    pub relative_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_installed_wine_runners() {
        let _runners = RunnerDetector::detect_installed_wine_runners();
    }
}
