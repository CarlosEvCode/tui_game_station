# TUI Game Station Documentation

Welcome to the official documentation for **TUI Game Station**, a high-performance, terminal-based gaming dashboard and launcher for Linux. TUI Game Station brings together retro emulator ROMs, native Linux executables, Windows applications (via Wine and Proton), and Steam titles into a single hardware-accelerated interface.

---

## Documentation Index

1. [Getting Started](getting-started.md)
   - Installation requirements and build steps.
   - First-run experience and the Welcome Wizard.
   - Setting up SteamGridDB API key.

2. [User Guide](user-guide.md)
   - Interface layout and pane architecture.
   - Normal Mode vs Big Picture Mode (`Alt+O` / `-b`).
   - Cross-platform Live Search (`/`).

3. [Game Management](game-management.md)
   - Adding games manually (Emulators, Native, Wine/Proton, Steam).
   - Editing and deleting game entries.
   - Automatic ROM folder scanner (`f`).

4. [Wine and Proton Compatibility](wine-and-proton.md)
   - Running Windows applications (`.exe`) on Linux.
   - Managing Wine prefixes and Proton runners (`c`).
   - Using `winecfg`, `winetricks`, and process termination.
   - Custom launch arguments, environment variables, and GameMode.

5. [Media Scraper](media-scraper.md)
   - SteamGridDB API integration.
   - Visual Media Selector (`w`).
   - Managing covers, hero banners, logos, and icons.

6. [Shortcuts and Controls Reference](shortcuts.md)
   - Complete keyboard and mouse shortcuts cheat sheet across all contexts.

---

## System Requirements

- **Operating System**: Linux (X11 or Wayland display server).
- **Terminal Emulator**: Modern terminal supporting 24-bit TrueColor and Unicode glyphs (e.g., Alacritty, Kitty, Foot, WezTerm, Konsole, GNOME Terminal).
- **Clipboard Utility (Optional)**: `wl-paste` (Wayland), `xclip` or `xsel` (X11) for pasting API keys.
- **Dependencies**: SQLite3 runtime (embedded), OpenSSL libraries.
