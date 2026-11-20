# Wish Engine — Architecture

The reasons for the decisions are in [DECISIONS.md](DECISIONS.md); this document explains how the parts fit together.
The code examples show the **intended way of use**; exact signatures become final in the relevant milestone.

## Layers

```
┌──────────────────────────────────────────────────────────────────┐
│  CLIENTS                                                         │
│  Claude / other agents       Human (terminal)    Human (editor)  │
│        │ MCP                      │ CLI                 │ egui   │
├────────┼──────────────────────────┼─────────────────────┼────────┤
│  wish_mcp                      wish_cli               wish_editor   │
│        └──────────────┬───────────┴─────────────────────┘        │
│  CONTROL PLANE        ▼                                          │
│  wish_control — commands · queries · sessions · diagnostics      │
├──────────────────────────────────────────────────────────────────┤
│  DOCUMENT LAYER                   │  RUNTIME                     │
│  wish_scene — scene/prefab document│  wish_app — loop, sessions   │
│  transaction · undo · canonical write│ wish_script — Wasm host     │
│  wish_asset — import, watching     │  wish_test — scenario, golden │
├──────────────────────────────────────────────────────────────────┤
│  SUBSYSTEMS                                                      │
│  wish_render   wish_physics   wish_audio   wish_input   wish_ui       │
├──────────────────────────────────────────────────────────────────┤
│  CORE                                                            │
│  wish_ecs — world, archetypes, queries, scheduling, snapshot     │
│  wish_schema — type schema, JSON ⇄ binary, JSON Schema           │
│  wish_core — identifiers, hash, diagnostic types, math           │
└──────────────────────────────────────────────────────────────────┘
        Guest side (compiled to Wasm):  wish_sdk + the game's own code
```

**Dependency rule:** arrows only go down. Subsystems do not know each other; they talk through the ECS (with components and events). No subsystem knows the control plane; the control plane sees all of them through the schema.

## Crates

| Crate | Responsibility | Milestone |
|---|---|---|
| `wish_core` | Identifiers, stable hash, `Diagnostic`, math re-export | M1 |
| `wish_schema` | `TypeSchema`, the `#[component]` derive macro, JSON ⇄ binary, JSON Schema generation | M1 |
| `wish_ecs` | World, archetype storage, queries, commands, resources, events, scheduler, snapshot, world hash | M1 |
| `wish_app` | Main loop, fixed step, plugin registration, session management | M1 |
| `wish_cli` | The `wen` binary | M1 → ongoing |
| `wish_scene` | Positioned strict JSON + canonical writer, scene/prefab document and its validation, transactions + undo/redo + `actor` log, prefab expansion, document → world (`SceneId`) | M2 |
| `wish_asset` | Asset index, `*.meta.json`, material files (`*.mat.json`: one look, a `MaterialFile`, that entities name in `Material.file`; both a checked canonical document and an asset; M12), finding/checking `AssetRef` by schema, `wen mv`, whole-project loading (`wen check/fmt`), polling watcher + hot reload. Importers and cache in M3 | M2 (+M3, M12) |
| `wish_render` | wgpu, render graph, materials, headless capture, debug views | M3 |
| `wish_window`, `wish_input` | Window, devices, action maps, injection/recording | M3 / M5 |
| `wish_control` | Control plane API | M4 |
| `wish_mcp` | MCP server (stdio + HTTP) | M4 |
| `wish_script`, `wish_sdk` | Wasm host, ABI, hot reload / guest SDK | M5 |
| `wish_test` | Scenario test runner, golden image, replay | M5 |
| `wish_physics` | Rapier integration (bodies, colliders, events, queries, character controller); its state is an ADR-026 extension | M6 |
| `wish_audio` | Audio components, deterministic audio event log; output is cpal + our own mixer (ADR-015) | M6 |
| `wish_anim` | Time of the skeletal animation player (the pose is computed in render) | M6 |
| `wish_text` | Fonts (Roboto embedded), deterministic text layout, glyph rasterization | M6 |
| `wish_ui` | In-game UI: flexbox layout (taffy), buttons, pointer | M6 |
| `wish_particles` | Stateless particle formula, alive count | M6 |
| `wish_editor` | AI cockpit (interface) + agent adapters | M8 |
| `wish_export` | Game packaging and export | M9 |

## Schema: the source of everything

```rust
/// Moves an entity at a constant speed.
#[component]
pub struct Mover {
    /// Movement speed.
    #[schema(unit = "m/s", range = 0.0..=50.0, default = 5.0)]
    pub speed: f32,
}
```

What is derived from this single definition:

