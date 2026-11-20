# Agent evaluations

Tasks an AI agent does in a fresh Wish Engine project **through MCP only**, checked afterwards by the engine's own tools (no person looks at the result). ADR-017 layer 4; design: [docs/design/M4-control-plane.md](../docs/design/M4-control-plane.md) §8.

The tasks, their setup, their checks and a reference solution for each live in [`crates/wish_evals/src/tasks.rs`](../crates/wish_evals/src/tasks.rs). Prompts are written the way a person would ask.

| Task | What the agent is asked | How it is checked |
|---|---|---|
| 01_red_cube | A red cube in the middle, on the ground, visible | colour, bottom at y = 0 ±5 cm, near the centre, ≥ 50 % visible |
| 02_stairs | Three platforms rising like stairs, jumpable | three visible cubes, each ≥ 0.95 m higher and < 4 m from the last |
| 03_sunset | Low orange sun, warm horizon | light colour, sun elevation 0.5–20°, `Sky.horizon` warm |
| 04_frame_statue | Point the camera at the statue | statue not moved, centred within 15 %, ≥ 5 % of the picture, ≥ 90 % visible |
| 05_crate_row | Five prefab instances in a row, 2 m apart | five instances, spacing 1.8–2.2 m along x, on the ground |
| 06_hidden_lamp | Find why the lamp is invisible and fix only that | lamp ≥ 50 % visible, camera/ground/sun unchanged |
| 07_who_falls | Run 120 ticks, note the entity that falls below 0 | `b` has the note "falls", `a` and `c` none |
| 08_behind_wall | Make the target visible without deleting the wall or moving the target | target unchanged and ≥ 60 % visible, wall exists |
| 09_import_model | Place a glTF model on the ground, visible | entity uses the asset, base at y = 0, ≥ 30 % visible |
| 10_night_scene | A second scene with the same camera and ground and a dim bluish moon | file exists, `check` passes, one light ≤ 1000 lux and bluish, camera identical, main scene unchanged |

## Running

```powershell
cargo build --release -p wish_cli -p wish_evals
C:\wish-build\target\release\wish-evals.exe selftest          # free: every check fails before and passes after the reference
C:\wish-build\target\release\wish-evals.exe run --tries 3 --model sonnet --effort medium --label <name>
```

`run` needs the Claude Code CLI (`claude`), signed in; it uses the account's quota (API-equivalent cost is printed and recorded). Each try gets a fresh project under `C:/wish-build/evals/<label>/` with the transcript (`transcript.jsonl`); the agent may only use the `wish` MCP server's tools (`--strict-mcp-config`, built-in tools disallowed). Results go to `evals/results/<label>.json`: pass/fail with the reason, tool calls, failed calls (and whether the very first one failed), turns, tokens, time, cost.

`wish-evals recheck --label <name>` runs the current checks again on a finished run (each try keeps `baseline.json`), without running the agent.

Exit criterion (M4): Sonnet 5.5 · medium passes at least 80 %. First results (16 November 2026): 30/30 twice; see `results/` and the design note §14 WP7.

## Game code tasks (M5)

- `11_walk_code` (make the player walk with the `move` action) and `12_collect_game` (a collecting game: `Coin`, `Score.points`, `GameState.won`). In these tasks the agent **may read and edit project files** (`Read`, `Edit`, `Write`, `Glob`, `Grep`; never `Bash`). It writes the game code, builds it with `script_build`, edits the scene with the tools, and proves the behaviour with its own scenario test.
- Check: the game code builds, `check` passes, the agent's own tests (at least one) pass, and a hidden scenario test the agent **has not seen** (`tests/zz_check.test.json`, deleted after the check) plays the game: walking speed and direction, standing still with no input; collecting the coins, the score, winning.
- For `wish-evals run`, the SDK pack must sit next to `wish-evals.exe`: `wen sdk` (the same `target/release` folder).

## The AI scorecard (M12)

`wish-evals` also measures the seven criteria of the AI scorecard (design: [docs/design/M12-ai-scorecard.md](../docs/design/M12-ai-scorecard.md)): correctness at the first attempt and in the end, with a replay of the result that must hold; errors, above all results the agent said were done (`RESULT: DONE`) that the hidden check refuted; a clean `check` and, optionally, a second agent asked to change the result; a change made by hand in the editor (headless); tokens and cost per passed try; the time to a proven result and the result's tick time at 10,000 entities; consistency across three tries. Task `42_lantern_night` is the long task (a one-paragraph brief to a finished mini game).

```powershell
C:\wish-build\target\release\wish-evals.exe selftest           # free: checks, measures and refutations on the reference solutions
C:\wish-build\target\release\wish-evals.exe estimate           # free: what a baseline campaign costs, from results/
C:\wish-build\target\release\wish-evals.exe campaign --label <name> --models sonnet:medium,haiku:medium --tries 3   # spends quota: ask first
```

A campaign writes `results/<name>.json` and `results/<name>.md` (the card with its inputs and the command that reproduces it). The held-out set lives outside the repository (`--held-out`); its runs print and keep numbers only.
