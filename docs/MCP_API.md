# Wish Engine — MCP API Draft (v0)

This is a draft of the tool surface. **Status (M4 completed, 16 November 2026):** 20 tools were implemented (`project_info`, `schema_get`, `docs_search`, `check`, `scene_get`, `entity_query`, `entity_get`, `edit_apply`, `edit_history`, `asset_list`, `asset_info`, `asset_move`, `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore`, `sim_stop`, `capture_frame`, `describe_view`, `logs_get`); the exact parameters are in the code (the `wish_control` parameter tables, visible with `tools/list`) and the exact behavior is in [design/M4-control-plane.md](design/M4-control-plane.md) §14–§15. `script_build`, `test_run`, `replay` come in M5; `profile_get` in M6; Streamable HTTP in M8a. The draft below is a historical record; the changes brought by the evaluations and the closing review are in the design note's §14 WP7–WP8 sections. Principles: [DECISIONS.md](DECISIONS.md) ADR-009, ADR-010.

## Connection

| Transport | Use |
|---|---|
| stdio | `wen mcp` — Claude Code / Claude Desktop starts the server itself. `wen new` writes `.mcp.json` into the project, so the connection is automatic. |
| Streamable HTTP | `wen mcp --http <port or address>` (M8a WP8) — a single endpoint `/mcp`: `POST` (request → `200` + JSON, notification → `202`), `DELETE` ends the session, there is no `GET` stream (`405`). The `initialize` reply gives an `Mcp-Session-Id`, and every later message carries it (`400` if missing, `404` if unknown). By default only `127.0.0.1`; always a token (`Authorization: Bearer`; if `WISH_MCP_TOKEN` is missing, one is generated randomly and shown at start — ADR-022); another address requires `WISH_MCP_TOKEN`; `Host` must be local. Every session is recorded under its own client name. A page from another site is rejected by `Origin` (`WISH_MCP_ORIGINS` adds extra allowances). `notifications/cancelled` takes effect instantly from another connection too. Code: `wish_mcp::http`. |

Every MCP tool has an exact command-line counterpart (`wen entity query …`, `wen sim step …`), and with `--json` it gives the same output.

## Common rules

- **Reply envelope:** `{ "ok": true, … }` or `{ "ok": false, "error": Diagnostic }`.
- **Budget:** the default reply is kept short. Paging with `detail: "summary" | "normal" | "full"` and `cursor`.
- **Tools that change things** accept `dry_run`; they return the diff, the diagnostics and the new `revision`.
- **Concurrency:** if `base_revision` is given and the document changed in the meantime, the operation is rejected and the current revision is reported.
- **Who did it (`actor`):** every transaction carries `human` or the agent's name. Because the interface is hybrid (ADR-018), the human also edits the same scene by hand; the agent sees the changes made by hand with `edit_history`, and in the replies of tools that change things it gets a summary of "entities someone else changed since your last call".
- **Entity addressing:** by scene ID (`"player"`); in a play session also by run-time ID.
- **Expressions:** a small expression language (a CEL subset) in queries and checks: `Transform.translation.y < 0 && has(RigidBody)`.

## Tools

### Project and information

| Tool | Purpose | Notable parameters |
|---|---|---|
| `project_info` | Project summary: scenes, script status, open sessions, engine version, pending diagnostics | — |
| `schema_get` | Component/resource/event schemas: fields, types, units, ranges, doc comments; `search` also knows the words people use ("mesh" finds `Primitive`, "position" `Transform`) | `names`, `search`, `detail` |
| `docs_search` | Search the engine guide (the docs are also served as `wish://docs/…` resources) | `query` |
| `check` | Validate the whole project: scenes, references, schemas, script build | `paths` |

### Scene authoring (edit document)

