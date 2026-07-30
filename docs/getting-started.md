# Getting Started

This guide covers installing TUI Game Station, walking through the initial setup wizard, and configuring your artwork scraper.

---

## Installation

### Prerequisites
Before building or running TUI Game Station, ensure you have the required build tools and libraries installed:

```bash
# Debian / Ubuntu / Linux Mint
sudo apt update
sudo apt install build-essential pkg-config libssl-dev libsqlite3-dev wl-clipboard xclip

# Fedora / RHEL
sudo dnf install gcc pkg-config openssl-devel sqlite-devel wl-clipboard xclip

# Arch Linux / Manjaro
sudo pacman -S base-devel pkgconf openssl sqlite wl-clipboard xclip
```

### Compiling from Source
Clone the repository and compile the release binary using Cargo:

```bash
cd ~/Proyectos/tui_game_station
cargo build --release --bin tui
```

The compiled binary will be placed at `target/release/tui`. You can copy it to your local binary path for system-wide terminal access:

```bash
cp target/release/tui ~/.local/bin/tui-game-station
```

---

## First Run and Welcome Setup Wizard

When launching TUI Game Station for the first time, the application automatically displays the full-screen **Welcome & Setup Wizard**.

### Wizard Tour Steps

1. **Step 1: Introduction**
   - Displays a overview of TUI Game Station features, multi-platform console support, and rendering engine capabilities.
   - Press `Right Arrow` or `Enter` to proceed.

2. **Step 2: Navigation & Key Features**
   - Highlights key shortcuts such as Big Picture Mode (`Alt+O`), Live Search (`/`), Visual Media Selector (`w`), Runner Manager (`c`), and Controls Cheatsheet (`?`).

3. **Step 3: Artwork Scraper Configuration (Optional)**
   - Allows you to configure a SteamGridDB API key for automatic HD artwork downloading.
   - You can copy your API key from a web browser and press `Ctrl+V` to paste it directly into the input field.
   - Use `Left Arrow` or `Right Arrow` to move the editing cursor, or `Backspace` and `Delete` to modify text.
   - Free API keys can be generated at: `https://www.steamgriddb.com/profile/preferences/api`

4. **Step 4: Initial Setup Complete**
   - Finalizes the initial configuration. Select `[ GET STARTED ]` or press `Enter` to complete the wizard and enter the main dashboard.

---

## Re-running the Welcome Wizard

You can re-launch the Welcome Setup Wizard at any time from within the application:

1. Press `s` or `Alt+S` to open the **Application Settings** modal.
2. Select `[ Re-run Welcome & Setup Wizard ]` and press `Enter`.
