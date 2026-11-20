# M12: the render foundation

Package 1 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0); in the full plan of that note it was F1 of Phase 1 plus the
"measure first" part of Phase 0. Backlog: the graphics area of
[M12-area-maps.md](M12-area-maps.md), gap 1, gap 8 and four quick wins.

Recommended model and effort: Fable 5.2, high (ROADMAP, M12).

Status: done, 18 November 2026 (section 5); the findings of its independent
review are addressed (section 7). Not committed by the agents that built and
fixed it (the package's instruction).

## 1. Problem

`Renderer::render` was one function of about 400 lines. It worked out the
frame's numbers, created every GPU object it needed (the HDR and depth
textures, a 2048x2048 shadow map, six buffers, four bind groups, the capture
target and its readback buffer), recorded four passes inline, and read the
picture back. Every effect on the M12 list (fog, bloom, FXAA, cascaded
shadows, a blended pass) needs an intermediate target, a switch, or a place
in the pass order, and none of the three existed:

- there was no place to add a pass except in the middle of that function;
- nothing was kept between frames, so a player's every frame allocated and
  freed a 16 MiB shadow map and everything else;
- there was no settings type: the shadow map's size was a constant;
- there was no measurement of what a frame costs, so performance work would
  have been guesswork and an agent could not say why a scene is slow.

The goal of this package is the structure, **without changing any picture**:
every existing golden image must pass unrefreshed.

## 2. What the code said (map claims, checked)

| Claim in the area map | Checked |
|---|---|
| `render` creates the HDR, depth and shadow textures and all buffers on every call | True (`renderer.rs`, the `gpu.checked("draw the frame")` block). The debug pass did the same, and `describe` runs it up to 129 times. |
| The shadow pipeline has `fragment: None` and position-only vertices, so masked materials cast solid shadows | True. |
| `Renderer::pose` rebuilds the index and edge buffers of every animated primitive every frame | True: `GpuMesh::new` built the edge list through a `BTreeSet` and created three buffers. It happened again for the id and describe passes. |
| `render` uses `scene.lights.first()` and drops other directional lights silently | True. No check rule warned either (searched `wish_control`). |
| The hint of `alpha_blend_not_blended` says "arrives later (M6)" | True, and M6 is done. |
| `Frame` already carries `drawn` | True; nothing else was counted. |
| No render benchmark exists | True (`wish_cli` had `bench` and `bench_script.rs` only). |

Two facts the map does not mention, found while reading:

- **The world hash covers the list of registered types.** `World::hash`
  starts with the registry's fingerprint and the number of resource slots.
  Registering a new engine resource therefore changes the hash of every
  world that registers the engine's types. See section 6.
- `Target::read` takes the GPU lock itself, so a readback can never run
  inside `Gpu::checked`; the pass order has to respect that.

## 3. Decisions

### 3.1 Pass structure

Options:

1. A render graph: passes declare the resources they read and write, and the
   graph orders them and allocates targets.
2. A fixed list of named pass functions, called in order by `render`, each
   with its inputs and outputs as parameters; a *frame plan* built on the CPU
   first; a pool that hands out targets.

Decision: **2**. The order of a forward renderer's passes is known and
short; a graph would hide it behind a scheduler and solve an ordering problem
we do not have. What later effects need is a place to stand and targets to
draw into, and a plain list gives both while staying readable top to bottom.
If the list ever needs conditional sub-graphs (per-camera effect stacks),
the pass functions and the pool are exactly what a graph would schedule, so
nothing here is thrown away.

A frame is now:

```
plan      CPU only: draw list, shadow fit, light clusters, uniforms,
          sprites and interface batches, warnings, statistics
shadow    draw list            -> shadow map
sky       frame uniforms       -> HDR
opaque    draw list, shadow map, lights -> HDR, depth
tone map  HDR                  -> picture
overlay   sprites, particles, depth -> picture
interface interface batch      -> picture
readback  picture              -> RgbaImage
```

`Renderer::render` is that list, about seventy lines. Files, all under
`crates/wish_render/src/renderer/`:

