# AI efficiency: fewer turns per task

User request, 18 November 2026, after the measured comparison with Unity
(`evals/results/compare-2026-11-18.md`): Wish Engine passed 8/8 for 0.70 $ and
Unity 6.3 7/8 for 0.80 $. The user asked why the difference is only ten cents.

## Problem

Every turn of an agent re-reads its whole context: the client's own
instructions, the tool definitions it loaded and the conversation. In the
comparison that was about 31 thousand tokens per turn, for both engines alike.
An engine can only change the **number of turns** and what the agent writes.
The transcripts of a Wish run (8 tasks, 41 tool calls) show where turns were
spent that an engine could have saved:

| Task | Calls | Turns that were not needed |
|---|---|---|
| falling_box | 10 | read the 10.8 kB guide (the MCP instructions said to); checked the schema; then `sim_start`, `sim_step`, `entity_get`, `sim_stop`, `check` to see the box come to rest |
| point_light | 6 | `Transform.position` instead of `translation` (error, retry); `scene_get` twice (the default detail shows names only) |
| paint_red | 5 | `entity_query` with one wrong name among the components to show failed the whole call |
| place_cube | 4 | `schema_get` search "mesh" found nothing (`Primitive` was not known by that word) |
| reparent | 4 | read the wheel back to check its world position |

The guide read also stays in the context of every later turn (about 2,700
tokens each time).

## Options

1. Shorter tool descriptions. The 34 tools are about 14 thousand tokens in
   all, but the client loads them on demand (5 to 9 per task), so this saves
   little per turn. Not now.
2. A new one-shot tool (start, run, report, stop). It would need its own row in
   every catalog and duplicates the session tools. Rejected.
3. **Answers that make the next call unnecessary, and instructions that make
   the first call right.** Chosen.

## Decision

- **`edit_apply` answers `now`:** the changed entities as they are after the
  change: the components the change touched (every component of a spawned
  one) and `world_position` when where it is may have changed (Transform,
  parent, spawn). At most 10 entities; despawned ones and dry runs show none.
- **`sim_step` takes `watch`:** up to 20 entity ids whose world position and
  physics velocity are reported at the end of the run. A wrong id is refused
  before anything runs.
- **Components to show are lenient:** an unknown name in `components` of
  `entity_query` or `entity_get` is left out with a short warning (the hint
  names the closest one) instead of failing the call. Names that *filter*
  (`with`) still fail, because the answer would be wrong; so does an
  `entity_get` whose every name is wrong.
- **`schema_get` search knows people's words:** "mesh" finds `Primitive` and
  `Model`, "position" `Transform`, "colour" `Material`, "gravity"
  `RigidBody`, "lamp" `PointLight`, and so on (`SEARCH_WORDS`).
- **The MCP instructions are a short reference card:** coordinates, the common
  components with their field names, that `edit_apply` saves and answers
  `now`, `sim_step` with `watch`. The guide is for game code and the whole
  picture, not a first step.

## Test plan

- `wish_control/tests/efficiency.rs`: `now` for a spawn under a parent, a set,
  a moved parent, a despawn and a dry run; `watch` reports a box at rest, a
  wrong id runs nothing; a wrong component name to show is a warning, to
  filter by an error; searches by people's words.
- Measured again with `tools/compare/run.py run --engine wish` (8 tasks): the
  goal is fewer than 4.5 calls per task on average without losing a task.

## Result (18 November 2026)

Measured with `tools/compare/run.py run --engine wish` before and after, same
model and effort (Sonnet 5.5 medium), 8 tasks:

| | Before | After |
|---|---|---|
| Passed | 8/8 | 8/8 |
| Tool calls | 40 (5.0 per task) | 27 (3.4 per task) |
| Tool errors | 0 and 2 (two runs) | 0 |
| Cost | 0.70 $ and 0.63 $ (two runs) | 0.53 $ |
| Tokens read / written | 1.49 M / 5,344 | 1.11 M / 3,898 |

The goal (fewer than 4.5 calls per task without losing a task) is met. Unity
6.3 on the same tasks: 7/8, 54 calls, 0.80 $ (`evals/results/compare-2026-11-18.md`).
What still costs turns: the client's own tool loading (one call per task), the
falling box (6 calls: starting, stepping and stopping the session are three
separate calls) and the camera task (a look before and a look after). The next round (the game-code loop, one
call to change and prove, a default scene detail that shows values) is after
1.0: release 1.1 of the train ([road-to-2-0.md](road-to-2-0.md) section 3).

