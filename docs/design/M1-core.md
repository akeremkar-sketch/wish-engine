# M1 Design Note — Core

Scope: `wish_core`, `wish_schema`, `wish_ecs`, `wish_app`, `wish_cli`.
Exit criteria: the M1 section of [ROADMAP.md](../ROADMAP.md). Related decisions: ADR-003, 004, 005, 006.

This note fixes **how** M1 is built. If a decision made here turns out wrong during implementation, this note is updated first, then the code.

## 1. Core decision: components are plain data

Every type that enters the world (component, resource, event) is **plain data**: `Copy`, fixed size, no pointers, no destructor (drop).

Consequences:

| Consequence | Why it matters |
|---|---|
| A snapshot is a byte copy | Rewind, forking and replay are cheap (ADR-006) |
| The host and the Wasm guest see the same bytes | No conversion layer (ADR-011) |
| Storage is only byte arrays | The `unsafe` surface is small, no class of destructor bugs |
| The world hash is defined over the data | Determinism can be verified (ADR-005) |

### Field types supported in M1

| Type | Size / alignment | JSON form |
|---|---|---|
| `bool` | 1 / 1 | `true` / `false` |
| `i8`…`i64`, `u8`…`u64` | 1–8 / 1–8 | number |
| `f32`, `f64` | 4, 8 | number (must be finite) |
| `Vec2`, `Vec3`, `Vec4` | 8, 12, 16 / 4 | `[x, y]`, `[x, y, z]`, `[x, y, z, w]` |
| `Quat` | 16 / 4 | `[x, y, z, w]` |
| `Mat4` | 64 / 4 | array of 16 numbers (column-major) |
| `Color` (linear RGBA, `f32`) | 16 / 4 | `[r, g, b, a]` |
| `Entity` | 8 / 4 | `"12v3"` at run time; scene id in a scene file (M2) |
| `Text` (handle into the world text table) | 4 / 4 | text |
| C-like `enum` (`repr(u8)`) | 1 / 1 | variant name |
| `[T; N]` | N × T | array |
| Nested schema struct | — | object |

**The largest alignment is 8**; the schema rejects anything larger. This lets columns be built on `Vec<u64>` (§4).

**Deliberately postponed:** `List<T>` (variable length), `Opt<T>`, enums that carry data, `AssetRef` (M2, with the asset server), `Mat3`. The hierarchy does not need a list (§4.6).

### Math types

`glam` is used with the **`scalar-math`** and **`libm`** features (the feature names will be verified with `cargo` in WP3):

- `scalar-math`: SIMD off; `Quat`/`Vec4`/`Mat4` are plain `f32` structs (alignment 4). A SIMD `Quat` has alignment 16 and causes padding in structs.
- `libm`: trigonometry etc. come from pure Rust, not from the platform's C library; this brings us closer to the cross-platform determinism goal (D2).

Cost: simulation math does not use SIMD. The render side can use its own SIMD path if needed (outside the determinism boundary). The M1 benchmark will show whether this is enough.

## 2. Storage layout

**Decision: the storage layout is the type's `#[repr(C)]` Rust layout.** Columns are byte arrays; native Rust code looks at them as `&T` / `&mut T` with no conversion.

Options considered:

| Option | Pro | Con | Result |
|---|---|---|---|
| **A. `repr(C)` layout = storage** | Zero-copy access; familiar query API; the schema comes free with `offset_of!` | Padding bytes can hold undefined content | **Chosen** |
| B. The schema's own packed layout | No padding; raw byte hash is always valid | Field-by-field copying on every access; wrappers that write back instead of `&mut T` | Rejected |
| C. Forbid padding (like bytemuck `Pod`) | Raw byte hash is always valid | The agent/human must deal with field order; `bool` and enums cannot be used | Rejected |

**Is the `repr(C)` layout the same across platforms?** For the allowed types, yes: on x86_64, aarch64 and wasm32, `u64`/`f64` have alignment 8; `usize`, pointers and `u128` are forbidden. The schema carries the offsets explicitly; when the guest SDK is loaded, it compares its own layout with the schema, and a mismatch produces a diagnostic.

### Padding bytes

This is the cost of option A. Two rules:

