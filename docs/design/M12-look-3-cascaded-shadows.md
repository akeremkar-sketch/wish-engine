# M12: cascaded shadows with masked casters

Package 6 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 6), the third package that changes what a picture can look
like. Backlog: the graphics area of [M12-area-maps.md](M12-area-maps.md),
gap 4. It builds on the pass structure of
[M12-render-foundation.md](M12-render-foundation.md) and follows the
patterns of [M12-look-1-sky-fog-grading-fxaa.md](M12-look-1-sky-fog-grading-fxaa.md)
and [M12-look-2-bloom.md](M12-look-2-bloom.md).

Recommended model and effort: Fable 5.2, high (ROADMAP, M12).

Status: built and tested on 19 November 2026 (section 6), and reviewed
(section 6.1). Not committed by the agent that built it (the package's
instruction).
Sections 1 to 5 were written before the code and then brought in line with
what the measurements changed; section 6 says what was measured.

## 1. Problem

The directional light has one shadow map, fitted around everything that is
drawn. Its texels are therefore as large as the scene is wide: a 2048 map
over a 12 m room has texels of 6 mm, over a 400 m level texels of 20 cm, and
one far object is enough to make it so. In any level larger than a room
every shadow is a blob, and the shadow right in front of the camera is as
coarse as the one on the horizon.

The answer every engine gives is several maps ("cascades"), each fitted to a
slice of what the camera sees: a small, fine one for what is near, larger and
coarser ones for what is far, where a pixel covers more ground anyway.
Shadows near the camera are then sharp whatever the scene's size.

The rule of the two look packages before this one holds: **schema defaults
are part of the file format** (ADR-027, rule 1). Cascades are opt-in, a
scene that does not ask for them is drawn by the single fitted map to the
byte, and no existing golden image is refreshed. Whether the default look
has cascades is decided at the look lock.

## 2. What the code said (map claims, checked)

| Claim in the area map | Checked |
|---|---|
| "The single map is fitted to the whole scene, so one far object blurs every near shadow" | True: `shadow::fit_light` takes the bounding boxes of every draw (`DrawList::casters`), whatever the camera sees. |
| "Masked foliage currently casts solid quads" | **No longer true.** The render foundation added a second shadow pipeline (`vs_masked`, `fs_masked` in `shadow_pass.wgsl`) that reads the base colour's alpha at the finest level and discards below the cutoff; `foundation.rs` has two tests of it. What was missing is only what this package adds: that it holds in every cascade. The cascades use the same two pipelines, so it does (section 3.5), and two tests say so. |
| "Depends on: persistent shadow targets, array texture or atlas" | The pool keeps targets. An atlas was chosen over an array texture (section 3.4). |
| "Keep the nearest comparison sampler and shader-side PCF so adapters compare the same texels" | Kept: the same sampler, the same nine whole-texel reads. |
| "Four depth renders per capture on WARP raise CI time" | Measured (section 6): 14 to 15 ms more for a 1280x720 frame on the software adapter. A cascade only draws the casters that can shadow it, so four cascades are not four times the draws; the new test file takes about 15 s there. |

Facts found on the way:

- **The light's matrix is a uniform of the shadow pass**, one per frame. A
  cascade needs its own, and the foundation's rule forbids two bind groups
  of one layout over different uniform buffers in a frame (section 3.4).
- **Every draw is a caster and a receiver.** There is no per-entity "casts
  shadows" switch, so "what can shadow this cascade" is decided by bounding
  boxes alone.
- **The debug views do not plan a lit frame.** They collect the draw list
  and draw it with a small uniform of their own, so a view that shows the
  cascades needs the cascades' numbers in that uniform (section 3.6).

## 3. Decisions

### 3.1 The settings

Two new fields of `RenderSettings`:

| Field | Type | Range | Default | What it is |
|---|---|---|---|---|
| `shadow_cascades` | `u32` | 1 to 4 | 1 | How many shadow maps the directional light has. 1 is the single map fitted around everything drawn, as before. 2 to 4 fit that many maps to slices of the camera's depth. |
| `shadow_distance` | `f32` | 0.1 to 10 000 m | 100 | With cascades: how far from the camera shadows are drawn. They fade out over the last tenth. Not used with one map. |

Each cascade is a map of `shadow_map_size` texels a side, so the existing
field keeps its meaning ("the side of a shadow map").

Why `shadow_distance` does nothing with one map: a default of 100 m that
applied to the single map would cut the shadows of every existing scene
larger than that. With one map the answer to "how far" is "everything", as
it always was.

Not added: a setting for where the cascades are split, and one for the
width of the blend between them. Both have one good value for almost every
scene (below); a field can be added later without a format change, a field
cannot be taken away.

Like every number game code can write, both are cleaned before use
(`shadow::count`, `shadow::distance`): 0 cascades draw as 1 and 99 as 4; a
distance that is not a number is 100, one out of range is the range's end;
and the distance is held to the camera's far plane.

