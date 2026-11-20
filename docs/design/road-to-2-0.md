# The road from 1.0 to 2.0

**Status:** plan, 18 November 2026, not started. It begins when 1.0 (the short
path, [M12-road-to-1-0.md](M12-road-to-1-0.md) section 0) is out. Every number
here is an estimate, not a measurement, and the plan is counted again in
tokens once 1.0 has been measured (section 2).

User decision, 18 November 2026: 1.0 is the short path. Everything else in the
M12 note is done after 1.0, nothing is dropped, and 2.0 is a general plan on
top of that: it may take longer, and its scores must be higher. A better
engine.

## 1. What 2.0 is

Wish Engine 2.0 is a free, open-source game engine for one person, an AI
agent, or both together, to make small and medium 3D and 2D games and ship
them to Windows, Linux, Steam Deck and the browser from one machine. It works
fully by hand with no AI connected.

**The one bet:** proof, not generation, is what makes AI useful for games.
The simulation is deterministic, so the engine itself can say whether a game
still works: one gate that an agent's work must pass, a replay that reproduces
any bug, a server that re-runs a player's run to confirm a score, and the same
bits on Windows, Linux and in a browser. The claim is published with every
release as a scorecard, with the command that reproduces each number.

2.0 is not for teams that need consoles, phones, a large asset store or
high-end graphics. It still trails Godot and Unity on platforms, maturity and
ecosystem, because those need people and years, not code. The first page says
so.

## 2. Rules of the train

These hold for every release from 1.1 to 2.0.

- **Something to see in every release.** Each release has at least one thing
  the user can look at, not only plumbing: a render or platform item (his
  first two wishes) wherever one fits, and otherwise a published game or
  gallery. Each release below names it.
- **The scorecard is a release gate.** Every release is measured with the AI
  scorecard (M12 note, "The AI scorecard") and published with its command. A
  release does not ship if correctness or errors got worse on the held-out
  tasks. Scores are reported with and without outside people.
- **"Supported" has one meaning.** A platform or an executor is called
  supported only when the committed replays give the same hashes on it in CI.
  Until then it is a preview. Replays carry the engine version and the
  executor's id. 2.0 publishes this as a table (the determinism matrix).
- **Formats after 1.0.** A format changes by addition, or with a migration
  proven on a corpus of older projects. There is one format window (1.2), one
  change of the game-code ABI (1.8), and at 2.0 a consolidation with a second
  look lock, each carried by a migration. 2.0 breaks nothing that a migration
  cannot carry, and removes no tool and no SDK item.
- **Simulation results.** A release may change what a simulation computes only
  in a minor version, listed in the changelog, never in a patch.
- **Dependencies.** `wasmtime`, Rapier, `wgpu`, `winit`, `egui` and the packed
  compiler stay pinned. An upgrade is its own package in a minor version,
  because it can change fuel counts, physics hashes or the bytes of a compiled
  game.
- **A maintenance reserve.** 15 to 20 percent of each release is kept for bug
  reports from real users, patch releases and CI upkeep. The package counts
  below do not include it.
- **A budget in tokens.** The owner's limit is tokens and time. The 13
  packages and the first campaign of 1.0 are measured, and each release is
  then planned in tokens. The cut order (section 6) triggers on that number.
- **Stopping points.** After 1.4 and after 1.7 the engine is a coherent,
  published whole. The train can stop at either.
- **One decision sheet per release.** The user's choices, the pictures to
  sign off by eye and the sounds to check by ear are collected once per
  release, not asked one by one.
- **A real game every two or three releases.** A game is made agent-first with
  the engine and released where anyone can play it, with its brief,
  transcript, cost and replay published, and a friction log that is fixed in
  the engine (the M7 method). Three are planned: at 1.6, at 1.9 (playable as
  a link) and during the beta of 2.0.

## 3. The train

About 130 packages in eleven releases, plus the maintenance reserve. That is
ten times the 1.0 short path. A release is 1 to 3 weeks of building and a
one-week release candidate that overlaps the next release.

### 1.1: the loop and the proof (about 14 packages)

