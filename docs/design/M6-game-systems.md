# M6 — Game systems: physics, audio, lights, animation, in-game UI, particles, 2D

Status: design (16 November 2026). Branch: `m6-systems`. Related decisions: ADR-005 (determinism), ADR-006 (snapshot, replay), ADR-011 (game logic in Wasm), ADR-012 (render), ADR-014 (physics: Rapier), ADR-015 (audio: Kira), ADR-017 (testing), ADR-020 (3D first, 2D as a subset). Candidate for a new decision: **ADR-026** (simulation state outside the ECS, §3).

## 1. Goal and limits

At the end of M6, a game code author (a person or an agent) makes a character that jumps, collides and stands on the ground; plays sound, and the agent reads "what played, where, how loud" without hearing the sound; plays a glTF model with skeletal animation lit by point and spot lights; shows an interface on screen that writes the score and can be clicked; and builds simple particles and a 2D scene (orthographic camera + sprite). **Every subsystem can be observed from the control plane and has headless tests** (the ADR-017 rule); a scene with physics passes the snapshot → restore and replay determinism tests (Windows ↔ Linux same hash).

**Exit criteria (ROADMAP):**
1. Every subsystem can be observed from the control plane and has headless tests.
2. A scene with physics passes the snapshot/restore and replay determinism tests.
3. The evaluation suite reaches 25 tasks; Sonnet 5.5 · medium ≥ 85%. (The run spends quota: **the user will do it at the very end**, decision of 16 November 2026; the tasks are kept ready with `selftest`.)

**Out of scope (on purpose):** vehicle physics and a joint editor (joints exist as data, there is no vehicle model), soft bodies/cloth, 3D audio reverb/occlusion, an audio mixing graph (only channel groups), an interface editor (M8a), an animation timeline editor (after 1.0, VISION), blend tree / state machine (game code picks clips itself; M11 candidate), GPU particles (CPU, on the render side), shadowed point lights (a single shadow map on a spot light is an end-of-M6 candidate; point light shadows after 1.0), text input (a text box in the interface; together with M8), gamepad (the `gilrs` license is checked; it is added to `wish_input` in WP4 or slips to M7).

## 2. Crates and dependency direction

| Crate | Responsibility | External dependency (license) |
|---|---|---|
| `wish_physics` | Physics components (declared in `wish_types`), ECS ⇄ Rapier sync, step, events, queries (ray, shape, overlap), character controller | `rapier3d` (Apache-2.0), `enhanced-determinism` |
| `wish_audio` | Audio components and events, **audio event log** (deterministic, in the world), output (only in windowed play) | `kira` (MIT/Apache), decoders `hound` (WAV), `lewton` (Ogg Vorbis) — `symphonia` is MPL-2.0, so it is **not used** |
| `wish_ui` | In-game interface: nodes, flexible layout, text, image, button; interaction (pointer → button events) | `taffy` (MIT), `fontdue` or `ab_glyph` (license/fit check in WP7) |
| `wish_anim` | Clips (glTF), player, skeleton pose computation (deterministic, fixed step) | — (`wish_import` already reads glTF) |
| `wish_render` (grows) | Point/spot lights + clustered forward+, skinning, sprite, orthographic camera, interface drawing, particle drawing, the `colliders` debug view | — |

Component declarations (`RigidBody`, `Collider`, `AudioSource`, `PointLight`, `UiNode`, `Sprite`, …) stay **in `wish_types`** (the crate opened in M5: no ECS, compiles on wasm32) → game code sees them as they are through the SDK. Systems are in their own crates. Direction: `wish_physics`, `wish_audio`, `wish_ui`, `wish_anim` → `wish_app`, `wish_ecs`, `wish_types`, `wish_core`. They do not know each other; they talk through the ECS (for example physics writes `Transform`, render reads it; the interface sends a button event, game code reads it). `wish_control` adds all of them to the session.

## 3. Physics (WP1–WP3)

### 3.1 Simulation state outside the ECS (ADR-026 candidate)

Rapier's state (body/collider sets, broad/narrow phase, island manager, warm-start impulses) does not fit in ECS columns (variable-size trees), and rebuilding it every tick is both expensive and poor in quality (the warm start is lost, stacks jitter). Options:

| Option | Plus | Minus |
|---|---|---|
| A. Build from scratch from the ECS every tick | A single source of truth, the ECS; snapshot is free | Expensive; no warm start → stacks jitter; contact events must be reconstructed |
| B. Keep the Rapier state in the world as a byte array | The world hash covers everything | Serialization every tick; breaks the `Plain` fixed-size rule |
| **C. App extension (chosen)** | Rapier lives on; snapshot by `Clone`; the ECS carries the observable state | The hash covers only the ECS (enough with the rule below) |

**Decision:** **extensions** are added to `wish_app::App`: `App::insert_extension::<T: Extension>(value)`; `Extension: Clone + Send + 'static` and `fn fingerprint(&self) -> Hash128` (optional, for diagnostics). `AppSnapshot` carries clones of the extensions, `restore` puts them back; a fork copies them too. **Rule (ADR-026):** every value that matters to the game (position, rotation, linear/angular velocity, sleep state) is **written back to the ECS components** every tick; the extension holds only derived/cache-like state (the broad phase tree, contact cache, warm start). So the world hash catches every drift (a drift shows up in a position/velocity within one tick) and replay recordings work unchanged. Extension state **cannot be a source of drift**: it is restored together with the snapshot; continuing after a restore is bit-for-bit the same as an uninterrupted run (test).

**Hot reload and editing:** the ECS is the source of truth; every tick the sync moves the definition in the ECS (body type, shape, mass, friction…) to Rapier with change tracking (`Changed`, added/removed); if the user changes a `Collider` by hand, its counterpart in Rapier is rebuilt. Writing `Transform` by hand/by code on a body with physics means "teleport" (a dynamic body keeps its velocity; `teleport` hint).

### 3.2 Determinism

