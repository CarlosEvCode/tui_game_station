# Shortcuts & Controls Audit (verified against code)

This document records the controls, shortcuts and navigation that are **actually implemented**, verified by reading the source (not by the in-UI help text, which is partly aspirational). A discrepancy table at the end compares this with `docs/shortcuts.md`, the cheatsheet modal and the main.rs help text.

- Source of truth (Keyboard): `tui/src/main.rs` event loop (lines ~172-1627).
- Source of truth (Actions): `tui/src/app.rs` (`Action` dispatch + `handle_gamepad_action`, lines ~2776-3168).
- Source of truth (UI footers/help): `tui/src/ui.rs`.
- Source of truth (Mouse): `tui/src/mouse_handler.rs`.
- Source of truth (Gamepad): `tui/src/gamepad.rs` + gamepad footers in `ui.rs`.

Date of audit: 2026-08-08.

---

## 1. Global shortcuts (any view)

| Key | Action | Notes |
| :--- | :--- | :--- |
| `?` | Open keyboard & mouse cheatsheet modal | `main.rs:1469` |
| `/` | Focus the search bar | `main.rs:1458` |
| `Alt+O` | Toggle Big Picture / Normal mode | `main.rs:1507` (also from search focus, `main.rs:1445`) |
| `q` | Quit application | `main.rs:1472` |
| `Esc` | Clear search query (if non-empty) | `main.rs:1465`; with empty query in main view it does nothing |
| `Ctrl+V` | Paste clipboard (only in text inputs) | `main.rs:1251` |
| `Home` / `End` | Jump text cursor to start/end (only in text inputs) | `main.rs:624`, `main.rs:642` |

---

## 2. Main view — Normal mode (no search focus)

Navigation keys from `main.rs:1456-1616`.

| Key | Action |
| :--- | :--- |
| `↑` / `↓` | Move selection in active pane (Platforms or Games). At the top of a pane, `↑` moves focus to the Search bar. |
| `←` / `→` | Switch focus between Games and Platforms panes (does **not** cycle emulator on keyboard). |
| `Tab` | Cycle focus: Search → Platforms → Games (via `TogglePane`). |
| `Enter` | Launch the focused game (`LaunchGame`). |
| `Space` | Toggle select the focused game. |
| `Delete` | Open the Confirm Delete modal for the selection. |
| `a` | Open "Add Games" Step 1 (import method: Folder Scan / Native / Wine / Steam). |
| `e` | On Platforms focus → open Folder Manager for the platform; on Games focus → open Edit Game form. |
| `c` | Open the Wine Tools menu (winecfg / winetricks / kill wine / open prefix). |
| `m` | Open the Manage Runners (Emulators) modal. |
| `w` | Open the Visual Media Selector (covers, heroes, logos, icons). |
| `s` | Open Settings modal. |
| `v` | Cycle view mode (Cards / Banner / Table). |
| `p` / `P` | Toggle "Show All Platforms". |
| `r` | Quick rescan of the current platform folder. |
| `g` | Fetch artwork for the selected game (SteamGridDB). |
| `f` | Force-close the running game (no confirm dialog — direct action + toast). |

### Search bar focused (`/`)

| Key | Action |
| :--- | :--- |
| Any character | Append to query and filter live. |
| `Backspace` | Remove last character. |
| `Enter` / `↓` | Commit (exit search focus to Platforms). |
| `Esc` | Clear query and exit search focus. |
| `Tab` | Cycle focus. |
| `Alt+O` | Toggle Big Picture. |

---

## 3. Big Picture mode

| Key | Action |
| :--- | :--- |
| `↑` | Move focus to the platform bar. |
| `↓` | Move focus to the carousel. |
| `←` / `→` | Platform bar focused → previous/next platform; carousel focused → previous/next game. |
| `Tab` | Cycle focus (platform bar / search / carousel). |
| `Shift+Tab` | Previous platform. |
| `Enter` | Open the Game **Detail** view (not a direct launch). |
| `p` / `P` | Open the Platform Selector popup. |
| `Alt+O` | Return to Normal mode. |

### Big Picture — Game Detail view (`main.rs:1380-1405`)

| Key | Action |
| :--- | :--- |
| `Enter` | Execute focused action (first action = Launch; others show "coming soon"). |
| `←` / `→` | Switch detail action. |
| `Esc` | Close detail view. |

### Normal-mode Game Detail view (footer `ui.rs:1973-2048`)

| Key | Action |
| :--- | :--- |
| `Enter` | Play the game. |
| `Esc` | Back to library. |
| `←` / `→` | Switch detail action. |

---

## 4. Modal keyboard handling — general patterns (`main.rs:180-1380`)