- Code freshness: a fingerprint of the game's sources; simulation, tests,
  replays and export never run stale code; unchanged sources skip the
  compiler; templates ship built modules, so the first Play is instant.
- A lean play path, fuel per system, and probes in CI: committed replays on
  Windows and Linux, the engine version in the replay header, hash jobs for
  macOS arm64 and for the simulation core on wasm32 (informational), a window
  smoke test.
- The verification gate: `check` at level full (a fresh build, every scenario
  test, saved replays, a seeded soak, a camera that sees something). Every
  later package exits through it.
- AI efficiency round 2, in two packages: answers that save a turn, and the
  game-code loop (search that returns whole sections, inline tests, failure
  traces with values).
- Background build and build on save.
- The rules experiment (3 packages): the expression language moves into a
  crate below the simulation; rules as data run by an engine system; the same
  hard game-logic tasks done as Rust and as rules, on two models, judged by
  the scorecard. The held-out task set is written here.
- Environment maps as a setting that is off by default (2 packages): the first
  thing to see.
- The editor's first hour (2 packages): orbit, pan, zoom and frame; a Create
  menu; duplicate, copy and paste; new scene; the menu split.
- Closing review and scorecard.

Why first: no open decision except quota and the HDR sky (section 7; the
rules decision comes with its results, at the end); it makes every later
package and every agent session correct and cheaper; and it answers the rules question
before the format window opens. The 2D kit left the draft's 1.1: it stands on
the gamepad, the presenter, editor picking and asset import, which come later.

### 1.2: the format window and the screen (about 14)

- The migration step and a compatibility corpus first: projects written by 1.0
  and 1.1 open, migrate and keep their meaning in CI. A file migrated by
  editing (written in the newer format on its next save) raises the `engine`
  key of `wish.json` the way `wen migrate` does, so that an older launcher
  never picks an engine that refuses it (1.0 closing review, D9).
- Player-indexed actions and replay format 2, as one hash-and-format change.
- Gamepad, with focus navigation for the in-game interface (2 packages).
- Save games (2 packages) and scene switching at run time as recorded,
  deterministic steps; animation cross-fade.
- The presenter (3 packages): drawing straight to the screen with vsync,
  interpolation between ticks, fullscreen, HiDPI, automatic software fallback,
  a forced device-loss test. The editor's own rasteriser is measured at 4K
  here and its draw path decided.
- The hard evaluation tier: whole small games from a brief (a menu, two
  levels, a save), bug-fix and regression tasks, three tries with spread.
- Checks on what a picture shows, inside the gate: text too small, interface
  boxes off screen or overlapping, the subject not visible.
- The showcase games re-dressed with the 1.0 look and environment maps.
- Closing review and scorecard.

Why here: everything that changes a format, a hash or how the picture reaches
the screen is done once, while few 1.0 projects exist. If the rules experiment
said yes, its three packages start here.

### 1.3: every desktop, small (about 14)