```
                        ┌─► ECS column layout (size, alignment, offsets)
                        ├─► JSON read/write (scene files, MCP payloads)
  #[component] ─► TypeSchema ─► JSON Schema (schema_get, editor validation)
                        ├─► Wasm ABI layout (the guest side sees the same bytes)
                        ├─► Editor inspector fields (range → slider)
                        └─► Docs (wish://docs/components/Mover)
```

What the agent sees in a `schema_get("Mover")` call: the field name, its type, unit, range, default and doc comment.

## World and session model

```
  DISK (source of truth)             MEMORY
  scenes/main.scene.json  ◄────►  Edit document (revision N, undo stack)
                                        │ sim_start (copy + seed)
                                        ▼
                                  Play session s1 ── snapshot ──► fork s2
                                  (world + physics + RNG + tick)
```

- **Edit document**: the in-memory counterpart of the scene file. Changes are made with transactions and written to disk in canonical form.
- **Play session**: a live world derived from the document. It advances deterministically. It does not write to disk. Several sessions can live at the same time (headless or windowed).
- Making a value in a session permanent is an explicit step (a patch from the session to the document).

## Frame loop

```
every frame:
  collect input (device → actions)  +  injected actions
  while accumulated time ≥ fixed step:
      ┌ FixedUpdate ─────────────────────────────────────────────┐
      │ 1. write input actions into the world                    │
      │ 2. game systems (Wasm + native), in explicit order       │
      │ 3. apply deferred commands (in fixed order)              │
      │ 4. physics step                                          │
      │ 5. hierarchy/transform propagation                       │
      │ 6. write events to the log, run the world inspector      │
      │ 7. tick++  (world hash if requested)                     │
      └──────────────────────────────────────────────────────────┘
  render (outside the determinism boundary): interpolation → render graph → present/capture
```

In headless mode there is no "accumulated time": `sim_step(n)` advances exactly n ticks, independent of real time and as fast as possible.

## Game logic (Wasm guest)

```rust
use wish_sdk::prelude::*;

#[system(stage = FixedUpdate)]
fn move_player(actions: Res<Actions>, time: Res<Time>, mut q: Query<(&Mover, &mut Transform)>) {
    let dir = actions.axis2("move");
    for (mover, tf) in q.iter_mut() {
        tf.translation += vec3(dir.x, 0.0, -dir.y) * mover.speed * time.delta;
    }
}
```

Hot reload flow:

```
scripts/src/*.rs changed
   → compile (wasm32)  ── error? → diagnostics (file, line, hint) → the old module keeps running
   → validate the new module (ABI version, schema compatibility)
   → at a tick boundary, drop the old module, instantiate the new one
   → if the schema changed: migrate component data by field name (new field = default)
   → the game continues from the tick where it was
```

Because state does not live in the guest, there is nothing to lose. If a system throws a trap or exceeds the fuel limit: that system is disabled, a diagnostic is produced, and the world keeps advancing.

## The journey of an edit

```
edit_apply({ base_revision: 41, ops: [ spawn …, set … ] })
  1. schema validation (field name, type, range, existence of references)
  2. if there is an error: nothing is applied → structured error + hint
  3. apply to the document → revision 42 → add to the undo stack
  4. write the canonical JSON to disk
  5. mirror live into open play sessions (if requested)
  6. reply: diff + world inspector warnings + revision
```

When a file is edited directly, steps 1–2 run in the file watcher; if there is an error, the last valid in-memory document is kept and the diagnostic is visible through `check`.

## Diagnostics: one format

A compiler error, a scene validation error, a run-time warning, an MCP error — all have the same structure:

```json
{
  "severity": "error",
  "code": "unknown_field",
  "message": "Component 'Transform' has no field 'position'.",
  "location": { "file": "scenes/main.scene.json", "path": "/entities/player/components/Transform/position" },
  "hint": "Did you mean 'translation'?",
  "candidates": ["translation", "rotation", "scale"],
  "docs": "wish://docs/components/Transform"
}
```

Diagnostic codes are stable and documented. The hint field is required: whoever writes a diagnostic must answer the question "how do I fix it?".

## World inspector

Cheap heuristic rules that run after every play step and every edit. They say in words what the agent would otherwise try to work out by looking at a screenshot:

- An entity fell out of the world (far outside the bounds)
- NaN/infinity in a transform, zero scale
- No light in the scene / no active camera / the camera sees nothing
- Colliders overlapping at spawn
- Unusual model scale (there may be a unit mismatch)
- A reference to a missing asset file

## Render pipeline

```
Scene extraction (ECS → render world)
   → culling → light clustering → shadow passes → opaque (PBR) → sky → transparent
   → post-processing (tone mapping …) → present | capture (PNG)
   side outputs (on request): entity ID · depth · normals · wireframe · colliders · lights · shadow cascades
```

