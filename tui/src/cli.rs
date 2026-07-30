use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "tui-game-station",
    author = "DeepMind Pair Team",
    version = "0.1.0",
    about = "TUI Game Station - Sleek Terminal Gaming Hub & Emulator Manager",
    long_about = "A high-performance Terminal User Interface (TUI) game launcher and manager supporting Wine, Proton, Steam, and console emulators with native high-res cover art."
)]
pub struct CliArgs {
    /// Launch directly into Big Picture mode (Console CoverFlow Carousel)
    #[arg(short = 'b', long = "big-picture", default_value_t = false)]
    pub big_picture: bool,

    /// Filter initial platform by name (e.g. "SNES", "PlayStation", "Wine", "Steam")
    #[arg(short = 'p', long = "platform")]
    pub platform: Option<String>,
}