### 3.2 The cascades

**Slices.** The camera's depth from 0 to the shadow distance `D` is cut at

```
split(i) = D * (0.75 * 1000^(i/N - 1) + 0.25 * i/N)        i = 1 .. N-1
```

the "practical split" of parallel-split shadow maps (Zhang et al. 2006):
three quarters logarithmic (each slice a fixed factor deeper than the last,
which is what keeps a shadow texel the same size *in the picture* at every
depth) and one quarter uniform (which stops the first slices from being too
thin to be worth a map). The logarithmic part is taken from `D / 1000`
instead of the camera's near plane, so the splits are fixed shares of the
distance: with four cascades 6.7%, 14.9% and 32.1% of it; with three 9.1%
and 24.2%; with two 14.9%. The camera's near plane should not move shadow
edges, and a person can know where the cascades are.

**A stable fit.** Each slice is enclosed in a **sphere** (for a perspective
camera the smallest one around the slice of the frustum, which has its
centre on the view axis; for an orthographic camera the one around the
slab). The map is a square around the sphere, seen from the light. Because
a sphere looks the same from every side, the square's size depends only on
the slice and the picture's shape, never on where the camera looks. The
square's centre is then **snapped to whole texels** of the map, in the
light's own frame. Together: when the camera turns or moves, the map's
texels stay where they were in the world and only different ones are
used, so a shadow's edge does not crawl. The price of the sphere is
resolution: a square around a sphere is larger than a tight box around the
slice (by about a third of its side for a usual field of view).

**Casters outside the slice.** A tree behind the camera shadows the ground
in front of it. Every draw whose bounding box, seen from the light, touches
the cascade's square is drawn into it, however far towards the light it is:
the map's near plane is pulled back to the nearest of them. Only draws that
miss the square, or lie wholly beyond the sphere's far side, are left out.
(So a cascade draws what can shadow it and nothing else; four cascades are
not four times the scene.)

**Bias.** Each cascade has the depth bias and the normal offset of the
single map, computed from its own texel size (two texels of depth, one and a
half texels along the normal), plus the pipeline's slope-scaled bias, which
has no unit.

**Filtering.** The 3x3 percentage-closer filter of the single map,
unchanged: nine reads through the nearest comparison sampler, each of one
whole texel, so every adapter compares the same texels.

**Which cascade a pixel uses, and the seam.** Two ways were weighed:

1. By depth: the pixel's view depth says which slice it is in.
2. By the maps: the first (finest) cascade whose map holds the pixel, with
   the filter's footprint inside it.

