# Hop

A Wish Engine project. You work on it through the engine's tools (MCP server `wish`, started by
`wen mcp`; the same tools run on the command line as `wen call <tool>`), not by editing files by hand.

## How to work

1. `project_info` first: what is here, and how many problems there are.
2. `schema_get` (with `search`) before writing component values: every field has a type, unit and range.
3. Change scenes with `edit_apply`: many operations in one call, all or nothing. Use `dry_run` to check first.
4. Look before you claim: `capture_frame` for a picture, `describe_view` for what is on screen and how
visible it is, `entity_query` for values.
5. Every error has a `hint`. Read it; it says how to fix the call.

## Rules

- Units: meters, seconds, kilograms, radians in the API (Transform also takes `euler_deg`, in degrees).
- Coordinates: right-handed, +Y up, a camera looks along -Z.
- A person may edit the files or the scene by hand at the same time. `edit_apply` tells you what changed
(`changed_by_others`); never overwrite their work: use `base_revision` when you prepared changes earlier.
- Scene files are canonical JSON written by the engine. If you must edit one as text, run `wen fmt` after.
- `check` verifies the whole project; run it before you finish.

## Game code

- Behaviour lives in `scripts/src/lib.rs`: Rust with `use wish_sdk::prelude::*;`. Declare data with
`#[component]` / `#[resource]` / `#[event]` (every field gets a `///` comment), logic with `#[system]`
functions taking `Query`, `Res`, `ResMut`, `Commands`, `EventReader`, `EventWriter`; list them in
`wish_sdk::export! { components: [...], systems: [...] }` at the end.
- Edit the file, then `script_build`: errors come back with file, line and hint; running sessions switch
to the new code without losing their state. Then `sim_start` / `sim_step` and look.
- All state lives in components and resources (the module starts fresh every tick). No clock, files or
network: time comes from `Res<Time>`, randomness from `wish_sdk::random_f32()` (deterministic).
- Prove it with a scenario test: `tests/<name>.test.json` runs the game from a scene and a seed through steps
  (`{ "input": { "move": [0, 1] }, "frames": 60 }`, `{ "until": "Score.points >= 3", "max_frames": 600 }`,
  `{ "expect": "Score.points >= 3" }`, `{ "capture": { "golden": "tests/golden/win.png" } }`); `test_run`
  says which step failed, at which tick, and what the values were. Run it before you finish.
- Input is actions, not keys: `input.json` maps keys to named actions (`move`, `jump`), and a system takes
`actions: Actions` (`actions.axis2("move")`, `actions.pressed("jump")`). To play the game yourself, give
`sim_step` an `input` such as `{ "move": [0, 1], "jump": true }`: it is held for that many ticks.