- `rapier3d` with the `enhanced-determinism` feature (the same result across platforms; SIMD and parallelism off). No parallel step (ADR-005).
- Insertion order is deterministic: entities are processed in a fixed order (archetype, row); the `Entity ↔ RigidBodyHandle` mapping is a `BTreeMap`.
- Recorded hash test: a physics box stack for 600 ticks → hash recorded; in CI, Windows and Linux must find the same hash (like the recorded hash tests in M1). If it does not hold: the ADR-014 trigger, the problem is documented.
- The boundary between Rapier's own `glam`/`nalgebra` version and ours (`glam` 0.34, `scalar-math` + `libm`) is crossed only with arrays (`[f32; 3]`); math types are not leaked.

### 3.3 Components and resources (`wish_types::physics`)

- `RigidBody { kind: Dynamic | Fixed | Kinematic, linear_damping, angular_damping, gravity_scale, can_sleep, ccd, lock: Locks }` — the `Locks` flags are for 2D (Z translation, X/Y rotation locked → 2D physics does not need a separate engine, ADR-020).
- `Collider { shape: Box | Sphere | Capsule | Cylinder | Plane(a thin box instead of a half space) | Mesh(from a model, Fixed only), size: Vec3 (by shape), offset, friction, restitution, density, sensor: bool, layer: u32, mask: u32 }`. Scale: the scale of the entity's `GlobalTransform` is applied to the shape (non-uniform scale is supported for box/cylinder, the largest axis for sphere/capsule; a diagnostic warning).
- `Velocity { linear: Vec3, angular: Vec3 }` — written back every tick; if code/tools write it, it is applied to the body.
- `ExternalForce`/`Impulse`: an SDK command (`commands.apply_impulse(e, v)`) → at the next physics step (a command event); the persistent force component is `Force { force, torque }`.
- `CharacterController { up, max_slope_deg, step_height, snap_to_ground, offset }` + `CharacterMotion { desired: Vec3 }` (the desired movement that code writes) → output `CharacterState { grounded, ground_normal, hit_wall }` (written every tick; game code reads it). Rapier `KinematicCharacterController`.
- `PhysicsSettings` resource: `gravity` (−9.81 Y), `substeps`, solver iterations.
- Events: `CollisionStarted { a, b }`, `CollisionEnded { a, b }`, `TriggerEntered/Exited` (sensor); sent in a fixed order (sorted by the entity id pair).

### 3.4 Queries: game code and agent