Decision: **2, with a blend.** The squares are larger than their slices, so
2 uses the finer map over more of the picture than 1 would, and it is the
only one of the two that can promise the filter never reads past a map's
edge. But where one cascade ends and the next takes over, the size of the
shadow's texels jumps (by two to three times), and an edge that crosses that
line would show a step. So the cascades are walked from the finest: each
has a weight at the pixel, 1 well inside its map and falling to 0 over a
band at its border (4% of the map's side, at least 2 texels), and takes its
weight of what the finer ones left. Nothing jumps: every weight is a
continuous function of the place. The sphere of a slice lies wholly in its
map's weight-1 part, so every visible point up to the shadow distance is
covered, and a point in a band has two maps read (18 texel reads instead of
9). What no cascade covers is lit.

The band and the few texels kept free beside it cost each map about 9% of
its side; without the band the seam would have to be hidden by something
else.

**The fade.** From 90% of the shadow distance to all of it the shadow fades
to lit, by view depth (the measure the slices use), so shadows do not end
at a line across the ground.

**An orthographic camera** gets the same slices and spheres (the sphere
around a slab of its box). Its pixels are the same size at every depth, so
it gains less from several cascades than a perspective camera does; what it
gains is a map fitted to what it sees instead of to the whole level.

**A camera in a small scene.** The cascades are fitted to the camera, not
to the scene: slices that reach beyond everything simply hold nothing
there. (Fitting them to the scene's extent as the camera sees it would make
the squares change size as the camera moves, and the shadows crawl.)

### 3.3 How "off" stays exact

With `shadow_cascades` 1 the frame is planned by the code that planned it
before (`shadow::fit_light`, untouched) and drawn into the texture it was
drawn into, with the commands it was drawn with. Three things do change
under it, and each was measured (section 6):

- **The opaque pass's fragment function has one more branch**: "if the
  frame has cascades, ask them; else ask the single map" around the one
  call of `shadow_visibility`. The function itself is the text it was. This
  is the choice the first look package made for this pass (its lighting
  code must not exist twice) and found exact on both adapters; the same
  measurement was repeated for this branch. Had it moved a pixel, the
  cascades would have got an entry point and pipelines of their own, as the
  sky and tone mapping passes did.
- **The frame uniform is longer** (the cascades' numbers after the fog's),
  and so is the debug views' uniform. Existing fields keep their places.
- **The light's matrix is bound with a dynamic offset** (section 3.4). For
  the single map the offset is 0 and the matrix is the one it was.

The shadow map's binding does not change: one depth texture, read through
the same sampler (section 3.4).

### 3.4 On the GPU

**An atlas, not an array.** The cascades are tiles of one depth texture
(two side by side for two cascades, two by two for three or four; with
three, one tile stays empty), each `shadow_map_size` texels a side, held to
what the device allows. Against an
array texture:

- the frame's bind group layout does not change, so the opaque pass binds
  what it always bound and "off" has one thing fewer to prove;
- the pool hands out plain 2D targets, which is all it knows;
- one render pass fills it: a viewport and a scissor rectangle per tile.

The price is care at the tiles' borders: the filter must not read the next
tile. It cannot: a cascade has weight only where the filter's footprint is
inside its tile (section 3.2), and a read with weight 0 counts for nothing.

**One matrix per cascade, one buffer.** The shadow pass's light uniform
holds a matrix for each cascade, 256 bytes apart, and the pass binds it
with a **dynamic offset**, as the draws' uniforms are bound. One buffer,
one bind group, the foundation's rule kept by the means it names. The
single map uses offset 0.

**The same two pipelines.** Solid and alpha-masked casters go through the
pipelines they went through; nothing in `shadow_pass.wgsl` changes. So a
masked caster casts its cut-out shape in every cascade by construction.

**The numbers the shaders read** are one struct, `Cascades`
(`cascades.wgsl`): for each cascade its matrix, its tile in the atlas and
its bias; and the count, the band, the fade. It is the last field of the
frame uniform and of the debug views' uniform, written by one function
(`write_plan` in `shadow.rs`), and read by one set of shader functions that
both the lit pass and the cascade view call. No shader divides by anything: the
maps are orthographic (no perspective divide), and every reciprocal is
worked out on the CPU from cleaned numbers.

**Memory.** Four cascades at the default size are a 4096x4096 depth
texture, 64 MiB instead of the single map's 16. A 3840x2160 view with every
effect on was 354 MiB (bloom note, section 3.5); with four cascades it is
402 MiB, and the pool's budget goes from 384 to **448 MiB** (`pool.rs`,
`BUDGET_BYTES`, with the sum in its comment).

### 3.5 Masked casters

Verified in the code (section 2) and by the two existing tests. New here:
a fence panel with a cut-out half near the camera and one far from it, each
in the cascade that covers it, cast their cut-out shape (section 5).

### 3.6 Seeing it from the control plane, and by hand

- **`capture_frame view: "shadow_cascades"`**: every surface in the colour
  of the cascade its shadow is read from (red, green, blue, yellow from the
  finest; the colours mix in a blend band), grey where it has no shadow
  (beyond the shadow distance, or no shadow map at all), white where the
  single map covers it. Surfaces are shaded a little by the way they face,
  so that the picture still reads as the scene. It is a debug view like the
  others (`View::ShadowCascades`, `fs_cascades` in `debug_pass.wgsl`), is in
  `View::NAMES`, and so in `wen capture --view`, `wen view --view` (and key
  8 in its window; 6 and 7 are the two views that had no key), and the
  editor's view list ("Shadow cascades").
- **`stats.shadow_cascades`** in `capture_frame` and `describe_view`: how
  many shadow maps the lit picture drew (0 without shadows, 1 for the single
  map). `stats.shadow_casters` counts the draws into all of them together.
  The `shadow_cascades` view answers the number of the lit picture whose
  maps it shows (it draws none itself, so its `shadow_casters` is 0); every
  other debug view answers 0.
- **A warning, `shadows_out_of_range`**: cascades are asked for and nothing
  that is drawn can shadow any place within the shadow distance, so no
  shadow map is drawn at all (`stats.shadow_cascades` is 0 although the
  settings ask for more). It speaks in the measure the distance uses,
  depth in front of the camera, and says how far the nearest thing is only
  when that explains the picture (something ahead of the camera, beyond the
  distance; a thing behind the camera or far to the side gets no number).
  It names both ways out (a longer distance, or one map), comes with the
  lit picture, with `describe_view` and with the `shadow_cascades` view,
  and in a picture of a scene file its place is that file and
  `/resources/RenderSettings/shadow_distance`.
  **Its limit:** it is what bounding boxes can prove. A ground that reaches
  under the camera is within reach of the cascades even when everything the
  camera looks at is beyond the distance; that picture has no shadows
  either and gets no warning. The `shadow_cascades` view is the answer
  there: it is grey. Not added: a warning for cascades asked for while
  shadows are switched off; that is a quality preset doing its job.
- `RenderSettings` is found by more of the words people use (`cascade`,
  `cascades`, `blurry`, `sharp`, `distance`).
