# M12: HDR bloom

Package 5 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 5), the second package that changes what a picture can look
like. Backlog: the graphics area of [M12-area-maps.md](M12-area-maps.md),
the bloom part of gap 2. It builds on the pass structure of
[M12-render-foundation.md](M12-render-foundation.md) and follows the
patterns of [M12-look-1-sky-fog-grading-fxaa.md](M12-look-1-sky-fog-grading-fxaa.md).

Recommended model and effort: Fable 5.2, high (ROADMAP, M12).

Status: built and tested on 19 November 2026 (section 6). Not committed by
the agent that built it (the package's instruction), and not reviewed yet.
Sections 1 to 5 were written before the code and then brought in line with
what the measurements changed (the firefly ceiling, the fade of fine
levels, the test scenes' grey sky); section 6 says what was measured.

## 1. Problem

The lit picture is drawn in light: an emissive surface, the sun's disc and a
strong highlight are many times brighter than white before tone mapping.
Tone mapping then rolls all of them off to the same white, so in the
finished picture a lamp looks like a white sticker and the sun like a white
coin. In a camera, and in the eye, such light spreads a little around
itself, and that glow is how a picture says "this is brighter than white".
ADR-012 names bloom on the HDR target in the baseline of 1.0.

The rule of the first look package holds here too: **schema defaults are
part of the file format** (ADR-027, rule 1). Bloom is opt-in, a scene that
does not ask for it is drawn by the code that drew it before, to the byte,
and no existing golden image is refreshed. Whether the default look has
bloom is decided at the look lock.

## 2. What the code said (map claims, checked)

| Claim in the area map | Checked |
|---|---|
| "Emissive is already in physical units, so bloom pays off at once" | True: an emissive value of 1 is 30 000 cd/m² (`EMISSIVE_NITS`), 1.5 times a white surface at the default exposure, and the HDR picture holds it un-clamped (`Rgba16Float`, pre-exposed). The sun's disc is held to 64 times white for exactly this package (`look.rs`, `SUN_DISC_PEAK`). |
| "Sampler bilinear precision differs between WARP and lavapipe; do bloom taps with `textureLoad` and weights computed in the shader" | Followed: no sampler takes part in any bloom pass. The classic filters are written out as fixed kernels over whole texels (section 3.3). |
| "Move particles before tone mapping so they can glow" | **Not this package.** The plan places "particles that glow" in release 1.9 (road-to-2-0.md). Sprites, particles and the in-game interface are still drawn after the whole look and do not bloom. The first look note and `CLAUDE.md` said the bloom package would move them; both are corrected. |
| "A blur chain can push more than 0.5% of pixels past 2/255" (between adapters) | Not between WARP and lavapipe: there is no lavapipe on this machine. Between WARP and a hardware adapter no pixel of a 1280x720 picture with bloom differs by more than 1/255 (section 6). |

Facts found on the way:

- **The frame has one rule that a blur chain meets first** (foundation note,
  section 3.2): on the software adapter of Windows, two live bind groups of
  one layout over *different* uniform buffers read the buffer of the group
  made last. A chain of down-sample passes with a small uniform each would
  be exactly that.
- **The pool keeps four sizes of one name.** A chain whose levels shared a
  name would push its own levels out; each level has a name of its own.
- **A texture cannot be read and written in one pass**, so mixing bloom
  into the HDR picture needs either fixed-function blending or a second
  full-size target.

## 3. Decisions

### 3.1 The setting

A new resource, `Bloom`, beside `Fog` and `ColorGrading`:

| Field | Range | Default | What it is |
|---|---|---|---|
| `strength` | 0 to 1 | 0: no bloom | The share of the light that is spread around. The light is moved, not added: an ordinary picture keeps its brightness (the limit is in section 6.1). |
| `spread` | 0 to 1 | 0.5 | How far the glow reaches: 0 keeps it close to the light, 1 lets it reach across the picture like a haze. |
| `threshold` | 0 to 64 | 0: all light | Only light brighter than this blooms; 1 is a white surface in the finished picture. The soft start begins at half the threshold, so 2 keeps everything up to white sharp. For stylised looks. |

Three fields. A resource of the scene, like the other look settings (a
per-camera override is still left for the package that first needs one).
With `strength` 0 the other two do nothing.

Options for the model:

1. **A threshold and an add** (the bloom of the 2000s): what is brighter
   than a threshold is blurred and added. Simple, and wrong in two ways: the
   picture gets brighter as bloom is turned up, and the threshold is a
   magic number that every scene has to tune against its exposure.
2. **Energy-conserving scatter** (the bloom of current engines): a share of
   *all* light is spread by a wide, smooth kernel and the same share is
   taken from where it was. Nothing to tune against exposure: a white wall
   keeps its colour because what it loses it gets back from its neighbours,
   and a lamp 50 times white visibly lights the pixels around it because
   2% of 50 is a whole white.
3. 2, with a threshold as an option, for looks that want only the brightest
   things to glow.

Decision: **3**, with the threshold made part of the same model instead of
a second one. Each pixel's light is split into the part that scatters and
the part that does not (`scattered` in `bloom_pass.wgsl`: all of it with
`threshold` 0; with a threshold, the part above it, with a soft start half
a threshold wide so that a surface crossing it does not pop). The chain
spreads the scattered part, and the last step takes `strength` times the
scattered part from each pixel and gives `strength` times the spread light
back:

```
picture = light - strength * scattered(light) + strength * spread(scattered(light))
```

With `threshold` 0 that is `mix(light, spread(light), strength)`. With a
threshold the picture still keeps its brightness, and there is no jump
between "no threshold" and "a tiny one", which a switch between mixing and
adding would have.

### 3.2 Where it is in the frame

```
shadow -> sky -> opaque -> BLOOM -> tone map (grading) -> FXAA -> overlay -> interface
```

After the sky and the opaque pass, on the pre-exposed HDR picture, so fog
is already in the light that blooms (a lamp in fog blooms as a fogged
lamp); before colour grading and the tone curve, so the glow is graded and
rolls off with everything else; before FXAA. Sprites, particles and the
interface are drawn after the whole look and neither bloom nor are bloomed
over, as before (section 2).

**How "on" is wired without disturbing "off".** The last bloom step
(`fs_composite`) writes the picture with its bloom into a second pooled HDR
target, `"bloomed"`. The tone mapping pass is unchanged in every way: the
same two functions, the same two pipelines, the same two layouts; a frame
with bloom hands its bind group the bloomed picture instead of the HDR
picture. A frame without bloom runs no bloom pass, takes no bloom target
and builds the groups it always built. `fs_main` of every existing pass is
textually untouched.

The other way would have been to blend the bloom into the HDR picture in
place (fixed-function blending, no second target, 63 MiB less at
3840x2160). It cannot do the threshold of section 3.1: what a pixel gives
up depends on that pixel's own light, which a pass cannot read from the
target it writes. A second path for "no threshold" was not worth two ways
of doing one thing.

### 3.3 The chain

The standard shape (Jimenez, "Next Generation Post Processing in Call of
Duty: Advanced Warfare", 2014): the picture is halved again and again with
a wide filter, then the levels are added back up from the coarsest, each
through a small tent filter. Every level is a blur twice as wide as the
one before at a quarter of the cost; together they are one kernel with a
bright centre and a long soft tail.

- **Levels.** Level 1 is half the picture's size, each next one half of the
  one before, sizes rounded **up** (so every texel of the finer level is
  under some texel of the coarser one, whatever the size; a texel of level
  k covers texels 2i and 2i+1 of level k-1). The chain stops when a texel
  of the next level would be more than a quarter of the picture's height
  (a blur wider than that is one colour for the whole picture and only
  veils it), and at twelve levels. There is always level 1, so a 1x1
  picture has a chain of one 1x1 level. 1280x720 has seven levels,
  3840x2160 nine.
- **Down-sampling** is the 13-tap filter of that talk. Its taps are
  bilinear reads at texel corners, so each is the mean of four texels: the
  whole filter is a fixed kernel over a 6x6 block of texels,
  `0.5 * a⊗a + 0.5 * b⊗b` with `a = [0,1,1,1,1,0]/4` and
  `b = [1,1,2,2,1,1]/8`. The shader reads the 36 texels with `textureLoad`
  and weights them itself.
- **The first down-sample** reads the HDR picture and does three more
  things (section 3.4): it cleans each texel, takes the part of it that
  scatters, and averages with weights that hold a single very bright
  texel.
- **Up-sampling** is the 3x3 tent of bilinear reads of that talk, which for
  a level exactly twice as fine is a fixed 4x4 kernel: along each axis the
  four coarse texels nearest the pixel, weighted `2 - distance` (7, 5, 3, 1
  sixteenths). Again 16 `textureLoad`s and arithmetic.
- **Coordinates** are integers held inside the texture (the nearest texel
  inside stands for one outside, as in the FXAA pass). The weights are
  worked out from the place asked for, before it is held, so an edge texel
  simply counts for what lies beyond it. No pass divides by anything that
  can be zero and none returns early: the one division by a sum, of the
  first down-sample's weights, has a sum of at least 1/33, and the two in
  the threshold's curve have their denominators held above zero.

**How the levels are weighted** is what `spread` sets, and it is where this
design leaves the usual one. The usual way (one "scatter" factor applied at
every step up) weights levels by their *number*, counted from the finest,
so the same setting gives a glow of the same size in pixels: half as wide,
as a share of the picture, in a picture twice as large. Here a 512x288
capture, a 1280x720 window and an editor view of any size show one scene,
and should show one look. So a level is weighted by its **reach**: the
size of one of its texels as a share of the picture's height.

```
weight(level) = reach^(2 * spread - 1) * min(1, 256 * reach)^2      reach = 2^level / height
```

then divided by their sum. With `spread` 0.5 every level counts the same
(the classic look: a kernel that falls off with the square of the
distance); below 0.5 fine levels count for more (at 0, each half as much
as the next finer: a tight glow), above it coarse ones (at 1, each twice
the next finer: a haze). The second factor fades out levels finer than
1/256 of the height, which only large pictures have: a 2160-pixel picture
has two levels a 540-pixel one lacks, and without the fade they would take
weight from the glow both can show. (Squared, so that the fade wins over
the first factor at every spread: at 0 the first factor doubles with each
finer level.)

What is left, said plainly:

- the grid of halvings: two picture sizes that are not a power of two
  apart have levels of slightly different reaches, and the widest level is
  between an eighth and a quarter of the height;
- small pictures and tight glows: below a spread of 0.5 the glow lives in
  the finest levels, and a picture less than about 500 pixels high does not
  have the ones a larger picture uses. Its glow is then wider than the
  larger picture's: at a spread of 0, two to three times as strong a
  tenth of the picture's height from the light (measured: section 6). From 0.5 up, and for larger
  pictures, the look is alike at every size; alike, not equal.

The reach of the coarsest level is what "across the picture" means for
`spread` 1: its glow is a bell about as wide as one of its texels (standard
deviation), an eighth to a quarter of the picture's height, so a light's
haze is seen a third of the height away and is all but gone beyond half of
it. A level more would be one veil over the whole picture.

**How the weights reach the GPU: no uniform.** Going up, level k becomes
`mix(level k, tent(level k+1), f)` with `f` = the share of the weight that
lies in the coarser levels. That is fixed-function blending with a
constant: the up-sample pipeline blends `source * constant + target *
(1 - constant)` and the pass sets the blend constant. So:

- the up-sample is done **in place**: no second set of level textures;
- **no bloom pass has a uniform of its own.** The down- and up-sample
  passes bind one texture and nothing else, through the layout the tone
  mapping pass already uses for "one picture read pixel by pixel". Sizes
  come from `textureDimensions`. The first down-sample and the last step
  read the three numbers of the setting from **one** small uniform buffer
  (`"bloom"`, 16 bytes), through two layouts of their own. So the rule of
  the foundation (never two groups of one layout over different uniform
  buffers) is met without a dynamic offset: there is one buffer.

Blending in a float target is arithmetic like the shader's, done by the
rasteriser; it is part of WebGPU core for `rgba16float`. If an adapter
turned out to blend differently enough to matter, the fallback is the
other sanctioned shape: a second set of level textures and one uniform
buffer with a dynamic offset per pass.

### 3.4 Bad input never spreads

A blur is the one place where one bad pixel becomes many.

- **Not a number.** The first down-sample cleans every texel it reads
  (`finite` in the shader): what is not a number counts as no light, a negative value
  as none, infinity as the brightest light there is. So one such pixel in
  the HDR picture is, at worst, one very bright pixel to the bloom, and the
  next rule holds it. The last step cleans the picture's own pixel the same
  way before it mixes. (The tone mapping pass already turned such a pixel
  black; with bloom on, an infinite pixel shows white instead, which is
  what it stands for.)
- **A ceiling.** Light brighter than 256 times white blooms as 256 times
  white (`BRIGHTEST`). Nothing a scene draws on purpose is near it (the
  sun's disc is held to 64), and a surface that overflowed the picture's
  16-bit floats cannot wash the whole picture out.
- **Fireflies.** One pixel many times brighter than its neighbours (a
  sub-pixel highlight, a broken normal) would otherwise bloom as a blob
  that flickers on and off as the camera moves. The first down-sample is an
  average weighted by `1 / (1 + luminance / 8)`: however bright one texel
  is, it moves the average of its neighbourhood like a texel eight times
  white at most. A bright *area* is not held: where all texels are bright
  their weights are equal.

  This is the "Karis average" of the same talk with one change. The
  original weight, `1 / (1 + luminance)`, holds a texel to *one* times
  white, which was tried first and measured: a light two pixels wide and a
  thousand times white then gave no glow at all (not one step of 255, ten
  pixels away), and a light nine pixels wide lost much of its glow at its
  own edge, where bright texels are averaged with dark ones. Small
  lights are what bloom is for (a lamp down a corridor, a crystal across a
  level), so the ceiling is 8: the nine-pixel light keeps 1.8 times the
  glow it had, and the two-pixel light of a thousand times white shows as
  one step of 255. The price that is left, by design: a light only a few
  pixels wide blooms less than its size and brightness say.
- **Settings.** Game code can write anything into a resource. `look.rs`
  cleans the three numbers (`strength` not a number: no bloom; `spread`:
  0.5; `threshold`: 0; out of range: the range's end) before anything is
  planned. The level weights and blend constants are worked out on the CPU
  from cleaned numbers and the picture's size alone.

### 3.5 Cost, and the pool

Per pixel of the picture: the first down-sample reads 36 texels for a
quarter of the pixels, the rest of the chain about as much again, the last
step 17: about 34 reads a pixel. Seven levels are 14 more full-screen
passes at 1280x720 (`stats.draw_calls` and `triangles` count them: two per
level). Measured: section 6. Reading whole texels instead of letting a
sampler filter is what this costs on a graphics card (four times the reads
of the same filters through a bilinear sampler), and it is the price of
one picture on every adapter.

Memory: the bloomed picture (8 bytes a pixel) and the levels (8 bytes a
pixel of each; together a third of the picture). At 3840x2160 that is 63.3
+ 21.1 MiB. The pool's budget was 320 MiB for a view of that size with
FXAA and a selection (270 MiB); with bloom such a view is 354 MiB, and the
budget is **384 MiB** (`pool.rs`, `BUDGET_BYTES`, with the sum in its
comment).

### 3.6 Seeing it from the control plane, and by hand

- `Bloom` is in `schema_get` with its documentation, ranges and defaults,
  is found by the words people use (`bloom`, `glow`, `glare`, `halo`,
  `neon`), and is set with `edit_apply` `set_resource`. A value outside its
  range, or a field it does not have, is refused with a hint. Its
  description says what glows and what does not (particles: a later
  release).
- `stats.effects` has **`bloom`**, in pass order: after `fog`, before
  `color_grading`. A debug view answers `[]`.
- No diagnostic is new.
- By hand: the inspector is generated from the schema, so `Bloom` is there
  with three number boxes and no editor code. A test adds it by hand and
  sets its three fields. No tool and no operation is new (no parity row),
  and type and field names are not interface text (no language file
  changes).
- The SDK re-exports `Bloom` (prelude included): game code can flash it.

## 4. What it adds to the world's types

One new resource, `Bloom`. Like every engine type it moves the world hash
of worlds that register the engine's types (foundation note, section 6). A
game module built against an older SDK keeps working unless it uses the
type; in the engine's own tree run `wen sdk` after this change. No recorded
hash of `wish_core`, `wish_ecs` or `wish_physics` is touched.

## 5. Tests

`crates/wish_render/tests/bloom.rs`, on the software adapter. Comparisons
between pictures drawn by different pipelines allow 1/255; only pictures
drawn by the same passes are asked for equal bytes.

| What | How |
|---|---|
| The defaults are the old picture | A scene without `Bloom`, one with it at its defaults, and one with `strength` 0 and the other two fields set: the same bytes, the same statistics, no `bloom` in `effects`. A renderer that drew with bloom draws the plain scene the same afterwards. |
| A light blooms around itself | A small panel 20 times white on a dark grey sky. Without bloom the pixels beside it are the sky's; with it they are lifted, more near the panel than far from it, the same on every side, in the panel's colour (an orange panel's glow has the proportions of its light), and the panel's own middle stays white-hot. The sky is grey and not black so that the tone curve's toe does not bend what is measured. |
| Nothing bright, hardly changed | A sunlit scene of rough surfaces with no light source in view, at a strength of 0.05: no pixel's light moves by more than a twentieth of white, the mean change is under a hundredth, and fewer than a tenth of the pixels move by more than 4 steps of 255. (The largest moves in steps of 255 are in deep shadow next to a bright ground: little light, many steps.) |
| The picture keeps its brightness | The same scene at a strength of 0.5: visibly softer, and its mean light is within 2% of the picture without bloom (measured: 0.03%). |
| `strength` | Twice the strength lifts the pixels beside the panel twice as much, to an eighth (measured as light). |
| `spread` | 47 pixels from the panel a spread of 1 gives more glow than 0.5 and 0.5 more than 0, each by more than half; and the lower the spread, the faster the glow falls off between 7 and 47 pixels. The default is 0.5 (the same bytes as spelling it out). |
| `threshold` | Three dim bars with dark gaps, and the bright panel beside them. Without a threshold the bars lose a share of their contrast; with a threshold of 2 the difference between a bar and a gap is what it was without bloom (to 5%) while the panel still glows. With the panel taken away the picture with bloom is the picture without (to 1/255). |
| The sun's disc blooms | The sky two and four radii from the disc's middle is brighter with bloom than without, more so nearer, in the sun's warm colour; the disc is still the brightest thing in the picture. |
| With everything | Sky light, sun disc, fog, grading, bloom and FXAA together, with a lamp in the scene: `effects` in pass order, the lamp's glow over the sky above it in its orange, the bloom's ten passes counted, one golden image. |
| What is drawn after the look | A sprite right beside a bright light and an interface panel over the sky next to it, at a strength of 0.5: every pixel of the panel, its text's edges included, and the sprite's middle are the same bytes with bloom as without, while the sky just under each is lit. |
| Sizes | 1x1, 2x2, 1x48, 48x1, 7x7, 3x5, 33x17 and 321x201: a picture comes back, no warning, two passes for each level the size has, and no pixel is brighter or darker than the range of the picture without bloom (the light is only moved). The chain's sizes, weights and blend constants have unit tests (`renderer/bloom.rs`). |
| An orthographic camera | The panel through an orthographic camera that frames it as the perspective one does: the same glow at 7, 27 and 47 pixels, to a tenth. |
| One look at every size | The same scene at 240x160 and at 480x320, spreads 0.5 and 0.8: the glow at the same place, as a share of the picture, is the same to a quarter. |
| Bad pixels | A small box whose emissive colour is "not a number" (shown to reach the HDR picture: without bloom its pixels are the pure black tone mapping makes of that). With bloom the picture is the picture of the same box black, to 1/255 outside the box, and the panel beside it glows as before. With the cleaning taken out of the shader this test fails (tried). An infinite box gives the picture of a box 600 times white (to 1/255), shows white itself, and the rest of the picture is still a picture. |
| Fireflies | Two lights that give off the same amount of light: one two pixels wide and a thousand times white, one six times as wide. The wide one glows; the glow of the small one is less than a tenth of it. |
| Numbers that are not numbers | `strength`, `spread` and `threshold` not a number or infinite: the picture of their fallbacks. Out of range: the picture of the range's end. |
| Reuse | A second frame with bloom creates nothing on the device; the first creates the bloomed picture, the levels and one buffer. |
| Golden images | `bloom_glow` (the panel scene) and `bloom_all` (everything on). |
| Tool answers | `wish_control/tests/view_tools.rs`: the schema of `Bloom`, the search words, a `set_resource` that writes exactly what was said, `effects` in `capture_frame`, `describe_view` and a session's picture, refusal out of range, and the first picture back to the byte when the resource is removed. |
| By hand | `wish_editor/tests/parity_ops.rs`: a person adds `Bloom` and types its three numbers; the scene file holds them, the picture uses `bloom`, the history has four changes by `human`. |

## 6. Result

Done on 19 November 2026, on the development machine (Windows 10, the
software adapter WARP on Dx12, and an NVIDIA GeForce RTX 3060 Ti on Vulkan).
The commands and their counts are in the package's report.

Verified:

- **No existing golden image changed** (`git status` shows no modified
  `.png`; two are new: `bloom_glow.png`, `bloom_all.png`) and every one
  passes without `UPDATE_GOLDEN`, in debug and in release.
- **The old pictures, byte for byte.** The 21 captures the existing render
  tests write (the look's nine, `basic`, `lights`, `many_lights`, `probe`,
  `sprites`, `ui`, the four debug views, `view_colliders`, `view_lights`)
  were kept from a run of the code before this package and compared with
  the ones the finished code writes: all 21 identical files.
- **The showcase scene, old binary against new, bloom off.**
  `examples/islands` at 1280x720 drawn by a build of the code before this
  package and by a build of the finished code: as it is, and with the
  first look package's five effects on; from the scene's camera and from
  one that looks at the sun. All eight pairs are the same files, on WARP
  and on the RTX 3060 Ti.
- **Software against hardware, bloom alone.** `examples/islands` with its
  crystals made bright and a sun disc, 1280x720, from the scene's camera
  and from one that looks at the sun, on WARP and on the RTX 3060 Ti: with
  bloom **no pixel of 921 600 differs by more than 1/255**, the same as
  without it. So the chain and its fixed-function blending give the same
  picture on these two adapters.
- **Software against hardware, everything on** (the pictures of section
  7): 22 and 26 pixels of 921 600 differ by more than 2/255 with bloom
  (0.003%), 13 and 12 without it, all at edges FXAA judged differently (the
  largest difference 37); the golden tolerance is 0.5%.
- **One look at every size** (the panel scene, strength 0.1; the glow as
  light, at the same place of the picture):

  | Spread | Place (pixels of the small picture from the middle) | 240x160 | 480x320 | 960x640 |
  |---|---|---|---|---|
  | 0.5 | 16 | 0.62 | 0.54 | 0.56 |
  | 0.5 | 30 | 0.137 | 0.116 | 0.116 |
  | 0.5 | 45 | 0.049 | 0.041 | 0.041 |
  | 0.8 | 30 | 0.197 | 0.188 | 0.206 |
  | 1 | 45 | 0.103 | 0.103 | 0.110 |
  | 0 | 20 | 0.182 | 0.095 | 0.067 |

  The last row is the limit section 3.3 names: a tight glow in pictures
  this small.
- **The picture keeps its brightness**: on the sunlit test scene the mean
  light with a strength of 0.5 is 0.03% below the picture without bloom.
  This is a scene with nothing far brighter than white in it; section 6.1
  says where it stops holding.
- **What it costs** (the look's golden scene, a frame drawn and read back,
  release build, the process held to four cores at low priority; the test
  `what_bloom_costs`, run by hand):

  | Adapter | Size | Without bloom | With bloom |
  |---|---|---|---|
  | RTX 3060 Ti, Vulkan | 1280x720 | 1.6 ms | 2.6 ms |
  | RTX 3060 Ti, Vulkan | 1920x1080 | 3.3 ms | 5.0 ms |
  | WARP (software), Dx12 | 1280x720 | 112 ms | 474 ms |
  | WARP (software), Dx12 | 1920x1080 | 243 ms | 1043 ms |

  On the graphics card bloom is about a millisecond at 1280x720. On the
  software adapter it is the most expensive part of the frame: a default
  512x288 capture with bloom takes about 60 ms longer, a 1280x720 one a
  third of a second.
- The cleaning matters: with it taken out of the shader, the test of a
  pixel that is not a number fails (on WARP by a glow over the whole
  picture: its `min` drops the bad value and keeps the ceiling).

Not verified:

- **Linux and lavapipe.** Nothing here ran on them. The two new golden
  images were made on WARP and are compared on lavapipe in CI at 2/255 for
  0.5% of the pixels; `bloom_all` has FXAA in it. The tests that ask for
  equal bytes compare pictures drawn by the same passes with the same
  numbers; the ones across pipelines allow 1/255, and one of them (a
  threshold nothing reaches leaves the picture as it is) rests on a float
  written to a half-float target and read back being the same number,
  which is not known for lavapipe.
- **Fixed-function blending on other adapters** than the two above
  (Dx12 on hardware, Metal, GL): the up-sample relies on it. It is WebGPU
  core for `rgba16float`; the fallback is in section 3.3.
- **Flicker.** Fireflies are a thing of moving pictures. What is tested is
  the still: one very bright pixel gives next to no glow. No sequence of
  frames was looked at.
- **The window** (`wen view`, `wen play`, the editor's views) was not
  opened; they draw through the same `Renderer::render`. The editor's
  inspector was driven by its test, not looked at.
- **The cost on other machines**, and what the pool holds at 3840x2160
  (the 354 MiB of section 3.5 is arithmetic, not a measurement).

Handed on:

- to the look lock: whether the default look has bloom, and at what
  `strength` and `spread` (the pictures of section 7 use 0.12 and 0.35; a
  default `spread` a little under 0.5 lifts shadows less in a bright
  scene), with the migration that writes the old values into existing
  files; presets;
- to release 1.9: particles that glow (they move before tone mapping), and
  whether sprites follow;
- a per-camera override of the look settings, as before;
- if the cost on the software adapter becomes a burden for tests or
  captures: the last step could read four texels instead of sixteen (a
  plain bilinear weight), at the price of a slightly blockier fine glow;
- the user guides do not mention the look settings yet (the "public face"
  package).

### 6.1 Review

One independent reader went through the change before it was committed
(read only: the code, the shader arithmetic by hand, the tests, this note).
No high finding; one medium and seven low. What was done with each:

1. **Medium: a test with no margin on another rasteriser.** "Twice the
   strength, twice the glow" was measured at single pixels where the weaker
   picture's glow is two or three steps of 255, so one step of rounding on
   another adapter would move the ratio out of its limit with bloom
   correct. Fixed: the ratio is now of the mean lift over 9x9 pixels
   (`lift_around`).
2. **Low: "the picture keeps its brightness" is not true of small, very
   bright lights.** The last step takes the full scattered share from each
   pixel, but the first step down spreads a firefly-weighted average
   (section 3.4), so a light a few pixels wide and many times white gives
   up more than its glow gives back: by the reader's arithmetic a 4x4-pixel
   lamp of 20 times white puts about half of its share into its level, a
   2x2 one of 50 times white about a sixth. At ordinary strengths the tone
   curve hides it (the lamp stays white-hot); at a strength near 1 such a
   lamp is visibly dimmer. Kept as it is, because the alternative is the
   flicker the weighting is there to stop; the claim is corrected in the
   type's description, in section 3.1 and in the shader's comment. The
   measured 0.03% is of a scene with nothing far brighter than white.
3. **Low: the `threshold` description promised too much.** The soft start
   begins at half the threshold, so at a threshold of 1 a surface of 0.9
   white already gives up a little. The description now says so: light
   dimmer than half the threshold is untouched, and 2 keeps everything up
   to white sharp.
4. **Low, not settled: cleaning what is not a number on Vulkan.** `finite`
   tests `light == light`, which a SPIR-V driver may fold away, and a
   `clamp` of such a value is not defined there. The tone mapping pass has
   used the same idiom since M3, but nothing tested it with such a value
   until this package: the bloom's test of it (which also checks what tone
   mapping alone makes of the value) is the first, and CI runs it on
   lavapipe. WGSL has no other way to test for such a value, so nothing
   was changed; if that test fails on an adapter, this is where to look.
   (After the commit: CI ran the test on lavapipe on Linux and on WARP on
   Windows, and it passed on both, as did the two new golden images.)
5. **Low: the test of one look at every size had little margin.** It now
   measures the mean lift over the same share of the picture at both sizes
   and allows 30% (a tight glow, the case the test leaves out, differs by
   two to three times).
6. **Low: the last step could write past the largest 16-bit float** (an
   infinite pixel beside others, a high threshold and a strength of 1),
   which tone mapping would draw black. Fixed: the sum is held to the
   largest number.
7. **Low: two of the test's "same on every side" pixels were one pixel
   nearer the panel than the one they are compared with.** Fixed.
8. **Low: bookkeeping** (the status documents and the new files), done
   with the commit.

Found clean by reading: the filter weights and their sums, block placement
at odd sizes and at 1x1, every read held inside its texture, no division by
zero, the blend state and the order of the passes, `mix_shares` and
`level_weights`, the foundation's GPU rules, that nothing changes with a
strength of 0, the cleaning of the settings, and the control plane's side.

## 7. To look at

The showcase game's scene, `examples/islands`, at 1280x720 on the software
adapter, drawn with the engine's own tools (`wen build`, then one `wen do`
of `edit_apply` and `capture_frame` calls) on a copy of the project; the
example in the repository is not changed. The pictures are in
`C:/wish-build/look-2/` on the development machine, not in the repository,
with the settings in `settings.json` beside them:

| File | What |
|---|---|
| `before.png` | The scene from its own camera with the first look package's settings (that note's section 7), without bloom. |
| `after.png` | The same with bloom. |
| `sun-before.png`, `sun-after.png` | The same two from the camera of that note that looks back up at the sun. |
| `glow-before.png`, `glow-after.png` | The same two from the scene's own camera, with the crystals made bright. |

**From its own camera the showcase has nothing that glows**: the sun is
behind the camera, and the crystals' `emissive` is `[0.1, 0.4, 0.5]`, dimmer
than a white surface. `after.png` therefore shows what bloom does to a
scene without lights in view: almost nothing, except that the one deep
shadow in the picture is a little lighter (the bright sky and ground
around it give it some of their light, as a lens would). The third pair
sets every crystal's `emissive` to `[2, 8, 10]` in the copy: without bloom
they are white dots, with it each has a cyan glow, which is how the picture
says what colour a light brighter than white is.

The bloom of the "after" pictures, to taste:

```json
"Bloom": { "strength": 0.12, "spread": 0.35 }
```
