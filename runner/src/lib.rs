use anyhow::{Context, Result};
use game_core::models::{Game, Runner};
use game_core::options::{load_emulator_options, merge_runner_options, resolve_flags, RunnerOptionEnv};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};
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
                if !ex.trim().is_empty() && std::path::Path::new(ex).exists() {
                    template = template.replace("{executable_path}", ex);
                } else {
                    anyhow::bail!("El ejecutable/AppImage para '{}' no existe en disco ({}). Presiona [m] para configurar o descargar.", r.name, ex);
                }
            } else {
                anyhow::bail!("El emulador '{}' no tiene configurado su ejecutable/AppImage. Presiona [m] para configurar o descargar.", r.name);
            }

            let mut local_envs = base_envs.clone();
            if let Some(prefix) = &game.wine_prefix {
                local_envs.insert("WINEPREFIX".to_string(), prefix.clone());
            }

            let (e, mut a, mut pe) = parse_command_string(&template, game)?;
            pe.extend(local_envs);

            // Inject TOML-defined emulator options + custom args before the ROM.
            let option_flags = resolved_runner_flags(r);
            if !option_flags.is_empty() {
                a.splice(0..0, option_flags);
            }
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
        let work_dir = game.working_dir.clone();
        Self::spawn_and_wait(&exe, &args, &envs, work_dir.as_deref()).await
    }

    /// Launch an emulator standalone (no ROM) reusing its configured options,
    /// so users can open the emulator UI / settings directly.
    pub async fn launch_standalone(runner: &Runner) -> Result<ExitStatus> {
        let exe = runner
            .executable_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("El emulador '{}' no tiene ejecutable configurado.", runner.name))?;
        if !std::path::Path::new(&exe).exists() {
            anyhow::bail!("El ejecutable/AppImage para '{}' no existe en disco ({}).", runner.name, exe);
        }

        let args = resolved_runner_flags(runner);
        Self::spawn_and_wait(&exe, &args, &HashMap::new(), None).await
    }

    async fn spawn_and_wait(
        exe: &str,
        args: &[String],
        envs: &HashMap<String, String>,
        work_dir: Option<&str>,
    ) -> Result<ExitStatus> {
        let mut cmd = Command::new(exe);
        cmd.args(args);

        if let Some(wd) = work_dir {
            cmd.current_dir(wd);
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

        let start = Instant::now();
        let pid = child.id().unwrap_or(0);
        tracing::info!(
            "[launch] spawned exe={:?} args={:?} pid={}",
            exe,
            args,
            pid
        );

        // For AppImages the PID captured above may be the AppImage *runtime*
        // wrapper rather than the process that runs the game. The runtime
        // either execs the real binary into the same PID or forks it as a
        // child (itself mounted under /tmp/.mount_*). Give it a moment to
        // settle, log the process tree, and remember the "real" process.
        let mut real_pid: Option<u32> = None;
        if is_appimage(exe) {
            tokio::time::sleep(Duration::from_millis(600)).await;
            log_process_tree(pid, "appimage-settle");
            real_pid = find_mount_process(pid);
            match real_pid {
                Some(rp) if rp != pid => tracing::info!(
                    "[launch] pid={} is an AppImage wrapper; real game process runs as pid={}",
                    pid,
                    rp
                ),
                Some(_) => tracing::info!(
                    "[launch] pid={} exec'd the AppImage binary into the same PID",
                    pid
                ),
                None => tracing::info!(
                    "[launch] no /tmp/.mount_* process under pid={}; waiting on the direct child",
                    pid
                ),
            }
        } else {
            log_process_tree(pid, "after-spawn");
        }

        let status = child.wait().await?;

        // If the wrapper exited while the real game process is still running
        // (AppImage runtime that forked instead of exec'ing), keep waiting so
        // the TUI only resumes once the game actually closed.
        if let Some(rp) = real_pid {
            if rp != pid && process_exists(rp) {
                tracing::info!(
                    "[launch] wrapper pid={} exited but real game process pid={} still alive; waiting for it",
                    pid,
                    rp
                );
                wait_until_pid_gone(rp).await;
            }
        }

        tracing::info!(
            "[launch] game process pid={} exited: {:?} after {:?}",
            pid,
            status,
            start.elapsed()
        );
        log_leftover_mount_processes();

        Ok(status)
    }
}