- By hand: the inspector is generated from the schema, so the two fields
  are there as number boxes. A test adds the resource and types both, and
  picks the new view from the view list. The view's name in the editor is
  interface text (all 11 language files). The view list had room for seven
  names before it scrolled; it now shows all of its names at once (a view
  that has to be scrolled to is one nobody finds, and the test could not
  click it either). No tool and no operation is new (no parity row).

## 4. What it changes in the world's types

Two fields of an existing resource, `RenderSettings`; no new type. Like
every change to an engine type it moves the world hash of worlds that
register the engine's types (foundation note, section 6). A game module
built against an older SDK that uses `RenderSettings` has to be rebuilt; in
the engine's own tree run `wen sdk` after this change. No recorded hash of
`wish_core`, `wish_ecs` or `wish_physics` is touched.

## 5. Tests

`crates/wish_render/tests/cascades.rs`, on the software adapter, with small
pictures and small maps. Shadows are measured on the ground, where they
fall, and compared with the unshadowed picture. No comparison rests on a
difference of two or three steps of 255 at one pixel: a shadow and lit
ground are some 140 steps apart in every channel, how dark a shadow is is
read as a mean over 3x3 pixels, and an edge is the place where a walk along
the ground crosses the middle between the two. Pictures drawn through
different shadow maps are allowed 1/255; only pictures drawn by the same
passes with the same numbers are asked for equal bytes.

| What | How |
|---|---|
| The defaults are the old picture | A scene without the fields, with them at their defaults, and with `shadow_cascades` 1 and another distance: the same bytes and statistics. A renderer that drew cascades draws the plain scene the same afterwards, and so does a fresh one. The captures of the existing tests, byte for byte against the ones kept from before the package (section 6). |
| A near shadow is sharp | A 2 m block ten meters from the camera on a ground 400 m wide. The width of its shadow's edge, walked in steps of 2.5 cm, with four cascades of 256 texels, with one map of 2048 texels (sixteen times the texels of the four cascades together), and what one map of 256 leaves of the shadow. |
| No seam | Lit ground alone, to beyond the shadow distance, under a high and under a low sun: the picture with cascades is the picture without shadows (to 1/255), so no line, stripe or self-shadowed patch at any border. A wall's shadow 60 m long that crosses every border: as dark at every depth up to the fade, the ground beside it as lit; and its edge is where the wall puts it at every depth, to two and a half texels of the cascade in use there (which cascade that is, is read from the view). |
| The fade | The same wall from above: its shadow is whole at 33 m of depth, about half gone at 38 m and gone at 42 m with a shadow distance of 40 m, and never darker farther away. |
| Stability | The block's shadow from close by through cascades of 128 texels, from four cameras. A texel of the first cascade's map is 7.8 cm across the edge on the ground, and each other camera is placed to shift an unsnapped map by half a texel there, 3.9 cm (3.9 cm sideways; turned by three quarters of a degree; half of each): the edge's place, the mean over thirteen lines of the ground, is the same from all four to 2 cm, and the view says every line is read from the first cascade. |
| The band, the far side, the guard | Straight down on bare ground through cascades of 64 texels, with a lens that sees all of the first cascade's map: the first cascade's share of each pixel along the `shadow_cascades` view's middle row and column. At all four borders (left and right: the blend band; below: the band; above: the fade at the map's far side) there are mixed pixels and no jump; the map is 23.4 bands wide where it counts, which is what two texels kept free at its border make it; the far side's fade is as wide on the ground as the band. **The same measurement is taken on the engine's shader with each of the three taken out** (a hard switch at the border, a hard switch at the far side, no two texels), by replacing one line of `cascades.wgsl` in a shader library, and must fail each time. |
| Casters outside the slice | A tower 30 m high 20 m behind the camera, the sun over the camera's shoulder: its shadow lies on the ground in front of the camera, up to its tip. |
| Masked casters | Two fence panels with a solid and a cut-out half, one near and one far: in front of each, the ground under the solid half is dark and under the cut-out half lit; without the mask both halves cast; and the view says the two shadows are read from different cascades. |
| An orthographic camera | The block from straight above on the 400 m ground: the shadow is dark, its sides are where the block puts them, one map of the same size has lost it, and the view shows a cascade. |
| A camera inside a small scene | The foundation's 12 m scene with four cascades up to 300 m (the first slice alone is deeper than the scene is wide): the picture agrees with the one map's at six points in and around the shadow. |
| Hostile settings | 0, 5, 99 and 4 294 967 295 cascades; a distance of zero, negative, the smallest float, not a number, either infinity, 10^12; a map size of 0: the picture of the cleaned value, byte for byte. |
| The view | `shadow_cascades` along the ground away from the camera: red, green, blue, yellow, grey, each there and in that order; the sky black; where the view shows a cascade the lit picture has the shadow, where it is grey it has none; white with one map, grey without shadows; red, green, grey with two cascades. Its statistics say 4, 1 and 0 maps for those three scenes, and another debug view 0. |
| Statistics | `shadow_cascades` and `shadow_casters` for none, one, two and four maps of a scene whose third surface is out of every cascade's reach; draw calls and triangles add up; `describe` gives the lit picture's. |
| The warning | A scene with nothing in reach: the picture without shadows, no map drawn, `shadows_out_of_range` with both ways out in its hint and the depth of the nearest thing ahead, from the lit picture, `describe` and the view (and not from another view); following either hint mends it. Unit test: the only thing drawn 15 m behind the camera with a distance of 20 m, 150 m behind it, and within the distance in depth but 300 m to the side all get no number; 150 m ahead gets "150 m in front of it". |
| Reuse | A frame with cascades after one without creates one texture (the atlas) and one buffer (the matrices); a second creates nothing; other cascade settings of the same sizes and going back to one map create nothing. |
| Cost | `what_cascades_cost` (`#[ignore]`, by hand): a frame of 401 surfaces with one map, two and four cascades. |
| Unit tests | `shadow.rs`: the splits are fixed shares; the sphere holds its slice for three directions, both kinds of camera and three picture shapes, and its radius does not depend on the camera's place; every corner of a slice is in its map's weight-1 part at three map sizes; a point of the world is a whole number of texels from where it was in the map after the camera turned or moved; what can shadow a slice is drawn into its map and nothing else; the atlas's tiles and what a small device does to them; every number the shaders read is finite; cleaned settings; a light without a direction and a broken camera. |
| Tool answers | `wish_control/tests/view_tools.rs`: the schema of the two fields (documentation, ranges, defaults), the search words, a `set_resource` that writes exactly what was said, `stats.shadow_cascades` in `capture_frame` and `describe_view`, the view through `capture_frame` (red near the camera), a wrong view name lists the new one, refusal out of range, the warning with its hint from three ways of looking and its entry in the catalogue, and the first picture back to the byte when the resource is removed; the warning's place in the scene file, and none from a session. |
| By hand | `wish_editor/tests/parity_ops.rs`: a person adds `RenderSettings` and types both numbers; the scene file holds them, the picture uses four cascades, the history has three changes by `human`; and picks "Shadow cascades" from the view list. |