1. **Padding is never interpreted.** A snapshot copies padding raw (it does not look at the content).
2. **The hash skips padding.** The world hash is computed field by field, following the schema. Types that have no padding and contain no `Text` are marked at registration (`raw_hashable`), and their columns are hashed raw in one call — the fast path. `Transform` (Vec3 + Quat + Vec3 = 40 bytes, no padding) is on the fast path.

### Validity (trust boundary)

A `bool` can only hold 0/1 and an enum can only hold a defined variant value; otherwise the native `&T` view is undefined behavior. So **untrusted bytes are validated against the schema before they enter a column**: values coming from JSON (valid by construction) and, later, columns that the Wasm guest writes back (M5). What native Rust code writes through `&mut T` is valid by construction.

## 3. `wish_schema`

### Data model

```rust
pub struct TypeSchema {
    pub name: String,          // "Transform"
    pub doc: String,
    pub size: u32,
    pub align: u32,
    pub kind: TypeKind,
}

pub enum TypeKind {
    Bool, I8, I16, I32, I64, U8, U16, U32, U64, F32, F64,
    Vec2, Vec3, Vec4, Quat, Mat4, Color,
    Entity, Text,
    Enum(Vec<VariantSchema>),            // name, value, doc
    Array { elem: Box<TypeSchema>, len: u32 },
    Struct(Vec<FieldSchema>),
}

pub struct FieldSchema {
    pub name: String,
    pub doc: String,
    pub offset: u32,
    pub ty: TypeSchema,
    pub unit: Option<String>,            // "m/s"
    pub range: Option<(f64, f64)>,       // closed range
    pub default: Option<serde_json::Value>,
}
```

### Contract and macro

```rust
/// # Safety
/// The type must be `repr(C)`, `Copy`, pointer-free, and `schema()` must describe the real layout.
pub unsafe trait Plain: Copy + 'static {
    fn schema() -> TypeSchema;
    fn default_value() -> Self;              // the single source of the default
    fn encode(&self, out: &mut [u8]);        // safe; writes padding as zero
    fn decode(bytes: &[u8]) -> Option<Self>; // safe; invalid bool/enum → None
}
pub trait Component: Plain { const NAME: &'static str; }
pub trait Resource: Plain { const NAME: &'static str; }
pub trait Event: Plain { const NAME: &'static str; }
```

The `#[component]` attribute macro (and `#[resource]`, `#[event]`, and `#[schema]` for nested structs/enums) generates:

- `#[repr(C)]` + `Clone, Copy` (`repr(u8)` for an enum)
- `unsafe impl Plain` — offsets with `core::mem::offset_of!`, size/alignment with `size_of`/`align_of`; no hand-written numbers
- `Debug`, `PartialEq` (and `Eq` for an enum) and `impl Default`. The default comes from `#[schema(default = …)]` expressions; it can be **any Rust expression** (`default = Vec3::ONE`). The JSON default in the schema is not written by hand: it is computed by `encode`-ing the Rust value and converting it to JSON with the schema — a single source of truth.
- Compile-time checks: every field type is `Plain`; alignment ≤ 8
- `///` comments → `doc`; `#[schema(unit = …, range = …, alias = …)]` → schema
- `alias`: a name that humans/agents would try instead of the real name (`position` for `translation`). It is **not accepted** as input; it only makes the "did you mean" hint exact (the one-right-way principle)
- Structs with no fields (`struct Player;`) are valid: zero-size "tag" components
- The default of an enum is its first variant

**Why `encode`/`decode`?** Because of these two safe, field-by-field methods: defaults come from a single source; adding/reading a component needs no `unsafe` (`unsafe` is left only to the zero-copy `&T` views); untrusted bytes are validated in a type-safe way with `decode`. The hot path (query iteration) does not use them.

The macro is a separate crate (`wish_schema_macros`), because procedural macros in Rust must be a separate crate.

### JSON ⇄ bytes

Entirely with safe code (`from_le_bytes`/`to_le_bytes` over byte slices), following the schema:

- Missing field → the schema default (zero if there is none).
- Unknown field → `unknown_field` diagnostic, with a "did you mean" suggestion.
- Number out of range → `value_out_of_range`; non-finite number → `non_finite_number`.
- For `Text` fields, the converter takes a text store context.

### JSON Schema and fingerprint

- A JSON Schema (2020-12) for every type: `description`, `minimum`/`maximum`, `default`, `x-unit`, `additionalProperties: false`.
- **Registry fingerprint:** the stable hash of all registered schemas (name, type, offset, size). Snapshots carry this fingerprint; restoring into a different set of types is rejected.

