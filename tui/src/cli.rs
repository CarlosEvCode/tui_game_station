use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "tui-game-station",
    author = "Carlos Magallanes (CarlosEvCode)",
    version,
    about = "TUI Game Station - Sleek Terminal Gaming Hub & Emulator Manager",
    long_about = "A high-performance Terminal User Interface (TUI) game launcher and manager supporting Wine, Proton, Steam, and console emulators.\n\nInstall or Update:\n  curl -fsSL https://raw.githubusercontent.com/CarlosEvCode/tui_game_station/main/install.sh | sh"
)]
pub struct CliArgs {
    /// Launch directly into Big Picture mode (Console CoverFlow Carousel)
    #[arg(short = 'b', long = "big-picture", default_value_t = false)]
    pub big_picture: bool,

    /// Filter initial platform by name (e.g. "SNES", "PlayStation", "Wine", "Steam")
    #[arg(short = 'p', long = "platform")]
    pub platform: Option<String>,

    /// Check and install the latest release update from GitHub
    #[arg(short = 'u', long = "update", default_value_t = false)]
    pub update: bool,

    /// Uninstall TUI Game Station from the system
    #[arg(long = "uninstall", default_value_t = false)]
    pub uninstall: bool,

    /// Purge configuration and database when uninstalling
    #[arg(short = 'P', long = "purge", default_value_t = false)]
    pub purge: bool,
}