| Key | General behavior |
| :--- | :--- |
| `Esc` | Close modal (special cases: AppSettings exits API-key editing first; ProtonDownloader steps back; WelcomeWizard finishes/skips). |
| `Tab` / `Shift+Tab` | Next / previous field (VisualMediaSelector: next/prev tab; WelcomeWizard: cycle step). |
| `↑` / `↓` | Move selection / navigate rows / navigate fields (per modal). |
| `←` / `→` | Cycle values, move text cursor, or select confirm option (per modal). |
| `Enter` | Confirm / activate / save (per modal). |
| `Space` | Toggle checkbox in forms; selects in list modals. |
| `Backspace` / `Delete` | Text editing; `Delete` also deletes focused folder (ScanFolderForm) / runner (ManageWineRunners). |
| `1`-`4` | Select tab directly in the Visual Media Selector. |
| `Ctrl+V` | Paste into text fields (WelcomeWizard, AppSettings, ConfigureApiKeyInput). |
| `Home` / `End` | Cursor jump in text fields (WelcomeWizard step 2, AppSettings API key). |

Context-specific letter shortcuts inside modals:

| Key | Context | Action |
| :--- | :--- | :--- |
| `p` | Add/Edit Game (Wine) field 4 | Open Wine/Proton runner picker |
| `p` | Add/Edit Game (Wine) field 5 | Open custom args editor |
| `p` / `w` | ManageRunnersStep1Platform | Open Wine runner manager |
| `t` | ProtonDownloader | Select next version |
| `d` | ManageWineRunners | Open Proton downloader |
| `r` | ScanFolderForm | Force rescan focused folder |
| `u` | About | Check for updates |

---

## 5. Modal-by-modal reference

| Modal | Up/Down | Left/Right | Enter | Other |
| :--- | :--- | :--- | :--- | :--- |
| **Add Games Step 1** (type selector) | Select method | — | Next | Esc cancel |
| **Scan ROMs Step 2** (platform) | Select platform | — | Configure scan form | Esc back |
| **Add Folder Scan** form | Fields | Cycle emulator/core | Add & scan | Space toggles Recursive + DAT |
| **Folder Manager** (ScanFolderForm) | Move focus | Change emulator/core value | Confirm / scan | Tab switch pane, Del delete, R rescan, Esc |
| **Manage Runners Step 1** (platform) | Select | — | Next | `p`/`w` wine manager, Esc |
| **Manage Runners Step 2** (Emulator Options) | Rows | Cycle values / action button | Execute action | Esc back |
| **Manage Wine Runners** | Select runner | — | Open Proton downloader | `d` downloader, Del delete runner |
| **Proton Downloader** | Select version | — | Download | `t`/`→` next, `←`/Esc back |
| **Download Core** | Select core | — | Download & install | Esc |
| **Detected Executables** | Select | — | Use executable | Esc |
| **Wine/Proton Runner picker** | Select runner | — | Apply to game | Esc |
| **Wine Tools** | Select tool | — | Execute tool | Esc |
| **Custom Args input** | — | Move cursor | Save | Backspace, Esc |
| **Confirm Delete (Game/Folder/Runner)** | — | Select NO/YES | Confirm | Esc cancel |
| **Platform Selector** | Select | — | Confirm | Esc |
| **App Settings** | Fields (5) | — | Reveal/edit/save | Tab cycle, Esc close |
| **Welcome Wizard** (4 steps) | — | Switch slide | Next / finish | Tab cycle, Esc skip, text editing in step 2 |
| **Visual Media Selector** | Select candidate | Switch tab | Apply | `1`-`4` tabs, Tab switch, Esc |
| **Fuzzy Search** | — (type to filter) | Cursor | Filter | Backspace, Esc clear/close |
| **About** | — | — | — | `u` check updates, Esc close |
| **Update Available** | — | — | Update now | Esc dismiss |
| **Windows Games Manager** | Select | — | Edit / add game | Esc |

---

## 6. Mouse controls (`mouse_handler.rs`)

| Gesture | Context | Action |
| :--- | :--- | :--- |
| Scroll up/down | Modals | Previous/next item |
| Scroll up/down | Big Picture | Previous/next platform (bar) or game (carousel) |
| Scroll up/down | Library | Previous/next platform (left pane) or game (right pane) |
| Left click | Library header | Focus search bar |
| Left click | Library left 25% | Select platform |
| Left click | Library right area | Select game |
| Left click | Big Picture top banner | Search (left 30%) / platform bar (prev-next arrows by third) |
| Left click | Big Picture carousel | Left/right third = prev/next game; center = refresh cover |
| Left click | Welcome Wizard | Footer thirds: prev/next slide; step 3 body = finish |
| Left click | App Settings | Select API key / wizard / save fields |
| Left click | Platform Selector | Select row (click same row = confirm; outside = close) |
| Left click | Manage Runners Step 2 | Select rows / action buttons (browse, download, save, open, delete) |

Note: double-click launch is **not** implemented (the cheatsheet advertises "DblClick").

---

## 7. Gamepad (`gamepad.rs`, `app.rs:2776-3168`)

