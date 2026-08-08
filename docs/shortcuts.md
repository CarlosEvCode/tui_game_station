# Controls and Shortcuts Reference

Complete reference of all keyboard shortcuts, mouse interactions, and gamepad controls in TUI Game Station, organized by operational context.

---

## Global Shortcuts (Always Active)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `?` | Show Cheatsheet | Open keyboard & mouse controls cheatsheet modal. |
| `/` | Live Search | Focus top search bar. |
| `Alt+O` | Toggle Big Picture | Switch between Normal TUI and Big Picture Mode. |
| `s` | Settings | Open Application Settings modal. |
| `Esc` | Clear / Back | Clear active search query, or close active modal / exit Big Picture. |
| `q` | Quit Application | Exit TUI Game Station cleanly. |
| `Ctrl+V` | Paste Clipboard | Paste text from system clipboard (in text input fields). |
| `Home` / `End` | Start / End Cursor | Jump text editing cursor to start or end (in text input fields). |

---

## Dashboard Navigation (Normal Mode)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `↑` / `↓` | Move Selection | Move up or down in current active pane (Platforms or Games). At top of pane, `↑` moves focus to Search bar. |
| `←` / `→` | Switch Pane / Cycle Emulator | Switch focus between Platforms and Games pane. On Platforms pane, cycles platform's active emulator/core. |
| `Tab` | Cycle Pane Focus | Cycle focus between Search bar, Platforms pane, and Games pane. |
| `Enter` | Launch Game | Launch highlighted game entry. |
| `Space` | Toggle Selection | Toggle select/unselect highlighted game entry. |
| `Delete` | Delete Game | Open Confirm Delete Game modal for current selection. |
| `a` | Add Game | Open Add Game Type Selector modal (Folder Scan / Native / Wine / Steam). |
| `e` | Edit / Folders | On Platforms focus → open Folder Manager for platform; on Games focus → open Edit Game form. |
| `c` | Wine Tools | Open Wine & Proton Tools menu (winecfg, winetricks, kill wine, open prefix). |
| `m` | Manage Emulators | Open Manage Runners (Emulators & Cores) modal. |
| `w` | Visual Media | Open Visual Media Selector (Covers, Heroes, Logos, Icons). |
| `v` | Cycle View | Cycle view mode (Cover Cards / Banner Cards / Table). |
| `p` / `P` | Toggle All Platforms | Toggle showing all platforms vs only active platforms. |
| `r` | Rescan Platform | Quick rescan current platform folder. |
| `g` | Fetch Artwork | Fetch artwork for selected game from SteamGridDB. |
| `f` | Force Close | Force close currently running game process immediately. |

---

## Big Picture Mode (`Alt+O`)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `←` / `→` | Navigate / Select | Scroll carousel (when focused on Carousel) or switch platform (when focused on Platform Bar). |
| `↑` / `↓` | Switch Focus Bar | Move focus up to Platform Bar (`↑`) or down to Carousel (`↓`). |
| `Tab` | Cycle Focus | Cycle focus between Platform Bar, Search bar, and Carousel. |
| `Shift+Tab` | Previous Platform | Switch to previous platform directly. |
| `p` / `P` | Platform Selector | Open Platform Selector popup dialog. |
| `Enter` | Open Detail View | Open Game Detail view for focused carousel game. |
| `Alt+O` / `Esc` | Exit Big Picture | Return to Normal Dashboard Mode (when search query is empty). |

### Big Picture Game Detail View

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `←` / `→` | Switch Action | Select action button (Play, Options, etc.). |
| `Enter` | Execute Action | Execute focused action (first action = Play game). |
| `Esc` | Close Detail | Return to Big Picture carousel. |

---

## Interactive Forms & Modals

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `↑` / `↓` | Move Field / Row | Move focus vertically between form fields, list items, or action buttons. |
| `←` / `→` | Cycle / Move Cursor | Cycle option values (e.g. emulators/cores), move text cursor, or switch confirm choices (NO/YES). |
| `Enter` | Confirm / Submit | Activate button, submit form, open file picker, or confirm selection. |
| `Space` | Toggle Checkbox | Toggle checkbox settings (e.g. GameMode, recursive scanning, DAT matching). |
| `Tab` / `Shift+Tab` | Next / Prev Field | Move focus to next or previous field. |
| `Ctrl+V` | Paste Clipboard | Paste text from system clipboard into input fields. |
| `Home` / `End` | Start / End Cursor | Jump cursor to start or end of text input string. |
| `Backspace` | Erase Previous | Delete character before text cursor. |
| `Delete` | Erase Next / Remove | Delete character at cursor, or remove focused folder/runner in management modals. |

---

## Visual Media Selector (`w`)

| Key Shortcut | Action | Description |
| :--- | :--- | :--- |
| `1` | Grid Covers Tab | View 600x900 and 920x430 grid cover candidates. |
| `2` | Hero Banners Tab | View 1920x620 hero banner candidates. |
| `3` | Logos Tab | View clear PNG logo candidates. |
| `4` | Icons Tab | View icon candidates. |
| `Tab` | Switch Tab | Cycle to next media category tab. |
| `↑` / `↓` | Select Candidate | Move selection through SteamGridDB candidates list. |
| `Enter` | Apply Artwork | Download and set selected candidate for current game. |
| `Esc` | Close | Exit Visual Media Selector. |

---

## Mouse Controls

- **Left Click**: Select platform in sidebar, pick game in list (double-click launches game), select form fields and action buttons, or switch visual media tabs.
- **Scroll Wheel**: Scroll game list, candidate lists, or platform lists up and down.

---

## Gamepad Controls

- **D-pad / Left Stick**: Move selection in lists, carousels, and forms.
- **`Ⓐ` (Cross)**: Launch game (Normal mode), open Detail view (Big Picture), confirm action in modals.
- **`Ⓑ` (Circle)**: Return to Platforms pane, exit Big Picture, close active modal.
- **`ⓧ` (Square)**: Cycle view mode (Normal mode), open Platform Selector (Big Picture).
- **`Ⓨ` (Triangle)**: Toggle game selection (Normal mode), toggle folder select (ScanFolderForm).
- **`LB` / `RB`**: Switch platform (Normal & Big Picture), switch tabs in Visual Media Selector / Scan Folder.
- **`Select`**: Toggle Big Picture mode.
- **`Start`**: Open Application Settings modal.
- **`R3`**: Delete focused game / folder.

