# Stability

**The short stability note of 1.0.** It says what a project made with Wish Engine can rely on from
version 1.0.0 on, and what it cannot rely on yet. Release 1.3 extends this file into the full
stability contract (stable and experimental labels, and snapshot tests of the tool catalogue and
the SDK); until then, this is all that is promised.

The rules behind it are architecture decisions:
[ADR-027](DECISIONS.md#adr-027--four-rules-before-the-freeze-defaults-answers-versions-claims)
(defaults, answers, versions, claims) and
[ADR-028](DECISIONS.md#adr-028--rules-of-the-release-train-supported-replays-results-dependencies-formats)
(the rules of the release train). Where this note and those records differ, the records hold, and
this note is corrected.

## Before 1.0.0

Nothing is frozen. The code is at 0.1.0; the betas and release candidates of 1.0 are `0.x.0` tags.
A 0.x number says which build it is, not what it is compatible with. Two things already hold:

- **Scene and prefab files of format 1 and format 2 are read.** A file of format 1 (written before
  the default look changed) is read as it was meant: where a default changed since, the file keeps
  the old value, so it looks and plays as it did. `wen check` names such a file with the warning
  `old_format`; `wen migrate`, or the next change saved through the engine, rewrites it as format 2
  with those values written in. A file of a format newer than the engine is refused
  (`unsupported_format`). Checked by `cargo test -p wish_control --test look_lock`.
- **Versions are three numbers**, `major.minor.patch`, with no suffix such as `-rc.1` (ADR-027,
  rule 3).

## From 1.0.0 on

1. **File formats change only by addition or with a migration** (ADR-028, rule 5). An addition
   leaves existing files byte for byte the same and keeps their meaning. Any other change comes
   with a migration that is proven on a corpus of projects written by older versions: in CI they
   open, migrate and keep their meaning. Schema defaults are part of the format, because files
   leave defaults out (ADR-027, rule 1): a default changes only in a planned window, with a
   migration. The planned windows are the format window of release 1.2 and the second look lock
   at 2.0.
2. **Old scene files keep opening.** Formats 1 and 2 are read as above, and a later format adds its
   step to the same migration.
3. **Simulation results change only in a minor version** (ADR-028, rule 3). What a simulation
   computes (a world hash for the same inputs, a fuel count, a physics result) may change in
   `1.y.0`, and the change is listed in the release notes. A patch version (`1.y.z`) carries fixes
   only and never changes it.
4. **Pinned dependencies are upgraded only as their own package, in a minor version** (ADR-028,
   rule 4): `wasmtime`, Rapier, `wgpu`, `winit`, `egui` and the packed compiler. Never in a patch,
   and never as a side effect of other work.
5. **Versions stay three numbers** (ADR-027, rule 3). A project's `wish.json` names the engine
   version that made it; that is a hint for choosing among installed engines, never a gate
   (`cargo test -p wish_control --test engines`). An engine reads the scene files of every older
   format and says so with the warning `old_format`; such a file changes only when it is saved or
   migrated (`cargo test -p wish_control --test look_lock`).

## Not promised yet

- **The tool surface**: the names, parameters and answers of the control-plane tools (MCP, `wen
  call`, `wen do`) and of the MCP resources. Release 1.3 freezes them under the full contract.
- **The SDK's source API**: what game code writes against (`wish_sdk`). Release 1.3 freezes it. The
  interface between game code and the engine is planned to change once, in release 1.8 (ADR-028,
  rule 5).
- **The word "supported"** for a platform. At 1.0.0, Windows and Linux are packaged and tested
  hosts. A platform is called supported only while the committed replays give the same hashes on
  it in CI as on the others (ADR-028, rule 1), from release 1.1 on.
- **Replays across versions.** A replay does not yet record the engine version that made it
  (ADR-028, rule 2, release 1.1). Replays recorded before 1.0.0 are not promised to play back.
- The editor's interface, the command line's text output (its `--json` answers are the tools'
  answers), and the Rust APIs of the engine's crates.

What 2.0 may change is bounded too: it breaks nothing that a migration cannot carry, and removes no
tool and no SDK item (ADR-028, rule 5).
