# Wish Engine — Architecture Decisions (ADR)

Each decision has: **what**, **why**, **rejected alternatives**, **risk and mitigation**, **re-evaluation trigger**.
Status: **Accepted** (applied) or **Provisional** (verified at the start of the relevant milestone).

Changing a decision is allowed; silently breaking it is not. A change is made with a new record in this file.

## Summary

| # | Topic | Decision | Status |
|---|---|---|---|
| 001 | Language | Rust (stable) | Accepted |
| 002 | Library strategy | The architecture is ours, mature libraries are used by wrapping them; we do not build on Bevy/Godot | Accepted |
| 003 | Data model | Schema first: storage, JSON, MCP schema, Wasm ABI, editor from a single schema | Accepted |
| 004 | ECS | Custom archetype ECS with schema-defined storage | Accepted |
| 005 | Time and determinism | Fixed step, seeded randomness, explicit system order; guarantee level D1 | Accepted |
| 006 | Snapshot / replay | World snapshot, restore, text-based replay record, hash per tick | Accepted |
| 007 | File formats | Strict JSON + canonical formatter + id-keyed structure + path-based references | Accepted |
| 008 | Source of truth | Files; two equal editing paths (direct file, MCP operation) | Accepted |
| 009 | Control plane | One internal API; MCP, command line and editor are its faces | Accepted |
| 010 | MCP tool design | A small number of high-leverage tools, batch operations, token budget, structured error | Accepted |
| 011 | Game logic | Wasm (wasmtime), stateless guest modules, first SDK Rust | Accepted |
| 012 | Render | wgpu, render graph, baseline first (at 1.0: cascaded shadows, sky-lit ambient, fog, colour grading, FXAA and bloom; environment maps in 1.1; MSAA only if the two-adapter measurement allows it), then layered advanced features | Accepted (amended twice, M12) |
| 013 | Shaders | WGSL + WESL module system | Provisional (amended, M12) |
| 014 | Physics | Rapier 3D, behind our own component API | Accepted |
| 015 | Audio | cpal + our own mixer (Kira dropped, M6) + observable audio event log | Accepted (updated) |
| 016 | Input | Text-defined action maps, injection and recording | Accepted |
| 017 | Testing | Scenario tests, golden images, hash verification, agent evals | Accepted |
| 018 | Interface | **Hybrid:** AI cockpit + full manual editing, both first-class; the engine works completely without AI too | Accepted (interface technology Provisional; amended, M12) |
| 019 | Platform | Windows and Linux as packaged hosts at 1.0; export to the other system, Steam Deck and a labelled macOS preview in 1.3; web constraints respected from the start, web export a preview in 1.9 | Accepted (amended twice, M12) |
| 020 | Scope direction | 3D core; 2D a subset at 1.0 and a first-class kit on top of the core in 1.7 | Accepted (amended twice, M12) |
| 021 | Conventions | glTF coordinate system, SI units, everything in the repository in English | Accepted |
| 022 | Security | Writes limited to the project folder, local network only, Wasm resource limits | Accepted |
| 023 | Product and distribution | Open source (MIT + Apache-2.0), free, no commission; Steam + web; zero install as one package per system at 1.0, and as a small base, on-demand components and a full offline bundle in 1.3; game export | Accepted (amended twice, M12) |
| 024 | AI connection | The user brings their own AI; provider-independent adapter, first provider Claude | Accepted (method Provisional) |
| 025 | Compiling and instantiating game code | A single `rustc` call without cargo + the prebuilt SDK shipped with the engine; no host linker; a fresh instance per module per tick | Accepted (with the M5 WP1 measurement) |
| 026 | Simulation state outside the ECS | `World` extensions: carried by snapshots, not part of the hash; every value the game sees is written back to components every tick | Accepted |
| 027 | Four rules before the freeze | Schema defaults are part of the file format; tool answers carry no wall-clock numbers; versions are three numbers; every public claim has a command that reproduces it | Accepted (code follow-ups open) |
| 028 | Rules of the release train | "Supported" means the committed replays give the same hashes on that platform and executor in CI, otherwise "preview"; replays carry the engine version and the executor id; simulation results and pinned dependencies change only in a minor version; formats after 1.0 change by addition or with a migration proven on a corpus | Accepted (applies from 1.1; code follow-ups open) |

---

## ADR-001 — Language: Rust

**Decision.** The whole engine is Rust (stable channel, current edition). The version is pinned with `rust-toolchain.toml`.

**Why.** Memory safety and data race safety at compile time; compiler errors are the best quality feedback source for the agent ("if it compiles, it is most likely right"); first-class ecosystems for everything we need, such as wgpu, wasmtime and rapier, are in Rust; Claude is strong in Rust.

**Alternatives.** C++ (widest game ecosystem, but memory bugs are silent and expensive in the agent loop), Zig (ecosystem not mature), C#/TypeScript (garbage collector, performance ceiling, determinism gets harder).

**Risk and mitigation.** Build times → small crates, incremental builds, game logic is in Wasm so the engine is rarely rebuilt. `unsafe` code → only inside `wish_ecs` and `wish_script`, tested with Miri.

**Trigger.** None (foundational decision).

## ADR-002 — The architecture is ours, we do not reinvent the wheel

**Decision.** The engine's architecture and invariants belong to us. Mature libraries that do one job well are used, and each is wrapped behind our own crate (so they stay replaceable):
`wgpu` (graphics), `winit` (window), `glam` (math), `rapier3d` (physics), `kira` (audio), `wasmtime` (Wasm), `gltf` + `image` (import), `rmcp` (MCP), `serde`/`serde_json`, `notify` (file watching), `gilrs` (gamepad), `egui` (editor), `taffy` + `cosmic-text` (in-game UI).
Exact versions are pinned to the latest stable versions at the start of M1.

**Why.** Writing a graphics API abstraction or a physics solver takes years and makes no difference. Our difference is in the data model, determinism and the agent interface.

**Alternative: an MCP layer on top of Bevy or Godot.** It gets to the first result much faster. Reason for rejection: the schema-first, determinism, snapshot and transaction invariants have to be guaranteed in the world store itself; adding them later to someone else's engine repeats the "patch" problem we are trying to avoid. *Honest note: if the only aim were "making a game quickly with AI", the right choice would be Bevy/Godot. Because the aim is the engine itself, we choose this path.*

**Risk and mitigation.** Library API changes → the wrapper crates keep the impact in a single place.

## ADR-003 — Schema-first data model

**Decision.** Every component, resource and event type has a runtime schema (`TypeSchema`). The following are derived from a single definition:

1. Memory layout (ECS columns)
2. JSON read/write (files and MCP)
3. JSON Schema (MCP `schema_get`, editor validation)
4. Wasm ABI layout (guest SDK bindings)
5. Editor inspector interface
6. Documentation

In Rust a type is defined once with `#[component]`; field doc comments (`///`), unit, range and default value are carried into the schema. With `schema_get` the agent sees the meaning, unit and valid range of a field.

**Why.** The agent's most frequent mistake is guessing field names/types/units. One source, six consumers: there is no second definition to keep in sync.

**Alternatives.** A separate interface definition language (the burden of keeping two sources in sync), compile-time reflection only (does not cover types that come from Wasm).

**Risk and mitigation.** The supported field types are limited (scalars, vectors, quaternions, color, enum, fixed array, text, list, optional, entity reference, asset file reference). This limit is deliberate: it makes the snapshot and hash cheap.

## ADR-004 — Custom archetype ECS

**Decision.** Our own archetype-based ECS (`wish_ecs`): columns are stored with byte layouts derived from the schema. Features: stable entity ids (index + generation), deterministic iteration order, deferred commands (applied in a fixed order), resources, schema-defined events, change tracking, an engine-managed hierarchy (parent/child).

**Why.** Components must be definable at run time (from a Wasm module); the world must be copyable and hashable in one go; the order must be deterministic. These three are properties of the storage layer.

**Alternatives.** `bevy_ecs` (mature and fast, but dynamic components are second-class, determinism and snapshot are not guaranteed), `flecs` (C, strong reflection but the Rust binding and the Wasm boundary add overhead).

**Risk and mitigation.** Performance and `unsafe` bugs → Miri + property-based tests (proptest), early benchmark; first a correct single-threaded version, parallelism later.

**Trigger.** If the benchmarks at the end of M1 are far below the target (100 thousand entities, 60 Hz), the option of a schema layer on top of `bevy_ecs` is re-evaluated.

## ADR-005 — Time and determinism