| Tool | Purpose | Notable parameters |
|---|---|---|
| `scene_get` | Scene outline: hierarchy tree, entity names, component names | `scene`, `root`, `depth`, `detail` |
| `entity_query` | Find entities: by component, name, expression, spatial proximity; an unknown name among `components` (to show) is left out with a warning, an unknown name in `with` fails | `with`, `where`, `near`, `limit`, `cursor` |
| `entity_get` | Component values of entities; values of resources (M7) | `ids`, `components`, `resources`, `session` |
| `edit_apply` | **Atomic transaction**: a list of `spawn`, `despawn`, `set` (merge), `replace`, `remove`, `reparent`, `rename`, `set_notes`, `set_resource` / `replace_resource` / `remove_resource`, `instantiate`, `set_override` / `replace_override` / `remove_override` operations (implemented in M2 as `wish_scene::EditOp`); answers `now`: the changed entities as they are after the change (touched components, `world_position`), so an agent need not read them back | `ops`, `base_revision`, `dry_run` |
| `edit_history` | Undo / redo / list the history | `action`, `steps` |
| `asset_list` | Asset files and import status | `kind`, `search` |
| `asset_info` | A report for one asset: triangle count, bounds, materials, sub-objects, scale warnings | `path` |
| `asset_move` | Move an asset file/folder and update all scene and prefab references in the same transaction (`wen mv`) | `from`, `to`, `dry_run` |
| `asset_create` | Produce a placeholder asset (M7): a `.wav` sound or a `.png` picture from a few numbers; no download needed | `path`, `sound`, `picture`, `overwrite`, `dry_run` |

### Simulation (play session)

| Tool | Purpose | Notable parameters |
|---|---|---|
| `sim_start` | Start a session from a scene | `scene`, `seed`, `headless` |
| `sim_step` | Advance: N ticks or until a condition holds; inject input; `watch` reports up to 20 entities' world position and physics velocity at the end | `session`, `frames`, `until`, `input`, `max_frames`, `watch` |
| `sim_snapshot` | Save / list a moment | `session`, `label` |
| `sim_restore` | Return to a saved moment or fork a new session from it | `snapshot`, `fork` |
| `sim_stop` | Close the session | `session` |
| `replay` | Save a replay recording / run it / find the hash drift | `action`, `path` |

During a session, `entity_query`, `entity_get` and `edit_apply` also apply to the live world with the `session` parameter.

### Observation

| Tool | Purpose | Notable parameters |
|---|---|---|
| `capture_frame` | Take an image (returned as MCP image content) | `camera`, `size`, `view`: `lit` \| `ids` \| `depth` \| `normals` \| `wireframe` \| `colliders` \| `lights` |
| `describe_view` | Semantic vision: visible entities, screen boxes, visibility percentage, distance | `camera`, `limit` |
| `logs_get` | Structured logs and events (including audio events) | `since`, `severity`, `kinds` |
| `physics_query` | Ask the physics world (M6): what a ray hits, what a shape covers, what touches an entity — in a session or in the scene file | `ray`, `overlap`, `touching`, `mask`, `session` |
| `profile_get` | **Planned, not built** (see the total below). Frame time breakdown, time per system, draw calls, memory | `session`, `frames` |

`capture_frame` and `describe_view` also answer **`stats`** (M12): what the picture takes to draw, in counts that are the same on every machine and every run, so they can be compared and asserted.

```json
"stats": { "width": 512, "height": 288, "draw_calls": 10, "triangles": 3870,
           "shadow_casters": 4, "point_lights": 0, "spot_lights": 0, "effects": [] }
```

