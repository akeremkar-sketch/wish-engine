# M4 — Control plane and MCP: design note

Status: written (15 November 2026). Read before writing code. Context: [ROADMAP](../ROADMAP.md) M4, [DECISIONS](../DECISIONS.md) ADR-009/010/017/018/024, [MCP_API](../MCP_API.md) (tool draft), [ARCHITECTURE](../ARCHITECTURE.md) "World and session model", "Journey of an edit".

## 1. Goal and limits

At the end of M4, Claude (or any MCP client) builds a scene, runs it, takes an image and verifies the result **only with MCP tools**, in an empty project opened with `wen new`. Everything is also done from the `wen` command line and gives **the same JSON**. The first 10 evaluation tasks pass at ≥ 80% with a mid-tier model (Sonnet 5.5 · medium).

**Out of scope (not in M4):**
- Game code and `script_build` (Wasm, M5). In M4 the component types are the engine's own types + the types of a built-in example (§4).
- Physics, input actions: the `input` parameter of `sim_step` is M5 (there is no action map). The `colliders` view of `capture_frame` is M5.
- `test_run` (scenario tests, ADR-017 layer 2) and `replay`: M5 (there are few meaningful scenarios without game code and input). `profile_get`: M6 (when the render/game profile has settled).
- **Streamable HTTP transport: postponed to M8a.** *(A plan change; it does not grow the scope, it shifts it.)* Reason: the purpose of HTTP is "to connect to a running editor or game"; in M4 there is no long-lived process to connect to — the `wen mcp` stdio server is already that process. When the editor (M8a) arrives, HTTP is opened inside the editor onto the same `wish_control` core. If done now, an HTTP stack and session token security would be added with no real use to test.

**Invariants:** 4 (single control plane) is the milestone itself: the existing project commands of `wen` (`check`, `fmt`, `mv`, `capture`, `describe`, `shaders`) also move to `wish_control`; the command line only turns flags into an argument object. 5 (single diagnostic format): tool errors are also `Diagnostic`. 9 (hybrid): every operation carries an `actor`; what is done manually (command line/file) and what is done by an agent enter the same history.

## 2. Crates and dependency direction

| Crate | Responsibility | External dependency |
|---|---|---|
| `wish_control` | **Workspace** (`Workspace`: project, open documents + history, playback sessions, renderer), **tool catalog** (name, description, parameter schema, output), **dispatcher** `call(name, args, actor) -> Value`, expression language, world inspector, diagnostic catalog | — (serde_json is already there) |
| `wish_mcp` | MCP server: JSON-RPC 2.0 stdio, `initialize`/`tools`/`resources`/`prompts`, `wish://` resources. Knows only `wish_control` | — |
| `wish_cli` | Thin client: `wen call`, `wen do`, `wen mcp`, `wen new` + shortcut commands (`capture`, `check`…), all `wish_control::call` | — |

Direction: `wish_control` → `wish_scene`, `wish_asset`, `wish_app`, `wish_render`, `wish_import`, `wish_transform`, `wish_examples`; `wish_mcp` → `wish_control`; `wish_cli` → both. `wish_window` is not tied to the control plane (`wen view` merges with the editor in M8a).