## 4. `wish_ecs`

### 4.1 State and registry split

```rust
pub struct World {
    registry: Registry,     // type info; only appended to after startup
    state: WorldState,      // all dynamic state
}
#[derive(Clone)]
struct WorldState { tick, entities, archetypes, resources, events, texts, rng }
```

**A snapshot is a copy of `WorldState`.** This split structurally prevents the "state that was forgotten in the snapshot" bug: everything dynamic is inside one struct.

### 4.2 Entities

`Entity { index: u32, generation: u32 }` (in `wish_core`, because the schema uses it too). The empty entity is `Entity::NULL`. Allocator: a generation array + a **LIFO** free list (deterministic). An entity → (archetype, row) location table.

### 4.3 Archetype tables

- An archetype = a sorted set of `ComponentId`s. Archetypes are numbered **in creation order** and iterated in that order.
- Archetype lookup is a `BTreeMap<Vec<ComponentId>, ArchetypeId>` (no iteration over a hash map).
- Column: `Column { words: Vec<MaybeUninit<u64>>, item_size, len }`. `u64` words guarantee alignment 8 safely; there is no manual memory allocation (`alloc`). The reason for `MaybeUninit` is in the §12 WP4 note.
- Each column keeps a `changed_tick: u32` per row (change tracking). Change marks affect system behavior, so they are part of the state and enter the hash.
- Removal: `swap_remove` (deterministic; the same operation sequence produces the same row order).

### 4.4 Access

- **Typed query:** `world.query::<(Entity, &Position, &mut Velocity)>()`, with the filters `With<T>` / `Without<T>`. Access is checked when the query is built (conflicting writes to the same component → diagnostic; unregistered component → diagnostic). `&mut T` access is given through a `Mut<T>` wrapper; the change mark is set only when something is really written.
- **Dynamic access:** a byte slice by `ComponentId`. JSON/MCP and, later, Wasm use this path.
- **Structural changes:** directly (`world.spawn`, `insert`, `remove`, `despawn`) or, during iteration, through a `Commands` buffer; the buffer is applied **in recording order** after each system.

### 4.5 Resources, events, texts

- **Resource:** one value per type, plain data, stored as bytes.
- **Event:** plain data; double-buffered. An event written at tick N is read for the rest of tick N and at tick N+1. The buffers are part of the state.
- **Text table:** an append-only table that merges duplicates; `Text(0)` is the empty text. The hash is computed over the content of the text, not over the handle number.

### 4.6 Hierarchy

A linked structure that needs no list, a single component:

```rust
#[component]
pub struct Hierarchy { parent: Entity, first_child: Entity, last_child: Entity,
                       prev_sibling: Entity, next_sibling: Entity }
```

It changes only through `world.set_parent` / `world.remove_parent` (the fields are read-only from outside). Child order is insertion order. An attempt to create a cycle produces a diagnostic. When an entity is despawned, its subtree is despawned too.

## 5. Time, scheduling, randomness

- **Tick:** the `Time { tick: u64, delta: f32 }` resource; 60 Hz by default. Simulation code cannot reach the wall clock.
- **System (M1):** `name` + `fn(&mut World) -> Result<()>` + `before`/`after` constraints. A system that returns an error is disabled, a diagnostic is recorded, and the world moves on (the same as the Wasm trap behavior in M5).
- **Order:** a topological sort by the constraints; among the ready nodes the **smallest by name** is chosen. The result is a single total order, independent of registration order. A cycle → diagnostic (with the names of the systems in the cycle).
- **Execution:** single thread, in order. Parallelism is outside the scope of M1; when it is added, it must produce the same hash.
- **Randomness:** `xoshiro256++` written in-house, seeded with `SplitMix64` (so there is no risk that the sequence changes when an external crate version changes; tested with reference vectors). Each system has its own stream: `stream = f(world seed, stable hash of the system name)`. ~~The stream states are inside `WorldState`.~~ **Changed in WP5:** the stream states, the systems' "last run" ticks and the disabled flags are the state of the scheduler (`App`); so a snapshot covers the scheduler together with the world (§6).

