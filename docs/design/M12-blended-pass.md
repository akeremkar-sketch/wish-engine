# M12: an alpha-blended pass

Package 8 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 8). Backlog: the graphics area of
[M12-area-maps.md](M12-area-maps.md), gap 6 ("Alpha-blended mesh pass"). It
builds on the pass structure of
[M12-render-foundation.md](M12-render-foundation.md) and on the material
files of [M12-materials.md](M12-materials.md), whose `AlphaMode` was made so
that `Blend` is an addition. Decisions it rests on: ADR-027 (defaults are
file format; tool answers carry no time).

Recommended model and effort: Fable 5.2, high (ROADMAP, M12: render).

Status: built and tested on 19 November 2026 (section 6). Not committed by
the agent that built it (the package's instruction).

## 1. Problem

Glass, water, ghosts, fading objects and many downloaded models with glTF
`BLEND` materials were drawn solid, with the warning
`alpha_blend_not_blended`. There was no way at all to make a surface let
what is behind it show through, other than cutting holes in it (`Mask`).

Wanted: surfaces that let what is behind them show through, drawn correctly
with the rest of the frame (lit, shadowed, fogged, bloomed, tone mapped),
asked for by a material file, a glTF material, and if it fits, a plain
`Material`; and every scene that does not ask drawn exactly as before.

## 2. What the code said (map claims, checked)

| Claim | Checked |
|---|---|
| `BLEND` materials are drawn solid with a warning | True. `wish_import` reads `AlphaMode::Blend`; `upload_model` counted such primitives in `GpuModel::blended`, drew them like `Opaque` and `collect` warned `alpha_blend_not_blended` once per model. |
| Depends on the pass structure | True, and it is enough: a pass is a function in `passes.rs` and a line in `Renderer::render`. The HDR picture and the depth buffer the pass needs already exist (`FrameResources`); the depth buffer is stored after the opaque pass for the sprites. No new target is needed. |
| Depends on the material alpha mode | True. `MaterialFile.alpha_mode` is `Opaque` or `Mask`; `Look::of` turns `Mask` into a cutoff. |
| The sort must be a deterministic function of the scene | The draw list is built in scene order (`RenderScene::draws` is entity order, parents before children); each item has its matrix and local box in `DrawList::casters`, at the same index. A sort on the CPU over those is the same in every process. |
| Decide the identity buffer and `describe_view` | `describe` counts pixels per identity in one identity pass and measures each entity alone in another; the identity pass draws the same draw list as the lit picture. |
| What reads the HDR picture's alpha | Nothing: bloom, tone mapping and grading read `.rgb` (`textureLoad(..).rgb`), and FXAA reads the tone-mapped picture. |
| A plain `Material`'s alpha | It is in the component (`base_color` is a `Color` with `a`), reaches the shader, and is used only for a `Mask` cut-out, which a plain `Material` never has. Every scene in the repository has alpha 1. |

## 3. Decisions

### 3.1 How a surface asks for it

- **A material file:** `alpha_mode: "Blend"`, an addition to `AlphaMode`
  after `Opaque` and `Mask`. Old files say `Opaque` or `Mask` (or nothing)
  and load and save byte for byte. `alpha_cutoff` means nothing with
  `Blend`.
- **A glTF model:** a `BLEND` material is drawn blended. The warning
  `alpha_blend_not_blended` is gone (nothing about a `BLEND` material is left
  undrawn; what blending as such cannot do is in section 5) and its code is
  taken out of the diagnostics catalogue.
- **A plain `Material`:** **`base_color` alpha below 1 blends.** Options were
  (a) only material files blend, (b) a new field (`Material.blend` or an
  `alpha_mode`), (c) the alpha. (a) leaves fading objects impossible: game
  code changes a `Material` every tick, never a material file. (b) adds a
  row to a box the materials package already had to keep short, and makes
  "half see-through" two edits, the first of which does nothing visible.
  (c) is what the field already says ("Opacity: 0 is fully transparent, 1 is
  fully opaque", `wish_core::Color`), and its default 1 is what every scene
  has: nothing that does not ask changes. A material file keeps its explicit
  mode, as glTF does: its alpha does nothing with `Opaque`.

**This is a change of meaning of stored values** (ADR-027, rule 1: defaults
are file format). The defaults are unchanged (a `Material`'s alpha is 1, a
material file's mode is `Opaque`), so a file that leaves them out draws as
before; but a file that *writes* a `Material` alpha below 1, or a glTF model
with a `BLEND` material, drew solid before and now draws see-through and
casts no shadow, and an alpha of 0 or below now hides the entity. Nothing in
the repository writes such an alpha (every scene, prefab and test has 1). It
is acceptable before 1.0 because there is no release yet whose files people
keep (versions are `0.x`, ADR-027 rule 3), because the value now does what
its own documentation always said (`Color.a`: "0 is fully transparent"), and
because glTF says `BLEND` is blended: the old drawing was the known gap, with
a warning. After 1.0 a change like this would need the format window and a
migration. To catch the one surprising case, `check` warns
`material_invisible` when a scene or prefab file gives a plain `Material` an
alpha of 0 or below (the whole entity is not drawn; game code that fades
something changes the world, not the file).

Hostile alpha, cleaned on the CPU before it reaches a shader: for a plain
`Material`, an alpha that is not a number, or 1 or more, is opaque (as
before); below 0 is 0. For a blended material file or glTF material, not a
number is 1, and the value is held to 0..1.

### 3.2 Where the pass is

```
shadow    (blended surfaces cast none: 3.5)
sky
opaque    (blended surfaces left out)
blended   back to front, over the HDR picture, depth tested, no depth write
bloom, grading, tone mapping, FXAA
sprites and particles, interface
```

In HDR before bloom, so that a lamp seen through glass blooms and the glass
is graded and tone mapped with everything else; before the sprites,
particles and the interface, which stay over the finished picture. The pass
runs only when the frame has a blended surface: a frame without one records
exactly the commands it did before.

### 3.3 The same lighting code: one fragment function

The blended pass uses the opaque pass's own shader functions (`vs_main`,
`fs_main` of `opaque.wgsl`) through two more pipelines; there is no second
copy of the lighting that could drift. `fs_main` already computed the
surface's alpha (for `Mask`); it now returns it as the output's alpha
instead of `1.0`. In the opaque pass that alpha lands in the HDR picture's
alpha channel, which nothing reads (section 2), and the colour is computed
by the same instructions as before. Directional light with its shadow (one
map or cascades), the cluster's point and spot lights, ambient or sky light,
the sky's reflection, emission and fog are therefore exactly the opaque
surface's.

**Straight alpha, blended by the hardware:** `colour * a + behind * (1 - a)`
(`SrcAlpha`, `OneMinusSrcAlpha`). Premultiplied output would have meant a
second fragment function (the output multiplied by alpha, emission and
specular kept), so a second entry point that drifts, or a change to
`fs_main`'s colour. Straight alpha is glTF's `BLEND` ("over"), and a
surface whose alpha goes to 0 is gone entirely, highlights and emission
included, which is what fading objects and ghosts want. The cost: clear
glass loses its highlights with its alpha (a highlight on alpha 0.1 glass is
a tenth as bright). Premultiplied glass with full-strength reflections is
later work (section 5).

### 3.4 Order: back to front, decided on the CPU

The blended surfaces of a frame are drawn in the order of a key computed
from the scene: the depth of the centre of each surface's bounding box
along the camera's view direction (`-(view * centre).z`), farthest first;
on equal depth, the draw list's order, which is entity order (and the
order of a model's parts). Not a number counts as 0. `f32::total_cmp`,
`sort_by`: the same order in every process for the same scene and camera.
A surface's two sides: a double-sided blended surface is drawn twice, its
back faces first (front faces culled) and then its front faces, so a closed
see-through box shows its far inside wall behind its near wall. Within one
mesh nothing else is sorted (no per-triangle order: section 5).

Depth: tested against the opaque surfaces' depth (`Less`, as the opaque
pass), never written; so an opaque object in front hides a blended one, one
behind shows through, and two blended surfaces do not hide each other.

A pixel whose alpha is below 1/512 (a fully transparent texel, a surface
faded to 0) is not drawn at all, in this pass and in the debug views
(3.6): half a step of an 8-bit alpha, so a picture's alpha 1/255 still
counts.

### 3.5 Shadows: blended surfaces cast none

A shadow map holds one depth per texel: a blended surface in it shadows
completely. Water, glass and smoke, the usual blended surfaces, would black
out what is under them; a threshold (cast where alpha is at least one half,
like a `Mask`) shadows a pond at alpha 0.6 entirely and still gets glass
wrong at 0.5. So blended surfaces cast no shadow: the shadow pass leaves
them out (`shadow_casters` does not count them), in the single map and in
every cascade. They *receive* shadows like any surface (3.3). They stay in
the map's fit (the boxes the map is placed around), so a pane that sticks
out of everything else still receives. A frame in which nothing that is
drawn casts (only see-through surfaces) plans no shadow map at all, so no
empty shadow pass runs and `shadows_out_of_range` does not fire; a cascade
lists only surfaces that cast (review, 6.1). Something that fades out and should
keep its shadow while it does is a case for a `Mask` or an opaque copy;
coloured or partial shadows from blended surfaces are later work.

### 3.6 Identity, `describe_view` and the debug views

- **Identity buffer (picks, the `ids` view):** blended surfaces are in it
  like any surface, where they are drawn at all (alpha at least 1/512): a
  click on a glass pane selects the pane, as in every editor.
- **`describe_view`:** an entity is visible at a pixel when it is the
  nearest surface there, **or the nearest solid surface there** (what shows
  through glass). When a picture has blended surfaces, `describe` draws a
  second identity pass without them and counts both: a box behind a window
  is visible (its `visible_percent` is what it would be without the window),
  and the window is visible too. Each visible entity with a blended part is
  marked `"blended": true`. A picture without blended surfaces is described
  by exactly one identity pass, as before.
- **Depth and normals views:** blended surfaces are drawn like any surface
  (nearest wins), cut where alpha is below 1/512; these views show
  geometry, not colour.
- **Wireframe, colliders, lights, shadow cascades:** blended surfaces are
  surfaces like any other. The debug views draw the same draw list as the
  lit picture.
- **`stats`:** a new count, `blended_surfaces`: how many blended draws the
  picture has (a model counts each blended part). It is the same number in
  every view, so an agent can ask any view. In the lit picture a
  double-sided blended surface is two draw calls and its triangles count
  twice; `draw_calls` and `triangles` say so.
- **Two limits of the visibility rule** (review, 6.1): only the nearest
  surface and the nearest solid one count, so a see-through pane behind
  another see-through pane, with nothing solid between, is covered by the
  nearer pane and listed as hidden though it shows in the picture; and since
  a pixel can count for two entities, the `visible_percent` of overlapping
  entities can add up to more than 100.

### 3.7 GPU rules

No new texture, buffer or bind group layout: the pass draws into the HDR
picture and tests against the depth buffer the frame already has, and binds
the frame's group 0, its draw uniform group (by dynamic offset, as the
opaque pass) and each draw's material group. No bind group is kept; the
frame is still finished by `Gpu::finish`. The pool's budget is unchanged
(no target). No new shader entry point (`ENTRY_POINTS` unchanged). No
division is added to a shader. WebGPU core only (blend factors and cull
modes).