- `draw_calls` and `triangles` cover every pass of the frame (the shadow map, the sky, the surfaces, bloom, tone mapping, FXAA, sprites and particles, the interface); `shadow_casters` is how many of those draws went into the shadow map (0: none was drawn); `point_lights` and `spot_lights` are the lights that reach some part of the view.
- `effects` lists the optional look effects the lit picture used, in the order of the frame's passes: `sky_light` (`Ambient.source: Sky`), `sun_disc` (`Sky.sun_size_deg` above 0, with a `DirectionalLight`), `fog` (`Fog.density` above 0), `bloom` (`Bloom.strength` above 0; it is two passes for each level of its chain, seven levels in a picture 720 pixels high), `color_grading` (a `ColorGrading` field off its default), `fxaa` (`RenderSettings.antialiasing: Fxaa`). All are off by default, so a scene that sets none answers `[]` and is drawn as it was before they existed; a debug view uses none. A sun disc asked for in a scene without a `DirectionalLight` is the warning `sun_disc_without_light`; in a picture of a scene file its `location` is that file and `/resources/Sky/sun_size_deg`.
- In `capture_frame` they describe the picture that was returned (a debug view counts its own pass); in `describe_view` they describe the lit picture of that view.
- **`stats` and the rest of these two answers carry no time value**: two runs on one adapter give the same answer (`wish_control/tests/view_tools.rs` compares the answers of two workspaces on the software adapter, pictures included). Time depends on the machine; `wen bench --render` measures it on the command line. This is not yet true of every tool: `script_build` answers `seconds` and `project_info` answers `last_build.seconds`, the open follow-ups of ADR-027.
- Both tools report what is not drawn, in `diagnostics`. Every `view` of `capture_frame` reports what holds for the scene whatever the view: entities that are not drawn and the scene's light warnings (`too_many_lights`, `extra_directional_lights`). The warnings that belong to the lit picture alone (`light_clusters_full`, and the interface's, such as `ui_atlas_full`) come with `view: "lit"` and with `describe_view`, which reports every warning of the lit picture it describes. Entities are named by their scene ids. `extra_directional_lights` (a scene with more than one `DirectionalLight`: only the first in scene order is drawn) names the light that is used and the ones that are not, and points at the first unused one in the scene file (`location.file` and `location.path`; a checkpoint's or a preview's picture carries no place).
- A tool call leaves at most 64 MiB of render targets in memory for the next call (the usual picture sizes are reused, a large capture is let go of when the call ends).
- The `RenderSettings` resource (`shadows`, `shadow_map_size`, `antialiasing`) is set like any other resource (`edit_apply` `set_resource`) and changes these counts; so are the look's resources (`Ambient`, `Sky`, `Fog`, `Bloom`, `ColorGrading`), which change `effects`.

### Verification and scripting

| Tool | Purpose | Notable parameters |
|---|---|---|
| `test_run` | Run scenario tests; on failure the relevant tick, expected/found value, visual difference | `paths`, `update_golden` |
| `script_build` | Compile game code; condensed diagnostics; hot reload if successful | `reload` |

### Project settings, cockpit and game design (M8a–M9c)

| Tool | Purpose | Notable parameters |
|---|---|---|
| `project_settings` | The project's name, default scene, input map (M8a) | `action`, `name`, `scene`, `input`, `dry_run` |
| `activity_get` | Activity feed: who changed what, for per-card undo (M8b) | `since`, `actor`, `limit` |
| `checkpoint` | Save / list / restore / delete a checkpoint (M8b) | `action`, `name`, `note`, `dry_run` |
| `permissions` | How much agents may change on their own; only the person changes it (M8b) | `action`, `mode`, `dry_run` |
| `proposals` | Changes waiting for the person's approval in suggest mode (M8b) | `action`, `id`, `dry_run` |
| `project_export` | Export the game to a folder that runs without the engine (M9) | `name`, `overwrite`, `dry_run` |
| `game_design` | The game's design in `design/game.json` (M9c): the game card (name, one sentence, genre, goal, core loop, controls, look, audience, must-haves, left for later), decisions that are valid at every change, plan steps, play-test notes (with tick and picture), multiple-choice questions the agent asks the person. Actions: `get`, `set_brief`, `set_workspace` (M10a: a workspace's own card fields, keeping every value), `decide`, `forget`, `plan_add`, `plan_set`, `plan_remove`, `note`, `note_done`, `ask`, `answer` (with `remember` it becomes a decision). IDs are deterministic (`next_id`); a broken file is never overwritten (`design_file_invalid`); `design/` is not counted as an asset | `action`, `fields`, `workspace`, `card`, `text`, `id`, `status`, `options`, `answer`, `remember`, `scene`, `tick`, `picture`, `dry_run` |
| `packages` | Packages (M10a): themes, workspaces, languages, recipes and templates in a folder with a `wish-package.json`. `list` (the project's, locked by `wish.lock`, and the person's; warns when files no longer match the lock), `check` (read a package folder without installing), `install` (scope `project` by default: copied into `packages/<id>/`, recorded in `wish.json` and `wish.lock`; scope `user` is a person's only), `remove` (to the trash, never deleted), `lock`. From the package index (M10b; a folder of packages or an `index.json`, given by `index`, `WISH_PACKAGE_INDEX` or the person's cached index): `search` (`query`; each result names its newest version that fits this engine, the installed versions and, per scope, whether an update waits), `add` (`id`, optional `version` requirement: the newest fitting version, its content hash checked when the index gives one), `update` (every installed package, or `id`, to its newest compatible version; `version` with `id` picks another; a package that cannot be updated is listed in `failed` and the others still are). An index names only folders inside itself (a link leading out is refused). Nothing is half-installed: a package is copied beside its place and moved in when complete. `wen pkg` calls it | `action`, `path`, `id`, `query`, `version`, `index`, `scope`, `replace`, `dry_run` |

