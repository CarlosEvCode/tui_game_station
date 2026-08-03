use std::env;
use std::process::Command;

/// Minimize the active TUI window before launching a game process,
/// adapted to Mango WM, Hyprland, Sway, Niri, KDE, GNOME, and X11 WMs.
pub fn minimize_active_window() {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let is_mango = desktop.contains("mango") || env::var("MANGO_INSTANCE_SIGNATURE").is_ok();
    let is_hyprland = desktop.contains("hyprland") || env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();
    let is_sway = desktop.contains("sway") || env::var("SWAYSOCK").is_ok();

    if is_mango {
        let _ = Command::new("mmsg").args(["dispatch", "minimized"]).output();
    } else if is_hyprland {
        let _ = Command::new("hyprctl").args(["dispatch", "movetoworkspacesilent", "special:minimized"]).output();
    } else if is_sway {
        let _ = Command::new("swaymsg").args(["move", "scratchpad"]).output();
    } else if desktop.contains("niri") {
        let _ = Command::new("niri").args(["msg", "action", "move-column-to-workspace-down"]).output();
    } else {
        let _ = Command::new("xdotool").args(["getactivewindow", "windowminimize"]).output();
    }
}

/// Restore the TUI window after the game process exits
pub fn restore_active_window() {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let is_mango = desktop.contains("mango") || env::var("MANGO_INSTANCE_SIGNATURE").is_ok();
    let is_hyprland = desktop.contains("hyprland") || env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();
    let is_sway = desktop.contains("sway") || env::var("SWAYSOCK").is_ok();

    if is_mango {
        let _ = Command::new("mmsg").args(["dispatch", "restore_minimized"]).output();
    } else if is_hyprland {
        let _ = Command::new("hyprctl").args(["dispatch", "movetoworkspace", "e+0"]).output();
    } else if is_sway {
        let _ = Command::new("swaymsg").args(["scratchpad", "show"]).output();
    } else if desktop.contains("niri") {
        let _ = Command::new("niri").args(["msg", "action", "move-column-to-workspace-up"]).output();
    } else {
        let _ = Command::new("xdotool").args(["windowactivate"]).output();
    }
}

/// Force or exit fullscreen mode for the active TUI window
pub fn set_fullscreen(enable: bool) {
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let is_mango = desktop.contains("mango") || env::var("MANGO_INSTANCE_SIGNATURE").is_ok();
    let is_hyprland = desktop.contains("hyprland") || env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();
    let is_sway = desktop.contains("sway") || env::var("SWAYSOCK").is_ok();

    if is_mango {
        let mode = if enable { "1" } else { "0" };
        let _ = Command::new("mmsg").args(["dispatch", "fullscreen", mode]).output();
    } else if is_hyprland {
        let mode = if enable { "1" } else { "0" };
        let _ = Command::new("hyprctl").args(["dispatch", "fullscreen", mode]).output();
    } else if is_sway {
        let mode = if enable { "enable" } else { "disable" };
        let _ = Command::new("swaymsg").args(["fullscreen", mode]).output();
    } else if desktop.contains("niri") {
        let _ = Command::new("niri").args(["msg", "action", "fullscreen-window"]).output();
    } else {
        let action = if enable { "add,fullscreen" } else { "remove,fullscreen" };
        let _ = Command::new("wmctrl").args(["-r", ":ACTIVE:", "-b", action]).output();
    }
}