### 3.8 Hybrid parity, the control plane, the SDK

- The editor edits all three ways by hand: the material file's `alpha_mode`
  drop-down in the browser (made from the schema, so `Blend` is there), the
  `Material.base_color` colour picker and its `w` box in the inspector, and
  a glTF file is changed in the modelling tool. New interface text: none.
- `capture_frame` and `describe_view` give `stats.blended_surfaces`;
  `describe_view` marks blended entities. `docs/MCP_API.md`, the tools'
  descriptions, `SEARCH_WORDS` (glass, transparent, see-through, fade...),
  a concept hint for components people ask for that do not exist
  (`Transparency`, `Opacity`, `Glass`), and a recipe "A glass window".
- `AlphaMode` is already in `wish_types` and the SDK prelude has
  `Material`; the new variant needs no re-export. A changed enum moves the
  world hash only for worlds that hold it; no recorded hash holds a
  `MaterialFile`.

## 4. Tests

`crates/wish_render/tests/blended.rs` (software adapter, small pictures):

| Test | What it shows |
|---|---|
| Nothing asks: the same picture | A scene without blending drawn with an alpha of exactly 1, with `Opaque` files: the same bytes as before; `blended_surfaces` 0; a scene that turns blending on and off again draws the same bytes. |
| The mix | A white-lit flat plane of known colour at alpha 0.25, 0.5, 0.75 over an emissive background of known colour: in linear light, over a window of pixels, the mix `a * front + (1 - a) * back` within the tolerance across pipelines (the front measured alone, opaque; the back measured alone). |
| Order | Two overlapping blended planes of different colours: the same picture whichever entity is first in the file and whichever the scene lists first; the nearer one's colour dominates. |
| Opaque in front, behind | An opaque box in front of a blended pane hides it (same pixels as with no pane); one behind shows through (the pane's pixels differ from the pane alone and from the box alone). |
| Lit, shadowed, fogged | A pane under an opaque caster: darker in the caster's shadow (single map and cascades); fog moves a far pane towards the fog colour as it does an opaque one. |
| Bloom through glass | A bright emitter behind a pane with bloom on: glow around it (lift outside its outline), and the pane does not stop it. |
| No shadow from glass | A pane between the sun and the ground: the ground's pixels are the same as without the pane, as long as the pane is not in front of them; `shadow_casters` does not count the pane. |
| Identity and describe | `ids` view: the pane's identity where it is; `describe_view`: the box behind the pane visible with the same `visible_percent` as without it, the pane visible and `blended`. A surface at alpha 0: not in the identity buffer, listed hidden. |
| Debug views | Depth and normals views show the pane; wireframe counts it as any surface; `blended_surfaces` the same in every view. |
| glTF `BLEND` | A model made in the test with a `BLEND` material is drawn blended (the background shows through), with no warning. |
| Double-sided | A double-sided see-through box: back faces drawn (the inside far wall changes the picture compared with single-sided); two draw calls. |
| Hostile alpha | Not a number, negative, above 1, in a plain `Material` and a material file: the cleaned value's picture, no non-finite pixel. |
| Cost | `#[ignore]`: 200 overlapping panes, software and hardware. |

Control plane (`wish_control`): a material file with `Blend` through
`asset_create` and `check`; `capture_frame` and `describe_view` answers
(`blended_surfaces`, `blended`); the recipe (`tests/recipes.rs`). Editor
(`wish_editor/tests/parity_ops.rs`): `alpha_mode` set to `Blend` in the
browser by hand, and a `Material`'s alpha typed in the inspector, each one
call by `human`.

Old pictures: before any render code changed, the render tests' captures
(`target/tmp/captures`) were kept, and `wen` of the commit before drew the
six example projects in the lit, identity, depth and normals views on the
software and the hardware adapter (the hardware adapter's repeatability
checked first), and the Khronos gallery on both. After: the same pictures
with the new code, compared byte for byte.

## 5. Not in this package (later work)

- Order-independent transparency, per-triangle sorting, intersecting
  blended surfaces: the order is per draw (a model's parts are sorted, the
  triangles of one part are not).
- The sort key is the depth of each surface's box centre, so a large
  see-through surface and a small one in or above it can be drawn in the
  wrong order: a glass bottle standing over the far half of a wide water
  plane can have its centre farther from the camera than the water's
  centre, and is then drawn first, under the water, though it is above it.
  The way round today: split the large surface, or make one of them solid.
- Refraction, screen-space effects, coloured or partial shadows from
  blended surfaces.
- Premultiplied glass that keeps its reflections at alpha near 0, or a
  separate "transmission" amount.
- Sprites and particles are drawn over the finished picture: a particle
  behind glass is drawn over the glass, not through it. Particles in HDR are
  release 1.9 ([road-to-2-0.md](road-to-2-0.md)), which is where they can
  be sorted with blended surfaces.

## 6. Result

Measured on 19 November 2026 on this machine: the hardware adapter is an
NVIDIA RTX 3060 Ti on Vulkan, the software adapter WARP on Dx12. Every build
and test ran on four cores at below-normal priority.

**Nothing that does not ask changed.**

- *The render tests' pictures.* Before any code changed, `cargo test
  --release -p wish_render` ran on the commit before (`17cf7c8`) and its 47
  captures (`target/tmp/captures`) were kept outside the repository. After
  the change, the same run: **all 47 byte-identical**, every golden image
  compared equal, and `git status` shows no modified `.png`.
- *The examples, both adapters.* `wen` built from `17cf7c8` drew the main
  scene of the six example projects (bricks, collect, first-pixel, hop,
  islands, quiz) at 480x270 in the lit, identity, depth and normals views on
  both adapters: 48 pictures, drawn twice to check that the hardware adapter
  repeats itself (the two runs were byte-identical). The `wen` of this
  package drew the same 48: **all 48 byte-identical**, and every answer says
  `blended_surfaces: 0`.
- *Textured glTF models, both adapters.* The Khronos gallery
  (`tests/khronos.rs`: Box, Duck, DamagedHelmet, MetalRoughSpheres,
  NormalTangentTest; none has a `BLEND` material): **all 10 pictures
  byte-identical** between the test binary of `17cf7c8` and this package's.
- *Files.* `MaterialFile` gained a variant only; old files are read and
  written by the same code. The two tests that used `"Blend"` as an example
  of an unknown variant (`wish_asset/tests/materials.rs`,
  `wish_control/tests/material_tools.rs`) now use `"Glass"`.
- *The returned alpha of `fs_main`* (3.3) changed no byte of any of these
  pictures on either adapter.

**The mix.** A pane at alpha 0.25, 0.5 and 0.75 over a backdrop, both in the
straight part of the tone curve: the mean of 8x8 pixels is the mix of the
pane alone and the backdrop alone, `a * front + (1 - a) * back` in linear
light, within 1/255 in every channel (software adapter;
`a_see_through_surface_mixes_with_what_is_behind_by_its_alpha`). A material
file with `Blend` at 0.5 draws the same bytes as a plain `Material` at 0.5.

**Tests** (commands as listed, each run as described above):

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --release -p wish_render` | 189 passed, 0 failed, 6 ignored (measurements and the gallery); `blended` 14 of 14 |
| `cargo test --no-fail-fast -p wish_types -p wish_asset -p wish_import -p wish_render -p wish_control -p wish_mcp -p wish_editor -p wish_cli -p wish_sdk` (debug) | 626 passed, 3 failed, 13 ignored; the three are below, fixed and run again |
| again: `-p wish_control --test view_tools --test blended_tools --test recipes --test diagnostics_catalog`, `-p wish_editor --test parity_ops`, `-p wish_mcp` | all passed (`parity_ops` 18, `view_tools` 10, `recipes` 2, `blended_tools` 2, `wish_mcp` 20) |

The three failures of the first debug run:

- `wish_control` `view_tools`: it compares the whole `stats` object, which
  now has `blended_surfaces`; the key was added to the expected object.
- `wish_mcp` `resources_and_prompts`: the guide (`wish://docs/guide`) grew
  past its 12,000 bytes. The guide lists the first sentence of every tool's
  description, and `describe_view`'s first sentence had gained
  `blended_surfaces`; the guide's own new line went too. The guide now says
  nothing new: the description says it after its first sentence, and
  `docs_search` "glass window" finds the recipe (tested).
- `wish_editor` `a_surface_is_made_see_through_by_hand`: the test looked for
  the `base_color` row while the browser still showed the new material
  file's own `base_color`, and typed into that one. The test now sets the
  ground's alpha first.

**Cost** (`what_blending_costs`, `#[ignore]`, release: 200 panes of
3 m by 2 m overlapping in the middle of a perspective view, a ground and a
sun; a frame drawn and read back, the mean of 20):

| Adapter | Size | Solid | See-through |
|---|---|---|---|
| RTX 3060 Ti, Vulkan | 1280x720 | 2.98 ms | 2.91 ms |
| RTX 3060 Ti, Vulkan | 1920x1080 | 5.71 ms | 5.33 ms |
| WARP, Dx12 | 1280x720 | 2968 ms | 3007 ms |
| WARP, Dx12 | 1920x1080 | 6725 ms | 6794 ms |

The two columns are not the same work: solid panes are 200 more draws into
the shadow map, see-through ones are sorted and blended (overdraw: every
pane is shaded where it is not hidden, instead of the nearest one
only). On the hardware adapter the saved shadow draws outweigh the
blending; on the software adapter blending costs about 1 to 1.3 percent.
The sort is on the CPU and is part of both figures.

**Pictures** (in `C:/wish-build/look-5/`, not in the repository; drawn on the
RTX 3060 Ti at 960x540 with the calls in `calls.sh`, output in
`calls-output.txt`; the project copy was deleted afterwards):

- `before.png`: `examples/first-pixel` with a glass pane in front of the red
  cube, a glowing lantern with a point light behind the pane, a blue box in
  front of the golden orb, and bloom; the pane's material file says
  `alpha_mode: "Opaque"` and the box's alpha is 1: what every surface was
  before this package.
- `after.png`: the same scene after two calls: the file says `"Blend"`
  (alpha 0.22, double-sided) and the box's `base_color` alpha is 0.45. The
  cube, the lantern and its bloom show through the pane, the orb through the
  box; neither casts a shadow any more (`shadow_casters` 7 to 5),
  `blended_surfaces` is 2, `check` is clean and the picture has no warning.

**Not verified:**

- The see-through tests run on the software adapter only (as CI does); on the
  hardware adapter only the two pictures above were looked at. Lavapipe
  (CI on Linux) has not drawn them yet.
- The new pass by eye beyond those two pictures and the test captures:
  models with many blended parts, intersecting see-through surfaces, glass
  seen at grazing angles.
- `wen view` and `wen play` with see-through surfaces (the window loop does
  not run in tests).
- No real model run of the AI scorecard or `wish-evals run` (both spend
  quota). The full workspace gate was not run by this agent (the caller runs
  it once before the commit).

### 6.1 Review

One independent reader went through the finished package (read only: the
code, the tests, this note). No high finding; one medium, one low-medium,
three low. What was done with each:

1. **Medium: the change of meaning was not named.** A `Material` alpha below
   1 and glTF `BLEND` drew solid before and see-through now, without a
   shadow, and a negative alpha now hides the entity. Section 3.1 now says
   that this is a change of meaning of stored values under ADR-027 rule 1
   (the defaults are unchanged) and why it is acceptable before 1.0. New
   `check` warning `material_invisible` (in `docs::DIAGNOSTICS`), at the
   alpha's place (`/entities/<id>/components/Material/base_color/3`, with
   its line), when a scene or prefab file gives a plain `Material` an alpha
   of 0 or below; an alpha of 0.5 is not warned about. Test:
   `wish_asset/tests/materials.rs`, the material notes test.