## 6. Result

Done on 19 November 2026, on the development machine (Windows 10, the
software adapter WARP on Dx12, and an NVIDIA GeForce RTX 3060 Ti on Vulkan).
The commands and their counts are in the package's report.

Verified:

- **No existing golden image changed** (`git status` shows no modified
  `.png`) and none is new: every one passes without `UPDATE_GOLDEN`, in
  debug and in release.
- **The old pictures, byte for byte.** The 29 captures the existing render
  tests write (the bloom's eight, the look's nine, `basic`, `lights`,
  `many_lights`, `probe`, `sprites`, `ui`, the four debug views,
  `view_colliders`, `view_lights`) were kept from a run of the code before
  this package and compared with the ones the finished code writes, in a
  debug and in a release build: all 29 identical files. So the longer
  uniforms of the lit pass and of the debug views, the branch in the opaque
  pass and the light's dynamic offset move no pixel on the software
  adapter.
- **The showcase scene, old binary against new, cascades off.**
  `examples/islands` at 1280x720 drawn by a release build of the code
  before this package and by one of the finished code: as it is, and with
  the six effects of the two look packages on. All four pairs are the same
  files, on WARP and on the RTX 3060 Ti. So the branch is exact on the
  hardware adapter too (the adapter on which two branches in the sky pass
  once moved twelve pixels), and the cascades did not need an entry point
  of their own.
- **A near shadow in a large scene** (a 2 m block ten meters from the
  camera on a ground 400 m wide, 320x240; the width of the shadow's edge in
  steps of 2.5 cm between dark and lit):

  | Shadow maps | Texels in all | Edge | The middle of the shadow |
  |---|---|---|---|
  | four cascades of 256 | 262 144 | 4 steps (10 cm) | dark (254 of 678) |
  | one map of 2048 | 4 194 304 | 15 steps (37 cm) | dark (254) |
  | one map of 256 | 65 536 | no edge | **no shadow at all** (678, the unshadowed ground) |

  One map of 256 has texels of a meter and a half there, and the map's
  bias, which is counted in texels, is larger than the block. Four cascades
  with a sixteenth of the texels of the 2048 map draw an edge almost four
  times as sharp as it does. (With four cascades of 2048 the edge is
  sharper than a step of the walk: measured once, 0 steps, not kept as a
  test.) By arithmetic, at the default sizes (2048, a distance of 100 m, a
  field of view of 60 degrees at 16:9) the four cascades have texels of
  0.8, 1.9, 4.0 and 12.6 cm; one 2048 map over a level 400 m wide has
  texels of 20 cm everywhere.
