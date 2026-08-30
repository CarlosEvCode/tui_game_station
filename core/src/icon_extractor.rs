use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Extracts an icon from a Windows `.exe` file using `wrestool` / `icotool` or `convert` ImageMagick,
/// saving the resulting PNG image to `~/.local/share/tui_game_station/icons/<entry_id>.png`.
pub fn extract_exe_icon(exe_path: &str, entry_id: i64) -> Result<PathBuf> {
    let exe = Path::new(exe_path);
    if !exe.exists() {
        anyhow::bail!("Executable file does not exist: {}", exe_path);
    }

    let icons_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("tui_game_station")
        .join("icons");

    std::fs::create_dir_all(&icons_dir)
        .with_context(|| format!("Failed to create icons directory: {:?}", icons_dir))?;

    let output_png = icons_dir.join(format!("{}.png", entry_id));
    let temp_ico = icons_dir.join(format!("{}_temp.ico", entry_id));

// Step 1: Use 7z to list `.rsrc/ICON/` files and extract the largest icon resource
    let l_output = Command::new("7z").args(["l", exe_path]).output();
    if let Ok(res) = l_output {
        if res.status.success() {
            let stdout_str = String::from_utf8_lossy(&res.stdout);
            let mut icon_entries = Vec::new();

            for line in stdout_str.lines() {
                if line.contains(".rsrc/ICON/") {
                    // Example line: ..... 204862 204840 .rsrc/ICON/21.ico
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let size: u64 = parts[1].parse().unwrap_or(0);
                        let path_name = parts[parts.len() - 1];
                        if size > 0 && path_name.ends_with(".ico") {
                            icon_entries.push((size, path_name.to_string()));
                        }
                    }
                }
            }

            // Sort by size descending to pick the highest resolution icon
            icon_entries.sort_by(|a, b| b.0.cmp(&a.0));

            for (_size, icon_rsrc_path) in icon_entries {
                let e_output = Command::new("7z")
                    .args(["e", "-so", exe_path, &icon_rsrc_path])
                    .output();
                if let Ok(e_res) = e_output {
                    if e_res.status.success() && !e_res.stdout.is_empty() {
                        let _ = std::fs::write(&temp_ico, &e_res.stdout);

                        let convert_cmd = if Command::new("magick").arg("-version").output().is_ok() {
                            "magick"
                        } else {
                            "convert"
                        };

                        let st = Command::new(convert_cmd)
                            .arg(&temp_ico)
                            .arg(&output_png)
                            .status();

                        let _ = std::fs::remove_file(&temp_ico);

                        if let Ok(st) = st {
                            if st.success() && output_png.exists() {
                                return Ok(output_png);
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 2: Try icoextract (Python tool commonly present in Arch/Omarchy/Fedora)
    let icoextract_status = Command::new("icoextract")
        .arg(exe_path)
        .arg(&output_png)
        .status();
    if let Ok(st) = icoextract_status {
        if st.success() && output_png.exists() {
            return Ok(output_png);
        }
    }

    // Step 3: Try wrestool to extract ICO resources from PE binary
    let wrestool_output = Command::new("wrestool")
        .args(["-x", "-t", "14", exe_path, "-o"])
        .arg(&temp_ico)
        .output();

    if let Ok(res) = wrestool_output {
        if res.status.success() && temp_ico.exists() {
            let convert_cmd = if Command::new("magick").arg("-version").output().is_ok() {
                "magick"
            } else {
                "convert"
            };
            let convert_status = Command::new(convert_cmd)
                .arg(&temp_ico)
                .arg(&output_png)
                .status();

            let _ = std::fs::remove_file(&temp_ico);

            if let Ok(st) = convert_status {
                if st.success() && output_png.exists() {
                    return Ok(output_png);
                }
            }
        }
    }

    anyhow::bail!("Could not extract icon from executable: {}", exe_path)
}