**Decision.**
- The simulation advances with a fixed step (default 60 Hz); render is separate and interpolates.
- There is no wall clock in the simulation. All randomness comes from the seeded `Rng` resource (derived streams per system).
- System order is explicit and complete (stable topological sort, ties broken by name). Parallel execution is only for systems with disjoint access; command buffers are merged in a fixed order.
- **Guarantee level D1:** same build + same platform + same scene/seed/input → bit-identical world hash.
- **D2 (cross-platform)** is a goal, not a guarantee: approached with Rapier `enhanced-determinism`, `libm`-based math in the simulation, and NaN canonicalization in Wasm.
- Render is **outside** the determinism boundary (GPU results are not bit-identical across drivers).

**Why.** Bug reproduction, automatic testing, rewind and later rollback netcode — all depend on this. For the agent, the trust that "the same steps give the same result" is essential.

**Risk and mitigation.** Determinism leaks (hash map iteration order, thread timing) → a "run twice, compare hashes" test in CI on every change; unordered collections are banned in simulation code (lint).

## ADR-006 — Snapshot, restore, replay

**Decision.** Snapshot = ECS columns + resources + physics state + RNG + tick counter. Replay record = start scene + seed + input stream per tick (text, JSON per line). A world hash can be computed for every tick; in replay, drift is reported at the first tick where it is detected. A session can be forked (two different attempts from the same moment).

**Why.** So the agent can "undo and try differently"; so bugs can be shared as files; so tests can set up their starting state cheaply.

**Risk and mitigation.** Wasm guest memory is not part of the snapshot → rule: guest modules are stateless, all state lives in the world (ADR-011). The engine enforces this by re-instantiating the module whenever it wants.

## ADR-007 — File formats

**Decision.**
- Format: **strict JSON**. The representation on disk and the representation sent over MCP are the same (a single representation).
- `wen fmt` canonical formatter: stable key order, stable number notation → clean diffs, byte-level round trip.
- Structures are **id-keyed**, not arrays: in a scene entities are in the form `"entities": { "player": {...} }`. Patches are addressed by id, not by index (`/entities/player/components/Transform/translation`).
- References are **paths relative to the project root** (`"assets/models/crate.glb#Mesh0"`), not GUIDs. `wen mv` updates all references when moving.
- Extensions: `*.scene.json`, `*.prefab.json` (instance = reference + overrides), `*.mat.json`, `input.json`, `*.test.json`, project manifest `wish.json`.
- Binary source assets stay in standard formats (glTF/GLB, PNG/KTX2, OGG/WAV) and carry an import setting `*.meta.json` next to them.
- Every file has a `"format": N` version field; old versions are upgraded with `wen migrate`.
- Big scenes are split into sub-scenes.

**Why JSON.** It is the structured format language models produce most reliably; tool support in every language; validation with JSON Schema; MCP is already JSON. RON is Rust-specific and models have seen less of it; TOML is weak with deep hierarchies; YAML is prone to indentation errors.

**Risk and mitigation.** No comment lines → an optional `notes` field on entities and components. Level of detail → files are kept small with prefabs + sub-scenes; the agent reads the part relevant to the query, not the whole file.

## ADR-008 — Files are the source of truth

**Decision.** Project state is the files on disk. There are two equal editing paths:
1. **Direct file editing** → file watcher → validation → hot reload. `wen check --json` gives structured diagnostics like a compiler.
2. **MCP `edit_apply` operation** → in-memory document → canonical file write.

Both produce the same canonical output. Play sessions (simulation) **never** write to scene files on their own; making something persistent is an explicit operation.

**Why.** Claude Code is very good at editing text files; instead of forbidding this, we make it safe. Versioning with git works naturally.

**Risk and mitigation.** Concurrent editing (human in the editor, agent over MCP) → every document has a revision number; operations pass an optimistic concurrency check with `base_revision`.

## ADR-009 — One control plane

**Decision.** `wish_control` is the single transport-independent API (commands, queries, sessions). `wish_mcp`, `wish_cli` and `wish_editor` are its thin clients. Rule: **a capability that is not in the control plane cannot be in the editor either.** MCP transports: stdio (started by Claude Code/Desktop) and Streamable HTTP (connecting to a running editor/game).

**Why.** It makes the class of problems "it exists in the editor but the agent cannot do it" structurally impossible. Because MCP is an open standard, other AIs can use the same surface too.

## ADR-010 — MCP tool design principles

**Decision.**
- **Few and powerful tools** (~20–25). Too many tools lower the agent's selection accuracy.
- **Batch operations:** many changes in a single call, atomic.
- **Token budget:** default responses are short; go deeper with the `detail` level and paging. Images are scaled down by default.
- **Structured error:** `code`, `message`, `path`, `hint`, `candidates`, `docs`. Every error says "what is wrong" and "how to fix it".
- **Every call that changes something** returns a diff, diagnostics and the new revision; `dry_run` is supported.
- **Docs are exposed as MCP resources** (`wish://docs/...`); the agent pulls them when needed.
- **Naming:** `name_action` (`entity_query`, `sim_step`).
- The design is verified not by intuition but by **agent evals** (ADR-017).

Draft tool list: [MCP_API.md](MCP_API.md).

## ADR-011 — Game logic in Wasm

**Decision.**
- Game logic runs inside `wasmtime` as WebAssembly modules. The first guest SDK is Rust (`wish_sdk`).
- Core Wasm modules + a versioned, small ABI. (The Component Model/WIT is followed; a switch is considered when browsers support it natively.)
- **Guests are stateless:** all game state lives in the ECS → **lossless hot reload** when code changes, complete snapshots.
- **Sandbox:** no file, network or clock access; only the API the engine provides. Fuel/time limit: an infinite loop does not freeze the engine, it produces a diagnostic. Trap → the system concerned is disabled, the engine keeps running.
- Component schemas are carried inside the module; they register with the engine when it loads.
- Systems declare the components they access; the engine transfers data in bulk (as column slices).
- Performance-critical extensions that extend the engine can be native Rust (not hot reloaded).

**Why.** Code written by the agent is untrusted: it can crash, it can enter an infinite loop. Wasm isolates this. It gives refresh within seconds without rebuilding the engine. Wasm is deterministic at its core. Other languages can be added later.

**Alternatives.** Native dynamic library hot loading (fragile, no isolation), embedded scripting language (Lua etc.: weak type safety, agent errors are left to run time).

**Risk and mitigation.** Host–guest boundary cost → bulk data transfer; benchmark in M5. Compile time of Rust guest code → small crate, incremental build, target < 10 s. A user who does not know Rust → the code is mostly written by the agent; a second guest language is after 1.0.

**Trigger.** If the boundary cost in the M5 benchmark exceeds 20% of the frame budget, shared memory views are designed.

**Measurement (M5 WP7, 16 November 2026, `wen bench --script`):** the same system with 10,000 entities takes 0.014 ms native and 1.87 ms as game code → the boundary is **11.2% of the frame budget** (below the trigger); fixed cost per tick 0.04 ms. Details and improvement notes: `docs/design/M5-game-logic.md` §17 WP7. **After closing (M5 WP9):** write-back checks every value (finite float, valid bool/enum) and writes only the rows that differ from what was sent; the same measurement is ~240 ns/entity, **14%** (below the trigger).

## ADR-012 — Render

**Decision.**
- `wgpu` (DX12/Vulkan on Windows; the same code on Metal and WebGPU).
- **Render graph** architecture: passes are independent nodes; new techniques are added without breaking the existing ones.
- **Baseline first (1.0):** clustered forward+, glTF-compatible PBR, cascaded shadow maps, HDR + tone mapping, ambient lighting (IBL), MSAA, basic post-processing. *(Made exact on 18 November 2026: see the amendment below; MSAA is now conditional.)*
- **Advanced layer (after 1.0, native platforms):** GPU-driven drawing (indirect draw + culling with a compute shader), bindless resources, meshlet/virtual geometry, ray-traced lighting. The target hardware (RTX class) supports this.
- **Headless render from day one:** no window, draw to a texture; software adapter in CI.
- **Debug outputs are first-class for the agent:** entity id buffer, depth, normals, wireframe, colliders. The id buffer is the basis of "semantic vision" (which entity is where, what percentage is visible).

**Why.** WebGPU is the portable graphics standard of the future. The render graph makes it possible to add "forward-looking" choices later; the baseline keeps the project finishable.

**Risk and mitigation.** Advanced features are not on the web → capability layers; the baseline runs everywhere. Scope → the advanced layer is not tied to 1.0.