2. **Low-medium: the by-hand test proved less than its name.** Its count and
   changed picture came from the ground's alpha alone; the `Blend` file was
   never used. `a_surface_is_made_see_through_by_hand` now goes: the
   ground's alpha 0.5 in the inspector (1 blended, the picture changes); a
   new material file named in the ground's `file` field by hand (solid by
   default: 0 blended, the picture changes again); the file's `alpha_mode`
   set to `Blend` in the browser (1 blended), then back to `Opaque` (0, and
   the picture is the solid one to the byte). Five calls, all by `human`.
   Then the file's own alpha is typed in the browser (0.3: still 1 blended,
   the picture changes again), and `Opaque` is chosen once more (0, and the
   picture is the solid one to the byte). Six calls, all by `human`.
   **A cause found on the way, in the test, not the editor:** at first the
   browser's `base_color` boxes seemed to take no click. They do. At
   1280x720 the asset panel (170 px high by default) shows its detail column
   in a scroll area only between y 604 and 693; the `base_color` row is drawn
   at y 706, under the status bar, where a click lands on the status bar
   (the AccessKit tree still lists the boxes there, with their rects). The
   first scrolling attempt overshot to y 586, under the panel's tabs. A
   person scrolls the panel or drags it taller (up to 420 px). The tests
   now scroll a browser row into the visible band first (`browser_box` in
   `parity_ops.rs`), and the package 7 test `a_material_file_is_made_and_
   changed_by_hand` now also sets the file's colour by hand.
