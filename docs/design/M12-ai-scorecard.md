# M12 package 10: the AI scorecard

**Status:** part a (the card as code, without running it) done on 20 November
2026, uncommitted at the time of writing. Part b, the first baseline campaign,
spends model quota and waits for the user's approval of the estimate in §6
(the plan's §7, decision 6: a spending cap).

The plan: [M12-road-to-1-0.md](M12-road-to-1-0.md), "The AI scorecard" and
§0 item 10. The rule it serves: [ADR-027](../DECISIONS.md), rule 4 (a
published number has a command that reproduces it and a recorded result with
its inputs).

## 1. Problem

`wish-evals` measured one thing well: whether a task passed, with calls,
errors, tokens and cost beside it. The card asks for seven criteria, in an
order, for two models, with a long task and a held-out set. Most of the
criteria are about the *result* (does a replay hold, can a person change it,
how fast does it run) or about what the agent *believed* (did it say it was
done when it was not), and neither was read before.

## 2. The card, criterion by criterion

Everything below is measured per task and per try, recorded in the try's row,
and summed up per model by `card.rs`. A card is a pure function of the rows:
the same rows give the same card (`wish-evals card --from` makes the Markdown
again from the JSON).

### 1. Correctness

- **Passed at the first attempt:** the hidden check, run the first time the
  agent stops. The agent stops once per `claude -p` run, so "the first time
  it says it is done" is the end of its first run.
- **Passed in the end:** after at most `--repairs` repair rounds (default 1).
  A repair round resumes the same session (`--resume`) with the check's
  reason ("A check of the result failed: ..."), the way a person who tried
  the result would report it. The rounds are counted (criterion 2), and their
  time and tokens belong to the try. Because the reason comes from the hidden
  check, "passed in the end", "refuted in the end", cost and tokens per pass
  and the time to a proven result are **partly helped by the harness**; the
  first-attempt rows are not. The card says so in its rows and in its "How
  this card was made" block.
- **Hidden tests with edge cases:** the existing checks, plus the long task's
  three hidden tests (§3).
- **A replay of the result holds** (`measures::replay_holds`), for game-code
  tasks that passed: on a copy of the result, a session of the default scene
  is recorded for 240 ticks with every action of `input.json` pressed in a
  fixed pattern (nothing; all on, two-axis actions right; forward; nothing),
  saved, and played back in a fresh workspace; it holds when every tick's
  world hash matches. Limit: the playback runs in the same process as the
  recording (a new `Workspace`); that hashes match across processes and
  machines is the engine's own test (`wish_cli/tests/parity.rs`, CI).

### 2. Errors

Read from the Claude Code transcript (`transcript.rs`, pure, unit-tested on
synthetic transcripts; `wish-evals rescore` reads a finished run again for
free):

- **Tool errors:** a result with `is_error`, or an engine answer `ok: false`.
- **Compile failures and errors:** `script_build` answers with
  `passed: false`, and their `errors` count.
- **Failed test runs:** `test_run` answers with `passed: false`.
- **Repair turns:** failures (any of the three above) after which the agent
  made another call. Plus the harness's repair rounds (criterion 1).
- **Believed right, refuted by the hidden test (false successes).** Every
  prompt ends with one fixed sentence (`transcript::CLAIM_SUFFIX`) asking the
  agent to end its last message with `RESULT: DONE` or `RESULT: NOT DONE`
  and what is missing. The claim is the **last** line that starts with
  `RESULT:` (case, spaces, backticks, asterisks, a leading `#`/`>` and list
  markers `-`, `*`, `•`, `1.` ignored): exactly `DONE` or `DONE.` is a claim
  of success; a word starting with `NOT` is not; anything else (`DONE,
  mostly`) is unstated. A last message without such a line is `unstated` and
  **counts as a claim of success** (an agent that stops without saying
  something is missing is read by a person as done); the card shows the
  false successes among stated claims and among unstated ones separately. No last message (an error, a
  time-out) is no claim. A false success is a claim of success whose check
  failed, counted at the first attempt and in the end, as a share of the
  claims. Deterministic: the same transcript always gives the same claim.
  Limit: the sentence changes the prompts, so these runs are not comparable
  with the runs of M4 to M7 on that point.

### 3. Quality

- **A clean `check`:** `check` on the result, errors and warnings recorded;
  clean is neither, reported as a share of the passed tries. A `check` that
  could not run is recorded as such and is not clean.
- **A second agent changes it correctly (optional, `--second-agent`, off by
  default, spends quota):** after a try passed, a fresh session that never
  saw the first one is asked for a change; a small hidden check judges it;
  its pass, cost, tokens, calls and time are recorded with the try.
  Follow-ups exist for seven tasks: `01_red_cube` (taller and blue),
  `02_stairs` (a fourth step), `11_walk_code` (6 m/s), `12_collect_game`
  (coins worth 2), `27_moving_platform` (a 4 m lift), `33_vertical_menu`
  (an Options button in the middle) and the long task (a fifth lantern).
  The game-code follow-ups also require the project's own tests to still
  pass. Limit: seven of 42; the rest have none yet.

### 4. People

`people::edit_by_hand`, on a copy of every passed result, with the editor's
headless machinery (`EditorApp`, `script::run_script`, widgets found by name
in the accessibility tree, as `wish_editor/tests/inspector.rs` does): the
project opens in the editor; an entity of the default scene is selected (by
id within each step: the first the agent made that has a `Transform`; else
the first it changed that is not a camera or a light; else the first it
changed; else the first placed entity that is not a camera or a light); the
x box of its
`translation` is clicked, a value 0.5 higher is typed and Enter pressed. It
passes when the value is in the scene, the change entered the history as one
operation whose `actor` is exactly `human`, and `check` reports no more
errors than before. The card's row says what it proves: the result opens in
the editor and takes an inspector edit by hand.
Deterministic; no model. Limits: one kind of change (a number in the
inspector) on the default scene; a task whose work is only in another scene
or only in the interface (no `Transform`) is judged on another entity of the
default scene. People in the user tests are the other half (release 1.4).

### 5. Efficiency

Tokens (input, output, cache read, cache write) and the client's
API-equivalent cost, summed over every try **including the failed ones and
the repair rounds**, divided by the number of passed tries. A run cut off
before the client's closing report (a time-out) has no reported cost: its
tokens are read from the assistant messages (each message once) and its cost
is estimated at list price (Haiku's for a Haiku alias, Sonnet's otherwise)
and marked `cost_estimated`; the card counts such tries. The model ids the
client reports (`modelUsage`) are recorded with each try and in the inputs,
beside the alias.

### 6. Speed

- **Time to a proven result:** the harness's clock (`Instant`) around each
  agent run; the time of a try that passed is the sum of its runs up to the
  one after which the check passed (repairs included; the agent's builds and
  its own tests are inside its runs). The checks themselves are not counted.
  No tool answer carries a time (ADR-027, rule 2). Median and 90th percentile
  of the passed tries.
- **How fast the result runs** (`measures::speed`), for game-code tasks that
  passed: on a copy, the default scene is grown to 10,000 entities by copying
  the entities that carry one of the game module's components (on a grid 40 m
  apart), then 30 ticks are timed after 5 of warm-up by the harness's clock
  around one `sim_step`, the way `wen bench --script` times game code; the
  number is milliseconds per whole tick (game code and engine systems).
  **Not meaningful**, and recorded as such with the reason, when there is no
  game code, no top-level entity carries a game component, or those entities
  are physics bodies (the number would be the physics engine's). Limits: the
  harness runs one try at a time, but the machine is not otherwise idle; a
  game whose cost grows with pairs of entities (the long task's lantern
  check) is measured with that cost, which is the point. A measurement that
  fails (a trap at 10,000 entities) is recorded as `tick_error` and counted
  on the card. The median pools tasks of different kinds; the per-try values
  are in the rows.

### 7. Consistency

Three tries per task (`--tries 3`, the default). Per model: tasks passed in
every try, in some, in none; the median over tasks of the coefficient of
variation of cost and of time across the tries.

### The gap between the models

For the first two models of a campaign (by default Sonnet 5.5 · medium, then
Haiku 5.5 · medium): rates as percentage points (first minus second), costs
and times as the second's over the first's.

## 3. The sets

- **The open set:** the 41 tasks of M4 to M7 and the long task, all in code
  (`tasks.rs`, `m6_tasks.rs`, `m7_tasks.rs`, `long_task.rs`).
- **The long task** `42_lantern_night` (`long_task.rs`). The brief is one
  paragraph: a player walking at 5 m/s inside a 16 m square, four lanterns
  to light (each counts once, turns warm yellow: red and green clearly above
  blue), a guard patrolling a line
  at 3 m/s that sends the player back and takes a life, lost at 0 lives (the
  player freezes), won when all are lit, a HUD text in an exact format
  (`Lanterns <lit>/<all>, lives <lives>`), a camera that shows the square,
  the agent's own tests that the game can be won (`GameState.won`) and lost
  (`GameState.lost`), and a replay of a win named `win`. The project starts as `wen new` makes it: the agent makes every
  entity. Hidden: an own test that names `GameState.won` and one that names
  `GameState.lost`; the six actors visible; three scenario tests (the win route
  with the HUD, standing still and the double count; the walls of the
  square; three catches, the reset, the frozen player, the HUD at 0 lives);
  the replay `win` plays back and ends won. The reference solution builds the
  game, places the scene, writes both own tests and records the replay with
  the tools. Six mutants must be refuted by the hidden checks (self-test): a
  lit lantern counts again; no walls (the player walks out of the square);
  the player still moves after losing; the guard turns at x = ±5; no replay
  named `win`; a `win` replay of a game that is not won. The lit colour is
  tested as red > 0.6, green > 0.4 and blue at least 0.2 below both. Its
  follow-up adds a fifth lantern. The time limit of its agent
  run is 40 minutes (15 for the others).
- **The held-out set** (`held_out.rs`), the mechanism only; the tasks are
  written in release 1.1 by someone who then does not improve the engine
  against them:
  - **Where:** a folder outside the repository, `C:/wish-build/held-out` by
    default or `--held-out <folder>`. A folder inside the repository is
    refused (the repository is public, and agents improving the engine
    search it). The repository is found from `CARGO_MANIFEST_DIR`, fixed at
    compile time: a `wish-evals` that was moved, or whose checkout moved,
    cannot find it, and then the guard refuses every folder rather than
    passing one.
  - **Format:** one `*.task.json` per task, data only: `id`, `prompt`,
    `files`, `setup` (`ops` for `edit_apply`, `files`), `check` (`game`,
    `check_passes`, `visible`, hidden `tests` in the scenario test format),
    `reference` (`files`, `build`, `ops`) and an optional `follow_up`. The
    open example [`src/example.task.json`](../../crates/wish_evals/src/example.task.json)
    shows it and runs in every self-test.
  - **Guard:** a held-out run prints a progress count and the card's numbers
    only; each try's project and transcript are deleted as soon as its
    numbers are read; rows keep no reason and no task id, and the written
    scorecard has no rows at all. `selftest --held-out` prints counts only;
    an unreadable task file is reported as a count. `selftest --task-file`
    gives details for one file, for the person writing a task, and refuses
    (without printing its id or reason) a file anywhere under the default
    held-out folder or a folder given with `--held-out` (both sides
    canonicalized, so `..` and links do not get around it).
  - **The client's own copy.** Claude Code keeps every session's transcript
    in the person's profile (`~/.claude/projects/<folder key>/<session>.jsonl`,
    or under `CLAUDE_CONFIG_DIR`), and that holds the task's prompt and the
    repair message with the hidden reason. A throwaway `CLAUDE_CONFIG_DIR`
    would keep it out, but the sign-in lives in the same folder, so a run
    from a fresh one is not signed in (not tried: it would start the
    client). Instead, after each held-out try the harness removes what the
    client kept of it (`held_out::forget_sessions`): the project folder named
    after the try's folder (every character that is not a letter or digit
    becomes `-`; the try's folder is used for nothing else), every entry one
    level below the client's folder named after one of the try's sessions
    (todos, file history, debug logs), and the lines of `history.jsonl` that
    name the try's folder or a session. Sign-in and settings are not touched;
    a unit test proves it on a made-up client folder. Limit: a client that
    keeps something new in another place would escape this; the list is
    checked again when the client is updated.
  - **The working rule** (CLAUDE.md, "The AI scorecard judges every AI
    package"): an improving agent never reads that folder or runs it except
    as a campaign. A studied task moves to the open set and is replaced.

## 4. The report

`wish-evals campaign --label <name>` writes `evals/results/<name>.json` and
`.md`. The card is in its order (1 to 7), one column per model and one for
the gap. The inputs: the label, the set, the number of tasks, tries, repair
rounds, whether the second agent ran, the engine version and commit and
whether the tree was dirty, the date (UTC), the machine (operating system,
architecture, threads, processor; no user or host name), the client's
version, the models with their effort and the model ids the client
resolved them to, and the exact command. A "How this card was made" block
follows them: the claim sentence appended to every prompt (so the error rows
are not comparable with the M4 to M7 runs), that repair rounds feed the
hidden check's reason back (which rows that helps), whether the second agent
ran, the order the tries ran in, and the cost cap with whether the campaign
stopped at it (`inputs.method` holds the same). The open set's JSON keeps
every try's row; the held-out one keeps none.

A try's transcripts (one run per file: `run-1.jsonl`, `run-2.jsonl` for a
repair round, `second.jsonl`) and its `baseline.json` are written beside its
project (`<try>.logs/`), never in it: the second agent works in the project
folder and must not read the first agent's transcript or the baseline.
`rescore` reads one run per file. A `--label` is a plain name (letters,
digits, `-`, `_`, `.`; no `..`, no folders), and an existing result is not
overwritten without `--overwrite`.

