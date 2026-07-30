# TUI Game Station

TUI Game Station is a terminal-based gaming launcher and dashboard for Linux systems. Built in Rust using Ratatui, it brings together retro emulator ROMs, native Linux binaries, Windows games (via Wine and Proton), and Steam titles into a single interface.

---

## Key Features

- **Multi-Platform Support**: Unified dashboard for retro emulators (3DS, DS, PS1, PS2, PSP, GameCube, Wii, Switch, SNES, GBA), Native Linux executables, Wine/Proton Windows applications, and Steam games.
- **Big Picture Mode (`Alt+O`)**: Hardware-accelerated 3D Cover Flow carousel interface optimized for large displays and gamepad navigation.
- **SteamGridDB Integration**: Automatic downloading and visual selection (`w`) of high-definition covers, hero banners, clear logos, and icons.
- **Wine & Proton Manager (`c`)**: Advanced runner detector, custom Wine prefixes, built-in ProtonGE downloader, `winecfg`, `winetricks`, process killer, and Feral GameMode integration.
- **Live Search (`/`)**: Real-time cross-platform library filtering with instant results.
- **Welcome Setup Wizard**: Full-screen initial configuration tour with clipboard support (`Ctrl+V`) for API key setup.
- **Secure API Key Storage**: Masked API key display in settings with interactive reveal and edit mode.

---

## Requirements

- **Operating System**: Linux (X11 or Wayland).
- **Terminal Emulator**: TrueColor (24-bit) unicode terminal emulator (e.g., Alacritty, Kitty, Foot, WezTerm, Konsole, GNOME Terminal).
- **Dependencies**: OpenSSL, SQLite3, build-essential / base-devel.
- **Clipboard Utility (Optional)**: `wl-clipboard` (Wayland), `xclip` or `xsel` (X11).

---

## Quick Installation

### Compiling from Source

```bash
# Clone the repository
git clone https://github.com/user/tui_game_station.git
cd tui_game_station

# Build release binary
cargo build --release --bin tui

# Install binary to local PATH
install -m 755 target/release/tui ~/.local/bin/tui-game-station
```

Run the application:

```bash
tui-game-station
```

---

## Documentation

Comprehensive documentation for all aspects of TUI Game Station is available in the [`docs/`](docs/) directory:

- [Getting Started](docs/getting-started.md): Installation, setup wizard, and API key setup.
- [User Guide](docs/user-guide.md): Interface layout, navigation, and Big Picture Mode.
- [Game Management](docs/game-management.md): Manual game creation, editing, deleting, and ROM scanner.
- [Wine and Proton Compatibility](docs/wine-and-proton.md): Wine/Proton runner setup, custom prefixes, launch args, and GameMode.
- [Media Scraper](docs/media-scraper.md): Artwork scraping with SteamGridDB and Visual Media Selector (`w`).
- [Shortcuts Reference](docs/shortcuts.md): Complete table of keyboard and mouse controls.

---

## License

This project is licensed under the MIT License.
