# M12: sky light, a sun disc, fog, colour grading, FXAA

Package 4 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 4), the first package that changes what a picture can look
like. Backlog: the graphics area of [M12-area-maps.md](M12-area-maps.md),
gap 2 without bloom, step 1 of gap 3, and four quick wins. It builds on the
pass structure of [M12-render-foundation.md](M12-render-foundation.md).

Recommended model and effort: Fable 5.2, high (ROADMAP, M12).

Status: built and tested on 18 November 2026 (section 6). Not committed by
the agent that built it (the package's instruction), and not reviewed yet.

## 1. Problem

The lit picture had one look. Ambient light was one colour on every surface
whichever way it faced, so the sides the sun does not reach had no shape.
Nothing faded with distance, so a level looked like a model on a table. The
picture came out of tone mapping as it was, with every edge a staircase.
ADR-012 names sky-lit ambient, fog, colour grading and FXAA in the baseline
of 1.0.

One rule decides the shape of the package. **Schema defaults are part of the
file format** (ADR-027, rule 1): scene files leave defaults out, so a default
that changed would change the look of every existing scene, example golden
and template thumbnail. The default look is switched once, at the look lock
(item 9 of the short path), signed off by the user by eye. So everything
here is **opt-in**: with every new setting at its default the picture is the
one it was, byte for byte, and no existing golden image is refreshed.

## 2. What the code said (map claims, checked)

| Claim in the area map | Checked |
|---|---|
| Ambient is one flat colour | True: `opaque.wgsl` added `diffuse_colour * frame.ambient.rgb / PI` to every surface. |
| "Metals reflect a blur-free gradient" | True, and it says more than the map did: the sky's reflection was already there (`environment_brdf * sky_radiance(reflected)`), on every surface and always at full strength, whatever `Ambient` said. It ignored roughness. A metal never saw the flat ambient at all (its diffuse colour is black). So "sky-lit specular" is not new light; what is new is that it follows roughness and the sky light's strength. |
| Quick win: "blend the sky's zenith and ground colours by the normal's Y" | Possible, but it leaves the horizon colour out, which is most of what a wall sees. The gradient's exact irradiance has a closed form that costs the same (section 3.1). |
| Quick win: FXAA is "pure arithmetic on the 8-bit image, so adapters agree" | Half true. The arithmetic is the same everywhere (every read is a `textureLoad`), but its input is the tone-mapped picture, which differs by up to 1/255 between adapters, and FXAA decides by thresholds. A pixel near a threshold can be smoothed on one adapter and not on another. That is inside the golden tolerance only as long as such pixels are few; it is not measured on lavapipe (section 6). |
| Fog in `opaque.wgsl` and `sky_pass.wgsl` from a small `Fog` resource; grading inside `tonemap_pass.wgsl` | Done that way. |
| "Changes every existing golden once" | Not in this package: see the rule above. It happens at the look lock. |
| "Move particles before tone mapping so they can glow" | Not here, and not in the bloom package either: the plan places "particles that glow" in release 1.9 (section 3.6). |
| At most four storage buffers per stage, the fragment stage uses three | Still three: the new numbers are in the frame uniform and in one small uniform of the tone mapping pass. |

Facts the map does not mention, found on the way:

- **The picture's HDR target cannot hold the sun.** A disc as wide as the
  real sun that gives 100 000 lux has a luminance of 1.5 billion cd/m². After
  exposure that is above what a 16-bit float holds; the target stores
  infinity, and tone mapping turns infinity into "not a number". The disc is
  capped (section 3.2).
- **The scene loader refuses values outside a field's range, game code does
  not.** A game system can write any number into a resource, "not a number"
  included. Every look setting is cleaned before it reaches a shader
  (`renderer/look.rs`), as `RenderSettings.shadow_map_size` already was.
- **An 8-bit sRGB picture read as light and written back is the same
  bytes** on the software adapter the goldens are made on (measured: section
  6). This is what lets FXAA run on the picture's own format and still leave
  a flat area untouched.
- **A branch on a uniform is not always free of side effects.** On the
  hardware adapter, adding two branches to the sky's fragment function
  changed 12 pixels of the plain sky by 1/255 (section 3, "How off is kept
  exact"). The software adapter did not show it.

## 3. Decisions

Where each setting lives, and why there:

| Setting | Where | Default (= today's picture) | Why there |
|---|---|---|---|
| Where ambient light comes from | `Ambient.source`: `Color` or `Sky` | `Color` | It is the ambient light's own choice, and it has to be a switch: the sky's light cannot be "0 of it" by default without another field saying which of the two is used. |
| How strong the sky's light is | `Ambient.sky_strength`, 0 to 10 | 1 (not used with `Color`) | Beside the switch it belongs to. 1 is what the sky's colours and luminance give by themselves. |
| The sun's disc | `Sky.sun_size_deg`, 0 to 45 degrees | 0: no disc | It is a part of the sky's picture. One number: its direction and colour are the `DirectionalLight`'s. |
| Fog | a new resource `Fog` (`tint`, `density`, `height_falloff`, `base_height`, `start`, `max_opacity`) | `density` 0: no fog | Six fields that belong together and to nothing that exists. A resource like `Sky`, which it takes its colour from. |
| Colour grading | a new resource `ColorGrading` (`contrast`, `saturation`, `temperature`, `tint`, `vignette`) | 1, 1, 0, 0, 0: nothing | The finishing of the picture, like `Exposure`, and separate from it so that "reset the grading" does not reset the exposure. |
| Anti-aliasing | `RenderSettings.antialiasing`: `Off` or `Fxaa` | `Off` | It trades cost for quality, which is what `RenderSettings` is for. An enum, not a flag, so that MSAA can join it if its measurement passes (ADR-012). |

All of them are resources of the scene, not of a camera: a per-camera
override is left for the package that first needs one, as the foundation
note says.

`renderer/look.rs` turns the scene's settings into the numbers the shaders
read (`Look`), and says which effects are on (`Effects`).

**How "off" is kept exact.** Neutral values alone (`x * 1`, `+ 0`) would
very likely give the same bytes, but "very likely" is not what the rule asks
for. Three ways were weighed:

1. A flag in a uniform that the shader branches on.
2. A second fragment function beside the old one, in the same shader file,
   and a second pipeline that only a frame with the effect uses.
3. Neutral values.

The first version used 1 everywhere. On the software adapter it was exact.
On the hardware adapter (RTX 3060 Ti, Vulkan) the surfaces and tone mapping
were exact too, but **the plain sky was not: 12 pixels of 921 600 differed
by 1/255** from the picture the old binary drew. The sky's fragment function
had gained two branches, and the driver's compiler worked out the same
gradient a little differently next to them. So:

- **The sky pass and the tone mapping pass use 2.** `fs_main` of
  `sky_pass.wgsl` and of `tonemap_pass.wgsl` is the text it was before this
  package, untouched; the sun's disc and fog are in `fs_look`, grading is in
  `fs_graded`, each with its own pipeline, chosen on the CPU. A frame that
  asks for nothing is drawn by the very same functions as before, on any
  driver. (A plain frame also has no grading buffer: the pool holds what it
  held.)
- **The opaque pass uses 1**: a flag for the sky's light and one for fog in
  the frame uniform. Its lighting code is a hundred lines that the next
  packages change (cascaded shadows, a blended pass); it must not exist
  twice. With the flags at 0 it runs the old instructions on the old
  inputs, and the measurement says that is exact on both adapters here.
- **FXAA is a pass that is not run.**

After that change the old and the new binary draw the showcase scene to the
same bytes on both adapters (section 6).

### 3.1 Sky light

With `Ambient.source: Sky` a surface is lit by the part of the sky it faces.

Options for the diffuse part:

1. Blend zenith and ground by the normal's Y (the map's quick win).
2. Nine spherical harmonics of the gradient, computed on the CPU.
3. The closed form for this sky.

Decision: **3**. The sky is a gradient that only changes with height
(`sky_radiance`: horizon to zenith and horizon to ground, each by the square
root of the height), so what a surface gathers only depends on how far up
its normal points, t = normal.y, and only the three zonal harmonics are not
zero. Worked out by hand, for the zenith, horizon and ground colours z, h, g:

```
E(t) / pi = (z + h + g) / 3  +  2/5 (z - g) t  +  5/168 (z + g - 2h) (3 t^2 - 1)
```

times the sky's luminance. Facing straight up that is 0.79 z + 0.21 h; the
exact integral is 0.8 z + 0.2 h. It is three lines in `sky.wgsl`
(`sky_irradiance`), needs no new uniform and no CPU table, and depends on
nothing but the frame's own sky colours: deterministic, and nothing from an
earlier frame.

The specular part, "in a simple honest form": a mirror shows the gradient
itself, as before; a rough surface gathers a wide part of the sky, which
tends to the sky's mean light around the reflected direction, and that mean
is the same `sky_irradiance / pi`. `sky_reflection` blends the two by the
roughness squared. No lookup texture, no pre-filtered cube: those come with
environment maps (release 1.1).

Both parts are scaled by `sky_strength` and by the material's occlusion, as
the flat ambient was. (Changed on 19 November 2026: `sky_strength` now
scales the diffuse part only, and the reflection is at full strength in
both modes; section 8.)

What the sky's light does not include: the sun's disc. The disc is the
picture of the `DirectionalLight`, whose light and highlight are already
computed; adding the disc to the sky's light or to reflections would count
the sun twice.

A number to know at the look lock: the default sky has a luminance of 20 000
cd/m², chosen so that the sky's colours show as authored at the default
exposure. That is about three times the mean of a real clear sky, so at
`sky_strength` 1 the default sky gives a top face 17 to 48 klux (red to
blue), where the default flat ambient gives 10 to 16. Whether the default
look uses a strength below 1 or a dimmer sky is a look decision, not made
here.

### 3.2 The sun's disc

Drawn by the sky pass where the directional light shines from, in the
light's colour.

- **Brightness.** A disc of solid angle Ω that gives the light's illuminance
  E has the luminance E / Ω: the disc and the light are one thing, and a
  wider disc is dimmer. That number is then held to **64 times a white
  surface after exposure** (six stops): the reason is the first fact of
  section 2, and no bloom should be fed more. With the default sun any disc
  under 14 degrees is at the cap.
- **Its edge.** A soft edge in light is not a soft edge in the picture: a
  disc 64 times brighter than white that fades linearly over a fifth of its
  radius still shows a hard rim where it crosses white. So the edge fades
  exponentially, from full strength at 0.8 of the radius to the brightness
  of a white surface exactly at the radius. The disc that shows is as wide
  as `sun_size_deg` says, however bright the light is, with a short glow
  outside it.
- **Below the horizon** there is no disc: there the gradient stands for the
  ground.
- The angle from the sun is `atan2(|d × s|, d · s)`, which keeps its digits
  for a disc of half a degree where `acos(d · s)` does not.
- A scene that asks for a disc and has no `DirectionalLight` gets the warning
  `sun_disc_without_light` (lit picture and `describe_view`), with a hint.

Limit: an orthographic camera looks at one point of the sky (the sky pass
aims its rays at the far plane, so with the default `far` they are within a
degree of each other), so the disc fills the background or does not show; a
short `far` makes a disc whose size depends on `far`. A scene seen through an
orthographic camera should leave it at 0, and the field's description says so.

### 3.3 Fog

Exponential fog whose thickness falls off with height:
`density * exp(-height_falloff * (y - base_height))`. With a falloff of 0 it
is plain distance fog; that is the default falloff, so that setting only
`density` does the predictable thing.

- **One function for surfaces and sky** (`fog.wgsl`, `fog_amount`): the
  integral of that thickness along the ray from the camera, in closed form,
  from `start` meters out. The opaque pass calls it with the surface's
  distance, the sky pass with 100 km. That is what "consistently" means
  here: a far hill and the sky behind it fade by the same law, so the hill's
  outline goes when the hill goes. Fog with no height falloff has no top and
  hides the sky as well; a falloff gives a sky that hazes at the horizon and
  clears overhead.
- **Colour.** The fog is the sky's horizon colour times `Fog.tint` (default
  white), at the sky's luminance. The horizon's colour is what a thick
  atmosphere scatters towards the eye, so by default far things fade into
  exactly the colour that is behind them and there is no seam; a test
  measures the seam (section 5). A tint is for mist of another colour.
- `max_opacity` keeps a share of far things, and of the sky, showing.
- Emissive surfaces are fogged like any other light from that surface.
- The mean of an exponential divides by the height gained, which is zero for
  a level ray. Both the general form and the level form are computed with
  safe denominators and one is selected (the shader rule of M3 WP3: no
  division by zero, and no early return around one). Exponents are held so
  that nothing overflows far below the fog's base.

Under an orthographic camera the fog on surfaces goes by depth: every ray is
parallel to the view axis and starts on the camera's plane, so a wall that
faces the camera is fogged evenly from its middle to its corners, and with a
height falloff each pixel's ray runs at its own height (review finding 2; the
first version measured from the camera's position there too, which drew a
vignette of fog).