**Cost control.** `--max-cost <dollars>` stops the campaign between tries
when the cost so far plus the mean cost of a try so far would pass the cap;
the card says it stopped and after how many tries, and keeps what was
measured. The long task's first try with the first model runs first: it is
the largest unknown of the estimate, so the first measured long try shows
early whether the cap holds.

## 5. The self-test

`wish-evals selftest` (free) now does, for every task of the open set and the
format example: the check fails on the prepared project and passes after the
reference; on the reference result, `check` has no error (warnings counted),
a replay holds (game tasks), the speed at 10,000 entities is measured where
meaningful, the editor changes it by hand; the follow-up's check fails on the
reference and passes after the follow-up's reference. Then ten deliberately
wrong results must be refuted: the long task's six mutants (§3); a replay
whose recorded hash was changed; a project with a broken
scene file in the editor; a broken `input.json` in `check`; a transcript that
says `RESULT: DONE` for a failed try, which must be a false success on the
card. The held-out guard (a folder in the repository) is checked too.
Unit tests (`cargo test -p wish_evals`): transcript reading and claims
(list markers, `DONE.`, several runs in one file, a run without a result),
the card's numbers and order, the "How this card was made" block, a check
that could not run, the gap, the held-out format, the guards (a folder in the
repository; a task file at any depth under a held-out folder), forgetting a
held-out try's client sessions, the editor's choice of entity.

