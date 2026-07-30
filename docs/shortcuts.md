# Controls and Shortcuts Reference

Complete reference of all keyboard shortcuts and mouse interactions in TUI Game Station, organized by operational context.

---

## Global Shortcuts (Always Active)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `?` | Show Cheatsheet | Open keyboard & mouse controls cheatsheet modal. |
| `/` | Live Search | Focus top cross-platform search bar. |
| `Alt+O` | Toggle Big Picture | Switch between Normal TUI and Big Picture 3D Mode. |
| `s` / `Alt+S` | Settings | Open Application Settings modal. |
| `Esc` | Cancel / Close | Cancel current operation, clear search, or close active modal. |
| `q` | Quit Application | Exit TUI Game Station cleanly. |

---

## Dashboard Navigation (Normal Mode)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `↑` / `↓` or `k` / `j` | Move Selection | Move up or down in the current active pane. |
| `←` / `→` or `h` / `l` | Switch Platform | Switch selected platform in sidebar. |
| `Tab` / `Shift+Tab` | Cycle Pane | Cycle focus between Sidebar, Game List, and Details Pane. |
| `1` - `9` | Select Platform | Select platform directly by index. |
| `Enter` | Launch Game | Execute highlighted game entry. |
| `Insert` / `a` | Add Game | Open Add Game Type Selector modal. |
| `e` | Edit Game | Open Edit Game Form for highlighted game. |
| `Delete` | Delete Game | Open Confirm Delete Game modal for highlighted entry. |
| `f` | Scan Folder | Open Folder Scanner Wizard. |
| `w` | Visual Media | Open Visual Media Selector (Covers, Heroes, Logos, Icons). |
| `c` | Runner Manager | Open Wine & Proton Runner Manager modal. |
| `r` | Refresh Library | Reload platforms and game lists from SQLite database. |
| `PageUp` / `PageDown` | Scroll Page | Scroll game list by full page up or down. |
| `Home` / `End` | Jump List | Jump to first or last entry in current game list. |

---

## Big Picture Mode (`Alt+O`)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `←` / `→` or `h` / `l` | Scroll Carousel | Scroll through 3D cover stage carousel. |
| `↑` / `↓` or `k` / `j` | Select Platform | Navigate platform sidebar list on the right. |
| `Enter` | Launch Game | Launch highlighted game. |
| `Alt+O` / `Esc` | Exit Big Picture | Return to Normal Dashboard Mode. |

---

## Interactive Forms & Modals (Add/Edit Game, Scan Folder, Emulator Options)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `↑` / `↓` | Switch Row / Field | Move focus vertically between form rows or action button bar. |
| `←` / `→` | Select Action / Cursor | Navigate action buttons inside button bar or move text editing cursor. |
| `Enter` | Activate / Reveal | Select file/folder picker, submit form, activate focused button, or reveal API Key. |
| `Space` | Toggle Checkbox | Toggle checkbox settings (e.g. GameMode, recursive scanning). |
| `Ctrl+V` | Paste Clipboard | Paste text from system clipboard (`wl-paste`/`xclip`/`xsel`). |
| `Home` / `End` | Start / End Cursor | Jump cursor to start or end of text string. |
| `Backspace` | Erase Previous | Delete character before cursor. |
| `Delete` | Erase Next | Delete character at cursor position. |

---

## Visual Media Selector (`w`)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `1` | Grid Covers Tab | View 600x900 and 920x430 grid cover candidates. |
| `2` | Hero Banners Tab | View 1920x620 hero banner candidates. |
| `3` | Logos Tab | View clear PNG logo candidates. |
| `4` | Icons Tab | View icon candidates. |
| `↑` / `↓` | Select Candidate | Move selection through SteamGridDB candidates list. |
| `Enter` | Apply Artwork | Download and set selected candidate for current game. |
| `Esc` | Close | Exit Visual Media Selector. |

---

## Mouse Controls

- **Left Click**: Select platform in sidebar, pick game in list, select form fields and action buttons (`[ Browse File ]`, `[ SAVE RUNNER ]`, `[ Download AppImage ]`), or switch visual media tabs.
- **Scroll Wheel**: Scroll game list, candidate lists, or platform lists up and down.