**Decision — we implement MCP ourselves (not the official Rust SDK `rmcp`).** The needed surface is small (one JSON-RPC message per line over stdio; ~10 methods). Our own implementation: (1) it does not need an asynchronous runtime (tokio) — the engine is single-threaded, deterministic and works with the GPU lock; (2) we map every error path to our own `Diagnostic` (instead of translating the SDK's error types); (3) the dependency tree and license surface stay small; (4) we control the protocol version negotiation. Cost: updating as the protocol changes is on us. Measure: compatibility tests with recorded real session transcripts (§9) and a real connection trial with Claude Code in WP6. The target protocol version is **2025-06-18** (structured output `structuredContent`, `outputSchema`, tool `title`); if the client asks for a newer version we reply with ours (the negotiation rule of the specification), and if it asks for an older one we use it if we support it.

## 3. Workspace model

```
Workspace (one process = one project)
├── project: root folder, wish.json (name, types, default scene)
├── registry: engine types + project types (M4: built-in example; M5: Wasm module)
├── documents: scene path → { SceneDocument, history (with actor), disk fingerprint }
├── sessions: "s1", "s2"… → { App, source scene + revision, seed, snapshots }
├── renderer: lazy (the GPU is opened at the first capture/describe call; if there is no GPU, only those tools give an error)
└── actors: actor → last seen revision (to report changes by others)
```

- **Files are the source of truth (invariant 3).** Every call that changes something writes the document to disk in canonical form **immediately** (atomic: temp file + rename). At the start of every call the workspace looks at the disk fingerprint of the open documents (the `fingerprint_dir` approach of M3, per document): if it changed from outside, it reloads and writes this to the history as `Actor::File`. If it is invalid, the last valid document stays and a diagnostic is added to the response of the call.
- **Documents open lazily:** a tool loads a scene the first time it touches it; if the `scene` parameter is not given, the default scene in `wish.json`.
- **Sessions** are copied from the current revision of the document (`sim_start`), do not write to disk, and are independent of each other; `sim_restore(fork: true)` opens a new session. Session ids are sequential (`s1`, `s2`…): deterministic and easy for the agent to read.
- **Single thread.** The MCP server handles calls in order; there is no concurrency problem. For a long `sim_step` there is an upper limit `max_frames` (default 10 000, at most 1 000 000) and the cancel notification (`notifications/cancelled`) is checked at the tick boundary.
- **Who did it:** in MCP `actor` comes from `clientInfo.name` that the client gives in `initialize` (`Agent("claude-code")`); on the command line `Human` (changeable: `--actor`). In every response that changes something, `changed_by_others`: the entities that other actors touched since the last call of this actor (at most 20, more as a count). This is how the agent sees an object that a human moved by hand (ADR-018 rule 4).

## 4. Project file and types

`wen new <folder> [--name <name>]` writes these:

```
wish.json              { "format": 1, "name": "...", "engine": "0.1.0", "scene": "scenes/main.scene.json", "types": { "example": "stress" } }
scenes/main.scene.json camera + sun + ground (empty but drawable start)
assets/                empty
.mcp.json              { "mcpServers": { "wish": { "command": "wen", "args": ["mcp"] } } }
CLAUDE.md              short guide specific to the project (tool loop, rules; points to wish://docs/guide)
.gitignore
```

- If `wen` is not in PATH, the full path of `wen` is written to `.mcp.json` (`std::env::current_exe`). For project portability the next `wen new`/`wen mcp` can refresh it; a note for M11.
- **Types:** the engine types are always there (the list in `wish_cli::types` moves to `wish_control`). `types.example` is the temporary path of M4 (today's `--example` flag moved into the project file); `types.module: "scripts"` comes in M5. The `--example` flag of `wen` keeps working backward compatibly if there is no `wish.json`.
- `wish.json` is also canonical JSON and `wen check` validates it too.

## 5. Tool catalog (M4)

Of the 24 tools in the draft, **20** are implemented in M4; `script_build`, `test_run`, `replay` come in M5, `profile_get` in M6.

| Group | Tools |
|---|---|
| Project | `project_info`, `schema_get`, `docs_search`, `check` |
| Scene | `scene_get`, `entity_query`, `entity_get`, `edit_apply`, `edit_history` |
| Asset | `asset_list`, `asset_info`, `asset_move` |
| Simulation | `sim_start`, `sim_step`, `sim_snapshot`, `sim_restore`, `sim_stop` |
| Observation | `capture_frame`, `describe_view`, `logs_get` |

(20 tools. `script_build`, `test_run`, `replay` in M5; `profile_get` in M6 → 24.)

**Shared contract** (the rules in the draft, made final):
- The response envelope is `{ "ok": true, ... }` / `{ "ok": false, "error": Diagnostic }`. Warnings are `diagnostics: [...]` (only if there are any).
- **Parameter validation in one place:** the parameters of every tool are defined by a table (name, type, required or not, default, range, doc). The same table produces (1) the MCP `inputSchema` (JSON Schema), (2) the command line help, (3) the parser. An unknown parameter → `unknown_parameter` + "did you mean" + `candidates`; a wrong type → `invalid_parameter` + the expected type and an example. (Instead of the general errors of serde; so that the agent can fix it on the first try.)
- `detail: "summary" | "normal" | "full"` (default `summary`) and `limit`/`cursor` in tools that return lists. The cursor is an opaque string, deterministic.
- The ones that change something (`edit_apply`, `edit_history`, `asset_move`): `dry_run`, `base_revision`; in the response `revision`, `diff` (added/deleted/changed component fields per entity, with JSON Pointers), `changed_by_others`.
- Entity addressing: scene id (`"player"`); in sessions also the scene id (for entities born in a session, the runtime id `"#<number>"`).
- **Expression language** (`entity_query.where`, `sim_step.until`): a small, deterministic subset — field paths (`Transform.translation.y`), number/string/boolean constants, `+ - * /`, comparisons, `&& || !`, `has(Component)`, `abs()`, `len()`, `distance(a, b)` (vectors). A parse error gives a location + hint. Only this slice of CEL; no full CEL dependency.
- **Images:** `capture_frame` returns MCP image content (PNG, base64); default 512×288 (token budget), at most 2048 per side. If `out` is given, it also writes to a file. In the JSON envelope `image: { mime, width, height, bytes, data? }` — on the command line `data` is written only if there is no `--out`.

**Important tool decisions:**
- `scene_get`: a tree (scene id, name, component names, children); `detail: full` also gives the component values. In big scenes `root` + `depth`.
- `entity_query`: `with` (component names), `where` (expression), `name` (glob: `crate_*`), `near` ({ point, radius } — by `Transform.translation`), `limit` (default 50), `cursor`. The result is only ids + `components` if asked.
- `edit_apply`: the `EditOp` list of M2 as it is (`wish_scene` is already atomic). An error has `location.op` (operation order) + JSON Pointer.
- `sim_step`: `frames` or `until` (expression) + `max_frames`; response: `tick`, `stopped_by` (`frames`|`until`|`max_frames`|`cancelled`), `world_hash`, event counts, world inspector warnings.
- **World inspector** (M4 rules): non-finite transform (`transform_not_finite`), an entity that falls out of the world (`entity_out_of_bounds`, default |y| > 1000 m), zero scale (`degenerate_transform`), capture of a scene without a camera. The rules are in a table; physics rules are added in M5.

## 6. Command line parity

- `wen call <tool> ['{json args}' | --<param> <value>…] [--actor <name>]` — a single call, JSON output **byte for byte the same as `structuredContent` in MCP** (canonical JSON).
- `wen do [file|-]` — a JSON array or a JSONL call list, **in one process in order**; each result is one line. Sessions and the undo stack live only inside the process (ARCHITECTURE: a session does not write to disk), so the command line equivalent of stateful flows (sim_start → sim_step → capture) is `wen do`.
- The shortcuts (`wen capture`, `wen check`, `wen mv`…) stay; they only turn flags into arguments and do `call`.
- **Automatic parity test:** a list of example calls for every tool; the same list is given (a) in process to `wish_control`, (b) to a `wen do` process, (c) to a `wen mcp` process by JSON-RPC; the three outputs are compared (images included — the same bytes on the same software adapter). If a new tool is added and its example is not written, the test fails (the catalog ↔ example list must match exactly).

## 7. MCP server (`wen mcp`)

- stdio: each line is a JSON-RPC message (the MCP stdio transport). stdout is only protocol; logs go to stderr.
- Methods: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`, `notifications/cancelled`. Unknown method → JSON-RPC `-32601`; a tool error is **not** a protocol error: the `tools/call` result is `isError: true` + the envelope (an MCP rule — the agent sees the error and fixes it).
- `tools/call` result: `content` (a short text summary + the image if any) and `structuredContent` (the full envelope), `outputSchema` from the tool catalog.
- **Resources:** `wish://docs/guide` (hand-written, ~2–3 thousand tokens to fit the agent context), `wish://docs/components/{Name}` (generated from the schema), `wish://docs/diagnostics/{code}` (diagnostic catalog), `wish://project/status` (live summary). The `docs` field in responses points to these.
- **Diagnostic catalog:** for every diagnostic code an explanation + typical fix, one table in `wish_control`. **Test:** every `Diagnostic::error/warning("code", …)` in the source code must be in the catalog (the repo is scanned); if one is missing, the test says its name. So a `docs` link is never empty.
- **Prompts:** "set up a new scene", "reproduce and fix a problem" ("investigate performance" in MCP_API is M6).

## 8. Evaluations (`evals/`)

- Each task is a folder: `task.md` (the prompt given to the agent, in the user's language), `setup` (the starting project: usually the output of `wen new`), `check.json` (verification calls run with `wen do` + expected results: e.g. "`entity_query where Transform.translation.y > 2` at least 3 entities", "`player` is visible in `describe_view`"). Verification uses the engine's own tools, no human eye.
- Runner: `evals/run` — a temporary project copy for every task, `claude -p "<task>" --mcp-config <project>/.mcp.json --model <model> --allowedTools mcp__wish__*` (only MCP tools; no file editing → the "only with MCP" criterion is met structurally), then `check.json`. Record: `evals/results/<date>-<model>.json` (success, number of tool calls, tokens, ratio of first invalid calls).
- The first 10 tasks (increasing difficulty): put a cube in an empty scene · a level with three platforms · change the light to sunset · place the camera so that it looks at an object · lay out five instances from a prefab · find and fix a wrong value in the scene · advance the simulation 120 ticks and find the one that falls · make an invisible object visible (describe_view) · import a model (glTF) and place it · notice a change a human made by hand and continue accordingly.
- **A decision that belongs to the user:** running the evaluations spends Claude usage (subscription/API). In WP7, approval is asked from the user before running; the number of repeats (suggestion: 3 per task, Sonnet 5.5 · medium) and the estimated cost are stated.

## 9. Test plan (with exit criteria)

| Exit criterion | Evidence |
|---|---|
| From an empty project, build a scene → run → take an image → verify, only with MCP | (a) a scripted end-to-end test: `wen new` + a typical 8-step loop by JSON-RPC to a `wen mcp` process (in CI, software adapter); (b) the `evals/` tasks with real Claude Code (locally, approved) |
| The command line equivalent of every MCP tool gives the same JSON | The §6 automatic parity test (for every tool in the catalog) |
| The first 10 tasks Sonnet 5.5 · medium ≥ 80% | The `evals/results/` record (3 tries per task) |
| Every invalid call gives a structured error with a hint | Fuzz test: for every tool, broken arguments derived from the catalog (missing required, wrong type, unknown parameter, out of range, non-existent scene/entity/session) → all `ok:false`, `hint` not empty, `code` in the catalog; the process never crashes |
| (extra) MCP protocol compatibility | Recorded JSON-RPC transcripts (initialize → tools/list → call → error → resource read) golden files; a manual connection trial with Claude Code (WP6) |
| (extra) Hybrid history | Agent change + a file edit from outside + agent change → `edit_history` shows the order and actors correctly; `changed_by_others` in the agent's response contains the human's entity |

## 10. Work packages and recommended model

| # | Work package | Model · effort |
|---|---|---|
| WP1 | `wish_control` skeleton: `Workspace`, `wish.json`, moving the type registry, parameter table → schema/parser, dispatcher, envelope; `project_info`, `schema_get`, `check`; `wen call` / `wen do`; parity test setup | Opus 5.5 · high |
| WP2 | Scene tools: `scene_get`, `entity_query` (+ expression language), `entity_get`, `edit_apply` (`dry_run`, `base_revision`, `diff`, `changed_by_others`), `edit_history`; disk reconciliation (`Actor::File`) | Opus 5.5 · high |
| WP3 | Asset tools (`asset_list`, `asset_info`, `asset_move`), `wen new` (skeleton, `.mcp.json`, project `CLAUDE.md`), moving the old `wen` commands to `wish_control` | Sonnet 5.5 · medium |
| WP4 | Sessions: `sim_start/step/snapshot/restore/stop`, `until`, cancel, `logs_get`, world inspector | Sonnet 5.5 · high |
| WP5 | Observation: `capture_frame` (image content), `describe_view`; in a document and in a session | Sonnet 5.5 · medium |
| WP6 | `wish_mcp` + `wen mcp`: JSON-RPC stdio, tools/resources/prompts, diagnostic catalog + scan test, `docs_search`, `wish://docs/guide`; protocol transcript tests; a real connection with Claude Code | Opus 5.5 · high |
| WP7 | `evals/`: 10 tasks, runner, checker; a run and record **with the user's approval**; fix the tool surface according to the results | Opus 5.5 · high (setup), evaluated: Sonnet 5.5 · medium |
| WP8 | Fuzz test, scripted end-to-end test, independent closing review, M4 result, merge to `main` | Opus 5.5 · high |

## 11. Risks

- **The guess of the tool surface may be wrong.** Measure: according to the WP7 results, changes of names/parameters are free (there is no external user yet); every change is justified with eval numbers (ADR-010).
- **The MCP specification is changing.** Measure: version negotiation + transcript tests; the protocol layer is isolated in `wish_mcp`.
- **Token budget:** `scene_get` can blow up in a big scene. Measure: default `summary`, `limit`, an upper limit on response size (if exceeded it is cut + `truncated: true` + a hint on how to narrow it).
- **A machine without a GPU:** everything except `capture_frame`/`describe_view` must work without a GPU; the renderer opens lazily, and instead of `--software` the software adapter can be chosen with `wish.json`/an environment variable.
- **The external edit race:** if a file is read before it is fully written (seen in M3) — on a load error retry once after 100 ms, then keep the last valid document.

## 12. Decisions / approvals that belong to the user

1. WP7: the evaluation runs spend Claude usage — approval before running (with the number of tries and the estimated cost).
2. Moving the HTTP transport to M8a is a technical decision (reason §1); the user is told for information.

## 13. Open questions

1. ~~`euler_deg`~~ — closed in WP2 (§14). Old text: the `euler_deg` short form for `Transform.rotation` in `edit_apply` (carried over from M3): writing quaternions is an error source for the agent. Decision in WP2: **yes**, but only as input convenience — the quaternion is written to the document (the file stays canonical), and both are shown in the response.
2. Should derived values (`GlobalTransform`) be shown in `entity_get` results? Suggestion: yes in sessions (marked read-only), no in a document.

## 14. Implementation notes

### WP1 — `wish_control` skeleton, `wen call` / `wen do`

- **Crate:** `wish_control` (`param`, `project`, `tool`, `tools/`, `types`, `workspace`). The list of engine types moved from `wish_cli::types` to here; `wish_cli::types` is now only a thin adapter for the old `--example` commands.
- **Parameter table (`Param`, `Kind`):** the parameters of each tool are a `const` array; from the same table `input_schema` (MCP JSON Schema: type, range, `enum`, `default`, `description`, `additionalProperties: false`), `parse` (parser + defaults) and `from_flags` (command line `--name value`; for lists a comma or JSON, a flag written alone is `true`) are produced. Error codes: `invalid_arguments` (not an object), `unknown_parameter` (+ "did you mean", `candidates`), `missing_parameter`, `invalid_parameter` (what was expected + an example spelling + the doc of the parameter); all with `location.path` = `/<parameter>`.
- **Envelope:** `Workspace::call(tool, args, actor) -> Value` never fails: a panic inside a tool turns into an `internal_error` response, the process stays alive. The response keys are sorted (serde_json `Map` = `BTreeMap`) → the same call gives the same bytes everywhere.
- **`wish.json`:** `format`, `name`, `scene`, `engine`, `types.example`; `invalid_project_file` errors with locations (unknown setting → suggestion). A broken or missing `wish.json` does not prevent opening the workspace: the tools work with the engine types, `project_info` and `check` report the problem (if missing, a `no_project_file` warning with the "`wen new`" hint).
- **Tools:** `project_info` (name, default scene, list of scenes/prefabs, entity count, the error/warning counts of `check`, type counts), `schema_get` (`names` / `search` (all words are searched in name, field names and docs) / `detail`: summary = first sentence + field names, normal = type/unit/range/default/doc of the fields, full = JSON Schema; managed types are `read_only`), `check` (`passed`, file/error/warning counts, diagnostics; if `paths` is wrong, `file_not_found` + the nearest file). Tool name rule: `snake_case`, `name_action` or a single verb (`check`, as in the draft).
- **`wen call <tool> ['{json}' | --param value …] [--root] [--example] [--actor]`** and **`wen do [file | -]`** (a JSON array or one call per line; each response is one line; a broken list is reported with the line number). The exit code comes from `ok`.
- **Parity test (`wish_cli/tests/parity.rs`):** example calls for every tool (including deliberately wrong ones) are run in process, with `wen call` and with `wen do`; the three outputs are the same **byte for byte**. If a tool in the catalog has no example, the test fails. The MCP leg will be added in WP6.
- **Tests:** `wish_control` unit (parameters and project file) + `tests/catalog.rs` (catalog rules: docs, defaults pass their own check, tools that change something carry `dry_run`; broken/missing `wish.json`; `schema_get` search). 316 tests green in total.

### WP2 — scene tools, expression language, reconciliation with files

- **Tools:** `scene_get` (tree; `root`, `depth`, `detail`; cut at 500 entities + `truncated` and a narrowing hint), `entity_query` (`with`, `where`, `name` glob, `near` {point, radius} by world position — nearest first, `components` values, `limit`, `cursor` = `after:<id>`), `entity_get` (effective values: prefab parts unfolded, defaults filled; `SceneId`/`Hierarchy`/`GlobalTransform` hidden unless requested; parts that come from a prefab are `from_prefab: true`), `edit_apply` (`ops`, `base_revision`, `dry_run`, `create`), `edit_history` (`list` / `undo` / `redo`, `steps`, `dry_run`). The ones that change something and `scene_get` return `changed_by_others`: the entities that other actors (a person, a file, another agent) touched since this actor last looked, and who they are.
- **Operation format (`ops.rs`):** `{ "op": "spawn", "id": ..., ... }`; all 15 `EditOp`s of M2. Errors are `invalid_op` + `location.op` + `/ops/<n>/<key>` + "did you mean". **The `euler_deg` short form (open question 1 closed):** in `Transform` values `[x, y, z]` degrees → the `rotation` quaternion (first Y yaw, then X pitch, then Z roll); only the quaternion is written to the file (rounded to 1e-7).
- **Expression language (`expr.rs`):** paths (`Transform.translation.y`, `[i]`, `.x .y .z .w` on vectors, `(a + b).x`), constants, `+ - * /` (element by element on vectors), comparisons (not chained), `&& || !` (short circuit), `has`, `abs`, `len`, `length`, `distance`, `min`, `max`, `matches` (glob). On an entity without the component the path is `null`; every comparison with `null` and with a type mismatch is false, division by zero is `null` — a query never breaks halfway. At compile time component and field names are checked against the schema (`expr_unknown_name` + suggestion), a syntax error comes with a column (`expr_syntax`); nesting depth and length are limited.
- **Document store (`documents.rs`):** a scene opens at the first touch; every change is written immediately **canonical and atomic** (temp file + rename). At every call the file is read again; if it changed from outside, the difference is turned into an **`Actor::File` operation** and applied (add → reparent → remove → component/override/note/resource changes) and it is verified that the result is the same as the file; if it is not the same, the file is taken as it is and `history_reset` information is given. So a person's change in a text editor enters the same history and can be **undone** (in the test: agent → file → agent; undoing two steps also undoes the person's change). If the file becomes invalid, the last valid document stays, the reading tools give a warning (`external_change_rejected`), the changing tools reject with `scene_file_invalid` — **a person's half-finished edit is never overwritten**; for a half-written file it is read once more after 100 ms. When prefab files change, the library is refreshed.
- **View:** `entity_query`/`entity_get` instantiate the document into a world and read it (cache per revision), so the same read path will be used with sessions (WP4). **Open question 2 closed:** in a document `GlobalTransform` is shown only if requested (`near` uses it for the world position).
- **Revisions are meaningful inside a process:** every process opens the document from revision 0; `base_revision`, undo and `changed_by_others` are valid during one session (the MCP server, `wen do`). The file is always the shared truth.
- **The parity test** became stateful: the example list is run as one session both in process and with `wen do` (in separate project copies), every response and the scene file in the result are the same byte for byte; every call that does not change something is also compared with `wen call` against a fresh workspace.
- **Tests:** `expr` (3), `ops` (3), `documents` (2) unit; `tests/scene_tools.rs` (7): an agent builds a scene (file canonical, `check` passes), queries (component, condition, name, distance + ordering, paging, errors), effective values and the nearest-id suggestion, a dry run/a failed operation/an old revision change nothing, a person + an agent in one history and undo, not overwriting a broken manual edit, a new scene + a prefab instance. 331 tests in total.

### WP3 — asset tools, `wen new`, moving the old commands

- **Tools:** `asset_list` (`kind`: model/image/other, `search`, `limit`; `wish.json`/`CLAUDE.md` and `.meta.json` are not listed), `asset_info` (for glTF the triangle/vertex counts, the number of drawn parts and triangles, size and bounds in meters, material/texture counts, import warnings; an unusual scale gives a `model_scale_unusual` warning: >100 m or <1 cm; `.meta.json` import settings; the list of scenes/prefabs that use the asset `used_by`; a wrong path `asset_not_found` + the nearest path), `asset_move` (`wish_asset::plan_move/apply_move`; `dry_run`). After a move the open scenes are updated through reconciliation with files (WP2): the move enters the scene's history as a `file` operation and can be undone.
- **`wen new <folder> [--name]` (`wish_control::scaffold`):** `wish.json`, the starting scene (camera, sun, ground — `check` passes with zero warnings), `assets/`, `.mcp.json` (the **full path** of `wen`: the client can start it even if it is not in PATH), the project `CLAUDE.md` (the agent loop and rules), `.gitignore`. It rejects a folder that already has a `wish.json` (`project_exists`), the name is validated. The capability is in the control plane, `wen` only chooses the folder name and the MCP command path.
- **Moved commands:** `wen check` and `wen mv` now call the `check` / `asset_move` tools; the `--json` output is the tool's own response (the same as in MCP). As a result `wen check --json` now carries a `passed` field (`ok` means "the tool ran"). `wen run`, `bench`, `capture`, `describe`, `shaders`, `view`, `watch` will move in WP4/WP5/M8a, while **`wen fmt` and `wen migrate`** deliberately stay on the command line: the engine already writes canonical on every write, `fmt` only fixes manually edited files; `migrate` upgrades the file version and there is a single version in M2. If a need arises in agent evaluations before these enter the catalog as a tool, they are added.
- **Fix (asset walk):** temporary files (`.part`, `.tmp`, `.swp`, `.wish-tmp`, `~`, `.bak`, `#…#`) no longer count as assets and do not trigger `wen watch`/file watching. This was an observed race (the watch test expected one "reloaded" instead of three), because editors that write atomically and the engine's own writes leave temporary files.
- **Tests:** `wish_control/tests/asset_tools.rs` (a new project passes clean and `.mcp.json` is correct; list/info/move + reference update + `file` in the history), parity examples (asset tools included), `wen new` end to end (+ rejection of an existing folder, full path). 334 tests in total.

### WP4 — sessions, log, world inspector

- **Tools (6):** `sim_start` (`scene`, `seed`; session id `s1`, `s2`…, at most 8 open), `sim_step` (`frames` or `until`, `max_frames`; `session` can be left out if a single session is open), `sim_snapshot` (`save` / `list`, `label`; if there is no label `snapN`, not overwritten), `sim_restore` (`snapshot`, `fork`), `sim_stop`, `logs_get` (`session`, `since`, `severity`, `kinds`, `limit`; incremental reading with `last_seq`). `entity_query` and `entity_get` take `session`: they read the live world (they return `tick`, instead of `revision`); entities born in a session get the name `#<id>`, scene entities keep their scene id. **Applying `edit_apply` to the live world in a session was left to M5** (translating `EditOp` to the live world, interaction with game systems is needed); to make something found in a session permanent, the same change is done in the scene with `edit_apply` (the tool description says this).
- **Session setup:** project types (+ the types and **systems** of the example in `wish.json`: `wish_examples::add_systems`; if the resources of bounce do not exist they are added with defaults, the value from the scene takes priority) → scene `instantiate` → the `transform_propagation` system (after all other systems; `GlobalTransform` is up to date every tick, included in the fingerprint of the schedule and in snapshots). The session **never writes** to disk. If the scene file changes during a session, the session is not affected (it has its own copy).
- **Determinism evidence (`tests/sessions.rs`):** the same scene+seed → the same `world_hash`, another seed → different; going back to a snapshot and running again gives a byte-identical hash; a **fork** (`fork`) from a snapshot in a new session reaches the same hash from where the original continued. The parity test also runs the same sequence in process and with `wen do` (separate processes): the hashes are equal across processes too.
- **`until`:** the condition is in the expression language; it stops right after the tick (`stopped_by: until`, `matched`: the first 10 names) when it is true for the resources of the world (`Stats.bounces >= 10`, resources are named like components; `expr` now also checks resource names/fields) or for **any entity**. If it is never true it stops at `max_frames` + an `until_not_reached` warning. A condition error is explained without running any tick (`/until`, column). Evaluating all entities every tick grows linearly with the entity count; in big worlds `max_frames` should be kept small (to be measured in M6, sampling/index if needed).
- **Events:** `wish_ecs::World::events_sent(name)`: the total number sent per type (including those dropped from the buffer); `sim_step` gives the difference of two readings as `events: { Bounced: 17 }`.
- **World inspector (at the end of a step, does not report again):** `transform_not_finite` (error), `entity_out_of_bounds` (|y| > 1000 m, warning); system errors (`system_failed`, the system is turned off, the world continues) go into `diagnostics`. All go into the log (`kind`: session / system / inspector; at most 1000 entries).
- **Tests:** `tests/sessions.rs` (6): determinism and seed, event counting + live query + condition with a resource, `until` (tick 21, `max_frames`, error), snapshot / restore / fork / an ambiguous session / a wrong-id suggestion, inspector + log (`since`, `severity`, `kinds`), limits. Parity: 14 new example calls. 340 tests in total; `wish_ecs` Miri clean.

### WP5 — observation: `capture_frame`, `describe_view`

- **`capture_frame`** (`scene` or `session`, `camera` by scene id, `width`/`height` 16–2048 (default 512×288: token budget), `view`: lit/ids/depth/normals/wireframe, `out`, `shaders`): returns a PNG. Response: `image { mime, width, height, bytes, data (base64) | path }`, `camera`, `view`, `drawn`, `adapter`, `diagnostics` (model could not be loaded, import warnings; once per file). If `out` is given the PNG is written inside the project and `data` is not included; the paths that can be written are limited: project-relative, no `..`/absolute/hidden, **it cannot be a scene/prefab/meta/`wish.json`** (`invalid_output_path`) — an image can never overwrite a document. In MCP (WP6) `data` will also be offered as an image content. `shaders` is for engine development (the same as `wen capture --shaders`).
- **`describe_view`**: the tool of `wen describe`: visible entities **starting from the nearest** (`entity`, `pixels`, `visible_percent`, `bounding_box`, `distance`), `limit` and `visible_not_listed`, `hidden` (left behind or outside the image). Both work **in a scene file or in a live session**; since a session is a copy, the session image does not change even if the scene changes later (in the test).
- **Architecture:** `Workspace` opens the GPU **lazily** (`render.rs::Drawing`): on a machine without a GPU only these two tools give an error. Models are loaded from the project, and reloaded when their file changes (size+time). The software adapter with `Workspace::set_software(true)` / the `WISH_SOFTWARE` environment variable / `--software`. The image is `extract`ed from the world view of the document (or the session world); render writes nowhere.
- **Command line parity:** `wen capture` and `wen describe` now call these tools (entity names: `unknown_entity` + suggestion, `not_a_camera`, `invalid_parameter` — since the view name and the size range come from the parameter table, they replace the old `unknown_view`/`invalid_capture_size` codes). `wen capture --out` can write to any path (a human command); the tool only inside the project. In the parity test the **PNG bytes are the same between in-process and `wen do`/`wen call`** (software adapter). The upper size limit is now 2048 (it was 8192).
- **Tests:** `tests/view_tools.rs` (3, release): PNG header, the same call the same bytes, five views, the `out` file, errors (view, camera, an entity that is not a camera, output path escapes, size); `describe_view` (`hidden` for covered/far, the wall first at 100%, `limit`); observation in a session. Parity: 5 new examples. `base64` unit tests (standard vectors, round trip). 345 tests in total (debug and release green).

### WP6 — MCP server (`wish_mcp`, `wen mcp`), docs, diagnostic catalog

- **`wish_mcp`** (knows only `wish_control`; no asynchronous runtime): `Server::handle(line) -> Option<line>` and `serve(stdin, stdout)`. Methods: `initialize` (version negotiation: 2025-06-18; 2025-03-26 and 2024-11-05 are accepted if asked; ours for an unknown version), `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, `prompts/get`; notifications (`notifications/initialized`, `notifications/cancelled`) have no response. Requests are handled in order. *(Changed in WP8: cancel now really works, see below.)* Errors: parse -32700, batch array / no method -32600 (batch messages do not exist in 2025-06-18), unknown method -32601 (the supported ones in `data`), missing parameter -32602, resource not found -32002 (`data`: `hint`, `candidates`). A panic inside the server becomes -32603, the process stays alive.
- **Tool result:** `structuredContent` = the envelope of the control plane (except image data), `content[0]` = the same JSON as text, if there is an image `content[1]` = `{ type: image, mimeType: image/png, data }` — the base64 is not sent twice. A tool error is **not** a protocol error, it is an `isError: true` result (the agent reads the hint). An unknown tool, in line with 2025-06-18, is a -32602 protocol error, and in `data` there is still `code: unknown_tool`, a suggestion and the valid names (fixed in WP8). `tools/list`: `title`, `description`, `inputSchema` (from the parameter table), `outputSchema` (`ok` required), `annotations` (`readOnlyHint` for the 11 tools that only read).
- **Actor:** `clientInfo.name` (`claude-code` → in history `agent:claude-code`).
- **Docs (`wish_control::docs`)**: `guide()` (the loop, the one-line of each of the 20 tools produced from the catalog, scene format, rules; < 12 KB, in a test), `type_page` (a table from the schema: field, type, unit, range, default, meaning), `diagnostic_page`, `search` (guide sections + type pages + diagnostic pages; all words must match, title match is weighted). Resources: `wish://docs/guide`, `wish://project/status` (the `project_info` response), `wish://docs/components/{name}` (also in the list), `wish://docs/diagnostics/{code}` (template). The **`docs_search`** tool was added: the 20th tool of M4.
- **Diagnostic catalog (`docs::DIAGNOSTICS`, ~150 codes, sorted)** and the **scan test** (`tests/diagnostics_catalog.rs`): in the source, the first string of the calls `Diagnostic::error/warning/info/new(`, `gpu_error(`, `failure(`, `warn(`, `problem(` counts as a code (with a word boundary: `io_failure(` does not count; unit tests excluded) and all must be in the catalog; the catalog is sorted and every entry is one or two sentences. In this WP the test immediately caught two new MCP codes (`unknown_diagnostic`, `unknown_resource_uri`). Known limit: codes produced in other ways (mapping tables, `located(...)`) do not enter the scan; they were added by hand (`json_syntax`, `gltf_parse`, `shader_syntax`, `value_out_of_range` …). The fuzz test in WP8 will also check separately that **every** code returned in responses is in the catalog.
- **`wen mcp [--root] [--software] [--example]`**: stdout is only protocol (the first problem goes to stderr, exit code ≠ 0); the `.mcp.json` of `wen new` starts this.
- **Tests:** `wish_mcp/tests/protocol.rs` (5): handshake and version, actor name in history, the list of 20 tools and their schemas, structured result + image block, resources (the page of every diagnostic code is read) and prompts, broken messages. `wish_cli/tests/parity.rs`: the **MCP leg** (the same example session by JSON-RPC to a `wen mcp` process: `structuredContent` the same as the in-process response, scene files the same) and **the scripted form of the exit criterion**: an empty project (`wen new`) → only with MCP reading the guide, `project_info`, `schema_get`, building three platforms + a ball, `check`, `sim_start`/`sim_step`, `describe_view` (all four visible), `capture_frame` (image block), verification with `entity_query`. 355 tests in total (debug + release).
- **A real connection with Claude Code was not tried:** there is no `claude` command line tool on this machine (the desktop app is used). The connection will need the Claude Code CLI to be installed in WP7 for the evaluation run (user approval); until then the user can try by opening a project created with `wen new` in Claude Code.

### WP7 — agent evaluations

- **Setup (`crates/wish_evals`, `evals/README.md`):** 10 tasks (`tasks.rs`: a prompt in Turkish, setup, verification with the engine's own tools, a reference solution). `wish-evals selftest` needs no model and is free: every check must **fail** on the setup and **pass** after the reference solution (10/10) — there can be no "task that passes by doing nothing". `wish-evals run`: every try is a fresh `wen new` project (MCP on the software adapter), `claude -p` only with the `wish` MCP tools of the project (`--strict-mcp-config`, built-in tools off, `--setting-sources project`), then the check; the transcript (`transcript.jsonl`) and the baseline (`baseline.json`) are kept with the try. `wish-evals recheck`: applies a corrected check to old tries without running the agent again. The record is `evals/results/<label>.json`: passed/failed and why, tool calls, faulty calls (is the first call faulty), turns, tokens, time, API-equivalent cost.
- **Environment (with user approval, 16 November 2026):** Claude Code CLI 2.1.325 was installed (`npm install -g @anthropic-ai/claude-code`); signed in with a claude.ai subscription, the runs spend from the subscription quota. The evaluated model is `claude-sonnet-5-5`, effort `medium` (verified in the transcripts).
- **Results:**

| Run | Passed | Faulty tool calls | Average calls / time | API equivalent |
|---|---|---|---|---|
| First (`2026-11-16-sonnet-5-5-medium`) | **30/30 (100%)** | 12 | ~8.8 / ~18 s | $2.44 |
| After fixes (`…-after-fixes`) | **30/30 (100%)** | 7 | ~8.3 / ~17 s | $2.36 |
| After the prefab description, only 05 (`…-crate-row-recheck`) | 3/3 (with the corrected check) | 1 | 10 / ~21 s | $0.28 |

- **What the evaluations found, and what was fixed (ADR-010: the surface by measurement, not by intuition):**
  1. *There was no tool to look inside a prefab* — in every try the agent tried to read the file with `Read`/`Bash` (a gap in invariant 4). `scene_get`/`entity_get`/`entity_query` now read a `.prefab.json` path read-only; changing it gives `prefab_read_only` + the hint "change the instance" (prefab editing comes with the prefab editor of M8a). Although the capability was added, the agent tried `Read` again in the first run: a capability is not seen unless it is written **in the tool description**; once it was added to the `scene_get` description the `Read` attempts disappeared completely.
  2. *Concepts of other engines* (`Collider`, `Body`, `Name`, `Visibility`): the concept map in `concepts.rs` — if an unknown name is a recognized concept, the hint says how the engine does it ("entities are named by their ids", "physics in M6", "visible ones come from Primitive/Model"); in one place, at envelope level, for every tool.
  3. *`set_override` in a prefab that has only a root*: the hint said "no valid name"; now it says "give the instance's own components / use set" (`wish_scene`).
  4. *Found by the self-test*: integer parameters rejected `640.0`; now a float with a whole value is accepted (unit tested).
  5. *The ambiguity of the task itself*: "2 m between them" can be read in two ways (2 m between centers or a 2 m gap between 1 m crates); the check now accepts both (with a consistent spacing). Lesson: a failed try is first examined for the correctness of the **check**.
- **The remaining faulty calls** are only asking for components that do not exist in the engine (now explained) and a wrong parameter name once; both were fixed with the first hint.
- **M4 exit criterion (Sonnet 5.5 · medium ≥ 80%): met, 100%.** A deliberate limit: the tasks are limited to the capabilities of M4 (scene, observation, session); game code and physics tasks are added in M5/M6. In M7 the Unity/Unreal MCP comparison will be done with this setup.

### WP8 — independent closing review and fixes

- **Review:** an agent that had not written the code (Opus 5.5 · high) read all of M4 and tried the claims by **reproducing** them. 2 high, 7 medium and a series of low findings. All were fixed; the reproduction of every high/medium finding is now a regression test: `wish_control/tests/review_m4.rs` (7) and `wish_mcp/tests/serve.rs` (2).
- **H1 — an image could overwrite a document.** The `out` ban of `capture_frame` looked at the name itself; on Windows `scenes/main.scene.json.` (a trailing dot/space) and `WISH.JSON` (upper/lower case) are the same file. Fix: a **white list** instead of a black list — `out` must end with `.png`; an absolute path, `:`, an empty/`.`/`..`/hidden part, a trailing dot/space and Windows device names (`con`, `nul`, `com1`…) are rejected; the folder is built inside the project and checked again with `canonicalize`. An existing file changes only with the new `overwrite: true` parameter (`output_exists`).
- **H2 — a change that could not be written stayed in memory.** `edit_apply` changed the document first and then wrote; when the write failed, the file and memory drifted apart, and the next successful write also put the "ghost" change on disk. Fix: the change is made on a **copy**, `commit_document` writes, and only if the write succeeds does it put the copy into the document (`edit_history` undo/redo use the same path; the old `mem::take` could leave an empty document in case of a panic).
- **Med1 — a rejected call consumed `changed_by_others`.** Now it is looked at first (`others_since`), and marked only after a successful call (`mark_seen`).
- **Med2 — `history_reset` reset the revision**, an old `base_revision` could be accepted by mistake. `SceneDocument::replace_content`: it records the difference as a single history entry, keeps counting revisions, and the changed entities go into `changed_by_others`.
- **Med3 — after going back to a snapshot the inspector warnings did not come again** (the list of already reported was kept): against the principle of same state + same step = same response. The list is cleared on restore.
- **Med4 — a long `sim_step` could not be canceled and locked the server** (100 thousand ticks ≈ 1.5 s; the upper limit is 1 million). `wen mcp` now reads the input in a **separate thread**; `notifications/cancelled` lowers a flag that belongs to the request running at that moment (or not yet started), `sim_step` looks at every tick boundary and returns the tick count up to then with `stopped_by: "cancelled"` (the world is always whole). Determinism is not affected: cancel does not go *inside* a tick.
- **Med5 — one file, several documents.** `Scenes/Main.scene.json` and `scenes/./main.scene.json` opened the same file as separate documents (separate history). Now open paths are checked against the **exact letter case** on disk (`path_case_mismatch` + the correct spelling) and `.`/`..`/hidden parts are rejected (`.git/…` cannot be written with `create`).
- **Med6 — when the default scene was moved** `wish.json` pointed to the old path and the move entered the history as `file`. `asset_move` now closes the moved open documents, updates `wish.json` (a `default_scene_moved` note) and reconciles the remaining open documents **with the actor of the caller**.
- **Med7 — a race between reading and renaming** (suspected, could not be reproduced): if the person saves exactly in between, their save could be overwritten. `commit_document` reads the disk once more right before the rename; if it changed, it does not write, `file_changed_during_edit` (hint: retry). The window got smaller but is not zero (there is no OS-level lock); it will be handled again with the file watcher in M8a.
- **Low findings:** `serve` reads bytes (invalid UTF-8 is -32700 for that line, the server does not close), a BOM on the first line is dropped (in a PowerShell pipeline `initialize` was lost); an unknown tool is -32602; a panic response keeps the id of the request; descriptions were fixed (`scene_get` is not idempotent — it marks what was seen; `capture_frame` and `sim_stop` `destructiveHint`); the `shaders` folder and `Model` paths cannot read outside the project (`validate_asset_ref`, `model_not_loaded` warning); at most 32 snapshots per session (`too_many_snapshots`); prefab paths are `prefab_read_only` in the changing tools.
- **New diagnostic codes** (in `docs::DIAGNOSTICS`): `default_scene_moved`, `file_changed_during_edit`, `output_exists`, `path_case_mismatch`, `prefab_read_only`, `too_many_snapshots`.
- **Deliberately left (an honest note):**
  - The protocol "golden transcripts" are not separate files, they are line by line assertions in tests (`protocol.rs`, `serve.rs`, the MCP leg of parity). The coverage is the same; the file format will be considered together with the HTTP transport in M8a.
  - The evaluation task set is a little different from the list in §8: the task "notice a change a human made by hand" is not tested with the agent but with a scripted test in `scene_tools.rs`/`review_m4.rs` (to make the agent do a manual edit, the setup would have to change a file in the middle of the run; it will be added with the scenario setup of M5). The prompts say the thresholds clearly.
  - `wen shaders`, `wen view`, `wen watch`, `wen run`/`bench` are not tools yet: they are engine development and window commands; they will enter the catalog with the editor (M8a) and the game modules (M5).
  - The cost of `until` is linear in the entity count (now it can be canceled); it will be measured in M6.
  - The diagnostic scan test cannot see some indirect codes; the fuzz test (`fuzz.rs`) separately checks that every code returned in responses is in the catalog.
  - **An observed flaky test (from M3, not touched in M4):** `wish_render/tests/models.rs::shaders_can_be_reloaded_from_files_and_a_mistake_changes_nothing` failed once in a full workspace debug run with "the sky is not blue"; right after that it passed 3 times alone, 4 times as a crate and 1 time in the full run. The cause was not found (suspicion: GPU timing between reload and draw). If it repeats, the first job is to record the color values in the failed run. To be looked at in the first session of M5.
- **Tests:** total **368** (debug and release green), clippy and fmt clean.

## 15. M4 result

**Completed — 16 November 2026.** All exit criteria were met:

| Exit criterion | Evidence | Result |
|---|---|---|
| From an empty project, build a scene **only with MCP** → run → take an image → verify | (a) `wish_cli/tests/parity.rs::an_agent_builds_runs_looks_at_and_verifies_a_scene_through_mcp_only` (`wen new` + a `wen mcp` process, in CI); (b) 10 tasks with real Claude Code (Sonnet 5.5 · medium), with only MCP tools open | ✔ |
| The command line equivalent of every MCP tool gives the same JSON | `parity.rs`: in process = `wen call` = `wen do` = `wen mcp`, byte for byte (PNG included); a tool without an example breaks the test | ✔ (20/20 tools) |
| The first 10 tasks Sonnet 5.5 · medium ≥ 80% | `evals/results/`: two full runs **30/30** (100%) | ✔ |
| Every invalid call gives a structured error with a hint | `wish_control/tests/fuzz.rs` (broken arguments derived from the catalog, all `ok:false` + `hint` + a code in the catalog, the process alive) + the diagnostic scan test | ✔ |
| (extra) MCP protocol compatibility | `wish_mcp/tests/protocol.rs`, `serve.rs`; a real Claude Code CLI connection (evaluations) | ✔ |
| (extra) Hybrid history | `scene_tools.rs` (agent → file → agent, undo), `review_m4.rs` (Med1, Med2, Med6) | ✔ |

**Numbers:** 20 tools, 4 resource kinds, 2 prompts, ~150 diagnostic codes, 368 tests. **Handed over to M5:** applying `edit_apply` to a live session; widening the evaluation set with game code/test tasks (`script_build`, `test_run`); the agent task "notice a manual edit". **To M6:** the measurement of the `until` cost. **To M8a:** Streamable HTTP, turning `wen view/watch/shaders` into tools, prefab editing, closing the read-write race with a file watcher.