First result on 20 November 2026 (before the review; release build, this
machine, 238 s): tasks
43/43 (the 42 of the open set and the format example), follow-ups 8/8,
replays hold 11/11, edited by hand in the editor 43/43, clean `check` 43/43,
refutations 5/5, the held-out guard refuses a folder in the repository. Tick
at 10,000 entities of the reference solutions: `11_walk_code` 3.4 ms,
`12_collect_game` 4.3, `26_follow_camera` 4.2, `42_lantern_night` 12.4,
`22_click_counter` 44.8 and `39_seconds_label` 38.7 (over the 16.7 ms frame
budget: copies of an interface text, and the engine's interface work, grow
with them); the other game tasks use physics bodies and are not meaningful.
The first full run found one flaw in the editor measure (it looked for the
inspector at a fixed place, and the panel grows wider for some components);
it now reads the panel's left edge from its heading.

After the review (§9; release build, 347 s while another build ran on the
machine): tasks 43/43, follow-ups 8/8, replays hold 11/11, edited by hand
43/43, clean `check` 43/43, **refutations 10/10** (the six mutants of the
long task, the changed replay, the broken scene, the broken `input.json`,
the false success on the card). The ticks at 10,000 entities were 4.2, 8.1,
83.4, 4.3, 55.3 and 12.2 ms for the same six tasks: the machine was busy, so
the numbers are not stable under load; a campaign should run on an
otherwise idle machine.