- **Game code:** ray/shape queries need an **import** (the frame is built beforehand, the query comes in the middle of the call). `wish.physics_query(ptr, len) -> i64` is added (a backward-compatible extra import inside ABI v1; old modules do not import it, so they are not affected). The query runs against the physics state **at the start of the tick** (read-only; independent of system order → deterministic). SDK: a `Physics` parameter: `physics.raycast(origin, dir, max) -> Option<Hit { entity, point, normal, distance }>`, `overlap_sphere`, `shape_cast`. Fuel: each query costs a fixed amount + an amount per result (deterministic).
- **Agent / tools:** a new tool **`physics_query`** (the 24th tool; reason: squeezing the ray, overlap and contact list over a scene/session into the parameter of another tool blurs the meaning): `{ session, ray: {from, dir, max} | overlap: {shape, at} | contacts: {entity} }` → hits with scene ids. `entity_get` gives a `physics` summary (is it asleep, contacts, `CharacterState`) as an extra field on an entity with physics. `capture_frame view: "colliders"` (already planned in MCP_API): collider wireframes, sensors in a separate color.
- **Inspector (M4's world inspector):** "a dynamic body fell below the ground" (`fell_out_of_world`, hint: a ground collider or a kill zone — the MCP_API example), "dynamic body without a collider", "collider with zero scale".

### 3.5 Cost

WP1 measures: 1,000 dynamic boxes (not sleeping), time per tick (`enhanced-determinism` without SIMD). Budget: under 25% of the frame budget (4 ms); if exceeded it is noted, and it is looked at again at the scale of the M7 showcase game.

## 4. Audio (WP4)

- **Assets:** `*.wav` (hound), `*.ogg` (lewton) go into the asset index; `AudioClip` meta (length, channels, sample rate) is extracted at import. A decode error is a diagnostic (`audio_decode`).
- **Components:** `AudioSource { clip: AssetRef, volume, pitch, looping, spatial, min_distance, max_distance, autoplay, playing }`, `AudioListener` (usually on the camera). **Command/event:** `PlaySound { clip, volume, pitch, at: Option<Entity>, position }` (one shot).
- **Deterministic core, non-deterministic output:** every tick a native system evaluates the sounds in the world: start/end/loop, **gain (dB) and direction (pan)** relative to the listener (with our own formula, at a fixed step) → the **audio event log** is a resource in the world (`AudioLog`, a fixed-size ring: the last 256 events) + a line in the session log (`logs_get kinds: ["audio"]`): "tick 412: `sfx/jump.ogg` started, source `player`, 0.8 → −6 dB at the listener, left 40%". The agent verifies sound without hearing it; a scenario test queries it with `expect` (`AudioLog.last_clip`… or a separate expression function). Real output (Kira) is only in `wen play`; if there is no output device it is skipped silently and never affects the simulation.
- **Volume groups:** an `AudioSettings { master, music, sfx }` resource.

## 5. Lights (WP5)

- `PointLight { color, intensity (lm), range }`, `SpotLight { color, intensity, range, inner_angle_deg, outer_angle_deg }` (direction: the entity's −Z), the directional light is from M3.
- **Clustered forward+:** the view volume is divided into 16×9×24 clusters; light assignment with a compute shader (in the WebGPU core; ADR-019). At most 64 lights per cluster, 1,024 lights in a scene. It also works on the software adapter (golden images).
- Shadow: the directional light is from M3; **spot light shadow** is an end-of-WP5 goal (a single map/light, at most 4); point light shadow is out of scope.
- Observability: `describe_view` does not give "the lights that light this pixel" for visible entities (expensive), but lists light entities if their influence sphere/cone is on screen; `capture_frame view: "lights"` (light influence areas as a wireframe) is a new view.

## 6. Skeletal animation (WP6)

- `wish_import` reads glTF skins and animations: joints become **entities** in the scene (hierarchy, `Transform`), `Skin { joints: [Entity; N]? }` needs a fixed size → the `Skin` component holds the joint count and the skin index inside the model; joint entities carry `Joint { skin_root, index }` (fixed-size components, variable lists are derived from the hierarchy).
- Clips stay not as entities but as **asset files** (the animation inside the glTF, `AssetRef` + name); `AnimationPlayer { clip: AssetRef, clip_name: Text, time, speed, looping, playing, weight }` (+ transition: `fade_from: Text`, `fade_time`).
- **Fixed step, deterministic:** the player system samples the clip every tick and writes the joint `Transform`s (render interpolation is on the M3 path). Because the pose is in the world, the snapshot/replay covers it; game code can read the joint position (for example attaching a sword to the hand bone = hierarchy).
- Render: a skinning shader (joint matrices in a storage buffer, an offset per entity).
- Observation: `entity_get` shows the player's clip/time; `describe_view` shows the clip name on an animated entity.

## 7. In-game interface (WP7)

- The interface is made of **entities** (hierarchy = interface tree): `UiNode { width, height (Val: px | % | auto), padding, margin, flex_direction, justify, align, gap, position: relative|absolute, offset }`, `UiText { text: Text, size, color, align }`, `UiImage { image: AssetRef, tint }`, `UiButton { action_name? }` → the state `UiInteraction { hovered, pressed, clicked_at_tick }` is written every tick; the event is `ButtonClicked { entity }`.
- **Layout is deterministic and at a fixed step** (`taffy`): the root screen size is a fixed virtual resolution (`UiSettings { reference_width, reference_height, scale_mode }`) — the window size does not leak into the simulation.
- **Pointer input as an action:** in `input.json`, the built-in actions `pointer` (axis2, in virtual-resolution pixels) and `click` (button); the agent presses a button with `sim_step input: {"pointer": [640, 360], "click": true}` (ADR-016's single clean point is kept).
- Text: font assets (`*.ttf`/`*.otf`), a default font that ships with the engine (**the license will be put to the user for approval**: does the MIT/Apache/BSD rule apply to fonts too? — §11), the glyph atlas in render.
- Observation: `describe_view` lists interface items with their screen boxes and texts ("`score_label`: 'Score: 3' (12, 8, 140×32)"); `capture_frame` draws the interface.

## 8. Particles (WP8)

- `ParticleEmitter { rate, burst, lifetime, speed, spread_deg, size_start/end, color_start/end, gravity, seed, max }`.
- **Stateless and deterministic:** the particle position = f(emitter seed, spawn index, age) — closed form; nothing is stored in the simulation, and render computes it every frame. Particles do not affect the game (no collisions); replay and snapshot are free, and render computes from the tick time.
- Observation: `entity_get` shows "the number of particles alive now" (from the formula), `describe_view` lists the emitter and the screen box it covers.

## 9. 2D subset (WP8)

- `Camera` gets `projection: Perspective | Orthographic` and `ortho_height` (visible height, m) (schema migration: the new field gets its default).
- `Sprite { image: AssetRef, rect (inside the atlas), color, flip_x, flip_y, anchor, pixels_per_unit, layer: i32 }`; draw order is layer + Z; with alpha blending.
- 2D physics: with `RigidBody.lock` (§3.3). Sprite animation: game code changes `rect` (a frame list is an M11 candidate).

## 10. Control plane (invariants 4, 8, 9)

| Capability | Tool surface |
|---|---|
| Physics queries | **`physics_query`** (new, the 24th tool), `entity_get` physics summary, `capture_frame view: colliders` |
| Editing physics/audio/interface/animation data | The existing `edit_apply` (components come from the schema; the same path by hand and by agent, invariant 9) |
| Audio observation | `logs_get kinds: ["audio"]`, `entity_get` (is it playing, gain) |
| Light observation | `capture_frame view: lights`, `describe_view` |
| Interface | `describe_view` (items, texts, boxes), `sim_step input: pointer/click` |
| Testing | Scenario tests (new functions: `count` in the expression language from M5; the `AudioLog` resource for audio) |

The parity test wants an example for the new tool (M4 rule); the diagnostic catalog wants every new code.

## 11. Decisions / approvals that belong to the user

- **Default font license (WP7):** common open fonts are under the SIL OFL license; the project rule is "only MIT/Apache/BSD-like". Options: a font with an Apache-2.0 version (for example the old Roboto/Droid Sans), or accepting the OFL for fonts. **The user is asked in WP7**; the download also needs approval.
- **Sound samples and test assets:** for tests, sounds are produced with code (a sine wave WAV) → no download is needed.
- **Evaluation runs:** the user will do them at the very end (16 November 2026).

## 12. Test plan (with exit criteria)

| Criterion | Evidence |
|---|---|
| Physics determinism | Recorded hash (box stack, 600 ticks) Windows = Linux (CI); snapshot → restore → continue = uninterrupted; a fork gives the same hash in two sessions; 10,000-tick replay with a physics game |
| Physics correctness | A falling box stops on the ground (y ≈ half height), restitution coefficient, ordered sensor events, character slope/step, ray query hits, 2D locks |
| Audio | Log lines (tick, gain, pan) as golden JSON; loop/end; nothing breaks in an environment with no output |
| Lights | Golden images (point, spot, 200-light clustering), with the software adapter |
| Animation | Sampling unit tests (linear/spherical interpolation, step), a Khronos sample (`RiggedSimple`/`Fox`: needs download approval → if not available, a two-bone glTF generated with code), golden image |
| Interface | Layout unit tests (taffy), a click scenario (`pointer`+`click` → `ButtonClicked`), the `describe_view` text list, golden image |
| Particles / 2D | Formula unit tests, golden images, orthographic `describe_view` |
| All | Every new diagnostic code in the catalog; parity test; hostile inputs (broken WAV/OGG/TTF, giant collider, NaN force) |

## 13. Work packages and recommended model

| WP | Work | Model · effort |
|---|---|---|
| WP1 | Proof work: `rapier3d` + `enhanced-determinism`, `App` extensions (ADR-026), box stack hash (two platforms in CI), cost measurement | Opus 5.5 · high |
| WP2 | Physics components, sync, events, character controller, adding to sessions, schema migration | Opus 5.5 · high |
| WP3 | Queries: the `physics_query` tool, SDK `Physics` + the `wish.physics_query` import, the `colliders` view, inspector rules | Opus 5.5 · high |
| WP4 | Audio: assets, components, log, Kira output (`wen play`) | Sonnet 5.5 · medium |
| WP5 | Point/spot lights, clustered forward+, spot shadow, the `lights` view | Opus 5.5 · high (render) |
| WP6 | Skeletal animation (import, player, skinning) | Opus 5.5 · high |
| WP7 | In-game interface (layout, text, button, pointer) | Sonnet 5.5 · high |
| WP8 | Particles, 2D (orthographic, sprite) | Sonnet 5.5 · medium |
| WP9 | Evaluation tasks 13–25 (`selftest`), an example game with physics, independent closing review, M6 result, merge to `main` | Opus 5.5 · high |

## 14. Risks

| Risk | Measure |
|---|---|
| Rapier gives different results across platforms | `enhanced-determinism`; recorded hash on two platforms in CI; if it does not hold, the ADR-014 trigger (not Jolt, first the source of the problem) |
| Slowness with `enhanced-determinism` | WP1 measurement; budget 25%; if needed sleep thresholds, substep setting |
| Extension state and the ECS drifting apart | The ADR-026 rule + the "restore → continue = uninterrupted" test on every physics feature |
| Clustered lights being slow on the software adapter | Golden images at small size; the compute shader is verified on WARP/lavapipe |
| Font license | §11, user decision |
| Scope (M6 = L) | Independent WPs; particles/2D at the very end, and if needed they slip to M7 ("this does not grow the 1.0 scope", the slip is noted) |

## 15. Implementation notes

(Added when each work package is done.)

### WP1 — proof work: Rapier determinism, ADR-026, cost

- **`wish_physics`** was opened, `rapier3d 0.36.1` (Apache-2.0), features: `dim3`, `f32`, `std`, `enhanced-determinism` (no SIMD and no threads).
- **Determinism:** a 6-level box pyramid + a thrown ball, 600 ticks, the position/rotation/velocity bits of every body hashed with `StableHasher`: `0x8c77d52755b303cb697ba766c515b051`. **Windows debug = Windows release = Linux (CI)** (a recorded hash test; it held in the first CI run). If it does not hold, the first thing to look at is the Rapier features (SIMD/parallel).
- **ADR-026** (extensions) was applied: `World::insert_extension/extension/extension_mut/remove_extension` (`wish_ecs/src/extension.rs`); the snapshot and `clone` carry them, and they do not enter the hash. Evidence: when the Rapier state is copied and continued, it is bit-for-bit the same as a plain run (`a_cloned_state_continues_bit_for_bit`; `PhysicsPipeline` cannot be copied, it is a scratch buffer, and a new one turned out to be enough).
- **Cost (release, built-in pyramid, average of 120 ticks):** 57 bodies 0.04 ms, 212 bodies 0.12 ms, **992 bodies 2.0 ms (12% of the frame budget)** → under the 25% budget.
- Note: design §3.1 said "App extensions"; the extensions were kept in `World` (they enter the snapshot automatically, and systems already access `World`).

### WP2 — physics components, sync, events, character controller

- **Components** (`wish_types::physics`, also in the SDK prelude): `RigidBody` (`BodyKind` Dynamic/Fixed/Kinematic, damping, `gravity_scale`, sleep, CCD, axis locks), `Collider` (Box/Sphere/Capsule/Cylinder, `size` × the entity's scale, `offset`, friction, bounce, density 1000 kg/m³, sensor, layer/mask), `BodyVelocity`, `BodyForce`, the event `BodyImpulse`, the resource `PhysicsSettings`, events `CollisionStarted/Ended { a, b, sensor }` (a < b), character: `CharacterController`, `CharacterMotion`, `CharacterState`.
- **Naming decision:** `Velocity`/`Force`/`Impulse` are very common names for games' own components (our examples, the M5 test game and the evaluation tasks define their own `Velocity`); if they were engine types every game code would get `script_type_mismatch`. The engine's are **`BodyVelocity`, `BodyForce`, `BodyImpulse`**; a concept hint guides an agent searching for "velocity/force/impulse".
- **`wish_physics::step`** (system `physics`, after game and example systems, before `transform_propagation`; the name `physics` is reserved for the engine): (1) ECS → Rapier: new/changed/removed colliders and bodies (in entity order), a sleeping body is woken when its collider changes; a `Transform` written from outside physics teleports, `BodyVelocity` sets the velocity (locked axes are masked — Rapier does not lock a velocity put from outside); (2) `BodyImpulse` events (`World::events_since(last_run)`: the position is in the snapshot); (3) step (substeps); (4) characters; (5) write-back (only changed values are written); (6) events. For child entities, the world ⇄ local transform goes through the parent's `GlobalTransform`.
- **Character controller — two findings:** (a) with a single movement under gravity, the shape was pressed into the ground and Rapier rejected the horizontal part → movement is in **two phases** (first horizontal: slide and step; then vertical); on the tick when a step is climbed, the downward movement is not applied (so that a round base does not slide back off the edge), and the step width is relative to the capsule radius. (b) When characters moved **before** the step, the query tree did not know the ground added in that tick, and the character sank into the ground → characters are moved **after** the step. A character going up does not count as `grounded` (the first tick of a jump).
- **Evidence:** `wish_physics/tests/world.rs` (a box that falls and rests, restore → continue = uninterrupted, ECS source of truth, events and sensor, a character climbs a step and stops at a wall, 2D locks, JSON; the **recorded world hash** `ffb4e843…` debug = release); `wish_control/tests/physics_tools.rs` (a stack built with tools falls and stops in a session, the scene file does not change; **snapshot, fork and a 600-tick replay bit for bit** — exit criterion 2; game code walks, jumps and lands by writing `CharacterMotion` and reading `CharacterState`).

### WP3 — queries: `physics_query`, ray in game code, inspector

- **`wish_physics::query`:** `PhysicsState::raycast` (first hit: entity, point, normal, distance; sensors excluded; layer mask; the asking entity can be excluded), `overlap` (sphere/box; sensors included), `touching` (contacts + sensor overlap), all in entity order. `query_state(world)`: the state of a world that has stepped, or if none, a state built for the question (only collision detection, nothing moves). `prepare(world)`: the state is built when a session starts; queries in the first tick also see the scene and the first step continues from it.
- **`physics_query` tool (the 24th tool):** on a session (the world at the last step) or on a scene file (colliders where the scene puts them); **exactly one** of `ray` / `overlap` / `touching`, `mask`; replies use scene ids; errors have locations (`/ray/from`), an unknown entity gets "Did you mean". The parity test (in-process = `wen do` = MCP) has three examples.
- **Ray in game code:** the new import `wish.physics_query(request, length, output, capacity) -> written` (a backward-compatible extra for ABI v1; old modules do not import it); the request/reply encoding is in `wish_sdk::physics`, the same code on both sides. SDK: `raycast(from, dir, max)`, `physics::cast(Ray { .., mask, ignore })`, in the prelude. **Against the world at the start of the tick** (physics does not run before the systems; the extension state is fixed during the tick) → independent of system order, deterministic (test: the same run gives the same hash). `wish_script` does not know physics: the **`HostServices`** interface (`ScriptRuntime::with_services`), applied by the control plane; during the call a read-only pointer to the world (`WorldRef`, with a `SAFETY` note, cleared as soon as the call ends). Every query costs a fixed **20,000 fuel**; hostile guest test: garbage pointer/length, a reply that does not fit, an infinite query loop → `script_out_of_fuel`, the app continues.
- **Inspector:** `rigid_body_without_collider`, `character_not_kinematic`; when a dynamic body falls out of the world, the `entity_out_of_bounds` hint mentions the ground collider and deleting with a sensor.
- **Moved:** `capture_frame view: colliders` (capsule/cylinder meshes and a colored line pass are needed) → to the render package, WP5.
- Evidence: `wish_physics/tests/world.rs::rays_overlaps_and_contacts_answer_questions`, `wish_control/tests/physics_tools.rs` (the tool on a scene and on a session, errors; game code measures the distance to the ground with a ray at the first tick and after settling, a repeated run gives the same hash; inspector), `hostile_game.rs::physics_questions_cannot_hurt_the_host`, the SDK encoding round-trip test.

### WP4 — audio

- **Components** (`wish_types::audio`, in the SDK prelude): `AudioSource` (clip, `AudioBus`, volume, pitch, loop, spatial, min/max distance, `playing`), `AudioListener`, `AudioPlayback` (written by the engine: position, `gain`, `pan`, `started`), `AudioOneShot` (marker), the resource `AudioSettings` (master/sfx/music), events `PlaySound`, `SoundStarted`, `SoundEnded`.
- **`wish_audio::step`** (system `audio`; the name is reserved for the engine; **after transform propagation**, so that it hears the new place of the objects): `PlaySound` → a short-lived entity (deleted when the clip ends); listener = the first `AudioListener` in entity order; for each source start/advance/loop/end; gain = volume × bus × master × listener × distance (`min/distance`, 0 at `max`), pan = direction · the listener's right; all are linear amplitude and IEEE basic operations (no log → the same across platforms). The end of a sound whose entity is gone is also reported. An unknown clip gives `audio_clip_unknown` once, and the source is closed.
- **Clip lengths** are **in the extension**, not in the world (ADR-026): the control plane reads the project's `.wav`/`.ogg` files when a session starts (`Workspace::audio_clips`; the length is cached by file size+date), and puts them with `wish_audio::install`. Decoders: `hound` (WAV 8/16/24/32 integer, 32 float), `lewton` (Ogg Vorbis); **Kira's default decoder `symphonia` is turned off because it is MPL-2.0**; limits: 128 MiB file, 1 hour, ≤8 channels (reduced to 2); a broken file gives `audio_decode` (no panic).
- **Audio event log:** the system writes lines to the extension, and after every step the control plane empties it and puts it in the session log with the `audio` kind (`logs_get kinds: ["audio"]`): "`assets/sounds/jump.wav` started on 'door': -12.0 dB at the listener, right 100%". Because the extension is in the snapshot, restore → continue produces the same log (test).
- **Output:** `wish_audio::output::Speaker` (feature `output`, off by default: cpal needs ALSA on Linux) — reads `voices()` from the world, plays **through `cpal` with our own small mixer** (volume, equal-power pan, pitch, loop; gain and pan are smoothed; the audio thread never waits for the game), **writes nothing** to the world; **Kira was dropped**: even with `symphonia` off, Kira depends on `triple_buffer`, which is MPL-2.0 licensed (the `cargo deny` in CI caught it); if there is no device, an `audio_device` warning, and the game goes on silent. It is connected to `wen play`, **for now Windows only** (Linux/macOS packages are M9). Trial by hand: `cargo test -p wish_audio --features output --test speaker -- --ignored --nocapture` (the device opened and the commands ran without error; **not verified by ear**).
- Evidence: `wish_audio/tests/audio.rs` (5: start/end/dB/pan, distance and direction, loop + one shot + a vanished entity, unknown clip, restore = uninterrupted), `clip` unit tests (WAV, broken files), `wish_control/tests/audio_tools.rs` (sound in the scene shows in `logs_get`; `check` catches a missing file; snapshot and replay bit for bit).

### WP5 — point and spot lights, clustered lighting, the `colliders` and `lights` views

- **Components** (`wish_types::render`, in the SDK): `PointLight { color, intensity_lm (800 = a 60 W bulb), range }`, `SpotLight { color, intensity_lm, range, inner_angle_deg, outer_angle_deg }` (direction: the entity's −Z). Lumen → candela = Φ/4π (also for the spot: brightness does not change as the cone narrows); falloff inverse square × a smooth window to zero at the range `(1 − (d/r)⁴)²`; the spot edge is a `smoothstep` between the inner and outer cosine.
- **Clustered forward+ (ADR-012 baseline):** the view volume is 64 px tiles × 24 exponential depth slices; lights are assigned to clusters **on the CPU, in light order** (`wish_render::cluster`: same scene → same lists), and the shader reads the cluster list from three **read-only storage buffers** (WebGPU core, works on the software adapter). The assignment is conservative: the projection of the light's bounding box + a margin of one slice in depth (even if the GPU's logarithm rounds a little differently, the light is not missed; a unit test checks that every lit point finds its light in its own cluster). At most 1,024 lights, 128 per cluster; if exceeded, a **warning** (`too_many_lights`, `light_clusters_full`), no silent cutting.
- **Finding:** in the first version, black tile-aligned rectangles appeared in a scene with 200 lights — because the slices were spread to the camera's far plane (1 km), the near slices were thick, the limit of 64 per cluster filled up, and the lights closest to the camera were thrown away (2,364 drops). Slices now go up to the range of the farthest light; the tile margin was removed (the box is already conservative); limit 128 → **0 drops** (unit test).
- **`capture_frame view: colliders`** (from WP3): wireframe + colliders (fixed green, moving cyan, sensor yellow; box/sphere/capsule/cylinder, with the scale physics uses). **`view: lights`**: wireframe + the range sphere of a point light, the inner/outer cone of a spot, in the color of the light. Both use a general **gizmo line pass** (`wish_render::gizmo`, `gizmo_pass.wgsl`: colored lines in world space, on top of everything) — also for future ray/path drawings.
- **Spot shadow was not done:** the ADR-012 baseline wants a directional light shadow (exists) and cascaded shadow maps; the spot shadow is added if the M7 showcase game needs it (to the M11/M7 list).
- Evidence: `wish_render/tests/lights.rs` (a red bulb and a white spot in a dark room: color, darkness outside the cone, fading with distance; 200 lights; two views; golden images `lights`, `many_lights`, `view_lights`, `view_colliders`), `cluster` unit tests, `wish_control/tests/physics_tools.rs::the_colliders_and_lights_views_come_through_the_tools`.

### WP6 — skeletal animation

- **Import (`wish_import`):** the TRS of nodes, `skins` (joints + inverse bind matrices), the `JOINTS_0`/`WEIGHTS_0` of primitives, `animations` (channels: translation/rotation/scale; step, linear — the short arc for rotation — and cubic spline). Morph weights give an `unsupported_animation` warning. Against a hostile file: at most 2M keyframes, 1,024 joints per skin, a mismatched length/non-finite value gives `gltf_invalid` (no panic). `Model::pose(clip, time)` node matrices, `Model::skin` skinning on the CPU (normals included). Tested with hand-computed expectations: a two-joint bar (`wish_import::samples::bending_bar`, a glTF generated with code — no download) bends correctly at 0 / 0.5 / 1 s.
- **Two departures from the design (with reasons):** (1) joints are **not entities**: models are drawn as a single entity; making every bone an entity would have been a big structural change. The model entity gets **`AnimationPlayer { clip, time, speed, looping, playing }`** (`wish_types::anim`), the event `AnimationFinished`. Time is in the world → the snapshot and replay cover it (test: a looping clip replay bit for bit). Game code reading a bone position (a sword in the hand) goes to M7. (2) skinning is **on the CPU**, and render uploads a per-frame mesh for every animated draw: shadow, id buffer, wireframe, `describe_view` see the correct pose without change. Skinning on the GPU is an M7/M9 optimization.
- **`wish_anim`:** system `animation` (the name is reserved for the engine; after game systems, before propagation): time += delta × speed; if the length is known it wraps when looping, and when not looping it stops at the end and sends `AnimationFinished`. Clip lengths are in the extension (ADR-026): the control plane reads `.gltf`/`.glb` files when a session starts (cache by file size+date), `wish_anim::install`.
- **Render:** `Draw.animation` (clip name, time); `Renderer::pose` once per draw call (render, views, id buffer, `describe`); models without animation are as before.
- Evidence: `wish_import/tests/animation.rs` (3), `wish_render/tests/animation.rs` (with the id buffer: t=0 upright, t=1 bent to the left, the lower half in place), `wish_control/tests/anim_tools.rs` (0.5 s halfway; `AnimationFinished` at the end, `playing` false; a picture in a session; loop replay bit for bit).

### WP7 — in-game interface

- **Font decision (user, 16 November 2026):** the font that ships with the engine is **Roboto 2.138, Apache-2.0** (`src/hinted/Roboto-Regular.ttf` from the v2.138 tag in Google's official repository, downloaded with the user's approval). The license rule was not broken (no OFL exception was needed). The file and its license are in `crates/wish_text/fonts/`; `*.ttf`/`*.otf` are binary in `.gitattributes`.
- **Crates:** `wish_text` (new; `ab_glyph` only here): font reading (guarded, `font_invalid`), **line breaking and glyph layout** (only addition/multiplication with the font's numbers → deterministic; a recorded width test is checked in Windows = Linux CI), glyph rasterizing, the `Fonts` extension (ADR-026; the project's fonts, Roboto if missing). `wish_ui` (new; `taffy` 0.15 only here, only flexbox+block): system `ui` (after game systems, before propagation; the name is reserved for the engine), `layout` + `interact`. `wish_render` uses `wish_text` (it does not know `wish_ui`: the interface types are in `wish_types::ui`).
- **Components (`wish_types::ui`):** `UiNode` (width/height `UiLength {unit: Auto|Px|Percent, value}`, padding/margin in CSS order, direction, justify, align, gap, grow, position Relative|Absolute + offset, background, visible, layer), `UiText` (text, font, size = font height in px, color, align, wrap), `UiImage` (PNG, tint), `UiButton {enabled}`; what the engine writes: `UiLayout` (position, size, is it shown, draw order) and `UiInteraction` (hovered, pressed, clicks, last_click_tick); the event `ButtonClicked`; resources `UiSettings` (virtual screen 1280×720) and **`Pointer`**.
- **Departure from the design (with reason):** the pointer is not an `input.json` action; `ActionSlot` values are in the range −1..1, and a pixel position does not fit. `pointer`/`click` are **built-in names** (rejected as action names in `input.json`): `wish_input::apply` writes them to the `Pointer` resource; replay recordings cover them unchanged (the values are already recorded). No resource is added to a world that was never given a pointer → old hashes are kept.
- **Layout rules:** every root box is inside a column the size of the virtual screen (sized by content; `%` is relative to the screen). Text boxes are sized with the measuring function; the measured size is **rounded up** so that a box rounded to a whole pixel holds the text without a new line break (drawing re-lays out with the same function at the same width). Boxes do not shrink (`flex_shrink` 0). Click: the topmost box decides; a box inside a button counts as the button, a box in front that has a background/image blocks the one behind; pressed + released over the same button = click.
- **Text from game code (a backward-compatible extra to ABI v1):** the imports `wish.text_intern(ptr, len) -> i64` and `wish.text_read(handle, out, cap) -> i32`; SDK `text("...")` / `text_of(t)`. During the call the world is lent to game code (`WorldRef` is now `*mut`, the same safety reason: the caller does not touch the world until the call ends). Limits: text ≤ 4096 bytes, ≤ 1024 new texts per call, fuel 2,000 + the byte count; a rejected text becomes empty text and a warning is written. Hostile WAT test (`texts_from_game_code_cannot_hurt_the_host`). **Note (M9 candidate):** the world text table only grows; a counter that changes every tick adds ~200 thousand texts an hour (a few MB). If needed, garbage collection of the text table or a number format field in the interface.
- **Render:** a single atlas on the CPU (4096 wide; white block + glyphs at picture scale + PNG pictures), quads per box, a single pass after tone mapping (`ui_pass.wgsl`, alpha blending, sRGB target). Glyphs sit on whole pixels → texel-by-texel copy, independent of the adapter. When hovered a button is ×1.2, when pressed ×0.8 in color. Pictures are loaded in tools/`wen view`/`wen play` with `load_ui_pictures` next to `Drawing::load_models` (`ui_image_not_loaded`). `wen play` gives the mouse as `Pointing`, and the control plane converts it to the virtual screen (`UiSettings::from_picture`).
- **Observation:** `describe_view` → an `interface` list (entity, the box in the picture, text, hovered/pressed if a button, picture); `entity_get` `UiLayout`/`UiInteraction`; `sim_step input: {"pointer": [x, y], "click": true}` (if only `click` is given, the last position of the pointer); inspector `ui_without_node`, `ui_font_missing`; `check` `font_invalid`.
- **Evidence:** `wish_text/tests/text.rs` (8; including hostile font fuzzing), `wish_ui/tests/ui.rs` (8: layout, %, text wrapping, hiding, click, blocking, layer, fitting), `wish_render/tests/ui.rs` (4 + golden `ui.png`: including the Turkish "Ş"), `wish_control/tests/ui_tools.rs` (7: click with a tool, `describe_view`, replay recording bit for bit, hints for wrong input, font/inspector, reserved names, **game code counts clicks and writes the label** + snapshot).
- Not done (out of scope/later): a text input box (M8), scrolling/clipping (overflow), an interface editor (M8a), drawing a scene without a camera (interface only) — for now a `Camera` is needed.

### WP8 — particles and 2D

- **Orthographic camera:** `Camera.projection: Perspective | Orthographic` and `ortho_height` (m). The new fields have defaults; because scene files do not write the default, old files load unchanged (no migration needed). A single place: `CameraView::projection`; clusters, shadow, views and `describe` use it, so they also work with orthographic.
- **`Sprite`** (`wish_types::sprite`): a picture (PNG) or a plain rectangle, `rect` (a sprite sheet frame, in pixels), `color`, `pixels_per_unit`, `anchor`, `flip_x/y`, `layer`, `smooth` (false = sharp pixels). Not lit. In the entity's XY plane, with its scale.
- **`ParticleEmitter` + `ParticleState`** (`wish_types::particles`) and **`wish_particles`** (a new crate): a **stateless closed formula** — the spawn of the n-th particle (burst + stream), its direction and speed come from a hash of seed+number (SplitMix64), and its position from its age. The alive count uses only addition/multiplication/rounding → deterministic; the system `particles` (the name is reserved for the engine) writes `ParticleState.alive` every tick (only when it changes). Particles are in the emitter's space (they move with the emitter); restarting = writing `start_time`.
- **Drawing (`wish_render::sprites`):** a single pass, **after** tone mapping and before the interface, to the sRGB target; the depth of the opaque pass is now stored (`Store`), and sprites/particles are tested against it (without writing) → if a 3D solid object is in front, they are hidden. Order: `layer`, then far to near. Particles are soft discs facing the camera. Pictures use the same CPU atlas and the same picture store as the interface (`set_ui_picture`, `load_ui_pictures` also loads sprite pictures); two samplers for soft/sharp. At most 100,000 particles/picture.
- **Observation:** `describe_view` → `sprites` (box, layer) and `particles` (box, alive count); because they are not in the id buffer, their boxes are computed from their positions (no visibility percentage). `entity_get` `ParticleState`; `until: "ParticleState.alive == 20"`.
- **Evidence:** `wish_particles/tests/particles.rs` (5: hand-computed numbers, burst/lifetime/limit/stopping, formula, hostile numbers, system), `wish_render/tests/sprites.rs` (3 + golden `sprites.png`: picture, flipping, layer, the cube hiding, particles, boxes), `wish_control/tests/two_d_tools.rs` (2).
- Not done: light/shadow on sprites, a particle picture (disc only), sprites in the id buffer (click-to-select), a sprite frame list (an M11 candidate, already in the design).

### WP9 — evaluation tasks, example game, closing review

- **Evaluation tasks 13–25** (`crates/wish_evals/src/m6_tasks.rs`): a falling box, a bouncing ball, a trigger zone, a character that jumps with game code, music in a loop, a desk lamp, playing an animation, a score text, a start button, a click counter with game code, a 2D sprite scene, campfire sparks, the entity first hit by the camera's center ray. The checks look at what the engine did (session run, `describe_view`, logs, hidden scenario tests). **`selftest` 25/25**; the real run spends quota → the user at the very end.
- **Example game `examples/hop`:** a character with physics (walk/jump), sensor coins (`CollisionStarted`), a sparkle when collected (a `ParticleEmitter` started instantly), the score and a win text in the interface (game code `text()`). Its test wins the game; it is built, tested, recorded and replayed bit for bit in CI (`script_tools.rs`).
- **Fixes found on the way:** the `Collider.restitution` doc said "1 bounces to the same height", but Rapier took the average of the two bounces → the combination rule is "maximum" (test). `wen fmt`/`wen check` now work without `--example` with the project's own types (game code included). Old SDK package: if a build error cannot find a type the engine knows, the hint says "the SDK package is older than the engine; `wen sdk`" (an old package in the development environment had failed two tasks).
- **Independent closing review:** 13 findings (1 high, 5 medium, 7 low); determinism and the `WorldRef *mut` safety came out clean. All were handled:
  1. (high) Game code could grow the world's text table without limit → a world budget (16 MiB, 1M texts; rejected and written to the log when exceeded), the per-call counter really resets per call (test: `game_code_cannot_fill_the_world_with_text`).
  2. `CollisionStarted` did not arrive for contacts that existed when the session started (the prepare step swallowed the events) → they are collected at prepare and sent in the first step (test).
  3. `CollisionEnded` did not arrive when a touching collider was removed → a handle → entity map of those removed this tick (test).
  4. Particle drawing had no real limit, and game code could write out-of-range values → `max`/`lifetime`/`burst`/`rate` are clamped, and the draw budget is shared among emitters (test).
  5. With a `#alt` part model reference and an unnamed clip (`#1`), the animation time did not wrap/was drawn wrongly → both fixed (tests).
  6. `wen play` did not load a picture/model used later → `Content::refresh` loads new ones after every game step (tried in a real window).
  7. There was no practical limit in audio decoding → a decoded sample budget (128 MiB), at most 384 kHz, a file size check before reading; the same for pictures.
  8. The character's collider is one tick behind in queries → **documented** (`physics_query` and SDK `raycast`); fixing it means updating the Rapier broad phase by hand, in M7 if the showcase game needs it.
  9. Overflow in particle counting → clamping and saturating arithmetic (the test of 4 covers it).
  10. A game system with `after = "physics"` could not open the session because of a cycle error → it is rejected at build with an explanation by `script_after_engine_system` (test).
  11. The physics and audio state was copied every tick → `World::take_extension` (no copy; tested in `wish_ecs`).
  12. Replay recordings did not know that the files the game reads (clip lengths, fonts) had changed → an `assets` fingerprint in the header, `replay_assets_changed` when it changes (test: `review_m6.rs`).
  13. A sprite `rect` that went beyond the picture was drawn stretched → the size comes from the shown part of the picture.

## 16. M6 result

**Completed — 16 November 2026.** One verification waits for a user step (the evaluation run).

| Exit criterion | Evidence | Result |
|---|---|---|
| Every subsystem can be observed from the control plane and has headless tests | Physics: `physics_query`, `entity_get`, the `colliders` view, inspector; audio: `logs_get kinds: ["audio"]`; lights: the `lights` view, golden images; animation: `entity_get`, a pose test with the id buffer; interface: `describe_view interface`, `sim_step pointer/click`; particles/2D: `describe_view sprites/particles`, `ParticleState`. Headless tests for each (the WP sections). | ✔ |
| A scene with physics passes the snapshot/restore and replay determinism tests | Recorded Rapier hash Windows = Linux (CI); snapshot → restore → continue = uninterrupted; fork; 600-tick replay (`physics_tools.rs`); the Hop example is recorded and replayed (CI). | ✔ |
| The evaluation suite reaches 25 tasks; Sonnet 5.5 · medium ≥ 85% | 25 tasks, **`selftest` 25/25**. The real run spends subscription quota. | ◐ **waiting for the user's approval** (`wish-evals run`) |

- **Numbers:** 24 tools; 6 new crates (`wish_physics`, `wish_audio`, `wish_anim`, `wish_text`, `wish_ui`, `wish_particles`); 2 example games (`collect`, `hop`); 530+ tests.
- **Pending user steps (together with those left from M5):** (1) `wish-evals run` — 25 tasks, spends quota, approval needed; (2) `C:\wish-build\package\clean-test.wsb` in Windows Sandbox (M5). The audio output should also be tried by ear (with `wen play`, Hop or a scene that has an audio source).
- **Carried over to M7:** spot light shadow; game code reading a bone position (a sword in the hand); updating the character collider in queries in the same tick (finding 8); sprites in the id buffer (click-to-select) and receiving light; a particle picture; drawing a scene without a camera (interface only).
- **To M8:** a text input box, interface scrolling/clipping, an interface editor (M8a).
- **To M9:** skinning on the GPU; Linux/macOS audio output; collecting unused texts in the world text table (limited by the budget today).
