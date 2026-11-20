# M5 — Game logic: Wasm host, SDK, hot reload, input, scenario tests

Status: design (16 November 2026). Branch: `m5-logic`. Related decisions: ADR-005 (determinism), ADR-006 (snapshot, replay), ADR-011 (game logic in Wasm), ADR-016 (input), ADR-017 (testing), ADR-022 (security), ADR-023 (embedded toolchain).

## 1. Goal and limits

At the end of M5, an agent (or a person) writes `scripts/src/lib.rs` in the project folder, the engine compiles it to Wasm, the running game **continues where it was** with the new code, the game is played with the keyboard, a scenario test proves the "you won" state, and a replay recording plays back for 10,000 ticks with no hash drift. Malicious or faulty guest code (infinite loop, trap, memory blow-up) does not freeze the engine.

**In scope:** `wish_script` (wasmtime host, ABI v1), `wish_sdk` + macros (guest), build driver and diagnostic mapping, hot reload + schema migration, `wish_input` (action maps, injection, recording), replay files, `wish_test` (scenario tests), the tools `script_build` / `test_run` / `replay`, `input` on `sim_step`, `session` on `edit_apply` (carried over from M4), `wen play` (play in a window), boundary cost benchmark, embedded toolchain trial.

**Out of scope (on purpose):** physics (M6; the mini game does its own simple collision with the SDK), audio (M6), gamepad (keyboard + mouse in M5; the gamepad is added to `wish_input` in M6, where the `gilrs` license is checked), a second guest language (M11), Component Model/WIT (follows ADR-011), third-party crates for game code (§5.3: only `wish_sdk` in the first version; goes on the M11 list), `wen export` (M9).

## 2. Crates and dependency direction

| Crate | Side | Responsibility | External dependency |
|---|---|---|---|
| `wish_script` | host | Module load/validation, manifest, type registration, wiring systems to `wish_app`, call frame, command buffer, fuel/memory/trap, trace → file:line | `wasmtime` (only here; invariant 6) |
| `wish_sdk` | guest | API for game code: `Query`, `Res`, `ResMut`, `Commands`, `Events`, `Actions`, `Time`, `Rng`, math, `log!` | none (only `core`/`alloc`/`std`) |
| `wish_sdk_macros` | guest (proc-macro built on the host) | `#[component]`, `#[resource]`, `#[event]`, `#[system]`, `wish_sdk::export!` | `syn`/`quote`/`proc-macro2` (already present) |
| `wish_input` | host | `input.json`, `ActionState` resource, device event → action, injection, recording stream | — |
| `wish_test` | host | Scenario tests (`*.test.json`), replay verification, drift search | — |

Direction: `wish_script`, `wish_input` → `wish_app`, `wish_ecs`, `wish_schema`, `wish_core`. `wish_test` → `wish_script`, `wish_input`, `wish_scene`, `wish_render` (for image comparison). `wish_control` uses all of them; `wish_window` only knows the event types of `wish_input`. Subsystems do not know each other.

**`wish_app` change:** `SystemFn` is a plain `fn` pointer today; a Wasm system carries a module handle. `System::dynamic(name, Box<dyn FnMut(&mut World, &mut SystemContext) -> Result<()> + Send>)` is added; the program's fingerprint includes the system's **code fingerprint** (the `StableHasher` hash of the module content), so "same build" (D1) covers Wasm code too.

## 3. ABI v1 (core Wasm module)

The contract rests on small, versioned bulk data that flows one way. The guest imports only from the `wish` module; **the module rejects every other import (WASI included)** (`script_forbidden_import`). File, network and clock access structurally do not exist (ADR-011, ADR-022).

**What the guest exports:**

| Name | Signature | Meaning |
|---|---|---|
| `memory` | memory | The guest's linear memory |
| `wish_abi_version` | `() -> i32` | `1`; if different, `script_abi_mismatch` + "rebuild the SDK with this engine" |
| `wish_manifest` | `() -> i64` | (pointer << 32 \| length): manifest JSON |
| `wish_alloc` | `(len: i32) -> i32` | Space where the host writes the frame |
| `wish_run` | `(system: i32, frame: i32) -> i32` | Run one system; returns: (pointer to) the command buffer, or 0 |

**Manifest:** module name, SDK version; types (in the **same** format as the schema JSON of `wish_schema` — component/resource/event, field docs, unit, range, default); systems (name, order, `before`/`after`, queries: read/written components, `with`/`without`; read/written resources; read/sent events). The host checks the manifest against the schema. If a guest type has the same name as an engine type (like `Transform`), their **fingerprints** must be equal (`script_type_mismatch`).

**Byte layout:** components are `Plain` (no pointers, no `usize`, no references). The `repr(C)` layout of these types is the same on wasm32 and x86_64 (`f32`/`u32` aligned to 4, `u64`/`f64` aligned to 8), so the guest sees **the same bytes as the ECS column** (ARCHITECTURE "Wasm ABI layout"). We do not assume this, we **check** it: the size/alignment/field offsets of every type in the manifest are compared with the host's `TypeInfo`. Padding bytes are never read (ECS rule): the host copies the field ranges into the frame with `Column::read_into`, and when writing back it also takes only the field ranges.