`wish-evals rescore` over the 41 transcripts of the M7 run (free): 473
calls, 14 tool errors, 1 failed build, 6 failed test runs, 21 repair turns;
every claim `unstated` (those prompts had no claim sentence).

## 6. What one baseline campaign costs (estimate, offline)

`wish-evals estimate` (free) reads every Sonnet try recorded in
`evals/results/` and prices a campaign: the 41 open tasks and the long task,
three tries each, one repair round, Sonnet 5.5 · medium and Haiku 5.5 ·
medium, with and without the second-agent step.

Basis: 104 Sonnet 5.5 · medium tries recorded in `evals/results/`
(`2026-11-16-sonnet-5-5-medium.json` 30, `...-after-fixes.json` 30,
`...-crate-row-recheck.json` 3, `m7-sonnet-5-5-medium.json` 41); every open
task has at least one. One try of each of the 41 open tasks costs $4.55 as
the client recorded it ($3.54 at today's list price), 11.1 M tokens (almost
all cache reads; 0.1 M output), 0.3 h of agent time, 12.5 turns a task. A
game-code task averages $0.17, 0.4 M tokens and 42 s; the long task is taken
as three to six of them ($0.51 to $1.03 a try).

| Campaign part (3 tries per task) | Tokens | $ (recorded prices) | $ (list price) | Agent time |
|---|---|---|---|---|
| Sonnet 5.5 · medium, 41 open + long task | 37.1 M – 43.0 M | $15.19 – $17.56 | $11.83 – $13.70 | 1.0 h – 1.2 h |
| Haiku 5.5 · medium, 41 open + long task | 40.8 M – 98.2 M | $0.84 – $2.02 | $0.84 – $2.02 | 0.6 h – 1.4 h |
| Second-agent step, Sonnet (7 follow-ups) | 5.5 M – 13.8 M | $2.34 – $5.76 | $1.83 – $4.53 | 0.2 h – 0.4 h |
| Second-agent step, Haiku (7 follow-ups) | 5.5 M – 27.6 M | $0.12 – $0.58 | $0.12 – $0.58 | 0.1 h – 0.5 h |