- **No seam.** Lit ground under cascades is the ground without shadows:
  the largest difference in any channel of any pixel is 0, under a sun at
  45 degrees and under one 11 degrees above the horizon (the test allows
  1). A wall's shadow from 4 to 30 m, across three cascade borders, is 256
  to 260 at every quarter of a meter (the unshadowed ground: 681).
- **Where an edge is.** A shadow's edge lies about a texel and a half
  inside where the geometry puts it (a lookup is pushed off the surface by
  that much; one map does the same). With cascades of 256 and a distance
  of 40 m the wall's edge, which belongs at x = 1 m, was at 0.92 to 0.94 in
  the second cascade, 0.86 to 0.92 in the third and 0.62 to 0.74 in the
  fourth. So **at a border the edge moves sideways by the difference of
  the two texel sizes**, spread over the blend band: with maps this small,
  20 cm at 22 m, three pixels of a 320x240 picture; at the default 2048,
  an eighth of that. This is the one thing left of a "seam", said plainly.
- **The fade** (a shadow distance of 40 m): the share of the shadow left
  is 1.00 up to 34 m of depth, 0.94 at 35.9 m, 0.40 at 37.8 m, 0.00 at
  41.4 m (in the picture's encoded brightness, so half the light is less
  than 0.5).
- **Stability.** From four cameras, each placed to shift an unsnapped map
  by half a texel across the edge (3.9 cm on the ground), the edge of the
  block's shadow is at x = 0.928, 0.926, 0.929 and 0.928 m: the same place
  to 0.3 cm (a pixel of those pictures is 1.5 cm). With the snap taken out
  of the fit it was at 0.899, 0.941, 0.938 and 0.939 m, 3.9 to 4.2 cm
  apart, and the test failed (section 6.1). The unit test says the same
  without a picture: after the camera turned or moved, a point of the world
  is a whole number of texels from where it was in each map (to 0.02 of a
  texel).
- **The band.** Along the view's middle row and column the first
  cascade's share goes from 0 to 1 over 9.2 pixels at the left and right
  borders and 13.0 at the lower border and at the far side, by at most
  0.11 and 0.08 from one pixel to the next; the map is 23.5 bands wide
  where it counts (23.4 by its numbers).
- **Masked casters**: the near fence panel's shadow was read from the
  second cascade and the far one's from the third, and both have the
  cut-out's shape.
- **Software against hardware, with cascades.** The pictures of section 7
  drawn on both adapters, with every effect on (FXAA decides by
  thresholds, as the first look note says): of 921 600 pixels, 694 differ
  by more than 2/255 with four cascades and 705 with one map in the level
  (0.08%); 28 and 22 in the showcase. The `shadow_cascades` view differs
  in 5 pixels of the level and in none of the showcase by more than 1/255.
  So the cascades add nothing to what the two adapters disagree about;
  the golden tolerance is 0.5%.
- **What it costs** (a frame drawn and read back, release build, the
  process held to four cores at low priority; `what_cascades_cost`, run by
  hand; 401 surfaces; cascades at the default 2048 and 100 m):

  | Adapter | Scene | Size | One map | Two cascades | Four cascades |
  |---|---|---|---|---|---|
  | RTX 3060 Ti, Vulkan | posts spread over 240 m | 1280x720 | 2.20 ms | 2.17 ms | 2.28 ms |
  | RTX 3060 Ti, Vulkan | the same | 1920x1080 | 3.97 ms | 4.29 ms | 4.27 ms |
  | RTX 3060 Ti, Vulkan | posts packed into 50 m | 1280x720 | 2.30 ms | 2.36 ms | 2.55 ms |
  | RTX 3060 Ti, Vulkan | the same | 1920x1080 | 4.31 ms | 4.32 ms | 4.30 ms |
  | WARP (software), Dx12 | posts spread over 240 m | 1280x720 | 179 ms | 183 ms | 193 ms |
  | WARP (software), Dx12 | the same | 1920x1080 | 382 ms | 391 ms | 405 ms |
  | WARP (software), Dx12 | posts packed into 50 m | 1280x720 | 277 ms | 283 ms | 292 ms |
  | WARP (software), Dx12 | the same | 1920x1080 | 599 ms | 604 ms | 614 ms |

  Draws into the shadow maps: 401 with one map; 194 and 234 with two and
  four cascades in the spread scene (most posts are beyond the distance,
  so the cascades draw *fewer* casters than the one map), 410 and 633 in
  the packed one. On the graphics card four cascades are within a quarter
  of a millisecond of one map, which is inside the run-to-run noise of the
  1080p rows. On the software adapter they cost 14 to 15 ms of a 1280x720
  frame (5 to 8%), most of it the clearing and filling of an atlas four
  times the size.