- Tool profiles (a core of at most 25 tools for outside agents, opt-in through
  a new project's `.mcp.json`) and a read-only session role.
- The stability contract in full, after the profiles: `docs/STABILITY.md`,
  stable and experimental labels, snapshot tests of the tool catalogue and the
  SDK.
- Components (2 packages): the compiler, the SDK and the export players become
  optional parts; a base package of about 70 MiB and a full one.
- Cross-export between Windows and Linux, with an export that verifies itself
  (runs headless and compares the hash).
- Steam Deck checked in the Steam Linux Runtime in CI; macOS arm64 as an
  unsigned preview (2 packages).
- Quality tiers Low, Medium and High with a render scale; culling, sorted
  draws and instancing; the MSAA measurement on two adapters.
- A reference site generated from the schema (2 packages), with an offline
  copy that Help opens in the browser (the Help panel itself comes with the
  panel registry of 1.4); a `doctor` action for first-run problems.
- Closing review and scorecard.

The download layer (2 packages: HTTPS fetch with checksums and a signed index)
has no release number. It starts when the user decides on it, and nothing in
this release waits for it.

### 1.4: by hand (about 13). First stopping point.

- A panel registry first, so every later panel is built once.
- Overlays and picking (2 packages), gizmo completion, a hierarchy that works
  at scale.
- Asset import and management (2 packages): file drop, import settings that
  importers read, thumbnails, sound audition.
- Console and problems with click-through to file and line; the play loop
  (sound in Play, keep a live edit, time scale, go to a tick, a picture
  preview of an agent's proposal); a profiler; long calls off the frame with
  progress and cancel; the Help panel.
- Ambient occlusion: the thing to see.
- Closing review, scorecard, and the first formal three-person test.

After 1.4 (about 55 packages) platforms, the editor and the loop have most of
their gain.

### 1.5: prove (about 12)

- The quality mode (3 packages): Fast, Balanced and Thorough; plan, build,
  prove with the gate, a read-only review (experimental), repair with caps;
  published as a curve of pass rate against cost.
- Verified runs (2 packages), and replays with a crash ring in the shipped
  player.
- Other agents: `AGENTS.md`, connection snippets, a generic command-line
  adapter.
- Tasks written by someone else: a subset of an outside agent-in-engine
  benchmark ported to this engine, if its licence allows.
- Shadows for spot and point lights (2 packages): the thing to see.
- Closing review and scorecard.

### 1.6: share and teach (about 12)

- Packages carry assets and prefabs, have dependencies and author tooling, and
  can prove themselves (checks as data, tasks with hidden tests).
- A seeded index (2 packages): starter asset packs, genre templates, recipe
  packs. Art as data (procedural textures, simple vector sprites), so an
  agent's result looks finished without a download.
- A feature gallery; tutorials executed in CI (2 packages; English, the first
  lesson in Turkish); honest pages (known limits, coming from another engine)
  and a launch kit.
- The first agent-made game released in public: with the gallery, the thing
  to see.
- Closing review and scorecard.

### 1.7: the 2D kit (about 12). Second stopping point.

- Sprites kept on the GPU, sheets and sprite animation, nine-slice.
- A 2D physics profile, tilemaps with painting (2 packages), a fixed viewport
  with a pixel-perfect camera and the pointer in the world.
- A 2D mode in the editor (2 packages); 2D templates, recipes, a tutorial and
  tasks; deterministic grid paths over tilemaps.
- Closing review and scorecard.

After 1.7 (about 91 packages) everything in the old full scope of M12 is done
except GPU skinning (1.8), the material editor (1.8), MSAA (1.9) and the
four-week beta with its freeze audit (2.0).

### 1.8: smaller and faster (about 10)

- The executor probe and its ADR: how game code runs where `wasmtime` cannot.
  It must answer fuel counts, a fresh instance per tick, NaN handling and
  stack depth before any web code is written.
- A lean runtime shared by the workspace and the player (2 packages); one
  deterministic packed asset file.
- ABI 2, once (2 packages): fewer copies between the engine and game code.
- GPU skinning; a material editor with a preview, and inspector depth; the
  engine's crates building for wasm32 in CI.
- Closing review and scorecard.

Each item pays on the desktop first (a smaller player, bigger scenes), so the
release is worth publishing even if the web slips.

### 1.9: the browser, as a preview (about 11)

- The web executor as the probe decided; a web presenter and shell (2
  packages); `project_export` with a web target, tested in a headless browser.
- Touch as input actions, with a virtual stick: a shared link is opened on
  phones.
- Textured, additive and soft particles, and particles that glow; wider glTF
  import (2 packages).
- The second agent-made game, playable as a link.
- MSAA, only if the measurement of 1.3 passed.
- Whether a reduced fallback for browsers without WebGPU is worth building,
  decided from data.
- Closing review and scorecard.

### 1.10: depth (about 11)

- Reflection probes and baked light probes (3 packages): the look gains depth.
- Experimental kits: game code in packages (2 packages).
- Autotile rules and Tiled/LDtk import; string tables for games in several
  languages; animation blending as a data asset; streamed music with buses.
- An upgrade checker that tells a project and its packages what 2.0 changes.
- The web leaves preview if the determinism matrix passes.
- Closing review and scorecard.

### 2.0: proven in public (about 9, no new features)

- A freeze audit; formats consolidated with migrations proven on the corpus;
  a second look lock (the default look switched once, by the user's eye).
- The determinism matrix published; a stated support window for 1.x.
- A public benchmark on several engines (2 packages; the first thing to cut).
- A beta of at least four weeks with an exposure floor per named platform;
  the formal three-person test again; release candidates; 2.0.0 and a planned
  2.0.1.

## 4. Scores

The comparison report's scale and weights. The 2.0 column is what the two
critics would defend to a neutral reviewer; the three planners had written
half a point to a point more in several rows.

| Area | Today | 1.0 | 2.0, by code alone | 2.0, if outside people come | Godot 4 | Unity 6 |
|---|---|---|---|---|---|---|
| Graphics quality | 3 | 5 | 6 | 6 | 6 | 8 |
| Platform support | 1 | 3 | 6 | 6.5 | 8 | 10 |
| Ecosystem and community | 1 | 1 | 3.5 | 4.5 | 7 | 10 |
| Maturity and docs | 2 | 3 | 5 | 6 | 8 | 9 |
| Editor tools | 4 | 4 | 7 | 7 | 8 | 9 |
| 2D games | 4 | 4 | 7.5 | 7.5 | 9 | 8 |
| Determinism, replay, tests | 10 | 10 | 10 | 10 | 4 | 4 |
| Working with AI | 9 | 9 | 9.5 | 10 | 5 | 7 |
| Code iteration speed | 8 | 8 | 9 | 9.5 | 9 | 5 |
| Lightness and install | 7 | 7 | 8.5 to 9 | 9 | 10 | 4 |
| Licence and cost | 10 | 10 | 10 | 10 | 10 | 6 |
| **"AI with small games"** | **64** | **67** | **81** | **84** | 72 | 68 |
| **"Commercial release"** | **36** | **44** | **65** | **69** | 77 | 83 |

On the way (this note's own estimate): about 75 and 55 after 1.4, about 79 and
60 after 1.7.

What the numbers say, plainly:

- 2.0 is a clearly better engine than 1.0, and it leads Godot and Unity on the
  total the engine is built for.
- It does not close the commercial gap. That gap is community, platforms and
  years, and no package count closes it.
- Graphics above 6 and ecosystem above 3.5 cannot be bought with packages
  alone. Graphics above 6 needs effects that depend on earlier frames
  (temporal anti-aliasing, an upscaler, real-time global illumination), and
  those break repeatable single-frame captures, the base of every golden
  test. How they could ever be verified is a design question to answer late in
  1.x. Ecosystem above 3.5 is the work of outside authors.
- "Working with AI" reaches 10 only with a score on tasks the engine's side
  did not write and an outside rerun. By 2027 every large engine has a free
  first-party agent surface; the lead is the closed, deterministic loop and
  published proof, not having tools.
- The "if outside people come" column also assumes the rules experiment says
  yes.

## 5. Not in 2.0

Scheduled after 2.0, not dropped:

- Android and iOS: devices, store accounts, a Mac and an executor without a
  JIT are needed, and CI alone cannot prove them. The lean runtime, packed
  assets and the presenter are their groundwork.
- Consoles: the SDKs are under NDA and cannot live in an open repository. The
  honest deliverable is a page that names the third-party route.
- A signed, notarised macOS build: one package once the user has a paid Apple
  account and a Mac user.
- Docking and specialist editors (an animation timeline, a shader graph,
  terrain): each is the size of a milestone.
- Temporal anti-aliasing, upscalers, real-time global illumination,
  screen-space reflections, volumetric fog: see section 4.
- Custom shaders from projects: after device-loss recovery has releases behind
  it.
- 2D lights and shadows, 2D skeletal animation, a 2D physics backend.
- Rollback netcode and multiplayer; parallel system execution.
- Several agents at once with merging forks, and the engine's own text
  language: experiments, and only if the rules experiment says yes.
- Editor and tool plugins (`wish_ext`); Steamworks and kernel anti-cheat as
  player variants.

## 6. The cut order

If tokens or time run short, cut in this order:

1. What the user did not ask for: the public benchmark, extra agent adapters,
   time-travel depth.
2. Second-tier depth: the material editor and inspector depth, half the
   templates and starter packs, autotile and Tiled import, kits, wider import.
3. By gain per package: MSAA, light probes, reflection probes. This conflicts
   with the user's order (graphics cut last); the plan follows his order until
   he says otherwise.
4. The macOS preview and the download layer.
5. The web chain, and only as a whole.

Never cut: code freshness and the verification gate, the format window with
its migration step and corpus, the presenter, components and cross-export, the
scorecard at every release, the stability contract, the closing reviews, the
four-week beta before 2.0.

## 7. Decisions that are the user's

Decided before the release candidates of 1.0 (M12 note, section 0): the name,
the download address, merging to `main`. Carried into the train: the download
layer inside the engine (no release number).

| When | Decision |
|---|---|
| Now | A spending cap per measurement campaign, instead of a yes each time |
| 1.1 | Rules: yes or no, with the results (a yes adds 3 packages) |
| 1.1 | An HDR sky to ship: generated in code, or a CC0 download |
| 1.2 | A gamepad library (a new dependency) or own code; a gamepad in hand |
| 1.3 | Where the line between the base and the extras runs; GitHub Pages |
| 1.4 | A folder-dialog library or own code |
| 1.5 | Approval to install other agent programs and to download an outside benchmark |
| 1.5 | How outside issues and pull requests are handled |
| 1.6 | Where the art comes from; a place for people to talk |
| 1.9 | What the executor probe names; `wasm-bindgen` and a headless browser |
| 2.0 | A call for testers; whether to ship if fewer than ten people came |
| Optional | A Windows code-signing certificate; an Apple Developer account; sponsorship |

Outside people: one or two informal runs at 1.3; three for the formal test at
1.4 and again at 2.0; ten or more for the 2.0 beta; a Steam Deck owner, a Mac
user and a Linux desktop user before those words are used without "preview".
The hardware question is open too: no Mac, Steam Deck or HiDPI screen is
owned, and audio has never been checked by ear.

## 8. Risks

- **The count is low.** The honest total is 125 to 135 packages and a reserve
  on top. Mitigation: every release is counted again at its start; two
  stopping points.
- **Tokens run out before 2.0.** Mitigation: the order is by dependency and
  then by gain; every release is a published end state; the cut order is
  written.
- **The owner's attention is the real limit.** Mitigation: one decision sheet
  per release; a spending cap replaces per-run approvals.
- **Outside people do not come.** Mitigation: every score is given without
  them; clocks start early (public releases from 1.1, the docs site at 1.3,
  playable links at 1.9).
- **The web fails late.** Mitigation: a wasm32 hash job from 1.1; the probe
  before any web code; a preview label for one release; the groundwork pays on
  the desktop anyway.
- **Competitors' agent surfaces improve.** Mitigation: the comparison tasks
  are re-run with each scorecard, and the lead is stated as measured.
- **Rules make two ways to write logic.** Mitigation: they enter only by the
  written rule (no worse on correctness for any task, better overall).
- **Handling outside issues and pull requests** when agents write the code and
  one person approves: not solved here; it needs a decision before 1.6.

## 9. How this plan was made

A read-only panel of agents, on 18 November 2026. Three planners each wrote a
complete train through one lens (the AI-native leap, 156 packages; closing the
commercial gap, 126; capacity and risk, 113). One researcher looked at where
the bar will stand in 2027 and 2028. Two critics judged the plans
(feasibility; honest scores and the user's priorities). Both ranked the
capacity plan first (7.5 and 7 of 10) and the commercial plan last, and both
asked for the same changes, which this note makes:

- the AI spine of the AI-native plan pulled forward (the hard tier at 1.2,
  profiles at 1.3, the quality mode at 1.5);
- something visible in every release instead of the High tier held to 1.9;
- a 2.0 with no new features, and the naming rule for "supported";
- a maintenance reserve, a dependency policy, a rule for simulation results
  and a budget in tokens, which no plan had.

Their full output is in [road-to-2-0-panel.md](road-to-2-0-panel.md). It is
agent-written: verify a claim against the code before building on it. The web
research was not checked by a person.