- **The baseline, both models, three tries, one repair round: about $16 to
  $20** (78 M to 141 M tokens), **2.4 h to 3.3 h** of wall time, one try at
  a time, the harness's checks and measures included.
- **With the second-agent step: about $18.50 to $26** (89 M to 183 M
  tokens), **2.6 h to 4.2 h**.
- A cap that covers both with room for a rerun of a few tries: **$30**
  (`--max-cost 30`).
- **The long task is the dominant uncertainty.** It has never been run; it
  is taken as three to six game-code tasks a try ($1.50 to $3.10 for its
  three Sonnet tries). If it cost ten times a game-code task and needed its
  repair round every time, Sonnet's part would grow by about $5, still under
  the cap. The campaign therefore runs the long task's first Sonnet try
  first: its measured cost tells, after one try, whether the range holds.

Assumptions, all in `estimate.rs`: list prices of 3 November 2026 (Sonnet
5.5 $2 / $10 per million input / output tokens, cache reads $0.10; Haiku 5.5
$0.10 / $0.50 up to 100K-token prompts, which every turn here is), cache
writes at 1.25 times the input price and Haiku's cache reads at a tenth of it
(not published; assumed); the long task costs three to six game-code tasks;
Haiku uses one to two times Sonnet's tokens and 0.6 to 1.2 times its time;
first attempts fail 0 to 10 % (Sonnet, from 71 of 71 recorded passes) and
20 to 40 % (Haiku, assumed), and a repair round costs half a try; the
harness's own time is about 20 s per game try and 8 s per scene try. The
"recorded" column is what the client reported for the recorded runs, at the
prices it used then; the list-price column recomputes those tokens at
today's list price. Haiku's cost is small next to Sonnet's at these prices,
so the range is decided by Sonnet's.

## 7. Commands

- Free: `wish-evals selftest`, `selftest --held-out [dir]`,
  `selftest --task-file <file>`, `card --from <file>`,
  `rescore --label <name>`, `recheck --label <name>`, `estimate`;
  `cargo test -p wish_evals`.