- The tests: section 5; all pass in debug and in release.

Not verified:

- **Linux and lavapipe.** Nothing here ran on them. No golden image is
  new, so nothing is compared across adapters by bytes; the tests that ask
  for equal bytes compare pictures drawn by the same passes with the same
  numbers. The margins that rest on a rasteriser's choices are the edge
  positions (allowed two and a half texels where one and a half were
  measured), the stability test (allowed 2 cm where 0.3 cm was measured,
  on a mean over thirteen lines), the band's width (allowed 3.5% where
  0.2% was measured) and the lit ground (allowed 1/255 where 0 was
  measured).
- **A viewport and a scissor rectangle inside a depth-only pass** on
  adapters other than the two above (Dx12 on hardware, Metal, GL): the
  atlas relies on them. They are WebGPU core.
- **Motion.** Stability is measured on stills from four cameras, and by
  the unit test of the texel grid. No sequence of frames was looked at:
  the staircase of a coarse cascade's edge, and the handover at a border
  as the camera moves, are things of moving pictures.
- **The windows.** `wen view` (and its new keys 6, 7 and 8), `wen play`
  and the editor's views were not opened; they draw through the same
  `Renderer`. The editor's view list was driven by its test, not looked
  at.
- **Large atlases.** Four cascades of 4096 are an atlas of 8192x8192, 256
  MiB: never drawn here. What a device with smaller textures does to the
  tiles has a unit test only. The 402 MiB of section 3.4 is arithmetic.
- **The cost on other machines**, and on an integrated graphics card,
  where filling 64 MiB of depth is not free.

What the measurements changed:

- The comparison "with and without cascades at the same map size" turned
  out to be no comparison: one small map loses the shadow altogether. The
  test compares with a map sixteen times as large and still wins.
- The seam test first asked for the edge "where the wall puts it" at every
  depth and learned that an edge sits a texel and a half inside; the claim
  and the test now say what is true (above).
- `shadows_out_of_range` was meant to say "everything you look at is
  beyond the distance". Bounding boxes cannot say that (a ground reaches
  under the camera); the warning says what they can prove, and its limit
  is in section 3.6 and in the code.
- The editor's view list scrolled from its eighth name on, which the
  by-hand test found by failing to click it.

Handed on:

- to the look lock: whether the default look has cascades, how many and
  how far, with the migration that writes the old values into existing
  files; presets (a cascade size smaller than the single map's would hold
  the cost on small machines);
- an edge moves by the difference of two texel sizes at a border (above).
  A normal offset that follows the surface's slope to the light, or a
  receiver-plane depth bias, would shrink the offset itself; both change
  the single map's picture too, so they belong to a look decision;
- the filter is still nine whole texels: a coarse cascade's edge is a
  staircase of three steps. A wider or weighted filter is a look decision
  with the same adapter question the bloom had;
- three cascades leave a quarter of their atlas empty (16 MiB at the
  default size);
- a tool call keeps at most 64 MiB of render targets for the next one,
  and four default cascades are exactly an atlas of 64 MiB: with them the
  atlas is let go after every `capture_frame` and made again by the next.
  The editor's views keep it. Raising that limit is a memory decision of
  the foundation's, not taken here;
- `shadows_out_of_range` cannot see the commonest case (above); what
  could is the depth buffer, read back, which no picture does today;
- a per-camera override of `RenderSettings`, as before;
- the user guides name the new view and do not explain the settings yet
  (the "public face" package);
- in the engine's own tree, run `wen sdk` after this change
  (`RenderSettings` has two more fields).

### 6.1 Review

One independent reader went through the change before it was committed
(the code, the shader arithmetic, the tests, this note). No high finding;
two medium and eight low. What was done with each:

1. **Medium: the picture test of stability would have passed with the
   snapping removed.** The reader worked out the unsnapped grid offsets of
   the first cascade for the test's four cameras: 0.63, 0.90, 0.91 and 0.85
   of a texel, so an unsnapped edge would have moved by about 2 cm, inside
   the 3.5 cm the test allowed. Fixed: the cameras are now placed to shift
   an unsnapped map by half a texel across the edge (3.9 cm on the ground),
   the edge's place is a mean over thirteen lines, and 2 cm is allowed.
   **Tried:** with the snap taken out of `fit_cascades` (built into a
   separate folder, nothing else changed) the four edges were at 0.899,
   0.941, 0.938 and 0.939 m and the test failed ("turned: the edge is at
   x = 0.941, it was at 0.899"); with the snap back they are at 0.928,
   0.926, 0.929 and 0.928 m and it passes.
2. **Medium: `shadows_out_of_range` could contradict itself.** "Nearest"
   was a straight-line distance to box centres while the distance is depth
   in front of the camera: with a distance of 20 m and the only thing drawn
   15 m behind the camera it said "nothing that is drawn is that near (the
   nearest is about 15 m away)". Fixed: the message says "in front of the
   camera", says what bounding boxes prove ("nothing that is drawn can cast
   a shadow into that part of the scene"), and gives a number only when it
   explains the picture: the depth of the nearest thing ahead of the
   camera, when that is beyond the distance. The failing case and two more
   are in the unit test.
