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
| `schema_get` | Component/resource/event schemas: fields, types, units, ranges, doc comments | `names`, `search`, `detail` |
| `docs_search` | Search the engine guide (the docs are also served as `wish://docs/…` resources) | `query` |
| `check` | Validate the whole project: scenes, references, schemas, script build | `paths` |

### Scene authoring (edit document)

| Tool | Purpose | Notable parameters |
|---|---|---|
| `scene_get` | Scene outline: hierarchy tree, entity names, component names | `scene`, `root`, `depth`, `detail` |
| `entity_query` | Find entities: by component, name, expression, spatial proximity | `with`, `where`, `near`, `limit`, `cursor` |
| `entity_get` | Component values of entities; values of resources (M7) | `ids`, `components`, `resources`, `session` |
| `edit_apply` | **Atomic transaction**: a list of `spawn`, `despawn`, `set` (merge), `replace`, `remove`, `reparent`, `rename`, `set_notes`, `set_resource` / `replace_resource` / `remove_resource`, `instantiate`, `set_override` / `replace_override` / `remove_override` operations (implemented in M2 as `wish_scene::EditOp`) | `ops`, `base_revision`, `dry_run` |
| `edit_history` | Undo / redo / list the history | `action`, `steps` |
| `asset_list` | Asset files and import status | `kind`, `search` |
| `asset_info` | A report for one asset: triangle count, bounds, materials, sub-objects, scale warnings | `path` |
| `asset_move` | Move an asset file/folder and update all scene and prefab references in the same transaction (`wen mv`) | `from`, `to`, `dry_run` |
| `asset_create` | Produce a placeholder asset (M7): a `.wav` sound or a `.png` picture from a few numbers; no download needed | `path`, `sound`, `picture`, `overwrite`, `dry_run` |

### Simulation (play session)

| Tool | Purpose | Notable parameters |
|---|---|---|
| `sim_start` | Start a session from a scene | `scene`, `seed`, `headless` |
| `sim_step` | Advance: N ticks or until a condition holds; inject input | `session`, `frames`, `until`, `input`, `max_frames` |
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
| `profile_get` | Frame time breakdown, time per system, draw calls, memory | `session`, `frames` |

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