Deliberately postponed: Bevy-style parameter injection (`fn sys(q: Query<…>, t: Res<Time>)`). In M1, systems take `&mut World`; the ergonomic layer will be added in M5, together with the access declaration, when the guest SDK is designed.

## 6. Snapshot and world hash

**Snapshot (current form after WP5):** two layers. `world.snapshot()` = a copy of `WorldState` + the registry fingerprint; `app.snapshot()` = the world snapshot + the tick counter + the `SystemState` of each system (random stream, last run tick, disabled flag) + the fingerprint of the system list. Everything that determines the future is in the second one. The first plan was: `world.snapshot() -> Snapshot` (a copy of `WorldState` + the registry fingerprint). `world.restore(&snapshot)` returns a diagnostic if the fingerprint does not match. It is taken only at a tick boundary. In M1 it is in memory; the on-disk format comes in M4/M5.

**World hash:** `world.hash() -> Hash128` (XXH3-128). In order: the fingerprint, the tick, the entity allocator (generations + free list), each archetype (component ids, row count, entity column, the data and change marks of each column), resources, event buffers, random streams. Sections are separated by a tag and a length.

What it guarantees: **same hash ⇒ same future** (with the same inputs). That is why everything that affects future behavior (including the free-list order) enters the hash.

What it does **not** guarantee: that two "semantically equal" worlds reached with different histories give the same hash. This is a state hash, not semantic equality. Equality at the scene-file level comes from the canonical format of M2.

## 7. The `unsafe` boundary

| Place | What for |
|---|---|
| `wish_schema` | Only the `unsafe trait Plain` declaration (the contract) |
| Output of `wish_schema_macros` | `unsafe impl Plain` (generated code, offsets from the compiler) |
| `wish_ecs` | Byte column ⇄ `&T`/`&mut T` conversion, query iterators |

Everywhere else `unsafe_code = "deny"` (workspace lint). **The rule in CLAUDE.md was updated according to this note:** `wish_schema` was added to the list for the contract declaration.

Safeguards: (1) a `// SAFETY:` justification in every `unsafe` block; (2) `wish_ecs` tests run under Miri (needs the nightly toolchain, installed in WP4); (3) model-based property tests.

## 8. Command line (M1)

Since there are no scene files (M2) or scripts (M5) in M1, `wen` runs built-in example worlds:

```
wen run --example bounce --headless --frames 10000 --seed 7 --json
  → { "tick": 10000, "world_hash": "…", "entities": 1000, "elapsed_ms": … }
wen schema --json          → the JSON Schema of all registered types
wen bench ecs --entities 100000 --ticks 600
```

`elapsed_ms` is wall-clock time and is measured only in the command-line layer; it never enters the simulation.

## 9. Test plan

| Exit criterion | Test |
|---|---|
| Two runs give the same hash | The example world, 10,000 ticks × 2; hash comparison every 100 ticks |
| Snapshot → restore → continue = uninterrupted | Snapshot at tick 5,000; the two paths are compared at tick 10,000 |
| JSON round trip is lossless | Property test: random value → JSON → bytes → JSON |
| JSON Schema generation | Golden output for the example types |
| Miri clean | `wish_ecs` unit tests, with small sizes |
| ECS correctness | Model-based property test: random spawn/despawn/insert/remove/set sequences are compared with a simple reference model |
| Ordering | Registration order is shuffled and the execution order does not change; a cycle produces a diagnostic |
| Randomness | Reference vectors; independence of the streams |
| Benchmark | 100 thousand entities, a simple movement system, the tick time (release) is recorded |

## 10. Work packages and recommended model

| # | Work package | Model · effort | Status |
|---|---|---|---|
| WP1 | Workspace, lints, licenses, CI, git | Sonnet 5.5 · medium | Done |
| WP2 | `wish_core`: `Entity`, `Diagnostic`, stable hash, suggestion helpers | Sonnet 5.5 · medium | Done |
| WP3 | `wish_schema` + macro: schema model, JSON ⇄ bytes, JSON Schema, fingerprint | Opus 5.5 · high | Done |
| WP4 | `wish_ecs`: storage, typed and dynamic access, queries, resources, text table | Opus 5.5 · high | Done |
| WP4b | `wish_ecs`: deferred commands, events, hierarchy, change tracking | Opus 5.5 · high | Done |
| WP5 | Scheduler, fixed step, randomness (`wish_app`) | Opus 5.5 · high | Done |
| WP6 | Snapshot and world hash | Opus 5.5 · high | Done |
| WP7 | `wish_cli`: `wen run`, `wen schema`, `wen bench` | Sonnet 5.5 · medium | Done (`wen bench` in WP8) |
| WP8 | Determinism tests, Miri, benchmark, M1 closing review | Opus 5.5 · high | Done |

