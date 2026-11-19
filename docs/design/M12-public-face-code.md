# M12 package 11a: the code follow-ups of ADR-027 and a link test

*(19 November 2026. The code part of item 11 of [M12-road-to-1-0.md](M12-road-to-1-0.md) §0, "the public face made true". The README pictures, the guides' pictures and the short stability note are package 11b.)*

## 1. Problem

ADR-027 ([DECISIONS.md](../DECISIONS.md)) set four rules on 18 November 2026 and listed what the code still broke:

- **Rule 2** (tool answers carry no wall-clock numbers): `script_build` answered `seconds`, and `project_info` repeated it as `scripts.last_build.seconds`. Nothing proved there was no third.
- **Rule 3** (versions across 0.x): `engines::pick` took the exact version, else `^wanted`; below 1.0 the caret means the same minor only, so a project of 0.2.0 would open in 0.3.0 with `"fit": "none"` and the note "no engine compatible with it is installed ... Its files may change", even with no format change between them. `wen migrate` did not set `engine` in `wish.json`; `check` said nothing about an `engine` that is not a version; §15 of the M10 note and the guides described the caret.
- **Rule 4** (every public claim has a command): a link check is one of the claims CI should run for free, and there was none.

## 2. Rule 2: no wall-clock numbers in answers

**Decision.** `BuildReport` loses its `seconds` field (not only the key): nothing in the control plane can put it back into an answer by accident. `wen build` measures around the call and prints "Built in X s" as before (`wish_cli/src/main.rs`; `tool_cmds::build_text` takes the measured time). The editor never showed the build time (its status line says "game code built (N warning(s))"), so there was nothing to keep there; adding a time would put a clock into its golden images. The two `wish_script` tests that printed or bounded the time measure it themselves.

**The search for others.** Every `Instant`, `SystemTime`, `elapsed`, `as_secs`, `UNIX_EPOCH` and modification time in `wish_control`, `wish_mcp`, `wish_script`, `wish_asset`, `wish_scene`, `wish_render` and the crates below them was read, with every answer key that names a time (`time`, `elapsed`, `duration`, `_ms`, `seconds`, `date`, `stamp`, `modified`). None reaches an answer, a resource or a diagnostic:

- modification times are cache stamps (audio and interface sizes, models, pictures, prefabs: `audio.rs`, `ui.rs`, `render.rs`, `documents.rs`; `wish_asset::hot`) and the check that an exported `game.wasm` is newer than its sources (`tools/export.rs`): they decide, they are not reported;
- the crash report names its file with the time (`crash.rs`): a local file, never an answer;
- `wish_mcp::http` uses the clock for deadlines and a token's randomness: transport, not answers;
- `script_build_timeout` says "stopped after 120 seconds": the configured limit, a constant, the same in every process;
- `seconds` in `asset_create`'s `sound` and in clip lengths is time inside the game (rule 2's last bullet).

**The guard** (`wish_cli/tests/parity.rs`):

- `every_tool_answers_the_same_bytes_in_two_processes`: a table with one row per tool of the catalogue (`two_process_calls`) calls every tool for real through `wen do` on a fresh `wen new` project with the SDK pack and compiler: an edit, assets made, read and moved, three sessions with a snapshot, a fork and a recorded, saved and played replay, pictures, the game code built and its test run, a checkpoint saved and listed, a design decision, and at the end the activity feed, the history and `project_info`. Then the same calls run in a second process on the same project made afresh, and the answers are compared byte for byte. A catalogue tool without a row fails the test, and so does a row whose tool never answers `ok`. `capture_frame` and `describe_view` draw on the software adapter. `project_export` is called as a dry run (a real export copies the player program, tens of MB, twice; its dry run lists the same files). *(Review finding 1: the first version called every tool with `{}`, and many only answered "missing parameter".)*
- `every_resource_reads_the_same_in_two_processes`: `tools/list`, `resources/list`, `prompts/list` and every listed resource, through two `wen mcp` processes.
- The existing parity tests already compared the curated examples in process against `wen do`, `wen call` and MCP (a second process each); they stay.