3. **Low: `stats.shadow_cascades` was 0 in the `shadow_cascades` view.** An
   agent taking that view of a scene with four cascades read 0 under the
   name of what it was looking at. Fixed: the view answers the number of
   the lit picture whose maps it shows; the other debug views answer 0;
   the tool's description and `MCP_API.md` say which is which.
4. **Low: the warning had no place.** Fixed: in a picture of a scene file,
   that file and `/resources/RenderSettings/shadow_distance` (the field
   the hint names first); a session's picture carries none.
5. **Low: no test failed with the blend band, the fade at the far side or
   the two texels at a tile's border taken out of the shader** (a hard
   switch still passed the seam test). Fixed: a new test measures the
   first cascade's share pixel by pixel in the view (section 5), and takes
   the same measurement on the engine's shader with each of the three
   taken out, on every run. Measured: with the engine's shader, bands of
   9.2 and 13.0 pixels, steps of at most 0.11, the map 23.5 bands wide;
   without the band, no mixed pixel and a step of 1.0 at three borders;
   without the fade, the same at the far side; without the two texels, the
   map 24.9 bands wide. Each of the three fails the measurement.
6. **Low: a thin margin in the sharpness test** (the wide edge had to be
   at least twice the sharp one plus four steps, which a legal sharp edge
   of six fails). Fixed: constants, at most 7 steps and at least 11
   (measured 4 and 15).
7. **Low, written down: a default four-cascade atlas is made again by
   every `capture_frame`.** A tool call keeps at most 64 MiB of render
   targets and that atlas is exactly 64 MiB. It costs time, not
   correctness: the atlas is cleared at the start of its pass whoever made
   it. The editor's views keep it. (Section 6, "Handed on".)
8. **Low, written down: four cascades of 4096 overrun the pool's budget in
   a 4K editor.** That atlas is 256 MiB where the budget of 448 MiB was
   worked out for four cascades of 2048 (64 MiB). The pool never refuses
   what a call needs; it lets go of what other views kept, so the cost is
   targets made again, not a wrong picture. Never drawn here.
9. **Low, written down: a shader override folder from before this
   package.** An old `common.wgsl` beside the engine's `opaque.wgsl` is a
   validation error that points at `opaque.wgsl` (it names
   `frame.cascades`, which the old file does not have); an old
   `opaque.wgsl` compiles, never asks the cascades, and so draws no shadows
   when cascades are on (the frame's single-map numbers say "no map").
   The same class as the first look package's fog fields: a developer's
   folder, to be brought up to date from the engine's own files.
10. **Low: bookkeeping** (the status documents), done with the commit.

## 7. To look at

Drawn with the engine's own tools (`wen new`, `wen build`, then one `wen
do` of `edit_apply` and `capture_frame` calls), 1280x720, on the software
adapter; the projects were made in a work folder and removed afterwards.
The pictures are in `C:/wish-build/look-3/` on the development machine, not
in the repository, with the settings in `settings.json` and the level's
calls in `level.jsonl` beside them. The look of both scenes is the one of
the bloom note's pictures (sky light, sun disc, fog, grading, bloom, FXAA);
"before" and "after" differ in `shadow_cascades` alone.

| File | What |
|---|---|
| `before.png` | A level 400 m wide made of primitives (a fence 140 m long, crates, a ladder, houses, trees, four far towers), with one shadow map of 2048. |
| `after.png` | The same with `"shadow_cascades": 4` (the default distance of 100 m). |
| `close-before.png`, `close-after.png` | The crates' part of both, three times as large. |
| `cascades-view.png` | `capture_frame` `view: "shadow_cascades"` of the "after" scene. |
| `islands-before.png`, `islands-after.png` | The showcase `examples/islands` from its own camera, with one map and with `"shadow_cascades": 4, "shadow_distance": 40`. |
| `close-islands-before.png`, `close-islands-after.png` | The player's part of both, three times as large. |
| `islands-cascades-view.png` | The view of the showcase. |

**The showcase does not show the difference.** It is 32 m long, so its one
shadow map already has texels of 1.6 cm: the two pictures differ in 763 of
921 600 pixels by more than 2/255, and the player's shadow is as sharp in
both. That is the reason for the level: there the one map has texels of 20
cm, the fence's shadow is a row of faint squares, the crates and the ladder
cast nothing, and the tree's shadow on the house is a smudge; with cascades
each of them is a shadow (92 039 pixels differ by more than 2/255). The red
wedge on the left house in the view is right: the first cascade's map is a
square seen from the sun, and that corner of it reaches the wall.
