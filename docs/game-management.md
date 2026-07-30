# Game Library Management

This guide covers adding, editing, and removing games, as well as using the automatic ROM folder scanner.

---

## Adding Games Manually

To manually register a game entry, press `Insert` or `a` to open the **Platform Type Selector**.

### 1. Retro Emulator ROMs
- **Platform**: Select target console (e.g., Nintendo 3DS, Nintendo DS, PlayStation 1, PlayStation 2, PSP, GameCube, Wii, Nintendo Switch, SNES, GBA).
- **Title**: Enter display title.
- **ROM File Path**: Path to the ROM file (`.iso`, `.cue`, `.3ds`, `.nds`, `.chd`, `.nsp`, `.xci`, etc.). Select the path field and press `Enter` to open the system file picker.
- **Custom Launch Command (Optional)**: Override default emulator launch command if needed.

### 2. Native Linux Executables
- **Title**: Game title.
- **Executable Path**: Path to binary or script (`.sh`, `.x86_64`, AppImage, or binary). Press `Enter` on the path field to open the file picker.
- **Working Directory**: Working directory for executable execution (defaults to binary parent folder).
- **Custom Arguments (Optional)**: Command line options passed directly to binary.

### 3. Wine / Proton Windows Applications
- **Title**: Windows game or application title.
- **Executable Path**: Path to `.exe` file. Press `Enter` to open the system file picker.
- **Wine Prefix**: Path to isolated prefix directory. Defaults to `~/.local/share/tui-game-station/prefixes/<game_id>`.
- **Runner**: Selected Wine or Proton version (System Wine, Proton Experimental, GE-Proton, Lutris, Bottling).
- **GameMode**: Enable/disable `gamemode-run` integration wrapper.
- **Custom Arguments**: Windows command line arguments (e.g., `-dx11`, `-fullscreen`).

### 4. Steam Titles
- **Title**: Steam game name.
- **Steam AppID**: Numeric Steam Application ID (e.g., `570` for Dota 2, `1091500` for Cyberpunk 2077).
- Executed automatically via `steam -applaunch <AppID>`.

---

## Editing and Deleting Games

### Editing an Entry
1. Highlight a game in the list.
2. Press `e` to open the **Edit Game Form**.
3. Use `Up` / `Down` or `Tab` / `Shift+Tab` to navigate between fields.
4. Modify fields (title, paths, arguments, Wine runner, GameMode setting).
5. Select `[ Save Game ]` or press `Enter` to confirm changes.

### Deleting an Entry
1. Highlight a game in the list.
2. Press `Delete` key.
3. Confirm deletion in the prompt dialog. (Note: Only the database entry and downloaded artwork cache are removed; your original game files or ROMs are never deleted).

---

## Automatic ROM Folder Scanner

The built-in folder scanner scans directories recursively for ROM files and registers them automatically.

### Running the Folder Scanner
1. Press `f` to open the **Folder Scanner Wizard**.
2. Select the target platform to register games under (e.g., `PS2`, `PSP`, `3DS`, `Switch`).
3. Focus `1. Folder Path` and press `Enter` to open the system directory picker.
4. Specify target file extensions (e.g., `iso, chd, bin, cue, nsp, xci`).
5. Focus `3. Scan Subfolders Recursively` and press `Space` or `Enter` to toggle.
6. Navigate to `[ START SCANNING ROMS ]` and press `Enter`. Games will be indexed into your database with clean title parsing.
