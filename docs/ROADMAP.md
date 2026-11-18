# Wish Engine — Roadmap

Each milestone has an **output**, an **exit criterion** (we do not move on until it is proven) and a **recommended model · effort** value.
Sizes are relative (S < M < L < XL); they are not a promise about the calendar.

## Overview

| # | Milestone | Summary | Size | Model · effort |
|---|---|---|---|---|
| M0 | Design | Vision, decisions, architecture, MCP draft | S | Fable 5.2 · xhigh — **done** |
| M1 | Core | Schema, ECS, fixed step, determinism, snapshot | L | First session Fable 5.2 · high → then Opus 5.5 · high — **done** |
| M2 | Data layer | Scene format, formatter, validation, asset server, transactions/undo | M | Sonnet 5.5 · high (design-heavy work packages Opus 5.5 · high) — **done** |
| M3 | First pixel | wgpu render foundation, glTF, headless capture | L | Opus 5.5 · high (core) → Sonnet 5.5 · medium (importers) — **done** |
| M4 | Control plane + MCP | One API, MCP server, command-line parity, first agent evals | M | First session Opus 5.5 · high (tool design) → Sonnet 5.5 · medium |
| M5 | Game logic | Wasm host + SDK, hot reload, input, scenario tests, embedded toolchain trial | L | Opus 5.5 · high (ABI, host) → Sonnet 5.5 · medium — **done** (two user checks pending) |
| M6 | Game systems | Physics, audio, in-game UI, animation, 2D subset | L | Sonnet 5.5 · medium (Opus 5.5 · high for hard integration) — **done** (eval run waits for the user) |
| M7 | A game with our own engine | Showcase game, agent eval suite, friction removal | M | Game-building agent: Sonnet 5.5 · medium and Haiku 5.5; fixes: Opus 5.5 · high |
| M8a | Editor foundation (manual editing) | Viewport, hierarchy, automatic inspector, handles, asset browser, play controls | L | Sonnet 5.5 · high |
| M8b | AI cockpit | Chat panel, in-cockpit agent, "show and tell", activity feed, checkpoints | L | Sonnet 5.5 · high (Opus 5.5 · high for interaction design decisions) |
| M9 | Productization → **1.0** | One-package install, export, first run, docs, Steam/web | L | Sonnet 5.5 · medium; release audit Opus 5.5 · high — **code side done** (17 November 2026, `m9-product`); what remains are user steps: [design §5](design/M9-productization.md) |
| M10 | Hub, packages and modding (M10a and M10b in **1.0**, M10c–e after it) | Registries, command palette, themes, workspace documents (the AI mode as a template), data packages, `wish.lock`, the Hub with packages and engine versions | L | Opus 5.5 · xhigh (M10a, M10c), high (M10b, M10d, M10e) — **M10a done** (17 November 2026); M10b in progress on `m9-product` (the Hub, the offline package index, side-by-side engine versions and the closing review done; downloading waits for the user): [design](design/M10-hub-and-packages.md) |
| M12 | The road to a strong 1.0 (**proposal**, grows the 1.0 scope; waits for the user) | Graphics baseline and look lock, Windows + Linux + Steam Deck with cross-export, a small base with components, 2D kit, editor musts, quality mode, verified runs, docs site, stability contract, beta | XL | Fable 5.2 · high (plan, foundations, render); Opus 5.5 · high and Sonnet 5.5 · medium for settled packages: [design](design/M12-road-to-1-0.md) |
| M11 | After 1.0 | Advanced rendering, web target, second scripting language, multiplayer | open-ended | Fable 5.2 / Opus 5.5 · high–xhigh |
| T1 | Interface design (parallel) | Mockups of the hybrid interface: manual-editing and cockpit flows, switching between the two, user testing | S | Opus 5.5 · high — starts together with M4 |

### Rule for choosing model and effort

