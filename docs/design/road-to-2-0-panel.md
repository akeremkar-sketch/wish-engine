# The road to 2.0: what the panel wrote

Raw material behind [road-to-2-0.md](road-to-2-0.md), kept as the backlog its
packages start from. Written by agents on 18 November 2026: three planners
(one lens each), one web researcher and two critics. **Verify a claim against
the code before building on it.** The web research was not checked by a
person. The plan that counts is road-to-2-0.md; where this file disagrees
with it, that file wins.

## Plan: The AI-native leap (key ai-native)

**Thesis.** Wish Engine 2.0 is a free, open-source, deterministic game engine for one person and one or more AI agents who make a small or medium game together, 3D or 2D, on Windows or Linux, and ship it to Windows, Linux, Steam Deck and, as labelled previews, the web and macOS. Everything in a project is text the engine can check, run, explain and merge, the game's logic included: most logic is written as rules (data) and Rust is kept for what rules cannot express. Every task ends at one verification gate the engine computes (tests, replays, a seeded soak, checks on what the picture shows). A quality mode spends more tokens to plan, build, prove, review and repair. Several agents work in forks that merge only when the gate is green. Packages carry recipes, checks and tests that show an agent can use them. A person can do each of these steps by hand in the editor with no AI connected. The claim is published, not asserted: every release carries a scorecard (held-out tasks, two models, a whole game from a paragraph) and 2.0 adds a benchmark that runs the same tasks on other engines, with the commands that reproduce it. It is not for mobile, consoles, online multiplayer or high-end graphics.

**One bet.** That a game kept entirely as checkable data in a deterministic engine, its logic included, lets the engine itself compute whether an agent's work is done, so "done" is a proof the engine publishes and not a claim the agent makes.

**Total packages:** 156. **Calendar:** About 8 to 13 months after 1.0, for 156 packages in eleven releases (1.1 to 1.10, then 2.0). The low end assumes about 25 machine hours a week from the one code-writing agent, sign-offs within a day and a standing spending cap; the high end about 14 hours a week. Fixed inside it: a release-candidate week per release and a 2.0 beta of at least four weeks. A no from the rules experiment makes the train about 9 packages shorter. Outside people (first runs, package authors, the benchmark rerun) can stretch the end but not the middle. All of this is an estimate, not a measurement.

### 1.1: Done means proven; the rules experiment (14 packages)