3. **Low: the sort key and a large surface.** Written in section 5 and as a
   line of the recipe "A glass window".
4. **Low: two limits of `describe_view`'s rule** (a see-through pane behind
   another with nothing solid between is listed hidden; percentages can add
   up to more than 100): written in section 3.6.
5. **Low (the reader was not sure): an empty shadow pass.** Confirmed: a
   scene whose only surfaces are see-through planned a map (and cascades)
   around them and drew an empty shadow pass. Fixed in `shadow::plan`: it
   is told which draws cast (`DrawList::casts`); when none does, no map is
   planned; a cascade lists only draws that cast. The maps are still
   fitted around every draw, so see-through surfaces receive shadows.
   `shadows_out_of_range` cannot fire for such a scene (it needs a map
   plan), and for a scene with solid surfaces the plan is what it was.
   Test: `blended::a_scene_of_glass_alone_draws_no_shadow_map` (one map and
   three cascades: no map, no caster, no warning; with a solid ground, a
   map again).
6. **Docs:** `docs/ROADMAP.md` lists package 8 as done.

After the fixes: `cargo fmt --all` and `cargo clippy --workspace
--all-targets -- -D warnings` clean; `cargo test --release -p wish_render`
190 passed, 0 failed, 6 ignored, and its 47 captures again byte-identical
to the ones kept from before the package, no `.png` modified; debug `cargo test --no-fail-fast -p wish_asset -p wish_render -p wish_control -p wish_editor -p wish_mcp`: 560 passed, 1 failed (the by-hand test, finding 2), 8 ignored; after its rewrite `parity_ops` 18 passed, `wish_asset` `materials` 10, `diagnostics_catalog` 2; fmt and clippy clean again.

## 7. To look at

- `after.png` against `before.png` in `C:/wish-build/look-5/`.
- Whether clear glass should keep its highlights (premultiplied output,
  section 5): at alpha 0.22 the pane's reflection is faint.
- Whether see-through surfaces should cast a shadow at some alpha, or have
  a switch for it (3.5); a fading character loses its shadow at the first
  tick its alpha drops below 1.
- The asset panel at its default height shows about three rows of a
  material file at 1280x720; a file's colour is below the fold until the
  panel is scrolled or made taller (6.1, 2). A taller default, or the
  colour first, would help a first-time user.
- `describe_view` counts what shows through glass as visible (3.6); an
  agent that asks whether a wall hides the player gets "no" for a glass wall.