- **Effort is chosen by the uncertainty of the work, not its size.** Well-defined work → medium. Design decisions, `unsafe` code, hard bugs → high. Only for architectural dead ends → xhigh.
- **Fable 5.2 / Opus 5.5:** things that are expensive to change later (data layout, ABI, tool surface, determinism).
- **Sonnet 5.5:** most of the implementation whose design is already settled.
- **Haiku 5.5:** repetitive work (docs, multiplying tests, bulk renaming) and the "does it also work with a weak model?" check in M7.
- If Sonnet gets stuck on the same problem twice, move that session up to Opus; do not make a third attempt with the same model.
- At the start of each milestone, the recommendation in this table is stated again and updated for that day's conditions.

### Work that can run in parallel

- **M2 and M3** are largely independent (data layer and render); they can move forward at the same time in separate sessions.
- **T1 (interface design)** needs no code; it can start together with M4.
- **M8a (editor foundation)** depends only on M3 and M4; it can move forward at the same time as M5–M7.
- Inside **M6**, physics, audio and in-game UI are independent of each other.

---

## Before starting: development environment

**Status: environment ready** (verified on 15 November 2026). Result of the check done on the machine:

| Item | Status | To do |
|---|---|---|
| Git | Installed (2.55) | — |
| Rust (`rustup`, `cargo`) | **Installed** (15 November 2026): rustc 1.100.0, `stable-x86_64-pc-windows-msvc` | — |
| MSVC build tools | **Installed** (15 November 2026): Visual Studio 2022 Build Tools 17.14.45, "Desktop development with C++" workload (MSVC v143, Windows 11 SDK 10.0.26100) | — |
| Code editor | Visual Studio Code installed | Recommended external editor for writing game code by hand (ADR-018) |
| Graphics card | RTX 3060 Ti | Suitable: DX12, Vulkan and hardware ray tracing (also enough for M10) |
| CPU / memory | Ryzen 5 5600 (12 threads) / 16 GB | Enough. If memory gets tight during builds, the number of parallel build jobs is lowered |
| Disk | 94.9 GB free on drive C: (15 November 2026; it was 21 GB on 10 November) | Enough. The toolchain + build output can reach 30–40 GB in this project; if free space drops below 30 GB, build output is cleaned |
| Project folder | `Masaüstü\wish-engine` (under OneDrive; the user's choice) | **Build output will be redirected outside OneDrive.** The Rust build folder produces tens of thousands of files; OneDrive syncing slows the build and causes file-locking errors. In the first work package of M1, `target-dir = "C:/wish-build/target"` is set with a local `.cargo/config.toml` that does not go into git. Syncing the source files on OneDrive is not a problem |

**Verification (15 November 2026).** A temporary Rust project was created; `cargo run` built and ran it, `cargo test` passed 1 test. The toolchain works end to end; there is no environment-related blocker in front of M1.

The only environment task to do in the first work package of M1: redirect build output outside OneDrive (the "Project folder" row above).

---

## M1 — Core

**Goal.** The deterministic, schema-driven world that everything else is built on.

**Work packages (in order).**
1. Workspace: crate skeleton, `rust-toolchain.toml`, CI (format, clippy without warnings, tests, dependency license check), git repository, `LICENSE-MIT` and `LICENSE-APACHE` files.
2. `wish_core`: identifiers, `Diagnostic`, stable hash.
3. `wish_schema`: `TypeSchema` model, `#[component]` derive macro, JSON ⇄ binary conversion, JSON Schema generation.
4. `wish_ecs`: entities, archetype storage, queries, deferred commands, resources, events, change tracking, hierarchy.
5. Scheduler: explicit system order, fixed step, seeded `Rng`.
6. Snapshot / restore and world hash.
7. `wish_cli`: `wen run --headless --frames N --seed S --json`.
8. Test infrastructure: property-based tests, Miri for the ECS, determinism tests, first benchmarks.

**Exit criterion.**
- A sample world runs headless for 10,000 ticks; the world hash of two separate runs is the same.
- At tick 5,000, snapshot → restore → continue gives the same hash as an uninterrupted run.
- A JSON Schema is generated for every component; the JSON → world → JSON round trip is lossless.
- Miri is clean; CI is green.
- Benchmark: the tick time of a simple system with 100 thousand entities is measured and recorded (for the ADR-004 trigger).

**Model · effort.** First session (a detailed design note on schema and storage layout): **Fable 5.2 · high**. Implementation: **Opus 5.5 · high**. Mechanical work such as skeleton/CI: Sonnet 5.5 · medium.

**Progress.** Detailed design: [design/M1-core.md](design/M1-core.md).

| Work package | Status |
|---|---|
| Design note | Done (15 November 2026) |
| 1. Workspace | Done (15 November 2026). The CI file was written but has not been run yet because there was no remote repository |
| 2. `wish_core` | Done (15 November 2026) |
| 3. `wish_schema` | Done (15 November 2026): macros, JSON ⇄ bytes, JSON Schema, fingerprint. 60 tests in total, format and clippy clean. Two of the exit criteria are met: "a JSON Schema is generated for every component" and "the JSON round trip is lossless" (with a property test for every finite `f32`/`f64`) |
| 4. `wish_ecs` | Done (15 November 2026): storage, typed and JSON access, queries, resources, text table. Model-based test + Miri |
| 4b. `wish_ecs` addition | Done (15 November 2026): deferred commands, events, hierarchy, change tracking. Model test + Miri |
| 5. Scheduler | Done (15 November 2026): `wish_app` (fixed step, system order, per-system random stream), `wish_core::Rng`. Same world for 600 ticks with the same seed |
| 6. Snapshot / hash | Done (15 November 2026): world and app snapshot, stable hash, `bounce` example. Exit criteria: 10,000 ticks × 2 same hash; at 5,000 snapshot → restore → continue = uninterrupted run |
| 7. `wish_cli` | Done (15 November 2026): `wen run`, `wen schema`, `wen examples`; same JSON envelope as MCP |
| 8. Test infrastructure, benchmark | Done (15 November 2026): `wen bench` (0.50 ms/tick at 100 thousand entities), independent closing review and fixing its findings |

**M1 result:** all exit criteria were met. After the repository was pushed to the private GitHub repository, CI is green on Windows and Linux. Details and review findings: [design/M1-core.md](design/M1-core.md) §12, "WP8 and M1 closing".

## M2 — Data layer

**Status: done (15 November 2026).** Design and implementation notes: [design/M2-data-layer.md](design/M2-data-layer.md). WP1–WP8: strict JSON + canonical writer, document and validation, transactions + undo, prefabs, document → world + `wen run --scene`, `AssetRef` + `wish_asset` + `wen mv`, `wen check/fmt/migrate/watch` + hot reload, M1 carry-overs (`trybuild`, single error message, `syn` 3) and an independent closing review (9 findings, all fixed and tested).

**M2 result:** all exit criteria were met; the evidence table is in the "M2 exit criteria" section of the design note's §12. Deviations from the plan (reasons in the note): file watching by polling instead of `notify`; `SceneId` is a reserved component, not a managed one; importers and cache were left to M3 (this was also the plan).

**Output.** `wish_scene` (scene/prefab document model, canonical formatter, patches addressed by id, atomic transactions, undo/redo, revisions), `wish_asset` (asset server, file watching, cache, `*.meta.json`), `wen check`, `wen fmt`, `wen mv`, `wen migrate`.

**Exit criterion.**
- Load → save produces a byte-for-byte identical file.
- Each of 30 deliberately corrupted sample files produces a diagnostic with a location and a hint (no crash or meaningless error).
- When a file changes on disk, the world is updated in less than 1 second; an invalid file does not corrupt the last valid state.
- 100 random operations + 100 undos = the starting file.

## M3 — First pixel

**Status: done (15 November 2026).** Design and implementation notes: [design/M3-first-pixel.md](design/M3-first-pixel.md) (§14: WP1–WP8 and "M3 closing"). The independent closing review gave 11 findings; two were high severity (a hostile glTF crashes / freezes the process) and were fixed; the remaining gaps (memory copy for sampling → M6, device-loss recovery → M5) are in the note.

**M3 result:** the exit criteria were met; the evidence table is in the design note. Honest gaps: the "looks correct" check of the Khronos models was verified by eye only (the models are not in the repository, there is no CI counterpart); the window loop does not run in CI and the content of the picture in the window was not checked by eye. Deviations: window presentation with `softbuffer` (the renderer draws off screen), our own `#import` preprocessor instead of WESL.

**Output.** `wish_window`, `wish_render` (render graph, PBR, directional light + shadow, camera, sky/ambient light), glTF import, headless capture (`wen capture`), debug views (id, depth, normal, wireframe).

**Exit criterion.**
- Standard glTF sample models look correct (compared against reference images).
- `wen capture` runs without a window; the golden image test passes in CI with the software adapter.
- "Which entity is at this pixel" is answered correctly from the id buffer.
- A shader error is reported in the shared diagnostic format, with file and line.

**First task.** Validate ADR-013 (WESL).

## M4 — Control plane and MCP

**Status: done (16 November 2026).** Design and result: [design/M4-control-plane.md](design/M4-control-plane.md) (§15 exit criterion evidence). 20 tools, MCP stdio server, command-line parity byte for byte, the first 10 eval tasks **30/30** with Sonnet 5.5 · medium (two runs). **Plan change:** the Streamable HTTP transport was moved to M8a (together with the editor); the reason is in §1 of the note.

**Output.** `wish_control`, `wish_mcp` (stdio; HTTP in M8a), command-line counterparts of all tools, `wen new` (project skeleton + `.mcp.json` + project `CLAUDE.md`), MCP doc resources, world inspector, the first 10 tasks under `evals/`.

**Exit criterion.**
- Starting from an empty project, Claude builds a scene **with MCP only**, runs it, takes an image and verifies the result — with no human intervention.
- The command-line counterpart of every MCP tool produces the same JSON (automatic parity test).
- The first 10 eval tasks reach ≥ 80% success with Sonnet 5.5 · medium; the measurements are recorded.
- Every invalid call returns a structured error with a hint.

**T1 starts here:** cockpit mockups and interaction flows (no code).

## M5 — Game logic

**Status: done (16 November 2026)**, merged into `main` from the `m5-logic` branch. Notes and evidence: [design/M5-game-logic.md](design/M5-game-logic.md) §17 (WP1–WP9) and §18 "M5 result". The independent closing review gave 20 findings (3 high: a negative `expect` always passed, files/environment could be read during the build, two types with the same name mixed up the values); all were fixed. Two verification steps for the user are pending: the eval run (quota) and the clean machine trial in Windows Sandbox.

**Output.** `wish_script` (wasmtime host, ABI, fuel/time limit, trap isolation), `wish_sdk` (guest SDK and macros), lossless hot reload, `wish_input` (action maps, injection, record/replay), `wish_test` (scenario tests), `script_build` and `test_run` tools.

**Exit criterion.**
- With MCP + file editing, Claude writes a playable mini game (move, collect, win) and proves it with a scenario test.
- Code change → running game < 10 seconds; the game state is kept.
- Guest code that has an infinite loop or throws a trap does not freeze/crash the engine; it produces a diagnostic.
- A recorded replay plays back for 10,000 ticks with no hash drift.
- The boundary cost benchmark is recorded (ADR-011 trigger).
- **Embedded toolchain trial (ADR-023):** on a clean Windows with no development tools installed, the packaged engine compiles and runs game code.

## M6 — Game systems

**Status: done (16 November 2026)**, merged into `main` from the `m6-systems` branch. Notes and evidence: [design/M6-game-systems.md](design/M6-game-systems.md) §15 (WP1–WP9) and §16 "M6 result". The independent closing review gave 13 findings (1 high: game code could grow the world's text table without limit); all were addressed. The eval suite has 25 tasks (`selftest` 25/25); the real run waits for the user. Audio uses cpal + our own mixer instead of Kira (ADR-015 was updated: MPL dependencies).

**Output.** `wish_physics` (Rapier: rigid bodies, colliders, ray queries, character controller), `wish_audio` (Kira + audio event log), `wish_ui` (text, image, button, flex layout), skeletal animation, point/spot lights and clustering, particles (basic), 2D subset (orthographic camera, sprite).

**Exit criterion.**
- Every subsystem can be observed from the control plane and has headless tests (ADR-017 rule).
- A scene with physics passes the snapshot/restore and replay determinism tests.
- The eval suite grows to 25 tasks; Sonnet 5.5 · medium ≥ 85%.

## M7 — A game with our own engine

**Status: done (16 November 2026).** Design and result: [design/M7-showcase-game.md](design/M7-showcase-game.md) (§11 "M7 result"); friction log: [friction.md](friction.md) (F1–F10, all fixed in the engine). The showcase game and two examples were built only with the engine's tools. **Sonnet 5.5 · medium 41/41 (100%)**; 11/11 with MCP Inspector. The setups for the Haiku run and the Unity comparison run are ready; they were not run by the user's choice (pending step); there is no Unreal (disk).

**Output.** Showcase game (decided: a small 3D platform game), two more small examples, an eval suite of 40+ tasks, friction log and its fixes, a short engine guide (`wish://docs/guide`).

**Method.** Have the games built deliberately with medium and small models. Every place where the agent gets stuck is an engine bug: either a tool is missing, an error message is not enough, or a document is wrong. The fix is made in the engine, not in the game.

**Exit criterion.**
- The showcase game can be played from start to finish, the scenario tests pass.
- Eval suite: Sonnet 5.5 · medium ≥ 90%. Haiku 5.5 results are recorded.
- The basic tasks are verified with at least one MCP client other than Claude.
- **Comparative measurement (user decision, 15 November 2026):** an engine-independent subset of the eval suite is also run with the same model on the MCP connections of Unity and Unreal; task success, number of steps, tokens and number of errors are reported side by side. The aim is to prove the claim "smoother with Claude" by measuring it; areas with bad results go into the friction removal list. The Unity/Unreal setup is submitted for approval separately at that time. (It is a measurement; it does not enlarge the 1.0 feature scope.)

## M8 — Hybrid interface

The interface is hybrid (ADR-018): manual editing and the AI cockpit are first-class at an equal level. The work is split in two; both are in `wish_editor`.

### M8a — Editor foundation (manual editing)

**Status: done (17 November 2026)**, `m8a-editor` → `main`. Design and result: [design/M8a-editor-foundation.md](design/M8a-editor-foundation.md) (technology decision: `egui` + our own CPU rasterizer, §2; exit criterion evidence §9).

- Manual end-to-end showcase scenario.
- Automatic parity table: 26 tools + 15 edit operations.
- Interface in 12 languages.
- MCP Streamable HTTP.
- Independent closing review: 35 findings, all addressed.
- CI did not run because of billing: verified only locally.

**Prerequisite.** T1's manual-editing mockups are complete; the ADR-018 interface technology is final. It depends only on the control plane (M4) and the render (M3); so it can start **in parallel** with M5–M7 and is also useful in the engine's own development.

**Output.** Viewport window and camera navigation; hierarchy; inspector generated automatically from the schema; move/rotate/scale handles; asset browser (drag and drop); material and prefab editing; input map editor; play/pause/step/rewind controls and snapshot timeline; console and diagnostics panel; performance panel; project settings; "open in external code editor"; **multilingual interface**: a "one language = one file" infrastructure, 10–12 languages with Latin/Cyrillic/Greek alphabets in 1.0 (TR, EN, DE, FR, ES, PT, IT, PL, RU, UK, EL…; user decision, 16 November 2026).

**Exit criterion.**
- **With no AI connected**, a user builds a section of the showcase game by hand only, plays it and makes it ready for export.
- Parity check: every scene/entity operation in the control plane has a manual counterpart in the interface (automatic check list).
- Every action in the interface is a control plane operation; undo/redo works for every action.
- A component newly defined in Wasm can be edited in the inspector without writing extra interface code.

### M8b — AI cockpit

**Status: in progress** (`m8b-cockpit`). Design: [design/M8b-cockpit.md](design/M8b-cockpit.md).

The parts that need no user are done (17 November 2026):
- selective undo,
- activity feed,
- checkpoints and before/after,
- permission modes,
- the editor's MCP endpoint,
- `wish_agent` (Claude Code + scripted),
- cockpit panel, show and tell,
- external agent connection.

Exit criteria 2–5 are tested. Remaining: criterion 1 (with 3 users, with a real model) and the T1 mockups.

**Prerequisite.** M8a; T1's cockpit mockups; ADR-024 (Claude adapter method) is final.

**Output.** A chat panel next to the viewport, an in-cockpit agent and AI connection settings, "show and tell", activity feed and per-card undo, automatic checkpoints, before/after comparison, permission modes, live observation.

**Exit criterion.**
- A user who has never seen the engine produces a playable mini game in 30 minutes just by talking (tested with at least 3 people).
- Every change the agent makes is visible in the activity feed and can be undone with one click.
- **Handover:** the user makes a change by hand, the agent sees it and continues on top of it; while the agent works the user edits another entity by hand and no change is lost.
- Changes made by hand and by the agent are visible in the same history, with information on who made them.
- When an external agent (Claude Code) and the in-cockpit agent work at the same time, a conflict does not cause data loss.

## M9 — Productization → 1.0

**Output.** One-package install (with the embedded toolchain), `wen export` (standalone game package), first-run flow (connect the AI or skip it → pick a template → first request or start by hand), example projects, user docs (TR/EN), updater (web version), optional crash report, third-party license file, end-user agreement (free, no commission), Steam page and website.

**Exit criterion — the "smooth" bar.** All the success criteria in [VISION.md](VISION.md) are met. In addition:
- On clean Windows 10 and 11 installs: download → first running scene < 5 minutes, no extra install.
- An exported game runs on a machine that does not have the engine installed.
- Known crash bugs are zero; open "high" priority bugs are zero.
- The repository is published openly (MIT + Apache-2.0), the contribution guide is written; the availability of the name, brand and domain name is verified.

**External requirements (they need money and accounts — things you have to do).** A Steamworks account and the Steam Direct application fee; domain name and hosting; a code signing certificate for the web version (without it Windows shows a warning).

## M10 — Hub, packages and modding

**Why.** The user's request (17 November 2026): the AI mode should be a basic template people can easily change, there should be a Hub like Unity's or Unreal's (projects, engine versions, plugins, mods), and the interface and much more should be moddable, with the groundwork laid early. Plan and reasons: [design/M10-hub-and-packages.md](design/M10-hub-and-packages.md). **This grows the 1.0 scope** (user decision, 17 November 2026: M10a and M10b go into 1.0).

- **M10a — Foundations (1.0):** registries (tools, commands, panels, themes, icons, languages, templates, diagnostics) that the engine's own parts go through; command palette (Ctrl+K) and remappable shortcuts; theme tokens and theme files; workspace documents (the AI mode and the manual mode are the two default workspaces; game card fields and ready requests come from data); package manifest, `wish.lock`, local data packages; the engine version in `wish.json`.
- **M10b — Hub (1.0):** the welcome screen grows into the Hub (Projects, Learn, Packages, Look, AI, Settings, Engine versions); a serverless package index (a git repository, signed entries); `wen pkg search/add/remove/update/lock`; side-by-side engine versions.
- **M10c — Code extensions (after 1.0):** the `wish_ext` Wasm ABI; tools, panels, importers and hooks from packages, in the game code's sandbox, with permissions.
- **M10d — Game packages (after 1.0):** components, systems, prefabs and assets as packages, compiled with the game.
- **M10e — Ecosystem (after 1.0):** publishing, signing keys, docs site, RFC process.

**Exit criterion (M10a + M10b).** The AI mode, the manual mode, the default theme and the templates are all loaded from the registries as packages; a theme, a workspace and a recipe package made outside the engine install from the Hub, change the editor without rebuilding it, and come off again cleanly; a project with a `wish.lock` opens with the same packages on another machine; every package action is a control-plane tool with a headless test.

## M11 — After 1.0 (order depends on need)

- Advanced rendering: GPU-driven drawing, bindless resources, meshlet/virtual geometry, ray-traced lighting, cascaded shadow maps (because a single map is fitted to the scene, a distant object softens nearby shadows: an M8a WP9 finding)
- Web target (WebGPU + Wasm)
- A second scripting language that needs no toolchain
- Parallel system execution
- Multiplayer (rollback netcode — the determinism infrastructure is ready)
- Linux and macOS packages
- Interface languages outside Latin/Cyrillic/Greek: Chinese, Japanese, Korean, Arabic (right to left), Hindi — needs a font (Noto, SIL OFL-1.1: adding it to the allowed list is a user decision) and right-to-left text layout (user decision, 16 November 2026)
- Translating the engine's diagnostic messages (`message`/`hint`): the editor interface is in 12 languages (M8a WP8), but the control plane's messages are in English (one language for agents and docs); translating a diagnostic into the person's language in the editor needs a translation file per code
- Accessible names per row in the editor: buttons such as "Remove" repeated on every row are told apart only by their row (an M10b closing review finding)
- In MCP Streamable HTTP, the stream started by the server (`GET` + SSE: progress notifications, requests from the server) and CORS for connecting directly from a browser (in M8a WP8 only `POST`/`DELETE`, responses are JSON)

---

## Risk register

| Risk | Effect | Mitigation | Checkpoint |
|---|---|---|---|
| Scope growth | The project never ends | The 1.0 scope is fixed ([VISION.md](VISION.md)); new ideas go to the M11 list | End of every milestone |
| The embedded toolchain cannot be packaged smoothly | The "zero install" promise cannot be kept | Trial on a clean machine in M5; fallback plan in ADR-023 | M5 |
| The custom ECS performs too slowly | Big scenes are slow | Early benchmark; fallback plan in ADR-004 | End of M1 |
| Wasm boundary cost | Game logic is slow | Bulk data transfer; benchmark; ADR-011 trigger | M5 |
| Determinism leak | Replay/test cannot be trusted | Double-run hash test on every change | Continuous (CI) |
| Silent bug in engine code written by AI | The foundation rots | Exit criteria demand evidence; an independent code review at the end of every milestone; Miri for `unsafe` | End of every milestone |
| The cockpit turns out unusable | The heart of the vision stays weak | T1: mockups and user testing before code; testing with 3 users in M8b | T1, M8b |
| The hybrid interface enlarges the scope (full manual editing) | 1.0 is delayed | M8 was split in two; the inspector is generated automatically from the schema; expert tools (animation, shader graph, terrain) are outside 1.0; M8a starts early and in parallel | Start of M8a |
| Disk/memory shortage on the development machine | Build fails/slow | Free disk space, redirect the target folder | Before M1 |
| One person + a team of agents; review load | Quality drops | Small steps, green CI at every step, writing decisions into documents | Continuous |

## Working order

1. **Every milestone in its own session(s).** One long session blurs the context.
2. **Start of session:** `CLAUDE.md` + the relevant milestone section + the relevant ADRs are read; the recommended model · effort is stated.
3. **A short design note first** (`docs/design/Mx-topic.md`), then code.
4. **Small steps:** every work package is built, tested and committed.
5. **Finish with proof:** exit criteria are shown with command output; we do not say "done", we show it.
6. **If a decision changes,** [DECISIONS.md](DECISIONS.md) is updated; when the milestone is done, its status in this file is marked.
7. **End of milestone:** an independent code review (separate session, high effort), then the move to the next stage.
