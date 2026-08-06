use anyhow::{Context, Result};
use game_core::models::{Game, Runner};
use game_core::options::{
    emulator_process_name, load_emulator_options, merge_runner_options, resolve_flags,
    RunnerOptionEnv,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::process::Command;

/// The real game process (and its AppImage mount) currently being run, so the
/// TUI can show a "running" indicator and force-close it from outside the
/// `spawn_and_wait` future. The wrapper PID is always known; the real PID is
/// the process that actually runs the emulator/game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunningProcess {
    pub wrapper_pid: u32,
    pub real_pid: Option<u32>,
    /// SquashFS mount point exposed by the AppImage runtime (`APPDIR`), when
    /// the launch went through an AppImage.
    pub mount_path: Option<String>,
}

static RUNNING: Mutex<Option<RunningProcess>> = Mutex::new(None);

pub struct GameRunner;

impl GameRunner {
    /// The process(es) the currently running game is using, if any. Shared with
    /// the TUI so it can render the running indicator and force-close the game.
    pub fn current_running() -> Option<RunningProcess> {
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Kill the currently running game: real process + its whole child tree +
    /// the AppImage wrapper + a best-effort FUSE unmount of its SquashFS mount.
    /// Returns a human-readable summary (best-effort, never fails hard).
    pub fn force_close_current_game() -> std::result::Result<String, String> {
        let Some(info) = Self::current_running() else {
            return Err("No hay un juego en ejecución para forzar su cierre.".to_string());
        };

        let mut killed: Vec<u32> = Vec::new();
        if let Some(real) = info.real_pid.filter(|p| *p != 0) {
            killed.extend(kill_process_tree(real));
        }
        if info.wrapper_pid != 0 {
            killed.extend(kill_process_tree(info.wrapper_pid));
        }
        killed.sort_unstable();
        killed.dedup();

        let mut unmounted: Vec<String> = Vec::new();
        if let Some(mount) = &info.mount_path {
            // Kill any lingering mounter still holding our own mount, then
            // release it (best-effort; FUSE usually auto-unmounts on daemon exit).
            for pid in proc_all_pids() {
                if is_known_mount_or_runtime(&proc_comm(pid).unwrap_or_default())
                    && proc_cmdline(pid).is_some_and(|c| c.contains(mount))
                {
                    kill_pid(pid);
                }
            }
            if try_unmount_mount_path(mount) {
                unmounted.push(mount.clone());
            }
        }

        let summary = format!(
            "Forzado cierre: {} procesos terminados{}",
            killed.len(),
            if unmounted.is_empty() {
                String::new()
            } else {
                format!(", {} montaje(s) FUSE liberado(s)", unmounted.len())
            }
        );
        tracing::warn!(
            "[force-close] {summary} (pids={:?}, mounts={:?})",
            killed,
            unmounted
        );
        Ok(summary)
    }

    /// Format and build executable command line string for a game.
    pub fn build_command_line(
        game: &Game,
        runner: Option<&Runner>,
    ) -> Result<(String, Vec<String>, HashMap<String, String>)> {
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
                        return PathBuf::from(wdir)
                            .join("prefix")
                            .to_string_lossy()
                            .to_string();
                    }
                }
                let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"));
                data_dir
                    .join("tui_game_station")
                    .join("wineprefixes")
                    .join(format!("p_{}", game.id))
                    .to_string_lossy()
                    .to_string()
            });

            let _ = std::fs::create_dir_all(&wine_prefix);

            if let Some(cmd) = &game.custom_command {
                if !cmd.trim().is_empty() {
                    let mut local_envs = base_envs.clone();
                    if cmd.contains("proton") {
                        local_envs
                            .insert("STEAM_COMPAT_DATA_PATH".to_string(), wine_prefix.clone());
                        let steam_dir = dirs::home_dir()
                            .map(|h| h.join(".local/share/Steam"))
                            .unwrap_or_default();
                        local_envs.insert(
                            "STEAM_COMPAT_CLIENT_INSTALL_PATH".to_string(),
                            steam_dir.to_string_lossy().to_string(),
                        );
                    } else {
                        local_envs.insert("WINEPREFIX".to_string(), wine_prefix.clone());
                    }
                    let mut shell_cmd = cmd.clone();
                    shell_cmd = shell_cmd.replace("{file_path}", &file_path);
                    shell_cmd = shell_cmd.replace("{wineprefix}", &wine_prefix);
                    (
                        String::from("sh"),
                        vec!["-c".to_string(), shell_cmd],
                        local_envs,
                    )
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
            // `{rom_dir}` is the folder that contains the ROM: MAME-style
            // emulators need it as `-rompath` so sibling/parent/BIOS sets are
            // found when the ROM itself is passed as a full path.
            let rom_dir = Path::new(&file_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".to_string());
            template = template.replace("{rom_dir}", &rom_dir);

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
            anyhow::bail!(
                "No suitable runner or executable command found for game: {}",
                game.title
            )
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
        let expected = runner.as_ref().and_then(|r| emulator_process_name(&r.name));
        let work_dir = game.working_dir.clone();
        Self::spawn_and_wait(&exe, &args, &envs, work_dir.as_deref(), expected.as_deref()).await
    }

    /// Launch an emulator standalone (no ROM) reusing its configured options,
    /// so users can open the emulator UI / settings directly.
    pub async fn launch_standalone(runner: &Runner) -> Result<ExitStatus> {
        let exe = runner.executable_path.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "El emulador '{}' no tiene ejecutable configurado.",
                runner.name
            )
        })?;
        if !std::path::Path::new(&exe).exists() {
            anyhow::bail!(
                "El ejecutable/AppImage para '{}' no existe en disco ({}).",
                runner.name,
                exe
            );
        }

        let expected = emulator_process_name(&runner.name);
        let args = resolved_runner_flags(runner);
        Self::spawn_and_wait(&exe, &args, &HashMap::new(), None, expected.as_deref()).await
    }

    async fn spawn_and_wait(
        exe: &str,
        args: &[String],
        envs: &HashMap<String, String>,
        work_dir: Option<&str>,
        expected_process: Option<&str>,
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
        tracing::info!("[launch] spawned exe={:?} args={:?} pid={}", exe, args, pid);

        // For AppImages the PID captured above may be the AppImage *runtime*
        // wrapper rather than the process that runs the game. The runtime
        // either execs the real binary into the same PID or forks it as a
        // child (itself mounted under /tmp/.mount_*). Give it a moment to
        // settle, log the process tree, and remember the "real" process by its
        // emulator name (the FUSE mounter `memfd:dwarfs` is excluded).
        let mut real_pid: Option<u32> = None;
        if is_appimage(exe) {
            tokio::time::sleep(Duration::from_millis(600)).await;
            log_process_tree(pid, "appimage-settle");

            if let Some(name) = expected_process {
                if let Some(found) = find_process_by_name(pid, name) {
                    real_pid = Some(found);
                    tracing::info!(
                        "[launch] identified real game process by name {:?}: pid={} comm={:?}",
                        name,
                        found,
                        proc_comm(found).unwrap_or_default()
                    );
                } else {
                    tracing::warn!(
                        "[launch] no process matching {:?} under pid={}; falling back to mount-based detection",
                        name,
                        pid
                    );
                }
            }
            if real_pid.is_none() {
                if let Some(found) = find_mount_process(pid) {
                    real_pid = Some(found);
                    tracing::warn!(
                        "[launch] fallback mount-based detection selected pid={} comm={:?}",
                        found,
                        proc_comm(found).unwrap_or_default()
                    );
                } else {
                    tracing::warn!(
                        "[launch] real game process not identified under pid={}; waiting on the direct child",
                        pid
                    );
                }
            }
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
                None => {}
            }
        } else {
            log_process_tree(pid, "after-spawn");
        }

        // Expose the running game to the TUI (force-close / indicator) for the
        // whole lifetime of the process, clearing it only once the game exited.
        let mount_path = real_pid
            .filter(|rp| *rp != pid)
            .and_then(|rp| env_value(rp, "APPDIR"))
            .or_else(|| env_value(pid, "APPDIR"));
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(RunningProcess {
            wrapper_pid: pid,
            real_pid,
            mount_path,
        });

        let status = child.wait().await;

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
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = None;

        let status = status?;
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
        .map(|s| {
            s.split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn proc_all_pids() -> Vec<u32> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
                out.push(pid);
            }
        }
    }
    out
}

