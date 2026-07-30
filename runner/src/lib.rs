use anyhow::{Context, Result};
use game_core::models::{Game, Runner};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use tokio::process::Command;

pub struct GameRunner;

impl GameRunner {
    /// Format and build executable command line string for a game.
    pub fn build_command_line(game: &Game, runner: Option<&Runner>) -> Result<(String, Vec<String>, HashMap<String, String>)> {
        let mut base_envs = HashMap::new();

        if let Some(env_str) = &game.env_vars {
            for token in env_str.split_whitespace() {
                if let Some((k, v)) = token.split_once('=') {
                    base_envs.insert(k.to_string(), v.to_string());
                }
            }
        }

        let (mut exe, mut args, envs) = if game.game_type == "wine" {
            let file_path = game.file_path.clone().unwrap_or_default();
            let wine_prefix = game.wine_prefix.clone().unwrap_or_else(|| {
                if let Some(ref wdir) = game.working_dir {
                    if !wdir.trim().is_empty() {
                        return PathBuf::from(wdir).join("prefix").to_string_lossy().to_string();
                    }
                }
                let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"));
                data_dir.join("tui_game_station").join("wineprefixes").join(format!("p_{}", game.id)).to_string_lossy().to_string()
            });

            let _ = std::fs::create_dir_all(&wine_prefix);

            if let Some(cmd) = &game.custom_command {
                if !cmd.trim().is_empty() {
                    let mut local_envs = base_envs.clone();
                    if cmd.contains("proton") {
                        local_envs.insert("STEAM_COMPAT_DATA_PATH".to_string(), wine_prefix.clone());
                        let steam_dir = dirs::home_dir().map(|h| h.join(".local/share/Steam")).unwrap_or_default();
                        local_envs.insert("STEAM_COMPAT_CLIENT_INSTALL_PATH".to_string(), steam_dir.to_string_lossy().to_string());
                    } else {
                        local_envs.insert("WINEPREFIX".to_string(), wine_prefix.clone());
                    }
                    let mut shell_cmd = cmd.clone();
                    shell_cmd = shell_cmd.replace("{file_path}", &file_path);
                    shell_cmd = shell_cmd.replace("{wineprefix}", &wine_prefix);
                    (String::from("sh"), vec!["-c".to_string(), shell_cmd], local_envs)
                } else {
                    let mut local_envs = base_envs.clone();
                    local_envs.insert("WINEPREFIX".to_string(), wine_prefix);
                    let cmd = format!("wine \"{}\"", file_path);
                    let (e, a, mut pe) = parse_command_string(&cmd, game)?;
                    pe.extend(local_envs);
                    (e, a, pe)
                }
            } else {
                let mut local_envs = base_envs.clone();
                local_envs.insert("WINEPREFIX".to_string(), wine_prefix);
                let cmd = format!("wine \"{}\"", file_path);
                let (e, a, mut pe) = parse_command_string(&cmd, game)?;
                pe.extend(local_envs);
                (e, a, pe)
            }
        } else if let Some(cmd) = &game.custom_command {
            let (e, a, mut pe) = parse_command_string(cmd, game)?;
            pe.extend(base_envs.clone());
            (e, a, pe)
        } else if game.game_type == "steam" {
            let appid = game.steam_appid.unwrap_or(0);
            let cmd = format!("steam steam://rungameid/{}", appid);
            let (e, a, mut pe) = parse_command_string(&cmd, game)?;
            pe.extend(base_envs.clone());
            (e, a, pe)
        } else if let Some(r) = runner {
            let mut template = r.command_template.clone();
            let file_path = game.file_path.clone().unwrap_or_default();
            template = template.replace("{rom}", &file_path);
            template = template.replace("{file_path}", &file_path);

            if let Some(ex) = &r.executable_path {
                template = template.replace("{executable_path}", ex);
            } else if template.contains("{executable_path}") {
                anyhow::bail!("No se ha configurado la ruta del ejecutable/AppImage para el runner '{}'. Presiona [m] para configurarlo.", r.name);
            }

            let mut local_envs = base_envs.clone();
            if let Some(prefix) = &game.wine_prefix {
                local_envs.insert("WINEPREFIX".to_string(), prefix.clone());
            }

            let (e, a, mut pe) = parse_command_string(&template, game)?;
            pe.extend(local_envs);
            (e, a, pe)
        } else if let Some(path) = &game.file_path {
            let (e, a, mut pe) = parse_command_string(&format!("\"{}\"", path), game)?;
            pe.extend(base_envs.clone());
            (e, a, pe)
        } else {
            anyhow::bail!("No suitable runner or executable command found for game: {}", game.title)
        };

        // Apply Gamescope wrapper if enabled (outermost wrapper)
        if envs.get("GAMESCOPE").map(|v| v == "1").unwrap_or(false) {
            args.insert(0, "--".to_string());
            args.insert(0, exe.clone());
            exe = "gamescope".to_string();
        }

        // Apply GameMode wrapper if enabled
        if envs.get("GAMEMODE").map(|v| v == "1").unwrap_or(false) {
            args.insert(0, exe.clone());
            exe = "gamemoderun".to_string();
        }

        // Apply MangoHud wrapper if enabled
        if envs.get("MANGOHUD").map(|v| v == "1").unwrap_or(false) {
            args.insert(0, exe.clone());
            exe = "mangohud".to_string();
        }

        Ok((exe, args, envs))
    }

    /// Launch game process asynchronously, isolating stdout/stderr to a log file to avoid TUI terminal corruption.
    pub async fn launch_game(game: &Game, runner: Option<&Runner>) -> Result<ExitStatus> {
        let (exe, args, envs) = Self::build_command_line(game, runner)?;

        let mut cmd = Command::new(&exe);
        cmd.args(&args);

        if let Some(work_dir) = &game.working_dir {
            cmd.current_dir(work_dir);
        }

        for (k, v) in envs {
            cmd.env(k, v);
        }

        // Redirect stdout & stderr to log file to prevent terminal text corruption
        let log_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("tui_game_station")
            .join("logs");

        let _ = std::fs::create_dir_all(&log_dir);
        let log_file_path = log_dir.join("last_game_launch.log");

        if let Ok(file) = File::create(&log_file_path) {
            if let Ok(err_file) = file.try_clone() {
                cmd.stdout(Stdio::from(file));
                cmd.stderr(Stdio::from(err_file));
            } else {
                cmd.stdout(Stdio::null());
                cmd.stderr(Stdio::null());
            }
        } else {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn game process: {} {:?}", exe, args))?;

        let status = child.wait().await?;
        Ok(status)
    }
}

fn parse_command_string(full_cmd: &str, _game: &Game) -> Result<(String, Vec<String>, HashMap<String, String>)> {
    let parts = shlex_split(full_cmd);
    if parts.is_empty() {
        anyhow::bail!("Empty command string");
    }

    let exe = parts[0].clone();
    let args = parts[1..].to_vec();
    let envs = HashMap::new();

    Ok((exe, args, envs))
}

fn shlex_split(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in cmd.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}