**Call frame (bulk transfer, ADR-011):** for each `wish_run` the host writes a single frame into guest memory: header (tick, `delta`, the system's `last_run`, random stream state), for each query the entity count + array of stable entity ids + a dense array (column slice) for each accessed component, resource bytes, read events. The guest changes writable arrays in place. On return the host copies the writable arrays back; **only entities whose bytes really changed** get their change tick advanced (change tracking stays honest; `query_since` works).

**Commands:** spawn/despawn entity, add/remove component, send event; the guest writes these into its own buffer (fixed binary encoding), and the host applies them after the call **in the order they were written** (ADR-005: command buffers merge in a fixed order). An invalid command (a type that does not exist, the managed component `Hierarchy`) stops the system like a trap and produces a diagnostic.

**Imports (`wish` module):** `log(severity, ptr, len)`, `panic(ptr, len)` (the SDK's panic hook: the message and `file:line` go into the diagnostic), `rng_next() -> i64` (the system's own seeded stream — `SystemContext::rng`), `action_index(ptr, len) -> i32` (action name → slot). No clock, file or network.

## 4. Guest SDK (`wish_sdk`)

```rust
use wish_sdk::prelude::*;

/// A coin that can be collected.
#[component]
pub struct Coin {
    /// Points given when collected.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
}

#[system(after = "move_player")]
fn collect(mut score: ResMut<Score>, players: Query<&Transform, With<Player>>,
           coins: Query<(Entity, &Transform, &Coin)>, mut commands: Commands) {
    for player in players.iter() {
        for (coin, at, c) in coins.iter() {
            if at.translation.distance(player.translation) < 0.6 {
                score.points += c.value;
                commands.despawn(coin);
            }
        }
    }
}

wish_sdk::export!(collect, move_player);
```

- The macros use the engine's own `#[component]` syntax (same attributes, same doc comment rule: an undocumented field is a compile error — invariant 1); the generated schema goes into the manifest. Engine types (`Transform`, `Camera`, `Primitive`, `Material`…) exist in the SDK with the same fields, and a fingerprint test keeps them equal to the engine (one test for each engine type: guest layout = host layout).
- Access is derived **from the signature** (`&T` reads, `&mut T` writes); the manifest carries this, and the host puts only the declared data into the frame. There is no way to access something that was not declared.
- Panic → `wish.panic` → trap: the diagnostic `script_panicked` carries the message and the location `scripts/src/lib.rs:42`.
- Math: `Vec2/Vec3/Quat` (inside the SDK, deterministic functions like `libm`; on the guest side the SDK's functions are used instead of platform functions like `f32::sin` — the D2 goal).

## 5. Build and toolchain (ADR-023)

### 5.1 Problem

Game code is compiled on the user's machine. Goal: work on a Windows with **no development tools installed**, and go from a code change to a running game in < 10 s. The trap: proc-macros and build scripts are compiled for the **host**, and on Windows that needs the MSVC linker (`link.exe`), which a clean machine does not have. The Wasm target's own link step uses `rust-lld` (it ships with rustc), so that is not a problem.

### 5.2 Options

| | Path | Host linker | Speed | Cost |
|---|---|---|---|---|
| A | `cargo build --target wasm32-unknown-unknown`, with the SDK proc-macros | **Needed** (proc-macro) | cargo resolution + incremental | Does not work on a clean Windows — eliminated, but kept for measurement |
| B | cargo, but no proc-macros in the SDK (declared with `macro_rules!`) | Not needed | cargo + incremental | Declaration syntax moves away from Rust; agent errors get worse |
| C | **Direct `rustc` without cargo**: `wish_sdk` (wasm32 rlib) and `wish_sdk_macros` (host dll) **come precompiled with the engine**; game code is compiled as a single crate with `--extern` | Not needed (macro dll is ready) | Fastest (no resolution, one rustc call) | Precompiled parts are tied to a fixed rustc version (already fixed: `rust-toolchain.toml`); in the first version game code cannot use third-party crates |

**Recommendation: C**, with B as backup. Reason: it fits the "seamless" criterion best (no network, no resolution, no linker, shortest build), and Rust's real attribute syntax is kept. This is a hypothesis; **WP1 decides by measurement** and writes the decision as ADR-025 (like the ADR-013 proof work in M3).

### 5.3 Proof method (WP1 and WP7)

- **Linker-free proof (automatic, in CI too):** the build runs with `PATH` holding only the toolchain folder, with `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=yok-baglayici.exe` ("no linker"), and with the Visual Studio environment variables removed. If linking for the host is attempted, the build fails → the test breaks.
- **Measurements:** cold build, build after a one-line change, module load (cranelift), instantiation; target change → game < 10 s (claim: < 3 s with C).
- **Package:** `wen toolchain pack` copies only what is needed from the fixed rustup toolchain (rustc, `rustc_driver`, LLVM, `rust-lld`, `rust-std-wasm32-unknown-unknown`, the precompiled SDK) into a folder; the size is measured and reported.
- **Clean machine (by hand, WP7):** Windows Sandbox (it exists on Windows 10 Pro, but must be turned on as an optional Windows feature — a step for the user) or another clean computer. The package folder is copied, and `wen build` + `wen play` are tried.

### 5.4 Diagnostics

The output of `rustc --error-format=json` is converted to the common diagnostic: `code` (like `rust_E0308`, or `rust_error`), message, `file:line:column` (project-relative), `hint` = rustc's help line + our own extra hints (for known SDK mistakes: "use `&mut` in the query", "add it to the `export!` list"). Condensing: follow-on errors caused by the same root error are counted under the first; the agent gets the first 10 errors + the total count. **If the build fails, the old module keeps running.**

## 6. Sandbox and determinism

- **wasmtime settings:** `consume_fuel(true)`, `cranelift_nan_canonicalization(true)`, threads/relaxed SIMD/GC/component model off, `StoreLimits` (memory 64 MiB by default, table limit), stack limit. Features: `default-features = false`, only `runtime`, `cranelift`, `std` (+ `wat` in tests).
- **Fuel is deterministic:** it counts instructions and never looks at the clock → an infinite loop stops **at the same instruction**, and the result is the same on every run (replay and determinism stay intact). The default budget is per system call (`wish.json` `scripts.fuel_per_call`); if exceeded, the `script_out_of_fuel` diagnostic + the system is disabled + the world continues. Cutting off by wall clock (epoch) is **not used**: it is not deterministic; the fuel limit alone prevents a freeze.
- **Trap** (`unreachable`, out-of-bounds memory, division by zero, stack overflow, memory limit) → system disabled (the meaning of today's `system_failed`), diagnostic `script_trap` + location (`file:line` from the Wasm name section + DWARF line tables; game code is compiled with line info).
- **Statelessness is enforced:** for every `wish_run` call a **fresh instance** from `InstancePre` (microseconds with the pooling allocator). The guest cannot keep state in a static variable; even if it did, it would be gone at the next call. So the snapshot is complete (ADR-006) and hot reload is lossless. WP1 measures the cost; if it is too expensive, we go down to one instance per tick (decision in ADR-025).
- **Randomness:** only `rng_next` (the system's seeded stream). Time: only `tick`/`delta` in the frame.

## 7. Hot reload and schema migration

```
scripts/src/*.rs changed (file watcher or script_build)
  → compile → error? diagnostics; old module keeps running
  → validate: ABI version, only `wish` imports, manifest, type layouts
  → at the tick boundary: register new types; migrate the data of a changed type by field name
      (old bytes → JSON with old schema → bytes with new schema; new field = default, removed field = dropped, field whose type changed = default + warning)
  → swap systems (disabled systems become enabled again), program fingerprint is updated
  → the game continues from the same tick
```

- The data of a removed component type stays in the world for the whole session (types are not deleted within a session) and is reported with a `script_type_removed` note; if it is still used in a scene file, `check` warns.
- **Scenes use game types:** the workspace's type registry = engine types + the types in the manifest of the last successful build (`scripts` takes the place of `types.example` in `wish.json`; `example` stays for example projects). If there is no compiled module, the scene still opens; unknown components are reported in `check` with the hint "run `script_build` first".
- Build output is `.wish/build/` (inside the project, in the `.gitignore` of `wen new`). Nothing is written anywhere outside the project folder.

## 8. Input (`wish_input`, ADR-016)

- `input.json`: actions and their bindings. Types: `button` (held/just pressed/released), `axis1`, `axis2` (e.g. `move` = WASD + arrow keys, diagonal normalized). Schema'd, canonical, validated by `check` (unknown key name → suggestion).
- In the world, the `ActionState` resource (fixed slots, `Plain`): actions settle into slots by their sorted names in `input.json`; the guest says `actions.axis2("move")`, and the name → slot mapping comes in the frame header.
- Sources: window (keyboard + mouse; `wish_window` events → action), **injection** (the `input` parameter of `sim_step`: `{ "move": [0, 1], "jump": true }` is held for all ticks of the step; `input_script`: tick → list of values), replay file. Input is written into `ActionState` at the start of the tick (ARCHITECTURE frame loop, step 1) — game code never sees the raw device.

## 9. Replay recordings (ADR-006)

- `replays/<name>.replay.jsonl`: a header line `{format, scene, scene_hash, seed, tick_rate, code_fingerprint}`, then only **changed** action values `{tick, actions}` and a `{tick, hash}` checkpoint every 60 ticks, and the final hash at the end.
- Playback: hashes are compared at the checkpoints; if there is drift, it runs tick by tick from the last matching point and reports the **first drifting tick** and the entities that changed at that tick (`replay_diverged`). If the code fingerprint differs, a `replay_code_changed` warning (playback still happens: the tool for "I changed the code, is the game still the same?").
- Recording: the session already keeps the action state of every tick; `replay save` writes it to disk. `wen play --record`.

## 10. Scenario tests (`wish_test`, ADR-017)

```json
{ "format": 1, "scene": "scenes/main.scene.json", "seed": 1,
  "steps": [
    { "input": { "move": [1, 0] }, "frames": 120 },
    { "expect": "Score.points >= 3", "message": "player collects the three coins" },
    { "until": "GameState.won == true", "max_frames": 600 },
    { "capture": { "camera": "camera", "golden": "tests/golden/win.png", "tolerance": 2 } }
  ] }
```

- `tests/*.test.json`, headless and deterministic. `expect` is M4's expression language (resources and entities; it can be narrowed to a single entity with `entity`). On failure: step number, tick, expression, **the values of the sub-expressions** (`Score.points = 2`), related entities nearby; for images, the diff picture goes to `.wish/test-output/`.
- Golden images use the software adapter and a perceptual tolerance (M3 rules); `update_golden` only when explicitly asked.

## 11. Control plane (invariants 4, 8, 9)

| Tool | What it does | Command line |
|---|---|---|
| `script_build` | Build; condensed diagnostics; if it succeeds, hot reload into open sessions (`reload`, on by default); time and module fingerprint | `wen build` |
| `test_run` | Scenario tests; `paths`, `update_golden` | `wen test` |
| `replay` | `save` (from the session), `run` (verify, first drift), `info` | `wen replay` |
| `sim_step` + `input` | Inject actions during the step | `wen call sim_step --input …` |
| `edit_apply` + `session` | Apply an operation to a live session (carried over from M4); not persistent, and the reply says so | — |

- `project_info` also reports script status (built or not, fingerprint, systems, last build errors); the `wish://docs/sdk` resource and the "game code" section of the guide; `wen new` comes with a `scripts/` skeleton (a small working system and a test) and `input.json`.
- **`wen play [--scene] [--record name]`**: real-time playback in a window (keyboard → actions), `scripts/` is watched, and when saved it is built and loaded **without stopping the game**; errors go to the title and stderr. It is the way for a person to play the game; for an agent, the same thing is `sim_*` + `capture_frame`.
- Every new diagnostic code goes to `docs::DIAGNOSTICS`; every new tool gets an example in the parity test (M4 rules).

## 12. Test plan (with exit criteria)

| Exit criterion | Evidence |
|---|---|
| Claude writes a playable mini game (move, collect, win) with MCP + file edits and proves it with a scenario test | (a) example project `examples/collect` in the repo + scenario test in CI; (b) new task(s) in `evals/`: the agent can edit the `scripts/` files (Read/Edit/Write only inside the project, no Bash), checked by a hidden scenario test (**run needs the user's approval**) |
| Code change → running game < 10 s; state is kept | Scripted test: session advances 300 ticks → source changes → `script_build` → same tick, same entity values, new behavior; time is measured and recorded (debug/release, cold/warm) |
| Infinite loop and trap do not freeze/crash the engine, they produce a diagnostic | Hostile guest suite (`wish_script/tests/hostile.rs`, hand-written `.wat` + Rust guests): infinite loop, deep recursion, memory blow-up, out-of-bounds access, division by zero, panic, invalid command, forbidden import, broken manifest, wrong ABI, broken Wasm bytes → all give a diagnostic, the engine continues, **same diagnostic at the same tick on the same input** |
| A recorded replay has no hash drift for 10,000 ticks | `wish_test` test: mini game records 10,000 ticks → playback in two separate processes → every checkpoint equal; with deliberately broken code the first tick of the drift is found correctly |
| Boundary cost benchmark recorded (ADR-011 trigger) | `wen bench --script`: native system vs Wasm system at 1/1,000/10,000 entities, fixed cost per call and cost per entity; result in the design note; whether the frame budget (20% = 3.3 ms) is exceeded |
| Embedded toolchain compiles and runs game code on a clean Windows | Automatic: build test with no linker + only the package folder (CI); by hand: trial on a clean machine (WP7, user step) |

## 13. Work packages and recommended model

| # | Work package | Model · effort |
|---|---|---|
| WP1 | **Proof work:** minimal wasmtime host (fuel, memory, trap, fresh-instance cost), measurement of build paths A/B/C + linker-free proof, first boundary cost measurement → **ADR-025** | Opus 5.5 · high |
| WP2 | ABI v1: `wish_sdk` + `wish_sdk_macros`, `wish_script` (manifest, type/layout validation, frame, commands, events, resources, rng, log, panic, trace → line), `wish_app` dynamic systems | Opus 5.5 · high |
| WP3 | Build driver + diagnostic mapping, `script_build` / `wen build`, workspace types from the manifest, `wen new` skeleton, hot reload + schema migration | Opus 5.5 · high |
| WP4 | `wish_input`: `input.json`, `ActionState`, SDK `Actions`, `sim_step input`, window keyboard/mouse → action, `wen play` (watch → build → load) | Sonnet 5.5 · medium |
| WP5 | Replay files: record, playback, drift search, `replay` / `wen replay`, 10,000-tick proof | Sonnet 5.5 · high |
| WP6 | `wish_test`: scenario tests, `test_run` / `wen test`, image comparison; `edit_apply` on a session | Sonnet 5.5 · medium |
| WP7 | Boundary cost benchmark (ADR-011 trigger); embedded toolchain package (`wen toolchain pack`) + linker-free CI test + clean machine trial (ADR-023 trigger) | Opus 5.5 · high |
| WP8 | Mini game example `examples/collect`, docs (guide, `wish://docs/sdk`), evaluation tasks + **approved** run, fixes based on findings | Opus 5.5 · high (setup); evaluated Sonnet 5.5 · medium |
| WP9 | Hostile guest and obfuscation tests, independent closing review, M5 result, merge to `main` | Opus 5.5 · high |

## 14. Risks

- **Embedded toolchain** (the "biggest product risk" of ADR-023): size (estimate 200–300 MB unpacked), antivirus false alarms, first build time. Measure: early measurement in WP1; if it does not hold, the ADR-023 trigger (precompiled SDK + a lighter path or a second language) comes into play.
- **Boundary cost:** a fresh instance + a frame copy on every call. Measure: WP1/WP7 measurement; ADR-011 trigger (shared memory views).
- **wasm32 ↔ x86_64 layout difference:** a check, not an assumption (§3); a layout equality test for every engine type.
- **Floating point D2:** no platform math in the guest (SDK functions), NaN canonicalization; the D1 guarantee is already limited to the same build.
- **wasmtime release speed** (a major version every month): does not leak outside `wish_script`; the version is pinned, updates are a separate commit.
- **rustc version dependency (option C):** the precompiled SDK is rebuilt when `rust-toolchain.toml` changes; on a version mismatch, `script_toolchain_mismatch` + hint.

## 15. Decisions / approvals that belong to the user

1. **Download (now, before WP1):** the `wasm32-unknown-unknown` target for the pinned toolchain (`rustup target add wasm32-unknown-unknown`, ~30 MB, static.rust-lang.org). Cargo dependencies (`wasmtime` 50.0.1, Apache-2.0 WITH LLVM-exception — allowed in `deny.toml`) come from crates.io during the build.
2. **CI:** adding the same target to the workflow (GitHub Actions time goes up a little).
3. **WP7:** clean machine trial — turning on the Windows Sandbox feature (a Windows setting, the user does it) or another clean computer.
4. **WP8:** the evaluation run spends subscription quota — approval before running.

## 16. Open questions

1. ~~An instance per call, or an instance per tick?~~ → WP1: per tick (ADR-025).
2. Third-party crates in game code: not available in option C. If the need is seen in evaluations, the cargo path (B) is added as a side path; note on the M11 list.
3. The number of `ActionState` slots (suggestion 64) and the action name length.

## 17. Implementation notes

### WP1 — proof work: host, instantiation cost, build path (ADR-025)

- **Decision: ADR-025** ([DECISIONS.md](../DECISIONS.md)): build path **C** (a single `rustc` call without cargo, precompiled SDK, no host linker) and **a fresh instance per module per tick** (a new one after a failed call). The hypothesis in §5.2 was confirmed by measurement; the "per call or per tick" question in §6 (open question 1) is closed.
- **`wish_script` (first version):** `ScriptEngine` (fuel on, NaN canonicalization, relaxed SIMD off / deterministic, memory64 and multi-memory off, 512 KiB Wasm stack, pooling allocator with 64 slots; wasmtime `default-features = false`, threads and component model not compiled), `ScriptModule::load` (compile, only `wish.log/panic/rng_next` imports, required exports, ABI version), `ScriptModule::run(system, frame, rng)` (fresh instance, copy the frame → `wish_run` → copy back; the system's random stream advances). For now an instance per call; the instance per tick comes in WP2 with the `wish_app` connection.
- **Error mapping:** `script_out_of_fuel`, `script_out_of_memory` (our own `ResourceLimiter` records the request, the guest sees -1 and Rust's allocator throws a trap), `script_stack_overflow`, `script_trap`, `script_panicked`, `script_call_failed`, load errors (`script_invalid_module`, `script_forbidden_import`, `script_missing_export`, `script_abi_mismatch`). **Location:** for a panic, the `file:line:column` sent by the SDK hook (ABI: `panic(message, file, line, column)`); for a trap, the **first game code frame** from the DWARF line tables of the Wasm trace (std/registry frames are skipped) and the function name (`trap_here`). The log is limited to 100 messages per call, and a message to 2000 bytes; nonsense pointers become empty text and cannot bring down the host.
- **Tests:** `tests/hostile.rs` (hand-written `.wat`: infinite loop, `unreachable`, infinite recursion, a 4 GiB memory request, division by zero, panic, 1000 logs, out-of-bounds write, nonsense pointer; WASI/clock import, a module with no exports, old ABI, wrong signature, broken bytes) — every error has a hint, **same diagnostic on the same input**, and then the module still runs; the global counter is 1 at every call (proof of a fresh instance); fuel use is the same across calls. `tests/rust_guest.rs`: a real Rust guest (compiled for wasm32 during the test): frame round trip, `index out of bounds` and `unwrap` panics with the correct line, an `unreachable` trap with the correct line and function name.
- **Measurements** (release, to reproduce use `--ignored --nocapture`): `rust_guest.rs::measure_call_cost`, `module.rs::profile` (step breakdown), `build_paths.rs` (paths A/C, build with a copied toolchain in an empty environment, size). The numbers are in ADR-025. Lowering the guest stack from 1 MiB to 64 KiB and shrinking the initial memory made no measurable difference (within noise); the instantiation cost comes from wasmtime's instance setup/drop work, not from memory size.
- **Environment:** the `wasm32-unknown-unknown` target was installed with the user's approval (16 November 2026) and added to `rust-toolchain.toml` (CI and new machines install it automatically).
- **Note:** the measurement guests are under `crates/wish_script/guests/`, outside the workspace (each with its own `[workspace]`); `spike` is the hand-written ABI, and it will be replaced in WP2 with what the SDK generates.

### WP2 — ABI v1: SDK, macros, manifest, frame, commands, dynamic systems

- **Important decision — a single schema source (invariant 1, ADR-003):** the guest uses the engine's **own** schema macros (`#[component]`, `#[resource]`, `#[event]`, `#[schema]`) and `wish_schema` (thanks to the precompiled SDK path of ADR-025, the dependency is not a problem). Engine types were moved to the new **`wish_types`** crate (`Transform`, `GlobalTransform`, `Camera`, `DirectionalLight`, `Shape`, `Primitive`, `Material`, `Model`, `Sky`, `Ambient`, `Exposure`, `Time`; it does not depend on the ECS, and it compiles on wasm32); `wish_transform`, `wish_render`, `wish_app` re-export them (outer paths and hashes did not change). So the declaration the guest sees is **the same source code** as the engine's; there is no separate "SDK copy" and no drift risk. This replaced the "with the same fields in the SDK" plan in §4.
- **Wire format:** `wish_schema::wire::{to_wire, from_wire}` — a JSON of `TypeSchema` that round-trips exactly (tested for every engine type and every field type, fingerprints equal); `from_wire` limits the depth and checks the result with `TypeSchema::validate` (a hostile description is rejected: `invalid_type_description`).
- **Manifest (`wish_manifest` → JSON):** `abi`, `sdk`, `types` (role + schema in wire format; the ones systems use are included automatically, and the ones listed in `export!` are added — for types that scenes will use but no system queries), `systems` (name, `before`/`after`, parameters: `query {columns [{type, write}], with, without}`, `res {type, write}`, `events {type}`, `send {type}`, `commands`). The host reads it (`Manifest::parse`, limits: 256 systems, 1024 types, 32 parameters, 4 MiB) and `GameModule::install` registers the types: if a type with the same name exists in the world, **its layout must be the same** (`register_*_schema` already did this) — engine types are checked this way (`script_type_mismatch`).
- **Frame (`wish_sdk::frame`, code shared by both sides):** header (`WSHF`, version, tick, section table) + a section aligned to 8 for each parameter: query = count, column count, entity bits, then the dense column of each component; resource = the bytes of the value (if it is not in the world, the host **adds the default**); events = count + values (those sent **since the system's last run**: `World::event_bytes_since(last_run)`, correct on restore because `last_run` is in the snapshot). The command and event writer sections are empty.
- **Guest side:** `Query<D, F>` (`D`: `Entity`, `&T`, `&mut T`, tuples up to 8; `F`: `With`, `Without`, tuples) decodes columns with `Plain::decode`, `iter`/`iter_mut`/`get`/`get_mut`/`single`/`entities`; **`&mut` columns and `ResMut` are written back into the frame when dropped (`Drop`).** `#[system]` (the new `wish_sdk_macros`, syn) turns the implicit reference lifetimes in parameter types into `'static` pointers and generates a pointer type (in the type namespace) with the same name as the function; `export!` (macro_rules) writes the four exports. In the guest, panic = the error reporting path (`abi::fail`); the panic hook sends the message and line to the host.
- **Commands (`wish_sdk::command`, shared code):** spawn (with a sequence number within the call), despawn, insert, remove, send, set_parent, remove_parent; types **by name** (the guest does not need to build a type table at run time); the decoder checks every length (broken bytes are an error, not a panic). The host applies them in the given order; a command that cannot be applied is **skipped and written to the log as a warning** (`script_command_failed`, at most 8 messages per call), the others run, and the system continues — the same meaning as the native command buffer (changed in WP9: it used to stop the system; the target of a deferred command may have gone away in the meantime). Despawning an entity that is already gone is not an error. Things that stop the system: bytes that cannot be decoded, more than 65,536 commands per call, a new entity when the world already has 4,194,304 entities.
- **Write-back is honest:** the host compares every writable value with the one in the world, writes only what **really changed** (change tracking stays correct), and validates it like any data coming from outside (`insert_bytes`: bool/enum bytes, text handles; `script_write_failed`). Added to `wish_ecs` with safe code: `component_bytes_into`, `resource_bytes`, `insert_resource_bytes`, `send_bytes`, `event_bytes_since` (validation in one place, `check_outside_bytes`).
- **`wish_app`:** `System::dynamic(name, Arc<dyn Fn>)`, `App::replace_system` (replaces the system with the same name, **keeps its random stream and `last_run`**, re-enables it if it was disabled), `App::remove_system`. **Decision (a change from the plan in §2):** the code fingerprint does **not** take part in the program's fingerprint — snapshots can be restored even after a hot reload ("rewind, fix the code, try the same moment again"); the code fingerprint is reported separately (`code_fingerprint` in the replay header, WP5).
- **Instance per tick (ADR-025):** `ScriptRuntime` (one per app; two sessions never share an instance) holds the tick's instance; a new one is opened when the tick changes, when a system runs a second time in the same tick (which means a restore), or when a call fails. Guest logs come with `take_logs()` (tick, system, severity, message).
- **`unsafe`:** in `wish_sdk` only at the guest boundary (`frame.rs`: slice from the frame pointer; `abi.rs`: imports, allocation). Safety reason: frames are never freed while the instance lives, sections do not overlap, and each section has a single writer. It is checked with Miri (`wish_sdk/tests/frame.rs`). Added to the allow list in CLAUDE.md.
- **Tests:** `wish_script/tests/game.rs` — a mini game written with the SDK (`guests/collect`: movement, collecting coins, score, events, a bonus spawned by a command, `After` ordering) in a real `App`: after 60 ticks the player x = 6 m, score 13, four coins; same start → same hash; going back to a snapshot and running again, and swapping the code between two ticks with a new runtime, gave **the same hash as an uninterrupted run**; a panicking system gives `script_panicked` + `src/lib.rs` + **the correct line**, an infinite loop gives `script_out_of_fuel`, both are disabled and the game goes on unchanged. `wish_types/tests/wire.rs` (wire format), `wish_sdk/tests/frame.rs` (query/resource/event decoding and write-back, command round trip, rejecting a broken frame/command), `wish_app` dynamic system test. 385 tests in total.

### WP3 — build driver, `script_build`, types, hot reload and schema migration

- **SDK package (`wish_script::build::pack_sdk`, `wen sdk`):** from the output of `cargo build -p wish_sdk --target wasm32`, it collects everything the game build needs into one folder: `wasm/` (wasm32 rlib/rmeta files), `host/` (proc-macro libraries: `wish_sdk_macros`, `wish_schema_macros`, `serde_derive`), `sdk.json` (the rustc version that built the package, the files of the crates that game code names: `wish_sdk`, `wish_schema`). In a packaged engine, `sdk/` comes next to the program; in development, `wen sdk` (default: next to `wen`) or `WISH_SDK`.
- **Build (`build_game`):** ADR-025 path C — `rustc --crate-type cdylib --target wasm32-unknown-unknown -C opt-level=1 -C panic=abort -C debuginfo=line-tables-only -C incremental=.wish/build/incremental --extern wish_sdk=… --extern wish_schema=… -L dependency=sdk/wasm -L dependency=sdk/host --error-format=json scripts/src/lib.rs`, with an **empty environment** (`env_clear`; only the compiler's folder and what Windows requires) — the same source gives the same module on every machine. The output goes first to `.wish/build/game.next.wasm`; if successful it replaces `game.wasm`, and **a failed build does not damage the previous module**. If the compiler is not found / the version does not match: `script_toolchain_missing` / `script_toolchain_mismatch` (the SDK and the compiler must come from the same package).
- **Diagnostics:** rustc's JSON messages → `script_compile_error` / `script_compile_warning`; the message is `[E0308] …`, the location is project-relative (`scripts/src/lib.rs`, line, column — paths are already like that because the compiler runs in the project root), the hint = rustc's `help`/`note` sub-messages (including the suggested fix) or `rustc --explain <code>`. At most 20 messages + total counts.
- **Measurement (development machine, debug test):** first build 2.4 s, **0.7 s after a one-line edit** (incremental). The exit criterion "< 10 s" holds easily (loading in the window is measured in WP4 with `wen play`).
- **Control plane:** `Workspace` holds the compiled module (`script.rs`): when a project opens, `.wish/build/game.wasm` is loaded if it exists (a reopened project knows the game's types without compiling); types are registered **in one place** (`types::register_all`: engine + example + module) — document views, the type registry and sessions use the same function; the cache of the document view is refreshed when the module changes. Sessions run the module's systems with a `ScriptRuntime`, with `transform_propagation` after all of them; the game code's logs are in `logs_get` with `kind: script`.
- **`script_build` tool (the 21st tool) / `wen build`:** `reload` (on by default), `dry_run` (it only builds and checks that it can be loaded, nothing changes). Reply: `passed`, `errors`, `warnings`, `seconds`, `systems`, `fingerprint`, `diagnostics` (compiler messages + session notes: `script_reloaded`, `script_type_changed`, `script_reload_failed`). If there is no source, `script_sources_missing` (before the compiler is looked for, so that the parity test stays independent of the environment). `project_info` now carries a `scripts` field (source, module: file, fingerprint, systems, the game's own types; last build; load problem).
- **Hot reload:** the new module enters every open session at the tick boundary: types are set up, systems of the old module that no longer exist are removed, the rest are swapped with `replace_system` (**random stream and `last_run` are kept**), new ones are added, and `transform_propagation` is reordered. The world is left untouched → the game continues where it was with the new behavior (in the test: a spinning top that rotates returns with the new code, and the angle is continuous).
- **Schema migration (`wish_ecs::World::retype_component` / `retype_resource` / `retype_event`):** when a type of the game changes its fields, every value is moved to JSON with the old schema, and from there into the new layout **by field name** (same name + the value still fits: kept; new field: default; the rest is reported in `Retyped::dropped`); added/changed ticks are kept; instances of events in the old layout in the buffer are discarded. Engine types (bound to a Rust type) and managed types are rejected (`retype_refused`). `GameModule::install` first **checks everything** (if one engine type does not match, nothing changes), then migrates and produces a `script_type_changed` note. Deterministic: the same migration twice gives the same hash (tested).
- **`wen new`** now comes with `scripts/src/lib.rs`: a single component (`Spin`, rad/s, range, default) and a real game code with a single system, small enough to be read and built upon; a "Game code" section in the project `CLAUDE.md`, `.wish/` in `.gitignore`; a "Game code" section in the MCP guide (`wish://docs/guide`).
- **Tests:** `wish_script/tests/build_game.rs` (package + build + run, edit time, error line and code, warning, keeping the old module, no source, version mismatch), `wish_control/tests/script_tools.rs` (`wen new` → `script_build` → `Spin` in the scene → session → continuing with the new code with state kept + log → old code on a broken build → `dry_run` → reopened project; keeping the values of a component that got a new field in the running game), `wish_ecs/tests/retype.rs` (migration, ticks, report, events, resources, determinism, engine type rejection). In the parity test the `script_build` example is a parameter error that does not depend on the environment (SDK/compiler change from machine to machine).
- **Real flow (release `wen`):** `wen sdk` → `wen new` → `wen build` 1.96 s; when one line changes, **0.41 s**; the error is `scripts/src/lib.rs:20:76` + `[E0277]` (rustc's long implementation lists are cut to the first 3 messages / 500 characters in the hint).
- **Disk finding:** because `wasmtime` is now linked into the test binary of every upper crate, the build folder with full debug info passed 40 GB and the disk filled up (linker LNK1180, then LNK1285 with broken PDBs). Fix: `[profile.dev.package."*"] debug = "line-tables-only"` (for dependencies); the folder is 8.7 GB. 392 tests in total.

### WP4 — input: `input.json`, `ActionState`, `Actions`, `sim_step input`, `wen play`

- **Decision (a change from §8):** the action state is the **`ActionState` resource** in the world (`wish_types::input`, no ECS, compiles on wasm32): 32 fixed slots, each slot has `id` (the 32-bit FNV-1a digest of the action name, never 0), `held`, `pressed`, `released`, `x`, `y`. The guest looks up the **digest**, not the name (`action_id`, `const fn`), so there was no need to carry a name table or add an import (the ABI was not touched). Slots are in the sorted order of the action names; a digest collision is rejected when `input.json` is validated. Because the resource is in the world, it **goes into** the snapshot and the world hash: the same input sequence gives the same hash, and rewinding rewinds the input state too. For a project with no actions nothing is added to the world (existing hashes did not change).
- **`wish_input`:** `input.json` (`format`, `actions`: `button` {keys}, `axis` {positive, negative}, `axis2` {up, down, left, right}; diagonal movement is normalized); key names are a closed set (A–Z, 0–9, F1–F12, Space, Enter, Escape, Tab, Shift, Ctrl, Alt, arrow keys, MouseLeft/Right/Middle); a wrong key comes with the hint "Did you mean 'Space'?", an unknown field/type with the serde location (`invalid_input_file`, line/column). `InputMap::evaluate(held keys) → action values` and `apply(world, map, values)`: compares with the previous tick's state and produces the `pressed`/`released` edges, turns NaN into rest, clamps values to [-1, 1], and does not write if nothing changed.
- **SDK:** the `Actions` system parameter (`held`, `pressed`, `released`, `axis`, `axis2`); an unknown name is read as rest. It shows in the manifest as the `ActionState` resource; if it is missing, the host adds the default.
- **Control plane:** every session reads `input.json` when it starts; the new **`input`** parameter of `sim_step` (`{"move": [0, 1], "jump": true}`) is held for all ticks of the call, then released (a `released` edge is born at the next call); an unknown action gives `unknown_action` + a suggestion, a broken value gives `invalid_parameter`. `check` also validates `input.json`; `wen new` writes the starter map (`move`: WASD + arrows, `jump`: Space); the project `CLAUDE.md` and the MCP guide describe `Actions` and `input`. The replay recording (WP5) will record this same input stream.
- **`wen play [--root] [--scene] [--size] [--software] [--exit-after]`:** a **live content** mode was added to the viewer in `wish_window` (`Content::live`/`play`; key names are turned into `input.json` names with `key_name`; all three mouse buttons included). In live mode the fly camera and view keys are off, **every key except Escape belongs to the game**, and the game is drawn from its own camera. `wen play` runs a control plane session (`sim_start`) at 60 ticks/s in real time (at most 5 catch-up ticks: a stutter does not make the game race); when a `.rs` file under `scripts/` is saved, `script_build` (+ `reload`) runs, the game moves to the new code **without stopping and keeping its state**, a build error shows in the title/stderr and the old code keeps running. The capability is in the control plane as `Workspace::play_step`/`play_scene` (invariant 4); the window is only one face of it. Scene file changes need a restart (it says so). `wen play --exit-after 30 --software` was tried locally (30 frames, exit 0); the window loop does not run in CI.
- **Tests:** `wish_input/tests/input.rs` (3), `wish_control/tests/script_tools.rs` (+2: a game with the SDK is played with `sim_step input` — walk forward, `pressed` only on the first tick, release and press again, same input in two sessions = same hash, wrong name/value errors, a broken `input.json` in `check`; `play_step`/`play_scene` with held keys), `sim_step input` examples in the parity test. 400+ tests in total.

### WP5 — replay recordings: the `replay` tool, `wen replay`

- **Format (a change from §9, `replays.rs`):** `replays/<name>.replay.jsonl`, one object per line: `header` (`format`, `scene`, `scene_hash`, `seed`, `tick_rate`, `code` = the game module's fingerprint, `ticks`), one `input` line for every input change (`tick`, `values`: action → [x, y]; valid from that tick on), a single `hashes` line (**the 64-bit truncation of the world hash after every tick**, 16 hexadecimal digits per tick). The plan's "a checkpoint every 60 ticks + running tick by tick again after a drift" was **removed**: because the original world is not at hand, only the hash of *every tick* finds the first drifting tick (10,000 ticks = 160 KB). The name is restricted (`a-z 0-9 _ -`, ≤ 40), the file is always under `replays/` (no path escape), writing is atomic, at most 54,000 ticks (a quarter of an hour).
- **Recording:** `Session::recording: Option<Trail>`; `replay record` starts when the session is at **tick 0** (`replay_not_at_start` + what to do), and `step_app` adds the input change and then the world hash at every tick (zero cost when not recording). Going back to a snapshot also rewinds the recording (`Trail::rewind`: everything after that tick is deleted, the input state is restored), so "wrong turn, undo, continue" becomes one consistent recording (tested).
- **Playback (`replay run`):** opens a new session (the scene + seed in the header), applies the same input at the same ticks, **compares the hash after every tick**; `passed`, `ticks_run`, `final_hash`; if there is drift, `diverged_at` (the first drifting tick), `expected_hash`/`actual_hash`, and **the session is left open at that tick** (look with `entity_get`/`entity_query`, close with `sim_stop`). If it passes, the session closes (`keep` holds it). If the scene file changed, `replay_scene_changed`, and if the game code is different, `replay_code_changed` are **warnings, and playback still happens**: this is the answer to "I changed the code, is the game still the same?". MCP cancellation (`notifications/cancelled`) cuts playback at a tick boundary (`stopped_by: cancelled`).
- **`wen replay <name>`** (a shortcut for `replay run`; exit code ≠ 0 on failure) and `wen call replay ...`; because recording and saving need a session, they go through `wen do` or MCP.
- **Evidence (exit criterion: "10,000 ticks, no hash drift"):** `script_tools.rs::a_ten_thousand_tick_replay_...`: a game with the SDK, 10 × 1000 ticks with five different input patterns, `save` (10,000 ticks), `run` → `passed`, the final hash is the same as the recording; it passes again **in a separate workspace** (a reopened project, the module loaded from disk); when the game's walking speed is raised 1.5 times, `diverged_at: 0` + `replay_code_changed` + an open session. The parity test found the save/play/not-found examples for `replay` byte-for-byte equal as in-process = `wen do` = MCP.
- **Note (a departure from the plan):** the replay format and playback are in `wish_control` instead of `wish_test` (it has direct access to sessions and the world); `wish_test` comes with scenario tests in WP6 and uses this.

### WP6 — scenario tests: `test_run`, `wen test`

- **Format (`scenarios.rs`):** `tests/**/*.test.json`: `format: 1`, `scene`, `seed`, `steps`. Step types: **run** (`frames` or `until` + `max_frames`, an optional `input` — the same input format as `sim_step`, the same validation), **`expect`** (M4's expression language; it passes if it is true on the world resources or on any entity, it is narrowed to a single entity with `entity`, `message` is shown on failure), **`capture`** (camera, size, `golden`, `tolerance`/`fraction`, `view`). An unknown key / broken step is rejected with `invalid_test_file` + the step path (`/steps/3/jump`) and appears in the result of the test as a `setup` error (the other tests run).
- **Running:** every test opens its own session (scene + seed), runs the steps in order, **stops at the first failed step**, and closes the session. A failure = `step`, `kind` (`expect`, `until_not_reached`, `system_failed`, `capture`, `golden_missing`, `entity_missing`, `setup`, …), `tick`, `message`, and **the values the expression read** (`Expr::path_values`: like `Transform.translation[2]: -2.0000007`; first the world, then the first 5 entities that have the value). If a system fails while running (`system_failed`, an inspector finding at error level), the test counts as failed.
- **Image comparison:** a `capture` step calls `capture_frame` **with the software adapter** (forced at the start of the step: golden images must be the same across machines, ADR-017), and compares with `tests/golden/*.png` using `RgbaImage::diff` + `Tolerance` (default channel difference 2, pixel ratio 0.1%). If there is a difference, the new picture is written to `.wish/test-output/<test>-step<N>.png` and it says how many pixels differ; if there is no golden, `golden_missing` ("look first, if it is right `update_golden`"); `update_golden` writes the pictures as golden, and `dry_run` writes nothing. Golden/output paths are only `.png` files inside the project (the `out` rule of capture_frame).
- **Tool and command:** `test_run` (the 23rd tool: `paths`, `update_golden`, `dry_run`; if `passed` is false, a test failed), `wen test [files] [--update-golden]` (exit code ≠ 0 on failure, "ok / FAILED + values" text for people). `wen new` now comes with `tests/boots.test.json` (the scene runs 60 ticks and there is a camera). `scripts/`, `tests/` and `replays/` **are not counted as assets** (`wish_asset::NON_ASSET_FOLDERS`; `asset_list` and `asset_move` do not see them).
- **Decision — applying `edit_apply` to a session was taken out of M5** (carried over from M4): the path the agent writing game code needs already exists (edit the scene → new session; edit the code → hot reload), and translating `EditOp` to the live world (15 operations, `SceneId` naming, prefab instances) is a job in itself and needs its own design. It will be handled in M8a (the editor's "edit while playing"); not on the M11 list, but an **M8a input**.
- **Tests:** `script_tools.rs`: four scenarios (passing, failing `expect`, `until` not enough, broken file) all with the correct explanation, path selection and the "Did you mean" error, files that do not exist; golden: missing → `dry_run` does not write → create → passes → when an asset changes, "% pixels differ" + output file → new golden. 401 tests in total.

### WP7 — boundary cost benchmark, embedded toolchain package

- **`wen bench --script`** (`wish_cli/src/bench_script.rs`): the same system (move every entity by its velocity) as a native system and as game code compiled the way `script_build` compiles it, at 1 / 1,000 / 10,000 entities; also a read-only system and an empty system (fixed cost per tick). Measurement (development machine, release, 600 ticks, `opt-level=2`):

  | entities | native | game code (read+write) | boundary | per entity | read only |
  |---|---|---|---|---|---|
  | 1 | 0.0002 ms | 0.046 ms | 0.046 ms | — | 0.042 ms |
  | 1,000 | 0.0017 ms | 0.20 ms | 0.20 ms | ~199 ns | 0.16 ms |
  | 10,000 | 0.014 ms | 1.87 ms | 1.86 ms | **~186 ns** | 1.19 ms |

  Fixed cost per tick (fresh instance + one call): **0.04 ms** (0.24% of the budget; the 5% trigger of ADR-025 is far away). **Worst boundary: 11.2% of the frame budget at 10,000 entities → below the 20% trigger of ADR-011**; shared memory views were not needed.
- **Finding and fixes:** the first measurement showed ~540 ns per entity (32% of the budget — above the trigger). (1) The host was reading and writing values entity by entity (position lookup and validation every time): **bulk** read/write **per archetype column** was added to `wish_ecs` (`World::archetype_column_bytes`, `World::write_archetype_column`: only changed rows are written, validation only if the type has fields that need it, a rejected value never changes the column). (2) Game code was compiled with `opt-level=1`, and the SDK's generic query code was expanded inside the game at that level: **`opt-level=2`** (build time for small code did not change: 0.5 s). Result ~186 ns/entity. The gap with the raw measurement in WP1 (13–15 ns, hand-written ABI) is the SDK's per-value `Plain::decode/encode` work and the growing/zeroing of guest memory at every tick; **note for M6**: see types without padding directly as slices, reuse the frame memory within the tick.
- **Embedded toolchain (`wish_script::build::pack_toolchain`):** from the pinned rustc only the compiler + its libraries (`bin/rustc`, `rustc_driver`), `rust-lld` + `gcc-ld/wasm-ld` and the wasm32 standard library; no pdb, no cargo, no rustup, no host std; `toolchain.json` (version, size). **417 MiB** (unpacked). The compiler binaries link the C runtime statically (no `VCRUNTIME` in the import table).
- **Proof (automatic):** `wish_script/tests/toolchain.rs` (`#[ignore]`, **runs in CI on every push**): installs the package, `build_game` builds in an empty environment with only the package's `bin`, and the module is played (the score is correct). Locally: first build 8.8 s (the first load of the newly copied dll), later ones < 1 s.
- **`wen package [--out]`:** `wen.exe` + `sdk/` + `toolchain/` + **`clean-test.wsb`** (Windows Sandbox: the package is read-only, network off, the result folder is writable, a script at startup) + `clean-test.ps1` (records the absence of development tools, `wen new` → `wen build` → edit → `wen build` → `wen test` → `capture_frame`; the log and `result.json` go to the result folder). The packaged `wen` must be built with the **static C runtime** (`RUSTFLAGS=-C target-feature=+crt-static`); `wen package` reads the PE import table of the running `wen.exe` and warns `package_needs_static_crt` if it sees `VCRUNTIME*.dll` (the false alarm of the text search in the first attempt was fixed). Package: 513 MB.
- **Local empty-environment trial:** with `env -i` (no PATH, cargo, rustup, VS variables) the `wen` in the package: `new` → `build` 7.49 s → after an edit 0.39 s → `test` passed → `capture_frame` wrote a picture.
- **Remaining (user step, ADR-023 exit criterion):** a trial on a real clean Windows — turn on the optional "Windows Sandbox" feature (a Windows setting; virtualization must be on), then double-click `C:\wish-build\package\clean-test.wsb`; the result lands in `C:\wish-build\package-result\`. This will show that it also works on a machine without the VC++ runtime and a GPU driver.

### WP8 — mini game example, SDK document, game code evaluation tasks

- **`examples/collect`** (in the repo, a real project): a player walking with WASD/arrows, three spinning gold coins, `Coin`/`Player` components, `Score`/`GameState` resources, a `Collected` event, `after` ordering, deleting coins with `Commands`, a win log with `info!`. The scene was built with tools (`edit_apply`, `euler_deg`), and `check` is clean. `tests/win.test.json` plays the game with input and **wins at 129 ticks** (three coins with `until` steps, `count(Coin) == 0`, the win persists). In CI: `script_tools.rs::the_collect_example_...` copies the example, builds it with no warnings, passes its two tests, records a win played by hand with the tools, and plays the replay back bit for bit. By hand: `wen play --root examples/collect` (first `wen build --root examples/collect`).
- **Two fixes from the example:** (1) **the game's resources exist with their defaults when the module is set up** (`Score` did not exist at tick 0, and `expect Score.points == 0` failed; the scene's values still take priority). (2) **When the project was outside the repo, `wen build` picked the wrong compiler** (the rustup proxy in PATH went to the default toolchain, which has no wasm32 target): the SDK package now records the path of the compiler that built it (`sdk.json` → `rustc_path`) and that one is used in the development environment; `pack_sdk` calls cargo and rustc from the engine's folder (so that the pinned toolchain is chosen).
- **`wish://docs/sdk`** (an MCP resource, `docs_search` finds it too): data definitions, field types, system parameters (`Query`, `Res`, `ResMut`, `Actions`, `Commands`, `EventReader/Writer`), `export!`, rules (including "forward = −Z"); the guide and the MCP `instructions` point to it.
- **Evaluation tasks 11–12** (`wish_evals/src/game_tasks.rs`): the agent can read and edit files (never Bash), checked by a hidden scenario test (the README above). **`wish-evals selftest` 12/12** (every check fails at the start and passes with the reference solution; free). **No real run was made:** it spends subscription quota and waits for the user's approval (leg b of the exit criterion).

### WP9 — hostile game modules, independent closing review

- **Hostile end-to-end tests** (`wish_script/tests/hostile_game.rs`, WAT modules):
  - **Coverage:**
    - lying manifests: 600 random corruptions; the accepted ones are set up and played;
    - broken write-back;
    - random or giant command buffers: 120 structure-aware fuzzed states, each run twice (checked to be deterministic);
    - writing to managed data.
  - **Checked in every case:** the app continues; world snapshot → restore → same hash; all entities can be dumped to JSON.
  - **What they found and fixed:**
    - NaN and infinite floats were silently entering the world. They are now rejected with `script_not_finite`; the diagnostic names the field.
    - A single `memory.fill` produced 3 million spawns per call. Now at most 65,536 commands per call (counted while decoding); game code cannot grow the world beyond 4,194,304 entities and 1 GiB of component data.
    - The total size of manifest types was unlimited. Now at most 16 MiB.
    - A command that could not be applied was shutting down the system. Now it is skipped and written to the log as a warning (the same as the native command buffer); despawning an entity that is already gone is not even counted as an error.

- **Independent closing review:** two reviewers worked in parallel (runtime/sandbox; tools/files). **20 findings** came out, all fixed.
  - **Regression tests:**
    - `wish_control/tests/review_m5.rs` (14 tests);
    - `wish_cli/tests/review_m5.rs`;
    - `hostile_game.rs` (`r2_…`, `r3_…`);
    - `source_check.rs` unit tests.

#### High-severity findings

- **T1 — a negated `expect` always passed.**
  - **Problem:** an `expect` with no entity name passed "if resources or any entity satisfy it".
    - `!has(Coin)` was always true, because a resource subject has no component.
    - `!(Camera.fov > 10)` was true on an entity with no camera.
    - So the "no coins left" step of the example game and of the evaluation check was checking nothing.
  - **Fix:**
    - The expression language moved to **three-valued logic**: a comparison with a missing value stays unknown, also under `!`.
    - A new `count(Component)` function was added.
    - An `expect` that negates `has()` without an entity name is rejected (`expect_ambiguous`, hint: `count(Coin) == 0`).
    - The example game's test and the hidden check were fixed.
- **T2 — game code could read the machine during the build.**
  - **Problem:** with `include_str!`, `include_bytes!`, `include!`, `env!`, `option_env!` and `#[path]`, any file and environment variable on the machine could be read and carried into `logs_get`.
  - **Fix:**
    - Source check before the build (`wish_script::source_check`): a lexical scanner that knows comments and strings. These names are forbidden everywhere, `path` in attributes and `#[$…]` are forbidden; links inside `scripts/` are forbidden.
    - `HOME`/`USERPROFILE` are not given to the compiler.
    - `.wish/build` cannot leave the project.
- **R1 — two types with the same name were mixing values.**
  - **Problem:** types with the same name in two modules (`player::State`, `enemy::State`) shared a single layout, and values were silently read wrongly.
  - **Fix:** the SDK also writes the clashing type to the manifest, and the host rejects it (`script_duplicate_type`).

#### Medium-severity findings

| Finding | Problem | Fix |
|---|---|---|
| T3 | A module that could not be used (for example one clashing with an engine type) replaced `game.wasm`. | It is built into a side file and checked, and only then put in place; `last_build` is honest. |
| T4 | The compiler had no time limit; an infinite `const` computation froze the MCP server. | 120 s limit, `script_build_timeout`. |
| T5 | `wen replay <name>` and `wen test <file>` did not accept a positional argument. | The positional argument is accepted. |
| T6 | The replay header read the code and the scene at the end of the recording. | They are captured at the start of the recording; new code during recording stops the recording. |
| T7 | Going back to a snapshot ahead of the recording shifted the recording. | The recording stops (`recording_stopped`). |
| T8 | `wen sdk/package --out` deleted the given folder unconditionally. | It is deleted only if empty or if wen made it (`output_exists`). |
| R2 | The layout of a giant event was recomputed at every `Send`, and the schema (including the default value of the array) was copied: 65,536 sends ≈ 1 hour. | The type layout is computed once per runtime, and the size is checked first: 0.26 s. |
| R3 | The `set_parent` chain was unlimited; every link walked the ancestors. | Depth at most 256 (`hierarchy_too_deep`). |
| R4 | Two writable parameters on the same data undid each other's changes. | Only rows different from what was sent are written; all are checked first (atomic). |
| R5 | While the systems of a tick shared a single instance, frames were never freed: 20 readers × 50k entities ran out of memory. | The guest reuses the frame and command buffer within the tick; if `wish_alloc` returns 0 it counts as out of memory; the location is the game's line, not the SDK file. |
| R6 | After a hot reload that changed a type, snapshots could not be restored. | The session's snapshots are moved to the new layout (`App::rebase`). |
| R7 | Game code had no memory budget in bytes. | Component data at most 1 GiB. |
| T9/R9 | A game system carrying an engine system's name silently disappeared. | It is rejected (`script_system_name_taken`). |

#### Low-severity findings

- **R8:** `Section` was `Copy` and `FrameReader::from_buffer` was a safe function; this could lead from safe code to memory aliasing and use of freed memory. Now `Section` has a single owner, `from_buffer` is `unsafe`; Miri is clean.
- **T10:** The scene path of a replay and the `test_run` paths could point outside the project. They are now checked.
- **T11:** `test_run` left the software adapter on after a capture step; a broken `input` was silently ignored. Now the adapter choice is restored, and a broken `input` gives `invalid_parameter`.

#### Other notes

- **Boundary cost measured again** (after the checks were added): ~240 ns/entity, **14%** of the frame budget (trigger 20%). The first fix attempt had gone up to 21% because of per-row memory allocation; it was fixed.
- **Known instability (outside M5):** two tests in `wish_render/tests/models.rs` gave a black image once while Miri was running at the same time. Alone they passed 8/8 twice; the crate was not touched. In M6 the GPU sharing of the render tests will be looked at.

## 18. M5 result

**Completed — 16 November 2026.** Two verifications wait for a user step (below).

| Exit criterion | Evidence | Result |
|---|---|---|
| Claude writes a mini game with MCP + file edits and proves it with a scenario test | (a) `examples/collect`: a scene built with tools, game code, a test that wins at 129 ticks; built, tested, recorded and replayed in CI. (b) Evaluation tasks `11_walk_code` and `12_collect_game`: game code is written with file tools, checked by a hidden scenario test; **`selftest` 12/12**. A real run spends subscription quota. | ◐ **waiting for the user's approval** (`wish-evals run`) |
| Code change → running game < 10 s, state is kept | `wen build` 0.4 s after an edit. Hot reload keeps the state and the random stream of the systems; schema migration by field name (`script_tools.rs`, `game.rs`). Snapshots are also moved to the new layout (R6). | ✔ |
| Infinite loop / trap does not freeze or crash the engine; it produces a diagnostic | `hostile.rs` (fuel, trap, stack, memory, panic, forbidden import); `hostile_game.rs` (manifest, command and write-back fuzzing, flood); build-time limit and source check (`review_m5.rs` T2, T4). | ✔ |
| Replay of 10,000 ticks with no hash drift | `script_tools.rs::a_ten_thousand_tick_replay_…` (also in a separate workspace). | ✔ |
| Boundary cost benchmark recorded (ADR-011) | `wen bench --script`: 14% (trigger 20%). | ✔ |
| Embedded toolchain builds and runs on a clean Windows | The packaged `wen` (static CRT) in an empty environment (`env -i`): new → build 7.5 s → edit 0.4 s → test → capture. CI has a packaged toolchain step. Package: `C:\wish-build\package` + `clean-test.wsb`. | ◐ **waiting for a user step** (Windows Sandbox) |

- **Numbers:** 23 tools, 6 new crates (`wish_script`, `wish_sdk`, `wish_sdk_macros`, `wish_types`, `wish_input`, `wish_evals`; tasks 11–12 were added to the last but one), 430+ tests.
- **Pending user steps:**
  1. `wish-evals run` (spends quota; approval needed).
  2. Turn on Windows Sandbox and run the file `C:\wish-build\package\clean-test.wsb` (the result lands in `C:\wish-build\package-result`).

  If either brings out a problem, the fix is made on a branch opened from `main`.
- **Carried over to M6:**
  - moving SDK value decoding/encoding down to a slice view for types without padding (boundary cost);
  - measuring the cost of `until`;
  - GPU sharing of the render tests.
- **Carried over to M8a:** applying `edit_apply` to a live session.