## 11. Open questions (to be closed during implementation)

1. ~~**`f32` JSON round trip.**~~ **Closed in WP3.** When writing, the shortest decimal form of the `f32` is used (`0.1` appears in JSON, not `0.10000000149011612`); when reading, the decimal text of the number is parsed directly as `f32` (no double rounding). Also, the `float_roundtrip` feature of `serde_json` was turned on: the default parser could be off in the last digit in `f64`. Property tests verify that for every finite `f32` and `f64`, the bytes → JSON → text → JSON → bytes path gives a bit-identical result.
2. **Are the disabled-system flags** part of the state? In M1 they are in the scheduler (outside the snapshot). They will be reconsidered in M5, together with the Wasm traps.
3. **Growth of the text table.** An append-only table can bloat in long sessions; compaction (during a snapshot), if needed, in M5.
4. **`scalar-math` performance.** If the WP8 benchmark shows it is not enough: plain `f32` structs in storage, conversion to SIMD types in computation.
5. **Repeated errors from the macro.** A field that is not plain data (like `String`) is reported at the right place with the right message; but the same error is repeated from three other places in the generated code (five messages for one error). The first message is clear enough; reducing the repeats and adding compile-error tests (`trybuild`) was left to WP8.
6. **`syn` version.** The macro was written with `syn` 2, while `serde_derive` brings `syn` 3. Both versions compile (a few seconds on the first build). The macro can be moved to `syn` 3 later.

## 12. Implementation notes

Decisions made during implementation, after the design, that affect later work packages.

### WP2–WP3

- **`Diagnostic` is one pointer wide.** Its content (`DiagnosticData`) lives on the heap; fields are reached directly through `Deref` (`diagnostic.code`). Why: its first form was 200 bytes and made every `Result<T>` that big, even with no error. Now `Result<()>` is 8 bytes. A test pins this.
- **Non-finite numbers.** Rejected when reading. When writing, the texts `"NaN"`, `"Infinity"`, `"-Infinity"` are produced; this way a corrupted world (for example a position that turned into NaN) can still be inspected.
- **`apply_json` is a patch.** Fields named in the object are written, the others stay as they are. To build a new value, use `from_json` (it starts from the defaults). The MCP `set` operation will be built directly on top of this.
- **Error location and name.** JSON conversion errors carry both a JSON pointer (`/waypoints/1/1`) and a readable name (`Mover.waypoints[1][1]`). The caller gives the place of the value in the document with `JsonContext::at_path`.
- **Schema validation** (`TypeSchema::validate`) always passes for schemas produced by the macro; its real purpose is to check, at the trust boundary, the schemas that will come from a Wasm module in M5.
- **Type-level defaults** are defined in two places and kept equal by a test: `Plain::default_value()` on the Rust side, `default_bytes` on the schema side (quaternion = identity, matrix = identity, color = opaque white, entity = `null`, enum = first variant, the others zero).
- **The fingerprint covers only the layout** (name, size, alignment, offset, enum values). The fingerprint does not change when the doc, unit, range, default or alias changes, because the meaning of the stored bytes does not change.

### WP4

- **Columns store `MaybeUninit`; padding is never read as bytes.** The first plan in §2 assumed "padding is always zero". This was wrong: when safe code assigns a whole value through `&mut T` (`*t = Health { .. }`), the compiler may leave the padding bytes uninitialized, and reading an uninitialized byte as `u8` is undefined behavior. Zeroing the padding afterwards was not safe either: `mem::forget` is safe code and can bypass the zeroing code. The solution:
  - For each type, the registry computes the **field spans** (`spans`): the byte ranges outside padding, merged.
  - Reading as bytes (`component_bytes`, JSON, and later the world hash) copies only these ranges; padding comes out as zero.
  - All value copies (moves between archetypes, `swap_remove`, and later the snapshot) are raw memory copies; this is defined for `MaybeUninit`.
  - So the two rules in §2 hold: padding is not interpreted; the hash skips padding.
