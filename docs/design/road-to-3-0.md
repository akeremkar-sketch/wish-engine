# The road from 2.0 to 3.0

**Status:** plan, 22 November 2026, not started. It begins when 2.0 is out
([road-to-2-0.md](road-to-2-0.md)). Every number here is an estimate. The
plan is counted again, release by release, when 2.0 has been measured.

## 1. Why a 3.0 plan now

The user, 22 November 2026, after the projected 2.0 comparison: some areas
do not grow as far as wished by 2.0, which is normal for the time, so 3.0
must go much further, in **the engine's interface (the editor and the
in-game interface), stability and backward compatibility, 2D, physics and
graphics quality**, and in every other area where the engine is weak.
Ecosystem and community, and proof by published games, are left out on
purpose: they need people and years, not code.

## 2. What 3.0 is

Wish Engine 3.0 is a **long-term-support** engine for one person, an AI
agent, or both together, that is strong on its own terms in 2D and 3D: an
editor a professional is at home in, an in-game interface toolkit as good as
any, physics, animation and audio deep enough for action games, a picture
that holds up next to the big engines at the scale of an independent game,
online play from a party game to a dedicated server, and a promise that a
project made with 3.0 opens, plays and replays the same years later.

The bet stays the one of 2.0: **proof, not generation.** Every new system is
deterministic where it touches the simulation, observable from the control
plane, testable headless, and judged by the AI scorecard.

**Targets (1 to 10, the comparison of 21 and 22 November 2026; 2.0 is
itself a projection):**

| Area | 2.0 | 3.0 |
|---|---|---|
| Graphics quality | 6 | 8 |
| Platforms | 5 | 7 |
| Maturity and documents | 6 | 8 |
| Editor tools | 7 | 9 |
| 2D | 8 | 10 |
| Performance and scale | 6 | 8 |
| Asset pipeline | 5 | 8 |
| Animation | 5 | 8 |
| Physics and sound | 6 | 8 |
| Online play | 7 | 9 |
| Ease of learning | 6 | 8 |
| In-game interface | 6 | 9 |
| Debugging and profiling | 8 | 9 |
| Stability and backward compatibility | 7 | 9 |
| Determinism, AI, iteration, lightness, licence | 8–10 | kept |

With the weights of the comparison: AI-made small game 74 → about **85**
(first of the seven engines compared), commercial release 59 → about **75**
(third, after Unity and Unreal at 83; the gap is ecosystem, published games
and consoles).

## 3. Rules of the 2.x train

Everything in section 2 of the road to 2.0 holds (something to see in every
release, the scorecard as a gate, two lanes at most, closing reviews), plus:

- **3.0 is an LTS.** It gets patch releases for at least two years: security,
  crashes, data loss, and no change of simulation results (ADR-028 rule 3).
- **Deprecation, not breakage.** Anything a project can use (a tool, an SDK
  item, a file field) is marked deprecated for at least two minor releases,
  with a diagnostic and a hint that names its replacement, before it goes; it
  goes only at 3.0, and only with a migration proven on the corpus.
- **Every new file kind** gets a format number, a canonical writer, a size
  cap, a migration slot and a corpus entry on the day it is born.
- **Every new interface** is built on the registries (commands, panels,
  themes, languages) and is reachable without a mouse.

## 4. The pillars

### 4.1 The engine's interface (editor 2)

- **Docking and multiple windows:** panels that dock, tab, float and move to
  a second screen; layouts saved per person and per project; a layout for
  2D, one for 3D, one for AI work. Built on the panel registry of 1.4.
- **Specialist editors, each built once on the registry:** an animation
  timeline and sequencer, a state-machine graph, a material and shader
  graph (it writes WGSL through the shader library, with the GPU rules), a
  terrain editor, a particle editor with a live preview, a sound mixer.
- **An interface designer** for the in-game interface (4.2): drag, align,
  anchor, preview at several screen sizes and languages, every change one
  control-plane call (invariant 9).
- **Professional manners:** search everywhere, multi-select editing in the
  inspector, prefab variants and nested prefabs in the editor, drag and drop
  from the asset browser into every field, undo of every action, keyboard
  navigation of every panel, a screen-reader pass with AccessKit, interface
  scaling, right-to-left languages.

### 4.2 The in-game interface (UI 2)

- **Interface files** (`*.ui.json`): layouts, styles (a small, deterministic
  subset of CSS: selectors by class and state, variables, transitions),
  templates and lists; hot reload; the same files drawn in the editor and the
  game.
- **Data binding**: a field of the interface shows a component or resource
  value, and a button writes an action, without game code for the plumbing.
