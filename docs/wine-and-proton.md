# Wine and Proton Compatibility Guide

TUI Game Station provides native integration for executing Windows applications (`.exe`) on Linux using Wine, Valve Proton, GE-Proton, Lutris runners, and Bottling presets.

---

## Wine & Proton Runner Manager (`c`)

Press `c` from the dashboard to open the **Wine & Proton Runner Manager**.

### Management Capabilities
- **Detect Installed Runners**: Scans system paths (`/usr/bin/wine`), Steam Proton directories (`~/.steam/root/compatibilitytools.d/`, `~/.local/share/Steam/steamapps/common/`), Lutris runners (`~/.local/share/lutris/runners/wine/`), and Bottles presets (`~/.local/share/bottles/runners/`).
- **ProtonGE Downloader**: Fetch and install the latest GE-Proton (GloriousEggroll) releases directly from GitHub.
- **Wine Prefix Utilities**:
  - `winecfg`: Launch Wine configuration GUI for the active game prefix.
  - `winetricks`: Open Winetricks to install DirectX DLLs, Visual C++ runtimes (`vcredist`), fonts, or PhysX.
  - `Kill Wine Processes`: Execute `wineserver -k` to terminate hung background Wine/Proton processes cleanly.

---

## Configuring Emulator & Runner Options

When configuring an emulator runner:

1. Use `Up` / `Down` or `Tab` / `Shift+Tab` to navigate through the interactive fields and action buttons:
   - **`1. Executable / AppImage Path`**: Type the executable path directly or select `[ Browse File ]` to open the file browser.
   - **`[ Download AppImage ]`**: Download the latest official runner AppImage online (when available).
   - **`[ SAVE RUNNER ]`**: Save configuration and activate the runner.
   - **`[ Delete from Disk ]`**: Remove downloaded runner binary from disk.
   - **`[ Deactivate ]`**: Deactivate the runner entry.
2. Press `Enter` on any highlighted field or button to perform that action instantly.

---

## Configuring Windows Games

When creating or editing a Wine game entry (`Add Game` / `Edit Game`):

### 1. Isolated Wine Prefixes (`wine_prefix`)
Each Wine application can run in its own dedicated, isolated 64-bit Wine prefix directory:
- **Default Location**: `~/.local/share/tui-game-station/prefixes/<game_id>`
- **Custom Prefix**: Specify any custom directory path (e.g., `~/Games/prefixes/cyberpunk`).

### 2. Selecting Runners
Assign specific Wine or Proton binaries per game:
- **System Wine**: Standard system `/usr/bin/wine`.
- **Proton Experimental / Proton 9.0 / Proton 8.0**: Official Valve Steam Proton compatibility tool.
- **GE-Proton**: Community-enhanced Proton build with media codecs and game fixes.
- **Lutris / Bottles Runners**: Custom Wine builds optimized for specific game engines.

### 3. Custom Launch Arguments (`custom_command`)
Pass command-line flags directly to the Windows `.exe`:
- Example 1: `-dx11 -no-launcher`
- Example 2: `+exec autoexec.cfg -fullscreen`

### 4. Environment Variables
Add runtime environment variables per game entry:
- `DXVK_HUD=fps,gpuname` (Display DXVK performance overlay)
- `MVK_ALLOW_METAL_BOUNDS=1`
- `VKD3D_CONFIG=dxr11` (Enable Ray Tracing)

### 5. GameMode Integration (`gamemode-run`)
Enable GameMode to automatically request Feral GameMode CPU/GPU performance optimization when the game launches.
- Wrap execution command with `gamemode-run`.
