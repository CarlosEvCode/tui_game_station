# Feature Gaps & Stub Audit (verified against code)

Companion to `docs/audit_shortcuts.md`. This document lists features that are **advertised or half-implemented in the UI but not backed by working code**, plus data that exists in the DB schema but is never populated. All claims were verified by reading the source on 2026-08-08.

- Source of truth (Actions / state): `tui/src/app.rs`.
- Source of truth (Rendering / footers): `tui/src/ui.rs`.
- Source of truth (Keyboard dispatch): `tui/src/main.rs` event loop (~172-1627).
- Source of truth (DB schema / writes): `core/src/db.rs`.

---

## 1. Big Picture — Game Detail view

### 1.1 Buttons other than "Play" are placeholders

`DETAIL_ACTIONS = ["Play", "Favorite", "Options", "Delete"]` (`ui.rs:22`). In the detail view only index 0 works:

- Keyboard `Enter` → if `detail_action_idx == 0` launches the game, otherwise shows toast *"This action will be available soon."* (`main.rs:1383-1391`).
- Gamepad `Confirm` → same logic (`app.rs:2779-2787`).

`Favorite`, `Options` and `Delete` are rendered but do nothing. In particular there is **no** "toggle favorite" (even though `favorite` column exists in the DB, see §2) and **no** way to delete a game from the detail view.

### 1.2 Metadata shown is hardcoded placeholder text

`render_game_detail_view` (`ui.rs:1706-1770`) draws YEAR / DEVELOPER / PUBLISHER / DESCRIPTION from **hardcoded strings** (`"YEAR"`, `"DEVELOPER"`, `"PUBLISHER"`, and `"No description available."`), not from the game record. There is no metadata editor anywhere in the app.

---

## 2. Game metadata & play statistics exist in the schema but are never populated

The `games` table defines (`db.rs` ~90-120):

```sql
release_year, developer, publisher, description, genre, rating,
favorite, play_count, play_time_seconds, last_played_at
```

Every code path that creates a game writes `None / 0 / false` for all of these (`app.rs:8420-8430`, `app.rs:9796`, `app.rs:9985`, `db.rs:1847-1856`). Nothing ever writes them afterwards:

- `favorite` is never toggled (see §1.1 — the Favorite action is a stub).
- `play_count` / `play_time_seconds` / `last_played_at` are never written: `check_game_exit` (`app.rs:2554`) only sets a status message and does not record play time.

Consequences: the detail view can't show real metadata, there is no "favorites" feature, and there is no play-time/play-count tracking.

---

## 3. Media (covers/banners/icons)

### 3.1 Everything is SteamGridDB-only; no "use my own image" flow

- `record_media_status` hardcodes `source = 'steamgriddb'` in its INSERT (`db.rs:1208-1225`).
- The `VisualMediaSelector` modal (`ui.rs:2973`) has 4 tabs (Candidates / Covers / Banners / Icons) and only searches SteamGridDB (`app.rs:7782-7826`). There is no option to pick a local file or drag/drop an image.
- Actual display resolves images **by filename convention only**: `media_dir/{covers,banners,icons}/{game_id}.{jpg|png|webp}` (`app.rs:2213-2219`, `get_media_dir` in `scraper/src/steamgriddb.rs:64-73`). So the "chosen" media from the selector is materialized as a file and matched back by ID — a user-owned image copied into the folder with the right name would work, but there is no UI to do that.

### 3.2 The Edit Game modal has no media section

`EditGameForm` (`ui.rs:4880`) for emulator/windows games only exposes Title, ROM Path, Working Dir, Emulator, Core, Custom Args and the env-flag checkboxes. There is **no** section to set/change the game's cover, banner or icon (contrast with the `VisualMediaSelector` which is only reachable from the main view via `w`). So the "edit media of this game" flow the user expects from the Edit Game dialog does not exist.

### 3.3 Deleting a game orphans its media files on disk

`ConfirmDeleteGameExecution` (`app.rs:7485-7510`) only calls `db.delete_games(...)`. The cover/banner/icon files in the media directory are **not** removed.

---

## 4. Actions that are dead stubs

| Action | Definition | Dispatched? | Effect |
| :--- | :--- | :--- | :--- |
| `OpenFuzzySearchModal` | implemented (`app.rs:3558`) | **never** | The Fuzzy Search modal is unreachable — no key/button dispatches it. |
| `ToggleSelectFolder` | `app.rs:7360` → `{}` | gamepad `Y` in ScanFolderForm (`app.rs:3124`) | No-op; the folder multi-select in the scan form is never toggled. |
| `SwitchScanFolderPane` | `app.rs:6994` → `{}` | `Tab` (`main.rs:233`) + gamepad Next/PrevTab (`app.rs:3142,3153`) | No-op; there is no second pane to switch to. |
| `FetchProtonReleases` | `app.rs:4527` → `{}` | **never** | No-op. |

---

## 5. AppSettings modal — inconsistent controls

The modal (`ui.rs:2501`) has 5 fields: 0 API Key, 1 Re-run Welcome Wizard, 2 About, 3 Check Updates, 4 Save.

| Input | field 1 | field 2 | field 3 | field 4 |
| :--- | :--- | :--- | :--- | :--- |
| Keyboard `Enter` (`main.rs:~796`) | `OpenWelcomeWizardModal` | `OpenAboutModal` | `CheckForUpdates` | `SaveAppSettings` |
| Gamepad `Confirm` (`app.rs:3031-3055`) | `ResetRunnerConfig` (**no-op here**, it only matches `ManageRunnersStep2Config`, `app.rs:4034-4066`) | `About` | `CheckForUpdates` | closes modal **without saving** |
| Mouse click (`mouse_handler.rs`) | `OpenWelcomeWizardModal` | `SaveAppSettings` (hit zone ≥5 maps to field 2) | — | — |

Gaps:

- Gamepad cannot save settings: field 4 just sets `modal_state = None` instead of dispatching `SaveAppSettings`.
- Gamepad field 1 triggers `ResetRunnerConfig`, which does nothing while the AppSettings modal is open.
- Mouse hit-zones only cover fields 1-2 and mislabel "field 2" (actually opens the save handler).

---

## 6. Minor gaps

- **Windows Games Manager**: `handle_windows_games_enter` (`app.rs:1413-1422`) only handles "Add new" (index == len) and "Edit" (any row). There is **no** way to remove a game from the Windows list from this modal.
- **Delete runner only removes the exe**: `ConfirmDeleteRunnerExecution` / `DeleteRunnerDownload` (`app.rs:4094-4135`) deletes the executable file and resets the runner row, but leaves the runner's directory (prefix, config, etc.) on disk.
- **`SaveApiKey` duplicates `SaveAppSettings`** (`app.rs:7673-7687` vs `7766-7777`): both just write `steamgriddb_api_key`; the modal can reach both paths via different bindings.