- **Text for every language:** shaping (a permissive shaper such as
  `rustybuzz`), fallback fonts, right-to-left, emoji in monochrome, rich text,
  localisation tables with plural rules.
- **Input:** gamepad and keyboard focus everywhere, safe areas, scaling by
  screen density, a virtual keyboard on touch screens.
- **Proof:** golden images of every widget in every look, a layout test at
  ten screen sizes, and scorecard tasks that build a menu, an inventory and a
  settings screen.

### 4.3 Stability and backward compatibility

- **The determinism matrix** of 2.0 grows into a published promise: which
  platforms and executors give the same hashes, checked on every commit.
- **Old replays forever:** a replay names its engine version (ADR-028); the
  engine manager (M10b) opens the right engine side by side, so a replay made
  with 2.4 still verifies under the 2.4 engine when 3.1 is current.
- **A compatibility corpus** of real projects (the examples, the templates,
  the agents' games, projects people send) opened, migrated and played in
  CI for every format and every release since 1.0.
- **ABI shims:** game modules built against an older SDK keep loading
  (ABI 1 and 2 adapters), until the deprecation window says otherwise.
- **`wen upgrade`:** a dry run that lists every change a newer engine would
  make to a project (formats, deprecated items, behaviour changes from the
  changelog), then does it as one undoable step with a checkpoint.
- **Continuous fuzzing** of every file reader, every network message and the
  game-module boundary; a release does not ship with an open crash.

### 4.4 2D, all the way

- 2D lights with normal maps and soft shadows from tilemaps and sprites;
  2D particles on the GPU; parallax layers; a 2D camera kit (follow, confine,
  shake, zoom to fit).
- Importers for Aseprite, LDtk and Tiled; rule tiles and autotiling;
  isometric and hexagonal maps.
- A dedicated 2D physics backend if the measurement of release 1.1 says the
  plane-held 3D one is not enough (`rapier2d`, pinned, with its own
  recorded hashes); one-way platforms, slopes, 2D joints.
- 2D navigation over tilemaps (grid and navigation polygons), shared with
  4.5.
- Pixel-art discipline: integer scaling, no sub-pixel jitter, a palette
  check.
- **A sprite and pixel-art editor inside the editor** (user request,
  22 November 2026: 2D at 10): layers, a palette, brushes, onion skin and
  frame-by-frame animation that writes sheet files; what GameMaker is loved
  for, with every stroke one control-plane call (invariant 9).
- **A 2D animation editor:** a frame timeline for sheets, and 2D skeletal
  animation with mesh deformation (bones, weights, an importer for an open
  format), all in the world's time.
- **2D effects:** outline, dissolve, palette swap, flash and distortion as
  ready nodes of the shader graph (4.1), each with an off and a cost.
- **What the others do not have, kept as the edge:** deterministic 2D
  physics and replays, online 2D with hidden information, multiplayer tested
  by bots without people, and 2D games made and proven by agents.
- **The claim is measured:** a 2D comparison harness (like `tools/compare/`
  with Unity) runs the same 2D tasks in Wish, Godot and GameMaker, by hand
  where a person must and with agents where an engine allows it; "2D at 10"
  is written only when that measurement and the closing review say so.

### 4.5 Physics, navigation and sound

- Joints in the editor, vehicles, ragdolls, continuous collision, physics
  materials and a collision-layer matrix; deterministic islands stepped in
  parallel (same hash as one thread).
- **Navigation:** navigation meshes baked from the scene (3D) and polygons
  from tilemaps (2D), paths, crowds with avoidance, all deterministic and
  queryable from the control plane.
- Sound: buses and a mixer, snapshots, reverb zones, spatial sound with
  occlusion, music with stems and transitions, streaming of long sounds; the
  audio log stays the proof.

### 4.6 Animation

- State machines and blend trees (edited in the graph of 4.1), cross-fades,
  layers and masks, root motion, events on frames.
- Inverse kinematics (two-bone, look-at, foot placement), retargeting
  between skeletons, morph targets.
- All of it in the world's time, so a replay plays an animation the same.

### 4.7 Graphics quality

- A render graph with GPU-driven culling and indirect draws.
- Global illumination from probes (baked and dynamic), reflection probes,
  screen-space reflections, temporal anti-aliasing with an open upscaler,
  volumetric fog and light shafts, decals, a terrain with layers, water.
- Custom shaders from projects, through the shader graph and checked WGSL
  modules, after device-loss recovery has releases behind it.
- HDR output and a third look lock (looks re-tuned on the new lighting, with
  a migration), signed off by the user by eye.
- Quality tiers keep the low end: every effect has an off and a cost in
  `stats`.

### 4.8 Performance, scale and platforms

- Parallel system execution with a deterministic schedule (same hash as one
  thread); 100,000 entities at 60 ticks a second as the target.
- Streaming of large worlds in chunks; level of detail for meshes; memory
  budgets visible in `stats`.
- Game code: Wasm SIMD, ahead-of-time compiled modules, and a cheaper
  boundary (ABI 3 if needed, with shims).
- **Platforms:** the web supported (WebGPU), Android and iOS as previews,
  then supported where the determinism matrix holds; ARM64 Linux. Consoles
  need closed SDKs and developer agreements, so the open engine cannot ship
  them; 3.0 documents a porting layer for those who have them.

### 4.9 Assets

- FBX import through a permissive library (for example `ufbx`), texture
  compression (KTX2 with Basis), mesh optimisation and generated levels of
  detail (for example `meshoptimizer`), import presets per folder.
- A Blender bridge: export from Blender straight into a project, with
  references kept.
- Asset streaming for large projects; thumbnails and audition everywhere.

### 4.10 Online play, all the way

- A dedicated server build of a game (headless, Linux), lag compensation
  for action games, interest management at a hundred players.
- A self-hostable matchmaking and lobby service (`wen lobby`), next to the
  relay; NAT traversal for direct play.
- Rollback for action games (1.7) extended to four players; spectators;
  match recordings that verify.

### 4.11 Learning and maturity

- Interactive tutorials inside the editor (a guided task that checks itself),
  one per genre template; the reference site of 1.3 grown into full manuals.
- The rules language (if the 1.x experiment said yes) as a first-class way to
  write game logic without Rust, editable in a graph view.
- A security review of the network and the module sandbox by an outside
  party (the user's decision and budget).

## 5. Releases

| Release | Theme | Packages (about) | The thing to see |
|---|---|---|---|
| 2.1 | Editor 2: docking, windows, layouts, the panel manners | 14 | the editor on two screens |
| 2.2 | UI 2: interface files, styles, binding, text shaping, the designer | 15 | a menu built by hand and by an agent |
| 2.3 | Stability: corpus, ABI shims, `wen upgrade`, fuzzing, LTS process | 12 | an old project upgraded in one step |
| 2.4 | 2D 3: lights, importers, rule tiles, 2D navigation, cameras, the sprite and pixel-art editor, the 2D animation editor, 2D effects, the 2D comparison | 20 | a lit 2D level drawn, animated and played inside the editor |
| 2.5 | Physics, navigation and sound | 15 | a crowd finding its way; a ragdoll |
| 2.6 | Animation 2 and the graph editors | 14 | a character with a state machine and IK |
| 2.7 | Graphics 2: render graph, GI probes, TAA, SSR, volumetrics, terrain, water | 18 | the showcase under the new light |
| 2.8 | Scale, assets and platforms: parallel systems, streaming, FBX, Android/iOS previews, web supported | 16 | a large world streaming; a game on a phone |
| 2.9 | Online 3: dedicated servers, matchmaking, lag compensation | 12 | a hundred bots on a dedicated server |
| 3.0 | LTS: freeze audit, the third look lock, the determinism promise, a six-week beta | 10 | the comparison table, measured |

About **146 packages**. At the pace of release 1.1 (two lanes) the code is
roughly two to four months of work; the calendar is longer, because each
release has a beta and the LTS has a six-week one.

## 6. Decisions that are the user's

- Mobile stores: an Apple developer account (yearly fee) and a Google Play
  account, if phones are to be more than a preview.
- Hosting a public matchmaking or relay service, or keeping everything
  self-hosted.
- An outside security review (its cost).
- Art for the showcase and the templates under the new graphics.
- The LTS length (two years proposed).

## 7. Risks

- **Scope.** Each pillar could be a year by itself. The cut order: 4.8's
  phones, 4.7's water and terrain, 4.1's terrain editor, 4.10's matchmaking
  service go first; stability (4.3) and the interface (4.1, 4.2) go last.
- **Determinism under parallelism.** Parallel systems and physics islands
  must give the same hash as one thread; if they cannot, they stay off in the
  simulation and are used only where results are not shared.
- **Dependencies.** Every new library (shaper, FBX, texture compression,
  mesh optimisation) is wrapped in its own crate and pinned (ADR-028 rule 4);
  a C library is used only with a permissive licence and inside `guarded`.
- **The comparison moves.** Other engines improve too; the targets are
  checked again at every closing review, with their reasons.