fn env_value(pid: u32, key: &str) -> Option<String> {
    std::fs::read(format!("/proc/{pid}/environ"))
        .ok()?
        .split(|b| *b == 0)
        .filter_map(|s| std::str::from_utf8(s).ok())
        .find_map(|e| e.strip_prefix(key)?.strip_prefix('=').map(str::to_string))
}

fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

async fn wait_until_pid_gone(pid: u32) {
    while process_exists(pid) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `comm` names of AppImage mount/runtime helper processes that serve the
/// SquashFS but never run the game itself. They must never be picked as the
/// "real" game process.
const MOUNT_OR_RUNTIME_COMMS: &[&str] = &[
    "memfd:dwarfs",
    "dwarfs",
    "apprun",
    "type2-runtime",
    "appimage-runtime",
    "appimagelauncher",
    "squashfuse",
    "squashfuse_ll",
    "fusermount",
    "fusermount3",
    "mount.fuse",
    "fuse-overlayfs",
];

fn is_known_mount_or_runtime(name: &str) -> bool {
    let lc = name.to_lowercase();
    MOUNT_OR_RUNTIME_COMMS.iter().any(|k| lc.contains(k))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// True when a process whose `comm`/`exe`/`cmdline` are given should be treated
/// as the real game process for the emulator `expected`.
fn name_matches_expected(expected: &str, comm: &str, exe: &str, cmdline: &str) -> bool {
    let exp = expected.to_lowercase();
    if exp.is_empty() {
        return false;
    }
    let comm_lc = comm.to_lowercase();
    let exe_lc = exe.to_lowercase();
    let cmd_lc = cmdline.to_lowercase();

    if comm_lc == exp {
        return true;
    }
    if basename(&exe_lc) == exp {
        return true;
    }
    // argv[0] may carry the real binary name even when /proc/pid/exe is gone.
    if let Some(first) = cmd_lc.split_whitespace().next() {
        if basename(first.trim_matches('"')) == exp {
            return true;
        }
    }
    // Last-resort inference: emulator binary names often prefix the real comm
    // (e.g. "dolphin" -> "dolphin-emu"). Require a word boundary after the
    // prefix so unrelated processes that merely share it (e.g. "dolphin2") are
    // not mistaken for the emulator.
    !comm_lc.is_empty()
        && comm_lc.len() > exp.len()
        && comm_lc.starts_with(&exp)
        && !comm_lc.as_bytes()[exp.len()].is_ascii_alphanumeric()
}

fn process_matches_name(pid: u32, expected: &str) -> bool {
    let comm = proc_comm(pid).unwrap_or_default();
    let exe = proc_exe(pid).unwrap_or_default();
    let cmdline = proc_cmdline(pid).unwrap_or_default();
    if comm.is_empty() && exe.is_empty() && cmdline.is_empty() {
        return false;
    }
    if is_known_mount_or_runtime(&comm) {
        return false;
    }
    name_matches_expected(expected, &comm, &exe, &cmdline)
}

/// Breadth-first search over `root`'s process subtree for a process matching
/// the emulator name, excluding known AppImage mount/runtime helpers.
fn find_process_by_name(root: u32, expected: &str) -> Option<u32> {
    let mut queue = VecDeque::from([root]);
    let mut seen = HashSet::new();
    while let Some(pid) = queue.pop_front() {
        if !seen.insert(pid) {
            continue;
        }
        if process_matches_name(pid, expected) {
            return Some(pid);
        }
        for c in proc_children(pid) {
            if !seen.contains(&c) {
                queue.push_back(c);
            }
        }
    }
    None
}

/// The process (or one of its descendants) whose command line references a
/// mounted AppImage (`/tmp/.mount_*`), i.e. the real game process. Descends the
/// whole subtree and skips known mount/runtime helpers so the FUSE mounter
/// (`memfd:dwarfs`) is never mistaken for the game.
fn find_mount_process(pid: u32) -> Option<u32> {
    let mut queue = VecDeque::from([pid]);
    let mut seen = HashSet::new();
    while let Some(p) = queue.pop_front() {
        if !seen.insert(p) {
            continue;
        }
        let comm = proc_comm(p).unwrap_or_default();
        if is_known_mount_or_runtime(&comm) {
            continue;
        }
        if proc_cmdline(p).is_some_and(|c| c.contains(".mount_")) {
            return Some(p);
        }
        for c in proc_children(p) {
            if !seen.contains(&c) {
                queue.push_back(c);
            }
        }
    }
    None
}

fn kill_pid(pid: u32) {
    if process_exists(pid) {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
    }
}

/// SIGKILL `root` and every descendant, deepest first (children before their
/// parent, so nothing gets reparented while we walk). Returns the killed PIDs.
fn kill_process_tree(root: u32) -> Vec<u32> {
    let mut order = Vec::new();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        order.push(pid);
        for c in proc_children(pid) {
            if !seen.contains(&c) {
                stack.push(c);
            }
        }
    }
    order.reverse();
    order.retain(|pid| process_exists(*pid));
    for pid in &order {
        kill_pid(*pid);
    }
    order
}

/// Best-effort release of an AppImage SquashFS mount point.
fn try_unmount_mount_path(mount: &str) -> bool {
    for cmd in ["fusermount3", "fusermount", "umount"] {
        if std::process::Command::new(cmd)
            .args(["-u", mount])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

fn log_process_tree(pid: u32, label: &str) {
    log_process_tree_depth(pid, label, 0, 6, &mut HashSet::new());
}

fn log_process_tree_depth(
    pid: u32,
    label: &str,
    depth: u32,
    max_depth: u32,
    seen: &mut HashSet<u32>,
) {
    if depth > max_depth || !seen.insert(pid) {
        return;
    }
    tracing::info!(
        "[{label}] {}pid={} comm={:?} exe={:?} cmdline={:?}",
        "  ".repeat(depth as usize),
        pid,
        proc_comm(pid).unwrap_or_else(|| "<gone>".to_string()),
        proc_exe(pid).unwrap_or_else(|| "<gone>".to_string()),
        proc_cmdline(pid).unwrap_or_else(|| "<gone>".to_string())
    );
    for c in proc_children(pid) {
        log_process_tree_depth(c, label, depth + 1, max_depth, seen);
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
        && env
            .custom_args
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

fn parse_command_string(
    full_cmd: &str,
    _game: &Game,
) -> Result<(String, Vec<String>, HashMap<String, String>)> {
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
        assert!(
            process_exists(self_pid),
            "current process must exist in /proc"
        );
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
    fn mount_or_runtime_names_are_recognized_and_excluded() {
        assert!(is_known_mount_or_runtime("memfd:dwarfs"));
        assert!(is_known_mount_or_runtime("AppRun"));
        assert!(is_known_mount_or_runtime("fusermount3"));
        assert!(!is_known_mount_or_runtime("azahar"));
        assert!(!is_known_mount_or_runtime("dolphin-emu"));
    }

    #[test]
    fn name_matching_is_case_insensitive_and_name_aware() {
        // Exact comm match.
        assert!(name_matches_expected("azahar", "azahar", "", ""));
        // Real binary basename match (Dolphin AppImage runs `dolphin-emu`).
        assert!(name_matches_expected(
            "dolphin-emu",
            "",
            "/tmp/.mount_dolphin_AbCd/usr/bin/dolphin-emu",
            ""
        ));
        // argv[0] basename match.
        assert!(name_matches_expected("melonDS", "", "", "\"melonDS\""));
        // Prefix inference ("dolphin" -> "dolphin-emu").
        assert!(name_matches_expected("dolphin", "dolphin-emu", "", ""));
        // Prefix inference requires a word boundary: unrelated processes that
        // merely share the prefix are not matched.
        assert!(!name_matches_expected("dolphin", "dolphinemu", "", ""));
        assert!(!name_matches_expected("dolphin", "dolphin2", "", ""));
        // Mount/runtime processes never match, even via prefix.
        assert!(!name_matches_expected("azahar", "memfd:dwarfs", "", ""));
        assert!(!name_matches_expected("azahar", "AppRun", "", ""));
        assert!(!name_matches_expected("", "anything", "", ""));
    }

    #[test]
    fn find_process_by_name_locates_a_real_child_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let found = find_process_by_name(pid, "sleep");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(found, Some(pid), "the direct child must be found by name");
    }

    #[test]
    fn kill_process_tree_handles_a_gone_pid() {
        assert!(kill_process_tree(u32::MAX).is_empty());
    }

    #[test]
    fn mame_template_expands_rom_dir_for_rompath() {
        let exe_path =
            std::env::temp_dir().join(format!("mame_test_{}.AppImage", std::process::id()));
        std::fs::write(&exe_path, []).unwrap();
        let game = game_core::models::Game {
            id: 0,
            platform_id: 0,
            folder_id: None,
            title: "1943u".to_string(),
            sort_title: None,
            game_type: "emulator".to_string(),
            file_path: Some("/roms/arcade/1943u.zip".to_string()),
            working_dir: Some("/roms/arcade".to_string()),
            custom_command: None,
            env_vars: None,
            wine_prefix: None,
            wine_runner_id: None,
            steam_appid: None,
            file_name: None,
            file_extension: None,
            file_size: None,
            file_hash_crc32: None,
            file_hash_md5: None,
            file_hash_sha1: None,
            serial: None,
            release_year: None,
            developer: None,
            publisher: None,
            description: None,
            genre: None,
            rating: None,
            favorite: false,
            play_count: 0,
            play_time_seconds: 0,
            last_played_at: None,
            created_at: String::new(),
            updated_at: String::new(),
            components: Vec::new(),
            is_missing_base: false,
        };
        let runner = game_core::models::Runner {
            id: 0,
            platform_id: None,
            name: "MAME".to_string(),
            runner_type: "appimage".to_string(),
            executable_path: Some(exe_path.to_string_lossy().to_string()),
            command_template: "\"{executable_path}\" -rompath \"{rom_dir}\" \"{rom}\"".to_string(),
            default_env: None,
            download_url: None,
            download_filename: None,
            is_default: false,
            is_active: false,
            env_vars: None,
        };

        let (_exe, args, _envs) = GameRunner::build_command_line(&game, Some(&runner)).unwrap();
        let _ = std::fs::remove_file(&exe_path);
        assert_eq!(args[0], "-rompath");
        assert_eq!(args[1], "/roms/arcade");
        assert_eq!(args[2], "/roms/arcade/1943u.zip");
    }

    #[test]
    fn wait_until_pid_gone_returns_for_a_gone_pid() {
        // A PID that can never exist (max u32) resolves immediately.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(wait_until_pid_gone(u32::MAX));
    }
}