- **All access goes through `&mut World`.** Including queries (`world.query::<Q>()` borrows the world mutably). The single-writer guarantee comes from the compiler; there is no run-time borrow check. When parallel systems are added (after M1), a `&World` path for read-only queries will be added.
- **Query access checking is done with `TypeId`**, not the component id: so a wrong query like `(&mut A, &A)` is rejected with the `conflicting_query_access` diagnostic even if `A` was never registered.
- **An unregistered component** matches nothing in a typed query (it is not an error); `With<T>` is empty, `Without<T>` passes unconditionally. With access by name (`set_json`), the `unknown_component` diagnostic and a suggestion of the nearest name are returned.
- **`set_json` is atomic.** The value is first applied on a copy; if it is rejected, the world does not change. If the component does not exist, it starts from the defaults; if it exists, it applies a patch. *(The closing review found that this claim was wrong for the text table; it was fixed in WP8, see the WP8 notes.)*
- **Resources** are single-row columns; the same typed, JSON and byte paths are used.
- **Postponed to WP4b:** the deferred command buffer (`Commands`), events, the hierarchy component, change tracking (`changed_tick` and the `Mut<T>` wrapper). These gain meaning together with the scheduler (WP5); it is safer as a separate, small work package.
- **No bulk insert.** Adding several components to one entity does an archetype move for each insert. If the 100-thousand-entity benchmark (WP8) shows this is not enough, a tuple-based `spawn_with` will be added.

### WP4b

- **Change marks are `u64`.** Each column keeps two marks per row: the "change tick" when it was added and when it was last written. With a per-system increment at 60 Hz, `u32` would overflow in a few weeks; `u64` does not overflow and comparisons stay simple. The world tick starts at 1; 0 is before everything.
- **The change tick and the simulation tick are separate concepts.** The change tick is only for the question "was it written after this?"; the scheduler (WP5) will advance it after every system.
- **`&mut T` now gives `Mut<T>`.** Reading sets no mark; writing (`DerefMut`) does. So in a loop you write `for mut position in world.query::<&mut Position>()?`. `Changed<T>` and `Added<T>` are row-level filters; they read the row's mark before the item is created. The tick to compare against is given with `query_since`; `query_filtered` uses 0.
- **Events** are double-buffered per type: an event sent in one update is readable in that update and the next; `update_events` drops the old half. Each event has a sequence number within its type; every reader with an `EventCursor` sees each event once, and a reader that is late skips the dropped ones.
- **Commands** (`Commands`) are applied in order; a command that fails is reported with its order in the buffer (`location.op`) and the others still run. An entity that will be created in the same buffer is referred to with `Target::Spawned(n)`. A typed insert stores a function pointer that knows the type; so no world access is needed when the command is recorded.
- **The hierarchy** is kept as a doubly linked list in a single component (`Hierarchy`: parent, first/last child, previous/next sibling); no list type is needed, and it stays plain data. The component is **managed by the engine**: it can be read (JSON included), but direct writing, inserting, removing, `get_mut` and a `&mut` query are rejected with the `managed_component` diagnostic; changes are made only with `set_parent` / `remove_parent`. Creating a cycle is rejected with `hierarchy_cycle`. Every walk that follows the links is bounded by the entity count; corrupt links cannot lock up the engine.
- **`despawn` also removes the subtree.** The model-based test verifies this and the sibling order with random `set_parent` / `remove_parent` / `despawn` sequences.
- **The suggestion algorithm was improved** (`wish_core::suggest`): swapping two neighboring letters now counts as a single error (`Hti` → `Hit`). The new event test caught this.

### WP5

