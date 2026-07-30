# User Guide

TUI Game Station features a dual-mode interface designed for both keyboard-focused desktop navigation and controller/media-center use.

---

## Interface Layout (Normal Mode)

Normal Mode is organized into three main interactive panes and status bars:

```text
+-----------------------------------------------------------------------+
|  TOP HEADER BAR: Search Bar [/] | Console Badges & Game Counts        |
+-------------------+---------------------------------------------------+
| PLATFORM SELECTOR | GAME LIST PANE                                    |
| [3DS] [DS] [PS1]  | > Persona 5 Royal                   [Wine]        |
| [PS2] [PSP] [Wii] |   Super Mario Odyssey               [Switch]      |
| [Switch] [Native] |   The Legend of Zelda: OOT          [N64]         |
| [Wine] [Steam]    |   God of War II                     [PS2]         |
+-------------------+---------------------------------------------------+
| DETAILS PANEL     | Cover Preview, Title, Platform, Path, Args,       |
| (Right Side)      | Wine Prefix, Last Played, Play Count              |
+-----------------------------------------------------------------------+
| ACTIVITY BAR      | Log Messages | Download Sliders | Toast Alerts     |
+-----------------------------------------------------------------------+
| FOOTER BAR        | Key Shortcuts Reference Cheatsheet                |
+-----------------------------------------------------------------------+
```

### 1. Top Header Bar
- **Interactive Live Search Bar (`/`)**: Type any string to instantly filter games across all platforms. Press `Esc` to clear search filters.
- **Platform Badges**: Displays total game count per console platform (e.g., `[3DS: 12]`, `[PS2: 8]`, `[Wine: 5]`).

### 2. Platform Selector (Left Sidebar)
- Lists all registered gaming platforms (Retro Emulators, Native Linux, Wine/Proton, Steam).
- Use `Up` / `Down` arrow keys to switch platforms.
- Numerical shortcuts `1` through `9` select platforms directly by index.

### 3. Game List Pane (Center)
- Lists games available under the currently selected platform.
- Displays game title and platform badge tags (e.g., `[3DS]`, `[DS]`, `[PS1]`, `[PSP]`).
- Use `Up` / `Down` arrow keys or `j` / `k` to move selection.
- Press `Enter` to launch the selected game.

### 4. Details Panel (Right Side)
- Shows high-definition rendered cover artwork (using Katatui / Ratatui terminal image rendering).
- Displays full game title, platform, executable/ROM path, runner configuration, custom launch arguments, play count, and last played timestamp.

### 5. Activity Bar & Controls Footer
- Shows background operations, toast notifications, download progress sliders, and primary action shortcuts.

---

## Big Picture Mode

Big Picture Mode (`--big-picture` / `-b` flag or `Alt+O` shortcut) is an immersive, hardware-accelerated 3D Cover Flow layout optimized for large monitors and gamepad use.

### Features
- **3D Cover Flow Stage**: Displays game covers in a carousel stage with centered highlight, depth scaling, and smooth animations.
- **Backdrop Hero Banners**: Automatically displays high-resolution hero artwork behind the game stage.
- **Platform Selector Panel**: Quick platform switcher panel located on the right side.
- **Top Header Bar**: Search bar and platform filter status.

### Big Picture Navigation
- `Left Arrow` / `Right Arrow` or `h` / `l`: Scroll through games in the 3D cover stage.
- `Up Arrow` / `Down Arrow` or `k` / `j`: Navigate platform sidebar.
- `Enter`: Launch highlighted game.
- `Alt+O` or `Esc`: Exit Big Picture Mode and return to Normal Mode.

---

## Cross-Platform Live Search

1. Press `/` in any mode to focus the top search bar.
2. Start typing the game title. The application automatically filters the game list across all platforms in real-time.
3. The platform selector sidebar automatically highlights platforms matching search results.
4. Press `Esc` to clear the search filter and restore full library view.
