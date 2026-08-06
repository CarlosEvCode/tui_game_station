use std::fs;
use std::path::Path;

/// Minimal, surgical patcher for RetroArch `retroarch.cfg` files.
///
/// `retroarch.cfg` uses root-level key-value pairs of the form:
/// `key = "value"` or `key = value`
///
/// This patcher reads values safely (stripping double quotes if present)
/// and updates or appends key-value pairs as `key = "new_value"`.

/// Read the current unquoted string value of `key` from `retroarch.cfg`.
pub fn read_retroarch_cfg_value(
    path: &Path,
    key: &str,
) -> Result<Option<String>, crate::config_patch::qt_ini::PatchError> {
    if !path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(path).map_err(|source| {
        crate::config_patch::qt_ini::PatchError::Read {
            path: path.to_path_buf(),
            source,
        }
    })?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == key {
                let val = v.trim().trim_matches('"').to_string();
                return Ok(Some(val));
            }
        }
    }

    Ok(None)
}

/// Patch `key` in `retroarch.cfg` to `"new_value"`.
/// If `key` is present, its line is replaced atomically.
/// If `key` is absent, it is appended to the file.
pub fn patch_retroarch_cfg(
    path: &Path,
    key: &str,
    new_value: &str,
) -> Result<(), crate::config_patch::qt_ini::PatchError> {
    let mut lines: Vec<String> = if path.is_file() {
        let content = fs::read_to_string(path).map_err(|source| {
            crate::config_patch::qt_ini::PatchError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
        content.lines().map(|l| l.to_string()).collect()
    } else {
        Vec::new()
    };

    let target_line = format!("{} = \"{}\"", key, new_value);
    let mut found = false;

    for line in lines.iter_mut() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim() == key {
                *line = target_line.clone();
                found = true;
                break;
            }
        }
    }

    if !found {
        lines.push(target_line);
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    fs::write(path, output).map_err(|source| {
        crate::config_patch::qt_ini::PatchError::TempWrite {
            path: path.to_path_buf(),
            source,
        }
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_retroarch_cfg() {
        let temp = std::env::temp_dir().join(format!("test_retroarch_{}.cfg", std::process::id()));
        let path = temp.as_path();

        fs::write(path, "video_fullscreen = \"false\"\nvideo_vsync = \"true\"\n").unwrap();

        assert_eq!(
            read_retroarch_cfg_value(path, "video_fullscreen").unwrap(),
            Some("false".to_string())
        );

        patch_retroarch_cfg(path, "video_fullscreen", "true").unwrap();
        assert_eq!(
            read_retroarch_cfg_value(path, "video_fullscreen").unwrap(),
            Some("true".to_string())
        );

        patch_retroarch_cfg(path, "video_driver", "vulkan").unwrap();
        assert_eq!(
            read_retroarch_cfg_value(path, "video_driver").unwrap(),
            Some("vulkan".to_string())
        );

        let _ = fs::remove_file(path);
    }
}