The editor's AI mode does every action of this tool by hand too (invariant 9); in every task the agent receives a summary of the design, and for choices that belong to the person it asks with `ask` and stops instead of guessing.

| `engines` | Engine versions side by side (M10b): `list` (the running engine, the ones installed in `engines/` of this computer's own settings folder and folders the person added; which one opens this project; `stale`: added folders that are no longer engines), `pick` (`path`: which engine opens another project: the exact version in its `wish.json`, else the newest compatible one, `fit` `exact`/`compatible`/`none`, `switch`), `add` (a person only: an engine folder with `wen` and `wish-engine.json`, kept where it is), `remove` (a person only: an added one is forgotten, an installed one goes to the trash, a stale folder is forgotten (`stale: true`); the running one cannot remove itself). A list written by a newer engine is never overwritten. `wen engines` calls it | `action`, `path`, `dry_run` |

**Total: 34 tools** (seven of the M8a–M9c ones and M10's `packages` and `engines` are above). The earlier count (in the implementation: the 20 of M4, in M5 `script_build`, `replay`, `test_run`, in M6 `physics_query`, in M7 `asset_create` (friction log F5: there was no way to produce a sound/picture with a tool); `profile_get` does not exist yet). (`asset_move` was added in M2: because it changes several documents and the disk together, it could not be a parameter of `edit_apply`, which is tied to a single scene.) A reason is required to add a new tool; first it is checked whether it can be solved as a parameter of an existing tool.

## Example replies

`sim_step` — it does not only advance, it summarizes what happened in that span:

```json
{
  "ok": true,
  "session": "s1",
  "tick": 600,
  "stopped_by": "frames",
  "world_hash": "9f2c41d07a3e…",
  "events": { "CollisionStarted": 14, "Despawned": 2 },
  "diagnostics": [
    {
      "severity": "warning",
      "code": "entity_out_of_bounds",
      "message": "Entity 'crate_7' is at y = -312 m; it probably fell out of the world.",
      "hint": "Add a floor collider below it, or add a KillZone to despawn fallen entities."
    }
  ]
}
```

`edit_apply` — a faulty transaction changes nothing:

```json
{
  "ok": false,
  "error": {
    "severity": "error",
    "code": "value_out_of_range",
    "message": "Mover.speed = 500 is outside the allowed range 0..=50 (m/s).",
    "location": { "op": 2, "path": "/entities/player/components/Mover/speed" },
    "hint": "Use a value within range, or widen the range in the component definition in scripts/src/lib.rs.",
    "docs": "wish://docs/components/Mover"
  }
}
```

## Typical agent loop

```
1. project_info                       → where am I, what is here?
2. schema_get(search: "light")        → which components, which fields?
3. edit_apply([spawn floor, light, camera, player])
4. (edit the scripts/src/lib.rs file) → script_build
5. sim_start → sim_step(frames: 120, input: {move:[0,1]})
6. describe_view + capture_frame      → is what I expect on the screen?
7. a problem → sim_restore → fix → go back to 5
8. write tests/…test.json → test_run  → proof
```

## MCP resources and prompts

- `wish://docs/guide` — a short engine guide (small enough to fit the agent's context)
- `wish://docs/components/{Name}` — component docs (generated from the schema)
- `wish://docs/diagnostics/{code}` — the explanation and fix of every diagnostic code
- `wish://project/status` — live project status
- Prompts: "start a new game", "reproduce and fix the bug", "inspect performance"

## How the design is tested

The tool surface is measured with the tasks under `evals/` (ADR-017). On every tool change, these are watched: the task success rate, the number of tool calls per task, tokens per task, the rate at which the agent makes an invalid call on the first try. If the numbers get worse, the change is reverted.