Limits, by design: under a perspective camera distance is measured from the
camera's position; the sky behind an orthographic view is fogged along the
sky pass's own rays; point and spot lights do not light the fog and nothing
casts a shadow into it (that is volumetric fog, after 2.0).

### 3.4 Colour grading

In the tone mapping pass, **before** the tone curve, on exposed light: white
balance, contrast, saturation, vignette, then Khronos PBR Neutral as before.
Grading light instead of the finished picture keeps the curve's promise: the
result is always in range and highlights still roll off.

- **White balance** is one 3x3 matrix made on the CPU (`white_balance` in
  `look.rs`): a von Kries adaptation in CAT02 cone space from the white the
  two numbers name (along the daylight curve for `temperature`, across it
  for `tint`) to daylight white. Only `+ - * /` on doubles, so every machine
  makes the same matrix.
- **Contrast** is a power law around middle grey (0.18 after exposure): the
  same change in stops above and below.
- **Saturation** mixes each colour with its luminance (Rec. 709 weights).
- **The vignette** darkens light towards the corners: nothing in the middle
  third, black at the corners at strength 1. It was cheap, so it is in.

A graded frame has one more uniform buffer for these (80 bytes, `grade`), in
a bind group of its own layout: one group of that layout a frame, so the
software adapter's rule about groups of one layout holds. A frame that is
not graded has neither.