- **`wish_core::Rng`** (xoshiro256++, SplitMix64 seeding) is the engine's only source of randomness. The published first output of SplitMix64 for seed 0 and the first two outputs of xoshiro256++ from the state [1,2,3,4] (also computed by hand from the algorithm) are fixed in a test; also a sequence of the derived methods (`below`, ranges) was recorded. `below` uses the unbiased Lemire method.
- **System:** `fn(&mut World, &mut SystemContext) -> Result<()>`. `SystemContext`: name, `Time`, the system's own `Rng`, `last_run` (the change tick at the end of the previous run; for the question "what changed since then" with `query_since`).
- **Order:** topological sort, the smallest by name among the free ones. Independent of insertion order (tested). If a constraint points to an unknown name: `unknown_system` + a suggestion; a cycle: `system_order_cycle` + the names in the cycle. In these two cases the tick does not run at all.
- **After every system** the change tick is advanced; **at the end of every tick** the event buffers rotate. The `Time` resource is written at the start of every tick.
- **A system that fails** is disabled (`system_failed`, which keeps the code, message, hint and location of the original diagnostic); the others keep running. `enable_system` turns it back on (later, after hot reload). A panic in native Rust systems is not caught: native code is trusted; untrusted code will run in Wasm in M5.
- **Time computations come from an integer rate.** `Time::at(tick, hz)`: `elapsed = tick / hz` (f64), `delta = 1/hz` (f32). `update(seconds)` accumulates in units of **ticks**, not seconds; so the `f32` rounding of 1/60 cannot shift the step count (0.05 s @ 60 Hz = exactly 3 ticks). The tests caught this. At most `max_steps_per_update` (default 8) ticks run in a single call; the rest are skipped and reported.
- **Determinism test:** 600 ticks with entities that spawn randomly, bounce and die randomly; the same seed gives the same world every 50 ticks, a different seed gives a different one. The result does not change when the system insertion order is reversed or when unrelated new systems that draw random numbers are added (streams are tied to the system name).

### WP6

- **The world hash** (`World::hash`, version tag `wish-world-hash-v1`): the registry fingerprint, the change tick, the entity table (the generation and location of every slot, the free-list order), archetypes in creation order (component ids, entities, the row marks and field bytes of every column), resources, event queues (sequence numbers included), the text table. Columns without padding are hashed in one call; for the others only the field ranges are read.
- **Texts**, unlike the first plan in §6, are hashed by handle number; since the text table itself enters the hash too, the state is still fully covered (future handles depend on the order of the table, which fits the "state hash" definition better).
- **Random streams are not in the world hash**, they are in the app hash (`App::hash`, `wish-app-hash-v1`): the world hash + the tick counter + the seed and tick rate + the fingerprint of the system set + each system's `Rng` state, last run tick and disabled flag (sorted by name). The "same hash ⇒ same future" guarantee is for the app hash.
- **The registry fingerprint is compared as a prefix.** A snapshot stores how many types were registered and their fingerprint; since the registry is only appended to, types registered later do not block a restore. Restoring into a world with different types is rejected with `snapshot_mismatch` and changes nothing.
- **The app snapshot** stores the system states **by name**; it can be restored into an app that adds the same systems in a different order. It is rejected if the system set (names + constraints), the seed or the tick rate differs. The real time accumulated for `update` is reset on restore (it is not simulation state).
- **Recorded hash value:** the hash of a fixed small world is recorded in a test (`06af2cce…`). If the hash definition changes, this test breaks; if the change is deliberate, it is updated together with the version tag.
- **The `bounce` example world** (the `wish_examples` crate): bodies that spawn randomly, fall, bounce, are sometimes tagged (text through the JSON path) and sometimes have a scout child in the hierarchy, and die randomly; bounces are sent as events and counted with a cursor, and change tracking is measured every tick. The hash and snapshot tests exercise every part of the engine with this workload.
- **M1 exit criteria (determinism), detail:** `bounce` runs 10,000 ticks twice, and the app hashes at every 100 ticks are the same. Snapshot at tick 5,000 → continue, restore → continue, and restore into a freshly built app → continue: all three give the same tick-10,000 hash as the uninterrupted run. (In a debug build all the tests take ~1.3 s.)

### WP7

- **The `wen` command-line tool** (the `wish_cli` crate, binary name `wen`): `run --example <name> [--frames N] [--seed S] [--hash-every K] [--headless] [--json]`, `schema [--example <name>] [--json]`, `examples`, `help`, `version`.
- **The `--json` output uses the same envelope as MCP:** `{ "ok": true, ... }` or `{ "ok": false, "error": <diagnostic> }`. So in M4 the command-line counterpart of every MCP tool will use the same format. In text output, errors are written to standard error as `error[code]: message` + `hint:`.
- **The argument parser was written by hand** (clap was not used): unknown commands and flags are reported in the shared diagnostic format with a "did you mean" suggestion.
- **Wall-clock time is measured only here** (`elapsed_ms`); it never enters the simulation.
- **Measurement (release build, this machine):** `wen run --example bounce --frames 10000 --seed 7` 29 ms; 182 entities; world hash `f1eb0801…`, app hash `837b9d39…` (the same on every run).
- Until the scene file (M2) arrives, `run` runs only the built-in examples; if `--example` is not given, the error says this clearly.