**Amendment (M12, 18 November 2026; the user's decision on the 1.0 scope, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md)).** The baseline above was the decision from the start, but M3–M9 built only a part of it: one shadow map fitted to the whole scene, a flat ambient colour, no post-processing, no anti-aliasing (the plan's §2). The M11 list in [ROADMAP.md](ROADMAP.md) had then moved cascaded shadow maps to after 1.0, against this record. The list was wrong and has been corrected; the baseline is confirmed and made exact:

- **In the 1.0 baseline, without condition:** cascaded shadow maps (with alpha-masked casters), image-based lighting (sky-lit ambient and environment maps) and basic post-processing (bloom on the HDR target, fog, colour grading, FXAA).
- **MSAA stays in the baseline only if a measurement says so.** Once the render pass structure of M12 Phase 1 exists, MSAA 4x is measured on the two software adapters the golden images rest on (WARP and lavapipe). If the two agree within the tolerance golden images are compared at, MSAA stays, as a setting that is not the default; if they do not, it cannot be golden-tested, so by ADR-017's rule it cannot count as done: FXAA is then the anti-aliasing of 1.0 and MSAA moves to the advanced layer. The result of the measurement is recorded here.
- **Past the baseline, planned but not promised by this record:** culling and instancing, ambient occlusion, spot-light shadows and GPU skinning, as High-tier settings that are not the default, after the look lock of M12 Phase 2.
- **The advanced layer** is unchanged and still not tied to 1.0; global illumination, reflection probes and TAA join it.
- The baseline still stays inside the WebGPU core (ADR-019), so the web export of 1.1 can draw the same picture.
- The defaults that make up the look are data that every scene file depends on (ADR-027, rule 1): the default look is switched once, at the look lock, and signed off by the user by eye.

