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
| 012 | Render | wgpu, render graph, baseline first, then layered advanced features | Accepted |
| 013 | Shaders | WGSL + WESL module system | Provisional |
| 014 | Physics | Rapier 3D, behind our own component API | Accepted |
| 015 | Audio | cpal + our own mixer (Kira dropped, M6) + observable audio event log | Accepted (updated) |
| 016 | Input | Text-defined action maps, injection and recording | Accepted |
| 017 | Testing | Scenario tests, golden images, hash verification, agent evals | Accepted |
| 018 | Interface | **Hybrid:** AI cockpit + full manual editing, both first-class; the engine works completely without AI too | Accepted (interface technology Provisional) |
| 019 | Platform | Windows desktop first; web constraints are respected from the start, web target later | Accepted |
| 020 | Scope direction | 3D first; 2D subset | Accepted |
| 021 | Conventions | glTF coordinate system, SI units, everything in the repository in English | Accepted |
| 022 | Security | Writes limited to the project folder, local network only, Wasm resource limits | Accepted |
| 023 | Product and distribution | Open source (MIT + Apache-2.0), free, no commission; Steam + web; zero install; game export | Accepted |
| 024 | AI connection | The user brings their own AI; provider-independent adapter, first provider Claude | Accepted (method Provisional) |
| 025 | Compiling and instantiating game code | A single `rustc` call without cargo + the prebuilt SDK shipped with the engine; no host linker; a fresh instance per module per tick | Accepted (with the M5 WP1 measurement) |

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
- **Baseline first (1.0):** clustered forward+, glTF-compatible PBR, cascaded shadow maps, HDR + tone mapping, ambient lighting (IBL), MSAA, basic post-processing.
- **Advanced layer (after 1.0, native platforms):** GPU-driven drawing (indirect draw + culling with a compute shader), bindless resources, meshlet/virtual geometry, ray-traced lighting. The target hardware (RTX class) supports this.
- **Headless render from day one:** no window, draw to a texture; software adapter in CI.
- **Debug outputs are first-class for the agent:** entity id buffer, depth, normals, wireframe, colliders. The id buffer is the basis of "semantic vision" (which entity is where, what percentage is visible).

**Why.** WebGPU is the portable graphics standard of the future. The render graph makes it possible to add "forward-looking" choices later; the baseline keeps the project finishable.

**Risk and mitigation.** Advanced features are not on the web → capability layers; the baseline runs everywhere. Scope → the advanced layer is not tied to 1.0.

## ADR-013 — Shader language *(Provisional)*

**Decision.** WGSL; WESL for modules/imports. Materials are defined with data (`*.mat.json`: shader + parameters); custom shaders are text files and are hot loaded. Compile errors are converted to the shared diagnostic format.

**Trigger.** At the start of M3 the maturity of the WESL tools is verified; if insufficient, `naga_oil` or our own simple import preprocessor.

**M3 verification (15 November 2026, M3 WP1).** Trial with `wesl` 0.6: imports work and the output is valid WGSL for `naga`; a syntax error in an imported module comes with module path + line/column. But (1) the error of importing a missing item is **without a location** ("in unknown module"), (2) `naga` validation errors point to the **converted names** in the merged output (`package_util_double`), (3) the version is pre-1.0 (the API is changing), (4) the tree is 83 packages. Invariant 5 (every error has a location and a hint) cannot be met with WESL today.
**Decision (for M3):** WGSL, parsing + validation with `naga` (`shader_syntax`/`shader_validation` diagnostics with a location, character column). For the engine's own shaders, module merging with a small, source-mapped import preprocessor behind a `ShaderLibrary` interface (WP3); a `naga` error is translated back to the original file/line with the source map. WESL is kept so that it can be put behind this interface later. **Re-evaluation:** when WESL reaches 1.0, or at the latest when user shaders (`*.mat.json`, M6) arrive. `naga_oil` was not tried: the version cycle tied to Bevy and its own syntax would make a later move to WESL harder.

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

## ADR-019 — Platform

**Decision.** The primary target is Windows desktop. Linux/macOS from the same code (built in CI). Web (WebGPU + Wasm) is a separate milestone, but its constraints are respected **from the start**: the baseline render path is limited to the WebGPU core, thread assumptions are not mandatory, guest modules are core Wasm.

## ADR-020 — 3D first

**Decision.** The core is 3D. 2D is added toward 1.0 as a subset, with an orthographic camera + sprite (and 2D physics when needed).

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
- **Zero install:** a single package. The toolchain needed to compile game code (a compiler targeting Wasm) comes with the engine or is silently installed by the engine on first launch. The user is **not asked** to install Rust, Visual Studio or any other tool.
- **Game export:** `wen export` (and one click in the interface) → a standalone game folder containing the runtime + packaged assets + Wasm modules. The exported game has no editor, MCP server or compiler.
- **Privacy:** by default no usage data is collected. A crash report is sent only if the user explicitly allows it.
- **Third-party licenses:** all chosen libraries (ADR-002) have permissive licenses (MIT/Apache-2.0); there is no obstacle to free distribution and commercial games. The attribution file is added to the package automatically; dependency licenses are checked in CI.

**Why.** A direct requirement of the vision. Publishing a free engine on Steam is possible (there is a one-time Steam Direct fee per application; the exact amount and terms are verified from Steamworks before release).

**Risk and mitigation.**
- *The biggest product risk: the embedded compile toolchain.* Compiling Rust → Wasm needs a compiler on the user's machine; packaging it smoothly (size, antivirus false alarms, first compile time) must be proven. → In M5 it is tried end to end on a **clean Windows install**.
- Unsigned executables get a Windows SmartScreen warning → a code signing certificate (paid) is needed for the web version; the Steam version does not have this problem.

**Trigger.** If the embedded toolchain does not reach the "smooth" bar in the M5 trial: a prebuilt SDK + a lighter compile path, or a second guest language that needs no toolchain, is brought into the 1.0 scope.

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

**Naming details.**
- Crate prefix `wish_`, doc addresses `wish://…`, project manifest `wish.json`.
- The command-line tool is `wen` (**W**ish **En**gine), **not** `wish`: the name `wish` is Tcl/Tk's own command and comes preinstalled on macOS and in Git for Windows; using the same name would clash.
- A quick web search on 10 November 2026 found no game engine with this name. This is not an exact trademark check; the availability of the trademark and domain name is verified separately before release on Steam (M9).

## ADR-026 — Simulation state outside the ECS: extensions

**Decision.** Working memory that does not fit into columns (Rapier's broad-phase tree, contact cache, warm-start impulses) is kept in the `World` as an **extension** (`insert_extension`, `extension`, `extension_mut`; one `Clone + Send + Sync` value per type). Rules: (1) every value visible to the game (position, rotation, velocity, sleep) is **written back** to the ECS components every tick; the extension carries only derived/cache state. (2) The extension does **not** go into the world hash (the components already reflect every drift). (3) A snapshot copies the extensions, `restore` puts them back, `clone` and fork take their own copy. (4) Restore → continue must be bit-identical to never having gone back; every subsystem that uses an extension proves this with a test.

**Alternatives.** Building the state from scratch from the ECS every tick (expensive, warm start is lost, stacks jitter); keeping the state in the world as a byte array (breaks the fixed-size `Plain` rule, serialization every tick).

**Evidence (M6 WP1).** `wish_physics/tests/proof.rs`: the Rapier state (excluding the pipeline, which is a buffer) is copied; the copy continuing from tick 150 gives the same hash as the straight run at tick 400 and the 200 ticks after it; the recorded pyramid hash is the same on Windows and Linux (CI), the same in debug and release.