| File | Holds |
|---|---|
| `pipelines.rs` | Bind group layouts and render pipelines, built once per renderer. |
| `models.rs` | Uploaded meshes and models, `upload_model`, posing animated models. |
| `draw_list.rs` | `DrawList`: the frame's draw calls, each resolved to its mesh, textures, culling and alpha mode; `collect`. |
| `shadow.rs` | Whether a shadow map is drawn, its size, and the light's view fitted around the casters (CPU). |
| `frame.rs` | `FramePlan` (everything a lit frame draws, before any GPU call), `RenderStats`, `FrameResources` (its targets and buffers from the pool, and the frame's own bind groups over them). |
| `pool.rs` | `Pool`: targets and buffers kept across frames. |
| `warp_probe.rs` | A probe of the adapter, run by hand (section 3.2). |
| `passes.rs` | The passes, one function each, with a table of what each reads and writes. |
| `debug.rs` | The debug views, the identity buffer and `describe` (as before). |

The debug views keep using the same `DrawList`. A draw is resolved once, in
`collect` (mesh, material textures, double-sided, masked), so the lit pass,
the shadow pass and every debug view read the same answer instead of each
matching on the draw's source.

**How a later effect plugs in:** a function in `passes.rs` (or a file of its
own when it grows), a target asked from the pool by name, a field in
`RenderSettings` if it has a switch, and one line in `Renderer::render`.

### 3.2 Pooled targets

Options for where frame objects live:

1. A typed struct (`hdr`, `depth`, `shadow`, ...) recreated when the size
   changes.
2. A pool of named objects: a pass asks for a texture by name, size, format
   and usage, and for a buffer by name and contents.

Decision: **2**. A later effect asks for `"bloom half"` at half size without
touching a struct every pass shares.

Rules of the pool:

- **Textures** are keyed by name, width, height, format and usage. Up to
  four sizes per name are kept (the editor draws a scene view, a game view
  and an identity buffer of different sizes in turn); the least recently
  used goes first.
- **Buffers** are keyed by name and usage. They are filled through the queue
  every frame (never `create_buffer_init`) and are replaced by one of the
  next power of two when the contents do not fit; a buffer is never made
  smaller.
- **Pictures** (the capture target and its readback buffer) are pooled like
  textures, one name per format (`picture`, `debug picture`, `ids picture`).
- Anything not used for 64 calls of the renderer is dropped, so a window
  that was resized does not keep its old targets for ever.
- **A budget, enforced when a call ends** (review finding, section 7): the
  pool knows the bytes of what it holds, and when a call has gone well the
  least recently used objects are dropped until what is kept fits 256 MiB.
  What the call itself used always stays, however large (the next call is
  most likely the same picture). A 3840x2160 window's lit picture (20 bytes
  a pixel), its identity picture and a 2048 shadow map are 238 MiB, so one
  such view is drawn without making anything, and the sizes a dragged
  window edge leaves behind do not pile up beside it. The rule above only
  runs at the start of a *later* call, so alone it let a renderer that is
  not called again hold everything it last made.
- `Renderer::trim(keep_bytes)` lets go of more, what the last call used
  included, for callers that draw now and then: `capture_frame` and
  `describe_view` call it with 64 MiB, so a tool server that took one large
  picture does not hold it until some later call (the usual sizes stay and
  are reused). The editor's views go through `Workspace::render_picture`,
  which does not trim. `Renderer::kept_bytes` says what is held.
- **Bind groups are not kept.** Each frame builds its own over the pooled
  objects (four for a lit frame) and drops them when it is done. A group
  built in the frame cannot point at a view that a changed size replaced,
  and see "What the software adapter does" below.
- After any failed call (a GPU error, a failed readback) the pool is
  emptied, so nothing half-used is ever reused (a readback buffer that is
  still mapped, for one).
- The pool counts the textures and buffers it creates
  (`Renderer::pool_counts`, hidden from the documentation: for tests).
- **The device counts every texture and buffer** (`Gpu::created`; review
  finding, section 7): the pool's own count cannot see a pass that creates
  its target directly, which is the mistake the reuse tests exist to catch.
  Every creation in the crate goes through `Gpu::create_texture` and
  `Gpu::create_buffer`, and `tests/rules.rs` reads the source and fails
  when a file calls the device itself. The reuse tests and `wen bench
  --render` read this counter, so "a second frame creates nothing" means
  nothing.