## 3. Rule 3: versions across 0.x, and the value of `opens_from`

**Decision.**

- `engines::OPENS_FROM` (a `Version`, re-exported beside `SCENE_FORMAT` as `wish_control::OPENS_FROM`): the oldest engine version whose projects this engine opens unchanged. `wen package` writes it into `wish-engine.json` as the optional key `opens_from` (the file stays format 1; an older engine ignores the key, since `read_engine` does not reject unknown keys). A key that is not a version, or names one newer than the engine's own, makes the folder not an engine.
- `engines::pick`: the exact version, else the newest engine for which `opens(engine, wanted)` holds: `opens_from ≤ wanted ≤ version`. An engine without `opens_from` (packaged before the key) is judged by the caret rule, only for a project older than 0.1.0 (`engines::UNSTATED_BEFORE`, which never moves): a 0.1.0 package from before the look lock calls itself 0.1.0 too and cannot read format 2, so it is not taken even as the exact version (review finding 2). `VersionReq` is unchanged: a package's `engine` requirement keeps Cargo's meaning.
- The note (control plane `Choice::note`, editor `welcome::engine_note`, 11 languages) for an older project nobody opens unchanged now reads: "This project was made with Wish Engine {project}, and a file format has changed since: {here} opens it, and its files will change when they are saved. Install {project} to keep them as they are." It is shown only when the project is older than the running engine's `opens_from`, which moves only at a format window, so it is true every time it is shown.
- The `engines` tool answers each engine's `opens_from` (`null` for an old package).
- `wen migrate` (`wish_control::migrate::migrate_project`) sets `engine` in `wish.json` to the migrating engine's version when it migrated at least one file and the key named an older version, none, or something that is not a version (the key is only ever raised: review finding 3); it answers `engine` with the version written. Opening, checking and editing never touch the key. A `wish.json` that cannot be read is the warning `migration_engine_not_set`.
- `check` reports an `engine` that is not a version (or not a string) as the warning `invalid_version`, at `wish.json` `/engine` with its line, with a hint naming this engine's version. A warning, not an error: the project opens and plays; only the choice of engine cannot use the key.

**What "opens unchanged" means after the look lock.** An engine opens a project unchanged when it reads every file of it with the meaning the project's own engine gave it, writes nothing on opening, and no format window lies between the two versions, so that saving keeps the files readable by the project's engine. The look lock (M12 package 9) was a format window: scene and prefab files went from format 1 to format 2. The engine reads format 1 with its old meaning (in memory, to the byte), but the next change saved writes format 2, which the engines before the switch refuse. By the definition, `opens_from` is therefore the first version that writes format 2.

**The value: `0.1.0`.** The workspace version has been 0.1.0 since M1 and has never been tagged or released; the builds before the switch and the builds after it both call themselves 0.1.0, and every project in existence names 0.1.0. The first version that writes format 2 is 0.1.0, so `opens_from` is 0.1.0, and the 0.x tags of the beta (0.2.0, ...) keep it until the next format window, the first package of release 1.2, which moves it to 1.2.0. The one case this cannot tell apart, a project made by a development build from before the switch and opened by a later engine, is opened as compatible without a note; its files are read with their meaning and `check` names them `old_format`, and the build that could have been hurt was never released.

**The guard.** `wish_control/tests/engines.rs` `opens_from_moves_with_the_formats` records rows of (`opens_from`, the format constants: scene, project, material, meta, package manifest, game ABI). The last row must equal the constants of the build and `OPENS_FROM`; consecutive rows must differ in their formats and have a strictly newer `opens_from`. A changed format constant fails the test until a row is added, and a row cannot be added without moving `opens_from`. Formats that are still literals in their readers (`input.json`, replays, scenario tests, `design/game.json`) are not covered; they become constants when one of them first changes.

## 4. The link test

