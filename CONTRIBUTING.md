# Contributing to Wish Engine

Thank you for helping. Wish Engine is a deterministic, schema-first 3D engine written in Rust. It
is fully usable by hand and fully controllable by AI agents over MCP. These rules keep it that way.

*Türkçe:* katkı kuralları aşağıda İngilizcedir, çünkü kod, tanılar ve commit iletileri İngilizce
yazılır. Planlama belgeleri (`docs/`) Türkçedir. Sorularınızı Türkçe de sorabilirsiniz.

## Build and test

You need:
- Rust 1.100.0 (pinned by `rust-toolchain.toml`; `rustup` installs it);
- the `wasm32-unknown-unknown` target;
- on Windows, Visual Studio Build Tools with the C++ workload.

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

Render tests run on a software adapter, so they give the same pictures on every machine. Golden
images are refreshed with `UPDATE_GOLDEN=1`; look at the difference before committing it. Render
tests must also pass in release (`cargo test --release -p wish_render`).

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
4. An idea outside the current scope goes to the M10 list in [docs/ROADMAP.md](docs/ROADMAP.md),
   not into the code.

## Where things are

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the layers and crates.
- [docs/MCP_API.md](docs/MCP_API.md): the agent interface.
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones and exit criteria.
- [docs/user/en/guide.md](docs/user/en/guide.md): the user guide.

## License

By contributing, you agree that your contribution is licensed under both the MIT license and the
Apache License 2.0, as the engine is.