Shared between the lit picture and the debug views: the depth buffer (same
size, format and use) and the draws' uniform buffer. A frame without a
shadow map binds a 1x1 map of its own name, so alternating between a lit and
an unlit scene does not make the real map come and go.

`Renderer::render` keeps taking `&self`: the pool sits behind a mutex that is
held for the whole call, readback included. Two threads sharing one renderer
therefore take turns, which the GPU lock made them do anyway, and the borrow
of uploaded models by the draw list stays simple. Lock order is always pool,
then GPU.

Everything still runs under `gpu_lock` inside `Gpu::checked`. Uniform buffers
are bound at their struct's size whatever the pooled buffer's capacity; the
light lists are bound whole (the shader reads only what the frame's counts
name, so what an earlier frame left behind the data is never read).

The sprite and interface passes take their atlas texture, their vertex
buffer and (sprites) their uniform buffer from the pool too, since the
review (section 7; the first version created them every frame, and the
pool's own count could not show it). What is *in* the atlas is still
rebuilt on the CPU and uploaded every frame; "sprites kept on the GPU" is
Phase 3. The sprite pass's two bind groups are over its one uniform buffer,
which the rule below allows.

Not pooled: a posed mesh still uploads its vertices every frame, one new
buffer for each skinned primitive (GPU skinning replaces that). The device's
counter puts it on record in a test that changes when it does.

#### What the software adapter does (found on the way; a rule for every later effect)

The first version of the pool also kept bind groups, under the identities of
what they bound. Every test of `wish_render` passed, goldens included. The
editor's tests then failed about every second run: after the camera turned,
the scene view showed the old picture. Narrowed down, with a probe that is
kept in the repository (`renderer/warp_probe.rs`):

> On WARP (the software adapter of Windows, the adapter the golden images
> are made on), of all live bind groups of **one layout** that bind a
> uniform buffer, every one reads the uniform buffer of the group **created
> last**.

| Adapter | Own layout each | One shared layout | Shared, bound with a dynamic offset |
|---|---|---|---|
| Microsoft Basic Render Driver (WARP), Dx12 | each its own | **the first group reads the last one's buffer** | each its own |
| NVIDIA GeForce RTX 3060 Ti, Dx12 | each its own | each its own | each its own |
| NVIDIA GeForce RTX 3060 Ti, Vulkan | each its own | each its own | each its own |

(`cargo test -p wish_render --lib warp_probe -- --ignored --nocapture`;
`WISH_PROBE_HARDWARE=1` and `WGPU_BACKEND=dx12|vulkan` for the other rows.
Windows 10 19045, wgpu 30.0.2. Samplers were probed the same way and are
not affected; a kept group over a texture only, the HDR picture's, stayed
right in the same experiments. Storage buffers were not probed on their
own: the rule is written for uniform buffers, and no group is kept anyway.)

Two layouts with equal entries are one layout to the device, so two
renderers on one device share theirs: with kept groups, the editor's window
drew through the uniform buffer of whichever renderer in the process had
made a frame group last. One renderer alone never showed it, because its
kept group *was* the last one made. The old code never kept a group, so it
did not show this; it had the gap of the second decision below instead.

Decisions:

- Bind groups are built in the frame that uses them and are never kept. It
  costs four small objects a frame (measured in section 5) and is the same
  on every adapter, so what is tested is what ships.
- **A frame is finished under the GPU lock** (`Gpu::finish`: submit, then
  wait). Not keeping groups was not enough. The software adapter executes a
  submitted frame while the next thread, having taken the lock, already
  makes its bind groups, and that changes what the executing frame reads.
  The lock used to be let go between the submit and the wait of the
  readback. Measured with the threaded test, eight copies of it running at
  once to keep the machine busy:

  | | Runs that drew a wrong picture |
  |---|---|
  | Everything made per frame, lock let go before the wait (how the code was before this package, emulated on the new code) | 31 of 48 |
  | Targets and buffers kept, groups per frame, lock let go before the wait | 2 of 24 |
  | The same, the frame finished under the lock (this package) | 0 of 144 |

  So **the code before this package had the gap too**: two renderers in
  one process drawing different cameras on two threads could show each
  other's frame, or a mix. No test did that, and one test at a time rarely
  hits the window. The wait costs nothing: the picture is read back right
  after anyway.
- **Rule for every later pass** (bloom's chain of down-sample passes is the
  first that will meet it): a frame must not hold two bind groups of one
  layout over *different* uniform buffers, unless the buffer is bound with
  a dynamic offset. Give such passes one uniform buffer with a dynamic
  offset per pass (as the draws' uniforms do), or put the numbers that
  differ into the same buffer.
- Two regression tests draw with two renderers on one device, in turn on
  one thread and on four threads at once; the first fails with kept
  groups, the second also without the wait when the machine is busy (it
  failed once in a run of the whole workspace before the wait was added).
- Because the threaded test only fails now and then, the rule itself is
  checked by reading the source (`tests/rules.rs`, review finding): the
  one `queue.submit` of the crate is in `Gpu::finish` (the probe of
  `draw_probe` uses it too now), next to the wait. The hand-run adapter
  probe is the one file that may make raw calls. Nothing can check by
  reading that no bind group is kept; that rule stays a rule.

These rules belong with the GPU rules in `CLAUDE.md` (not edited by this
package: another agent held that file): *build bind groups in the frame
that uses them and keep none; submit a frame with `Gpu::finish`; never two
groups of one layout over different uniform buffers in one frame unless
bound with a dynamic offset.*

### 3.3 RenderSettings

A resource in `wish_types::render`, registered with the other render types
(`wish_render::components::register`), re-exported by the SDK so that game
code can offer it as an option:

| Field | Type | Range | Default | Effect |
|---|---|---|---|---|
| `shadows` | `bool` | | `true` | Off: no shadow map is drawn and no light casts shadows, whatever its own `shadows` says. |
| `shadow_map_size` | `u32` | 64 to 4096 px | 2048 | The side of the directional light's shadow map. |

Only settings that exist today and take effect now. The defaults are today's
constants, so a scene without the resource draws exactly as before. The
renderer clamps the size itself (to the range, and to what the device
allows): game code can write any number into a resource.

The editor needed nothing: its inspector is generated from the schema (the
test that every engine field has a widget passes), type and field names are
not interface text, and a resource is not a tool or an operation, so there
is no parity row to add.

A per-camera override (the map's wording) is left for the package that first
needs one; nothing here prevents it.

### 3.4 Statistics

`RenderStats`, on `Frame` and `Description`, and under the key `stats` in the
answers of `capture_frame` and `describe_view`:

| Field | Counts |
|---|---|
| `width`, `height` | The picture's size. |
| `draw_calls` | Every draw call of the frame, all passes together. |
| `triangles` | The triangles those calls hand to the GPU. |
| `shadow_casters` | The draws into the shadow map; 0 when none was drawn. |
| `point_lights`, `spot_lights` | The lights that light some part of the view (the ones a cluster lists). |

They are counted from the frame plan, the same lists the passes draw, and
depend only on the scene, the camera and the picture's size. No time value
is in the answers of `capture_frame` and `describe_view` (Phase 0 rule): a
test compares the answers of two workspaces on two copies of a project,
pictures included, and they are equal. That is a claim about these two
tools only: `script_build` still answers `seconds` and `project_info`
`last_build.seconds` (the open follow-ups of ADR-027, not part of this
package). In `capture_frame` the statistics say what the returned picture
took (a debug view counts its own pass); in `describe_view` they say what
the lit picture of that view takes, from the one plan `describe` makes (its
draw list is also what the identity passes draw).

`wen bench --render` is where time is measured: it builds a fixed scene (a
grid of cubes and spheres, a ground, a sun with shadows, sixteen point
lights), draws it repeatedly on the hardware adapter when there is one (the
software adapter otherwise, or with `--software`), and prints the time of a
frame, drawn and read back. It is a command, not a tool.

### 3.5 Small fixes

- **More than one `DirectionalLight`:** warning `extra_directional_lights`
  (the first in entity order is the one drawn); in `docs::DIAGNOSTICS`.
  After the review (section 7):
  - it names the light that is used and the ones that are not **by their
    scene ids**. The renderer knows entities, not names, so a `RenderScene`
    carries `names` (filled in by the control plane; empty from `extract`)
    and warnings use `RenderScene::name_of`: `'moon'`, or `#3v0` for an
    entity nobody named, the form the tools use. The two older warnings
    about entities (`degenerate_transform`, `model_not_loaded`) use it too;
  - the control plane adds where the first unused light is in the scene
    file (`location.path`), when the picture is of a scene file;
  - it is said by every way of looking: the lit picture, every debug view
    and `describe` (`frame::light_warnings`, with `too_many_lights`).
    `describe` returns the warnings of the plan it makes for its statistics
    instead of throwing them away, so `light_clusters_full` and the
    interface's warnings reach `describe_view` as well.
- **`alpha_blend_not_blended`:** the hint no longer names a milestone.
- **Masked shadow casters:** a second shadow pipeline with a fragment stage
  (`vs_masked`, `fs_masked` in `shadow_pass.wgsl`) that samples the base
  colour's alpha and discards below the cutoff, used only for draws whose
  material masks. Opaque draws go through the old pipeline, unchanged, so
  their shadows are bit for bit the same. The texture is read at its
  **finest level** (`textureSampleLevel(.., 0.0)`; review finding): at the
  level the light's view would pick, a texture finer than the shadow map
  gives the pattern's mean alpha, which is under the cutoff everywhere for
  a sparse fence, and the fence cast no shadow at all. Limit, by design of
  a map that is coarser than the pattern: a regular pattern shows as
  patches of shade of the right density, not as the pattern. (The lit pass
  has the matching limit and is unchanged: a sparse cut-out seen from far
  away is read from a coarse level and thins out. Coverage-preserving mips
  or alpha to coverage are look decisions for the render lane.)
- **A shader file that lacks a function a pipeline needs** (review finding):
  the masked pipeline needs `vs_masked` and `fs_masked` of `shadow_pass`, so
  a shader override folder with a copy from before this package passed
  `wen shaders` and failed in the GPU as a generic error. `check_shaders`
  and `Renderer::with_shaders` now check a table of the functions each
  pipeline shader must define (`ENTRY_POINTS`; a unit test reads the
  pipeline code and fails when the table lacks one) and report
  `shader_entry_point_missing` in the shader's own file.
- **Posed meshes** reuse the uploaded primitive's index and edge buffers
  (`GpuMesh::with_vertices`); only the vertex buffer is new.

## 4. Tests

Unchanged pictures:

- every golden image in `wish_render/tests/golden` passes without
  `UPDATE_GOLDEN`;
- on top of the tolerance the goldens allow, the twelve captures the tests
  write were compared **byte for byte** with the ones written before the
  change: all twelve identical;
- `cargo test -p wish_render` and `cargo test --release -p wish_render`;
- the scenario goldens of `examples/islands` and `examples/bricks` and the
  editor's goldens pass in the workspace run (`cargo test --workspace` on
  the finished code: 774 passed, 0 failed, 16 ignored).

New tests (`wish_render/tests/foundation.rs` unless noted):

| What | How |
|---|---|
| Statistics of a known scene | The four-object scene: 10 draw calls, 3870 triangles, 4 shadow casters, worked out by hand; the same for any size; `describe` gives the lit picture's; the ids and wireframe views count their own pass. |
| Lights in view | A bulb and a spot in view and a bulb behind the camera: 1 and 1. |
| Sprites, particles, interface | `tests/sprites.rs`: 5 sprites and 50 particles are 110 triangles in 3 draw calls. `tests/ui.rs`: the interface is one draw call. |
| Reuse | Two frames of one size: the pool's counts do not move (4 textures, 7 buffers), and neither does the device's count of everything created on it. Another size: three textures and one readback buffer. Back to the first size: nothing is created and the picture is the first one, byte for byte. An identity buffer of a size already drawn makes only its own picture; `describe` draws all its identity pictures into one. |
| Sprites and the interface | `tests/sprites.rs`, `tests/ui.rs`: a second frame creates nothing on the device; a frame with fewer sprites or boxes after one with more, drawn by the same renderer, is the picture a fresh renderer draws (nothing of the earlier frame is read from the kept buffers). |
| What is not pooled | `tests/animation.rs`: a posed frame creates one buffer, the bar's vertices, and nothing else. |
| Budget | Unit tests in `pool.rs`: which entries go over a budget (oldest first, larger first among equals, the ending call's never); a pool with a small budget lets go when a call ends and keeps all of a call that is larger than the budget; the bytes of each kind of object. `foundation.rs`: `Renderer::trim` drops the largest first, the next frame makes exactly those again and draws the same picture; `kept_bytes` is the sum of the targets. |
| GPU rules, by reading the source | `tests/rules.rs`: `queue.submit` only in `Gpu::finish`; textures and buffers created only through the counted helpers; no buffer mapped at creation; the scan finds each mistake in a text that has them. |
| Renderers sharing a device | Two renderers taking turns on one device, and four threads with a renderer each: every picture and identity buffer is the renderer's own view (the regression tests of section 3.2). |
| No stale target | Six sizes (more than the pool keeps) drawn forwards and backwards by one renderer, identity buffers in between: every picture equals the one a fresh renderer draws. |
| No stale buffer | Scenes with different draws and lights drawn in turn by one renderer: every picture equals a fresh renderer's. |
| Idle targets | A size not drawn for 80 calls is made again; one drawn 8 calls ago is not. |
| Shadows off | `RenderSettings.shadows = false`: the picture is byte for byte the one of a light with `shadows: false`; no shadow caster is counted; the ground in the block's shadow is lit. |
| Smaller shadow map | At 64 texels the shadow is where it was and as dark, and its edge is wider: walking across it in 2.5 cm steps, 0 steps are in between at 2048 and 18 at 64. A size of 0 draws as 64. |
| Masked caster | A quad with a half-clear texture above a ground, sun straight down: under the cut-out half the ground is lit, under the solid half it is dark; without MASK both are dark. (With the old shadow pass this test fails: 259 against 719.) |
| Fine masked caster | The same quad with a 256 px texture, one texel in four solid, under a 256 texel shadow map (a shadow texel is six texture texels): 66 of 289 points of the ground under it are in part shade, its mean brightness is between the solid sheet's and open ground's. (Sampled at the light's mip level this test fails: 0 of 289.) |
| Two lights | Two `DirectionalLight` entities: one `extra_directional_lights` warning naming the light that is used and the one that is not (by id without names, by scene id with them), with a hint; the same warning from `describe` and from four debug views; the picture is the one with only the first light; seven lights name four and count the rest. |
| Names | `degenerate_transform` and `model_not_loaded` name the entity by its scene id when the scene carries one. |
| Light warnings everywhere | 1025 bulbs in one place: `render` gives `too_many_lights` and `light_clusters_full`; `describe` gives exactly the same warnings and statistics; the `lights` view gives `too_many_lights`. |
| Shader functions | `tests/shader.rs`: the shadow pass as it was before this package, put in the library's place, is one `shader_entry_point_missing` problem in its file naming `vs_masked` and `fs_masked`, from `check_shaders` and from `Renderer::with_shaders` alike; a function of the right name in the wrong stage does not count. Unit test: the table matches the functions the pipeline code names. |
| Posed buffers | Unit test in `models.rs`: a posed primitive's index and edge buffers *are* the uploaded one's; its vertex buffer is not. |
| Pool and shadow rules | Unit tests in `pool.rs` (growth sizes, eviction order) and `shadow.rs` (size clamps, switched off, the fit at three sizes). |
| Tool answers | `wish_control/tests/view_tools.rs`: `stats` in both answers with the known counts; no key of an answer names a time; `RenderSettings` is in `schema_get` with its range, is found by searching "shadow quality", is set with `edit_apply`, is written to the scene file, changes the counts and the picture; a size out of range is refused. |
| The same in every run | Two projects made and edited the same way, a workspace each: the answers of `capture_frame` (lit, ids, depth, wireframe) and `describe_view` are equal, pictures included, and equal again when asked twice. |
| A second sun | An agent adds `moon` to the project's scene: `capture_frame` says `'moon'` is used and `'sun'` is not drawn, with the place of `sun` in the scene file; `describe_view`, three debug views and a session's picture say the same; without `sun` the picture is the same and nothing is warned about. |
| Memory after a tool call | With a 4096 shadow map (64 MiB by itself) a `capture_frame` or `describe_view` leaves less than 64 MiB behind; the usual sizes stay (16 MiB) and the next picture is the same. |
| `wen bench --render` | `wish_cli/tests/wen.rs`: 40 surfaces on the software adapter: 82 draw calls, and no texture or buffer created on the device after the first frame. |

## 5. Result

All exit criteria met; the commands and their output are in the package's
report. Measured on the development machine (Windows 10, NVIDIA GeForce RTX
3060 Ti, Vulkan, release build), `wen bench --render` with its defaults:
2500 surfaces, 1280x720, 60 frames, each drawn and read back. Six runs of
each, back to back; other programs were running, so each line gives the
range of the runs without its one outlier, and the outlier.

| | Frame, mean of a run | Median frame | Outlier run |
|---|---|---|---|
| Targets and buffers kept (this package) | 5.4 to 5.7 ms (about 180 frames a second) | 5.2 to 5.6 ms | 7.4 ms |
| The same binary, the pool emptied before every call | 9.8 to 10.7 ms (about 100) | 9.2 to 10.1 ms | 16.6 ms |

The frame is 5002 draw calls and 2 428 086 triangles (2500 surfaces twice,
the sky and tone mapping), 2500 shadow casters, 16 point lights. After the
first frame no texture and no buffer is created; with the pool emptied each
frame makes 4 textures and 7 buffers again. Other sizes of the same scene:

| Scene | Adapter | Frame, mean |
|---|---|---|
| 100 surfaces (`--entities 100`) | RTX 3060 Ti, Vulkan | 2.7 ms |
| 2500 surfaces | RTX 3060 Ti, Vulkan | 5.5 ms |
| 2500 surfaces (`--software`) | WARP | 666 ms |

What the numbers say about what comes next: a frame of 100 surfaces takes
2.7 ms and one of 2500 takes 5.5 ms, so a thousand surfaces cost about
1.2 ms (two draw calls each; in the lit pass a pipeline and two bind groups
are set for every one), on top of a fixed 2.5 ms or so that includes reading
the 1280x720 picture back. The presenter (release 1.2) takes the readback out
of the player's path; how much of the 2.5 ms it is was not measured here.
Sorting by pipeline and material, instancing and culling (the map's gap 9)
are measurable against this baseline now. On the software adapter the same
frame takes two thirds of a second: golden scenes have to stay small, as
the map says.

Also checked by hand: `wen view ... --exit-after 30` on the hardware adapter
and `--view wireframe --software --exit-after 10` open, draw and close
without an error (the window loop does not run in CI; nobody looked at the
pictures).

Handed on:

- the two rules of section 3.2 for `CLAUDE.md`'s GPU rules;
- the sprite and interface atlas is rebuilt on the CPU and uploaded every
  frame (Phase 3, "sprites kept on the GPU"; the texture and the buffers
  themselves are pooled now);
- posed meshes' per-frame vertex upload (GPU skinning);
- a per-camera override of `RenderSettings`;
- a `wen check` rule for a second `DirectionalLight` (the warning needs a
  picture or a description today; `check` has no rules about what a scene
  means yet, and adding the first changes what every project's check says);
- a sparse cut-out far from the camera thins out in the lit picture, and a
  regular one under a much coarser shadow map casts patches (section 3.5);
- keeping bind groups would save four small objects a frame. It is possible
  where every uniform buffer in a kept group is bound with a dynamic offset
  (the probe's third column); not done, because the gain is not measurable
  next to 5000 draw calls and the rule is easy to break unseen.

## 6. A consequence to know about: world hashes change

Registering `RenderSettings` changes the registry fingerprint, and with it
the hash of every world that registers the engine's types, even when no
scene uses the resource. This is how the hash has always worked (every
milestone that added an engine type did the same), and nothing in the
repository records such a hash: the recorded values are in `wish_core`,
`wish_ecs` (its own test types) and `wish_physics` (transform and physics
types only), and replays in tests are recorded and played by the same build.
The workspace's tests pass unchanged.

A replay file recorded by an earlier build stops matching at its first tick.
Before 1.0.0 nothing is frozen (ADR-027, rule 3), so each render package of
the short path that adds a type may move the hash in the same way; this
package is the first. The format window, with its migration, is release 1.2,
and committed replays are a probe of release 1.1: they are recorded after
1.0, so no committed replay crosses this break.

## 7. Closing review

An independent review of the package gave twelve findings (three of them the
same point seen from different sides, two another). Each was checked against
the code; all were real. What was done, 18 November 2026:

| Finding | Checked | Done |
|---|---|---|
| The masked shadow pass samples the base colour at the mip level of the *light's* view, so a cut-out finer than the shadow map is under the cutoff everywhere and casts no shadow (before the package it cast a solid one) | True: reproduced with a test first (0 of 289 ground points shaded) | `fs_masked` reads level 0. Regression test with a 256 px pattern under a 256 texel map. The limit that remains is in section 3.5. |
| `describe` builds the lit plan for its statistics and drops the plan's warnings; the debug views say nothing about lights | True | `describe` makes one plan, draws its identity passes from the plan's draw list and returns the plan's statistics and warnings. The debug views add the scene's light warnings. Tests in `foundation.rs` and `view_tools.rs`. |
| `extra_directional_lights` names the light by a raw entity id, which no answer maps to a scene id | True (`3v0`) | `RenderScene::names`, filled by the control plane; the message names the used and the unused lights by scene id and says how the order is decided; the control plane adds the place in the scene file. The test removes the light the message calls unused and gets the same picture. |
| The pool only lets go at the start of a later call and has no limit in bytes, so an idle renderer holds what it last made | True | Bytes are tracked; a budget of 256 MiB is enforced when a call ends; `Renderer::trim` for callers that draw now and then, used by the two tools with 64 MiB. Unit tests and a control plane test. |
| Nothing checks that a replaced shader has the functions the pipelines need | True | `ENTRY_POINTS` table, `shader_entry_point_missing`, in `check_shaders` and `Renderer::with_shaders`. |
| "No time value is in any tool answer" is false (`script_build`, `project_info`), and the test that guards it only reads key names | True | The sentence is scoped to the two tools in `MCP_API.md`, here and in the code; the never-built `profile_get` row is marked. The test compares whole answers from two workspaces, and the key filter knows bare `seconds` and `ms`. `script_build` and `project_info` themselves are ADR-027's follow-ups, not this package's. |
| The "objects created" counter only counts the pool, so it cannot see a pass that creates its own target; the sprite pass's per-frame uniform buffer (named by the specification) was still created every frame | True | `Gpu::created` counts every texture and buffer; `tests/rules.rs` keeps creation in one place; the reuse tests and the benchmark read it. The sprite and interface passes now take their atlas texture and buffers from the pool (a second frame creates nothing); posed vertices are the one thing left, pinned by a test. |
| The rule "submit with `Gpu::finish`" is guarded only by a test that fails now and then (not confirmed by the reviewer) | True as far as it can be checked: the threaded test has no assertion about the wait | `tests/rules.rs` reads the source: one `queue.submit` in the crate, in `Gpu::finish`. `draw_probe` uses `Gpu::finish` too. The three rules still have to go into `CLAUDE.md`'s GPU rules (that file was another agent's while this was written). |

Not done, with the reason: a `wen check` rule for a second `DirectionalLight`
(suggested twice as something to consider). `check` reads files, schemas and
references today and has no rule about what a scene means; the first such
rule changes what every project's check answers and belongs in a package of
its own. The warning now reaches every picture, every description and the
editor's status line.

### Second review

The fixes above went further than the first review's findings (the counted
creation helpers, the pool's byte budget, the pooled sprite and interface
passes, the entry point table), so that layer was read again by two reviewers,
with each finding checked by a sceptic. Three findings were confirmed, all low,
and one claimed crash was refuted (the interface pass returns before it binds
an empty buffer). Done, 18 November 2026:

- `MCP_API.md` said every view reports what the lit picture does not draw.
  The debug views report the scene's light warnings; `light_clusters_full` and
  the interface's warnings come with the lit view and with `describe_view`.
  The sentence now says so.
- The hint of `extra_directional_lights` described the entity order as
  "`order`, then by id". The order is parents before their children, siblings
  by `order`, then by id, a prefab's entities after their instance. The hint
  and the catalogue entry are corrected.
- The warning's place had a path and no file, which reads as a pointer into
  the call's arguments. It now names the scene file too, and a checkpoint's or
  a preview's picture carries no place.

Verified before the commit: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo test --workspace` (792
passed, 0 failed, 16 ignored), `cargo test --release -p wish_render`; after the
three fixes the tests of `wish_render`, `wish_control` and `wish_cli` again
(305 passed, 0 failed). No golden image changed. Not run: Linux and lavapipe
(CI runs on `main` and on pull requests only).