### 3.5 FXAA

One full-screen pass, `fxaa_pass.wgsl`, after tone mapping, lit picture
only. With `antialiasing: Fxaa` tone mapping writes a pooled picture
(`"tone mapped"`) and FXAA writes the picture; with `Off` tone mapping writes
the picture itself and no pass is added, as before.

- The algorithm is the "quality" one of FXAA 3.11 (edge by local contrast,
  direction from the 3x3 neighbourhood, a search along the edge both ways,
  the sub-pixel blend). The original leans on bilinear filtering for its
  half-pixel reads; here **every read is a `textureLoad`** and the two
  texels are averaged in the shader, so no sampler's precision takes part
  (the map's constraint for anything that feeds a golden). The search takes
  eleven growing steps and reaches 25 pixels each way.
- It reads the picture as light (the sRGB target decodes on read), judges
  edges by the square root of the luminance, and blends light. A pixel that
  is not on an edge is written back as it was read; there is no early
  return, the unchanged pixel is selected at the end.
- **Before the interface, the sprites and the particles** (next section).

### 3.6 Sprites, particles and the interface

They are drawn **after** the whole look, in that order, as before:

| | Fog | Grading | FXAA |
|---|---|---|---|
| Surfaces and the sky | yes | yes | yes |
| Sprites, particles | no | no | no (drawn after it) |
| The in-game interface | no | no | no (drawn after it) |

Why:

- **The interface** is not in the world: fogging or grading a score would be
  wrong, and FXAA would soften text. Its pixels are byte for byte the same
  with the look on (tested).
- **Sprites and particles are authored in display colours and drawn unlit**
  onto the tone-mapped picture; that is what "a sprite shows as it was
  drawn" has meant since M6. Their edges are texture edges and soft discs,
  already filtered, so FXAA has nothing to mend on them and would only blur
  pixel art. Fogging them where they are drawn now would need the fog's
  colour after tone mapping and grading, a second copy of both in another
  place.

The cost of this, said plainly: **in a foggy 3D scene a far sprite or spark
stands out**, and saturation 0 leaves them in colour. Release 1.9 moves
particles before tone mapping so they can glow (road-to-2-0.md, "particles
that glow"; an earlier version of this note said the bloom package would,
which the plan does not); there fog and grading reach them for free. Whether
sprites follow (a 3D billboard wants fog, a 2D game's art does not want
grading) is a decision for that release or the look lock, and one switch on
`Sprite` would settle it.

### 3.7 Seeing it from the control plane

- Every setting is in `schema_get` with its documentation, unit, range and
  default, is found by the words people use (`fog`, `haze`, `contrast`,
  `white balance`, `vignette`, `antialiasing`, `fxaa`, `sun disc`,
  `skylight`), and is set with `edit_apply` `set_resource`. A value outside
  its range or choices is refused with a hint.
- `stats` in `capture_frame` and `describe_view` has **`effects`**: the
  names of the effects the lit picture used, in pass order (`sky_light`,
  `sun_disc`, `fog`, `color_grading`, `fxaa`). A debug view answers `[]`.
  FXAA is also one more draw call and triangle in the counts.
- The new warning `sun_disc_without_light` is in `docs::DIAGNOSTICS`.
- By hand: the inspector is generated from the schema, so the two new
  resources and the four new fields are there with number boxes and choice
  lists, with no editor code. A test adds all five resources by hand and
  sets ten fields (section 5). No tool and no operation is new, so there is
  no parity row to add, and type and field names are not interface text, so
  the language files are untouched.

## 4. What it adds to the world's types

Every engine type moves the world hash (foundation note, section 6). This
package adds:

| | |
|---|---|
| New resources | `Fog`, `ColorGrading` |
| New enums (field types) | `AmbientSource`, `Antialiasing` |
| New fields on existing resources | `Sky.sun_size_deg`, `Ambient.source`, `Ambient.sky_strength`, `RenderSettings.antialiasing` |

All are exported by `wish_types` and re-exported by `wish_sdk` (the prelude
included), so game code can change the weather or flash the grading. A game
module built against an older SDK that uses `Sky`, `Ambient` or
`RenderSettings` has to be rebuilt (in the engine's own tree: `wen sdk`
first). No recorded hash of `wish_core`, `wish_ecs` or `wish_physics` is
touched: none of them registers a render type.

## 5. Tests

`crates/wish_render/tests/look.rs`, on the software adapter, unless noted.
Each effect is measured, not only compared with a golden.

| What | How |
|---|---|
| The defaults are the old picture | A scene with no look resource, one with all five present at their defaults, and one with the defaults spelled out and the fields that only matter when an effect is on set to other values: the same bytes, the same statistics, no effect on. Fog with `max_opacity` 0 is no fog. A renderer that drew with everything on draws the plain scene the same afterwards. |
| The old pictures, byte for byte | Outside the test suite: the twelve captures the existing tests write, compared with the ones written by the code before this package (section 6). Every existing golden passes without `UPDATE_GOLDEN`. |
| Sky light | A white box over a grey ground, sun straight down, seen from below, under a sky of three unmistakable colours. Flat ambient: the underside and the shaded ground are both bluish. Sky light: the underside takes the ground's red (more than 2.2 times its green as light, the formula says 2.8), the shaded ground the zenith's blue, the underside is brighter, half the strength is darker with the same hue, and the sky's own pixels are the same bytes. |
| Roughness | Two metal balls and no sun. The point of each nearest the camera reflects the horizon. Without sky light both show the grey horizon (the old behaviour: roughness ignored). With it the mirror is unchanged and the rough ball's green falls behind its red and blue: it gathers the zenith and the ground with the horizon. |
| Sun disc | Its middle is where the light comes from, the brightest thing in the picture, in the light's warm colour; 5 degrees are 17 to 24 pixels wide where 20 are expected, 12 degrees 43 to 56; some pixels are between disc and sky (the soft edge); twice the radius away every pixel, ground included, is the same bytes; a wider disc is not brighter. |
| No light, or a set sun | No `DirectionalLight`: `sun_disc_without_light` with a hint, from `render` and `describe`, and the picture of no disc. A sun below the horizon: the picture of no disc, no warning. |
| Fog by distance | Red blocks 4 m and 77 m away. Thick fog: the far block, the block high up and the sky are one colour, the horizon's. Light fog: the near one goes 2 to 30% of the way to it, the far one more than 75%. With `start` 10 the near one is the same bytes. With `max_opacity` 0.5 the far one goes about half way, and the sky is not the fog's colour either. |
| Fog by height, and the seam | With a falloff, the block 30 m up keeps much more of its colour than the low one as far away; without it both go. The sky at the horizon is the fog's colour and the sky overhead is not. Without fog the far ground and the sky above it are two colours; in a pink fog the rows on both sides of the horizon are one, and pink. |
| Grading | Saturation 0: no pixel of the picture has a colour. 1.6: the red cube is redder. Contrast 1.5: lit ground brighter, shaded ground darker; 0.6: the reverse. Temperature and tint move the grey ground warm, cool, magenta, green. Vignette 1: the four corners are black, the middle is within 1/255, the middle of the top edge is dimmed and not black. Unit tests in `look.rs`: the white balance matrix (identity for nothing, warm is warm, finite at the ends of the range), the matrix inverse. |
| FXAA | A bright tile turned 25 degrees on a flat dark sky. Without: no pixel between tile and sky. With: more than 60. **Every pixel whose eight neighbours equal it (more than 60 000) is the same bytes.** One more draw call, triangle and texture; a second frame creates nothing on the device. |
| What is drawn after the look | A lit scene with a sprite in it and an interface panel over it, under thick fog, saturation 0, vignette 1 and FXAA: every pixel of the panel, the edges of its text included, and the sprite's middle are the same bytes as without; the ground beside the sprite is grey fog. |
| Numbers that are not numbers | Every setting "not a number" or infinite: the picture of the defaults. Every setting far out of range: the picture of the range's end. |
| Golden images | One scene made in code (a ground, a red cube, a rough metal ball, a pillar and a block far away, a low sun ahead): `look_plain`, each effect alone (`look_sky_light`, `look_sun_disc`, `look_fog`, `look_grading`, `look_fxaa`) and `look_all`. |
| Tool answers | `wish_control/tests/view_tools.rs`: the schema of every setting (unit, range, default); the search words; five `set_resource` operations write exactly what was said to the scene file; `effects` in `capture_frame`, `describe_view` and a session's picture; a debug view has none; out-of-range values and unknown choices are refused with a hint; the warning without a sun; removing the resources gives the first picture back, to the byte. |
| By hand | `wish_editor/tests/parity_ops.rs`: a person adds `Fog`, `RenderSettings`, `Ambient`, `ColorGrading` and `Sky` from "Add resource...", types eight numbers and picks two choices; the scene file holds them, the picture uses all five effects, and the history has fifteen changes by `human`. |

Changed existing tests: two, each by one line, for the new field of the
statistics (`foundation.rs`: `effects` in the expected `RenderStats`;
`view_tools.rs`: `"effects": []` in the expected `stats`). Nothing else an
existing test expects has changed: a plain frame makes the same four
textures and seven buffers.

## 6. Result

Done on 18 November 2026, on the development machine (Windows 10, the
software adapter WARP on Dx12, and an NVIDIA GeForce RTX 3060 Ti on Vulkan).
The commands and their counts are in the package's report.

Verified:

- **No existing golden image changed** (`git status` shows no modified
  `.png`) and every one passes without `UPDATE_GOLDEN`, in debug and in
  release.
- **The old pictures, byte for byte.** The twelve captures the existing
  render tests write (`basic`, `lights`, `many_lights`, `probe`, `sprites`,
  `ui`, the four debug views, `view_colliders`, `view_lights`) were kept
  from the run before this package and compared with the ones the finished
  code writes: all twelve identical.
- **The showcase scene, old binary against new**: `examples/islands` at
  1280x720 drawn by the release binary built before this package and by the
  new one. The same bytes on WARP, and the same bytes on the RTX 3060 Ti.
  (Before the sky pass got its second function the hardware picture
  differed in 12 pixels by 1/255; section 3.)
- The scenario tests of `examples/islands`, its golden picture included,
  pass (`wish_control`, `the_islands_showcase_is_won_by_its_tests_and_replays`).
- **Software against hardware with everything on**, the two pictures of
  section 7: of 921 600 pixels, 13 and 8 differ by more than 2/255 (0.0014%
  and 0.0009%; the largest difference 35 and 8, at edges FXAA judged
  differently), where the golden tolerance is 0.5%. With nothing on, the
  same scene differs by at most 1/255 between the two.
- A flat pixel goes through FXAA unchanged to the byte on WARP: so reading
  the sRGB picture as light and writing it back is exact there.

Not verified:

- **Linux and lavapipe.** Nothing here ran on them. The new golden images
  were made on WARP and are compared on lavapipe in CI at 2/255 for 0.5% of
  the pixels; `exp`, `pow`, `atan2` and FXAA's thresholds are where the two
  could part. The FXAA test that flat pixels are unchanged **to the byte**
  rests on lavapipe's sRGB round trip being exact, which is not known; if
  it is not, that test fails there and the fix is to give the FXAA pass a
  view of the picture without the sRGB encoding.
- **What the effects cost.** `wen bench --render` was not run with them on
  (it needs a release build of the whole workspace). Sky light and fog are a
  few instructions a pixel; FXAA is one more pass of up to about fifty
  texture reads a pixel at an edge and nine elsewhere.
- **An orthographic camera** with a sun disc was reasoned about (section
  3.2), not drawn. Fog under one is drawn and measured since the review.
- The window (`wen view`, `wen play`) was not opened; they draw through the
  same `Renderer::render`.

Handed on:

- to release 1.9 ("particles that glow", road-to-2-0.md; not to the bloom
  package, as this list first said): particles move before tone mapping;
  fog, grading and bloom then reach them; decide whether sprites follow
  (section 3.6);
- to the look lock: which of these become the default (`Ambient.source`,
  `sky_strength` or a dimmer default sky, `sun_size_deg`, `antialiasing`),
  with the migration that writes the old values into existing files;
  presets;
- a per-camera override of the look settings;
- a `wen check` rule for a sun disc without a light (the warning needs a
  picture or a description today, like `extra_directional_lights`);
- the user guides do not mention the new settings yet (the "public face"
  package).

## 6.1 Review

One independent reviewer read the whole change (19 November 2026), without
building it. No high finding; one medium and five low. What was done:

1. **Medium, fixed.** Two tests asked for the same bytes from the sky pass's
   two fragment functions (`fs_look` normalized the ray before the gradient,
   `fs_main` does not). `fs_look` now passes the gradient the raw ray, as
   `fs_main` does, and those two comparisons allow 1/255. The test of the
   defaults compares one pipeline with itself and stays exact.
2. **Low, fixed.** Fog under an orthographic camera was radial; it goes by
   depth now (section 3.3), with a test
   (`under_an_orthographic_camera_fog_goes_by_depth`).
3. **Low, fixed.** `sun_disc_without_light` names its place in a picture of
   a scene file: that file and `/resources/Sky/sun_size_deg`.
4. **Low, fixed.** The pool's budget is 320 MiB (was 256): a 3840x2160 view
   with FXAA and a selection is 270 MiB. The bloom package adds targets and
   must look at it again.
5. **Low, decided: the names stay.** `Fog` and `ColorGrading` join the flat
   namespace game types share, so a game type of the same name with another
   layout is rejected. They stay unprefixed like `Sky`, `Ambient`, `Camera`
   and `Material`: the physics types got the `Body` prefix because games
   define `Velocity` as a matter of course, and a game's fog of war has
   other natural names. No example, recipe or evaluation task uses either.
6. **Low, fixed.** New tests: `Fog.base_height` (an inverted sign would have
   passed before), fog under an orthographic camera, FXAA on pictures one
   pixel wide or high.

Also from the review: the description of `Sky.sun_size_deg` says to leave
it at 0 under an orthographic camera (section 3.2). Noted and left: a shader
override folder with a stale `opaque.wgsl` alone compiles and ignores sky
light and fog while `effects` lists them (a developer's folder; the other
two passes fail loudly with `shader_entry_point_missing`).

## 7. To look at

The showcase game's scene, `examples/islands`, at 1280x720 on the software
adapter, drawn with the engine's own tools (`wen build`, then `wen call
edit_apply` and `wen call capture_frame`) on a copy of the project; the
example in the repository is not changed. The pictures are in
`C:/wish-build/look-1/` on the development machine, not in the repository:

| File | What |
|---|---|
| `before.png` | The scene as it is, from its own camera. |
| `after.png` | The same camera with the settings below. |
| `sun-before.png`, `sun-after.png` | The same two from a camera added behind the last island that looks back up at the sun (the scene's own camera has the sun behind it, so `after.png` cannot show the disc). |

The settings of the two "after" pictures, to taste:

```json
"Ambient": { "source": "Sky", "sky_strength": 0.3 },
"Sky": { "sun_size_deg": 4 },
"Fog": { "density": 0.015, "height_falloff": 0.2 },
"ColorGrading": { "contrast": 1.25, "saturation": 1.25, "temperature": 0.1, "vignette": 0.3 },
"RenderSettings": { "antialiasing": "Fxaa" }
```

The sky's own colours are the example's and were left alone.

## 8. Amendment, 19 November 2026: the sky's reflection at full strength

**Decision (the user's, 19 November 2026).** `Ambient.sky_strength` scales
the sky's light on surfaces (the diffuse part, `sky_irradiance`) and nothing
else. The sky's reflection (`sky_reflection`, the specular part) is at full
strength in the sky-light mode, times the material's occlusion only, as it
always was in the `Color` mode. Section 3.1 had scaled both.

**Why.** The default look of 1.0 is Natural, whose `sky_strength` is 0.3
(the default sky is about three times as bright as a real one, section
3.1). Scaling the reflection with it made every metal reflect 0.3 of the
sky it faces, so the gold coins of `hop` and `collect` and the ball of
`first-pixel` went dark and dull under the default look
([M12-look-lock.md](M12-look-lock.md), sections 10.4 and 10.10). The
strength exists to bring the sky's *light* to a real level; the
reflection shows the sky as the sky pass draws it, and a mirror that
showed a sky three times darker than the one behind it would be wrong at
any strength.

**Rough metals.** A rough surface reflects `sky_irradiance / pi`, the
sky's mean radiance around the reflected direction, which is never
brighter than the brightest part of the gradient it averages. Measured on
gold spheres at roughness 0.2, 0.5 and 0.9 under the default sky with a
sun (1280x720, hardware adapter; mean linear luminance of each sphere):
`Color` mode 0.233, 0.237, 0.198; sky light before the change 0.062,
0.099, 0.103; sky light after it 0.231, 0.237, 0.200. So the sky-light
mode now matches the `Color` mode within about 1%, with the roughest
ball a little less saturated (it mixes the zenith's blue and the ground
in, where the `Color` mode shows the gradient sharp at every roughness).
Not too bright.

**What it changes.** Only the sky-light mode, and in it only the specular
part: every picture with `Ambient.source: Sky` changes where a surface
reflects (metals most; a dielectric's 4% reflection a little). The
`Color` mode, every format-1 scene (read with a flat ambient) and the look
`plain` are unchanged to the byte. No file's values change, the scene
format does not change, and the recorded defaults do not change. The
*meaning* of `sky_strength` in a file that sets the sky-light mode does
change; such files have existed for a day (since package 4, 18 November
2026) and no release has been made, so there is no migration.

**Tests.** `look.rs::a_rough_metal_reflects_a_wide_part_of_the_sky` now
also draws its metals at `sky_strength` 0, 0.3 and 4 and asks for the same
picture to the byte; the new
`look.rs::the_sky_strength_scales_the_light_on_surfaces_and_not_the_reflection`
asks a matte ball to be darker at 0.3 than at 1 and a gold ball to be the
same. The pictures that changed are listed in
[M12-look-lock.md](M12-look-lock.md), section 10.10.