`describe_view` reads the ID buffer: for every visible entity, its screen box, visibility percentage and distance to the camera.

## Game project layout

`wen new my_game` produces:

```
my_game/
  wish.json            project manifest (name, engine version, entry scene, tick rate)
  scenes/main.scene.json
  prefabs/
  assets/                 glb, png, ogg … + *.meta.json
  scripts/                game logic (Rust → Wasm)
    Cargo.toml
    src/lib.rs
  input.json              action maps
  tests/*.test.json       scenario tests
  CLAUDE.md, AGENTS.md    project-specific agent guide (generated for the engine version)
  .mcp.json               so that Claude Code connects to the engine MCP server automatically
  .wish/                   cache, snapshots, replays (not in git)
```

Example scene (format 1, finalized in M2; in the canonical file the keys are written in byte order, and `wen fmt` does this. Exact rules: [design/M2-data-layer.md](design/M2-data-layer.md) §2–§3 and §12):

```json
{
  "format": 1,
  "entities": {
    "sun": {
      "components": {
        "Transform": { "rotation": { "euler_deg": [-45, 30, 0] } },
        "DirectionalLight": { "illuminance_lux": 100000, "shadows": true }
      }
    },
    "player": {
      "prefab": "prefabs/player.prefab.json",
      "components": { "Transform": { "translation": [0, 1, 0] } }
    },
    "player_camera": {
      "parent": "player",
      "components": {
        "Transform": { "translation": [0, 2, 6] },
        "Camera": { "fov_y_deg": 60 }
      }
    }
  }
}
```

Example scenario test:

```json
{
  "format": 1,
  "scene": "scenes/main.scene.json",
  "seed": 7,
  "steps": [
    { "input": { "move": [0, 1] }, "frames": 120 },
    { "expect": "entity('player').Transform.translation.z < -5" },
    { "capture": "after_move", "compare": "golden", "tolerance": 0.02 }
  ]
}
```

## Hybrid interface

The interface is hybrid (ADR-018): manual editing and the AI cockpit live in the same window, on top of the same control plane. Neither is a new capability layer; both are the visual face of what the core already provides.

**Manual editing tools:**

| Tool | Core mechanism it relies on |
|---|---|
| Inspector | `TypeSchema` → fields are generated automatically (range → slider, enum → list, unit → label); every change is an `edit_apply` transaction |
| Handles (move/rotate/scale) | Live preview during the drag, a single `edit_apply` transaction on release |
| Hierarchy, asset browser | `scene_get`, `entity_query`, `asset_list` + drag and drop → `edit_apply` |
| Play/pause/step/rewind | `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore` |
| Console, performance panel | `logs_get`, `profile_get` |

**One history.** Every transaction carries who made it (`actor`: `human` or the agent's name). Changes made by hand and by the agent sit in the same revision chain, the same undo stack and the same activity feed. The agent sees what the human did by hand with `edit_history`.

**AI cockpit elements:**

| Cockpit element | Core mechanism it relies on |
|---|---|
| Activity feed, per-card undo | `edit_apply` transactions + revisions + undo stack |
| Automatic checkpoints | Document revision mark + session snapshot |
| "Show and tell" | Entity ID buffer (clicked pixel → entity) + `describe_view` |
| Before/after comparison | Document diff + images captured from two revisions |
| Permission modes | `dry_run` in the control plane + approval gate |
| Live observation | Session event stream (logs, captures, test results) |

The agent inside the cockpit talks through a provider-independent adapter (ADR-024); external agents (Claude Code, etc.) connect through MCP. The activity of both appears in the same feed, because both produce the same control plane operations.

## Distribution and export

```
Engine package (Steam / web)          Exported game (wen export)
  wen executable                        game executable (runtime only)
  cockpit                               packaged assets
  MCP server                            compiled Wasm modules
  embedded Wasm build toolchain         (no editor, MCP server or compiler)
  example projects + docs
```

The user installs no extra tools (ADR-023). The exported game does not need an engine installation.

## Threading model

- **M1–M5:** the simulation is single-threaded (correctness and determinism first). Asset loading and render preparation can run in the background; they do not touch simulation state.
- **Later:** systems with disjoint access run in parallel; command buffers merge in a fixed order. A parallel run and a single-threaded run must produce the same hash (tested).
- The design does *not require* threads (for the web target).

## Engine repository layout

```
wish-engine/
  crates/        the crates above
  examples/      small example projects (also test data)
  evals/         agent evaluation tasks
  docs/          these documents + the user guide
  CLAUDE.md      working rules for the agent that develops the engine
```
