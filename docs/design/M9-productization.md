# M9 — Productization → 1.0

**Status:** started (17 November 2026), branch `m9-product` (on top of `m8b-cockpit`; the user's steps for M8b are waiting). Recommended model · effort: export architecture Opus 5.5 · high; docs, license file, first run Sonnet 5.5 · medium; release audit Opus 5.5 · high.

## 1. Goal and boundary (ROADMAP)

Content:
- single-package install (embedded toolchain),
- `wen export` (standalone game package),
- first-run flow,
- example projects,
- user docs (TR/EN),
- updater (web),
- optional crash report,
- third-party license file,
- end-user agreement,
- Steam page and website.

**Owned by the user:**
- Steam account and fee,
- domain name and hosting,
- code signing certificate,
- making the repository public,
- trials on clean Windows 10/11.

This note covers what can be done without waiting for these.

## 2. Decisions

### 2.1 Export (`project_export`, 31st tool; `wen export`; "Export game" in the editor)

- It is a **control plane tool** (invariant 4): the editor, the command line and the agent use the same path.
- **Check first:** if the project has errors, it is not exported (`export_has_errors`). If there is game code but it is not built, it is not exported (`export_needs_build`; hint: `script_build` first).
- **Output:** `exports/<name>/` inside the project. Tools write only inside the project (M4 rule). `exports/` is not counted as assets (`wish_asset::NON_ASSET_FOLDERS`). An existing one is overwritten only with `overwrite`.
- **Package:**
  - `<name>.exe`: a renamed copy of the player.
  - `game/`:
    - `wish.json`, `input.json`;
    - scenes and prefabs;
    - assets (every file in the asset index; except `scripts/`, `tests/`, `replays/`, `exports/`);
    - `.wish/build/game.wasm`.
  - `LICENSES.txt`: the engine's MIT and Apache-2.0 texts, the third-party list.
- **Not in the package:** the editor, the MCP server, the compiler toolchain, the game's Rust source (ADR-023).

### 2.2 Player (`wish_player`)

- Library + `wish_player` binary. It runs the game with the control plane's play session (`sim_start` / `play_step`); the same content as `wen play`, but build and hot reload are off.
- The window title is the project's name. Sound: the speaker on Windows.
- `--headless --frames N`: runs N ticks without a window and writes the world hash as JSON. The tests use this to prove that the exported game plays bit for bit the same as the project.
- Place of the player: `wish_player(.exe)` next to `wen`; `WISH_PLAYER` or `Workspace::set_player_path` points to another one.
- `wen play` uses `wish_player::Game` with build and hot reload on: one game loop.

## 3. Work packages

| WP | Work | Model · effort |
|---|---|---|
| WP1 | Player + `project_export` + `wen export` + editor menu; headless hash equality test | Opus 5.5 · high |
| WP2 | Third-party license file (generator script + freshness test) | Sonnet 5.5 · medium |
| WP3 | First-run flow (connect the AI or skip → template → first request or by hand) | Sonnet 5.5 · high |
| WP4 | User docs TR/EN, contributing guide | Sonnet 5.5 · medium |
| WP5 | Packaging (`wen package` + player), clean machine trial (user), release audit | Opus 5.5 · high + user |

## 4. Implementation notes

### WP1 + WP2 — export, player, licenses (17 November 2026; Opus 5.5 · high)

- **`wish_player`** (library + binary):
  - `Game` (window content; moved from `wen play`), `play`, `play_headless`, `load_models` (`wen capture` uses it too).
  - The binary (`wish_player.exe`): plays the `game/` folder or `--game <folder>`.
  - `--headless --frames N` writes a JSON: the tick count and the world hash.
  - Errors are written to standard error and to `player-error.txt`. In release no console window opens.
  - `wen play` now calls `wish_player::play` (`develop: true`: build and hot reload on). One game loop.
- **`project_export`** (31st tool), `wen export`, "Export game" in the editor:
  - Order: first unbuilt code (`export_needs_build`; otherwise the game's types look like "unknown component"), then `check` (`export_has_errors`), then the player (`player_missing`).
  - Package: `<name>.exe` and `game/` under `exports/<name>/` (documents, assets, settings, `.wish/build/game.wasm`); `LICENSES.txt`. `dry_run` lists what will be copied.
  - `exports/` counts neither as an asset nor as a document (`wish_asset::EXPORTS_FOLDER`).
- **`THIRD_PARTY_LICENSES.md`** (in the repository root, produced by `tools/third_party.py`):
  - The 451 libraries in the lock file and the 211 different license texts they distribute, each text once (~635 KB).
  - It is embedded with `wish_control::licenses::THIRD_PARTY`.
  - `tests/third_party.rs` checks that every library in `Cargo.lock` is in the file (run the script when a dependency changes).
- `wen package` puts the player in the package too; if it is missing, it warns (`package_without_player`).
- **Proof** — `wish_player/tests/export.rs`:
  1. The showcase game is copied.
  2. Export without a build is rejected.
  3. The code is built and the game is exported.
  4. The program in the package runs headless as a separate process.
  5. After 300 and 900 ticks the world hash is **bit for bit the same** as the project's own.

  Also: a bad project and a bad name are rejected; the export does not enter the asset list.
- **Owned by the user:** trying the package on a clean Windows (it runs on a machine without the engine installed, exit criterion 2).

### WP3 — first-run flow (17 November 2026; Opus 5.5 · high)

- **When `wen edit` opens without a project, the welcome screen** (`wish_editor::welcome::Launcher`). Project folder: `--root`, otherwise the `wish.json` in the current folder.
  - winit allows one event loop per process. So the welcome screen and the editor are in the same window: when a project is opened, the launcher switches to the editor.
- **The screen:**
  1. Who helps:
     - Claude Code (its path is shown if found),
     - or nobody (by hand).

     Another MCP agent can be connected later with "Connect an outside agent".
  2. What to start with: an empty project or templates. The showcase game is first and the default.
  3. Name and folder. Default: `Documents/Wish Projects/<name>` (on Turkish Windows the folder is `Belgeler`). The name is suggested by language and an unused name is chosen.

  On the right: recent projects and opening with a folder path. Language choice is at the top right.
- **Create:**
  - `scaffold::create_project` or the new **`scaffold::create_from_template`**.
  - What is not copied from the template:
    - what the engine produces: `.wish/`, `exports/`, `target/`;
    - files specific to a project: `wish.json`, `.mcp.json`, `CLAUDE.md`, `.gitignore`.

    These files are rewritten for the new project.
  - Then the game code is built (`script_build`): the template scenes use the game's own types.
  - The editor opens. If a helper was chosen, the Agent panel comes open and the found Claude Code is used.
- **Preferences** (`editor.json`): language, the last 8 projects, the helper choice. Each writer changes only its own key (`wish_editor::prefs`). The editor puts the project it opens at the top of the recent projects.
- **Place of templates:**
  1. `WISH_TEMPLATES`,
  2. `templates/` next to the program,
  3. the repository's `examples/` in a development build.

  `wen package` puts the examples into `templates/` (`scaffold::copy_template`). On the command line: `wen new <folder> --template islands`.
- **Finding Claude Code:** `wish_agent::claude::find`. In order:
  1. `WISH_CLAUDE`,
  2. `claude.exe`/`claude.cmd` in PATH (npm),
  3. `~/.local/bin`.

  It only looks, it does not start it. `ClaudeCode::default` uses this too.
- **No new capability:** creating a project is the same scaffold function as `wen new`, opening is `Workspace::open`. The parity table did not change. New diagnostic code: `invalid_template`.
- **Proof:**
  - `wish_editor/tests/welcome.rs`:
    - creating from a template with a helper: the game code is built, `check` is clean, the Agent panel is open, preferences were written;
    - opening from recent projects on the second launch;
    - an empty project by hand (the next name instead of a full folder);
    - explaining a folder that is not a project.
  - `wish_control/tests/templates.rs`: copied/not copied, valid project, `copy_template`.
  - `wish_cli/tests/parity.rs`: `wen new --template` and an unknown template.
  - `languages.rs`: 11 languages complete.

### WP4 — user docs and contributing guide (17 November 2026; Opus 5.5 · high)

- **Guides:** `docs/user/en/guide.md` and `docs/user/tr/guide.md`. The same ten sections:
  1. install and welcome screen,
  2. editor (layout, navigation, shortcuts, views),
  3. editing by hand,
  4. play / replay / test,
  5. game code,
  6. AI (cockpit, permission modes, activity, outside agent),
  7. export,
  8. command line,
  9. troubleshooting,
  10. project folder.
- **Button names were taken from the real interface:** the `t("...")` texts in the English source and `lang/tr.json`. When the interface changes, the guide must change too.
  - In the Turkish interface "entity" = "nesne", "asset" = "varlık"; the guide says so too.
- **`CONTRIBUTING.md`** (English, with a Turkish note): build and verification, the nine invariants, a table of rules enforced by tests, code conventions, working method.
- **`README.md`** updated: what it can do, getting started, building from source, docs (Turkish + English summary).
- **The user docs can also be reached from inside the engine:**
  - `wish://docs/guide` (for agents),
  - `wish://docs/sdk`,
  - `wish://docs/recipes`,
  - `docs_search`.

  The guide points to these and does not repeat them.

### Independent review (WP1–WP4; 17 November 2026)

An independent agent that read the code gave 12 findings; all were handled. Regression tests:
- `wish_control/tests/review_m9.rs`,
- `wish_editor/tests/parity_ops.rs::the_game_is_exported_from_the_menu`,
- `wish_editor/tests/welcome.rs`,
- `wish_player/tests/export.rs`.

| # | Finding | Fix |
|---|---|---|
| 1 (high) | Copies inside `exports/` were counted as the project's files in `check`, `fmt`, `asset_move` and the watcher: an old copy blocked re-export, and `wen mv` rewrote the exported scenes | `wish_asset::index::walk_files` never sees `exports/` at the root |
| 2 | Export deleted the old one first (a failure halfway takes the old one too); the editor always overwrote | It is built in a side folder (`exports/.<name>.exporting`) and swapped in when done. The editor asks first: *Replace the previous export?* (Replace / Cancel) |
| 3 | If `exports` was a link, it could write and delete outside the project | A link (a junction on Windows too) is rejected, and the canonical path must be inside the project: `export_outside_project` |
| 4 | The player's C runtime DLL was not checked | `wish_control::pe::needs_c_runtime_dll`. `project_export` warns (`export_needs_c_runtime`), `wen package` checks the player too |
| 5 | `create_from_template` and `create_project` overwrote existing files | If one of the files to be written exists, nothing is written: `project_files_exist` |
| 6 | The welcome screen counted code that could not be built as success (it looked at `ok`) | It looks at `passed` and shows the compiler's first error |
| 7 | The exported code might not be the code the project runs | The fingerprint of `game.wasm` on disk must equal the loaded module's, and the sources must not be newer than it; otherwise `export_needs_build`. The package writes the verified bytes |
| 8 | A create that stopped halfway left a half folder | It is built in a side folder (`.<name>.creating`) and moved when done; on an error it is deleted and the suggested name moves on |
| 9 | Windows device names (`con`, `nul`, `com1`...) were accepted | `scaffold::reserved_on_windows`: an export name is rejected, automatic names get `_game` / `-game` added |
| 10 | An unreadable `editor.json` was counted as empty and overwritten | It is copied aside as `.json.broken`; writing is a side file + rename |
| 11 | The hidden folders of a template (`.git`, `.claude`...) passed to the new project | Hidden entries are not copied |
| 12 | If the Documents folder was moved to OneDrive, a wrong place was suggested | On Windows the shell folder is read from the registry (`User Shell Folders\Personal`, variables expanded). On this machine `OneDrive\Belgeler` (OneDrive\Documents) |

## 5. Release audit (WP5, 17 November 2026)

The [VISION.md](../VISION.md) success criteria and the M9 exit criteria. Proof and status for each. Symbols:
- ✔ proven,
- ◐ code ready, waiting for a user step,
- ✗ not done.

| Criterion | Proof | Status |
|---|---|---|
| Scene change → observation < 2 s | When the file changes the world updates in < 1 s (`wen watch` polls every 100 ms, M2). The editor polls the scene every 0.4 s and the project every 2 s | ✔ |
| Game code change → running game < 10 s | `wen build` 0.4 s after an edit; hot reload keeps the state (M5 §18) | ✔ |
| Zero hash drift in a 10,000-tick replay | `script_tools.rs::a_ten_thousand_tick_replay_…` (M5); replay with physics (M6) | ✔ |
| Agent evaluation suite ≥ 90% (Sonnet 5.5 · medium) | 41/41 (100%), `evals/results/m7-sonnet-5-5-medium.json` | ✔ |
| Playable mini game from zero, in one agent session | Evaluation tasks 11–12 and 26–41 (writing game code, checked with a hidden scenario test); the showcase game was built only with the tools (M7) | ✔ |
| Complete use without AI | Editor: building a scene, playing, testing, export. All done by hand under `wish_editor/tests`; the "Nobody" option on the welcome screen | ✔ |
| Manual/agent parity (checked automatically) | `wish_editor::parity::PARITY` + `tests/parity.rs`: every tool and every `edit_apply` operation is tied to a test that does it by hand | ✔ |
| First run: download → connect the AI → first request → working scene < 5 min | Welcome screen → template → build → editor ~10 s (`welcome.rs` test). With **real Claude Code**, first request ("put a red cube") → cube in the right place + `check` clean: **25.7 s** (`wish_editor/tests/cockpit_real.rs`, run by hand). Repeat on a clean machine is with the user | ✔ |
| Install: one package, no compiler install | `wen package`: `wen` + player + SDK + packaged compiler + templates; builds in an empty environment (M5). The clean Windows Sandbox trial is with the user | ◐ |
| Zero agent-caused crash/freeze | Hostile game modules (`wish_script/tests/hostile_game.rs`), fuzzing (`wish_control/tests/fuzz.rs`), the fuel limit; the M4–M9 closing reviews | ✔ |
| Undoability: every agent change with one click | Undo per activity card; selective undo (`edit_history revert`); checkpoints (M8b) | ✔ |
| Export: one command/click, standalone package | `project_export` / `wen export` / editor menu. The exported showcase game in a separate process is bit for bit the same as the project (`wish_player/tests/export.rs`) | ✔ |
| The exported game runs on a machine without the engine | The package contains no engine file (test). The clean machine trial is with the user | ◐ |
| < 5 min on clean Windows 10 and 11, no extra install | A user step (Windows Sandbox: `clean-test.wsb`) | ◐ |
| Zero known crashes, zero open high-priority bugs | The findings of every milestone's closing review were closed (M9: 12/12). The relative path bug found by the package trial was fixed. No open bug record | ✔ |
| Optional crash report | Local only: `wish_control::crash` (an uncaught panic → a report file; nothing is sent); the editor says so once | ✔ |
| Repository public, contributing guide written | `CONTRIBUTING.md`; the repository was made public by the user's decision on 17 November 2026 (before that the history was scanned: no personal data or keys). CI runs again | ✔ |
| Name, brand and domain suitability | A first web research: `docs/release/name-research.md` (no engine with the same name; "WISH" is another company's mark in software classes). Expert research and the domain name are with the user | ◐ |
| End-user agreement, Steam page, website | Drafts: `docs/release/` (agreement TR/EN, Steam texts TR/EN, one-page site). Legal review, account, domain name and release are with the user | ◐ |
| Updater (web) | It needs a download address (hosting, domain name); when the address is known, a "new version available" check is added. Until then the guide and the agreement say "it does not download by itself" | ✗ |

**Steps owned by the user (before 1.0), in the suggested order:**
1. ~~Rebuild the package~~: done (below in "Package and end-to-end trial").
2. **Direct trial with the package** (user decision: on their own machine instead of a Windows Sandbox trial). `clean-test.wsb` in Windows Sandbox. Also opening an exported game without the engine.
3. ~~Cockpit run with real Claude Code~~: done (below).
4. **User test with 3 people** (M8b criterion 1) and the T1 mock-ups.
5. **Listening to the sound output by ear** (`wen play`).
6. **CI billing.** GitHub does not start the jobs.
7. **Name/brand/domain, end-user agreement, making the repository public, Steam, website, code signing.**

### WP5 — package, end-to-end trial, crash report, drafts (17 November 2026)

- **Package:** `C:\wish-build\package` (558 MB), `wen` and `wish_player` with the static C runtime. No warnings. Contents: SDK, packaged compiler, 5 templates, `clean-test.wsb`.
  - To rebuild it, two steps:
    1. `RUSTFLAGS=-C target-feature=+crt-static cargo build --release -p wish_cli -p wish_player --target x86_64-pc-windows-msvc --target-dir C:/wish-build/target-static`
    2. With that `wen.exe`, `package --out C:/wish-build/package`.
- **End-to-end trial** (empty environment with `env -i`: only `system32` in PATH, no development tools):
  - `wen new --template hop` → `wen build` → `wen test` (2/2) → `wen export`.
  - The exported `hop_copy.exe --headless --frames 300` runs in its own folder.
  - The world hash is the same as the project's (`1670b0d3…`).
- **Bug found by the trial (there since M5):** with a relative project path like `wen build --root game`, the build silently failed.
  - Cause: the compiler runs in the project folder, but the output path given to it was relative; the module was written under `game/game/.wish/...`.
  - Fix: `build_game_within` makes the root absolute. The message is no longer empty ("exited with ... and wrote no module").
  - Test: `wish_script/tests/build_game.rs::a_project_given_by_a_relative_path_builds`.
- **Crash report** (`wish_control::crash`):
  - The panic hook notes the last panic (message, place, backtrace).
  - `guard` writes a report only for a panic that escapes from `main`. A caught panic (a hostile model file, a failing tool) leaves no report.
  - Places:
    - `wen`: `<config>/crashes/crash-<time>-wen.txt`;
    - player: next to the game (the player may not know the engine; they send the file to whoever made the game).
  - Nothing is sent.
  - The editor tells about a new report once (`crash_notice`, `crash_seen` in the preferences).
  - The config folder is in one place: `wish_control::crash::config_folder` (the editor preferences use it too).
  - Tests: `wish_control/tests/crash.rs`, `welcome.rs::a_crash_report_is_told_once`.
- **Drafts** (`docs/release/`, not published): end-user agreement, Steam texts, one-page site.

### WP5 review (17 November 2026)

The independent review gave 7 findings; all were fixed. Tests:
- `wish_control/tests/crash.rs`,
- `wish_script/tests/build_game.rs`.

| # | Finding | Fix |
|---|---|---|
| 1 (medium) | If the player was in a folder that cannot be written (Program Files), the report and the error file silently disappeared; in the windowed build there is no console either | Tried in order: next to the program, `%LOCALAPPDATA%\<game>`, the temp folder. The windowed build opens the error in Notepad |
| 2 (medium) | Panics in side threads produced no report; a panic of the MCP input reader counted as a clean exit | The `crash::thread` wrapper (the Claude Code reader, the MCP HTTP accept and connection threads, the stdin reader). A reader panic is now an error |
| 3 | A report could carry another thread's last panic | Notes are kept per thread; only the note of the same thread with the same message is taken |
| 4 | The backtrace was symbolized on every caught panic (slow; risk of a double panic in the hook) | The backtrace is captured but resolved only when a report is written |
| 5 | The crash notice was translated before the language was set | After the language (and `--lang`) is set |
| 6 | Reports went to the roaming `%APPDATA%`; overwriting within the same second; unlimited growth | `%LOCALAPPDATA%\Wish\crashes`; the process number in the name; the newest 20 reports are kept |
| 7 | The relative path test would run wrong on a different drive | The test is skipped if the drives differ |

### Real Claude Code run (17 November 2026; with the user's approval "do what you can yourself")

- **Setup:** `wish_editor/tests/cockpit_real.rs` (`#[ignore]`; `cargo test -p wish_editor --test cockpit_real -- --ignored --nocapture`).
  - The editor opens, a request is typed into the Agent panel, Send is pressed.
  - Claude Code (npm `claude.cmd`, the person's account) works through the editor's MCP endpoint.
- **Result:** the request "a red cube 2 m in front of the camera, sitting on the ground, named crate" → a red cube at `[0, 0.5, 5]`. The agent's flow:
  1. It read the scene and the schema.
  2. It made an `edit_apply`.
  3. It checked its own work with `describe_view` and `capture_frame`.
  4. It ran `check`.

  The change is in the single history under the agent's name and in the activity feed; a checkpoint was taken before the task. Time was 27.3 s and 25.7 s (two runs).
- **Bug it found:** Claude Code repeats its last message as the result; it showed twice in the transcript. The cockpit now closes the same text with "Done." The `Scripted` fake agent now behaves like this too, so the regression test is in `cockpit.rs`.

### Interface refresh (17 November 2026; user request: "the start screen and the engine interface could be better", "icons vector and trouble-free", "we can grow the scope")

- **Theme** (`wish_editor::style`):
  - Our own dark palette; the brand color is purple `#8B6CFF`.
  - Rounded corners; tight spacing in panels, generous in cards.
  - Component titles as bands.
  - The error count is colored (error red, warning amber).
- **Vector icons and logo** (`wish_editor::vector`):
  - Each icon is an SVG path on a 24 grid (curves, round caps and joins).
  - Drawn with anti-aliasing by `vello_cpu`. It is the library and version egui draws text with; no new download.
  - It becomes a texture once at the exact pixel size it is shown, and is kept in the context.
  - Logo: a four-pointed sparkle with curved edges, a purple gradient and a light glow, with a small sparkle beside it.
  - The hierarchy shows the entity kind with an icon: camera, sun, light, model, interface, sound, sprite, particle, prefab.
- **Top bar:**
  - Groups: undo/redo · view · play/record (green play, red record) · move/rotate/scale/world/snap/camera · game view · agent.
  - In a narrow window a group moves to the next row without being split.
- **Welcome screen:**
  - A header with the logo; helper choice with large cards.
  - Templates as cards, with the **picture of the game's real first moment**. The pictures are `examples/*/.template/thumbnail.png`; they were made with the engine's own `capture_frame`. Because they are in a hidden folder they do not count as assets and do not pass to the new project; `wen package` carries them with the templates.
  - Name, folder and Create are in an always visible bottom action bar.
  - On the right, recent projects and a "Useful to know" card.
- **Inspector:** "Add component" is at the top; reachable without scrolling.
- **Scene + Game view** (user suggestion):
  - It is shown side by side when the middle area is at least 900 px wide.
  - The Game view looks from the scene's camera, with the game's interface. It is redrawn only when the scene version, the game tick or the size changes.
  - While the game runs, the Scene view stays on the editor camera; selection and handles are there.
  - The *Game view* button in the top bar opens or closes it; the preference is kept in `editor.json`.
  - Test: `wish_editor/tests/game_view.rs`.
- **Clipboard:** Ctrl+V/C/X with `arboard` (inside `wish_window`). This was found in real use, where a request could not be pasted into the welcome screen.