fn is_appimage(exe: &str) -> bool {
    exe.to_lowercase().contains(".appimage")
}

fn proc_cmdline(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|s| s.replace('\0', " "))
}

fn proc_comm(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn proc_exe(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.display().to_string())
}

fn proc_children(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .ok()
        .map(|s| s.split_whitespace().filter_map(|t| t.parse().ok()).collect())
        .unwrap_or_default()
}

fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

async fn wait_until_pid_gone(pid: u32) {
    while process_exists(pid) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The process (or one of its children) whose command line references a
/// mounted AppImage (`/tmp/.mount_*`), i.e. the real game process.
fn find_mount_process(pid: u32) -> Option<u32> {
    if proc_cmdline(pid).is_some_and(|c| c.contains(".mount_")) {
        return Some(pid);
    }
    proc_children(pid)
        .into_iter()
        .find(|c| proc_cmdline(*c).is_some_and(|cl| cl.contains(".mount_")))
}

fn log_process_tree(pid: u32, label: &str) {
    tracing::info!(
        "[{label}] pid={} comm={:?} exe={:?} cmdline={:?}",
        pid,
        proc_comm(pid).unwrap_or_else(|| "<gone>".to_string()),
        proc_exe(pid).unwrap_or_else(|| "<gone>".to_string()),
        proc_cmdline(pid).unwrap_or_else(|| "<gone>".to_string())
    );
    for c in proc_children(pid) {
        tracing::info!(
            "[{label}] pid={} child pid={} comm={:?} cmdline={:?}",
            pid,
            c,
            proc_comm(c).unwrap_or_else(|| "<gone>".to_string()),
            proc_cmdline(c).unwrap_or_else(|| "<gone>".to_string())
        );
    }
}

/// Report any process still referencing an AppImage mount after a game exits,
/// to spot orphaned FUSE mounts / leftover processes.
fn log_leftover_mount_processes() {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if let Some(cmd) = proc_cmdline(pid) {
            if cmd.contains(".mount_") {
                tracing::info!(
                    "[launch] leftover AppImage process pid={} comm={:?} cmdline={:?}",
                    pid,
                    proc_comm(pid).unwrap_or_default(),
                    cmd
                );
            }
        }
    }
}

/// Resolve the emulator-options flags + custom args stored on a runner row.
fn resolved_runner_flags(runner: &Runner) -> Vec<String> {
    let Some(env_json) = &runner.env_vars else {
        return Vec::new();
    };
    let env: RunnerOptionEnv = game_core::options::from_env_json(env_json);
    let Ok(defs) = load_emulator_options(&runner.name) else {
        return Vec::new();
    };
    if defs.is_empty()
        && env.custom_args
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    {
        return Vec::new();
    }
    let options_map = env.emulator_options.clone().unwrap_or_default();
    let merged = merge_runner_options(&defs, &options_map);
    resolve_flags(&defs, &merged, env.custom_args.as_deref().unwrap_or(""))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_appimage_detects_appimage_paths() {
        assert!(is_appimage("/home/x/.local/bin/eden.AppImage"));
        assert!(is_appimage("/opt/melonDS-x86_64.AppImage"));
        assert!(!is_appimage("/usr/bin/steam"));
        assert!(!is_appimage("wine"));
    }

    #[test]
    fn proc_helpers_report_the_current_process() {
        let self_pid = std::process::id();
        assert!(process_exists(self_pid), "current process must exist in /proc");
        assert!(proc_comm(self_pid).is_some_and(|c| !c.is_empty()));
        assert!(proc_exe(self_pid).is_some_and(|p| !p.is_empty()));
        assert!(proc_cmdline(self_pid).is_some_and(|c| !c.is_empty()));
    }

    #[test]
    fn find_mount_process_falls_back_to_none_for_non_appimage() {
        // The test binary itself is not an AppImage mount: detection must
        // return None so the caller keeps waiting on the direct child.
        let self_pid = std::process::id();
        assert!(find_mount_process(self_pid).is_none());
    }

    #[test]
    fn wait_until_pid_gone_returns_for_a_gone_pid() {
        // A PID that can never exist (max u32) resolves immediately.
        let mut runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(wait_until_pid_gone(u32::MAX));
    }
}

