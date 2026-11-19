# Contributing to Wish Engine

Thank you for helping. Wish Engine is a deterministic, schema-first 3D engine written in Rust. It
is fully usable by hand and fully controllable by AI agents over MCP. These rules keep it that way.

*Questions are welcome in Turkish too.*

## Build and test

You need:
- Rust 1.100.0 (pinned by `rust-toolchain.toml`; `rustup` installs it);
- the `wasm32-unknown-unknown` target;
- on Windows, Visual Studio Build Tools with the C++ workload;
- on Linux, the ALSA headers and `pkg-config` (`libasound2-dev pkg-config` on Debian and Ubuntu):
  sound output links ALSA.

Before every commit, all three must be clean:

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

Render tests run on a software adapter (WARP on Windows, lavapipe on Linux), so a picture does not
depend on the graphics card. The two adapters differ slightly (by at most 1/255 a channel when
measured in M3), and golden-image comparisons allow a small tolerance for that. Golden images are refreshed with `UPDATE_GOLDEN=1`; look at the
difference before committing it. Render tests must also pass in release
(`cargo test --release -p wish_render`).

If you touch a crate with `unsafe` code (`wish_ecs`, `wish_sdk`), Miri must be clean too:

```bash
cargo +nightly miri test -p wish_ecs
```

## The invariants

A change that breaks one of these is wrong, however useful it looks. The reasons are in
[docs/DECISIONS.md](docs/DECISIONS.md).

1. **Schema first.** No type enters the world without a schema. Every field has a doc comment, and
   a unit and a range where they apply.
2. **Determinism.** Simulation code must not use:
   - the wall clock;
   - unseeded randomness;
   - iteration over unordered collections;
   - results that depend on thread timing.
3. **Files are the source of truth.** Documents are canonical JSON: load and save gives the same
   bytes.
4. **One control plane.** A new capability goes into `wish_control` first. MCP, the command line
   and the editor call it. Nothing may exist only in the editor.
5. **One diagnostic format.** Every error has a `code`, a `message`, a location and a **`hint`**.
   An error without a hint is unfinished. New codes go into `wish_control::docs::DIAGNOSTICS`.
6. **Dependencies point down.** Subsystems talk through the ECS, not to each other. An outside
   library is used only from its own wrapper crate.
7. **Game code is untrusted.** A WebAssembly guest must not be able to crash or freeze the engine,
   and no state lives in the guest.
8. **Observable and tested.** A feature is done when the control plane can observe it and a
   headless test covers it.
9. **Hybrid parity.** The engine works with no AI connected. Every scene or asset operation an
   agent can do can be done by hand in the editor. Both kinds of change enter one history, with
   who made them (`actor`).

## Rules the tests enforce

| You add | You also add |
|---|---|
| A control-plane tool | an example in `wish_cli/tests/parity.rs`; a row in `wish_editor::parity::PARITY` naming the editor test that does it by hand; a `dry_run` parameter if it changes anything |
| An `edit_apply` operation | a `PARITY` row and its by-hand test |
| A diagnostic code | its entry in `docs::DIAGNOSTICS` |
| Editor text | `t("...")` / `tf("...", ...)` with a literal, and its translation in all 11 files of `crates/wish_editor/lang/` |
| A dependency | a license check (MIT, Apache-2.0 or BSD-like only; see `deny.toml`), then `python tools/third_party.py` |
| A `Plain` type or a macro message | refreshed `trybuild` snapshots (`TRYBUILD=overwrite`), with the difference reviewed |
| An engine type, a field or a default | the recorded defaults refreshed (`UPDATE_DEFAULTS=1 cargo test -p wish_control --test look_lock`); a default changes only in a planned format window, with a migration ([docs/STABILITY.md](docs/STABILITY.md)) |
| A link in any Markdown file | a target that exists, with its letter case and `#anchor` (`cargo test -p wish_control --test doc_links`) |
| A number, a comparison or a "works on" in public text | the command that reproduces it, or who checked it, on what and when ([ADR-027](docs/DECISIONS.md#adr-027--four-rules-before-the-freeze-defaults-answers-versions-claims), rule 4) |

## Code conventions

- **Language:** code, identifiers, diagnostics, comments and commit messages are in English.
- **Naming:** crates are named `wish_*`; the command-line tool is `wen`.
- **License:** every crate's `Cargo.toml` has `license = "MIT OR Apache-2.0"`.
- **Errors:** library code has no `unwrap`, `expect` or `panic!`. Errors are returned as a
  `wish_core::Diagnostic` through `wish_core::Result`.
- **`unsafe`:** only in `wish_ecs`, `wish_script`, `wish_sdk` (the guest boundary) and the `Plain`
  contract of `wish_schema`. Each block has a `// SAFETY:` comment.
- **Hashes:** anything hashed for the world or for disk uses `wish_core::StableHasher`.
- **Doc comments:** every public item has one; they flow into schemas and the MCP docs.
- **Coordinates and units:**
  - right-handed, +Y up, cameras look down −Z;
  - metres, seconds, kilograms;
  - radians in the API.
- **Integration tests:** files in `tests/*.rs` start with
  `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`.

## How work is done

1. Write a short design note before code (`docs/design/`): the problem, the options, the decision,
   the test plan.
2. Work in small steps. Each step builds, is tested and is committed.
3. Show that it works: run the command that proves it, and report its output. If a test fails, say
   so with the output.
4. An idea outside the current scope goes to the M11 list in
   [docs/ROADMAP.md](docs/ROADMAP.md#m11--after-10-order-depends-on-need), not into the code.

## Releases

`.github/workflows/release.yml` builds the release archives
([docs/design/M12-release-pipeline.md](docs/design/M12-release-pipeline.md)). A tag of three
numbers that equals the workspace version (`0.2.0`; no `v`, no suffix) builds the Windows `.zip`
and the Linux `.tar.gz` with `SHA256SUMS`, checks them, and makes a draft GitHub Release that a
person publishes. A push that changes the packaging code runs the same build as a dry run.

To make one locally, build the shipped programs with the `dist` profile, then package:

```bash
cargo build --profile dist -p wish_cli -p wish_player
target/dist/wen package --out <folder>/package
```

On Windows, set `RUSTFLAGS=-C target-feature=+crt-static` and add
`--target x86_64-pc-windows-msvc` (the programs then are in `target/x86_64-pc-windows-msvc/dist`),
so that they start on a Windows without the Visual C++ runtime. The game SDK's three macro
libraries do not get the flag yet, so building game code still needs that runtime; `wen package`
warns about it (`package_needs_static_crt`;
[docs/design/M12-release-pipeline.md](docs/design/M12-release-pipeline.md), section 6).

The pictures of the README are drawn by the engine: after `cargo build --release -p wish_cli`,
`python tools/readme_pictures.py` draws them again into `docs/images/`. Look at each one before
committing it.

## Where things are

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the layers and crates.
- [docs/MCP_API.md](docs/MCP_API.md): the agent interface.
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones and exit criteria.
- [docs/user/en/guide.md](docs/user/en/guide.md): the user guide.

## License

By contributing, you agree that your contribution is licensed under both the MIT license and the
Apache License 2.0, as the engine is.
