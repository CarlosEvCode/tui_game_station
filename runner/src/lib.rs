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
        let mut env_vars = HashMap::new();

        if let Some(cmd) = &game.custom_command {
            return parse_command_string(cmd, game);
        }

        if game.game_type == "steam" {
            let appid = game.steam_appid.unwrap_or(0);
            let cmd = format!("steam steam://rungameid/{}", appid);
            return parse_command_string(&cmd, game);
        }

        if let Some(r) = runner {
            let mut template = r.command_template.clone();

            let file_path = game.file_path.clone().unwrap_or_default();
            template = template.replace("{rom}", &file_path);
            template = template.replace("{file_path}", &file_path);

            if let Some(exe) = &r.executable_path {
                template = template.replace("{executable_path}", exe);
            } else if template.contains("{executable_path}") {
                anyhow::bail!("No se ha configurado la ruta del ejecutable/AppImage para el runner '{}'. Presiona [m] para configurarlo.", r.name);
            }

            if let Some(prefix) = &game.wine_prefix {
                env_vars.insert("WINEPREFIX".to_string(), prefix.clone());
            }

            return parse_command_string(&template, game);
        }

        if let Some(path) = &game.file_path {
            return parse_command_string(&format!("\"{}\"", path), game);
        }

        anyhow::bail!("No suitable runner or executable command found for game: {}", game.title)
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
