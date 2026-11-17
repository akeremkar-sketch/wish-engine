# M10 — Hub, packages and moddability (plan)

**Status:** approved 17 November 2026 (user decision: M10a and M10b go into 1.0, M10c–e come after it); M10a in progress.
**Recommended model · effort:** M10a and M10c Opus 5.5 · xhigh (lasting interface decisions); M10b, M10d and M10e Opus 5.5 · high.
**Scope warning:** This plan grows the 1.0 scope. The recommendation is in §9: the base infrastructure (M10a) and the Hub (M10b) go into 1.0; plugins with code (M10c), game packages (M10d) and the ecosystem (M10e) come in 1.x releases.

## 1. The request

- AI mode should be a **base template**; people should be able to change it easily and make their own workspaces.
- There should be a **Hub** like Unity Hub and Epic Launcher: projects, engine versions, plugins, modes.
- The interface and many other things should be moddable and multi-purpose.
- The infrastructure for these should be built now, early.

## 2. Why now

When a plugin infrastructure is added later, the engine's own parts stay "special": plugins become second class, internal APIs leak outside, compatibility breaks. The real value of building early is that **the engine's own features also go through the same registries**: AI mode, themes, tools and templates become "first-party plugins". So:

- plugin authors use the same API the engine uses;
- the gaps of the API show while we build the engine;
- the engine becomes moddable without being changed.

Today many things are already data: languages (`lang/*.json`), templates (`examples/*`), recipes (`recipes.md`), the diagnostic catalog, the tool catalog. The plan gathers them under one **package** concept.

## 3. Fit with the invariants

| Invariant | What it means for plugins |
|---|---|
| 2 Determinism | Package versions are fixed by a lock file (`wish.lock`, with hashes); the replay carries the fingerprint of the packages. Code that enters the game is still deterministic Wasm. |
| 4 Single control plane | A plugin registers a new capability in `wish_control` **as a tool**; the interface, MCP and the command line see it from the same place. A plugin capability that lives only in the interface does not exist. |
| 5 Single diagnostic format | The errors of plugins also carry `code`, `message`, location and `hint`; the code prefix is the name of the package. |
| 7 Game code is untrusted | Plugin code runs in Wasm, in the same sandbox as game code (fuel, memory limit). File, network and project access happen only through **permissions** that are asked for in the manifest and approved by the person. There is no native plugin. |
| 9 Hybrid parity | Every tool a plugin adds can be used by the AI and by hand; the plugin's panel calls its tools. |
| License | Every package declares its SPDX license; the Hub compares the license with the allowed list (like `deny.toml`) and shows it. |

## 4. Package model

Everything is a **package**: a folder with a `wish-package.json` manifest (or its `.zip`).

```json
{
  "format": 1,
  "id": "com.example.narrative-workspace",
  "name": "Narrative Workspace",
  "version": "1.2.0",
  "engine": ">=1.0, <2.0",
  "license": "MIT",
  "kinds": ["workspace", "recipes", "theme"],
  "capabilities": ["project.read", "design.write"],
  "dependencies": { "com.example.dialogue-kit": "^0.3" },
  "contents": { "workspaces": ["workspaces/story.json"], "themes": ["themes/ink.json"] }
}
```

**Package kinds** (from harmless to powerful):

