# Media and Artwork Scraper Guide

TUI Game Station integrates with **SteamGridDB** to automatically download, render, and manage high-definition artwork for your game collection.

---

## SteamGridDB API Key Setup

1. Obtain a free API key at: `https://www.steamgriddb.com/profile/preferences/api`
2. In TUI Game Station, press `s` or `Alt+S` to open **Application Settings**.
3. Paste your API key into the `SteamGridDB API Key` field using `Ctrl+V` and save.

---

## Visual Media Selector (`w`)

Highlight any game in your library and press `w` to open the **Visual Media Selector**.

```text
+-----------------------------------------------------------------------+
|  VISUAL MEDIA SELECTOR                                                |
|  [ 1. Grid Covers ]  [ 2. Hero Banners ]  [ 3. Logos ]  [ 4. Icons ]   |
+-----------------------------------------------------------------------+
| Search Candidate Query: [ Persona 5 Royal                           ] |
| Candidates Found:                                                     |
| > Candidate #1 - 600x900 (Official Steam Cover)                       |
|   Candidate #2 - 600x900 (Custom Alt Art by ScraperUser)              |
|   Candidate #3 - 920x430 (Horizontal Grid Banner)                     |
+-----------------------------------------------------------------------+
| Artwork Preview Panel | Downloader Progress Bar                       |
+-----------------------------------------------------------------------+
| [Enter] Apply Artwork  | [1-4] Switch Tab  | [Esc] Close             |
+-----------------------------------------------------------------------+
```

### Media Types Supported

1. **Grid Covers (Tab 1)**: Vertical (600x900) or Horizontal (920x430) grid cover images displayed in the main dashboard details pane and Big Picture 3D stage.
2. **Hero Banners (Tab 2)**: Ultrawide background banners (1920x620) rendered behind Big Picture mode.
3. **Clear Logos (Tab 3)**: Transparent PNG game title logos.
4. **Icons (Tab 4)**: Square game icons.

### Managing Candidates
- Use `Up` / `Down` arrow keys to browse artwork candidate results returned by SteamGridDB.
- Press `Enter` to download and set the selected artwork candidate for the active game.
- Downloaded assets are cached locally under `~/.local/share/tui-game-station/media/`.
