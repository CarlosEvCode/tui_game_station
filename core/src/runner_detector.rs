use std::path::PathBuf;

pub struct RunnerDetector;

impl RunnerDetector {
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

        // Check if binary is in PATH
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(':') {
                let bin_path = PathBuf::from(dir).join(binary_name);
                if bin_path.exists() && bin_path.is_file() {
                    return true;
                }
            }
        }

        // Check standard Linux binary paths
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

        // Check flatpak applications if binary_name matches known flatpaks
        if binary_name == "steam" && dirs::home_dir().map(|h| h.join(".var/app/com.valvesoftware.Steam").exists()).unwrap_or(false) {
            return true;
        }

        false
    }
}