- Code freshness: a content fingerprint of scripts/, one ensure_code_fresh used by sim_start, test_run, replay and export; stale code is rebuilt through the checked side-file path or refused with a hint; unchanged sources skip the compiler (1)
- The verification gate: check level full (project check, fresh build, every scenario test, saved replays, a seeded soak, a camera that sees something); one answer with a diagnostic and hint per failed gate; the Health panel and wen check call it (1)
- AI efficiency round 2: ritual turns removed from the scaffold and the guide, small scenes answer values, sim_start runs and closes in one call, keep_world and look_at, problem counts in edit, build and test answers, AGENTS.md, lean flags for the cockpit agent (1)
- The game-code loop: docs_search ranks by any word and returns the whole section, one address per recipe, test_run takes an inline test, a failed expect carries min, max and tick of each path it read (1)
- Background build and build on save as a control-plane capability; templates ship built modules; the compiler warms up when a project opens (1)
- Lean play path (no hash or inspector in the player's frame); one committed replay per example run on Windows and Linux in CI; macOS arm64 and wasm32 determinism as informational jobs; a window smoke test (1)
- Profiler basics: fuel per game system from sim_step, wen bench --project, fuel budgets asserted in CI (1)
- The rules experiment (3): the expression language moves out of wish_control into a crate below the simulation and is compiled against columns; rule components (trigger, condition, actions) run by an engine system with no compile step; the same game-logic tasks written as Rust and as rules, judged by the scorecard
- Environment maps: an equirect .hdr becomes irradiance and a prefiltered cube on the CPU, cached, drawn as the skybox; particles that glow. Added as non-default settings (2)
- Editor first hour, part 1: orbit, pan, zoom, frame; a Create menu; duplicate, copy and paste; new scene (1)
- Closing review with a fix budget; transcript audit (every read-back found becomes an answer fix); scorecard campaign (1)

Why here: Every later AI package exits through the gate and is judged against a fresh baseline, and the rules answer decides what 1.4 builds. Changed from the draft: the 2D kit (about 8 packages) moves to 1.2 and 1.4, because with the gate and the experiment it would make 1.1 larger than 1.0, and its templates should be written after the rules answer. Environment maps are the one must-tier render item the short path left out.

Needs from the user: A spending cap per campaign; Yes or no on rules, with the results (a yes grows the scope); An HDR sky to ship: generated in code, or approval to download a CC0 one

### 1.2: A whole small game; the first migration (14 packages)

- The first migration step, in wish_control (wen migrate calls it), and a compatibility corpus in CI: projects written by 1.0 and 1.1 open, migrate and keep their meaning (1)
- Player-indexed actions and replay format 2 (build id, strict mode, by path, a hash every N ticks, keyframes) as one hash-and-format change with its migration; the SDK stays source-compatible, player 0 by default (1)
- Gamepad: button and axis names in the input map, read in wish_window through one wrapper dependency, accepted by sim_step input and replays (1)
- Focus navigation for the in-game interface with gamepad and keys (1)
- Scene switching at run time as a recorded, deterministic step; animation cross-fade (1)
- Save games: engine-owned slots written from a snapshot in a versioned canonical format, requested by game code, visible from the control plane (1)
- The presenter (2): swapchain with vsync, interpolation between ticks, fullscreen, HiDPI, window settings in project_settings, automatic software fallback, a forced device-loss test; readback stays the tested fallback
- 2D kit, first half (3): sprites kept on the GPU with order and Y-sort; sprite sheets, SpriteAnimation and Aseprite JSON import; fixed viewport, pixel-perfect camera, the pointer in the world
- Editor first hour, part 2: the menu split, HiDPI, cursor shapes, feedback beyond one status line (1)
- Scorecard hard tier: whole-game briefs with a menu, two levels and a save; bug-fix and regression tasks; a per-try folder the agent cannot leave; three tries with spread (1)
- Closing review, transcript audit, campaign (1)

Why here: A long task cannot mean a finished game until a game can have a menu, levels, a save and a gamepad. These are also the changes that need the first migration after 1.0, so they are made together, once. The presenter is the largest fixed frame cost and the base of every later platform. Changed from the draft: the editor musts are spread over 1.3 to 1.8; only the first-hour items stay here.

Needs from the user: Approve the gamepad dependency after its licence check; Sign off fullscreen, vsync and HiDPI by eye on a real screen; A gamepad in hand for one manual check

Needs from outside people: One or two informal first runs by people who have not seen the engine

### 1.3: The desktop, small (15 packages)

- Tool profiles as a view over the one catalogue (a core of at most 25 for outside agents, all for the cockpit) and a read-only session role (1)
- Components (2): the compiler, the SDK pack and export players become components in the machine folder (list, add, remove, check, each with dry_run); base and full archives per host from the release pipeline
- The fetch layer (2): HTTPS to a file with a size cap, SHA-256, a signature check; used by components, the package index, engine versions and the updater
- Cross-export: project_export takes a target and copies that system's prebuilt player (1)
- Steam Deck: the Linux export run inside the Steam Linux Runtime in CI, 1280x800 fullscreen by default, gamepad in the templates, a check list (1)
- macOS arm64 as an unsigned preview (2): CI job, .app bundles, the Command key, ad-hoc signing; never gates a release
- Windows export polish and slim players: the game's icon and version, strip and LTO, a player that links less of the control plane (1)
- First-run robustness: a doctor action, a folder picker, a diagnostic with a hint for every missing component (1)
- Quality tiers Low, Medium, High and a render scale; the MSAA measurement on WARP and lavapipe, MSAA as a non-default setting if they agree (1)
- Frustum culling, sorted opaque draws, instancing of identical mesh and material (1)
- Editor: grid, axis indicator, icons and picking for lights, cameras, empties and sprites; silhouette selection (1)
- Closing review, transcript audit, campaign; the eleven scores re-scored (1)

Why here: Platforms are one of the user's two first priorities, and this is the draft's 1.3 unchanged in substance. Profiles come first so the catalogue does not grow past 34 unguarded. A shorter path from download to first request is what every later outside test depends on.

Needs from the user: The HTTP/TLS, SHA-256, signature and archive libraries (licences to approve); The index repository's name and owner; GitHub Releases as the download address; Where the line between base and extras runs (open decision 8); A Windows code-signing certificate (money), or accept the SmartScreen warning

Needs from outside people: A report from a real Steam Deck before the word playable is used; Someone with an Apple-silicon Mac to open the preview

### 1.4: Logic as data; 2D complete (15 packages)

- Rules v1, if the experiment said yes (3): rule documents with triggers (events, contacts, timers, actions, state enter and exit), conditions, actions (set, add, spawn a prefab, despawn, send an event, play a sound, switch scene, save) and state machines; unknown names are diagnostics with hints at load; edit_apply operations; a rule table in the editor (invariant 9). If no: these packages become the Wasm boundary work and kits
- Rules that explain themselves: fire counts per rule, why a rule did or did not fire at tick T, rule coverage reported by test_run and the gate (1)
- 2D physics profile on Rapier 3D: a world setting that locks bodies to the plane, 2D shapes, one-way platforms, a collider sized from the sprite (1)
- Tilemaps (3): chunked tile data with merged colliders and one mesh per chunk; paint, fill and read as rows of characters with a legend; a palette in the editor
- Editor 2D mode: pan and zoom, placement on the plane, pixel grid and snap, frame picker, collider handles (1)
- 2D content and proof (2): platformer, top-down and puzzle templates (rules first where rules won), recipes, evaluation tasks, a small CC0 tileset and character as an engine package
- Art as data, part 1: procedural textures (noise, gradient, brick, checker) and SVG-subset sprites rasterized at import with the vector rasterizer already in the tree; asset_create makes both (1)
- Nine-slice and picture options for the interface; clipping and scrolling (1)
- Ambient occlusion on indirect light, a non-default High-tier setting (1)
- Closing review, transcript audit, campaign with rules and Rust side by side (1)

Why here: Rules must land before recipes, templates, the quality mode and web export are built on game code, or each is written twice. Tilemaps as rows of characters and logic as data are the two pieces of 2D a language model handles best, so the long task gets cheaper here. Changed from the draft: the quality mode moves to 1.5 because it needs a complete small-game feature set to be measured.

Needs from the user: Where 2D art comes from (open decision 5): made as data, a CC0 pack to download, or commissioned

### 1.5: Quality mode; proof as a product (15 packages)

- Quality mode (3): acceptance on plan steps (a test or a condition); the pipeline Plan, Build per step in a fresh context, Prove with the gate, Review by a read-only second run, Repair with a round and spending cap, restore the checkpoint on failure; Fast, Balanced, Thorough in the editor; wish-evals run --mode records pass rate against cost
- Other agents: typed schemas for array and object parameters generated from the parser, a data-driven command-line adapter, --agent in both runners (1)
- Checks on what a picture shows (2): view lints in the gate (text too small, interface boxes overlapping or off screen, the subject not visible, low contrast, more than one camera angle); before and after picture differences as data, with a preview on proposals
- A deterministic play-test bot: seeded, budgeted, tries to reach the win condition and reports where it gets stuck (1)
- Replays and a ring of recent input in the shipped player, written beside the crash report (1)
- Verified runs (2): replay verify with a strict mode and an expected result, a signed verdict, wen verify in batches; shipped as the first optional package with a recipe, a template and a server example
- Art as data, part 2: layered synthesized sound effects and simple procedural meshes (extrude, lathe) as text assets (1)
- Editor (2): console and problems with filter and click-through to file and line; play loop polish (sound in Play, keep a live edit, time scale, game view presets)
- Spot-light shadows in a shadow atlas for a bounded number of lights (1)
- Closing review, transcript audit, campaign across the three modes (1)

Why here: The quality mode needs the gate (1.1), the hard tier (1.2), the read-only role (1.3) and rules (1.4) before a pass-rate-against-cost curve means anything. Verified runs need replay format 2 (1.2) and a Linux package for servers (1.0).

Needs from the user: A spending cap per quality-mode task and per campaign; Approval to install one or two other agent programs for testing; Where signing keys for verdicts live (never in project files)

Needs from outside people: The formal three-person test (thirty minutes, talking only) can first be run here

### 1.6: Packages agents can use; docs that cannot drift (15 packages)

- Packages carry assets and prefabs (2): read-only asset roots addressed as packages/<id>/..., export copies what is referenced, the replay fingerprint covers them
- Dependencies with deterministic resolution recorded in wish.lock; author tooling (wen pkg new, index, pack; packages check runs a template's scenario tests); an authoring page for people and one for agents (2)
- Packages that prove themselves (2): a package may carry checks (rules for the project checker, as data) and evaluation tasks with hidden tests; packages check --prove runs them; the index records model, tries, date and the command (ADR-027 rule 4)
- A seeded index (2): starter asset packs, a template per genre, recipe packs for enemies, health and damage, spawning, menus and pause, timers, level change; with no index configured the engine's own packages answer
- Asset import and management in the editor (2): file drop and dialog, delete, duplicate, folders, model and prefab thumbnails, sound audition, import settings that are read
- A reference site generated from the schema, an offline copy in the package, help inside the editor (2)
- Tutorials executed in CI: a first game by hand, with the AI mode, in 2D; the same lessons as missions in the Hub; concept, limits and coming-from pages; the first lesson in Turkish (1)
- The stability contract in full: stable and experimental surfaces, a snapshot of the tool catalogue that forces a changelog entry, deprecation rules (1)
- Closing review, transcript audit, campaign (1)

Why here: The user ranks ecosystem and docs next after graphics and platforms, and both need calendar time with outside people, so their formats ship as soon as downloads (1.3) and rules (1.4) exist. A package that carries its own checks and proof of use is what lets an agent use it right on the first call.

Needs from the user: Site hosting and, optionally, a domain (money); A place to talk and who moderates it; Index rules: licences allowed, a label for AI-made content, takedown; Art for starter packs beyond what art as data covers

Needs from outside people: First outside package authors; an ecosystem score above 3.5 depends on them

### 1.7: A link anyone can open (web preview) (15 packages)

- The lean runtime (3): session build and step move into a crate that Workspace and the player both use; project files are read through one interface (a folder, or a sorted archive without timestamps)
- A second way to run game code (3): an ADR on fuel across executors first; Pulley on native as the proof; modules precompiled at export behind a small host interface; for the browser its own Wasm engine with limits injected at export
- Web export, WebGPU only, labelled a preview (4): a non-blocking GPU path on wasm32, a web presenter and loop, audio after a user gesture, project_export target web with a local preview, a headless-browser smoke test in CI; rules-only games first, Rust game code second
- GPU skinning in the opaque, shadow, id and debug pipelines (1)
- glTF coverage: texture transform, sampler wrap and filter, vertex colours, unlit, morph targets (1)
- Editor: gizmo completion and hierarchy at scale (2)
- Closing review, transcript audit, campaign (1)

Why here: Web needs the presenter (1.2) and slim players (1.3). Its lowest-risk first step is a rules-only game, which the engine runs itself and so needs no guest executor in the browser (1.4). It is the highest-risk item of the train, so it sits in the middle where a slip does not hold 2.0.

Needs from the user: Approve wasm-bindgen and web-sys after a licence check, and a browser download in CI; An itch.io or pages account to host a test build

Needs from outside people: People opening the test build in different browsers and systems

### 1.8: The editor a person stays in (15 packages)

- Profiler panel: per-system bars, entity, draw and triangle counts, physics numbers (1)
- Inspector depth: mixed values, per-field override mark and revert, apply to prefab, arrays that grow, pickers with thumbnails, accessible names per row (1)
- Material editor: texture slots, a shared material library, a preview sphere (1)
- Particle and animation preview in edit mode (1)
- Seeing game logic (2): overlays for velocities, contacts and rays, a watch panel, an event inspector, debug lines from game code; session diff between two ticks, last written by system X at tick T, go to tick, a thinning ring of snapshots in the timeline
- Support for hand-written code: rust-project.json and SDK sources in the pack, so an outside editor completes and shows errors (1)
- Long calls leave the frame (2): build, tests, export and long sim_step run with progress and cancel; the MCP server-started stream carries progress and lets game_design ask reach a person through an outside client
- Dockable layout with a panel registry and saved layouts (2)
- Interface layout on the canvas: select, move and resize interface nodes with their boxes shown (1)
- Reflection probes, a non-default setting (2)
- Closing review, transcript audit, campaign (1)

Why here: The first-hour, picking, import, console and gizmo items were spread over 1.1 to 1.7. What is left does not move the AI card's first three items but decides card item 4: whether a person can maintain by hand what an agent made, and the formal user tests.

Needs from the user: Sign off reflection probes by eye

Needs from outside people: A second round of first runs, by hand only, with no AI connected

### 1.9: Several agents at once (14 packages)

- Forks and merges (3): a fork is a working copy with its own build folder and sessions; a three-way merge of canonical documents (scenes, prefabs, rules, input maps, tests) in wish_scene; a merge enters the one history only when the gate is green on the merged result, and conflicts become proposals for the person
- The orchestrator in wish_agent, tested with scripted adapters: plan steps with disjoint claims run in parallel forks, a read-only reviewer per merge; lanes in the cockpit (1)
- A direct model adapter (2): the engine drives a model API or a local endpoint itself with only its own instructions and core tools in context; keys in the operating system's store; an agent package kind for other adapters
- Tools from packages, experimental (3): a tools kind with a prebuilt Wasm module, a manifest of tools with parameter tables, nested calls made by the host with actor ext:<package>, a generated form in the command palette
- Kits, experimental (2): code in packages compiled by one more rustc call, a namespacing rule decided by ADR first, source checks on package sources, a first kit taken from the example games
- The Wasm boundary (2): slice views for padding-free types, read-only columns neither copied back nor compared, Changed and Added filters, buffers reused across ticks
- Closing review, transcript audit, campaign with one, two and four agents (1)

Why here: Parallel agents are safe only once the gate (1.1), logic as mergeable data (1.4) and the reviewer role (1.5) exist. Code from packages waits until the package formats of 1.6 have had one release of use.

Needs from the user: The user's own API key for the direct adapter (spends money), or a local model; Approve the experimental label on kits and tools from packages

Needs from outside people: Package authors trying kits and tools before the 2.0 freeze

### 1.10: The small language; depth of look and 2D (14 packages)

- The small language (5), only if rules won and the recorded list of what rules could not express shows functions would cover it: a text form of rules with typed functions checked against the schema; run by the engine under a deterministic step budget, with no compiler to install; errors with file, line and hint; recipes and evaluation tasks in both forms; labelled experimental at 2.0
- 2D depth (5): particles with pictures, additive blending and layers, parallax and backgrounds; text in the world with outline and pixel fonts; 2D lights (2 of the 5); autotiling
- A baked irradiance probe grid: placed automatically, baked on the CPU by one tool action, cached, golden-tested (2)
- Material shaders from projects and packages, with errors mapped to file and line (1)
- Closing review, transcript audit, campaign (1)

Why here: All of it is depth past the M12 scope and the first to be cut. The language needs several releases of rules in use and a second decision made from data, not from taste.

Needs from the user: Yes or no on the language, with the 1.4 to 1.9 results; Sign off 2D lights and probe lighting by eye

### 2.0: The published proof (10 packages)

- The public benchmark (3): an engine-neutral task and check format; game-code and whole-game tasks; adapters for Wish, Unity and Godot; at least two agents and two models; three tries with spread; every transcript published; a way for others to submit an adapter
- A results page generated per release: the scorecard, bench numbers and test counts, each with its command (1)
- The showcase games re-dressed with the new look, and a feature gallery of one small scene per feature (2)
- If the user has the accounts (2): signed and notarized macOS; how native SDKs attach as engine-built player variants, with Steamworks as the first
- Freeze audit and the 2.0 contract: which experimental surfaces become stable, one planned window for the default look with its migration, the compatibility corpus from 1.0 on passes; a launch kit (1)
- A beta of at least four weeks with no feature merges, weekly builds, release candidates, 2.0.0 and a planned 2.0.1; closing review (1)

Why here: A proof is published last, on the engine as it ships, and a beta needs calendar time that code cannot shorten. Expected weighted totals at 2.0: 'AI with small games' about 85 (83 without outside people) against Godot 72 and Unity 68 as they stand today; 'commercial release' about 71 (67) against Unity 83 and Godot 77.

Needs from the user: Approve installing Godot and keeping Unity for the benchmark runs, and a spending cap for them; Apple Developer and Steamworks accounts (money), or drop those two packages; Art for the re-dressed showcase; Whether to publish numbers that name other engines; Sign off the 2.0 default look by eye

Needs from outside people: Ten or more outside first runs on named platforms; Three people for the formal test; At least one outside rerun of the benchmark before it is called reproduced; A jam with a by-hand track and a with-agent track

### Scores at 2.0

| Area | With outside people | Without | Basis |
|---|---|---|---|
| Graphics quality | 6.5 | 6.5 | Environment maps (1.1), tiers, culling and instancing (1.3), ambient occlusion (1.4), spot shadows (1.5), GPU skinning (1.7), reflection probes (1.8), the baked probe grid and material shaders (1.10), art as data and the re-dressed showcase; 6 if the probe grid and reflection probes are cut; no outside people needed, only the user's eye. |
| Platform support | 6.5 | 6 | The presenter and gamepad (1.2), components, cross-export, Steam Deck and the macOS preview (1.3), the web preview (1.7); the extra half point needs a real Deck report, an Apple account for signed macOS and outside browser tests; no mobile or consoles. |
| Ecosystem and community | 4.5 | 3.5 | Packages with assets, dependencies, author tooling, proofs of use and a seeded index (1.6), kits and tools from packages (1.9), a jam; above 3.5 only outside authors and calendar time can earn. |
| Maturity and docs | 6 | 5 | The migration and compatibility corpus (1.2), the generated reference site, tutorials run in CI and the full stability contract (1.6), eleven public releases and a four-week beta; the sixth point is outside first runs and time in people's hands. |
| Editor tools | 7.5 | 7.5 | First hour (1.1, 1.2), picking (1.3), 2D mode, tile palette and rule table (1.4), console and play loop (1.5), asset import (1.6), gizmos and hierarchy (1.7), profiler, inspector, material editor, docking and logic debugging (1.8); 7 if docking and canvas layout are cut; no specialist editors. |
| 2D games | 8 | 8 | The 2D kit (1.2, 1.4), nine-slice, tilemaps as rows of characters, SVG sprites, then particles, parallax, world text, 2D lights and autotiling (1.10); 7.5 if 1.10's 2D depth is cut. |
| Determinism, replay, tests | 10 | 10 | Already the top of the scale; replays committed in CI on two systems (1.1), replay format 2 and saves (1.2), replays in the shipped player and verified runs (1.5) turn it from a test feature into a product one. |
| Working with AI | 10 | 9.5 | The gate and game-code loop (1.1), hard tier (1.2), rules (1.4), quality mode, picture checks and play-test bot (1.5), packages that prove themselves (1.6), several agents (1.9) and the public benchmark (2.0); the last half point is an outside rerun and the three-person test, and rivals at 7 today will not stand still. |
| Code iteration speed | 9.5 | 9.5 | Code freshness, background build and instant first Play (1.1), rules with no compile step (1.4), the small language (1.10), kits (1.9); 9 if the rules experiment says no. |
| Lightness and install | 9 | 9 | A base of about 70 MiB with components on demand and slim players (1.3), and rules-only games that never need the compiler component (1.4); 8.5 if rules lose; the size is the note's estimate, not a measurement. |
| Licence and cost | 10 | 10 | MIT plus Apache-2.0, no fee or share, the user's own AI account; nothing in the train changes it, and every new dependency passes the permissive-licence check. |

### Not in 2.0

- Android and iOS: they need devices, store accounts, a Mac, touch and lifecycle work, and iOS needs the no-JIT executor proven first; the lean runtime and second executor of 1.7 keep the door open, and this is the first candidate for 2.1.
- Consoles: the SDKs are under NDA and cannot live in an open repository; the honest deliverable is a documented porting path.
- A kernel anti-cheat: proprietary and outside the sandbox model; verified runs cover leaderboards and the docs say what they do not cover.
- Rollback netcode and multiplayer: transports, relays and NAT are a product of their own; player-indexed input and replay format 2 (1.2) are the groundwork that would be a breaking change later.
- Real-time global illumination, GPU-driven drawing, virtual geometry, ray tracing, volumetric fog, screen-space reflections and upscalers: an agent-made small game gains more from good defaults, probes and art as data; TAA comes in only if the MSAA measurement fails.
- Point-light shadows, compressed textures (KTX2), mesh LOD, occlusion culling and custom post-effect shaders: no task on the scorecard needs them, and KTX2 needs a dependency decision.
- Specialist editors (animation timeline and state machine, shader graph, terrain) and panels from packages: a panel ABI is a lasting contract, and review tools matter more to agent-and-person work; Blender, a text file or an agent cover them until then.
- Parallel system execution and a simulation/render thread split: the highest determinism risk on the list and no scorecard item needs it yet; the profiler of 1.1 will say when.
- A Rapier 2D backend, 2D skeletal animation, isometric and hex tilemaps, Tiled and LDtk import: the 2D physics profile and Aseprite import cover the measured tasks; importers can arrive as tools from packages.
- Interface languages outside Latin, Cyrillic and Greek, and translated diagnostics: they need a font licence decision and right-to-left layout, and agents and docs use one language.
- Paid content, ratings and reviews in the package index: they need moderation, payments and outside volume first.
- A generative model inside the engine: the user brings their own AI (ADR-024); the direct adapter of 1.9 uses the user's key or a local model.
- Visual node scripting: rules as data with a table editor is the engine's answer; a graph view of rules can follow if people ask.
- An editor that runs in the browser: the web target of 1.7 is for games; the whole control plane on wasm32 is a separate project.

### Risks

- The rules experiment says no, or rules win on cost and lose on correctness. Mitigation: The decision rule is already written (no worse on correctness and errors for any task). On a no, the rules packages become the Wasm boundary work and kits, the language slot is dropped, and the bet rests on the gate and the scorecard; iteration stays at 9 and lightness at 8.5.
- The scorecard is tuned to itself: the home side writes the tasks. Mitigation: Held-out tasks rotate and are never studied; two models and three tries; every transcript published; the 2.0 benchmark uses an engine-neutral format and is called reproduced only after an outside rerun.
- The owner's attention: eleven releases, each with decisions, by-eye sign-offs and a campaign. Mitigation: One decision sheet per release, a standing spending cap, sign-offs batched at the release candidate; new render features are non-default settings, so no release except 2.0 waits on a default-look decision.
- Tokens: 156 packages is about twelve times the 1.0 short path. Mitigation: No release is larger than 15 packages; settled packages run on a smaller model by the roadmap's rule; the cut order removes about 45 packages without touching the M12 scope.
- Web export does not hold: blocking GPU calls, limits inside a browser's Wasm engine, wasm32 determinism. Mitigation: An ADR on fuel before any code; rules-only games first, which need no guest executor; the wasm32 determinism job runs from 1.1; labelled a preview and never gates a release.
- The first format change after 1.0 breaks projects or replays already in people's hands. Mitigation: The migration step and a corpus of 1.0 and 1.1 projects land before the change; a replay stays valid on the engine that recorded it (engines side by side, ADR-005); a migrated replay is re-recorded and says so.
- The tool surface grows past what a model chooses from well (34 today). Mitigation: Profiles before any new tool; a new capability is a parameter first (ADR-010); a campaign that gets worse reverts the change.
- Quality mode and parallel agents spend more than the person expects. Mitigation: Caps per stage and per task shown before a run; the pass-rate-against-cost curve is published; Balanced is the default.
- A merge of two agents' work passes each fork's tests and breaks the game. Mitigation: The gate runs on the merged result, not on the forks; conflicts become proposals; every merge is one undoable entry in the single history.
- Code from packages (kits, tools) freezes an ABI too early or opens a hole in the sandbox. Mitigation: Experimental through 2.0; namespacing decided by ADR first; the same source checks and hostile-input tests as game code; installing is a person's action only.
- Two forms of game logic double the docs, recipes and evaluation tasks. Mitigation: A recipe exists as rules only where the experiment showed rules win; Rust stays for the recorded list of what rules cannot express; reference pages are generated from the schema for both.
- Ecosystem and maturity need people who may not come. Mitigation: Two numbers for those scores; first-party packs and proofs of use fill the index; a jam after 1.7 when a game opens as a link; no claim of a community before there is one.
- Competitors move: free official agent surfaces, command lines built for agents, community agents. Mitigation: The lead is kept where it is costly to copy (determinism-backed proof, logic as data); the benchmark is rerun on rivals' current versions; the scores here are against today's columns.

### Decisions that are the user's

- The name, before 1.0's freeze (file names carry it): still open.
- A spending cap per scorecard campaign (eleven campaigns) and per quality-mode task.
- Rules: yes or no after the 1.1 experiment (a yes grows the scope). The small language: a second yes or no before 1.10.
- New dependency: a gamepad library (1.2).
- New dependencies: HTTP/TLS, SHA-256, a signature verifier, an archive codec (1.3).
- Downloads: the index repository's name and owner; GitHub Releases as the address.
- New dependencies: wasm-bindgen and web-sys; a browser downloaded in CI (1.7).
- Money and accounts: a Windows code-signing certificate; an Apple Developer account and access to a Mac; a Steamworks account and the Steam Direct fee; a domain and site hosting; an itch.io account.
- Art: where showcase, template and starter-pack art comes from; approval for any CC0 pack or HDR sky download.
- Outside people: informal first runs (1.2), three for the formal test (1.5 and 2.0), ten or more for the 2.0 beta, package authors (from 1.6), one outside rerun of the benchmark, a real Steam Deck.
- Installs for measurement: other agent programs (1.5), Godot for the benchmark (2.0); Unity stays installed.
- The direct model adapter: the user's own API key (money) or a local model.
- Publishing numbers that name other engines, and the benchmark's rules for outside submissions.
- Index policy: licences allowed, a label for AI-made content, takedown; a place to talk and who moderates it.
- The line between base and extras (open decision 8); whether kits and tools from packages ship as experimental.
- By-eye sign-offs: the presenter, each High-tier setting, 2D lights, probe lighting, and one default-look window at 2.0.
- Merging to main and tagging: a standing yes per release, or asked each time.
- What 2.0 may break: the plan assumes nothing (2.0 opens every 1.x project with a migration).

### Cut order

- First, what the user did not ask for: the direct model adapter, the Steamworks variant, signed macOS, docking, interface layout on the canvas, Hub missions, time-travel depth, questions through the MCP stream.
- Second, depth past the M12 scope: the baked probe grid, material shaders from packages, 2D lights and autotiling, text in the world, tools from packages, kits.
- Third, the widest form of the bet: the small language's text form (rules v1 stays).
- Fourth, second-tier depth inside M12: inspector depth, the material editor, particle and animation preview, half the templates and starter packs.
- Fifth: several agents at once (the orchestrator first, forks and merges last).
- Last: web export, then anything in graphics or platforms inside the M12 scope (High-tier items, cross-export, Steam Deck, the macOS preview).
- Never cut: code freshness and the verification gate; the scorecard with held-out tasks, published every release; the rules experiment (its answer, whichever it is); the first migration and the compatibility corpus; the release pipeline; the stability contract; closing reviews; the 2.0 beta.

### Claims the planner verified

- All paths are under the repository root read on 18 November 2026, uncommitted work under crates/ ignored.
- 34 tools: crates/wish_control/src/tools/mod.rs, CATALOG, a compile-time constant (so a package cannot add a tool today).
- The expression language is in crates/wish_control/src/expr.rs (1,123 lines) and evaluates through a Subject trait at the tool layer; a rules system below the control plane needs it moved (dependencies point down).
- The world inspector's rules are Rust code (sessions.rs, inspect()), not data, so a package cannot carry a check today.
- packages.rs KINDS is languages, recipes, templates, themes, workspaces; MAX_BYTES is 64 MiB.
- wish_agent has two adapters, ClaudeCode and Scripted; Actor::Agent(String), base_revision and changed_by_others exist; no three-way merge of documents exists in wish_scene or wish_control.
- No read-only session role: READ_ONLY in crates/wish_mcp/src/lib.rs only sets the readOnlyHint annotation.
- sources_newer_than exists only in tools/export.rs; no ensure_code_fresh or stale-code check in wish_control.
- crates/wish_player has no replay or record code and depends on the whole of wish_control.
- No gamepad, HTTP/TLS, SHA-256 or signature dependency in any Cargo.toml; no save-game or scene-switch command for game code.
- wasmtime 50.0.1 is used only by wish_script; pulley crates are in Cargo.lock; pollster (blocking GPU calls) is used in wish_render.
- MAX_SESSIONS is 8 (sessions.rs); docs_search requires every word (docs.rs search_with uses all); recipes.md has 8 recipes.
- wen migrate is in crates/wish_cli/src/project_cmds.rs, not in wish_control, and has no migration step.
- game_design plan steps carry a status only; no acceptance field.
- asset_create already synthesizes a .wav and draws a shape .png from numbers; vello_cpu 0.2.0 is a workspace dependency.
- wish-evals run takes --tries, --model and --effort; there is no held-out set and no Haiku result in evals/results.
- evals/results/compare-2026-11-18.md: Wish 8/8, 3.4 calls per task, 0.53 $ after efficiency round 1; Unity 6.3 7/8, 6.8 calls, 0.80 $; one run per engine, eight scene tasks, no game code.
- The report's weighted totals reproduce from its weights (Wish 64 and 36, Godot 72 and 77, Unity 68 and 83), so the 2.0 totals in the 2.0 release entry use the same arithmetic.

### Not verified

- Every package count is my estimate in 3-to-5-hour units. By this count the M12 remainder alone is about 75 packages, against the note's coarser 45 to 50 for the whole scope.
- Licences and dependency trees of the candidate libraries (a gamepad library, an HTTP/TLS stack, SHA-256, a signature verifier, an archive codec, wasm-bindgen and web-sys): not checked, no network was used.
- Whether wasmtime 50's Pulley and precompiled modules behave as the area map assumes, and whether a browser's Wasm engine can be given deterministic limits by instrumentation at an acceptable cost.
- Whether Rapier state restores bit-identically from a save file.
- Whether vello_cpu rasterizes identically on Windows, Linux and wasm32 (needed for SVG sprites made at import).
- The base size of about 70 MiB: the note's estimate.
- Whether rules meet the frame budget at 10,000 entities: that is what the experiment measures.
- The area maps' transcript statistics (27% ritual calls, 12 to 28 calls for game-code tasks): not recomputed, the transcripts are outside the repository.
- Competitor facts (Unity's CLI and MCP, Unreal's MCP, Godot's policy, outside benchmark results) come from the area maps' web research and were not rechecked.
- The 1.0 scorecard does not exist yet, so its headroom and the cost of a campaign are unknown.
- The uncommitted render foundation in the working tree was not read; the plan assumes it matches its design note.
- The scores are subjective judgements on the report's scale, set against the competitor columns as they stand today.

## Plan: Close the commercial gap: 2.0 is the version with which a small team ships a commercial game and does not regret the engine. Judged on the "commercial release" total (platforms x3, maturity x3, graphics x2, ecosystem x2, editor x2), with the AI scorecard as a gate on every release. (key commercial)

**Thesis.** Wish Engine 2.0 is for a team of one to five people, with or without an agent, that wants to sell a small or medium game on desktop, the web and phones. It has what a shipped game needs and 1.0 lacks (save games, gamepads, scene switching, localisation, navigation, a 2D kit, shadows on every light, baked bounced light, a docking editor with animation, particle and material tools, code packages, Steamworks), a stability contract that has held through ten 1.x releases and one planned break with a tested migration, and the thing no other engine has: the same replay gives the same world on every platform it names. Expected "commercial release" total: about 72 with outside people, about 66 without (Godot 77, Unity 83 as they stand today; 60 at full M12 scope). The gap narrows from 17 points to about 5; it does not close. What remains is consoles, community size and years. "AI with small games": about 86 (82 without outside people). All scores are judgements, not measurements.

**One bet.** A platform, an executor or a feature is called supported only when the same committed replays give the same hashes on it, so a small team or its agent tests a game once on one machine and the proof holds for every build it ships.

**Total packages:** 126. **Calendar:** 8 to 14 months after 1.0; an estimate, not a measurement. 126 packages at 3 to 5 machine hours is 380 to 630 hours of the one code agent: about 3 months of building at two packages a day. On top: ten weeks of fixed beta time (four at 1.5, six at 2.0), the user's review sittings, and waiting on decisions (downloads and TLS block 1.3; Steam, Apple and Google accounts block parts of 1.8 and 1.9). A release about every 3 to 6 weeks; 1.5, where the full M12 scope is complete, about 3 to 5 months after 1.0. It depends most on the user's token budget, then on outside testers and devices.

### 1.1: A game you can finish (13 packages)

- Ground truth after 1.0: a real migration step in wish_control (wen migrate has none today), the recorded defaults table, a compatibility corpus of 1.0 projects opened and played in CI, snapshots of the tool catalogue and SDK prelude, one committed replay per example on both hosts, arm64 and wasm32 hash jobs as informational
- Code freshness: content fingerprint of scripts, auto-build in sim, test, replay and export; lean play path (no hash or inspector per frame in the player); templates ship built modules; no-op builds skip the compiler
- Verification gate (check at level full: build, tests, replays, seeded soak) and a background build with build-on-save as a control-plane code watch
- The presenter: swapchain, vsync, interpolation between ticks, fullscreen, window settings in the project file; readback kept as the software path with automatic fallback
- HiDPI for games and the editor; device-loss recovery with a forced-loss test
- Gamepad, player-indexed actions with quantised axes, focus navigation for in-game UI
- Replay format 2 (build id, hash every N ticks, keyframes, by path, strict); record, play and a crash ring in the shipped player
- Save games: engine-owned slots, versioned, loaded with field-by-field migration, requested by game code and visible from tools
- Scene switching at run time as a recorded step; animation cross-fade
- Editor first hour: orbit, pan, zoom, frame; Create menu; a duplicate op; new and delete scene; menu split; all shortcuts in the command table
- AI efficiency round 2 and the game-code loop: start-run-report in one call, inline tests, failure traces, ranked docs search, keep_world, look_at
- Rules experiment: prototype (trigger, condition, actions as scene data, run by an engine system) and the two-way measurement on the scorecard; decision written to DECISIONS.md
- Closing review with a fix budget, scorecard campaign, 1.1.0

Why here: Reordered from the draft, which had these in 1.2. Each item either touches a format (cheapest while few projects exist, and it brings the migration step every later change needs) or blocks any shipped game (save, scenes, gamepad, vsync). The 2D kit moves to 1.2: its pixel-perfect viewport and the editor's HiDPI stand on the presenter, and 2D is last in the user's own priority list. The plan note itself names scene switching, save games and the editor's first hour as the first things to pull forward.

Needs from the user: Approve the reorder (2D kit to 1.2); Approve a gamepad library (for example gilrs; licence and tree not checked); Spending cap for the scorecard and rules campaign; The rules decision after the results; Look at the presenter by eye on a real screen (the window loop does not run in CI)

Needs from outside people: Nothing blocks; a first informal run by one or two people is useful here

### 1.2: 2D kit and editor musts (14 packages)

- Panel registry: app.rs (5,414 lines) split into registered panels, layout as data, same picture; the base for docking and package panels
- Sprite renderer foundation: textures kept on the GPU, premultiplied alpha, order and Y-sort, sprites in the id buffer, warnings where things are dropped
- Sprite sheets, SpriteAnimation, Aseprite JSON import, picture size, frames and clips in asset_info
- 2D physics profile on Rapier 3D: plane lock as a world setting, polygon and merged tile colliders, one-way platforms, a checker rule
- Fixed viewport, pixel-perfect camera, pointer-to-world helpers, sprite picking
- Tilemaps: chunk components, tileset, paint, fill and read ops (rows of characters), chunk meshes, merged colliders
- Editor 2D mode: tile palette and painting, sheet slicing, drop a picture to make a sprite
- Nine-slice, image fit modes, clipping and scrolling for in-game UI
- Scene-view overlays and picking (grid, icons, outline, entities without a mesh) and gizmo completion (multi-entity, plane handles, snap preferences)
- Hierarchy at scale (collapse, filter, range select, reorder, virtual rows) and inspector depth (mixed values, per-field overrides, arrays, clip names)
- Asset import and management: file drop, an import action, delete to trash, folders, thumbnails, import settings that importers read, sound audition
- Console and problems with click-through to file and line, palette search over entities, assets and docs; play loop polish (sound in editor Play, keep a live edit, time scale, timeline ring, step back, proposal preview)
- Profiler: profile on sim_step, fuel per system (assertable in tests), per-pass render statistics, editor bars, wen bench --project
- 2D templates (platformer, top-down, puzzle), recipes, evaluation tasks; closing review; scorecard; 1.2.0

Why here: Needs the presenter and the editor's first hour from 1.1. The panel registry comes first so every panel after it is built once. Largest single lift for editor (4 to 6.5) and 2D (4 to 7).

Needs from the user: A small CC0 tileset and character, or another art source; Try tile painting and the 2D view by hand; Spending cap for the campaign

Needs from outside people: Nothing blocks

### 1.3: Every desktop, small base (12 packages)

- Fetch layer: HTTPS GET with a size cap, SHA-256, an ed25519-signed index, checked unpack; ADR-022 and the privacy wording amended first
- Components: a components tool; toolchain, sdk and export.<target> components; base (about 70 MiB, an estimate) and full offline bundles from the release pipeline
- Online package index, engine downloads and a signed updater on the fetch layer
- Lean runtime: session build and step in a crate the player and Workspace share; the player no longer links tools, docs and scaffold; strip and LTO with unwinding kept
- Virtual project files: one read interface (folder or deterministic archive); an exported game ships one packed file; the asset fingerprint is unchanged
- Executor seam: modules precompiled at export, a player without Cranelift, one interface another executor can fill
- Cross-export Windows to Linux and back through player components (project_export target); verify: true runs the export headless and compares the hash
- Windows export polish: the game's icon and version resource written into the player
- Steam Deck: the Linux export in the Steam Linux Runtime in CI, 1280x800 fullscreen, gamepad actions in templates, a checklist
- macOS arm64 preview: CI job, .app bundle, Command key, ad-hoc signing, recorded hashes and replays on arm64
- First run: doctor, a folder picker, wen help for users, AGENTS.md; rules v1 if the experiment said yes (game logic in a base without the compiler)
- Closing review, scorecard, 1.3.0

Why here: Players become components here, so the lean runtime, virtual files and the executor seam are done in the same release, once; they are the root of web and phones. It waits on the download decisions open since M10b: if they are still open, 1.4 runs first.

Needs from the user: Download address and the index repository's owner; Approve an HTTP/TLS library that fits deny.toml (none is in Cargo.lock today); Custody of the signing key (CI secret); A folder-picker dependency or own code; Windows code-signing certificate (optional, money)

Needs from outside people: One report from a real Steam Deck before the word playable; One person with a Mac to open the preview

### 1.4: High tier and scale (10 packages)

- Environment maps: guarded .hdr import, CPU prefilter cached under .wish/, skybox, an environment asset kind
- Frustum culling (view and per cascade), draws sorted by pipeline and material, instancing, cached sprite and UI atlases; measured with wen bench --render
- GPU skinning in the lit, shadow, id and debug pipelines; morph targets
- Spot and point shadows in a shadow atlas, with a cap and a warning
- Ambient occlusion with a fixed pattern and no history; an ao debug view
- Anti-aliasing: MSAA 4x if the two-adapter measurement allows it, else TAA with a fixed warm-up in captures; ADR-012 records which
- Particles and glTF coverage: textured, additive and soft particles; texture transform, sampler modes, vertex colours, unlit
- Simulation scale: SDK slice views, read-only columns not copied back, Changed and Added filters, transform dirty tracking; fuel budgets asserted in CI
- Simulation thread and render thread split (no effect on results); Low, Medium, High tiers and render scale as data
- Closing review, the gallery looked at by the user, scorecard, 1.4.0

Why here: Before the docs and showcase work, so screenshots, gallery and tutorials are made once with the final 1.x look. It needs no account or download decision, so it is the release that runs when 1.3 waits. Graphics 5 to 6; every item is a non-default setting, so no scene changes meaning.

Needs from the user: Look at the gallery and the Khronos models by eye; Decide anti-aliasing if the MSAA measurement fails

Needs from outside people: Nothing blocks; other GPUs and drivers are only seen by outside people

### 1.5: Prove, teach, package (full M12 scope complete) (13 packages)

- Tool profiles (a core of at most 25 of today's 34 tools), a read-only role, batch, typed op schemas
- Other agents: connection snippets, a generic command-line adapter, --agent in both runners; the cockpit agent's context tuned
- Hard evaluation tier (whole small games, bug fixes, multi-scene work), isolated per-try folders, three tries; a seeded play-test bot
- Quality mode: acceptance on plan steps; plan, build, prove, review, repair tested with Scripted adapters; Fast, Balanced, Thorough; a spending cap
- Verified runs: replay verify, a signed verdict, batch wen verify, a first-party package with a server example
- Packages carry assets and prefabs (read-only roots, export copies them, attribution reaches LICENSES.txt); dependencies in the manifest and wish.lock
- Author tooling: wen pkg new, index, pack, publish; an authoring page for people and agents; a package's tests run before publish
- Seeded index: starter packs (props, UI kit, sounds, an animated character), genre templates, recipe packs
- Docs: wen docs --out, a reference site from schema, tools, diagnostics and SDK, an offline copy, in-editor help and Explain
- Tutorials executed in CI (first 3D game by hand, with AI, first 2D game), concept pages, known limits, coming-from pages; the first lesson in Turkish
- Showcase games re-dressed with the High tier, a feature gallery with goldens, a public quality page per release, Hub missions
- Stability contract in full (formats, tools, SDK, codes, manifests; stable and experimental labels; deprecation and support windows); project files for external code editors
- Four-week beta (ten outside first runs, the three-person test), closing review, scorecard, 1.5.0

Why here: After 2D, editor, desktop and High tier, so tutorials, the contract and the index describe a product that holds still. Largest weighted lift of any release: maturity 3 to 4.5, ecosystem 1 to 3, AI 9 to 9.5. The long beta starts the maturity clock that only calendar time runs.

Needs from the user: Where the art comes from; Enable Pages and Discussions; a place to talk; Spending cap for the hard tier and quality-mode runs; Approve installing other agent programs; The name must be final before the docs site is public

Needs from outside people: Ten or more beta first runs on named platforms; Three people who have never seen the engine; First outside package authors (ecosystem above 3 needs them)

### 1.6: Web, and 2D round 2 (11 packages)

- Non-blocking GPU path for wasm32 (Gpu::checked without block_on); ADR first
- wasm32 build of the lean runtime; world hashes and committed replays pass in Node in CI
- Game code in the browser: Pulley compiled into the player or the browser's Wasm engine with injected fuel, chosen by measurement; fuel differences decided in an ADR; hostile-module tests
- Web player: canvas surface, input, audio after a gesture, the packed project fetched as one file
- project_export target: web, a local preview server, headless-browser screenshot and hash check in CI
- WebGL2 fallback (lights without storage buffers) only if reach data asks for it; player size work
- 2D lights with an ambient tint, normal-mapped sprites, occluder shadows
- Autotile rules; Tiled and LDtk import
- 2D effects: textured, additive and shaped particles in layers; parallax; repeating backgrounds; a follow-camera helper; world text with outline
- Web-playable examples on the docs site; a share action that exports for web and writes a page
- Closing review, scorecard, 1.6.0

Why here: Needs the presenter (1.1) and the lean runtime, virtual files and executor seam (1.3). First new platform after the desktop trio because it needs no account, device or signing and CI can verify it. 2D round 2 rides along: the web is where 2D games ship. VISION.md and ADR-019 still call web the headline of 1.1 and must be corrected.

Needs from the user: Approve correcting VISION and ADR-019 (web moves from 1.1 to 1.6); Hosting for web builds; WebGPU only, or the WebGL2 fallback too

Needs from outside people: People on other browsers and on phones' browsers

### 1.7: The workshop: docking, specialist editors, the systems they edit (13 packages)

- Docking: drag, split, float, maximise, saved layouts in workspace documents; accessible names per row
- Background jobs: build, test, export and long steps off the frame with progress and cancel; MCP progress stream
- Animation blending and a state machine as a data asset, root motion, two-bone IK
- Animation editor: timeline, clip preview, state machine view
- Particle curves and gradients as fixed keys; a particle editor with a preview time
- Material editor with a preview sphere and a shared library; in-game UI layout on the canvas
- Navigation: deterministic navmesh bake cached under .wish/, path queries as a host service and in the SDK, a nav debug view
- Physics joints, sensors, solver settings and documented budgets
- Game localisation: string tables (CSV or PO), locale switch, font fallback for CJK and right-to-left text
- Audio depth: streamed music, a bus mixer with low-pass and reverb
- Visual debugging and time travel: debug draw from game code, a watch panel, go-to-tick, a diff between ticks, who wrote this value
- Each new system ships with recipes, evaluation tasks and parity rows (counted as one package)
- Closing review, scorecard, 1.7.0

Why here: After the panel registry (1.2) and after the 1.5 beta has said what hurts in daily use. Each game system lands together with its editor, so it is editable by hand the day it exists (invariant 9). Navigation, joints, blending and localisation are absent from the code today and from the area maps; a commercial game needs them.

Needs from the user: Docking: a crate (licence and egui 0.37 fit not checked) or an own splitter tree; Add OFL-1.1 to the allowed licences for a CJK and right-to-left font; Listen to the audio effects; try the editors by hand

Needs from outside people: Daily users of the editor: 7.5 instead of 7 needs their reports

### 1.8: Code you can share (11 packages)

- Kits: a game package kind compiled as an rlib and passed with --extern, a namespacing rule for type names, source_check on package sources, hostile tests
- First kits taken from the examples (character controller, inventory, dialogue, follow camera), each with scenario tests and evaluation tasks
- wish_ext tool plugins: prebuilt Wasm, tools declared with Param tables, nested calls by return-and-resume, actor ext:<package>, permissions shown at install
- Panels and importers from packages on the panel registry (declared widgets, no native code)
- Native SDKs as engine-built player variants; a Steamworks variant (achievements, cloud saves on save slots, overlay, input glyphs) as a component outside the repository
- Publishing: signing keys, wen pkg publish to the index repository, provenance fields (licence, AI-made), a gallery page generated from the index
- macOS as a signed, notarised host and target; Windows signing in the release pipeline
- Agent packages and task packs: a package can ship evaluation tasks that show an agent can use it
- Direct model adapter (a model API or a local endpoint through the fetch layer; keys in the OS store)
- Rules, second decision on the scorecard: grow into a small budgeted language, or stop
- Closing review, scorecard, 1.8.0

Why here: After the stability contract (1.5): kits and plugins turn the SDK and the tool surface into a contract with third parties. After the fetch layer (1.3). The account-bound items (Steamworks, Apple, signing) sit together so one round of the user's paperwork unblocks them.

Needs from the user: Steamworks account, Steam Direct fee, accepting the Steamworks SDK licence; Apple Developer account (yearly fee); a Windows certificate; Index rules: licence policy, takedown, AI provenance; The second rules decision

Needs from outside people: Package and kit authors: ecosystem 4.5 instead of 3.5; A Mac owner to confirm the notarised build

### 1.9: Phones, and the console seam (9 packages)

- Touch input (pointer and multi-touch actions), a virtual stick widget, app lifecycle (suspend, resume, surface recreated)
- Mobile render tier: Low preset tuned, render scale, ASTC and ETC2 textures through a transcoder as an optional component
- Android player per ABI, assets from the APK through virtual files, hash checked in an emulator in CI
- APK and AAB assembly and signing by the engine, or the Android SDK as a component: decided by a spike
- iOS player on the interpreter (no JIT), a simulator job on a macOS runner; device build and signing documented, export from a Mac; labelled preview
- Store checklists, icons and splash, safe areas, orientation, UI scaling for small screens
- Console seam: window, surface, audio, input, files and executor behind interfaces; a porting guide; the partner model stated in the docs
- Low-end performance: frame budget measured on a named phone and on a Steam Deck; fixes
- Closing review, scorecard, 1.9.0

Why here: Needs everything web needed plus touch, lifecycle, an interpreter for iOS, a mobile tier, devices and store accounts. It is the most outside-dependent release, so nothing else waits behind it.

Needs from the user: Google Play account; approve an Android SDK/NDK download if the spike needs it; Approve a texture transcoder dependency; A phone to test on, or testers

Needs from outside people: Testers with an Android phone, an iPhone and a Mac: without them both targets stay preview; A console porting partner (cannot be planned)

### 1.10: Light that bounces (11 packages)

- Reflection probes: baked cubes, box projection, blending; cached
- Screen-space reflections with a fixed pattern, falling back to probes
- Global illumination: baked irradiance probe volumes (deterministic bake under .wish/), a gi debug view; lightmaps for static meshes if probes fall short
- Volumetric fog and light shafts; decals
- TAA and an upscale path (if not chosen in 1.4), auto exposure, depth of field; each with a fixed warm-up rule for captures
- Custom material and post-effect shaders from projects and packages, hot reloaded, safe after device loss; WESL looked at again (ADR-013)
- Mesh LOD generated at import, conservative CPU occlusion culling, BCn textures on desktop
- HDR output where the surface allows it; a tone-map choice
- Terrain as an optional package: heightfield mesh, splat material, collider
- Look presets rebuilt on the new features; the user reviews the gallery by eye
- Closing review, scorecard, 1.10.0

Why here: Heaviest by-eye review and the smallest weighted lift per package (graphics 6 to 7 is 1.2 points of the commercial total), so it comes after platforms, editor and ecosystem. It also prepares the one default-look switch that only the format window of 2.0 may make.

Needs from the user: Sign off the new look by eye; Art for the gallery scenes

Needs from outside people: Artists or art packs: graphics 7 instead of 6.5 is judged from what games look like

### 2.0: The second format window (9 packages)

- Format window 2: fields added across 1.x consolidated, deprecated keys removed, each change with a migration; wen migrate proven on the compatibility corpus and on outside projects
- Look lock 2: the default look switched once to the bounced-light look, signed off by eye; goldens, thumbnails and gallery refreshed in that one change
- Tool surface 2 and SDK source API 2: only what 1.x marked deprecated is removed; profiles become the default
- Determinism matrix: recorded hashes and committed replays on Windows, Linux, macOS arm64, web, Android and iOS; a platform that fails is labelled preview
- Public multi-engine benchmark: engine-neutral tasks and checks, Godot and Unity adapters, several agents, published transcripts, an outside reproduction
- A 1.x long-term-support branch with a stated window; the 1.x to 2.0 upgrade guide tested on the corpus
- Docs for 2.0: a versioned site, a manual for every shipped feature, the console page, two finished sample games with sources
- Beta of six weeks with an exposure floor per platform, the formal user test, release candidates
- Independent closing review, final scorecard on held-out tasks, 2.0.0 and a planned 2.0.1

Why here: After 1.0 a format or a default may change only by addition or with a migration (ADR-027): a major version is the one place to consolidate, and the only place the default look may switch. No features merge here.

Needs from the user: Approve the list of what breaks; The length of the 1.x support window (a promise of his time); Spending cap for the benchmark; approve installing Godot and other agents; Sign off look lock 2

Needs from outside people: Beta users on every named platform; An outside party that reruns the benchmark; Shipped or in-progress games by other people: maturity 6 instead of 5

### Scores at 2.0

| Area | With outside people | Without | Basis |
|---|---|---|---|
| Graphics quality | 7 | 6.5 | 1.4 (culling, instancing, GPU skinning, shadows on every light, AO, AA) and 1.10 (baked probe GI, reflection probes, SSR, volumetrics, custom shaders) reach Godot's feature list without real-time GI; 7 needs showcase art and other people's GPUs, and Unity 7's real-time GI will move the scale. |
| Platform support | 7 | 6 | Presenter and gamepad (1.1), desktop trio with cross-export and Deck (1.3), web (1.6), signed macOS (1.8), Android and an iOS preview plus the console seam (1.9); Godot's 8 needs partner console ports and years on devices, and without devices and accounts phones and macOS stay CI-verified previews. |
| Ecosystem and community | 4.5 | 3.5 | Online index (1.3), asset packages and seeded packs (1.5), kits, tool plugins, Steamworks and publishing (1.8); everything above 3.5 is packages and answers from outside authors. |
| Maturity and docs | 6 | 5 | Migration step and compatibility corpus (1.1), contract, generated docs, tutorials run in CI and the long beta (1.5), then a second format window with proven migrations and a 1.x support branch (2.0); the maturity half needs strangers, shipped games and about a year. |
| Editor tools | 7.5 | 7 | First hour (1.1), musts and profiler (1.2), docking, animation, particle, material and UI editors with background jobs (1.7), plugin panels (1.8); Godot's 8 needs a script debugger and hardening by daily users. |
| 2D games | 8.5 | 8 | 2D kit (1.2), 2D lights, autotiling and Tiled/LDtk/Aseprite import with web export (1.6), phones (1.9); 9 needs 2D skeletal animation and years of polish, and mobile is unproven without devices. |
| Determinism, replay, tests | 10 | 10 | Held at 10 by the determinism matrix on every new platform and executor (1.3, 1.6, 1.9, 2.0); replays in the shipped player (1.1) and verified runs (1.5) make it visible to players. |
| Working with AI | 10 | 9.5 | A scorecard campaign gates every release; efficiency round 2 and the gate (1.1), quality mode, hard tier and other agents (1.5), task packs and a direct adapter (1.8), the public benchmark (2.0); 10 needs an outside reproduction, and Unity and Epic move fastest here, so against their 2027 state the lead is narrower than the number. |
| Code iteration speed | 9.5 | 9.5 | Freshness, background build and prebuilt modules (1.1) give 9; 9.5 only if the rules experiment says yes and logic can be written without the compiler (1.3, 1.8); 9 if it says no. |
| Lightness and install | 9 | 9 | Base of about 70 MiB with components, a lean player and precompiled modules (1.3); below Godot's 10 while Rust game code needs a compiler component of about 400 MiB; the base size is an estimate. |
| Licence and cost | 10 | 10 | Unchanged: MIT + Apache-2.0, no fee, no commission; proprietary SDKs stay optional components outside the repository. |

### Not in 2.0

- Real-time multiplayer and rollback netcode: needs relay infrastructure, a security review and every executor proven bit-identical first; 2.0 ships what it stands on (player-indexed input, replay 2, the determinism matrix)
- First-party console ports: console SDKs are under NDA and cannot live in an open repository; 2.0 ships the seam and a porting guide, a port comes from a partner
- Real-time global illumination, ray tracing, virtual geometry, GPU-driven drawing: they leave the WebGPU core and the software-adapter test path; baked probes are the answer that fits a small engine
- Visual shader graph and node-based scripting: each is milestone-sized; text shaders, rules and an agent cover the need first
- A code editor and a breakpoint debugger inside the engine: external editors get project files; time-travel tools (go-to-tick, who wrote this value) come first
- A second general scripting language (beyond what the rules decision gives): it doubles SDK docs, recipes and evaluation tasks
- Parallel system scheduler: the highest determinism risk on the list; the simulation and render thread split gives most of the headroom
- Rapier 2D backend, 2D skeletal animation, hex maps: narrower use; a benchmark has to ask for them
- Paid asset store, ratings and reviews: payments, moderation and legal work are company-level decisions for the user
- A kernel anti-cheat: proprietary and per platform; verified runs are the engine's own answer
- Translated diagnostics and reference docs: translations rot fastest, and agents and tests need one language (localisation of games is in 1.7)
- FBX import: proprietary format; the documented path is Blender to glTF
- Editor in the browser or on tablets, XR, Windows on ARM: no demand measured yet
- Generative models inside the engine: out of scope by VISION; the user brings their own AI

### Risks

- Size: about 126 packages for one owner with limited tokens, after he rejected 45 to 50 before a first release Mitigation: Every release stands alone and ends shippable; 1.6 to 1.10 can be reordered or dropped without breaking each other; re-plan at each release with the scorecard and the re-scored table
- A second executor (Pulley or a browser engine) counts fuel differently, so a runaway system stops elsewhere and hashes differ Mitigation: ADR before code; supported is claimed per platform and executor pair; replays carry the executor's id; verified runs stay on x86-64 native until the matrix passes
- Float drift on arm64 and wasm32 (std sin and cos in particles, glam, Rapier) is unproven Mitigation: Informational hash jobs from 1.1; a platform that fails stays preview; simulated paths use only operations that match
- Web blockers in today's code: block_on in every GPU check, wasmtime as the only executor, three storage buffers for lights Mitigation: Non-blocking GPU path as its own package with an ADR; WebGPU first; the WebGL2 fallback only if reach data asks
- After 1.0 changes must be additive: fields pile up, and a 1.0 engine rejects unknown manifest keys (MANIFEST_KEYS) and unknown types Mitigation: Migration step and compatibility corpus first (1.1); packages state an engine lower bound; one planned break at 2.0 with a tested migration
- History-based effects (TAA, auto exposure) break repeatable captures and goldens Mitigation: A fixed warm-up is part of the capture request; goldens stay on a fixed setting; MSAA preferred if the two adapters agree
- The owner's eye and ear are the bottleneck (looks, gallery, audio, devices) Mitigation: One review sitting per release with headless pictures prepared; only the two look locks wait on a sign-off
- Maturity and ecosystem cannot be coded Mitigation: Betas at 1.5 and 2.0, a quiet pre-release each minor, one-step publish, packages an agent can author; scores are reported with and without outside people
- The AI lead erodes while the work goes to platforms and graphics; the 41-task suite is already saturated Mitigation: Held-out tasks, two models and the long task every release; a release does not ship if correctness or errors got worse; every new subsystem lands with recipes, tasks and parity rows; core tool profile of at most 25
- The editor is CPU-rasterised (4.9 ms of a 6 ms budget at 1080p): docking, 4K and timelines may not fit Mitigation: Measure in the HiDPI package of 1.1; dirty-region painting; decide a compositing fallback inside wish_render (ADR-018) before 1.7
- Kits and plugins create lasting ABIs and a new trust boundary Mitigation: Experimental labels until 2.0; Wasm sandbox only, no native plugins; permissions at install; hostile tests in each closing review
- macOS, iOS and Android cannot be verified without hardware and accounts Mitigation: Preview label until a named person reports from a named device (ADR-027 rule 4); CI covers build, hashes, emulator and simulator
- Competitors move: Unity 7 (real-time GI, WebGPU) is announced for early 2027, Godot ships twice a year Mitigation: Scores are against today's columns and say so; re-score against current competitors at every release
- More platforms multiply CI time, and the machine has one build folder Mitigation: Heavy targets in a nightly matrix; the clean-build script after every package; one code agent at a time stays the rule

### Decisions that are the user's

- The reorder: runtime musts (presenter, gamepad, save games, scene switching) in 1.1 and the 2D kit in 1.2; High tier (1.4) before prove-teach-package (1.5); web at 1.6
- Correct VISION.md, ADR-019 and ROADMAP, which still say web export is the headline of 1.1, before the first page of 1.0 repeats it
- The name, before the 1.0 release candidates (file names carry it) and before a docs site is public
- Download address and the index repository's owner; an HTTP/TLS library (new dependency; blocks 1.3)
- New dependencies, each needs approval and a licence check: a gamepad library, a folder picker, a docking crate or own code, an .hdr decoder, a texture transcoder (KTX2/Basis), a navmesh library or own code
- Add OFL-1.1 to the allowed licences for a CJK and right-to-left font
- Signing: custody of the ed25519 release and index key; a Windows code-signing certificate (money)
- Apple Developer account (yearly fee) for notarised macOS and for iOS
- Google Play account; approval for an Android SDK/NDK download if the spike needs one
- Steamworks account, the Steam Direct fee, and accepting the proprietary Steamworks SDK licence as an optional component
- Devices or testers: a Steam Deck, a Mac, an Android phone, an iPhone, a HiDPI screen
- Where the art comes from: showcase games, starter packs, gallery scenes, a tileset and character
- A spending cap per measurement campaign (eleven campaigns, one per release) and for the public benchmark; approval to install other agents and Godot
- Outside people: ten or more first runs for each beta, the three-person test, package authors, someone to rerun the benchmark, a console porting partner
- Hosting and accounts: Pages for the docs site and web builds, Discussions or a chat space, a domain
- Rules: enter or not after the 1.1 experiment; grow into a small language or stop at 1.8
- Anti-aliasing if the MSAA measurement fails (TAA changes the capture rule)
- Index rules: licence policy, takedown, labelling of AI-made content; the wording of plugin permissions
- What breaks at 2.0, and the length of the 1.x support window
- Look lock 2 by eye; audio by ear on each platform
- Merging to main and the release cadence; whether minors may be merged or skipped if tokens run short

### Cut order

- Cut first, no score row and not asked for: terrain package, HDR output, depth of field, direct model adapter, agent packages, Hub missions, time-travel depth
- Then second-tier depth: the WebGL2 fallback, panels from packages (keep tool plugins), audio depth, screen-space reflections (keep probes), volumetric fog and decals
- Then iOS (keep Android), then navigation and joints (they can come later as engine updates without a format break)
- Then 1.10 as a whole moves after 2.0: graphics stays at 6 and the commercial total loses about 1 point
- Last: phones (1.9), which costs about 2 points and the most outside help
- Never cut: the migration step and compatibility corpus, the determinism matrix rule, the scorecard campaign per release, the presenter, save games, scene switching, gamepad, the fetch layer and components, the lean runtime and executor seam, the stability contract, the betas, the look locks, the closing reviews

### Claims the planner verified

- Recomputed both weighted totals from the score table: 36, 60, 77 and 83 (commercial) and 64 (AI with small games) match the given numbers; the 2.0 totals of 72, 66, 86 and 82 use the same weights
- wish_player depends on wish_control, so the player links the whole control plane (crates/wish_player/Cargo.toml)
- Sound output in the player is a cfg(windows) dependency feature at HEAD (same file)
- wasmtime is built with runtime and cranelift only; pulley-interpreter 50.0.1 is in Cargo.lock but not enabled (Cargo.toml lines 80-88)
- GPU setup and every check block: pollster::block_on and PollType::Wait in crates/wish_render/src/gpu.rs (lines 93, 119, 191-193, 226)
- The device is opened with Features::empty() and downlevel_defaults; lights use three storage buffers (shaders/lights.wgsl 18-21)
- softbuffer is used only in wish_window (shell.rs, viewer.rs)
- No gamepad or touch code in wish_input, wish_window or wish_types
- No save-game or scene-switch command in wish_sdk, wish_types or wish_control
- Replay limit is MAX_TICKS = 54_000 (crates/wish_control/src/replays.rs:29)
- Only DirectionalLight has a shadows field (crates/wish_types/src/render.rs); PointLight and SpotLight have none
- Skinning is on the CPU (wish_import::anim::skin, Renderer::pose); AnimationPlayer holds one clip with no blending
- No navigation, joint, game-localisation, breakpoint or netcode code in any crate (searched)
- Package KINDS are languages, recipes, templates, themes, workspaces; limit 64 MiB; unknown manifest keys are rejected (crates/wish_control/src/packages.rs)
- tools::CATALOG is a compile-time constant with 34 tools; no wish_ext code exists
- crates/wish_editor/src/app.rs is 5,414 lines; no docking or panel registry in wish_editor
- Cargo.lock has no HTTP or TLS client, no gamepad library, no file dialog and no docking crate; android-activity and ndk are present through winit
- deny.toml allows MIT, Apache-2.0, BSD, BSL-1.0, ISC, Unicode-3.0, Zlib; no MPL and no OFL
- CI runs on windows-latest and ubuntu-latest only; the repository has no git tag; the workspace version is 0.1.0
- project_export names the program with cfg!(windows) and has no target parameter (tools/export.rs 89, 223)
- wish_agent has claude.rs and scripted.rs only; the expression language exists (wish_control/src/expr.rs, 1,123 lines)
- docs/design/M12-render-foundation.md reports the render foundation done and uncommitted, with wen bench --render at 5.5 ms for 2,500 surfaces on the development GPU (read, not rerun)
- VISION.md and ADR-019 state that web export is the headline of 1.1

### Not verified

- Every size: package counts, hours, the calendar, and the base of about 70 MiB
- All eleven scores and both totals are judgements on a subjective scale
- Whether Pulley is fast enough for game code, and how its fuel compares with Cranelift's
- Whether wgpu 30 and this renderer run on wasm32 with WebGPU or WebGL2; browser reach of WebGPU in 2027
- Determinism of the engine's float code and Rapier on arm64 and wasm32
- Whether the engine compiles on macOS at all; whether iOS can be exported without a Mac
- Licences and fit of every proposed dependency (gamepad, TLS, folder picker, docking, transcoder, navmesh, .hdr decoder); Apple signing tools I recall as MPL-licensed, not checked
- Competitor facts: taken from the area maps' web research, which is agent-written and partly from search summaries; Unity 7's date and features are announcements
- Whether the editor's CPU rasteriser holds docking and 4K inside its frame budget
- Whether a read-only session role exists (only per-tool readOnlyHint was seen in wish_mcp)
- Whether CI is green at the current commit
- Uncommitted work under crates/ was ignored as instructed, so what 1.0 has already changed (audio on Linux, render passes) was read from the design note only
- Not read: area-map lines 1015 to 1117 (launch research) beyond the first lines, and the M10, M8b and M9 design notes
- No build, test or engine command was run; nothing in the repository was changed

## Plan: Capacity and risk: the plan one owner with limited tokens can finish, packages counted honestly, ordered by dependency, measured before built. Finding 1: one score point is worth, over both weighted totals together, platforms 2.5, maturity 2.5, AI 2.4, iteration 2.0, ecosystem 1.9, editor 1.9, graphics 1.6, 2D 1.3, lightness 1.0. Finding 2: best gain per package is the desktop platform release (about 12 packages for +2 platforms and +1.5 lightness), then docs and content, then editor and 2D. Finding 3: popular items that buy almost nothing on the totals: High-tier effects after the look lock (about 9 packages for half a graphics point), docking, 2D lights, netcode, parallel systems, a second scripting language. Finding 4: the M12 remainder is about 86 packages when an L item is counted as two, not the roughly 35 the old estimate implied. (key capacity)

**Thesis.** Wish Engine 2.0 is a free, open-source game engine for one person, an AI agent, or both together, to make small and medium 3D and 2D games and ship them to Windows, Linux, Steam Deck and the browser from one machine. What sets it apart is proof: the simulation is deterministic, so one free call says whether a game still works, a replay file reproduces any bug, and a server can re-run a player's run to confirm a score. It works fully by hand with no AI connected. It is not for teams that need consoles, phones, a large asset store or AAA graphics; at 2.0 it still trails Godot and Unity on platforms, maturity and ecosystem, because those need people and years, not code. Expected weighted totals at 2.0 (same arithmetic as the report): 'AI with small games' about 83, or 80 without outside people (Godot 72, Unity 68); 'commercial release' about 68, or 63 without (Godot 77, Unity 83).

**One bet.** That proof, not generation, is what makes AI useful for games: one deterministic gate that says a game works, giving the same bits on Windows, Linux and in a browser, with that claim re-measured on held-out tasks at every release.

**Total packages:** 113. **Calendar:** About 6 to 10 months after 1.0; if 1.0 lands in late December 2026, that is June to October 2027. All of it is an estimate, not a measurement. It depends on: (1) packages per week the owner's tokens allow, the main factor: 113 packages is 23 weeks of building at 5 a week, 14 weeks at 8; (2) the four-week beta before 2.0 and whether ten outside people come; (3) how fast decisions arrive (downloading, art, accounts, the rules yes or no); (4) the executor probe: a bad result costs 2 to 4 more packages or moves the web past 2.0. Each release is 1 to 3 weeks of building plus a one-week release candidate that overlaps the next release. Adjustments to the count: +3 to +6 if the rules experiment says yes, -1 if MSAA fails its measurement, +1 with an Apple account.

### 1.1: The loop and the proof (11 packages)

- Code freshness: content fingerprint of scripts; auto-build in sim_start, test_run, replay and export; skip the compiler when nothing changed; templates ship built modules (instant first Play)
- Lean play path (no hash or inspector per frame in the player); fuel per system through sim_step profile; CI probes: committed replays on Windows and Linux, wasm32 and macOS arm64 hash jobs (informational), a window smoke test
- Verification gate: check level full (fresh build, every scenario test, saved replays, seeded soak, a camera that sees something); Health panel and wen check call it
- AI efficiency round 2a: no ritual calls, small scenes answer values, sim_start runs and closes in one call, keep_world, look_at, nested enum names, clip names and picture sizes in asset_info
- AI efficiency round 2b, the game-code loop: docs_search ranks and returns whole sections, one resource per recipe, inline tests, failure traces with min, max and tick
- Background build, build on save, a code watch in the control plane, Problems tab filled automatically
- Rules experiment, prototype: rules as data in the scene file (trigger, condition, actions) on the grown expression language, run by an engine system, no compile step
- Rules experiment, measurement: the same tasks as Rust and as rules, two models, judged by the scorecard; the same campaign re-measures round 2
- Editor first hour (2 packages): orbit, pan, zoom, frame (orthographic too); Create menu; a duplicate operation and copy/paste; new and delete scene; menu split
- Independent closing review with a fix budget; scorecard published with its command

Why here: The cheapest release: no new dependency and no open decision except quota. It makes every later content package and agent session correct and cheaper, gives later packages the gate to exit through, and measures the rules bet before the format window opens in 1.2. The 2D kit moved out of the draft's 1.1: it stands on gamepad, the presenter, editor picking and asset import, which come later.

Needs from the user: A spending cap or approval for two measurement campaigns; The rules yes or no, with the results in hand; A by-hand try of the editor first hour

### 1.2: The format window and the screen (12 packages)

- Migration machinery: the first real wen migrate step in wish_control, and a compatibility corpus (frozen 1.0 templates opened, checked and played in CI)
- Stability contract in full: docs/STABILITY.md, stable and experimental labels, snapshot tests of the tool catalogue and the SDK prelude
- Player-indexed actions and replay format 2 (build id, strict mode, by path, a hash every N ticks, keyframes); format 1 still read; migration
- Gamepad (2 packages): buttons and quantised axes in the input map, in sim_step input and in replays; input-map editor; focus navigation for the in-game interface
- Save games: engine-owned slots written from a snapshot, versioned canonical format, migrated by field name, visible from the control plane
- Scene switching at run time as a recorded deterministic step; animation cross-fade
- Presenter (3 packages): swapchain with vsync inside wish_render; interpolation between ticks, fullscreen, window settings, HiDPI; automatic software fallback and device-loss recovery with a forced-loss test
- Replays and a crash ring of recent input in the shipped player
- Closing review, fix budget, scorecard

Why here: Everything that changes a format, a hash or how the picture reaches the screen is done once, while few 1.0 projects exist: after 1.0 every later break costs a migration. Verified runs, Steam Deck, the 2D viewport, tutorials and the web all stand on these. If the rules experiment said yes, its 3 packages start here.

Needs from the user: Approve a gamepad library (new dependency) or own XInput/evdev code; a gamepad to test in hand; Eyes on smoothness and fullscreen: the window loop does not run in CI

### 1.3: Desktop everywhere (12 packages)

- Components (2 packages): compiler, SDK pack and export players as optional parts, from a local archive first; components tool, wen component, Hub tab; --base (about 68 MB) and --full packages; strip and LTO
- Download layer (2 packages; starts when the user decides, nothing else waits for it): HTTPS fetch with a size cap, SHA-256, signed index, checked unpack; used by components, packages, engines and the updater
- Cross-export: project_export target (windows-x64, linux-x64, macos-arm64) copies a prebuilt player; Export dialog
- Steam Deck: export run in the Steam Linux Runtime in CI, 1280x800 fullscreen, gamepad actions in the templates, a check list; the game's icon and version in the Windows program
- macOS arm64 unsigned preview (2 packages): CI job, .app bundles, Command key, ad-hoc signing, recorded hashes and replays on arm64
- Quality tiers Low/Medium/High, a render scale, and the MSAA measurement on WARP and lavapipe
- Docs generator and reference site (2 packages): wen docs --out from the schema, tool tables and diagnostics; offline copy in the package; a Help panel in the editor
- Closing review, fix budget, scorecard

Why here: Platforms are the user's first priority and the best gain per package in the plan. It needs the presenter and gamepad of 1.2. The docs site is here because maturity needs calendar time: that clock should start early.

Needs from the user: Downloading: index repository name and owner, the HTTP/TLS library, a signing key as a CI secret; Where the base and extras line runs; Enable GitHub Pages; Optional: a Windows code-signing certificate

Needs from outside people: A Steam Deck owner, before the word playable is used; A Mac (arm64) user to open the preview; One or two informal first runs, one of them on a Linux desktop

### 1.4: By hand (12 packages)

- Overlays and picking (2 packages): grid, axis, icons; click lights, cameras, sprites and interface in the view; silhouette outline; an overlays parameter on capture_frame
- Gizmo completion: multi-entity transform, plane handles, snap settings, snap to surface
- Hierarchy at scale: collapse, filter, range and marquee select, reorder, hide and lock, virtual rows
- Asset import and management (2 packages): OS file drop and an import tool, delete to trash, folders, thumbnail cache, import settings that importers read, previews and sound audition
- Console, problems, search: filters, click-through to file and line, go-to in the palette, every shortcut in COMMANDS
- Play loop: sound in the editor's Play, keep a live edit, time scale, a timeline ring with go-to-tick; picture preview of agent proposals
- Profiler: per-system time (kept outside the world) and fuel, render statistics, wen bench --project
- Long calls off the frame: build, tests, export and replay with progress and cancel
- First-run robustness: a doctor action, folder picker, wen help for users, AGENTS.md
- Closing review, fix budget, scorecard

Why here: The hybrid promise and every outside tester need an editor that survives the first hour, and the content of 1.5 and 1.6 is made with these tools. Long calls come after the background build of 1.1. A natural stopping point: after 1.4 (47 packages) platforms, the editor and the loop have most of their gain.

Needs from the user: A by-hand trial; golden images by eye; sound by ear; Approve a folder-dialog library (new dependency) or own code

Needs from outside people: The formal three-person test, first round: people who have never seen the engine

### 1.5: Share and teach (11 packages)

- Packages carry assets and prefabs: read-only asset roots, export copies what is referenced, attributions reach the exported game, per-kind size limits
- Package dependencies in wish.lock; author tooling (wen pkg new, index, pack); an authoring page for people and for agents
- Environment maps: .hdr to SH and a prefiltered cube on the CPU, a new asset kind, drawn as the sky
- Showcase games re-dressed with the 1.0 look and environment maps
- Feature gallery: one small scene per feature, golden-tested, with a generated page
- Seeded index (2 packages): starter asset packs (props, UI kit, sounds, a character), 3D genre templates, recipe packs for enemies, health, spawning, menus, timers
- Tutorials executed in CI (2 packages): first 3D game by hand, first game with an agent; English, first lesson in Turkish; pictures from the headless editor
- Honest pages: known limits, coming from Unity, Godot or Bevy, a launch kit; hand-written code in an outside editor (rust-project.json, SDK sources) after a probe
- Closing review, fix budget, scorecard

Why here: The highest gain per package after platforms (maturity weighs 3 and ecosystem 2 in 'commercial release'), and it is what a stranger sees on day one. Tutorials by hand need the editor of 1.4. Graphics rise cheaply here: environment maps and re-dressed scenes move the picture more than the next eight effects.

Needs from the user: Where the art comes from; approval per download or generation; Eyes on the re-dressed scenes; Enable Discussions or name a place to talk

Needs from outside people: Package authors: ecosystem above 3.5 is their work; People who follow a tutorial and say where they stopped

### 1.6: 2D kit (11 packages)

- Sprite foundation: textures kept on the GPU, premultiplied alpha, frame inset, order and Y-sort, sprites in the id buffer, warnings instead of silent drops
- Sprite sheets and SpriteAnimation: a sheet asset, clips, Aseprite JSON import, slicing operations
- 2D physics profile on the existing Rapier 3D: plane lock as a world setting, polygons, one-way platforms, collider from sprite, a checker rule
- Tilemaps (2 packages): chunk components, a culled mesh and merged colliders per chunk; paint, fill and read operations (rows of characters with a legend); a describe_view summary
- Fixed viewport, pixel-perfect camera with whole-number upscaling in the presenter and in captures, pointer in the world, sprite picking
- Editor 2D mode (2 packages): drop a picture to make a sprite, pixel grid and snap, frame picker, clip preview, tile palette with paint, erase and fill
- Nine-slice and fit modes for the interface, parallax, flat background, follow-camera helper, text in the world
- 2D content: platformer, top-down and puzzle templates, recipes, a 2D tutorial run in CI, open and held-out tasks, a small CC0 tileset and character package
- Closing review, fix budget, scorecard

Why here: Moved from the draft's 1.1. It stands on five things built once: gamepad and player-indexed actions, the presenter (pixel-perfect scaling), editor picking and navigation, import settings, packages that carry assets. Built earlier, about three of its packages would be built twice. 2D is also last in the user's stated order.

Needs from the user: Art for the tileset and character; Eyes on pixel art; Quota for the 2D tasks

Needs from outside people: People who make a 2D game with it, to harden it

### 1.7: Prove (10 packages)

- Tool profiles: a core of at most 25 tools for outside agents, all tools for the cockpit; a read-only session role; lean flags for the cockpit agent
- Other agents: AGENTS.md and connection snippets, typed schema for ops, a generic command-line adapter, --agent in both runners; a measured probe of a direct model adapter (tokens per passed task)
- Hard evaluation tier: whole small games from a brief, bug-fix and regression tasks, multi-scene work; per-try folders the agent cannot leave; a seeded play-test bot
- Quality mode (3 packages): acceptance on plan steps; pipeline plan, build, prove with the gate, review by a read-only second run (experimental), repair with caps; Fast, Balanced, Thorough; wish-evals run --mode
- Verified runs (2 packages): replay verify in strict mode, a signed verdict, a wen verify batch command; a first-party package with a recipe, a template and a server example
- Time-travel depth: a session diff between two ticks, and which system last wrote a value
- Closing review, fix budget, scorecard with the quality-mode curve (pass rate against cost)

Why here: The quality mode needs the gate (1.1), a read-only role and a tier hard enough to show a difference: the 41-task suite is saturated, so nothing here can be judged earlier. Verified runs need replay format 2 (1.2) and the Linux package. It moves the two totals little (AI has half a point of headroom) but it is the engine's main goal. Second stopping point: after 1.7 (79 packages) only the web and the High tier remain.

Needs from the user: A spending cap: this is the costliest campaign (three modes, two models, three tries); Approval to install other agent programs; Whether a direct model adapter is wanted (it needs an API key)

### 1.8: Smaller and faster (web groundwork) (10 packages)

- Executor probe and ADR: three ways to run game code where wasmtime cannot (the browser's engine with injected fuel, an interpreter in the player, Pulley), measured for speed and for where fuel runs out
- Lean runtime (2 packages): session build and step move into a crate that Workspace and the player share; the player no longer links the control plane
- Virtual project files and one deterministic packed asset file (sorted, no timestamps) so the replay fingerprint holds
- ABI 2, once (2 packages): slice views, read-only columns neither copied back nor compared, Changed and Added filters, reused frame buffers; modules precompiled at export behind an executor interface
- Culling (view and per cascade), sorted draws, instancing, cached sprite and interface atlases
- GPU skinning in the opaque, shadow, id and debug pipelines
- Engine crates build for wasm32 in CI with the recorded hashes checked in Node; a non-blocking form of Gpu::checked, tested headless
- Closing review, fix budget, scorecard

Why here: The web cannot be built without these, and each pays on the desktop first (a smaller player that starts faster, bigger scenes), so the release is worth publishing even if the web slips. The probe decides the executor before any web code is written. The one ABI change after 1.0 happens here, together.

Needs from the user: Approve what the probe names (an interpreter or a metering transform: new dependency)

### 1.9: The browser (preview) and the light (12 packages)

- Web executor as the probe decided, with fuel and memory limits and traps mapped to file and line
- Web presenter and shell (2 packages): canvas surface, async GPU path, a loop that never blocks, input, audio after a user gesture, saves in browser storage
- project_export target web: HTML shell, packed assets, Export dialog; CI smoke in a headless browser with a screenshot at tolerance and the hash test
- Ambient occlusion on indirect light, fixed pattern, with an ao debug view
- Spot-light and point-light shadows in one atlas, capped with a warning (2 packages)
- Particles and import coverage (2 packages): textured, additive and soft particles with 2D shapes; texture transform, sampler modes, vertex colours, second UV set, morph targets
- Material editor with a preview sphere; inspector depth (mixed values, per-field override, arrays that grow)
- MSAA 4x as a High-tier setting, only if the 1.3 measurement passed; otherwise recorded in ADR-012 and skipped
- Closing review, fix budget, scorecard; web labelled a preview, WebGPU only

Why here: The web is the largest platform step code alone can make, and a link is the cheapest way to reach the outside people three scores wait on. It ships as a preview one release before 2.0 so the long beta can test it. The High-tier effects come this late because they buy the least per package; they complete the M12 scope.

Needs from the user: Approve wasm-bindgen and web-sys (new dependencies) and a headless browser download; Eyes on each effect's golden image

Needs from outside people: People who open the web build in Chrome, Firefox and Safari on their own machines

### 2.0: Proven in public (12 packages)

- Reflection probes: capture cubes prefiltered on the CPU
- Baked irradiance probes (2 packages): a probe volume baked on the CPU, deterministic, cached under .wish/; an irradiance debug view
- Experimental kits (2 packages): game-code packages compiled with the game by one more rustc call; source_check on package sources; the first kit taken from an example game
- Autotile rules and Tiled/LDtk import
- Web out of preview: showcase and template games playable as links on a page; called supported only if hashes and replays pass in CI
- Public multi-engine benchmark (2 packages): engine-neutral task and check format, a Godot adapter, repeated tries, published transcripts and a results page
- Freeze audit and a beta of at least four weeks with fix packages (2 packages); the formal three-person test again
- Release candidates, closing review, final scorecard on held-out tasks, 2.0.0 and a planned 2.0.1

Why here: Only items that need everything before them, plus the one thing code cannot shorten: four weeks of strangers using it. The beta runs while the last feature packages are built, so the calendar time is not idle.

Needs from the user: Host a jam or a call for testers (an itch.io page); Approval to install Godot for the benchmark, or cut it; A spending cap for the final campaign; Optional: an Apple Developer account for a signed macOS build; The call if the beta floor is not reached: ship with the shortfall stated, or wait

Needs from outside people: Ten or more first runs on named platforms; Three people for the formal test; An outside reproduction of the benchmark; First outside package authors

### Scores at 2.0

| Area | With outside people | Without | Basis |
|---|---|---|---|
| Graphics quality | 6.5 | 6 | On top of the 1.0 look: environment maps and re-dressed scenes (1.5), culling, instancing and GPU skinning (1.8), AO, spot and point shadows, particles (1.9), reflection and baked irradiance probes (2.0); no SSR, volumetrics or TAA, so breadth stays below Godot; 6 if the scenes are not re-dressed with real art. |
| Platform support | 6.5 | 6 | Windows and Linux hosts, cross-export, Steam Deck checked in the runtime, macOS arm64 preview (1.3), web export on WebGPU (1.8 to 2.0); 6.5 needs a real Deck report, Mac users and a signed build; no Android, iOS or consoles, so below Godot's 8. |
| Ecosystem and community | 4 | 3.5 | Download layer (1.3), assets, prefabs, dependencies, author tooling and a seeded first-party index (1.5), verified runs package (1.7), experimental kits (2.0); everything above 3.5 is packages and answers written by outside people. |
| Maturity and docs | 5.5 | 4.5 | Stability contract with a corpus held across ten releases (1.2 on), generated docs site and Help panel (1.3), tutorials run in CI (1.5, 1.6), a four-week beta; the docs half can reach 7 by code, the maturity half needs strangers, shipped games and months. |
| Editor tools | 7 | 6.5 | First hour (1.1), the musts with profiler and background jobs (1.4), 2D mode with tile painting (1.6), material editor and inspector depth (1.9); no docking and no animation timeline; 7 assumes hardening by outside users. |
| 2D games | 7.5 |  | The 2D kit (1.6), textured particles (1.9), autotile, Tiled/LDtk import and web export (2.0); no 2D lights, 2D skeletal animation or mobile, so below the 9 of Godot, GameMaker and Defold. |
| Determinism, replay, tests | 10 |  | Unchanged on the scale; it becomes a product: cross-OS replays in CI (1.1), replays and a crash ring in the player (1.2), verified runs (1.7), wasm32 hashes (1.8). |
| Working with AI | 10 | 9.5 | Gate and round 2 (1.1), tool profiles, hard held-out tier and quality mode (1.7), every release measured on two models; 10 only if outsiders reproduce the public benchmark (2.0) and the three-person test passes; rivals shipped agent surfaces in 2026, so the lead may shrink in relative terms. |
| Code iteration speed | 9 |  | Code freshness, instant first Play, background build and build on save (1.1), long calls off the frame (1.4), ABI 2 (1.8); 9.5 only if the rules experiment says yes (logic as data, no compile step). |
| Lightness and install | 8.5 |  | A base of about 68 MB with the compiler and export players on demand, strip and LTO (1.3), a player without the control plane (1.8); below Godot's 10 while changing game code needs a 418 MB compiler; 9 if rules let simple games skip it. |
| Licence and cost | 10 |  | MIT + Apache-2.0, no fee and no commission; no package changes it. |

### Not in 2.0

- Android and iOS: 6 or more packages each, a device, store accounts and (for iOS) a Mac and a no-JIT executor; CI alone cannot prove them. The lean runtime, packed assets and presenter are their groundwork, so they are the first items after 2.0.
- Consoles: SDKs are under NDA and cannot live in an open repository; the honest deliverable is a page that names the third-party route.
- Signed, notarised macOS: only a paid Apple account and a Mac user are missing; one package once the user has both. Until then the preview label stays.
- Editor and tool plugins (wish_ext): a lasting ABI and a new trust boundary for authors who do not exist yet; kits, recipes, workspaces and an agent over MCP cover most of it. Revisit when outside authors ask.
- Docking and saved layouts: every golden image and position-based test changes, for about a third of an editor point.
- Specialist editors (animation timeline, shader graph, terrain, an in-engine code editor): each is milestone-sized; Blender, a text editor or an agent fills in.
- TAA, upscalers, auto exposure: they depend on earlier frames, which breaks repeatable single-frame captures, the base of every golden test.
- Screen-space reflections, volumetric fog, decals, GPU-driven drawing, virtual geometry, ray tracing: the expensive half of graphics, and several leave the WebGPU core.
- Custom material and post-effect shaders from projects: a bad shader can lose a software device (CLAUDE.md GPU rules); wait until device-loss recovery has releases behind it.
- 2D lights and shadows, 2D skeletal animation, a Rapier 2D backend, isometric and hex maps: two or more packages each for narrow sets of games; 2D lights need the sprite pass moved into HDR.
- Rollback netcode and multiplayer: there is no networking layer and no score row moves; player-indexed input and replay format 2 keep the door open.
- Parallel system execution: the highest determinism risk on the list and no measured need; the profiler of 1.4 decides later.
- A second scripting language: XL, and it doubles docs, recipes and tasks; the rules experiment answers the same need first.
- Steamworks and kernel anti-cheat as player variants: proprietary SDKs and a partner account; verified runs are the engine's own answer.
- A direct model adapter as a product: only if the 1.7 probe shows clearly fewer tokens per passed task; the engine would then own an agent loop.
- Hub missions, interface languages beyond Latin, Cyrillic and Greek, translated diagnostics, server-started MCP streams: none blocks a game being made; the fonts need a licence decision.

### Risks

- The count is still low. The M12 remainder is about 86 packages here; the old estimate implied about 35. Mitigation: L items are counted as two; every release is capped at 12 and re-counted at its start; 1.4 and 1.7 are publishable stopping points.
- Format changes after 1.0 (player-indexed actions, replay format 2, the ABI) break the stability note. Mitigation: One window in 1.2 with the migration step and corpus built first; one ABI bump in 1.8; everything else by addition; format 1 replays stay readable.
- Web export fails late: fuel counts differ between executors, wasm32 float results differ, GPU calls block. Mitigation: A wasm32 hash job from 1.1; the executor probe and ADR before any web code; a preview label for one release; 'supported' only when hashes and replays pass in CI; the groundwork pays on desktop anyway.
- Outside people do not come, so maturity, ecosystem and AI stay at the lower numbers. Mitigation: Every such score is given without them; clocks start early (public releases from 1.1, docs site at 1.3, playable links at 1.9); only the 2.0 beta waits for people, and its floor is the user's call.
- The owner's attention is the real limit: pictures, sound, decisions. Mitigation: One sign-off sheet per release; decisions listed up front; a spending cap replaces per-run approvals; only the download layer waits on an open decision.
- Tokens run out before 2.0. Mitigation: Order is by dependency, then gain per package; every release is a published end state; the cut order is written.
- No hardware for macOS, Steam Deck or a Linux desktop: CI is the only check. Mitigation: Words follow evidence (ADR-027 rule 4): preview, checked in the runtime; never playable or supported without a named report.
- The quality mode spends more tokens than it earns. Mitigation: Built behind the gate and measured on the hard tier as pass rate against cost before it is offered as a default; a cap per task.
- A rules layer makes two ways to write logic, so docs, recipes and tasks double. Mitigation: It enters only by the written rule (no worse on correctness for any task, better overall); a text language needs a second measurement.
- Effects pile up: golden churn and CI time on the software adapters. Mitigation: High-tier settings are off by default; the two-adapter measurement after each effect; small test scenes; MSAA only if it passes.
- Competitors' agent surfaces improve (Unity, Unreal, GameMaker and Defold shipped them in 2026). Mitigation: Comparison tasks are re-run with each scorecard; claims are published with their commands; the lead is stated as measured.
- One build folder, so one code agent at a time. Mitigation: A docs lane runs beside it (tutorials, pages, package content made with the engine's own tools); campaigns run between packages.
- A new dependency fails the licence check (TLS, gamepad, dialog, web). Mitigation: Check before design; fallbacks are named: a curl child process, own XInput/evdev code, OS dialogs.

### Decisions that are the user's

- Carried from 1.0: the name, and merging to main.
- Downloading (blocks 2 packages in 1.3): index repository name and owner; an HTTP/TLS library and a signature crate (new dependencies; fallbacks: the OS's TLS or a curl child process); a signing key kept as a CI secret.
- Art (1.5, 1.6): where showcase, starter-pack and tileset art comes from (CC0 downloads or generation); approval per download.
- A spending cap per measurement campaign instead of a yes each time: ten release scorecards, the rules experiment (1.1), the quality-mode curve (1.7, the largest), the public benchmark (2.0).
- Rules layer: yes or no after the 1.1 results. Yes adds 3 packages; a text language later adds 2 to 3 more and needs a second measurement.
- Gamepad (1.2): a gamepad library (new dependency) or own code; a gamepad to test in hand.
- Folder dialog (1.4): a dialog library (new dependency) or own code.
- Where the base and extras line runs (1.3): compiler, SDK pack and export players as components; kits experimental at 2.0; plugins after 2.0.
- Accounts and switches: GitHub Pages (docs site, web demos), Discussions or a chat place, an itch.io page and jam for the 2.0 beta.
- Optional money: a Windows code-signing certificate; an Apple Developer account (without it macOS stays a preview); the Steam fee if that channel opens.
- Web (1.8, 1.9): approve what the executor probe names (an interpreter or a metering transform), wasm-bindgen and web-sys, and a headless browser download.
- Benchmark (2.0): approve installing Godot and other agent programs (disk), or cut it.
- Direct model adapter: wanted or not after the 1.7 probe (it needs an API key).
- Outside people: one or two informal runs at 1.3; three for the formal test at 1.4 and again at 2.0; ten or more for the 2.0 beta; a Steam Deck owner; a Mac user; a Linux desktop user.
- Cut-order conflict: the numbers say cut High-tier graphics early; your stated order says cut graphics last. The plan follows your order until you say otherwise.
- Sign-offs by eye and ear, once per release: presenter smoothness, each effect's picture, re-dressed scenes, pixel art, sound.
- The beta floor at 2.0: if fewer than ten outside first runs arrive, ship with the shortfall stated, or wait.

### Cut order

- First, what the user did not ask for: the public benchmark and Godot adapter, extra agent adapters and the direct-model probe, time-travel depth.
- Second-tier depth: inspector depth and the material editor, half the templates and starter packs, autotile and Tiled import, kits, import coverage.
- Then by gain per package: MSAA, AO, point-light shadows, irradiance probes (about nine packages for half a graphics point). This conflicts with the user's order (graphics cut last): the user chooses.
- Then: the macOS preview (it never gates a release) and the download layer (the offline bundle works without it).
- Last: the web chain, and only as a whole; the lean runtime and ABI 2 still pay on the desktop.
- Never cut: code freshness and the verification gate, the format window with its migration step and corpus, the presenter, components and cross-export, the scorecard at every release, the stability contract, the closing reviews, the four-week beta before 2.0.
- Stopping points if tokens run out: after 1.4 (47 packages) and after 1.7 (79 packages); each is a published, coherent engine.

### Claims the planner verified

- wish_player links the whole control plane: crates/wish_player/Cargo.toml depends on wish_control, wish_script, wish_render, wish_window.
- wasmtime 50.0.1 with the cranelift feature is the only executor, used only in crates/wish_script (Module::new at module.rs:58, consume_fuel on in engine.rs); pulley-interpreter is in Cargo.lock but named in no source or manifest.
- GPU calls block: pollster::block_on in crates/wish_render/src/gpu.rs lines 99, 125, 197-199; the device is opened with Features::empty() and downlevel_defaults (127-128).
- No swapchain: no create_surface or get_current_texture in crates/wish_render/src; windows present through softbuffer (wish_window viewer.rs 326, 469-470; shell.rs 204, 294-295); no fullscreen or scale-factor code in wish_window.
- No gamepad in wish_input, wish_window or wish_types; no Tilemap, SpriteAnimation, load-scene or save-slot type in wish_types or wish_sdk.
- Package KINDS are languages, recipes, templates, themes, workspaces; 64 MiB per package (crates/wish_control/src/packages.rs 27-33).
- The tool catalogue has 34 tools (crates/wish_control/src/tools/mod.rs 40-75).
- Only export checks for stale code (tools/export.rs 149-154, sources_newer_than at 309); sim.rs, testing.rs and replay.rs have no such check.
- The player's step computes the hash and runs the inspector: play_step (script.rs 433) calls step_session, which calls inspect and app.hash() (sessions.rs 649-651).
- project_export has no target parameter; the player name is chosen with cfg!(windows) (tools/export.rs 89, 223).
- wish_agent has two adapters (claude.rs, scripted.rs); the editor has no orbit camera and ops.rs has no duplicate operation.
- No HTTP, TLS or signature crate in Cargo.lock; sha2 0.10.9 and flate2 are already there.
- Local package measured with du: 559 MB total, toolchain 418 MB, sdk 73 MB, wen.exe 40.2 MB, wish_player.exe 30.7 MB; so the base is about 68 MB.
- Cargo.toml has no [profile.release] or dist profile yet; wish_cli has no docs, doctor, component or verify command.
- Replay limit is 54,000 ticks (replays.rs 29); MAX_SESSIONS is 8 (sessions.rs 16); wish_evals has no held-out set yet.
- crates/wish_control/src/expr.rs (1,123 lines) is the expression language the rules prototype would grow; wish_types has no rule or trigger type.
- Rapier is built with enhanced-determinism (Cargo.toml 94); sound output is Windows-only in wish_player/Cargo.toml today.
- No wasm32 cfg exists in wish_ecs, wish_app, wish_physics, wish_render or wish_control: the engine's own wasm32 build is untested.
- The task's weighted totals reproduce from the eleven scores (64 and 36 today, 67 and 44, 78 and 60); the 2.0 totals use the same arithmetic.
- m9-product is 30 commits ahead of main.

### Not verified

- Whether an L item fits one 3 to 5 hour package; I count it as two. The total could be 100 to 125.
- Whether wasmtime's runtime or Pulley can be built for a browser host, and the speed and licences of an interpreter or a fuel-metering transform: stated from general knowledge; the 1.8 probe decides.
- Whether the engine crates compile for wasm32-unknown-unknown (std::thread appears 7 times in wish_control, wish_script, wish_app and wish_ecs sources), and whether hashes match on wasm32 and arm64.
- Licences and dependency trees of a gamepad library, a dialog library, any TLS crate, a signature crate, wasm-bindgen and web-sys.
- MSAA and filtered-float agreement between WARP and lavapipe.
- The size gain from strip and LTO; the 68 MB base is arithmetic on measured folders, not a built package.
- Whether the 1.0 short path lands as listed: the render foundation is in progress in the working tree and I did not read it.
- The competitor scores (taken as given) and the 2026 competitor facts in the area maps' web research (not rechecked). The 2.0 scores are judgements, not measurements.
- The cost of one scorecard campaign (held-out set, two models, three tries, a long task): no measurement exists yet.
- The 41 tasks, the 41/41 result and the M7 transcript counts: taken from CLAUDE.md and the area map, not re-counted.
- Whether today's Windows export runs under Proton, and how rust-analyzer loads the packed proc-macro libraries.
- Whether a save game restores Rapier state bit for bit (the save-games design depends on it).
- CI state on GitHub; nothing was built or run for this plan.

## Web research: where the bar will stand in 2027 and 2028

Where the bar will stand for a 2.0 shipping in 2027-28, from web research on 18 Nov 2026. Three things moved in 2026. (1) Agent access became first-party everywhere that matters: Unity shipped an MCP server in open beta (May) and announced a free MCP + CLI + public API with Unity 7 (Preview Dec 2026, release Q1 2027); Epic shipped an experimental MCP plugin in UE 5.8 (17 Jun) and says MCP will be integral to UE6 (early access late 2027); GameMaker built Claude Code into its new CLI; Defold added an editor HTTP API with /eval and OpenAPI for agents. Godot and Bevy still have only community plugins. So "has an MCP" is table stakes by 2027; the remaining differentiators are a closed, headless, deterministic build-run-test-capture loop and published scores on outside benchmarks (three academic agent-in-engine benchmarks appeared in 2026, all Godot-based, best agents at 41-54%). (2) Rendering defaults rose: no-bake dynamic GI is reaching the mainstream tier (Unity Surface Cache GI for URP in 6.7 LTS, UE MegaLights production-ready and Lumen Lite, Godot SDFGI, Bevy Solari experimental), alongside contact shadows, area lights, SSR, HDR output and upscaler frameworks. (3) Platforms: WebGPU is on by default in Chrome, Safari 26 and Firefox on Windows/macOS but still incomplete on Linux and Firefox Android; Unity calls WebGPU production-ready in 6.6; Godot web is still WebGL2. Steam Machine launched (June 2026) and inherits Deck Verified, making SteamOS the open-engine console baseline; real consoles remain NDA-gated (Defold added Xbox free for approved developers in Oct 2026; Godot relies on W4 and porting houses). On maturity, the incumbents sell predictability: Unity LTS yearly with two years of support and a "no breaking changes" Unity 7; Godot a written rule for what may break in minor vs major; Bevy still 0.x with breaking releases and no editor after six years. Everything dated after June 2026 rests on search results I could not cross-check against my own knowledge; see caveats.

### 1. AI: first-party agent surface

**Bar.** By 2027 every major engine has a free, first-party MCP server and/or CLI that lets an outside agent read and edit scenes, scripts, assets and build settings; an engine that only offers this is merely at parity. Bring-your-own-model and opt-in are the norm.

**Evidence.** Unity: MCP server in open beta, post dated 11 May 2026 (scene management, C# scripts, console, component values, build settings, custom tools in C#; needs Unity 6 + AI Assistant package + Unity Cloud + beta subscription). Unite Seoul recap 21 Jul 2026: Unity 7 includes a free MCP, CLI and API, 'free and without subscription'; CLI demo validated an asset and pushed bundles to a Switch 2 without the Editor; `unity command eval` runs C# in a running Editor or Player. Unity 7 Preview Dec 2026, release Q1 2027. Epic: UE 5.8 released 17 Jun 2026 with an experimental MCP plugin (Blueprints, assets, levels, materials, meshes; loopback HTTP); Epic wants MCP to be 'an integral part of UE6', UE6 early access late 2027. GameMaker: GM-CLI with Claude Code built in, 30 Apr 2026 (query project, find bugs, manage build configs). Defold: editor HTTP API gained OpenAPI (1.12.3) and /eval (1.13.0) 'for agentic workflows', H1 2026 post 30 Jun 2026; 'will not be forced on users'. Godot: no first-party assistant or MCP found; many community MCP plugins in the Asset Library. Bevy: none first-party found.

- https://unity.com/en/blog/unity-ai-mcp-how-to-get-started
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://egamers.io/unity-7-roadmap-public-api-cli-and-a-free-mcp-bring-the-editor-to-non-users-and-coding-agents/
- https://forums.unrealengine.com/t/unreal-engine-5-8-released/2729274
- https://engadget.com/2196807/epic-games-details-how-its-embracing-gen-ai-in-unreal-engine
- https://knowledge.edge-online.com/p/ai-goes-native-in-unreal-engine-the-five-main-takeaways-from-state-of-unreal-2026
- https://press.opera.com/?p=3924
- https://defold.com/2026/06/30/Defold-H1-2026/

### 1. AI: closed loop and public benchmarks

**Bar.** The differentiator in 2027-28 is the closed loop (build, run headless, step, capture, test, replay, with deterministic results) plus scores on outside, reproducible benchmarks rather than only an in-house task suite. Visual feedback to the agent (screenshots/video) is expected, since it measurably raises success.

**Evidence.** GameDevBench (arXiv 2602.11103, v1 11 Feb 2026, v2 30 Jun 2026, ICML 2026): 333 Godot tasks, best agent 53.8%; visual feedback lifted GPT-5.4 from 41.1% to 52.0%; gameplay tasks 51.4% vs 2D graphics 33.0%. GameCraft-Bench (arXiv 2606.17861, 16 Jun 2026): 140 Godot tasks in 15 game families, end-to-end playable games verified by replayed demonstrations plus rubric judging; strongest agent 41.46%, most below 40%; failures are incomplete content, visual feedback and presentation. Also reported: SWE-Game (247 tasks from 41 Godot reference games, incl. Godot-to-Unity porting) and JAMER (8,133 game-jam projects, Godot headless checks). All are Godot-based; none covers a new engine, so a 2.0 would need to port one or publish its own tasks so outsiders can rerun them. Incumbent docs I could read list editing and inspection tools; test running or play-mode control by agents is claimed only in secondary sources.

- https://arxiv.org/abs/2602.11103
- https://arxiv.org/abs/2606.17861
- https://arxiv.org/pdf/2609.33678
- https://huggingface.co/papers/2606.19830.md
- https://github.com/waynchi/gamedevbench

### 1. AI: AI-native engines and agent-built games

**Bar.** 'AI-native' (text-first scenes, headless render, MCP commands) is no longer unique: small engines with exactly this shape appeared in 2026 and funded prompt-to-game platforms exist. By 2.0 the proof expected is shipped games and measured outcomes, not the architecture claim.

**Evidence.** March 2026 round-up names nAIVE Engine (Rust, WebGPU, YAML scenes, headless rendering, MCP over JSON-RPC), Arcane Engine (code-first 2D, Rust core + TypeScript, agent protocol, no editor) and Mirror Engine (alpha); its author predicts these patterns get absorbed by mainstream engines within two years. Funding: SPARQ opened an $8.5M seed for an 'AI-native game engine' (beta rolling out), Astrocade raised $56M, Bitmagic $4M (text-to-3D-game, Steam playtest). Self-described AI-built Steam titles in 2026: Codex Mortis ('100% AI-developed', divisive reception), Catvivors (Claude Code + Codex, Early Access 13 Mar 2026), Void Balls (reported 10 days, 8 parallel agents). Epic's Sweeney: AI 'will be involved in nearly all future production'.

- https://app.cinevva.com/signals/2026-03-24-ai-native-engines-are-shipping
- https://thenextweb.com/news/sparq-85m-seed-a16z-scout-ras-al-khaimah
- https://pocketgamer.biz/astrocade-raises-56m-to-expand-ai-powered-games-creation-platform/
- https://app.cinevva.com/signals/2026-03-23-solo-devs-shipping-not-vibing
- https://www.tweaktown.com/news/109355/codex-mortis-is-a-100-ai-made-roguelite-bullet-hell-and-its-proving-divisive-as-hell-too/index.html

### 2. Graphics: default feature set of small engines

**Bar.** 'Good enough for an indie 3D game' in 2027-28 means, out of the box: PBR with clearcoat-class materials, cascaded directional shadows (up to 4 splits) with soft filtering plus screen-space contact shadows, point/spot/area lights, reflection probes and SSR, SSAO, at least one GI path (baked lightmaps and/or a real-time one), volumetric fog, bloom, DOF, a modern tonemapper (AgX/ACES), TAA plus MSAA/FXAA options, a temporal upscaler (FSR 2 class) and HDR display output.

**Evidence.** Godot feature list (stable docs): Forward+/Mobile/Compatibility renderers; LightmapGI, VoxelGI, SDFGI, SSIL; TAA, FSR2, MSAA, FXAA, SSAA; PSSM 2/4 splits with PCSS-like blur; SSAO, SSR, glow, DOF, volumetric fog, AgX. Godot 4.7 (June 2026): HDR output on Windows/macOS/iOS/visionOS/Linux Wayland, AreaLight3D. Godot 4.8 beta 1 (15 Oct 2026): screen-space contact shadows, multi-bounce AO, mip-level texture streaming, decals in Compatibility. Bevy 0.19 (19 Jun 2026): contact shadows, physically based SSR, rectangular area lights, parallax-corrected cubemaps, vignette/lens distortion, GPU-driven large scenes (many_cubes 49.5 ms to 18.8 ms). Defold 2026 roadmap: Light component and PBR extension (a 2D-first engine catching up).

- https://docs.godotengine.org/en/stable/about/list_of_features.html
- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/
- https://bevy.org/news/bevy-0-19/
- https://www.defold.com/2026/02/10/Defold-A-look-ahead-for-2026

### 2. Graphics: no-bake dynamic GI and upscalers

**Bar.** By 2028 a real-time, no-bake global illumination option that runs on mid-range and handheld hardware, and a pluggable upscaler layer (FSR/DLSS/platform upscalers), are expected in the mainstream tier, not only in the high-end pipeline. A 2.0 renderer needs one scalable dynamic GI path with a baked fallback.

**Evidence.** Unity (Unite Seoul recap, 21 Jul 2026): Surface Cache GI, 'fully dynamic global illumination for URP without baking', preview in 6.7 alpha and broad availability with 6.7 LTS (Q4 2026 target); upgraded Upscaler Framework with partner neural upscalers (reported: DLSS 4, FSR, PSSR, STP); Neural Texture Compression. Unreal 5.8 (17 Jun 2026): 'MegaLights is now Production-Ready' aimed at 60 FPS on current consoles; Lumen Lite 'up to twice as fast as Lumen High Quality', incl. handhelds. Bevy: Solari experimental real-time raytraced lighting, improved in 0.18 (13 Jan 2026) and 0.19. Godot: SDFGI/VoxelGI already default options in Forward+.

- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://www.invenglobal.com/articles/24003/unity-engine-7-changing-the-development-paradigm-and-the-roadmap-ahead
- https://forums.unrealengine.com/t/unreal-engine-5-8-released/2729274
- https://bevy.org/news/bevys-sixth-birthday/
- https://bevy.org/news/bevy-0-19/

### 3. Platforms: web

**Bar.** Web export with WebGPU as the main path and a WebGL2 or reduced fallback is the 2027-28 bar; WebGPU alone still misses part of Linux and Firefox on Android. Small first download (about 1 MB empty project is the best-in-class figure) and a 64-bit memory option are competitive points.

**Evidence.** gpuweb implementation status (edited 2 Oct 2026): Chromium shipped on Windows/macOS/ChromeOS since 113, Android 12+ since 121 (ARM/Qualcomm/Intel), Linux only Intel Gen12+ (144) and NVIDIA on Wayland (147), rest behind a flag; Firefox shipped on Windows (141) and macOS (145/147), Linux Nightly only, Android behind a flag; Safari 26 on macOS/iOS/visionOS. Unity: WebGPU 'production-ready in Unity 6.6' with full URP, compute shaders and indirect rendering; WASM64 up to 16 GB reported. Godot: web default is still the Compatibility (OpenGL/WebGL2) renderer; WebGPU exists only in a third-party fork. Defold: empty HTML5 project about 1.06 MB, wasm only (asm.js removed). Unreal: no web target mentioned in 5.8.

- https://github.com/gpuweb/gpuweb/wiki/Implementation-Status
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://docs.godotengine.org/en/stable/about/list_of_features.html
- https://github.com/dwalter/godotwebgpu
- https://defold.com/2026/06/30/Defold-H1-2026/

### 3. Platforms: Steam hardware and consoles

**Bar.** For an open-source engine, the console baseline is SteamOS: Steam Deck and Steam Machine Verified (Machine: Deck input rules, at least 1080p at 30 fps). Real consoles stay NDA-gated, so the credible 2.0 story is a private, free-for-approved-developers port kit or a named porting partner, at least for Switch 2.

**Evidence.** Godot docs: the only console officially supported is Steam Deck; Switch/Xbox/PS5 via W4 Games middleware (Godot 4.3+) or porting houses (Pineapple Works lists Switch 1 and 2, Xbox, PS5). Defold: Switch, PS4/PS5 via gated builds; Xbox added in 1.14.0 (12 Oct 2026), free for Microsoft-approved developers, paid for by the Foundation and 'progressing slowly' per its Feb 2026 roadmap. GameMaker: console export needs an enterprise licence even after GMRT becomes free on 1 Jan 2027. Steam Machine launched late June 2026 from $1,049; Deck Verified titles are automatically Verified for it. Unity demoed pushing content to Switch 2 from its CLI.

- https://docs.godotengine.org/en/4.5/tutorials/platform/consoles.html
- https://w4games.com/w4consoles
- https://defold.com/manuals/nintendo-switch
- https://www.windowscentral.com/gaming/xbox/xbox-just-gained-support-for-defold-game-engine
- https://www.tomshardware.com/video-games/pc-gaming/valve-says-steam-machine-and-steam-frame-shipping-this-summer-company-confirms-release-window-as-it-expands-verified-program
- https://www.gematsu.com/2026/06/steam-machine-launches-june-30-starting-at-1049
- https://www.pocketgamer.biz/gamemaker-makes-gmrt-free-for-commercial-use-from-2027/

### 3. Platforms: mobile

**Bar.** Android and iOS export with automated SDK setup, Vulkan/Metal by default, safe-area and touch helpers (virtual joystick) is standard for small engines; building on the device itself is now a Godot feature.

**Evidence.** Godot 4.7 (June 2026): stable Android editor build/publish on device (GABE), built-in VirtualJoystick node, gyro input; 4.8 beta: editor downloads and manages Android and Java SDKs automatically. Defold H1 2026: Vulkan default on Android with OpenGL fallback, Safe Area on Android/iOS, Crashlytics extension. Unity 6.6/6.7: Apple API modernization, Android Adaptive Performance signals. UE 5.8: Android SDK setup, Platform Preview, handheld Lumen Lite. Bevy lists iOS and Android as supported, with older community notes calling setup more complex.

- https://godotengine.org/releases/4.7/
- https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/
- https://defold.com/2026/06/30/Defold-H1-2026/
- https://www.invenglobal.com/articles/24003/unity-engine-7-changing-the-development-paradigm-and-the-roadmap-ahead
- https://forums.unrealengine.com/t/unreal-engine-5-8-released/2729274

### 4. Ecosystem: package and asset indexes

**Bar.** An official index with versions per asset, changelogs, ratings/reviews, publisher analytics, in-editor install and update, and a CLI for publishing is the bar; paid assets are the expected next step. Before a major version, the engine vendor is expected to test the most-used packages itself.

**Evidence.** Godot Asset Store: 'live and stable' on 22 May 2026, integrated in Godot 4.7 (reviews, ratings, analytics, multiple versions, changelogs, tags); old Asset Library deprecated and going read-only, assets not migrated automatically; roadmap: paid assets (selected creators first), drag-and-drop install, CLI upload, global addons, asset updater, Steam-like quality score. Bevy (10 Aug 2026): Bevy Assets listings 501 (from 442), crates.io downloads 6.66M (from 2.75M), 1,527 contributors; growth rides on the language's own package registry and a plugin-first architecture. Unity: tested the 68 most popular Asset Store packages for Unity 7 and published an upgrade guide. Defold: community asset store built with editor scripts; team wants an in-editor asset browser. Epic: MetaHuman under MIT licence.

- https://godotengine.org/article/introducing-the-godot-asset-store/
- https://store.godotengine.org/roadmap
- https://bevy.org/news/bevys-sixth-birthday/
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://www.defold.com/2026/02/10/Defold-A-look-ahead-for-2026

### 5. Editor tools

**Bar.** Expected in 2027-28: fully dockable/floatable panels, version-control-friendly text files with small diffs, near-instant play mode, project-wide rename/refactor, shader previews, profiler and remote inspector, editor scripting; browser-based or multi-user editing is appearing at the top end. A young engine without a shipped editor is visibly behind.

**Evidence.** Godot 4.6 (Jan 2026): Modern theme default, bottom panels become regular docks. Godot 4.7: inline shader previews, vertex snapping, MeshLibrary editor, searchable menus. Godot 4.8 beta: main screens become docks, object serialization reworked to avoid oversized VCS diffs, experimental project-wide symbol renaming. Unity: Fast Enter Play Mode default for new projects from 6.6; code reload only for changed code in Unity 7; browser-based Studio editor with real-time multi-user collaboration. Bevy (Aug 2026): editor still not upstream after six years ('very, very close now'); community prototype Jackdaw; Feathers widgets, gizmo and grid groundwork in 0.19. Defold: editor scripting, non-modal dialogs, breakpoints tab, free-fly camera.

- https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/
- https://godotengine.org/releases/4.7/
- https://linuxiac.com/godot-4-6-open-source-game-engine-released-with-workflow-improvements/
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://bevy.org/news/bevys-sixth-birthday/
- https://defold.com/2026/06/30/Defold-H1-2026/

### 5. 2D tooling

**Bar.** A first-class 2D kit means: tilemap editor, sprite atlases and batching for thousands of sprites, 2D lights and shadows with normal maps, GPU and CPU particles, a modern 2D physics core (Box2D v3 class), text shaping incl. right-to-left, plus scene-painting/scatter tools and a 2D profiler. 2D-first engines compete on runtime price and openness.

**Evidence.** Godot: TileMaps, 2D point/directional lights with normal/specular maps and soft shadows, GPU/CPU particles; 4.7 adds 2D Scene Paint Mode (scatter nodes), DrawableTexture2D, one-way collision any direction; 4.8 overhauls the 2D toolbar. Unity (Jul 2026): Physics Core 2D multithreaded on Box2D v3, RenderSprite APIs for thousands of sprites without GameObjects, new 2D profiler, 3D-as-2D sample; Unity 7 reported to add 'AI agent-driven 2D authoring layers'. GameMaker: GMRT runtime launched 30 Apr 2026, free for commercial use from 1 Jan 2027, JS/TS/C# planned, source-available for desktop/mobile/web. Defold H1 2026: expanded Box2D scripting API, text shaping and RTL fonts.

- https://docs.godotengine.org/en/stable/about/list_of_features.html
- https://godotengine.org/releases/4.7/
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://press.opera.com/?p=3924
- https://www.gamedeveloper.com/programming/gamemaker-s-gmrt-runtime-will-soon-be-free-for-all-commercial-devs
- https://defold.com/2026/06/30/Defold-H1-2026/

### 6. Maturity: stability contract, LTS, cadence

**Bar.** A 2.0 is expected to come with a written rule for what may break in patch, minor and major releases, a supported previous line during the transition, a feature release every 3-5 months, and (for studios) an LTS of about two years. 'No breaking changes' is now a headline marketing claim.

**Evidence.** Unity support page: LTS once a year, supported two years (+1 for Enterprise/Industry); update releases several per year, supported until the next; 6.0 LTS to Oct 2026, 6.3 LTS to Dec 2027; Unity 7: 'No breaking changes, no new language to learn'. Godot release policy (4.7 docs): renames/removals and changed defaults only in majors, narrow breakage allowed in minors; only the latest two minors supported (4.7 full, 4.6 full, 4.5 partial, 4.4 EOL); minors about every 4-5 months (4.5 Sep 2025, 4.6 Jan 2026, 4.7 Jun 2026, 4.8 Q4 2026 est.); 3.6 LTS best effort; Godot 5 not branched. Bevy: 0.17 (30 Sep 2025), 0.18 (13 Jan 2026), 0.19 (19 Jun 2026), each with breaking changes and a migration guide; no 1.0 plan stated. Epic: UE 5.8 reported as last planned major UE5; UE6 early access late 2027, full release 12-18 months later. Defold: releases every 1-2 months; 'a roadmap is just a plan, not a contract'.

- https://unity.com/releases/unity-6/support
- https://unity.com/blog/unite-seoul-keynote-2026-recap
- https://docs.godotengine.org/en/4.7/about/release_policy.html
- https://bevy.org/news/bevys-sixth-birthday/
- https://bevy.org/learn/migration-guides/introduction/
- https://engadget.com/2196807/epic-games-details-how-its-embracing-gen-ai-in-unreal-engine
- https://www.defold.com/2026/02/10/Defold-A-look-ahead-for-2026

### Lessons from other major versions

- Godot 3 to 4: do not let the major version absorb every idea. 4.0 took about four years; the team's own post (29 Nov 2022) admits it kept promising features for 4.0 and deferring them, and switched to fixed merge windows where a late feature moves to the next release and releases are never delayed for 'just one more feature'. Decide the 2.0 feature list early and ship the rest in 2.1, 2.2. https://godotengine.org/article/release-management-4-0-and-beyond/
- Godot 3 to 4: say honestly what x.0 is and keep the old line alive. Godot told users 4.0 would have workflow-breaking bugs, shipped quick 4.0.x fixes, recommended 3.5 for conservative users and kept 3.x as LTS (3.6 still supported in 2026). A converter existed but needed manual fixes, and users reported 4.0 as buggy and stayed on 3.x. Plan a supported 1.x line and a migration tool whose limits are documented. https://docs.godotengine.org/en/4.7/about/release_policy.html and https://godotengine.org/article/godot-4-0-sets-sail/
- Blender 2.8: spend the breaking budget on the biggest barrier to newcomers plus one headline capability. 2.80 (July 2019) changed the interface conventions people complained about for years and added a real-time renderer; the Development Fund 'almost doubled' (from 22.4k EUR/month at the start of 2019), Epic gave a three-year MegaGrant, Ubisoft and NVIDIA joined. A 2.0 that visibly fixes the first hour gets a second look from people who bounced off 1.0. https://code.blender.org/2019/07/development-fund-report-july-2019/
- Blender 2.8, the cost side: the same report says not building new features for almost two years 'was really taking its toll', and 2.80 broke the add-on API so every extension needed an update. Keep the stabilization freeze short and give package authors a compatibility window and a checker before 2.0 lands.
- Unity 6 and 7: trust and upgrade safety are features. Unity cancelled the Runtime Fee (Sept 2024) before Unity 6 shipped, and a 2026 retrospective still calls the trust damage lasting. Unity 7 is pitched as 'No breaking changes, no new language to learn', with the risky foundations (CoreCLR, Surface Cache GI) shipped and production-tested in 6.x first, 68 top Asset Store packages tested, an upgrade guide and Project Auditor flagging issues in advance. Land 2.0's risky foundations in late 1.x minors and make 2.0 the quiet switch. https://unity.com/blog/unite-seoul-keynote-2026-recap and https://unity.com/en/blog/unity-is-canceling-the-runtime-fee
- Bevy: frequent breaking releases with good migration guides keep contributors moving, but users ask for a stable 1.0, and a flagship tool that is always 'next' hurts: six years in, the editor is still not upstream ('very, very close now') and the project calls itself 'drastically underfunded for our ambitions'. Do not make a 2.0 depend on one long-promised centerpiece; land its prerequisites in shippable steps (as Bevy did with BSN in 0.19) and match scope to funding. https://bevy.org/news/bevys-sixth-birthday/
- Overlap, not a cliff (Unreal, GameMaker, Defold): Epic plans UE6 early access in late 2027 with full release 12-18 months later while UE5 continues; GameMaker launched its new runtime beside the old one, kept the old language and made the new runtime free from 2027; Defold deprecated Live Update APIs in 1.12.4 before removing them in 1.13.0 and still warned of migration work. Run old and new side by side and deprecate one release before removing. https://engadget.com/2196807/epic-games-details-how-its-embracing-gen-ai-in-unreal-engine and https://defold.com/2026/06/30/Defold-H1-2026/
- AI features in a major version land better when optional and open: Epic stresses 'Your sources, your pipeline and your workflow' and bring-your-own-model, Unity made its MCP and CLI free and without subscription, Defold says AI workflows 'will not be forced on users', while a self-described '100% AI' Steam game drew divided reception. Keep AI opt-in, model-agnostic, and back claims with reproducible numbers (public benchmarks show best agents at 41-54%).

### Caveats

- My own knowledge ends in June 2026. Everything dated after that (Unity 7 announcement and dates, Unity 6.6/6.7, Godot 4.8 beta, Defold 1.13/1.14, GameMaker free-runtime decision, gpuweb status of 2 Oct 2026, Bevy birthday post) comes from pages fetched today through a summarizing reader; I could not cross-check them independently.
- Epic's own State of Unreal news page returned 403. UE 5.8 MCP details and UE6 timing (early access late 2027, full release 12-18 months later; 5.8 as the last planned major UE5) come from Epic's forum post plus Engadget and Edge coverage, not Epic documentation. The forum post itself does not label the plugin experimental; press coverage does.
- Unity MCP: I could not confirm from Unity's own docs whether agents can run play mode or the test runner; that claim is from secondary sources. A third-party guide says Unity deprecated the in-Editor MCP server and moved it into the Unity CLI; unconfirmed. Whether Unity AI left beta is unconfirmed. One article gives conflicting years for the Unity 7 release; Unity's recap says Preview in December and release in Q1 of the following year (2027). Unity 6.7 LTS release date is unconfirmed (a beta thread existed in October 2026).
- Local observation, not a web source: the Unity MCP tool names visible in this session include scene/GameObject management, script edit/validate, console, camera and scene-view capture, profiler queries, package manager, asset generation and a generic run-command tool; no dedicated test-runner tool name is visible. I did not call any of them.
- Benchmarks: GameDevBench figures differ by version (v1: 132 tasks, best about 47-54.5%; v2: 333 tasks, 53.8%). The top GameCraft-Bench agent's name comes from a secondary write-up. SWE-Game (arXiv 2609.33678) and JAMER were seen only in search results and the SWE-Game identifier looks unusual; verify before citing. I found no benchmark built on Unity, Unreal or a new engine.
- AI-native engines: I found funding rounds, alphas and betas, not a publicly launched engine in which an agent reliably builds complete games. The nAIVE/Arcane/Mirror descriptions come from one vendor blog (a company with its own engine). '100% AI-built' Steam titles are self-described and mostly reported by one outlet.
- Godot: no official Foundation position on an in-editor assistant or MCP was found (absence of evidence only). Godot 4.8 was in beta 3 with no stable date confirmed as of today. The claim that 4.7 added a wasm64 web export is from secondary sources; the official 4.7 page I read does not mention web. The console docs page for 4.7/stable returned 404; I used the 4.5 version, and W4's Switch 2 status is unconfirmed. No asset count or revenue-share terms were published for the Asset Store.
- Bevy: I found no 2026 primary statement on web (WebGPU vs WebGL2 default), mobile or console status, and no 1.0 or stability plan; cadence figures are derived from release dates.
- Steam Machine launch date conflicts between sources (25, 29 or 30 June 2026); price $1,049 is consistent. Valve has not published what share of the catalog is Verified.
- Defold Xbox support in 1.14.0 rests on secondary coverage (Windows Central and others); I did not read the official release post. Defold is source-available under its own licence rather than an OSI open-source licence.
- blender.org's 2.80 page returned 403; the Blender lesson uses the Development Fund report of 20 July 2019 and secondary coverage. The post-MegaGrant monthly figure (about 84k EUR by Oct 2019) is from secondary sources.
- 'Good enough for an indie 3D game' and every statement about where the bar will be in 2027-28 are my synthesis and extrapolation from current default feature lists and announced roadmaps, not a published standard; vendor roadmaps are explicitly subject to change.
- Not researched in depth: Unreal's Fab marketplace terms, Unity Asset Store numbers, GameMaker marketplace status, WebAssembly threading/COOP-COEP hosting constraints, and adoption figures for any engine version.

## Critic: Feasibility, capacity and sequence: can one owner with limited tokens and one code-writing agent finish it; are the package counts honest against the area maps' own sizes; does each item come after what it stands on; does anything break the promise made at 1.0; which scores lean on people who are not there yet. Checked against the repository at HEAD (d638576), uncommitted work under crates/ ignored; nothing was built or run. (key feasibility)

**Verdict.** 2.0 should be the capacity plan's train with the AI spine of the ai-native plan pulled forward and three rules from the commercial plan: about 113 packages as written, honestly 125 to 135 once a maintenance reserve and the under-counted items (web executor, save games) are added, which is ten times the 1.0 short path and must be re-planned in tokens after 1.0 is measured. Order: 1.1 the loop, the gate, environment maps, the rules experiment with tasks hard enough to decide it; 1.2 the one format window with the migration step first, the presenter and the hard tier; 1.3 the desktop, small, with tool profiles; then the editor by hand, share and teach, the 2D kit, the quality mode, the web as the one new platform, and a 2.0 that adds no features: freeze, published proof, a long beta, and nothing broken. Mobile, consoles, docking and specialist editors, real-time global illumination, netcode, several agents at once and an own language (unless the experiment says yes, and then as experimental) are scheduled after 2.0, not dropped. The scores I would defend are about 81 on 'AI with small games' and about 65 on 'commercial release' by code alone, 83 and 67 if outside people come: clearly ahead of Godot (72) and Unity (68) on the first, still more than ten points behind them on the second, and that gap is community, platforms and years that no package count closes. The plans' higher totals (85 to 86 and 71 to 72) rest on a rules bet that has not been run, on phones nobody can test, and on outside people counted as present.

### Ranking

- **capacity**: 7.5 of 10.
  - Strength: The only count that survives a recount. I re-added the area maps' own sizes (S about half a package, M one, L two, XL four) for everything outside the 1.0 short path and got about 90 to 100 packages for the M12 remainder before web; this plan says 86, ai-native about 75, the M12 note implied about 35. It is still about 10 percent low, but it is the right order of magnitude.
  - Strength: Everything that changes a format, a hash or the way the picture reaches the screen is done once, in 1.2, with the migration step and the compatibility corpus built first. Verified: wen migrate has no migration step today (crates/wish_cli/src/project_cmds.rs), so the first format change after 1.0 is also the first migration.
  - Strength: Two written stopping points (after 1.4 at 47 packages, after 1.7 at 79) where what is published is a coherent engine. No other plan can be stopped early without leaving half a chain.
  - Strength: Names a fallback for every dependency decision (curl child process, own XInput/evdev, OS dialogs) and lets the download layer start whenever the user decides, with nothing waiting behind it. The download decision has been open since M10b.
  - Strength: An executor probe and an ADR before any web code, and web groundwork (lean runtime, packed assets, precompiled modules) that pays on the desktop even if the browser slips.
  - Strength: The most defensible score table of the three, with separate numbers where outside people are needed, and it tells the user openly that its own gain-per-package finding conflicts with his stated order (graphics cut last).
  - Weakness: Its 2.0 breaks its own discipline: reflection probes, baked irradiance probes (2), experimental kits (2), autotile and Tiled import and the web promotion all land in the release that is also the freeze and the four-week beta ('the beta runs while the last feature packages are built'). Kits open a new trust boundary and a namespacing ADR; they need a release of exposure before a freeze, not the freeze itself.
  - Weakness: The AI spine is late for the engine's stated main goal. Hard evaluation tier, tool profiles and the read-only role all wait until 1.7. Verified: the suite is 41/41 (evals/results/m7-sonnet-5-5-medium.json) and there is no held-out set in crates/wish_evals, so six scorecards (1.1 to 1.6) would have no correctness headroom; and the rules experiment in 1.1 cannot discriminate on tasks both forms pass.
  - Weakness: It freezes a snapshot of the tool catalogue in 1.2 and then changes what an outside agent sees in 1.7 (profiles). After 1.0 that is a tool-surface change; it has to be opt-in or come before the snapshot.
  - Weakness: Graphics, the user's first priority, gets almost nothing between 1.0 and 1.5 (tiers and a measurement), and environment maps wait until 1.5 although ADR-012's amendment lists them in the baseline without condition (docs/DECISIONS.md, ADR-012).
  - Weakness: Several single packages are too small: the web executor with fuel, memory limits and trap mapping is one package (the area map sizes it L, risk high); save games is one package although WorldSnapshot is memory-only and Rapier is built without a serialization feature (verified: Cargo.toml line 94, crates/wish_ecs/src/world.rs 289-303); material editor plus inspector depth is one.
  - Weakness: No package is reserved for bug reports from real users, patch releases or dependency upgrades across 6 to 10 months, and the wasm32 hash job promised in 1.1 contradicts 'engine crates build for wasm32' in 1.8 (it is feasible for the simulation core only: I found no thread use in wish_ecs, wish_app or wish_physics; the plan's own note placed thread use there wrongly).
  - Weakness: Its reason for leaving docking out ('every position-based test changes') is weaker than stated: the project rule is that editor tests find widgets by name, so a panel registry with the same default layout changes neither tests nor golden images.
- **ai-native**: 6.5 of 10.
  - Strength: Counted in the same honest unit as the capacity plan, and its verified-claims list is the most accurate of the three. I rechecked and confirmed: 34 tools as a compile-time constant; the expression language lives in crates/wish_control/src/expr.rs (1,123 lines) so a rules system below the control plane needs it moved; package KINDS and the 64 MiB limit; no read-only role (only readOnlyHint in crates/wish_mcp/src/lib.rs, although ADR-022 says a --read-only mode exists); the player depends on all of wish_control; wasmtime only in wish_script.
  - Strength: The best order for the AI work: the gate and the game-code loop in 1.1, the hard tier in 1.2 when a game can first have a menu, levels and a save, tool profiles in 1.3 before the catalogue grows, the quality mode only after all of them.
  - Strength: Environment maps in 1.1 as non-default settings: the one must-tier render item the short path left out, at the first possible moment.
  - Strength: '2.0 breaks nothing: every 1.x project opens with a migration' is the strongest stability position and matches what the market now sells (Unity 7's 'no breaking changes').
  - Strength: Ideas that raise correctness without outside people: checks on what a picture shows inside the gate, a seeded play-test bot, packages that carry their own checks and proof of use, a transcript audit at every closing review, art as data.
  - Strength: New render features are non-default settings, so no release except 2.0 waits on a default-look decision; and it gives an explicit cut order that removes about 45 packages without touching the M12 scope.
  - Weakness: 156 packages in eleven releases is twelve times the 1.0 short path, each release with a campaign, a decision sheet and by-eye sign-offs. The owner's attention, not the agent, is the limit the plan itself names.
  - Weakness: The spine is a bet on an experiment that has not run. The thesis already says 'most logic is written as rules'; rules v1 (4), the small language (5) and 'rules-only games first' on the web all assume a yes. The area map sizes a compiler-free language XL; five packages does not cover parser, checker, budgeted interpreter, diagnostics, editor parity and a second set of docs, recipes and tasks.
  - Weakness: Release 1.9 (forks and three-way merges, an orchestrator, a direct model adapter, tools from packages, kits: about 12 packages) is what the user did not ask for, spends more tokens per task against the scorecard's efficiency line, and adds a merge subsystem where a silent error corrupts a project.
  - Weakness: Sequence errors. Asset import and import settings arrive in 1.6, after sprite sheets (1.2) and the tile palette (1.4): verified that import settings are stored but read by nothing (meta.import is only echoed by asset_info). Gizmo completion and hierarchy at scale (must-tier editor gaps) arrive in 1.7, after the by-hand tutorials (1.6) and the first formal three-person test (1.5). Pulley on native is built in 1.7 although the only targets that need it (iOS, consoles) are outside its 2.0. The showcase is re-dressed only at 2.0.
  - Weakness: The fetch layer is scheduled in 1.3 as if the user's download decisions were made, with no alternative order if they are not.
  - Weakness: Scores count outside help as zero where it is not: editor 7.5, 2D 8 and graphics 6.5 are given the same value with and without outside people, and Working with AI is 10. Graphics is judged from what games look like, which needs art; an editor nobody else has used daily is not a 7.5.
  - Weakness: 'Material shaders from projects and packages' is one package; the area map sizes it L and the project's own GPU rules record that a bad shader can lose a software device.
- **commercial**: 4.5 of 10.
  - Strength: The best single rule of the three: a platform or an executor is called supported only when the same committed replays give the same hashes on it, with the executor's id in the replay. It extends ADR-019's naming rule into a matrix and should be adopted whatever plan wins.
  - Strength: Sees structural costs the others miss: the panel registry before any more panels (verified: crates/wish_editor/src/app.rs is 5,414 lines, no registry); the CPU-rasterised editor as a limit (verified: 0.6 ms layout plus 4.3 ms paint at 1080p in docs/design/M8a-editor-foundation.md, so 4K is about four times the paint); manifests that reject unknown keys (verified: MANIFEST_KEYS in crates/wish_control/src/packages.rs), which makes 'by addition' harder than it sounds; VISION.md lines 104 and 110 and ADR-019 still saying web is the headline of 1.1.
  - Strength: 2.0 as a quiet release with no features, a scorecard campaign as a gate on every release ('does not ship if correctness or errors got worse'), and export with verify: true.
  - Strength: Lists what a sold game needs and no other plan has: animation blending and a state machine, navigation, joints, game localisation, streamed music and a mixer. Groups the account-bound items so one round of the user's paperwork unblocks them, and gives a fallback order when the download decision is still open.
  - Weakness: The package count is a count of bullet points. Release 1.1 alone holds the migration step and corpus, code freshness, the gate, background build, the presenter, HiDPI, device-loss recovery, gamepad, player-indexed actions, focus navigation, replay format 2, replays in the player, save games, scene switching, the editor's first hour, AI efficiency round 2 and the rules experiment: 22 to 25 packages by the area maps' sizes and by the other two plans' unit, listed as 13. Release 1.2 and 1.7 are the same. Counted honestly the plan is well over 200 packages, four to five times what the user rejected before a first release.
  - Weakness: 'About 3 months of building at two packages a day' assumes about 56 machine hours a week from an owner whose stated limit is tokens.
  - Weakness: Phones (Android and iOS, both XL in the area map) are nine bullets in 1.9 with no device, no Mac, no store account and an interpreter that is in Cargo.lock but has never run here. The plan's own determinism-matrix rule would keep both at preview, so they cannot carry Platform support to 7.
  - Weakness: Release 1.10 puts reflection probes, screen-space reflections, baked global illumination, volumetric fog, decals, TAA and an upscale path, auto exposure, depth of field, custom shaders, mesh LOD, occlusion culling, compressed textures, HDR output and terrain in 11 packages; the area map sizes that group XL and several of them break repeatable single-frame captures.
  - Weakness: Breaks at 2.0 what it sold through 1.x: deprecated tool and SDK items are removed (game code has no automatic migration) and profiles become the default; and a 1.x long-term-support branch is a promise of one person's time. Engines side by side (M10b) already keep an old project on its exact engine.
  - Weakness: A six-week beta with an exposure floor per platform on six platforms needs dozens of outside testers; it is counted as calendar time.
  - Weakness: The headline scores rest on the parts that cannot be delivered or verified: graphics 7, platforms 7, 2D 8.5 and a commercial total of 72.

### Scores the critic would defend

- **Graphics quality: 6.** The plans say 6.5 to 7. On the report's scale Bevy is 5 with cascades, SSAO, TAA, SSR and volumetric fog, and Godot is 6 with three GI methods, SSR, volumetrics, TAA and an upscaler. The combined plan adds environment maps, culling and instancing, GPU skinning, AO, spot and point shadows, textured particles, reflection probes and baked irradiance probes: still a shorter list than Godot's today, with no real-time GI while that becomes the mainstream default in 2027. 6 is defensible with the showcase re-dressed; 5.5 if the probe packages are cut; 6.5 needs real art, which is an open decision; 7 needs the under-counted 1.10 of the commercial plan.
- **Platform support: 6.** Windows and Linux hosts, cross-export, Steam Deck checked in the runtime, a macOS arm64 preview and a WebGPU-only web export reach Bevy's 6 without its mobile. WebGPU alone still misses part of Linux and Firefox on Android. 6.5 needs a real Deck report, a signed macOS build (an Apple account) and outside browser tests; 5.5 if the web is still a preview at 2.0. 7 needs phones, which cannot be verified without devices.
- **Ecosystem and community: 3.5.** Formats, author tooling, a seeded first-party index and experimental kits are what code can give. Defold is 4 after years of community and Bevy 5 with about 500 listed assets; a repository that went public on 17 November 2026 with no users cannot claim 4.5 a year later. 4 if a handful of outside authors publish; everything above 3.5 is outside people and calendar time.
- **Maturity and docs: 5.** The docs half can reach 6 to 7 by code (generated reference, tutorials executed in CI, a stability contract held across about ten releases with a compatibility corpus). The maturity half is months old, has one maintainer and no game shipped by anyone else. Bevy is 4 with six years and no stability promise, so 5 is fair; 5.5 with ten or more beta users and one outside game; 6 is not defensible inside this calendar.
- **Editor tools: 7.** The musts, profiler, background jobs, 2D mode with tile painting, material editor, inspector depth and logic debugging justify half a point over the full-scope 6.5. It is conditional: the editor is drawn by a CPU rasteriser measured at 4.9 ms at 1080p, HiDPI is unproven, and nobody outside has used it daily. 6.5 if either stays true. 7.5 would put it between GameMaker and Godot without animation, sprite or shader editors.
- **2D games: 7.5.** The kit plus autotile, Tiled/LDtk and Aseprite import, textured particles and web export. Unity is 8 with 2D lights, skeletal animation and a dedicated 2D physics core; a kit on Rapier 3D with a plane lock and no skeletal animation is not its equal. 8 only if 2D lights land and outside people have made 2D games with it; 8.5 is not defensible.
- **Determinism, replay, tests: 10.** Unchanged, and all three plans agree. It stays 10 only if two rules are written: a platform or executor is named supported only when committed replays match on it, and what a 1.x release may change in simulation results (with the engine version in the replay header, which format 1 does not carry today).
- **Working with AI: 9.5.** The gate, held-out tasks on two models, the hard tier, the quality mode and tool profiles defend 9.5 by code. 10 means nothing better exists and is proven: it needs an outside rerun and tasks not written by the home side, while first-party agent surfaces are becoming standard (Unity's free MCP and CLI, Epic's plugin). Today's evidence is 41 saturated home-written tasks and one run of eight scene tasks.
- **Code iteration speed: 9.** Code freshness, background build, build on save, prebuilt templates and the measured 0.18 to 0.4 s rebuild with state kept reach Godot's and Defold's 9. The plans' 9.5 depends on a yes from an experiment that has not run (logic without a compile step); do not count it until it does.
- **Lightness and install: 8.5.** Measured today in C:\wish-build\package: 559 MB total, toolchain 418 MB, SDK 73 MB, wen.exe 39 MB, player 30 MB, so a base near 70 MB is arithmetic, not hope. It stays below Godot's 10 and Defold's 9 because any change to game code still needs the 418 MB compiler component. 9 only if rules win or the lean player is shown to be much smaller.
- **Licence and cost: 10.** MIT plus Apache-2.0, no fee, no share. To keep it: proprietary SDK variants (Steamworks) stay outside the repository, and adding OFL-1.1 for fonts is a user decision that does not change the score. With these eleven numbers the report's arithmetic gives 'AI with small games' about 81 (83 with outside people) and 'commercial release' about 65 (67), against 78 and 60 at full M12 scope, Godot 72 and 77, Unity 68 and 83 as they stand today.

### Grafts

- From ai-native: move the hard evaluation tier to 1.2 (whole-game briefs with a menu, two levels and a save; bug-fix and regression tasks; a per-try folder the agent cannot leave; three tries with spread), and build its game-logic slice in 1.1 together with the rules experiment.
- From ai-native: tool profiles and the read-only session role by 1.3, before the components tool becomes the 35th tool. Ship profiles as opt-in written into new projects' .mcp.json, so existing projects keep every tool.
- From ai-native: environment maps in 1.1 as non-default settings (an .hdr reader as own guarded code or an approved dependency; a generated sky or an approved CC0 download).
- From ai-native: the rule that 2.0 breaks nothing. Every 1.x project opens with a migration; no tool or SDK item is removed at 2.0; new looks reach new projects through presets that templates write explicitly, so schema defaults never have to change.
- From ai-native: checks on what a picture shows inside the gate (text too small, interface boxes off screen or overlapping, the subject not visible) and before-and-after picture differences as data. They raise the scorecard's first two lines without outside people.
- From ai-native: packages that prove themselves (checks as data, evaluation tasks with hidden tests, the index records model, tries, date and command) when packages gain assets and dependencies.
- From ai-native: a transcript audit at every closing review (every read-back found becomes an answer fix) and a results page generated per release with the command for each number (ADR-027 rule 4).
- From ai-native: art as data (procedural textures, SVG-subset sprites through the vector rasteriser already in the tree, synthesised sounds), after a check that vello_cpu gives the same pixels on Windows and Linux. It reduces what waits on the open art decision.
- From commercial: the determinism matrix as the definition of 'supported' for every platform and every executor, with the executor's id in the replay; and project_export with verify: true, which runs the export headless and compares the hash.
- From commercial: the panel registry (split app.rs into registered panels, layout as data, the same picture) before the editor musts of 1.4, so every later panel is built once. Docking itself stays late and cuttable.
- From commercial: measure the editor's CPU rasteriser at 4K inside the HiDPI package and decide the draw path (dirty-region painting, or compositing inside wish_render under ADR-018) before docking, timelines or tile palettes are built on it.
- From commercial: the scorecard as a release gate (a release does not ship if correctness or errors got worse on the held-out set).
- From commercial: correct VISION.md lines 104 and 110, ADR-019, the M11 list in ROADMAP.md and section 5 of the M12 note, which still call web export the headline of 1.1, and ADR-012's amendment, which still lists environment maps in the 1.0 baseline without condition. Both are public claims that the short path made untrue.
- From commercial: decide in the 1.0 stability note how an older engine treats a manifest key it does not know. Today it rejects it (MANIFEST_KEYS), so every additive manifest change needs an engine lower bound in the package.
- From commercial, late and first to cut: animation blending with a state machine as a data asset, a deterministic grid pathfinder on tilemaps before any navmesh, streamed music with buses, and game string tables for Latin, Cyrillic and Greek. A sold game needs them and the winner has none.
- From commercial: group the account-bound items (Steam, Apple, Windows signing) in one release so one round of the user's paperwork unblocks them, and keep one release with no account or download dependency ready to run first when a decision is still open.

### Sequence fixes

- Capacity 2.0: move reflection probes, baked irradiance probes, kits, autotile and Tiled import, and the web promotion into 1.9 or a new 1.10. 2.0 is then the freeze audit, the published proof, the beta and the release candidates. Dependency: a beta tests what ships (the M12 note's own Phase 5 rule is 'no feature merges'), and kits need a namespacing ADR and a release of use before a freeze.
- Hard tier: capacity 1.7 and commercial 1.5 move to 1.2. Dependency: the open suite is saturated at 41/41 and no held-out set exists yet, so a scorecard without harder tasks cannot show any release getting better or worse; and the rules decision rule ('no worse on correctness for any task') decides nothing on tasks both forms pass.
- Tool profiles and the read-only role: capacity 1.7 and commercial 1.5 move to 1.3 at the latest. Dependency: ADR-010 asks for 20 to 25 tools, the catalogue has 34, and components adds a 35th in 1.3; the capacity plan also snapshots the catalogue in 1.2, so a later default profile would be a tool-surface change after the snapshot.
- The full stability contract comes after AI efficiency round 2 and after the profile decision, not before. Dependency: round 2 changes default answers ('small scenes answer values by default'); the full-scope plan had it before the freeze for that reason. The short note at 1.0 must say plainly that it covers file formats only and that tool answers and the SDK are frozen by the full contract.
- Environment maps: capacity 1.5 and commercial 1.4 move to 1.1 or 1.2. Dependency: the showcase re-dress, the feature gallery and every tutorial picture are taken with them, and ADR-012 already lists them in the baseline.
- ai-native: asset import and import settings (1.6) must come before sprite sheets and Aseprite import (1.2) and the tile palette (1.4). Dependency: import settings are stored in the meta file and read by nothing today, and the editor has no import path; otherwise filter and slicing settings are built twice.
- ai-native: gizmo completion and hierarchy at scale (1.7) must come before the by-hand tutorials (1.6) and the first formal three-person test (1.5). Dependency: both are must-tier gaps in the editor map; a talking-only test run on an editor without them measures the gaps, not the engine.
- ai-native: drop Pulley on native from 1.7 and put the capacity plan's executor probe and ADR in its place. Dependency: Pulley serves only no-JIT targets (iOS, consoles), which are outside every 2.0 except the commercial one; a browser cannot host wasmtime at all, so the browser decision stands alone.
- ai-native: the showcase re-dress moves from 2.0 to right after environment maps. Dependency: the README, the gallery, the tutorials and any store page are made from it, and the graphics score is judged from pictures.
- Commercial 1.1 must be split in two (the loop and the proof; then the format window and the screen), as the other two plans do. Dependency: by the area maps' sizes it is 22 to 25 packages; and the scorecard campaign that judges round 2 and the rules experiment should not run across a hash-and-format change.
- Commercial: phones (1.9) move after 2.0. Dependency: touch, lifecycle, a no-JIT executor, a Mac, store accounts and devices; none is owned, so neither target could pass the plan's own matrix beyond an emulator.
- Save games are designed in the same window as replay format 2 and scene switching (all three plans do this) but counted as two packages, not one. Dependency: a load is a recorded step, so the replay must carry the save's fingerprint; WorldSnapshot exists only in memory; Rapier is built without a serialization feature, so physics state must be rebuilt on load and that rule written down.
- The engine version in the replay header should not wait for replay format 2. Dependency: the format 1 header carries format, scene, scene_hash, seed, tick_rate, code, ticks and assets, and no engine version (crates/wish_control/src/replays.rs). The first 1.x release that changes any simulation result makes every replay recorded on 1.0 diverge with nothing to say why. It is one additive key: first package of 1.1, or a fraction of a package inside 1.0's stability work if the user allows (this grows the 1.0 scope slightly).
- The wasm32 hash job in 1.1 (all three plans) is feasible for the simulation core only (wish_core, wish_ecs, wish_app, wish_physics: no thread or wasm cfg use found). It needs a way to run wasm32 tests, which is a toolchain target download and so a user approval; the full engine on wasm32 stays where the lean runtime is.
- The quality mode can return from capacity's 1.7 to about 1.5 once the hard tier is in 1.2 and the read-only role in 1.3: those two and the gate are its only real prerequisites, and the user's accepted draft had it at 1.4.
- The fetch layer cannot have a release number in any plan. Dependency: the HTTP/TLS library and the index repository's owner have been open since M10b. Components from a local archive, cross-export and Steam Deck do not need it and must not be written as if they did.

### Missing from every plan

- A maintenance budget. No plan reserves packages for bug reports from real users after a public 1.0, for 1.x.y patch releases, or for CI upkeep as platforms multiply; every 'fix budget' covers the closing review only. Reserve 15 to 20 percent of each release, or the feature counts are fiction from 1.2 on.
- A dependency-upgrade policy. wasmtime is pinned at =50.0.1 (security fixes, and a new version may count fuel differently), Rapier 0.36 (an upgrade changes physics hashes), wgpu 30, winit 0.30, egui 0.37, and the packed rustc 1.100.0 (a new compiler changes game.wasm bytes and so the replay's code fingerprint). Over 6 to 14 months each of these is a hash-changing or golden-changing event; none is scheduled, and web export will probably force at least one.
- A written rule for what a 1.x release may change in simulation results. Replays, tick-exact scenario tests ('wins at tick 129') and verified-run servers all depend on it. Proposed: results change only in a minor, listed in the changelog, never in a patch; the replay names its engine version; the corpus defines 'keeps its meaning' as tests pass and replays are re-recorded with a note; engines side by side are the long-term support.
- A budget in tokens, not packages. The owner's limit is tokens and time, and no plan converts one into the other; a scorecard campaign (held-out set, two models, three tries, a long task) has never been priced. Measure the 13 packages and the first campaign of 1.0, then re-plan 1.1 onward in that unit and publish the actual packages per week.
- Tasks written by someone else. Three outside agent-in-engine benchmarks appeared in 2026, all on Godot. Porting a subset of one to Wish (if its licence allows; a download the user must approve) gives evidence the home side did not write, years before a home-made multi-engine benchmark at 2.0 could be called reproduced.
- Questions the executor probe must answer and no plan lists: ADR-025's one fresh instance per tick has only been measured on wasmtime (16 to 28 microseconds); NaN canonicalization is a wasmtime setting (crates/wish_script/src/engine.rs) with no browser equivalent; stack-overflow depth differs between browsers; trap-to-file-and-line rests on wasmtime backtraces.
- The life of the signing key and a threat model for what it signs: rotation, revocation, loss, and a review of the updater and of packages that carry code. The plans list only where the key is kept.
- A hardware plan. No Mac, Steam Deck, phone or HiDPI screen is owned and audio has never been checked by ear (pending since M6). The plans list these as decisions; none says which targets stay labelled preview for the whole train if nothing is bought or borrowed, and what that does to the platform score.
- A performance floor on named low-end hardware (an integrated GPU, Deck class) published with its command. Only the commercial plan schedules it, late; the presenter and tiers are meaningless without a number to hold.
- How outside issues and pull requests are handled when agents write all the code and one person approves: triage is owner attention, the scarcest resource in every plan, and an ecosystem score above 3.5 cannot happen without it.
- Review notes. Verified in the repository at HEAD: the facts cited above with file names, the package folder sizes, glam built with libm, three storage buffers in lights.wgsl, CI on Windows and Ubuntu only, no git tag, pulley and wasm-bindgen present in Cargo.lock, no gamepad, HTTP or TLS crate. Not verified: every package count including my recount (estimates from the area maps' size letters), licences of any proposed dependency, the competitor facts in the web research, whether CI is green, the uncommitted render foundation, and all scores, which are judgements.

## Critic: Critic lens: honest scores and the user's priorities. I judged each plan on five questions: would a neutral reviewer sign its 2.0 scores against the competitor columns and the moving bar; does it build the main goal (most effective with AI) or only name it; does it put graphics and platforms first as the user asked; does it finish the full M12 scope before adding new things; and how long the owner waits for something he can see. Read-only; nothing was built or run. Verified at HEAD (git show, uncommitted crates/ ignored; docs read from the working tree): the tool catalogue has 34 tools; package KINDS are languages, recipes, templates, themes, workspaces (no asset kind) and unknown manifest keys are rejected; crates/wish_control/src/expr.rs is 1,123 lines and lives in the control plane; wish_player depends on wish_control and has Windows-only sound output; gpu.rs blocks with pollster::block_on; no create_surface or swapchain anywhere, softbuffer only in wish_window; no gamepad and no touch code in wish_input, wish_window or wish_types; lights.wgsl uses three storage buffers; only DirectionalLight has a shadows field; wasmtime is built with runtime and cranelift only, pulley is in Cargo.lock unused; Cargo.lock has no HTTP or TLS client but already contains sha2, flate2, wasm-bindgen and web-sys; project_export has no target parameter; only export checks for stale code; MAX_TICKS is 54,000 and MAX_SESSIONS is 8; wish_evals has no held-out set and evals/results has only Sonnet runs; the Unity comparison is one run of eight scene tasks (8/8 against 7/8); app.rs is 5,414 lines; CI runs Windows and Ubuntu only; no git tag, version 0.1.0; all three plans' weighted totals reproduce from their own score tables. I also counted the area maps' gaps by size (S 0.5, M 1, L 2, XL 4): must plus should is about 119 package units before reviews and campaigns, 13 of them in 1.0. Found while reading: VISION.md still says web export is the headline of 1.1 and that the tool surface and SDK source API freeze at 1.0, and ADR-012's amendment still lists environment maps in the 1.0 baseline without condition; none of that is true under the short path. Not verified: everything in the web research dated after June 2026 (Unity 7, Steam Machine, the three agent benchmarks), package sizes under C:\wish-build, the area maps' transcript statistics, licences of any proposed dependency, and every package size and calendar, which are estimates in all three plans and in this review. (key scores-and-priorities)

**Verdict.** 2.0 should be the capacity plan's skeleton with three changes, because it is the only plan that can be finished and whose scores a neutral reviewer would nearly sign. First, reorder it by the user's wishes instead of by points per package: environment maps in 1.1, the High tier by 1.4, one thing he can see in every release. Second, give it the ai-native plan's AI spine (hard tier at 1.2, quality mode by 1.5, picture checks, packages that prove themselves, a score on tasks someone else wrote) without its multi-agent release or its own language, which stay experiments. Third, take the commercial plan's definition of the major: a quiet 2.0 with a second format window, a second look lock, the determinism matrix and a 1.x support window, and no phones. That is about 120 to 130 honestly counted packages with stopping points after 1.4 and 1.7. The scores I would defend are graphics 6, platforms 6, ecosystem 3.5, maturity 5, editor 7, 2D 7.5, determinism 10, AI 9.5, iteration 9, lightness 9, licence 10: about 81 on 'AI with small games' and 65 on 'commercial release' (84 and 68 if outside people come and rules win), against 67 and 44 at 1.0. It is a clearly better engine than 1.0 and keeps the lead on the AI total, but it does not close the commercial gap to Godot and Unity, and graphics above 6 or ecosystem above 3.5 cannot be bought with packages alone. The plan should say so on its first page.

### Ranking

- **capacity**: 7 of 10.
  - Strength: The only count that survives a recount. My tally of the area maps' must and should gaps gives about 119 package units before reviews and campaigns; 113 with every L counted as two and every release capped at 12 is the right order of size. Two written stopping points (after 1.4 at 47 packages, after 1.7 at 79) mean the owner always holds a finished engine.
  - Strength: The most defensible scores. Its 'without outside people' column (80 and 63) is within two points of what I would sign (81 and 65), and its thesis says plainly that 2.0 still trails Godot and Unity on platforms, maturity and ecosystem because those need people and years.
  - Strength: Format, hash and presentation changes are made once, at 1.2, with the migration step and the compatibility corpus built first; the docs site starts at 1.3 so the maturity clock runs early.
  - Strength: The only plan with no dependency inversion in 2D: the kit comes after gamepad, the presenter, editor picking, import settings and packages that carry assets.
  - Strength: An executor probe and an ADR before any web code, and web groundwork (lean runtime, packed assets, precompiled modules) that pays on the desktop even if the web slips.
  - Strength: It states its conflict with the user's order openly (the numbers say cut High-tier graphics early, the user says graphics last) and leaves the choice to him.
  - Weakness: It orders releases by gain per package on two weighted totals, not by the user's stated order. The first render package after 1.0 is environment maps at 1.5, about 55 packages in, and the High tier waits for 1.9. Graphics is the user's first wish, and ADR-012 already lists environment maps in the 1.0 baseline. This is the plan that makes him wait longest for a picture.
  - Weakness: It serves the main goal with M12's own AI list and adds nothing new. The hard evaluation tier and the quality mode both wait for 1.7, the seventh release, while it runs a campaign at every release on a suite it calls saturated. The public benchmark, which its own AI 10 rests on, is the first item in its cut order.
  - Weakness: The 2D kit is sixth (the accepted draft had it first) and the quality mode seventh (the draft had 1.4): a larger reorder than the plan admits.
  - Weakness: Its 2.0 merges new features (reflection probes, irradiance probes, kits, autotile, web leaving preview) while the four-week beta runs, so the beta does not test what ships. M12's own Phase 5 rule is no feature merges.
  - Weakness: No second look lock. Every 1.x render feature is a non-default setting (ADR-027 rule 1), so a new scene in 2.0 still has the 1.0 look and the graphics score a reviewer sees out of the box is lower than the one claimed.
  - Weakness: The rules prototype is counted as one package although the expression language must first move out of wish_control (dependencies point down); tool profiles at 1.7 come after the catalogue snapshot of 1.2 that is meant to freeze the tool surface.
- **ai-native**: 6.5 of 10.
  - Strength: The only plan that builds the main goal instead of naming it: the gate at 1.1, the hard tier at 1.2, picture checks in the gate, a play-test bot, packages that carry their own proof of use, a transcript audit at every closing review, and a published benchmark. Each item is tied to a line of the AI scorecard.
  - Strength: Something to see in every release: environment maps at 1.1, the presenter at 1.2, the desktop at 1.3, ambient occlusion at 1.4, spot shadows at 1.5. It has the best cadence for the user's two first wishes.
  - Strength: The most careful verification list (every item I spot-checked holds) and an honest list of what was not checked.
  - Strength: The rules bet has a written decision rule and a fallback, and rules-only games as the first web step remove the browser executor from the critical path.
  - Strength: Art as data (procedural textures, SVG sprites through the vello_cpu already in the tree, synthesized sounds) attacks the open question of where art comes from, and the research says presentation is where agents fail.
  - Strength: The not-in-2.0 list is right: phones, consoles, netcode and real-time global illumination stay out, with reasons.
  - Weakness: 156 packages is twelve times the short path, with eleven releases and eleven campaigns, for an owner who rejected 45 to 50. It becomes finishable only after its own cut order removes about 45 packages, which is a different plan.
  - Weakness: It does not finish M12 before adding new things. Rules v1 (1.4), art as data and picture checks land before the docs site, the tutorials and the full stability contract (1.6), asset import (1.6) and the editor a person stays in (1.8). The by-hand half of the hybrid promise waits longest here.
  - Weakness: Release 1.9 (forks and three-way merges, an orchestrator, a direct model adapter, tools from packages, kits: 14 packages) multiplies token spend, the owner's scarcest resource, and moves no score row. Rules, then a language, then several agents is a centrepiece that is always next: the Bevy editor lesson in the research.
  - Weakness: Dependency inversions: a tileset 'as an engine package' at 1.4 before packages carry assets at 1.6 (KINDS has no asset kind today); the 2D editor mode at 1.4 and Aseprite import at 1.2 before asset import and import settings at 1.6; the rules decision at 1.1 before the hard tier at 1.2.
  - Weakness: Scores half a point to a full point high: graphics 6.5 with a strict subset of what earns Godot its 6 and 'no outside people needed'; ecosystem 4.5, above Defold, within a year; maturity 6; editor 7.5 with no specialist editors; 2D 8, level with Unity; AI 10.
  - Weakness: It lists wasm-bindgen and web-sys as new dependencies to approve; both are already in Cargo.lock at HEAD. The real new approvals for web are a browser download in CI and hosting.
- **commercial**: 4.5 of 10.
  - Strength: The best single rule of the three: a platform or executor is called supported only when the committed replays give the same hashes on it, ending in a determinism matrix. It is ADR-019's naming rule turned into the product.
  - Strength: The only plan that completes the full M12 scope cleanly (at 1.5) before adding platforms, with the High tier at 1.4 so galleries and tutorials are made once.
  - Strength: The right definition of a major version: 2.0 as a second format window, a second look lock, deprecations removed, a 1.x support branch and no features. That is the Unity 7 and Godot 4 lesson from the research.
  - Strength: It finds real holes for a shipped game that the area maps lack (localisation with CJK and right-to-left text, navigation, joints, animation blending, audio buses), flags that VISION and ADR-019 still promise web in 1.1, puts a panel registry before the panels, and names the CPU-rasterised editor as a risk.
  - Strength: Every score is given with and without outside people.
  - Weakness: The count is not defensible: every bullet is one package. Docking (an L in the area map), a navmesh, an animation state machine with root motion and IK, an Android player, an iOS player, baked global illumination with lightmaps, and TAA with upscaling, auto exposure and depth of field are each counted as 1. Its 1.1 alone is 24 to 26 packages by the other two plans' counts. The real size is about 200 to 250, not 126, and the calendar falls with it. This owner cannot finish it.
  - Weakness: The highest scores and the least defended. Graphics 7 puts Wish above today's Godot with Godot's list minus real-time GI, each feature built once and tested on software adapters. Platforms 7 counts phones the plan itself says stay previews without devices and accounts. 2D 8.5 is above Unity.
  - Weakness: It is judged on the commercial total, where AI weighs 0.5. The hard tier, tool profiles and quality mode wait for 1.5 and nothing AI-new is added beyond M12 except task packs. It names the main goal.
  - Weakness: Phones, a console seam, terrain, Steamworks and HDR output need the most accounts, devices, money and outside people of any plan, for things the user did not ask for.
  - Weakness: Rules v1 is hidden in a first-run bullet at 1.3, and the 2D templates are written at 1.2 before the rules answer can shape them.
  - Weakness: Touch input arrives at 1.9, three releases after the web export whose links will be opened on phones.

### Scores the critic would defend

- **Graphics quality: 6.** All three are high (6.5, 6.5, 7). Godot holds its 6 with real-time GI, lightmaps, SSR, volumetric fog, TAA, an upscaler and decals. A Wish with cascades, image-based light, AO, shadows on every light, GPU skinning, reflection probes and baked irradiance probes is a subset of that, built once and tested on two software adapters. A neutral reviewer puts it level with today's Godot at best, and the research says the bar is moving toward no-bake GI. 6 holds only if a second look lock makes the bounced-light look the default and the showcase is re-dressed with real art; without those it is 5.5. A 6.5 needs SSR, volumetric fog, decals and temporal anti-aliasing as well, which I size at 15 to 20 real packages.
- **Platform support: 6.** Windows and Linux hosts, cross-export, Steam Deck by a real report, a macOS preview and a web export on WebGPU that passes the hash and replay test. That is where M12's own table puts a web preview. Bevy holds 6 with web and both phones, roughly; one-step export and replay-verified platforms balance the missing phones. 6.5 needs a signed macOS, a fallback for the browsers WebGPU still misses, and named device reports. 7 needs phones on real devices, which this owner cannot verify.
- **Ecosystem and community: 3.5.** Code and first-party content reach 3. Defold holds 4 with a decade of forum answers and hundreds of extensions; Bevy holds 5 with about 500 listed assets and 1,500 contributors. A year after 1.0, with a seeded index, kits and a handful of outside packages, 3.5 is the most a reviewer would sign. The plans' 4 to 4.5 assume a community that cannot be scheduled. With no outside authors it is 3.
- **Maturity and docs: 5.** The docs half can reach 6.5 to 7 by code (a generated reference, tutorials run in CI, a contract with a corpus). The maturity half stays near 3.5 a year after 1.0 with few or no shipped games by others. Bevy holds 4 after six years and hundreds of shipped projects. The plans' 5.5 to 6 are half a point to a point high. 5.5 needs a real beta population and at least one game shipped by someone else.
- **Editor tools: 7.** The area map's own ladder: 6.5 after the musts, 7 with background jobs, visual debugging, a material editor and docking or a panel registry, 8 only with an animation timeline and hardening by daily users. The review tools no other editor has (proposal previews, checkpoints, go-to-tick, who wrote this value) justify 7 without specialist editors. 7.5 is an undercount of the work behind it. 6.5 if the logic-debugging and material tools are cut.
- **2D games: 7.5.** The kit, tilemaps with autotile, 2D lights, textured particles and a web export put it clearly above Bevy's 6. Unity holds 8 with 2D skeletal animation, a real 2D physics core, rule tiles and phones; Wish has none of those and runs 2D physics on a plane-locked 3D solver. 8 needs outside people shipping 2D games with it. 8.5 is not defensible.
- **Determinism, replay, tests: 10.** Unchanged, on one condition: the naming rule holds. A platform or executor whose hashes and committed replays do not pass is labelled preview. A 'supported' web or arm64 build that drifts would cost this score, which is the engine's whole argument.
- **Working with AI: 9.5.** All three write 10 with an outside reproduction and 9.5 without; I defend the 9.5. The measured lead is one run of eight scene tasks (8/8 against 7/8) and a saturated in-house suite. By 2027 every large engine has a free first-party agent surface, so Unity's 7 becomes about 8. The gate, a hard held-out tier, the quality mode and published numbers earn 9.5. 10 is earned only when someone outside reruns the numbers and Wish posts a score on a task set it did not write.
- **Code iteration speed: 9.** Freshness, background build, build on save, prebuilt template modules and the state-keeping hot reload that already exists put it level with Godot and Defold at 9. 9.5 means better than them and needs a no-compile path for logic, which is exactly what the rules experiment decides. Do not book it before the answer.
- **Lightness and install: 9.** The capacity plan's 8.5 is slightly modest. A base of about 68 MiB (arithmetic on measured folders), a compiler fetched once and shared across engine versions, and a player without the control plane or the JIT backend is Defold's class, not below it. It stays under Godot's 10 while any change to game code needs a 418 MiB compiler. 8.5 if the lean player slips or the built base measures above about 80 MiB.
- **Licence and cost: 10.** MIT plus Apache-2.0, no fee and no share. It holds as long as proprietary SDKs (Steamworks, platform kits) stay optional components outside the repository and every new dependency passes deny.toml.
- **Weighted totals from the eleven scores above: 81.** 'AI with small games' about 81 and 'commercial release' about 65 (weights as in the report). If outside people come, rules win and the conditional half points are earned: about 84 and 68. For comparison: 67 and 44 at 1.0, 78 and 60 at full M12 scope; Godot 72 and 77, Unity 68 and 83 today, both moving up. The honest gain from the full scope to 2.0 is about 3 and 5 points; from 1.0 it is about 14 and 21. The plans' 83 to 86 and 68 to 72 are 2 to 7 points high.

### Grafts

- From ai-native: environment maps in 1.1 as a non-default setting, then one visible render or platform item in every release (tiers, culling and instancing by 1.3; ambient occlusion and spot shadows by 1.4 to 1.5). Do not hold the High tier until 1.9.
- From ai-native: the hard evaluation tier at 1.2 (whole-game briefs with a menu, two levels and a save; bug-fix and regression tasks; per-try folders; three tries with spread), with its game-logic tasks built in 1.1 beside the rules experiment.
- From ai-native: count the rules experiment as three packages. Moving the expression language out of wish_control into a crate below the simulation is a package by itself. If the answer is yes, take 'rules that explain themselves' (fire counts, why a rule did or did not fire at tick T, rule coverage in the gate).
- From ai-native: picture checks in the verification gate (text too small, interface boxes overlapping or off screen, the subject not visible, low contrast) and before-and-after picture differences as data. The research's benchmarks say visual feedback is the measured lever and presentation is where agents fail.
- From ai-native: packages that prove themselves (checks as data, evaluation tasks with hidden tests, packages check --prove, the index recording model, tries, date and command). It is the one ecosystem feature an agent-first engine can have that the others do not.
- From ai-native: a transcript audit in every closing review (each read-back found becomes an answer fix) and one decision sheet per release for the owner.
- From ai-native: art as data, part 1 (procedural textures and SVG-subset sprites through the vello_cpu already in the tree), so an agent's result is presentable without a download decision.
- From ai-native: tool profiles and the read-only role by 1.3, before the tool catalogue is snapshotted, and the quality mode at 1.5 instead of 1.7.
- From commercial: the naming rule as the plan's spine. Supported means the committed replays give the same hashes on that platform and executor; replays carry the executor's id; 2.0 publishes the determinism matrix.
- From commercial: 2.0 as the quiet major. A second format window (additive fields consolidated, deprecated keys removed, each with a migration proven on the corpus), a second look lock (the default look switched once, by eye), profiles as the default, a stated 1.x support window, an upgrade guide tested on the corpus, and no feature merges.
- From commercial: correct VISION.md, ADR-019 and the roadmap now. They still say web export is the headline of 1.1, and 1.0's own package 11 is 'the public face made true'.
- From commercial: game localisation (string tables, a locale switch, font fallback for CJK and right-to-left text; adding OFL-1.1 is a user decision). The other two plans exclude languages entirely, and a game sold on Steam needs them.
- From commercial: project_export with verify: true (run the export headless and compare the hash), and a frame budget measured on a named low-end device before the tiers are called done.
- From commercial: the panel registry (app.rs is 5,414 lines) before the editor musts, so each later panel is built once. It gives the same picture and needs no docking decision.
- From commercial, lower priority: deterministic path queries as a host service (grid paths over tilemaps first), with a recipe and evaluation tasks. Whole-game briefs with enemies need it and no other plan has it.
- From commercial: an exposure floor per named platform in the 2.0 beta, and scores reported with and without outside people at every release.

### Sequence fixes

- Capacity, hard evaluation tier at 1.7: move to 1.1 and 1.2. The plan runs a campaign at every release and decides rules at 1.1, but the suite is saturated (41/41 on Sonnet medium; the plan says so itself) and M12's decision rule compares correctness per task. A campaign on a saturated suite can only measure cost.
- All three, the rules measurement at 1.1 before any hard tier: build the hard game-logic tasks with the experiment. Otherwise 'no worse on correctness for any task' is decided on tasks every model already passes, and efficiency alone picks the winner.
- Capacity, tool profiles and typed op schemas at 1.7 after the full contract and catalogue snapshot at 1.2: a core of at most 25 tools changes what an outside agent sees by default, which is a tool-surface change. Move profiles before the snapshot (ai-native's order: profiles 1.3, contract after) or label the default profile experimental in the contract.
- Capacity, environment maps at 1.5 and the High tier at 1.9: neither waits on anything after 1.0. They need only the pass structure, the cascade atlas and the asset kinds 1.0 ships, and ADR-012's amendment lists environment maps in the 1.0 baseline without condition. Environment maps belong in 1.1 and the High tier in 1.3 to 1.4.
- Capacity, new features inside 2.0 while the beta runs (reflection probes, irradiance probes, kits, autotile, web leaving preview): a beta tests only what was merged before it. Move them to 1.9 or a 1.10 and keep 2.0 for the switch, the beta and the release candidates.
- ai-native, a tileset and character 'as an engine package' at 1.4: packages cannot carry assets until its own 1.6 (KINDS today is languages, recipes, templates, themes, workspaces). Ship the art inside the templates at 1.4, or move asset packages to 1.3.
- ai-native, the 2D editor mode at 1.4 and Aseprite import at 1.2: both stand on file drop, an import action and import settings that importers read, which it schedules at 1.6. Move asset import to 1.3 or the 2D second half after it (capacity's order).
- ai-native, the full stability contract at 1.6: the fetch layer and online index (1.3), rule documents (1.4), save slots and replay format 2 (1.2) are new formats in strangers' hands three to four releases before the snapshot that guards them. Put the contract at 1.2 or 1.3 and label rules experimental.
- Commercial, 1.1: the presenter, HiDPI, device loss, gamepad, player-indexed actions, replay format 2, save games, scene switching, the editor's first hour, efficiency round 2, the gate and the rules experiment are 24 to 26 packages by the other plans' counts. It is two releases; split it at the format window.
- Commercial, 2D templates and recipes at 1.2 before rules v1 at 1.3: templates and recipes depend on which form of logic won, so they are written twice if the answer is yes. Write the templates after the rules answer (ai-native's reason for moving the 2D content).
- Commercial (touch at 1.9, web at 1.6) and the other two (no touch at all): a shared web link is opened on phones. The built-in pointer input exists, but there is no multi-touch action or virtual stick in wish_input, wish_window or wish_types. Touch as input actions belongs in the web release.
- All three move web export from 1.1 to 1.6, 1.7 or 1.9, and the dependencies prove them right (blocking pollster calls in gpu.rs, wasmtime as the only executor, a player that links the control plane, no presenter). The wrong release is in the documents: VISION.md, ADR-019 and the roadmap must stop promising 1.1 before 1.0 ships.
- Capacity, quality mode at 1.7: it depends only on the gate (1.1), the hard tier and the read-only role. With those moved to 1.2 and 1.3 it fits 1.4 to 1.5, where the accepted draft had it, and the 'first game with an agent' tutorial of 1.5 can then teach the mode people will use.

### Missing from every plan

- A score on a task set the engine's side did not write. All three build their own 'engine-neutral' benchmark; none ports or submits to an existing agent-in-engine benchmark (the research names three, all Godot-based; I could not verify them). The home side writing the tasks is the first objection a neutral reader raises.
- A real game, made agent-first with the engine and released where anyone can play it, with the brief, the transcript, the cost and the replay published. It should be repeated as a dogfood milestone with a friction log every two or three releases (the M7 method), not only 're-dressed showcases' at the end. The research says shipped games and measured outcomes are the proof expected by 2.0, and it is the thing the owner can show.
- A statement at 1.0 of what is not frozen. Efficiency round 2 (default answer shapes), player-indexed actions (the SDK and the world hash), replay format 2 and tool profiles were designed as pre-freeze changes and now come after 1.0, while VISION.md still says the tool surface and SDK source API freeze at 1.0. No plan reconciles this.
- A cost model. Every plan asks for 'a spending cap' and none estimates tokens or money per package or per campaign, although data exists (41 tasks for 4.59 $, eight comparison tasks for 0.53 $). The cut order should trigger on a number per release, because the owner's tokens are the stated limit.
- A support budget for when people do come: who answers issues and reviews outside contributions, and a one-step report bundle (crash report, replay, doctor output) a stranger can attach. Ecosystem and maturity scores depend on answers, and the owner's attention is the scarcest resource in every plan.
- An upgrade checker one release ahead of 2.0 that tells a project and its packages what will change, with a compatibility window for package authors (the Blender 2.8 and Unity 7 lessons). Commercial has the upgrade guide; nobody has the checker.
- A measurable definition of 'small or medium game': entities, physics bodies, draw calls and session length on named hardware. All three theses use the phrase; today about 1,000 active bodies cost 12% of a frame and a replay ends at 54,000 ticks (15 minutes), and no plan says replay format 2 lifts that limit.
- SteamOS as the named console baseline, with a frame budget: Steam Deck at 1280x800 and, if the research holds, Steam Machine at 1080p and 30 frames a second, which inherits Deck Verified. Also a cheap probe of whether today's Windows export runs under Proton. Only commercial measures low-end hardware at all.
- A decision, on paper and late in 1.x, on how history-dependent effects (temporal anti-aliasing, an upscaler, any no-bake GI) could ever be verified under the repeatable-capture rule. Every plan rightly leaves real-time GI out of 2.0, but without this spike graphics stays capped near 6 in 2.x as well, while the bar moves.
- A fallback decision for web reach made from data: WebGPU alone still misses part of Linux and Firefox on Android per the research, and the lights need three storage buffers that WebGL2 does not have. Only commercial mentions it, as an option.
- A funding or token plan for a train of 110 to 150 packages (the research's Bevy lesson: match scope to funding). Whether to accept sponsorship is the user's decision; no plan asks him.