Buttons: D-pad / left stick = navigation (repeat: 250 ms initial, then 100 ms). Rumble/axis notes in `gamepad.rs`.

| Button | Normal mode | Big Picture | Detail view | In modal |
| :--- | :--- | :--- | :--- | :--- |
| `A` / Cross | Games pane → Launch; Platforms pane → move to Games | Open Game Detail | Play / action | Confirm (per modal) |
| `B` / Circle | Games pane → move to Platforms | Back to Normal mode | Back | Close modal |
| `X` / Square | Cycle view mode | Open Platform Selector | — | — |
| `Y` / Triangle | Toggle select game | — | — | ScanFolderForm: toggle select folder |
| `LB` / `RB` | Prev / Next platform | Prev / Next platform | — | VisualMedia: switch tab; ScanFolder: switch pane |
| `Select` / Back | Toggle Big Picture | — | — | — |
| `Start` / Menu | Open Settings | — | — | — |
| `R3` | Delete (confirm modal) | — | — | ScanFolderForm: delete folder |

Footer legend (main view, `ui.rs:1046-1117`): `[Ⓐ] Launch [Ⓑ] Back [ⓧ] View [Ⓨ] Select [LB/RB] Tab [Select] BigPic [Start] Settings [R3] Delete`.
Footer legend (Big Picture, `ui.rs:1602-1692`): `[Ⓐ] Details [Ⓑ] Normal Mode [ⓧ] Consoles [LB/RB] Console`.
Footer legend (Detail, `ui.rs:1973-2048`): `[Ⓐ] Play [Ⓑ] Back [◀ ▶] Action`.

---

## 8. Discrepancies between documentation and actual behavior

### 8.1 `docs/shortcuts.md` vs actual

| Claimed in shortcuts.md | Actual |
| :--- | :--- |
| `k`/`j`, `h`/`l` (vim keys) | **Not implemented.** Only arrow keys. |
| `1`-`9` select platform | **Not implemented** in main view (`1`-`4` only set tabs inside Visual Media Selector). |
| `Insert` adds game | **Not implemented.** Use `a`. |
| `f` = Scan Folder | **Wrong.** `f` force-closes the running game. Folder scan is reached via `a` → "Folder Scan". |
| `c` = Runner Manager | **Wrong.** `c` opens the Wine Tools menu; Runner Manager is `m`. |
| `r` = Refresh library | **Partial.** `r` only rescans the current platform folder. |
| `PageUp`/`PageDown`, `Home`/`End` list jumps | **Not implemented** in the main view. |
| `Esc` exits Big Picture | **Not implemented.** Only `Alt+O` (Esc in Big Picture only clears an active search). |
| `Alt+S` opens settings | **Not implemented.** Only `s`. |
| Big Picture `↑`/`↓` navigate platforms | **Inaccurate.** `↓` focuses the carousel; `↑` focuses the platform bar; platform switching is `←`/`→`. |
| `Enter` in Big Picture launches | **Wrong.** `Enter` opens the Detail view; launching requires a second `Enter`. |

### 8.2 main.rs help-text docstring vs actual

The help block at `main.rs:220-560` is aspirational:

| Claimed | Actual |
| :--- | :--- |
| `D` deletes selected game | **Not bound.** Delete key opens the confirm modal. |
| `C` copies game | **Not implemented.** |
| `B` toggles Big Picture | **Not implemented.** `Alt+O`. |
| `L` launch alias | **Not implemented.** |
| `O` opens Folder Manager | **Not implemented.** Use `e` while on Platforms focus. |
| `←`/`→` cycle emulator/core | Keyboard only moves pane focus; emulator cycling is gamepad-only. |
| Y/N confirmations | Confirm dialogs use `←/→` + `Enter` (NO/YES), **no Y/N keys**. |
| About modal has tabs | About is a single static modal (`u` checks updates). |
| `L`/`W`/`U`/`K`/`T` wine keys | These are items inside the Wine Tools menu, not global keys. |
| `[Esc] Quit?` | Esc never quits; it backs out / clears search. |

### 8.3 Cheatsheet modal (`ui.rs:5513-5618`) vs actual

| Claimed | Actual |
| :--- | :--- |
| `Enter / DblClick` launch | Double-click is **not** implemented (mouse handler has no double-click). |
| `Tab / p` "Switch Consoles" | `Tab` cycles pane focus; `p` toggles "Show All Platforms" (normal) / platform selector (Big Picture). Approximation, not "switch consoles". |

The rest of the cheatsheet (search `/`, `Esc` clear, `Space` select, `Del` delete, `F` force-close, `g` cover fetch, `r` rescan, `m` emulators, `c` wine tools, `s` settings, `v` view, `Alt+O` big picture) matches actual behavior.

### 8.4 Footer labels vs actual

The persistent footers (`ui.rs:1045-1206`, `1602-1703`, `1973-2048`) match actual key handling in all audited cases.