- **Spends quota, needs the user's approval:** the baseline,

  ```
  cargo build --release -p wish_cli -p wish_evals
  C:\wish-build\target\release\wen.exe sdk
  C:\wish-build\target\release\wish-evals.exe campaign --label baseline-1.0 --models sonnet:medium,haiku:medium --tries 3 --repairs 1 --max-cost 30
  ```

  and `--second-agent` for the step of criterion 3. The held-out card is the
  same command with `--held-out` once the set exists (release 1.1).
  `wish-evals run` is now a campaign of one model.

## 8. What is not built, or not verified

- **Nothing was run against a model.** The transcript reader is tested on
  synthetic transcripts and was run over the 41 recorded transcripts of M7
  (`rescore`, §5); the repair round (`--resume` with feedback), the claim
  sentence and the second agent have not been seen in a real run.
- The held-out tasks (release 1.1, by design).
- Follow-ups for 35 of the 42 tasks.
- The people measure is one inspector edit on the default scene; renaming,
  adding, moving with the gizmo and other scenes are not covered.
- The replay holds within one process; across processes it is the engine's
  own test.
- Haiku has no recorded run: its cost and time in §6 are assumptions.
- The editor measure and the speed measure add time to every passed try;
  measured in the self-test, not in a campaign.

## 9. Review (20 November 2026)

One independent reader (read only) found no high defect and seven medium
ones; all were fixed the same day, with the low ones.

| # | Finding | Done |
|---|---|---|
| M1 | `selftest --task-file` refused only a file directly in the default held-out folder, ignored `--held-out` and subfolders, and printed the id and reason. | `held_out::in_held_out`: canonical ancestors, the default folder and any `--held-out` value; refused without details. `repository()` is now `Option`, and an unresolved repository makes the in-repository guard refuse. Unit test with a nested folder and a `..` path. |
| M2 | Claude Code keeps each session's transcript in the person's profile, holding the held-out prompt and the hidden reason. | `held_out::forget_sessions` after each held-out try (§3); a throwaway `CLAUDE_CONFIG_DIR` was not used because the sign-in lives there. Unit test on a made-up client folder (sign-in and other projects kept). |
| M3 | The card did not say how it was made. | "How this card was made" block and `inputs.method`: the claim sentence, repair feedback and the rows it helps (also marked "harness-assisted" in the rows), the second agent, the order, the cap. |
| M4 | The second agent ran in the folder holding the first agent's transcript and the baseline; a file of several runs undercounted. | Transcripts (one run per file) and the baseline go to `<try>.logs/` beside the project; `transcript::read` sums every `result` event; `rescore` reads one run per file. |
| M5 | The long task's hidden tests demanded unstated details. | The colour test is red > 0.6, green > 0.4, blue at least 0.2 below both, and the brief says "red and green clearly above blue"; the HUD's exact format is in the brief; the agent's own tests must name both `GameState.won` and `GameState.lost`. |
| M6 | The long task had one mutant. | Six (§3), each refuted in the self-test. |
| M7 | No cost control; a timed-out run recorded $0. | `--max-cost`; the long task's first try runs first; a run without the client's report has its tokens read from its messages and its cost estimated and marked, counted on the card. §6 names the long task as the dominant uncertainty. |
| L | `people.rs` checked the actor with `contains("human")`. | Compares `actor == "human"`; the card row says "opens in the editor and takes an inspector edit by hand". |
| L | Claims: list markers; `DONE` prefixes; stated and unstated mixed. | Markers stripped; only `DONE` or `DONE.`; false successes split into stated and unstated. |
| L | A `check` that failed to run counted as clean; a failed 10,000-entity measurement was dropped. | Recorded as not clean (`check_could_not_run`) and as `tick_error` (`tick_failed` on the card). |
| L | Only the model alias was recorded. | The `modelUsage` ids per try and per model in the inputs. |
| L | `--label` could be a path; results were overwritten. | Plain names only; `--overwrite` needed for an existing result. |
| Note | A large tool result that is not JSON reads as Null (so it counts as neither an error nor a failed build). | Kept; documented here. |
| Note | The tick median pools tasks of different kinds. | Kept; the per-try values are in the rows (§2, 6). |
| Note | The `wish_editor` dependency makes `wish-evals` heavier. | Kept: the people measure needs the editor's own machinery. |

The estimate (§6) did not change: no assumption it uses changed. The cap
stays $30.