| Kind | What | Code? | Example |
|---|---|---|---|
| `theme` | Color tokens, fonts, icon set (SVG paths), corner and spacing sizes | No | "Night blue", high contrast |
| `language` | Interface translation | No | Japanese |
| `template` | New project template (today's `examples/*`) | No (game code is compiled) | "Top-down RPG" |
| `workspace` | The layout of AI mode: panels, game card fields, ready-made requests, ready-made decisions and questions, extra instructions for the agent | No | "Story game workspace" |
| `recipes` | Recipes and examples the agent reads (`wish://docs/...`) | No | "Dialogue system recipes" |
| `agent` | Definition of an agent adapter (command line, arguments, MCP configuration) | No (external program) | Another MCP client, a local model wrapper |
| `tool` | A new tool for the control plane (Wasm) | Yes | "Level checker" |
| `panel` | A panel for the editor (Wasm; returns a declarative interface tree) | Yes | Dialogue graph editor |
| `importer` / `exporter` | Asset importer, export target (Wasm) | Yes | `.aseprite` importer |
| `game` | Game package: components, systems, prefabs, assets | Yes (compiled with the game) | "Platformer kit" |

**Project level:**
- `wish.json` → `packages: { id: version range }` and the `engine` version.
- `wish.lock` → resolved versions and SHA-256s (goes into git).
- Packages are in the user cache (`%LOCALAPPDATA%\Wish\packages\<id>\<version>`); if wanted, they are copied into the project (`packages/`, offline and reproducible).

**Editor level:** themes, languages, workspaces and agent adapters are installed into the person's preferences.

## 5. Architecture: everything is a registry entry

Today there are fixed lists: `CATALOG`, `LANGUAGES`, the `style.rs` constants, `ai_mode::CARD`, `ready_requests()`. These become **registries**. The engine's own parts are written into the same registry too.

| Registry | Place | Today | Later |
|---|---|---|---|
| Tools | `wish_control` | `tools::CATALOG` (fixed) | `Registry<Tool>`: the engine's tools + the Wasm tools of packages (namespaced: `package.tool`) |
| Commands | `wish_editor` | Actions built into buttons | Commands with ids (`editor.play`, `ai.next_step`); a **command palette (Ctrl+K)**, changeable shortcuts; plugins add commands; the agent can call commands too |
| Panels | `wish_editor` | Fixed layout inside `app.rs` | Panel registry + layout document; AI mode becomes a `workspace` document |
| Themes | `wish_editor::style` | Constants | Tokens (`color.accent`, `radius.widget`...) + theme JSON; the default theme is a package too |
| Icons | `vector::parts` | Fixed paths | Icon registry; a theme can change the icon set |
| Languages | `lang/*.json` | Embedded | Embedded + package languages |
| Templates | `examples/*` | Folder | `template` package |
| Diagnostics | `docs::DIAGNOSTICS` | Fixed | The engine's + the packages' (namespaced) |
| Agent adapters | `wish_agent` | Claude Code, Scripted | `agent` packages; Claude Code is the first package |

**Workspace document:** AI mode itself is this file:

```json
{
  "format": 1,
  "name": "With AI (default)",
  "layout": {
    "left":   [{ "panel": "design.card" }, { "panel": "design.decisions" }],
    "center": [{ "panel": "view.game" }],
    "bottom": [{ "panel": "design.plan" }, { "panel": "design.notes" }, { "panel": "project.health" }],
    "right":  [{ "panel": "agent.conversation" }]
  },
  "card": [
    { "key": "title", "label": "Name" },
    { "key": "pitch", "label": "In one sentence", "lines": 2 },
    { "key": "classes", "label": "Character classes", "list": true }
  ],
  "ready": [{ "name": "Add a level", "request": "..." }],
  "agent": { "instructions": "...", "tools": ["*"], "model": null }
}
```

- The fields of the game card come from data. So there can be cards for specific genres: "classes" for an RPG, "characters" for a story game.
- The `game_design` tool reads the card fields from the workspace (schema first: the type and limits of the fields are there).
- The person can drag and change the layout in the editor, save it and share it as a package.

## 6. Hub (growth of the welcome screen)

Not a separate program, but the evolution of the welcome screen (the screen that comes when `wish-engine.exe` is opened). Tabs:

| Tab | Content |
|---|---|
| **Projects** | Recent projects, new project (templates = `template` packages), open, show in folder; the engine version of each project |
| **Learn** | Guide, example games, recipes, short missions ("make your first game in 10 minutes") |
| **Packages** | Installed packages; search, install, update, remove from the index; license, permissions, dependencies, release notes |
| **Appearance** | Themes, workspaces, languages, icon sets; instant preview |
| **Engine versions** | Versions installed side by side; a project opens with the version in its `wish.json`; update (signed) |
| **AI** | Agent adapters (Claude Code and others), account status, default model, spending summary |
| **Settings** | Folders, privacy (nothing is sent), crash reports |

**Package index (registry), serverless and free:**
- An open git repository (`wish-registry`), one JSON per package: versions, download address (GitHub Releases etc.), SHA-256, license, the publisher's public key.
- The Hub downloads the index and searches locally. It works offline. No commission, no payment (in line with the vision).
- Publishing = a PR to the index (`wen pkg publish` prepares it). The signature is ed25519; an unverified package is clearly marked.
- A person can add other indexes too (inside a company, community).

## 7. Plugins with code (M10c) and game packages (M10d)

**Editor plugins (Wasm):**
- `wish_ext` ABI v1: separate from the game ABI, versioned.
- A plugin registers: tools (with schema parameters; the `Param` table is the same), panels (each frame they return a declarative interface tree: title, text, button, field, list; the editor turns it into egui), importers and exporters, `check` rules, event hooks (before saving, after a task).
- A plugin cannot write files on its own: it calls control plane tools, and these calls enter the history with `actor: "ext:<package>"`.
- The sandbox is the same as `wish_script`'s (fuel, memory). Hostile package tests (manifest, ABI fuzzing) are required.

**Game packages:**
- Source package: it enters the same `rustc` build as the game code as `--extern` (an extension of ADR-025).
- It brings its own components, systems, prefabs and assets; their schemas are namespaced.
- Version resolution is semver; a lock file; the replay carries the package fingerprint.

## 8. Extra strategies for "making the engine better"

| Strategy | Why | When |
|---|---|---|
| **Command palette and command ids** | Everything by keyboard; moddable shortcuts; the agent can call commands too | M10a |
| **Theme tokens** | The interface changes without recompiling; accessibility (high contrast, large text) | M10a |
| **Dockable panels** | The person builds their own layout, as in Unity and Unreal; the workspace document can be edited by hand | M10a |
| **Engine version pinning + migration** | Old projects open safely with the new engine (`wen migrate` already exists) | M10a |
| **Git-friendly merging** | Thanks to canonical JSON, a 3-way merge driver for scene and design files (`wen merge`); teamwork without a server | M10b |
| **Signed updater** | Safe one-click update from the Hub | M10b |
| **Linux and macOS packages** | For the community and plugin authors | M10b–M10e |
| **`profile_get`** | Planned but not done; it also measures the cost of plugins | M10c |
| **Plugin development kit** | `wen ext new/test/pack/publish`, a template plugin, docs generated from the schema | M10c |
| **RFC process** | Lasting changes in the plugin API are discussed in public | M10e |

## 9. Work packages and order

**M10a — Base infrastructure (recommended for 1.0)**
1. Registries: tool, command, panel, theme, icon, language, template, diagnostic. The engine's parts move into the registry, behavior does not change.
2. Command palette (Ctrl+K) and changeable shortcuts.
3. Theme tokens and theme JSON; the default theme is a package.
4. Workspace document: AI mode and manual mode become two default workspaces; game card fields and ready-made requests come from data; the layout is saved.
5. Package manifest, `wish.lock`, local package folder; data packages (theme, language, template, workspace, recipe).
6. The `wish.json` engine version field.
7. Tests: for each registry a "the engine's part also goes through the registry" test; hostile manifests; parity.

**M10b — Hub (recommended for 1.0)**
1. Welcome screen → Hub tabs (Projects, Learn, Packages, Appearance, AI, Settings).
2. Package index format and `wen pkg search/add/remove/update/lock`.
3. Install, remove, update; license and permission display; signature verification.
4. Engine versions: side-by-side install, opening the project with the right version.
5. Example packages: a theme, a workspace ("Story game"), a recipe package.

**M10c — Plugins with code (1.x)**
1. The `wish_ext` ABI and sandbox; permissions.
2. Tool, panel, importer and hook kinds.
3. The `wen ext` development kit; the example "Level checker".

**M10d — Game packages (1.x)**
1. Compiling source packages with the game, namespaced schemas.
2. The example "Platformer kit".

**M10e — Ecosystem (1.x)**
1. Publishing flow, signing keys, the index repository.
2. Docs site, RFC process.

## 10. Decisions owned by the user

- Scope: should M10a and M10b go into 1.0 (recommendation: yes)?
- The name and owner (GitHub account) of the package index repository; later, together with the name and brand decision.
- Which will be the first official packages (recommendation: dark and light themes, the "Story game" workspace, "Platformer" recipes).

## 11. Risks

- **Scope:** every work package is the size of a milestone. The order and the 1.0 boundary must be kept.
- **Freezing the API early:** ABI v1 comes out marked "experimental"; the version field exists from day one.
- **Security:** plugin code is treated as untrusted as game code; no file or network access without permission; hostile tests are part of the closing review.
- **Determinism:** packages are tracked in the replay and in the hash; playback without a lock file gives a warning.

## 12. M10a in detail (decisions before code)

**Work packages, in order** (each one is built, tested and committed on its own):

| WP | Content | Why in this order |
|---|---|---|
| WP1 | Package core in the control plane: manifest, validation, versions and version requirements, install / remove / list / check / lock, `wish.lock`, the `packages` tool (33rd) | Everything else is a kind of package |
| WP2 | Theme tokens and theme packages; the default theme is a package | The first content kind; it changes the look without rebuilding |
| WP3 | Workspace documents: the AI mode's layout, card fields, ready requests and agent instructions from data; workspace packages | The user's main wish: the AI mode as a template |
| WP4 | Command registry, command palette (Ctrl+K), remappable shortcuts | Every action gets an id that packages and agents can name |
| WP5 | Templates, languages and recipes from packages | The remaining data kinds |
| WP6 | Closing review | |

**Package files.**
- A package is a folder with `wish-package.json` at its root. Hidden files and folders (`.git`, `.wish`) are not part of it.
- `id`: lowercase, dot-separated, at least two parts (`com.example.dusk`), 3 to 64 characters of `a-z 0-9 - .`. The engine's own packages start with `wish.`; a package from outside cannot take that prefix.
- `version`: `major.minor.patch`, numbers only (pre-release tags later if needed).
- `engine`: a version requirement: comparators `>=`, `>`, `<=`, `<`, `=`, `^`, `~`, joined with commas (`>=0.9, <2.0`). A bare version means `^`.
- `license`: an SPDX expression. Known permissive licenses pass silently; anything else installs with the warning `package_license_unknown`, so the person decides.
- `contents`: kind → list of package-relative paths. Kinds in M10a: `themes` (`.json`), `workspaces` (`.json`), `languages` (`.json`), `recipes` (`.md`), `templates` (folders with a `wish.json`). An unknown kind is an error with the closest real one as the hint.
- Limits: 4,096 files, 64 MiB, no links (a link could point outside the package).

**Two scopes.**
- `project`: inside the project, `packages/<id>/`, recorded in `wish.json` (`"packages": { "<id>": "<requirement>" }`, only written when there is at least one, so old projects stay byte-identical) and in `wish.lock` (canonical JSON: every package's id, version and content hash, the stable hash of its sorted files). The project carries its packages: it opens the same way on any machine, offline.
- `user`: the person's own packages (themes, languages, workspaces), in `<config>/packages/<id>/`.
- `packages/` is not an asset folder.

**Install, remove, lock.**
- Install copies the folder into a side folder, checks it there, and only then moves it into place (a failed install leaves nothing behind). The same id at another version is replaced only with `replace: true`.
- Remove moves the package to `.wish/trash/packages/` (project) or `<config>/trash/packages/` (user), never deletes it: a removal can be undone by hand (ADR-022).
- `lock` writes `wish.lock` from what is installed; `list` warns (`package_lock_mismatch`) when a package's files no longer match its lock entry.
- Every mutating action has `dry_run`; every action enters the history and activity feed with its actor.