`wish_control/tests/doc_links.rs` reads every Markdown file of the repository outside hidden, `target`, `exports` and `node_modules` folders (the root's, `docs/`, the crates' recipes and guides, the examples', the evaluations' and the tools'), outside fenced code blocks and inline code: inline links and images, reference definitions, and `href`/`src` of HTML tags. A relative target must exist inside the repository with the letter case it has on disk (GitHub and the Linux CI are case-sensitive; Windows is not), and an `#anchor` into a Markdown file must be one of its headings by GitHub's rules (text as shown, lower case, letters, numbers, `-` and `_` kept, each space a `-`, repeats numbered `-1`, `-2`) or an explicit `<a id>`. `http(s)` links are printed with `--nocapture` and not fetched; other schemes (`wish://`, `mailto:`) are not files. A second test pins the anchor rules on examples taken from the repository (an em dash, a Turkish heading, code in a heading, repeats). It runs in CI with `cargo test --workspace`.

## 5. Result

Verified on 19 to 20 November 2026 on the development machine (Windows, WARP), at four cores and low priority:

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test -p wish_script -p wish_control -p wish_mcp -p wish_editor --no-fail-fast`: 389 passed, 0 failed, 6 ignored (the ignored ones are the hand-run measurements and the real-model run, as before).
- `cargo test -p wish_cli`: 50 passed, 0 failed, 1 ignored; among them the two two-process tests (every tool and every resource, game code built in both processes) and the two `wen migrate`/`check` tests of rule 3.
- `wish_control/tests/engines.rs` (7 tests) and `doc_links.rs` (2): 57 Markdown files, 221 relative links, 7 external links listed; **no broken link was found**, so none was fixed. A probe file with a wrong letter case, a missing folder and two missing anchors was reported on all four and removed.
- `wish-evals selftest`: 41/41.

The full workspace gate is run once before the commit.

## 6. Review

One independent reader (read only) found no high defect; two medium and three low findings, all fixed on 20 November 2026:

1. **Medium: the two-process guard barely exercised stateful tools.** With `{}`, about eight tools only answered "missing parameter", and nothing made an edit, a checkpoint or a replay, so the feed, the history and the checkpoint list were nearly empty. *Fixed:* the table of real calls above; a tool without a row, or a row whose tool never answers `ok`, fails the test. *Proof:* a measured `seconds` was put back into `script_build`'s answer for one run; the test failed at that call (`call 33: ("script_build", {})`, the two answers differing in `"seconds"`), and the change was taken out again. The test takes about 22 s, both processes and the build included (the resource test about 1 s).
2. **Medium: `pick` could choose a pre-lock 0.1.0 engine for a format-2 project.** An engine packaged before the look lock calls itself 0.1.0, has no `opens_from`, was taken as `Exact` for a 0.1.0 project, and then refuses its format-2 files (the user's own `C:\wish-build\package` is such a build). *Fixed:* `opens` judges an engine without the key by the caret only for a project older than `UNSTATED_BEFORE` (0.1.0, fixed for good: it records when the key began), and `pick` takes an exact version only among the engines that `opens` accepts. In practice no engine without the key is chosen for any project that exists; it stays in the list. Test: `an_engine_without_opens_from_is_not_taken_for_a_project_it_may_not_read`, with a pre-lock folder (0.1.0, no key) read from disk.
3. **Low: `migrate` could lower a newer `engine` key** (a 0.5.0 project migrated by 0.3.0 got 0.3.0). *Fixed:* the key is written only when it is older than this engine, missing, or not a version; tested with a project naming 9.0.0 and one naming `0.2`.
4. **Low: a bad `opens_from` gave the generic hint of an invalid engine folder.** *Fixed:* the message names the key and its value, and the hint says to fix it (three numbers, no newer than `version`) or remove it.
5. **Low: the link test.** *Fixed:* reference links `[text][id]` and `[text][]` must have a definition in their file (labels compared without case and extra space); indented code blocks are skipped (outside lists, after a blank line), and fences are recognized at any indentation; only `.git` and `.wish` among hidden folders are skipped, so a Markdown file in `.github` or a template's `.template` is checked.

## 7. Not verified

- `wen package` writing `opens_from` was not run end to end (it builds the whole package); `engine_file_text`, which it calls, is tested.
- The Hub's display of the new note was not looked at in a window; the text and its 11 translations are checked by the language test only.