### WP8 and the M1 closing

**Benchmark** (`wen bench`, `stress` workload, release build, this machine: Ryzen 5 5600): 100,000 entities, 600 ticks. Average tick 0.50 ms, worst 0.88 ms (the 60 Hz budget is 16.67 ms). Building the world 51 ms, world hash 5.5 ms, snapshot ~3 ms, restore ~3 ms. The fallback trigger of ADR-004 (bevy_ecs) was not needed; `scalar-math` (math without SIMD) is enough for now.

**Independent closing review.** A separate agent reviewed `unsafe` safety and determinism by reading the code, not the design rationale. It found no practical memory-safety hole; it brought these findings (each verified in the code):

| # | Finding | Decision |
|---|---|---|
| 1 | A huge array schema with zero-size elements could pass validation and lock up the engine (breaks principle 7) | **Fixed:** `validate` checks the limits first: depth ≤ 32, value ≤ 1 MiB, the full walk of one value ≤ 2^20 nodes, an array with zero-size elements is forbidden. Registration validates the schema before any other operation walks it |
| 2 | A rejected JSON value still grew the text table (`set_json`, `set_resource_json`, `send_json`, commands) | **Fixed:** texts are first collected in `PendingTexts`, and if the value is accepted they are committed with the same handles |
| 3 | Binding a Rust type to a store registered with a schema relied on 128-bit hash equality | **Fixed:** structural comparison with `TypeSchema::same_layout`. Snapshot restore is still checked with the fingerprint prefix: snapshots are produced only in-process with `World::snapshot` and cannot be created from outside |
| 4 | A component registered only with a schema was not visible in a typed query; an equal hash could give a different future | **Fixed:** the typed lookup falls back to a name + structural layout match (`Table::lookup`) |
| 5 | NaN bit patterns enter the hash | **Deliberately postponed:** in the same build, NaN bits are deterministic (D1 holds). It is the job of the cross-platform goal D2; the fix (canonicalizing NaN in the hash and in JSON) would slow every hash, so it will be done together with D2 |
| 6 | The real time accumulated for `update` is not in the app hash | **Doc fixed:** "equal hash ⇒ equal future" is for `step`/`run`; real time is not part of the replay |
| 7 | A text handle that the world did not give could be stored with raw bytes | **Fixed:** `insert_bytes` checks the handles (`invalid_text_handle`). Native code can insert the `Text` of another world; this is a documented limitation |
| 8a | When the generation overflowed in an index reused 2^32 times, an old handle could become valid | **Fixed:** an index whose generations are used up is retired |
| 8b | If a downstream crate turns on `serde_json/preserve_order`, the JSON field processing order changes | Noted: deterministic for the same build; within the scope of D2 |

The review also confirmed these are sound: the bounds of column copies, raw `MaybeUninit` copies, reading only field ranges, ZST pointers, query pointers staying valid without re-borrowing, the `Changed` filter reading before the item, access checking, the forms the macro rejects (generics, unions, tuple structs, user `repr`), archetype moves and `swap_remove` location fixes, hierarchy links, event cursors, command targets, no use of HashMap/wall clock/OS randomness.

**M1 exit criteria:**

| Criterion | Status |
|---|---|
| 10,000 ticks twice, same hash | Met (`wish_examples/tests/determinism.rs`) |
| Snapshot at 5,000 → restore → continue = uninterrupted | Met (in the same app and in a freshly built app) |
| JSON Schema of every component; lossless JSON round trip | Met (property test for every finite `f32`/`f64`) |
| Miri clean | Met (`wish_ecs`, strict pointer provenance) |
| CI green | Met (15 November 2026, after the repo was pushed to GitHub): fmt, clippy, test on Windows and Linux; cargo-deny license check. The recorded world hash and the `Rng` sequences gave the same output on Linux too (the first evidence for cross-platform determinism D2) |
| 100 thousand entity benchmark recorded | Met (above) |

**Handed over from M1 to M2:** compile-error tests (`trybuild`) and the repeated error messages of the macro for a field that is not plain data; the move to `syn` 3.