**Amendment (cut to the short path on 18 November 2026; the user's second decision of that day, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md) §3).** The amendment above describes the full baseline. It stands as the baseline this record aims at, and nothing in it is dropped, but 1.0 ships a part of it and the rest arrives release by release:

- **In 1.0:** the render pass structure with pooled targets and one `RenderSettings`; cascaded shadow maps with alpha-masked casters; sky-lit ambient with a sun disc; fog, colour grading and FXAA; bloom on the HDR target; material files and textures on primitives; an alpha-blended pass; and the look lock.
- **Environment maps: release 1.1**, as a setting that is off by default. So "image-based lighting" at 1.0 means the sky-lit ambient only.
- **The MSAA measurement: release 1.3** (not "right after the pass structure"), with the rule above unchanged. MSAA itself is in 1.9, and only if the measurement passed; FXAA is the anti-aliasing of 1.0 either way.
- **Past the baseline:** quality tiers (Low, Medium, High) with a render scale, culling, sorted draws and instancing in 1.3; ambient occlusion in 1.4; shadows for spot and point lights in 1.5; GPU skinning in 1.8; textured, additive and soft particles in 1.9. Reflection probes and baked light probes are now scheduled, in 1.10, and leave the undated advanced layer.
- **The advanced layer** keeps the rest and is after 2.0: temporal anti-aliasing, upscalers and real-time global illumination depend on earlier frames, which breaks the repeatable single-frame captures every golden test rests on; screen-space reflections and volumetric fog are after 2.0 with them.
- **The look lock is in 1.0** and is still the one switch of the default look before 1.0; a second look lock is planned at 2.0, again signed off by the user by eye (ADR-027, rule 1, as amended).
- The web export the baseline must be able to draw for is a preview in 1.9, not "the web export of 1.1" (ADR-019, as amended).

## ADR-013 — Shader language *(Provisional)*

**Decision.** WGSL; WESL for modules/imports. Materials are defined with data (`*.mat.json`: shader + parameters); custom shaders are text files and are hot loaded. Compile errors are converted to the shared diagnostic format.

**Trigger.** At the start of M3 the maturity of the WESL tools is verified; if insufficient, `naga_oil` or our own simple import preprocessor.

**M3 verification (15 November 2026, M3 WP1).** Trial with `wesl` 0.6: imports work and the output is valid WGSL for `naga`; a syntax error in an imported module comes with module path + line/column. But (1) the error of importing a missing item is **without a location** ("in unknown module"), (2) `naga` validation errors point to the **converted names** in the merged output (`package_util_double`), (3) the version is pre-1.0 (the API is changing), (4) the tree is 83 packages. Invariant 5 (every error has a location and a hint) cannot be met with WESL today.
**Decision (for M3):** WGSL, parsing + validation with `naga` (`shader_syntax`/`shader_validation` diagnostics with a location, character column). For the engine's own shaders, module merging with a small, source-mapped import preprocessor behind a `ShaderLibrary` interface (WP3); a `naga` error is translated back to the original file/line with the source map. WESL is kept so that it can be put behind this interface later. **Re-evaluation:** when WESL reaches 1.0, or at the latest when user shaders (`*.mat.json`, M6) arrive. `naga_oil` was not tried: the version cycle tied to Bevy and its own syntax would make a later move to WESL harder.

**Amendment (18 November 2026; 1.0 is the short path, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md)).** Material files are in 1.0 (item 7 of the short path). Custom shaders from projects are after 2.0 (the train's §5: after device-loss recovery has releases behind it). Until then the shaders that are text files and hot loaded are the engine's own (`wen shaders`, M3 WP7).

## ADR-014 — Physics: Rapier

**Decision.** `rapier3d`, behind our own components (`RigidBody`, `Collider`, …). Runs with a fixed step; its state is serializable (goes into the snapshot); `enhanced-determinism` is on.

**Alternatives.** Jolt (very strong, but C++ binding, build and snapshot complexity), our own physics engine (out of scope).

**Trigger.** If character controller or vehicle physics needs exceed Rapier, Jolt is re-evaluated; thanks to the wrapper, game code is not affected.

## ADR-015 — Audio: cpal + our own mixer

**Decision.** Output is `cpal` (Apache-2.0) and `wish_audio`'s own small mixer; decoders are `hound` (WAV) and `lewton` (Ogg Vorbis). **Update (M6 WP4, 16 November 2026):** the first decision was `kira`; because Kira's dependencies (`symphonia`, `triple_buffer`) are MPL-2.0, it did not fit the license rule (only MIT/Apache/BSD-like) and was dropped. Because the agent cannot hear sound, an **audio event log** is kept ("tick 412: `jump.ogg` played, volume 0.8, position …") and can be queried from the control plane. Audio output is outside the determinism boundary; audio *events* are inside it.

## ADR-016 — Input: action maps

**Decision.** Game code reads not the raw device but only **actions** (`move`, `jump`). The mappings are in `input.json`. The control plane can inject, record and replay actions.

**Why.** A single clean point so the agent can actually play the game and tests can script the input.

## ADR-017 — Testing and verification

**Decision.** Four layers:
1. **Unit/property tests** (Rust; Miri for the ECS).
2. **Scenario tests** (`*.test.json`): load a scene → give input → advance N ticks → verify an expression on the world / compare an image.
3. **Determinism tests:** run the same replay twice, compare the hashes; snapshot → restore → continue = uninterrupted run.
4. **Agent evals** (`evals/`): an agent does tasks such as "make a bouncing ball", "build a level with three platforms" over MCP; success rate, number of tool calls and token cost are measured. It is also deliberately run with a **mid-tier model**: if the engine works smoothly even with a weak model, it is really agent-friendly.

Golden image comparisons are perceptually tolerant and run in CI with a fixed software adapter.

**Rule.** A feature is **not considered finished** until it can be observed/inspected from the control plane and has a headless test.

## ADR-018 — Interface: hybrid (AI cockpit + full manual editing)

**Decision — hybrid.** *(User decision, 11 November 2026.)* The two ways of working are **first-class at an equal level** and the user switches between them at any moment:

- **With AI:** cockpit elements (below).
- **By hand:** classic editor tools as in today's engines.

Four rules of the hybrid:

1. **Fully usable without AI.** When no AI is connected (no account, no internet, quota used up), the engine works completely: building scenes, editing components, importing assets, materials, playing, debugging, testing, export.
2. **Two-way parity.** Every scene/entity operation the agent can do can also be done by hand; every operation done by hand is also a control plane operation the agent could do.
3. **One history.** Changes made by hand and by the agent go into the same operation/revision/undo stack and the same activity feed; every operation carries who did it (`actor`: human or agent name).
4. **Handover.** The agent sees the changes the human makes by hand ("the object I just moved" is understood). While the agent works, the human can edit another place; the entities the human touched are reported to the agent.

**Manual editing tools (1.0):** viewport and camera navigation; hierarchy; inspector (generated **automatically** from the schema: every component, including those defined in Wasm, can be edited without writing extra code); move/rotate/scale handles; asset browser (drag and drop); material editor; prefab editing; input map editor; play/pause/step/rewind controls and snapshot timeline; console and diagnostics panel; performance panel; project settings.

**Game code** can also be written by hand: with an external code editor (e.g. VS Code) — the engine watches the file and hot loads it. A full code editor inside the engine is not in the 1.0 scope (there is "open in external editor" + a read-only viewer).

**Expert tools outside 1.0:** animation timeline editor, visual shader graph, terrain sculpting, level painting brushes. Until 1.0 these are done with an external tool (Blender), a text file or AI.

**AI cockpit elements.** The interface is not a classic editor with a chat window added; the AI elements are part of the design from the start:

1. **Viewport + conversation** side by side; the scene is visible while a request is typed.
2. **"Show and tell":** selecting an object, region or time range in the scene is added to the request as structured context (entity id, screen box, tick range).
3. **Activity feed:** every agent operation is a card (diff + reason + result); undo per card.
4. **Automatic checkpoints:** before every agent task a snapshot and document revision are marked.
5. **Before/after comparison** and accept/reject.
6. **Permission modes:** suggest only · apply-and-review · fully autonomous.
7. **Live observation:** the agent's sessions, the images it captured, test results.

All changes (by hand or by agent) go through the control plane; the interface has no special authority. Human and agent can edit at the same time (ADR-008 revisions). Interface languages: Turkish and English.

**Why hybrid.**
- Small and visual jobs (moving an object a little, picking a color) are faster by hand than describing them; big and repetitive jobs are faster with AI. The two complement each other.
- Today's models need a human for work that requires taste and fine visual tuning. If AI were the only way, every mistake of the model would lock the user out.
- A user who has no AI account, works offline or has run out of quota must also be able to use the engine. The user having control gives trust.
- A correction a human makes by hand is the fastest and clearest feedback for the agent.
- The architectural cost is low: thanks to the single control plane and the schema, the manual tools are another face of the same operations; the inspector is generated from the schema.

**Rejected alternative — a fully AI-based, automatic engine.** It means a smaller interface, but for the reasons above it is less functional today. As models improve, the weight shifts to AI by itself; the hybrid design does not prevent this.

**Why the core first.** Both the cockpit elements and the manual tools are the visual face of the invariants the core already provides (operation, undo, snapshot, diff, schema, semantic vision) — no extra "magic" is needed.

**Scope effect.** This decision **enlarges the 1.0 scope**: full manual editing roughly doubles the interface work. Mitigation: the interface milestone was split in two (M8a editor foundation, M8b AI cockpit); expert tools were kept outside 1.0; the inspector is not written by hand, it is generated from the schema.

**Interface technology — final (16 November 2026, M8a WP1).** In-process `egui` 0.37 (`egui` + `epaint`, MIT OR Apache-2.0; default fonts off, the engine's Roboto is used), drawn with **our own deterministic CPU rasterizer** and following `wish_window`'s "draw off screen, show the picture" path. `egui-wgpu` was not chosen: it strains invariant 6 (wgpu only in `wish_render`) and makes headless testing harder; `egui_software_backend` 0.0.x is not mature. Evidence: at full HD (6,524 triangles, 60 labels + 8 inspector panels) layout 0.6 ms + drawing 4.3 ms < 6 ms budget; the output is bit-for-bit deterministic and tested with a golden image. The web interface option (HTTP + webview) and M8b's rich text need will be evaluated separately; because the core is independent of the interface, the engine is not affected if the technology changes. Details: [design/M8a-editor-foundation.md](design/M8a-editor-foundation.md) §2 and WP1.

**Risk and mitigation.** The cockpit design is tested with mockups before code is written (T1). Because the core is independent of the interface, the engine is not affected even if the technology changes.

**Amendment (18 November 2026; 1.0 is the short path, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md) §3).** The hybrid decision and its four rules stand. The list "Manual editing tools (1.0)" above is the aim, not what 1.0 ships: at 1.0 manual editing is what is built today (M8a and after), and the rest arrives on the train. The editor's first hour (an orbit camera, a Create menu, duplicate, new scene) is release 1.1; picking, gizmo completion, asset import and management, a console with click-through and a profiler are release 1.4. At 1.0 a material is edited as the `Material` component in the inspector and as a material file (item 7 of the short path); a material editor with a preview is release 1.8. Docking and the specialist editors are after 2.0.

## ADR-019 — Platform

**Decision.** The primary target is Windows desktop. Linux/macOS from the same code (built in CI). Web (WebGPU + Wasm) is a separate milestone, but its constraints are respected **from the start**: the baseline render path is limited to the WebGPU core, thread assumptions are not mandatory, guest modules are core Wasm. *(The first two sentences were replaced on 18 November 2026: see the amendment.)*

**Amendment (M12, 18 November 2026; the user's decision on the 1.0 scope, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §3).** "Windows first, Linux/macOS built in CI" is replaced by:

- **Hosts (where people work): Windows and Linux**, both first-class and both **packaged**. A tag builds the base and the full archive (ADR-023) for each, with checksums and every licence text, and the Linux package is tested end to end in CI like the Windows one. "It builds in CI" no longer makes a platform one we name.
- **Export targets (where games run): Windows, Linux and Steam Deck**, from either host. Game code is Wasm and everything else is data, so exporting for another system copies that system's prebuilt player (a component, ADR-023); nothing is cross-compiled on the user's machine. Steam Deck is checked in the Steam Linux Runtime, and the word "playable" is used only after a report from a real device.
- **macOS (arm64):** an unsigned preview, labelled as one, that never gates a release. A signed, first-class macOS target comes after 1.0.
- **Web (WebGPU + Wasm):** the headline of 1.1, no longer an undated later milestone. Its constraints are still respected from the start, as written above.
- **Naming rule:** a platform is called supported only while the recorded world hashes and the committed example replays pass on it in CI (ADR-027, rule 4). macOS arm64 and wasm32 run as informational jobs until they do. ADR-005 is unchanged: D1 is the guarantee, D2 a goal.

**Why.** Platform support is judged by what a person can download and run, and by where their players are; it was one of the user's two first priorities. `winit`, `wgpu` and Rapier already run on Linux, so what was missing was packaging and testing, not a port.

**Amendment (cut to the short path on 18 November 2026; the user's second decision of that day, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md) §3).** The direction of the amendment above stands; what 1.0 contains of it, and when the rest arrives:

- **In 1.0:** Windows and Linux are both packaged hosts. A tag builds one archive for each (the base and full split is release 1.3: ADR-023), with checksums and every licence text; the Linux package is tested end to end in CI, and audio works on Linux. Each host exports games for itself.
- **Release 1.1:** the probes: committed replays on Windows and Linux in CI, hash jobs for macOS arm64 and for the simulation core on wasm32 as informational jobs, a window smoke test.
- **Release 1.3:** export to the other system through player components, with an export that verifies itself; Steam Deck checked in the Steam Linux Runtime in CI ("playable" still only after a report from a real device); macOS arm64 as an unsigned preview. A signed, notarised macOS build is after 2.0.
- **Web: a preview in release 1.9, not the headline of 1.1.** Before it, in 1.8, a probe and its own ADR decide how game code runs where `wasmtime` cannot. The web leaves preview in 1.10 only if the determinism matrix passes (ADR-028). The constraints respected from the start are unchanged.
- **Naming rule:** restated for platforms and executors, with "preview" as the only other word, in ADR-028.
- Android, iOS and consoles are after 2.0.

## ADR-020 — 3D first

**Decision.** The core is 3D. 2D is added toward 1.0 as a subset, with an orthographic camera + sprite (and 2D physics when needed).

**Amendment (M12, 18 November 2026; the user's decision on the 1.0 scope).** The core stays 3D (one world, one renderer). 2D is no longer a subset but a **first-class kit** built on that core: sprites kept on the GPU, sprite sheets and sprite animation, nine-slice, a 2D physics profile, a fixed viewport with a pixel-perfect camera and the pointer in the world, tilemaps with painting (M12 Phase 3; how the physics profile is built is decided in that package's design note). 2D lights and autotiling come after 1.0.

**Amendment (cut to the short path on 18 November 2026; the user's second decision of that day, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md) §3).** At 1.0, 2D is what the original decision said and what is built: a subset, with an orthographic camera and sprites. The first-class kit of the amendment above is **release 1.7** (sprites kept on the GPU, sheets and sprite animation, nine-slice, a 2D physics profile, tilemaps with painting, a fixed viewport with a pixel-perfect camera and the pointer in the world, a 2D mode in the editor); it comes after the gamepad and the presenter (1.2) and editor picking and asset import (1.4), which it stands on. Autotile rules and Tiled/LDtk import are release 1.10. 2D lights and shadows, 2D skeletal animation and a 2D physics backend are after 2.0. The core stays 3D throughout.

## ADR-021 — Conventions

**Decision.**
- **Coordinate system:** the same as glTF — right-handed, +Y up, the camera looks toward −Z. No conversion is needed on import.
- **Units:** meter, second, kilogram. Angles in the API are radians. Fields that could be ambiguous in files carry the unit in their name or schema (`fov_y_deg`, `unit` in the schema).
- **Color:** linear workflow; color textures are sRGB.
- **Language:** code, identifiers, API names, diagnostic messages, commit messages and every document in the repository (design notes, ROADMAP, DECISIONS, README, `CLAUDE.md`) are in English. *(Changed 17 November 2026, user decision: the repository is public, so everything on GitHub is English. Planning documents were Turkish until then and were translated.)* The Turkish user guide (`docs/user/tr/guide.md`) and the editor's `lang/*.json` are localizations, not exceptions.
- **Agent docs:** `CLAUDE.md` is the main source; for other AIs `AGENTS.md` points to the same content.

## ADR-022 — Security

**Decision.** The MCP server writes only to the project folder. The HTTP transport binds only to the local machine and asks for a session token. *(Implementation, M8a: `wen mcp --http` always asks for a bearer token — `WISH_MCP_TOKEN` or a random one generated and shown at start; a non-loopback address only with `WISH_MCP_TOKEN`; `Origin` and `Host` check; the MCP session id is separate, each client with its own id.)* There is a `--read-only` mode. Wasm guests are limited in fuel, memory and time. Destructive operations (file deletion) run as undoable operations.

## ADR-023 — Product and distribution

**Decision.**
- The engine is **open source**: MIT + Apache-2.0 dual license (the standard of the Rust ecosystem). The repository carries these license files from the first commit. *(User decision, 10 November 2026.)*
- The engine is **free**; no revenue share, license fee or subscription is taken from games made with the engine. The chosen licenses already guarantee this: a game developer can sell their game under any license and at any price.
- Distribution channels: **Steam** and **our own website**. Updates: on Steam, Steam's own mechanism; in the web version, a built-in updater.
- **Zero install:** a single package. The toolchain needed to compile game code (a compiler targeting Wasm) comes with the engine or is silently installed by the engine on first launch. The user is **not asked** to install Rust, Visual Studio or any other tool. *("A single package" was replaced on 18 November 2026: see the amendment below. The promise in the last sentence stands.)*
- **Game export:** `wen export` (and one click in the interface) → a standalone game folder containing the runtime + packaged assets + Wasm modules. The exported game has no editor, MCP server or compiler.
- **Privacy:** by default no usage data is collected. A crash report is sent only if the user explicitly allows it.
- **Third-party licenses:** all chosen libraries (ADR-002) have permissive licenses (MIT/Apache-2.0); there is no obstacle to free distribution and commercial games. The attribution file is added to the package automatically; dependency licenses are checked in CI.

**Why.** A direct requirement of the vision. Publishing a free engine on Steam is possible (there is a one-time Steam Direct fee per application; the exact amount and terms are verified from Steamworks before release).

**Risk and mitigation.**
- *The biggest product risk: the embedded compile toolchain.* Compiling Rust → Wasm needs a compiler on the user's machine; packaging it smoothly (size, antivirus false alarms, first compile time) must be proven. → In M5 it is tried end to end on a **clean Windows install**.
- Unsigned executables get a Windows SmartScreen warning → a code signing certificate (paid) is needed for the web version; the Steam version does not have this problem.

**Trigger.** If the embedded toolchain does not reach the "smooth" bar in the M5 trial: a prebuilt SDK + a lighter compile path, or a second guest language that needs no toolchain, is brought into the 1.0 scope.

**Amendment (M12, 18 November 2026; the user's decision on the 1.0 scope: a small base engine, everything else as extras).** "Zero install: a single package" becomes three things:

- **A small base.** The editor, the tools, the host's own player and the templates; the plan estimates it at about 70 MiB (an estimate, not a measurement; the packed compiler alone is 417 MiB, M5 WP7). Templates ship with built modules, so a new project plays as downloaded, without the compiler.
- **Components on demand.** The compiler with its SDK pack, and the export players of other systems (ADR-019), become optional components. The engine adds one itself the first time it is needed: one step inside the engine, never an install by hand, and a step that needs a missing component says so with a diagnostic and a hint. Components come from a local archive first; fetching them over the network waits for the user's open decisions on the download address and the HTTP/TLS library (M10b).
- **A full offline bundle.** The base and every component in one archive, for a machine without a network. It is what "a single package" was.

What does not change: the user is **never asked** to install Rust, Visual Studio or any other tool by hand; the engine works with no network once a bundle is on the machine; the compiler and the SDK pack are the same pinned version wherever they come from (`script_toolchain_mismatch`, ADR-025); an exported game carries no editor, MCP server or compiler. Both the base and the full bundle are built for Windows and for Linux (ADR-019): Linux is packaged, not only built.

Still open (the user's decision 8 in the plan's §7): where exactly the line between the base and the extras runs at 1.0. The plan's Phase 1 starts with the compiler, the SDK pack and the export players as components; the proposal adds looks, asset packs, templates and verified runs as packages, keeps physics, audio and the interface compiled in, and leaves code plugins for after 1.0.

**Why.** The user asked for a small base engine with everything else as extras. One archive that carries the compiler spends the download on a part a first session may never use, and repeats it for every engine version kept side by side (M10b). **Risk and mitigation.** A base without a compiler works against "zero install" if adding it is not one step, or if a machine has no network → the full bundle exists for exactly that, and templates play without the compiler.

**Amendment (cut to the short path on 18 November 2026; the user's second decision of that day, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md) §3).** The three-part shape of the amendment above stands as the direction, and arrives after 1.0:

- **In 1.0: one package per system**, for Windows and for Linux, built from a tag with checksums and every licence text. It carries everything, the compiler included: it is what the amendment above calls the full offline bundle, and what "a single package" was. The promise that the user is never asked to install a tool by hand is unchanged.
- **Release 1.1:** templates ship with built modules, so the first Play needs no compile.
- **Release 1.3:** components (the compiler, the SDK and the export players become optional parts), the base of about 70 MiB (still an estimate) and the full bundle. The user's decision on where the line between the base and the extras runs is asked then, not before 1.0.
- **No release number:** the download layer (fetching over HTTPS with checksums, a signed index). It starts when the user decides on it, and no release waits for it; until then components come from a local archive.
- **Later:** packages that carry assets and prefabs, with dependencies and author tooling, in 1.6; game code in packages (kits) as an experiment in 1.10; editor and tool plugins, and Steamworks or a kernel anti-cheat as player variants, after 2.0.

## ADR-024 — AI connection: the user brings their own AI

**Decision.**
- The engine contains no AI model and does not sell an AI service. The user connects their own account/subscription.
- The connection is **optional**: the engine works completely without an AI connected (ADR-018).
- Two connection paths, both first-class:
  1. **External agent:** Claude Code, Claude Desktop or any client that speaks MCP connects to the engine's MCP server. (Ready in M4.)
  2. **In-cockpit agent:** the chat panel in the interface works over a provider-independent **agent adapter** interface. The first adapter is Claude. (M8.)
- Credentials are not stored as plain text in the engine's own files; the operating system's credential store or the provider's own session is used.
- The cockpit shows all agent activity in the same activity feed, whichever path it comes from (the external agent also produces the same operations over MCP).

**Why.** The "no commission" principle covers AI usage too. Provider independence keeps the engine from depending on a single company; the Claude priority is kept by it being the first and best-tested adapter.

**Provisional part.** The exact method of the in-cockpit Claude adapter (driving Claude Code as a subprocess, the Claude Agent SDK or the direct API) is chosen at the start of M8, according to that day's current docs.

## ADR-025 — Compiling and instantiating game code

**Decision.**
- **Build path C:** game code (`scripts/`) is compiled for `wasm32-unknown-unknown` **without cargo, with a single `rustc` call**. `wish_sdk` (wasm32 rlib) and `wish_sdk_macros` (host proc-macro library) are produced in the engine's own build and ship with the engine; the game build uses them with `--extern`. Linking is done with `rust-lld` (comes with rustc). **No host linker (MSVC `link.exe`) is needed at all** on the user's machine.
- **Embedded toolchain** = a minimal copy of the pinned rustc: `rustc` + `rustc_driver`, `rust-lld` (+ `gcc-ld/wasm-ld`), `rust-std-wasm32-unknown-unknown`, the prebuilt SDK. Cargo, rustup and Visual Studio are not needed.
- **Instantiation:** per module, **one fresh instance per tick**; the system calls in that tick share it, and if a call fails (trap, fuel, memory) the instance is thrown away and the next call gets a new one. State is never carried from tick to tick: because snapshots are taken at the tick boundary they are complete, and hot reload at the tick boundary is lossless.
- wasmtime runs with the pooling allocator (64 slots).

**Why (M5 WP1 measurements, 16 November 2026, development machine, Windows 10, release; reproduced with `wish_script/tests/build_paths.rs` and `rust_guest.rs`):**
- *Path A (cargo + proc-macro):* when the linker is broken via an environment variable, the build **fails** — the proc-macro is linked for the host; it does not work on a clean Windows.
- *Path C:* compiled and the module ran in an empty environment with `env_clear()` (only the copied toolchain on `PATH`, no rustup/VS variables); `--print link-args` showed that the linker is `rust-lld`. Time: first build 1.95 s (including the first load of the freshly copied 200 MB `rustc_driver` dll), **0.18 s after a single-line change**, 0.13 s with no change. It stays comfortably within the < 10 s target.
- *Toolchain size:* unpacked 437 MiB (`rustc_driver` 201 MiB, `rust-lld` 117 MiB, wasm32 std 100 MiB), 141 MiB with gzip. Small next to Unity/Unreal installs; the ADR-023 trigger was not triggered. (Compression with `zstd`/`xz` makes it smaller still; the unneeded `gcc-ld` wrappers can be dropped — WP7.)
- *Fresh instance cost:* store 0.4 µs + instantiation 6.4 µs + export/allocation 2.3 µs + drop 7.2 µs ≈ 16–28 µs (varies with the measurement). A fresh instance per call would cost ~0.5 ms per tick in a game with 20 systems; with one instance per tick it is ~20 µs + a few µs per system. The pooling allocator is ~20% faster than on-demand allocation.
- *Boundary cost:* including copying two 24-byte columns (position + velocity) to the guest and back, ~13–15 ns per entity (native loop ~1.5 ns). At 10,000 entities ~135 µs per call: 0.8% of the frame budget. The ADR-011 trigger (20% = 3.3 ms) is far away.

**Alternatives.** Path A (cargo + proc-macro): no linker on a clean machine — eliminated. Path B (without proc-macros, `macro_rules!` declarations): works but loses Rust's real attribute syntax and good compiler errors; C gives the same result without paying the same cost. A fresh instance per call: the strictest statelessness, but ~10 times more expensive than one instance per tick and it gains no guarantee (the snapshot is already at the tick boundary).

**Cost.** In the first version game code cannot use third-party crates (only `wish_sdk`); the prebuilt SDK is tied to the pinned rustc version (regenerated when `rust-toolchain.toml` changes; a mismatch is explained with `script_toolchain_mismatch`). Within one tick two systems can talk through a `static`: it is deterministic but the SDK does not recommend it and does not guarantee it.

**Trigger.** If agents are seen needing third-party crates again and again in evals, a cargo side path similar to path B (with crates that need no linker) is added. If the cost of one instance per tick exceeds 5% of the budget in the benchmark (WP7), the way of reusing the instance between ticks and resetting the memory is tried.

---

## User decisions

| Topic | Decision | Date |
|---|---|---|
| Open source? | **Yes** — MIT + Apache-2.0 (ADR-023) | 10 November 2026 |
| First showcase game (M7) | **A small 3D platform game** — tests physics, camera, input, audio and interface together | 10 November 2026 |
| Name | **Wish Engine** — "dile, olsun" (Turkish, roughly "wish it, and let it be"); English, short, memorable | 10 November 2026 |
| The 1.0 scope (M12) | **Yes** to the scope in [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §3, starting with graphics and platforms. It grows the 1.0 scope to about three times what was built so far (ADR-012, 019, 020, 023 amended; ADR-027 added). The other decisions of its §7 are still open | 18 November 2026 |
| The 1.0 scope, cut | **The short path**, the same day: 1.0 is the about 13 packages of [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0, with graphics and platforms first. Everything else in the M12 note is done after 1.0; nothing is dropped (ADR-012, 019, 020, 023 and 027 amended again; ADR-028 added). The name, downloading and merging to `main` are needed before the release candidates | 18 November 2026 |
| After 1.0 | The train of [design/road-to-2-0.md](design/road-to-2-0.md): releases 1.1 to 1.10, then 2.0, with stopping points after 1.4 and after 1.7 | 18 November 2026 |

**Naming details.**
- Crate prefix `wish_`, doc addresses `wish://…`, project manifest `wish.json`.
- The command-line tool is `wen` (**W**ish **En**gine), **not** `wish`: the name `wish` is Tcl/Tk's own command and comes preinstalled on macOS and in Git for Windows; using the same name would clash.
- A quick web search on 10 November 2026 found no game engine with this name. This is not an exact trademark check; the availability of the trademark and domain name is verified separately before release on Steam (M9).

## ADR-026 — Simulation state outside the ECS: extensions

**Decision.** Working memory that does not fit into columns (Rapier's broad-phase tree, contact cache, warm-start impulses) is kept in the `World` as an **extension** (`insert_extension`, `extension`, `extension_mut`; one `Clone + Send + Sync` value per type). Rules: (1) every value visible to the game (position, rotation, velocity, sleep) is **written back** to the ECS components every tick; the extension carries only derived/cache state. (2) The extension does **not** go into the world hash (the components already reflect every drift). (3) A snapshot copies the extensions, `restore` puts them back, `clone` and fork take their own copy. (4) Restore → continue must be bit-identical to never having gone back; every subsystem that uses an extension proves this with a test.

**Alternatives.** Building the state from scratch from the ECS every tick (expensive, warm start is lost, stacks jitter); keeping the state in the world as a byte array (breaks the fixed-size `Plain` rule, serialization every tick).

**Evidence (M6 WP1).** `wish_physics/tests/proof.rs`: the Rapier state (excluding the pipeline, which is a buffer) is copied; the copy continuing from tick 150 gives the same hash as the straight run at tick 400 and the 200 ticks after it; the recorded pyramid hash is the same on Windows and Linux (CI), the same in debug and release.

## ADR-027 — Four rules before the freeze: defaults, answers, versions, claims

*(M12 Phase 0, 18 November 2026: decided on paper before the code of M12, as the plan asks ([design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §4). What is said below about today's code was read from the code on that day. The changes the rules call for are listed as follow-ups; **none of them is made yet**.)*

**Decision.** Four rules that hold from now on. The stability contract of 1.0 (M12 Phase 4) builds on them.

**Rule 1 — Schema defaults are part of the file format.**

A component in a scene or prefab file is a patch on its type's defaults: only the fields that differ from the default are written ([design/M2-data-layer.md](design/M2-data-layer.md) §2). The default of a field is therefore data that every existing file depends on without containing it. Changing one changes the meaning of every scene that left the field out, and with it golden images, template thumbnails, recorded hashes and replays.

- A default may change only inside a **planned format window**, and only with a **migration** that says, for each changed default, what happens to existing files: either they keep their meaning (the migration writes the old value into every file that left the field out), or they take the new value on purpose (written in the changelog; golden images, thumbnails and recorded values are refreshed in that same change).
- Two windows are planned before 1.0: the format window of M12 Phase 1 (the single hash-and-format break) and the look lock of Phase 2 (the one switch of the default look, signed off by the user by eye). Outside a window a default is as fixed as a field name.
- Adding a field whose default reproduces what the engine did before is not a default change: existing files stay byte-identical and keep their meaning.
- This replaces open question 2 in §11 of the M2 note, which called the silent change "a deliberate choice". (The `wen fmt --explicit` named there was never built.)

*What must change (follow-up; not done).* The guard comes first, in M12 Phase 0: a recorded table of every type's defaults, refreshed on purpose like a golden image, so that a test fails when a default changes outside a window. The migration comes with the format window: `wen migrate` has no migration step yet (it reports which files are current and refuses the rest: `wish_cli/src/project_cmds.rs`), so the first changed default is also the first migration. That window brings the step (in `wish_control` first: invariant 4) and a test that a migrated file keeps its meaning.

**Rule 2 — Tool answers carry no wall-clock numbers.**

The answer of a control-plane tool depends on the project, the call and the session, and on nothing else: the same call gives the same bytes in another process (the parity test compares in-process, `wen do` and MCP byte for byte). A duration, a timestamp or a date breaks that, puts the machine's speed into an agent's context, and invites tests that compare times.

- No tool answer, resource or diagnostic carries a duration, a timestamp or any other value read from the wall clock.
- Durations belong to the two faces a person looks at: the command line's own output (`wen build` says how long the build took; `wen bench` and `wen run` measure) and the editor (status line, profiler). They measure around the call themselves.
- What an agent needs to judge cost comes in deterministic units: ticks, counts (entities, draw calls, triangles), bytes, fuel per system.
- Time inside the simulation is not the wall clock and is not affected: ticks, `Time` in the world, clip lengths, a sound's `seconds`.

*What must change (follow-up, M12 Phase 0; not done).* Two answers break the rule today. `script_build` answers `seconds` (`wish_script::build::BuildReport::to_json`), and `project_info` repeats it as `scripts.last_build.seconds` (`wish_control/src/script.rs`). Both keys go; `wen build`, which prints the time from the answer today (`wish_cli/src/tool_cmds.rs`, `build_text`), measures it itself, and so does the editor. The parity test shows `script_build` only on a mistake today; with the key gone, a test builds the same sources in two processes and compares the answers byte for byte. Removing a key is a change of the tool surface, so it is made now, before the freeze. These two were found by reading the code, not by a tool that proves there is no third: the two-process comparison is extended to every tool as the guard.

**Rule 3 — Versions are three numbers.**

- A version is `major.minor.patch`: three numbers and nothing else. No pre-release suffix (`-rc.1`, `-beta`), no build metadata. The code already reads it this way: `wish_control::packages::Version::parse` refuses anything else (`invalid_version`), and the engine's own version (`Cargo.toml`, `0.1.0` today), `wish-engine.json`, package manifests, the package index and the `engine` key of `wish.json` all go through it. It stays that way: a suffix would need an ordering rule in every one of those places.
- **Pre-releases are `0.x.0` tags.** Each phase of M12 ends with one (0.2.0, 0.3.0, ...); beta builds and release candidates are numbered the same way. A fix to a pre-release is `0.x.y`. A tag is never moved or reused. `1.0.0` is the first version under the stability contract.
- **What a 0.x number means.** Before 1.0.0 nothing is frozen, so a 0.x number says which build it is, not what it is compatible with. File formats change only in a format window (rule 1); a patch version carries fixes only.
- **The `engine` key of `wish.json` across 0.x.** It names the engine version that made the project. It helps choose among the engines on the computer; by itself it is never a gate. The order:
  1. the same version, if it is installed, as today (a game plays bit for bit the same only on the engine it was made with: ADR-005);
  2. else the newest installed engine that is not older than the project **and opens it without changing a file**. Before 1.0.0 that is decided by file formats, not by arithmetic on the number: every engine states the oldest version whose projects it opens unchanged, and that statement moves only in a format window. Such an engine is compatible, and it opens the project without a note;
  3. else the running engine opens the project and says why, as today: a newer project → install that version; an older project with a format window in between → its files will change. The note is then true every time it is shown.

  From 1.0.0 on the same rule gives what the caret rule gives today: every 1.x opens the project of an older 1.y.
- The key is rewritten in one case only: a migration sets it to the migrating engine's version, because older engines can no longer read the files. Opening or editing a project without a migration never touches it (invariant 3: files stay byte-identical). Known limit: a project that starts using a type added by a newer engine still names the older one, and the older engine answers with its ordinary unknown-type diagnostic; whether the key should move on such a write is left to the stability contract.

*What must change (follow-up, before the 0.2.0 tag; not done).*

- `wish_control::engines::pick` takes the exact version, else `^wanted`, and below 1.0 the caret means the same minor only (`VersionReq::matches`, `Op::Caret`). So a project made with 0.2.0 finds no compatible engine in 0.3.0: `Choice::note` says "no engine compatible with it is installed, so 0.3.0 opens it. Its files may change", and the `engines` tool answers `"fit": "none"`, even when no format changed between the two. `wish_control/tests/engines.rs` pins this (`pick("0.2.0")` is `None`: "0.x minors are not compatible"). The five examples (`"engine": "0.1.0"`) would show the note in the editor on the day the workspace version becomes 0.2.0, on any computer without a 0.1.x engine beside it.
- Change `pick`, not `VersionReq`: for a package requirement the caret keeps the meaning it has in Cargo. In `pick`, the compatible engines become those with `opens_from ≤ wanted ≤ version`; the test above changes with it, and a new one opens a 0.2.0 project in 0.3.0 with `"fit": "compatible"` and no note.
- `opens_from` (the name is a proposal) is a constant beside the format constants, `0.1.0` until the first format window. `wen package` writes it into `wish-engine.json` as an optional key: the file stays format 1, and engines packaged before the change ignore it (`read_engine` does not reject unknown keys). An engine whose file has no such key is judged by today's caret rule. A test fails when a format constant changes and `opens_from` does not.
- The note has a second copy in the editor (`wish_editor::welcome::engine_note`, with its translations); it must follow the same choice.
- `wen migrate` must set `engine` when it migrates (rule 1's follow-up brings the first migration step).
- `wish.json` accepts any text as `engine` today (`wish_control/src/project.rs`), and one that is not a version is silently read as naming none (`engines::project_version`): `check` should report it (the `invalid_version` code exists).
- Package manifests: a bare `"engine": "0.2"` means `^0.2`, which is 0.2.x only, and raises `package_engine_mismatch` in 0.3.0. During 0.x a package writes a lower bound (`">=0.2"`), as the engine's own four packages do (`">=0.1"`); the user guides should say so, and the package author tooling of Phase 4 should write it that way.
- §15 of [design/M10-hub-and-packages.md](design/M10-hub-and-packages.md) ("never another major, or another minor below 1.0") describes the code as it is; it is corrected in the same change as the code.

**Rule 4 — Every public claim has a command that reproduces it.**

A number, a comparison or a "works on" in anything the public reads (README, site, store page, release notes, user docs, the feature gallery) is published together with the command that reproduces it from a checkout or from a released package, and with where its recorded result lives in the repository.

- A measured claim names its inputs (engine version, model and effort, number of tries, date, machine or adapter), because a rerun has to be able to match it.
- A claim only a person can check (a look signed off by eye, "playable on Steam Deck" from a real device, a user test) names who checked it, on what and when, instead of a command.
- A claim with neither is not published. A claim whose command stops reproducing it is taken down or corrected in the same change.
- CI runs the commands that cost nothing (hashes, replays, package tests, link checks). Commands that spend model quota or need hardware are rerun at a pre-release at the most, with the user's approval as always.
- What exists already in this form: the evaluation runs (`wish-evals run`, results in `evals/results/`), the comparison with Unity (`tools/compare/run.py`), the MCP Inspector check (`tools/mcp_inspector_check.py`), `wen bench`, `wen replay`.

**Why.** All four are cheap to decide now and expensive to discover later. A default changed by accident re-lights every scene anyone has made; a clock in an answer makes byte-for-byte parity a matter of luck; a version scheme chosen at the first tag is the one every project file carries; and the engine's whole argument is that it proves what it says (VISION, "Finish with proof"), which has to hold for its own first page too.

**Alternatives.** Writing every field into every file, so that defaults stop mattering (much longer files, prefab overrides lose their meaning, every existing file changes). Keeping durations in answers and masking them in tests (the mask becomes the thing that is tested). Semantic-version suffixes for pre-releases (an ordering rule in five readers, for something a plain `0.x.0` already says). Trusting the caret across 0.x (a false warning on every project at every pre-release, which teaches people to ignore the true one).

**Risk and mitigation.** The follow-ups are code and are not done: until they are, the rules are broken in the places listed above → the guard of rule 1 and the changes of rules 2 and 3 are Phase 0 work, before the 0.2.0 tag, each with the test named beside it; the first migration step comes with the format window of Phase 1. A format floor that nobody remembers to move → the test that ties it to the format constants.

**Trigger.** The stability contract (M12 Phase 4) restates rules 1 and 3 for the time after 1.0; if it needs to change one of them, it changes this record first.

**Amendment (cut to the short path on 18 November 2026; the user's second decision of that day, [design/M12-road-to-1-0.md](design/M12-road-to-1-0.md) §0; the order after 1.0: [design/road-to-2-0.md](design/road-to-2-0.md)).** The four rules stand as written. What the text above ties to the phases of M12 moves, because 1.0 is now the short path and the phases describe the work after it:

- **Rule 1, the two windows.** "Two windows are planned before 1.0" no longer holds. **The look lock is in 1.0**: the one switch of the default look before the first release, signed off by the user by eye; a **second look lock** is planned at 2.0. **The format window is release 1.2**, after 1.0. Projects made with 1.0 and 1.1 will exist by then, so it opens with the migration step and a compatibility corpus (projects written by 1.0 and 1.1 open, migrate and keep their meaning in CI), as the rule already asks of any changed default. After 1.0 a default changes only in one of those windows; ADR-028 carries the rule for formats in general.
- **Where the follow-ups go.** Everything called "M12 Phase 0" or "before the 0.2.0 tag" above is in 1.0, as part of its package for the public face (the guard table of defaults; the two keys of rule 2 and the two-process comparison; `engines::pick` and `opens_from`), before the first pre-release tag. The migration step of `wen migrate` is the first package of release 1.2, and `opens_from` first moves there. The package author tooling that writes a lower bound for `engine` is release 1.6.
- **Rule 3, pre-releases.** The phases of M12 no longer end in pre-releases before 1.0. The `0.x.0` tags are the beta builds and release candidates of 1.0. After 1.0.0 the releases of the train are `1.1.0` to `1.10.0` and then `2.0.0`; a patch version carries fixes only and never changes what a simulation computes (ADR-028). How a release candidate of a later release is numbered without a suffix is not decided here; it is decided before the first one is tagged.
- **Rule 3, "the first version under the stability contract".** `1.0.0` ships with a short stability note (after 1.0 a format changes only by addition or with a migration). The stability contract in full (`docs/STABILITY.md`, stable and experimental labels, snapshot tests of the tool catalogue and the SDK) is release 1.3, and the trigger above applies to it there.
- **Rule 4** is unchanged; the scorecard published with every release and the determinism matrix of 2.0 are claims in its sense.

**Amendment (19 November 2026; the look lock of 1.0 happened, M12 package 9, [design/M12-look-lock.md](design/M12-look-lock.md) "Switched").** The first of rule 1's windows was used, as the rule asks: a planned window, a new format number, a migration.

- **What changed.** The user chose the look "Natural" as the default look of 1.0, by eye, from comparison pictures of nine scenes (19 November 2026). Fourteen defaults of six resources changed (`Ambient.source` and `sky_strength`, `Sky.sun_size_deg`, three of `Fog`, four of `ColorGrading`, two of `Bloom`, `RenderSettings.antialiasing` and `shadow_cascades`); no component default and no other resource default did. The scene and prefab **file format is 2**. The defaults of both formats are recorded (`crates/wish_scene/src/defaults/format-1.json`, never edited again, and `format-2.json`), and the guard test of rule 1 now holds format 2's.
- **How old files keep their look.** A file of format 1 means what it meant: the format number says which defaults a file was written against. The engine reads a format-1 file through the migration **in memory** (every value the file relied on whose default changed is filled in, a resource the scene did not set included), so it draws to the byte what it drew before, with nothing to run first; `check` names such files with the warning `old_format`; the file itself is rewritten as format 2 only by `wen migrate` or by the next change saved through the engine, and both write the same bytes. A file newer than the engine is refused as before. So the first migration step exists from 1.0 on (the amendment above placed it in release 1.2); the format window of 1.2 adds its step after this one and brings the compatibility corpus, of which the format-1 fixtures of the test suite are now the first part.
- **What takes the new value on purpose** (rule 1's second branch, "written in the changelog; golden images, thumbnails and recorded values are refreshed in that same change"): the repository's own examples and templates, the scene of `wen new`, and every scene made from then on. Their golden images and template thumbnails were refreshed in that change and are listed in the note.
- **What a migrated scene costs.** A resource that is written into a scene is a resource in its world, so the world hash of a format-1 scene differs from the hash the old engine gave it, and a replay recorded on it before the switch diverges at tick 0. No replay is committed, and 0.x promises no replay compatibility; the release note says "record replays again". Scenes of format 2 that set no look resource hash as before.
- **Two things the switch made rules of the renderer, not defaults:** an orthographic camera never shows the sun's disc, and a scene without a directional light has no disc and gets no warning unless `Sky.sun_size_deg` was changed from its default.
- **Still open from the follow-ups above:** `wen migrate` does not set the `engine` key of `wish.json` (rule 3), and `opens_from` does not exist; an engine of 1.0 reads formats 1 and 2, an older engine refuses a format-2 file (`unsupported_format`, "update the engine"). Both stay with the package for the public face.

## ADR-028 — Rules of the release train: supported, replays, results, dependencies, formats

*(18 November 2026. The train from 1.1 to 2.0 is a schedule and lives in [design/road-to-2-0.md](design/road-to-2-0.md); its §2 has rules of both kinds. The ones recorded here are architecture: they bind the code, CI and every release whatever the schedule becomes. They apply to every release of the train, from 1.1 on (its §2). At 1.0.0 Windows and Linux are named as packaged and tested hosts (ADR-019); the word "supported" is first used when the committed replays of release 1.1 run on both in CI. Rule 3 already holds for the patches of 1.0 (ADR-027, rule 3, as amended), and rule 5 is what the short stability note of 1.0 says.)*

**Decision.**

**Rule 1 — "Supported" has one meaning.** A platform or an executor (what runs game code: `wasmtime` today) is called **supported** only while the committed replays give the same hashes on it in CI as on every other supported one. Anything else that is shipped is called a **preview**, in every text the public reads, and a preview never gates a release. There is no third word. 2.0 publishes the result as a table of platforms and executors (the determinism matrix). This is the naming rule of ADR-019, widened from platforms to executors; ADR-005 is unchanged (D1 is the guarantee, D2 a goal).

**Rule 2 — A replay says what made it.** A replay carries the engine version and the id of the executor it was recorded with, beside what its header has today. Playback on another version or executor is allowed and says so, as a code or scene change does today (M5 WP5); a divergence is then reported against the right cause.

**Rule 3 — Simulation results change only in a minor version.** A release may change what a simulation computes (a world hash for the same build inputs, a fuel count, a physics result) only in a minor version, and the change is listed in the changelog. A patch version never does: every committed replay holds across `x.y.*`.

**Rule 4 — Pinned dependencies are upgraded as their own package.** `wasmtime`, Rapier, `wgpu`, `winit`, `egui` and the packed compiler stay pinned. An upgrade is a work package of its own in a minor version, never a side effect of other work and never in a patch, because it can change fuel counts, physics hashes or the bytes of a compiled game. It exits through the committed replays and the golden images like any other package.

**Rule 5 — Formats after 1.0.** A format changes by addition (existing files stay byte-identical and keep their meaning), or with a migration proven on a corpus of projects written by older versions: they open, migrate and keep their meaning in CI. Schema defaults are part of the format (ADR-027, rule 1). The train plans one format window (release 1.2) and one change of the game-code ABI (release 1.8); at 2.0 it consolidates the formats and switches the default look a second time (ADR-027, rule 1, as amended), both carried by migrations proven on the corpus. 2.0 breaks nothing that a migration cannot carry and removes no tool and no SDK item.

**Why.** The engine's argument is that it proves what it says. "Works on Linux" or "runs in the browser" is then a claim like any other (ADR-027, rule 4), and for a deterministic engine the claim has an exact test: the same replays, the same hashes. A replay that does not say which engine and executor made it cannot tell a bug in the game from a change in the engine. And a person who made a game with 1.3.0 has to be able to take 1.3.1 without their replays, their recorded tests or their players' verified runs changing under them.

**Alternatives.** "Supported" as "it builds in CI" (what "Linux/macOS from the same code" meant until M12). Tiers with more names (each new word needs its own test, and people read them all as "works"). Letting patch versions fix simulation bugs (the fix is a behaviour change to every recorded replay; it waits for the next minor). Upgrading dependencies as they come out (a hash that moves with no line in the changelog that explains it). Formats that change freely with a version number that old engines refuse (rule 5 keeps old projects opening).

**Risk and mitigation.** The corpus does not exist yet → the first package of release 1.2 builds it from projects written by 1.0 and 1.1, before the format window opens. A simulation bug or a security fix in a pinned dependency that cannot wait → the next minor is released early; the rule is not bent. Rule 1 makes a platform lose its word when its CI job breaks → that is the intent: the text is corrected in the same change (ADR-027, rule 4).

*What must change (follow-up; not done).* The train puts the engine version into the replay header in release 1.1 and makes replay format 2 part of the format window of 1.2. The executor id is not scheduled by name; it must be in the header before a second executor exists (the probe of 1.8, the web executor of 1.9). The hash jobs for macOS arm64 and wasm32 are release 1.1, as informational jobs. A check that a patch release changed no recorded hash is not written.

**Trigger.** The executor probe of release 1.8: if a second executor cannot give the same results, rule 1 decides what it may be called, and this record is amended before any web code is written. The stability contract of release 1.3 restates rule 5; if it needs to change it, it changes this record first.
