# M12: material files and textures on primitives

Package 7 of the short path ([M12-road-to-1-0.md](M12-road-to-1-0.md),
section 0, item 7). Backlog: the graphics area of
[M12-area-maps.md](M12-area-maps.md), gap 5. It builds on the draw list of
[M12-render-foundation.md](M12-render-foundation.md). Decisions it rests on:
ADR-007 (file formats: `*.mat.json` is named there), ADR-013 as amended on
18 November 2026 (material files are in 1.0; custom shaders from projects are
after 2.0), ADR-027 (defaults are file format; tool answers carry no time).

Recommended model and effort: Fable 5.2, high (ROADMAP, M12).

Status: built and tested on 19 November 2026 (section 7). Not committed by
the agent that built it (the package's instruction). Sections 1 to 6 were
written with the code and then brought in line with what building it
changed (3.1 says what was built first and why it was taken back); section 7
says what was measured and what was not.

## 1. Problem

People and agents build most scenes from the built-in shapes, and a shape
could only be one flat colour: `Material` had `base_color`, `metallic`,
`roughness` and `emissive`, and nothing else. Textures existed only inside
glTF models. A `Material` put on a `Model` entity did nothing, and nothing
said so. That caps how good anything made inside the engine can look,
whatever the lighting does.

Wanted: a material that carries textures and the options the renderer
already draws for glTF materials, usable on a `Primitive`, as a file that
several entities share, and a defined, documented meaning for a `Material` on
a `Model`.

## 2. What the code said (map claims, checked)

| Claim | Checked |
|---|---|
| A primitive can only be a flat colour | True. `collect` drew every primitive with the default 1x1 textures (`draw_list.rs`). |
| A `Material` on a `Model` is silently ignored | True. `extract` copied it into the draw; `collect` never read it for a model, and no check rule mentioned it. No scene in the repository has both on one entity. |
| The renderer already draws textured PBR | True, for models: five slots (base colour, metallic-roughness, normal, occlusion, emissive), alpha mask with masked shadows, double-sided, one trilinear repeating sampler, mip chains made on the CPU (`texture.rs`). All of it is per draw, so a primitive can use it unchanged. |
| Primitives have texture coordinates | They have some, but not usable ones: the cube's sides were upside down for a picture (v grew upwards), the plane's were transposed, and no shape had tangents, so a normal map could do nothing. |
| An image decoder exists | `wish_import::load_png` (guarded by `catch_unwind`, at most 4096 pixels a side). PNG only: the JPEG decoder inside the glTF importer is not reachable for a file on its own, and this package adds no dependency. |
| The document machinery could take a third document kind | Not inside this package. `documents.rs` is built around `SceneDocument` (entities, components, prefabs, transactions, undo by inverse operations, external changes turned into operations). See 3.4. |
| A replay's `assets` fingerprint covers what a session reads | It covers what the *simulation* reads from files: clip lengths, animation lengths, fonts. Model geometry, sprites' pictures and (now) materials are read by the renderer only and are not in it. See 3.8. |
| An export takes every asset | True: `project_export` copies the documents and the asset index. Material files are in both lists. |

## 3. Decisions

### 3.1 The component names a file; the file holds the look

Options:

1. `Material` keeps its four numbers and gains `file`; the file holds a
   second, larger schema type (numbers, textures, options).
2. A new component names the file; `Material` stays as it is.
3. `Material` itself gains the textures and options, and `file`; a material
   file holds one `Material`.

Decision: **1**.

Option 3 was built first, because it is one type instead of two and makes
"a texture on a box" a single `edit_apply`. Building it showed what it
costs: `Material` became 17 fields. In the inspector that is a box of 17
rows on every shape, which pushes `Transform` and the component's own
buttons below the fold at ordinary window sizes (13 of the editor's tests
failed for that reason alone, 7 of the inspector's 8), and 96 bytes on every
entity with a material instead of 40. It also was more than the package
asked for: a second way to have textures, and 16 fields that mean nothing
whenever `file` is set. It was taken back.

With option 1:

- `Material` gains one field. The inspector's box is 5 rows; every old
  scene, test and habit about `Material` stays as it was.
- The rule is short: four numbers for a plain look, or a file for
  everything. A material is an asset, as in every engine people come from,
  and as the M3 note already had it ("`MeshRenderer { mesh: AssetRef,
  material: AssetRef }`... the `.mat.json` file comes later").
- A material file can never name another one, because its type has no such
  field: reference cycles cannot be written.
- The cost: a texture needs two calls (make the file, name it), and the
  file's type is a schema type that is in no world, which `schema_get`, the
  type pages and the search had to learn (kind `file`, 3.9).

Option 2 would leave two components to choose from on one entity; a field
of the component people already look at is found sooner.

### 3.2 The types

`Material` (the component) gains, with a default that is left out of files,
so every old scene file loads and saves byte for byte:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `file` | AssetRef | none | a material file to take the whole look from |

`MaterialFile` (`wish_types::render`, a `#[schema]` struct: what a material
file holds):

| Field | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `base_color` | Color | white | | colour of the surface |
| `metallic` | f32 | 0 | 0 to 1 | |
| `roughness` | f32 | 0.5 | 0 to 1 | |
| `emissive` | Vec3 | 0 | | light given off |
| `base_color_texture` | AssetRef | none | | PNG, sRGB; multiplied by `base_color`; its alpha is the surface's alpha |
| `metallic_roughness_texture` | AssetRef | none | | PNG, numbers; blue times `metallic`, green times `roughness` (glTF) |
| `normal_texture` | AssetRef | none | | PNG, tangent space, green up (glTF) |
| `normal_scale` | f32 | 1 | 0 to 8 | strength of the bumps |
| `occlusion_texture` | AssetRef | none | | PNG, red; darkens ambient light only |
| `occlusion_strength` | f32 | 1 | 0 to 1 | how much it darkens |
| `emissive_texture` | AssetRef | none | | PNG, sRGB; multiplied by `emissive` (0 by default, as in glTF) |
| `uv_scale` | [f32; 2] | [1, 1] | | repeats across the surface |
| `uv_offset` | [f32; 2] | [0, 0] | | slides the textures, after the scale |
| `alpha_mode` | `AlphaMode` | `Opaque` | `Opaque`, `Mask` | whether alpha cuts holes |
| `alpha_cutoff` | f32 | 0.5 | 0 to 1 | alpha below it is a hole (`Mask`) |
| `double_sided` | bool | false | | whether back faces are drawn |

Its defaults are the look of a shape without a `Material`, so an empty
material file changes nothing. The four numbers have the defaults and ranges
they have on the component.

`AlphaMode` is a new schema enum with two variants. `Blend` belongs to the
next package and is an addition: a third variant with the next value, and no
existing value changes.

**Unlit is left out.** A lit frame is drawn in HDR and tone mapped; the tone
mapper (Khronos PBR Neutral) and the look (fog, bloom, grading) change what a
surface's colour becomes on screen, so "this colour exactly, unlit" cannot be
had by a flag in the opaque pass. It would be a second fragment path and a
definition of what fog and bloom do to it. Not cheap, not exact: not in this
package. `emissive` gives a surface light of its own; sprites are the unlit
path.

**JPEG is left out.** Textures outside models are PNG only (section 2).
A texture field that names another kind of file is a `texture_not_png`
warning in `check` and a `texture_not_loaded` warning in the picture.

### 3.3 The file

```json
{
  "format": 1,
  "material": {
    "base_color_texture": "assets/textures/brick.png",
    "roughness": 0.9,
    "uv_scale": [4, 4]
  }
}
```

- `material` is a patch on the defaults of `MaterialFile`, exactly as a
  component is written in a scene. It is left out when no field is written:
  the canonical file of the default look is `{ "format": 1 }`.
- Canonical form is the scene files' (`wish_scene::json::to_canonical`):
  sorted keys, the same layout, numbers in the notation they were written
  in. Load then save is byte-identical.
- No shader field (ADR-013 as amended): a material file is data only.
- A material file is **both an engine document and an asset**. A document:
  strict JSON with a format number, checked by `check`, rewritten by
  `wen fmt`, known to `wen migrate` and to `wen watch` (which reloads on any
  project file and refuses a project with errors). An asset: it is in the
  asset index, because scenes refer to it through an `AssetRef`, and
  `asset_list` lists it with kind `material`. `wish_asset::is_document` is
  true for it and the index includes it.
- Code: `wish_asset::material` (`parse_material`, `check_material`,
  `MaterialDocument` with `check`, `decode`, `effective`, `textures`,
  `to_canonical`). The schema is `MaterialFile`'s own (`wish_asset` now
  depends on `wish_types`, which is below it), so what a file may say and
  what the renderer is given cannot drift apart.

### 3.4 Making and changing a material file

Options for the control plane:

1. A material document kind beside scenes and prefabs: opened, edited by
   operations, with its own undo history, external changes merged in.
2. A new tool that patches a material file.
3. `asset_create` makes a material file from `material: {...}`; with
   `overwrite: true` it makes it again.

Decision: **3**, the smallest honest one. `asset_create` already makes
files from a few values, has `dry_run`, refuses to replace without
`overwrite`, keeps to the output path rules, and is recorded in the activity
feed with its actor. Its answer carries the look as written, so nothing has
to be read back. The file is written atomically, in canonical form, only
after every field is checked against the schema. A texture the look names
that is not in the project yet is a warning in the answer; the file is
still made (`check` reports it as the error it is).

In the editor: *New... > Material file* in the browser makes one (the same
call, as `human`), and a selected material file shows its fields where the
browser shows the file, with the inspector's own widgets over the
`MaterialFile` schema. A change is one `asset_create` over the file with
every field it writes, by `human`. An entity takes the look in the
inspector: the `file` field of its `Material` (typed, picked from the list,
or the file dragged onto it).

**What is left (not in this package):** a material file has no undo history
of its own. A change to it is in the activity feed with who made it, and a
checkpoint or version control brings an old one back, but `edit_history`
does not list it and Ctrl+Z does not take it back. (Naming a file on an
entity is a scene edit and has all of that.) A material document kind with
operations and undo is the full version; it belongs with the material
editor of release 1.8 (`road-to-2-0.md`).

Invariant 9 asks that manual and agent changes enter the same history with
who made them. Checked: `asset_create` changes things and is not a "look"
action, so `activity::worth_noting` records every call of it, by a person
or an agent, in the activity feed with its actor; both tests prove it
(`material_tools.rs`: `agent:painter`; `parity_ops.rs`: three entries, each
`human`). What is missing is undo, not the record. Decision: undo of a
material file is left for release 1.8.

### 3.5 What a `Material` means, entity by entity

- **On a `Primitive`:** the shape's look. With `file`, the file is the whole
  look and the numbers of the entity's own `Material` are not used (`check`
  warns when they are written too: `material_fields_unused`). If the file
  cannot be used, the entity is drawn with its own numbers and the picture
  carries a warning (`material_not_loaded`).
- **On a `Model`:** only `file` counts. A material file replaces **every**
  material of the model: each part keeps its mesh and its texture
  coordinates and takes the file's numbers, textures and options. A
  `Material` without `file` does nothing on a model, as before, and `check`
  now says so (`material_unused_on_model`).

  Why not let the four numbers replace the model's materials too? Because
  every old scene must draw as it did, and because an agent that adds a
  default `Material` to a textured model out of habit would wipe its
  textures. Naming a file is a deliberate act. Why the whole model and not
  one material of it? A per-part override needs a way to name parts (the
  `#sub` half of an asset reference, not used by the renderer yet); the
  whole model is what can be defined, documented and tested now, and it
  covers the common cases (one model in several skins, a flat ghost or
  placeholder look).

  A model part without texture coordinates shows a textured file as one
  flat colour (`material_needs_uvs`); a normal map needs tangents in the
  model file.

### 3.6 In the renderer

- **Extraction** is what it was, plus one field: `Draw::material_file`, the
  path the entity's `Material.file` names. The numbers of a `Material` are
  copied as they always were.
- **Material files and textures are given to the renderer**, never read by
  it: `Renderer::set_material(path, look)` and
  `Renderer::upload_texture(path, colour, pixels)`, as `upload_model`
  works. A `Look` is a `MaterialFile` with its paths resolved and every
  number cleaned (`Look::of`). A texture is uploaded once for each way it is
  read (sRGB for the base colour and emission slots, plain numbers for the
  others), with the mip chain made on the CPU, through the counted
  `Gpu::create_texture` (`texture::upload`, unchanged).
- **Sampling** is what the model path does: the one material sampler
  (repeat, linear, trilinear, no anisotropy). No new filtering. What was
  found: the lit pass samples materials through the sampler (derivatives
  pick the mip level), the masked shadow pass reads level 0; both are as
  they were, and the tests of this package are written so that they do not
  rest on how an adapter filters at a texel edge (section 6).
- **Bind groups.** One for each distinct set of five textures, made the
  first time a frame draws it (`Renderer::texture_set`, inside
  `Gpu::checked`) and kept until a texture changes. The rule "build bind
  groups in the frame and keep none" is about groups over uniform buffers
  (the software adapter's fault, `M12-render-foundation.md` 3.2); these bind
  textures and the sampler only, as the kept groups of models always have.
- **The shapes.** `mesh::primitive_textured` gives each shape texture
  coordinates in glTF's convention (the picture's top left is [0, 0]) and
  tangents: every face of a cube shows the picture upright, a plane shows it
  as an image viewer does to a camera looking along -Z from above, a sphere
  wraps it once with its top at the north pole. These are a second set of
  vertex buffers over the same triangles. **A surface without textures is
  drawn from the old mesh with the old textures**, and an entity without a
  file with the numbers it always had, so it is the draw it always was by
  construction, not by measurement. A textured surface without a normal map
  is drawn with a normal scale of 0, so the default normal texture (which
  is one step of 255 away from "straight out") does not tilt it.
- **UV scale and offset** are a fifth vector of the draw uniform
  (`uv_transform`, 192 to 208 bytes, inside the 256 byte stride), applied in
  the vertex stage of the opaque, masked shadow and debug passes:
  `uv * scale + offset`. For every old draw it is `uv * 1 + 0`, which is
  exact in IEEE arithmetic; that this changes no pixel on either adapter is
  measured in section 7, not assumed.
- **A missing texture never takes the picture down.** The slot binds its
  default and the frame carries `texture_not_loaded` with the entity, the
  file and the slot.

### 3.7 Loading and hot reload

`wish_control::render::load_materials` is called wherever models are loaded:
by `Drawing::load_models` (every picture the tools and the editor draw), by
`wish_player::load_models` (`wen capture`, `wen view`) and by the player's
`refresh` (every picture of `wen play`). It reads the material files the
scene's entities name and the textures those files name, and remembers each
file's length and modification time, so only what changed is read again. A
saved material file or texture is therefore in the next picture that is
drawn: in the tools at the next call, in `wen play` at the next frame
(better than models there, which are loaded once), in `wen view` when its
fingerprint of the project's `.json` and `.png` files changes. The editor's
views draw through the same `Drawing`; when the editor redraws after a file
changed outside it is what it was before this package.

Hostile input: a path that is not inside the project is never read; a
material file over 256 KiB is not read; a picture over the size a 4096x4096
PNG can have is not read; decoding is `wish_import::load_png` (inside
`catch_unwind`, at most 4096 pixels a side); all material textures of a
renderer together are held to 2^28 pixels (the budget of one model's
textures). Every failure is a warning with the file (and for a texture its
slots), and the last good copy is dropped, so what is drawn is what the
files say.

### 3.8 Replays and exports

- **Export:** material files and their textures are in every export,
  because exports copy the documents and the asset index. Tested.
- **Replay fingerprint: not included, deliberately.** The fingerprint is
  what the simulation reads from project files, so that a replay can say "a
  divergence may be just that" (`replay_assets_changed`). Materials and
  textures are read by the renderer only: they cannot change a world's hash,
  and a warning that a replay may diverge because a wall was repainted would
  be false. Model geometry and sprites' pictures are not in the fingerprint
  for the same reason. A test changes a material file and a texture between
  recording and playing a replay and requires that it passes without the
  warning. (The brief for this package asked that the fingerprint include
  them; this is where the code led somewhere else. What a picture looks like
  is guarded by golden images in scenario tests, which do compare.)

  Checked again when the package was finished: the fingerprint was added by
  the M6 closing review (finding 12, `M6-game-systems.md` WP9) because
  "recordings did not know that the files the game reads (clip lengths,
  fonts) had changed"; it is computed when a session starts
  (`sessions.rs`, `build_app`) from exactly those files, and its only use
  is the `replay_assets_changed` warning, whose hint reads "a divergence
  below may just be that". Game code sees a `Material.file` only as a
  path, never the file's contents, and no engine system reads a material
  file or a texture. Adding them would make the warning fire for changes
  that provably cannot cause a divergence, which makes it worth less for
  the changes that can. Decision: kept out.

### 3.9 Seeing it from the control plane

- **The file's type** is documented like any type: `schema_get` lists
  `MaterialFile` with kind `file` (fields, units, ranges, defaults; found
  by the words people use: texture, tiling, cutout, bump...), it has a type
  page (`wish://docs/components/MaterialFile`) and `docs_search` finds it.
  `wish_control::docs::file_types` is the list of such types (one so far).
- `asset_list` kind `material`; `asset_info` on a material file: `material`
  (every field with its value), `written` (what the file writes),
  `textures` (slot, path, whether it exists), `used_by`; on a picture,
  `used_by` now names material files too.
- `asset_move` rewrites references to a material file in scenes and prefabs
  and the texture references inside material files; a material file keeps
  its ending when it moves.
- The guide, the recipe "A texture on a box" (run by `tests/recipes.rs`),
  the concept hints (texture, skin, tile, uv, normal map...) and the MCP
  instructions say how.

New diagnostic codes, all in `docs::DIAGNOSTICS`:

| Code | Where | Meaning |
|---|---|---|
| `material_file_too_large` | check, loading | over 256 KiB |
| `not_a_material_file` | check (error) | `Material.file` names a file that is not `*.mat.json` |
| `texture_not_png` | check (warning), `asset_create` (warning) | a texture field names a file that is not a PNG |
| `material_fields_unused` | check (warning) | `file` and numbers on one entity |
| `material_unused_on_model` | check (warning) | a `Material` without `file` on a `Model` |
| `material_not_loaded` | picture (warning) | the file could not be used; says why |
| `texture_not_loaded` | picture (warning) | the texture could not be used; names the file and the slots |
| `material_needs_uvs` | picture (warning) | a textured file on a model part without UVs |
| `material_uv_unusable` | check (warning) | a `uv_scale` of 0, or tiling beyond 4096 (7.1) |
| `warnings_cut_short` | picture (warning) | more than 32 material warnings in one picture (7.1) |

Existing codes cover the rest: `json_syntax`, `json_number_out_of_range`,
`type_mismatch`, `unknown_key`, `unknown_field` (a `file` inside a material
file is one), `unknown_variant`, `value_out_of_range`, `wrong_length`,
`invalid_asset_ref`, `missing_format`, `unsupported_format`,
`asset_not_found` (a missing material file in a scene, a missing texture in
a material file: with the file, line and column), `not_canonical`,
`image_invalid`, `texture_too_large_for_gpu`.

The two "unused" warnings look at entities as they are written; what a
prefab instance inherits or overrides is not followed.

## 4. What it changes in the world's types

`Material` has a new field (36 to 40 bytes), so the registry fingerprint and
the hash of every world that registers the engine's types change, as with
every package that added a type
([M12-render-foundation.md](M12-render-foundation.md), section 6). No
recorded hash in the repository covers these types. Game code compiled
against an older SDK must be rebuilt (`wen sdk`, then `script_build`): the
layout of `Material` changed. No game code in the repository builds a
`Material` by naming every field. `MaterialFile` and `AlphaMode` are in no
world and are not in the SDK's prelude.

## 5. Not in this package

- Blended alpha (package 8), custom shaders (after 2.0), environment maps,
  a material editor with a preview (release 1.8).
- Unlit, JPEG (3.2).
- Textures written on the entity itself (3.1).
- Undo for a material file (3.4).
- Overriding one material of a model (3.5).
- Texture settings in meta files (filtering, sRGB, compression): the slot
  decides how a picture is read, and there is one sampler.
- Evaluation tasks (the AI scorecard is package 10).

## 6. Tests

Structural first: a test says which colour must be where, or which of two
pictures is the brighter at a point, by a wide margin, with samples in the
middle of flat areas, so that it holds on every adapter. No new golden
image.

- `wish_render` unit: the textured shapes keep their triangles and carry a
  right-handed picture frame; the untextured ones are untouched.
- `wish_render/tests/materials.rs` (software adapter): a picture reads the
  right way round on the plane and on three faces of the cube; it wraps a
  sphere left to right from the pole; scale repeats and offset slides; a
  normal map brightens the side that faces the light, for red and for green,
  and `normal_scale` 0 switches it off; a mask cuts the shape and its
  shadow (compared with the solid plane at the shadow's place); a material
  file gives the look, a missing one falls back with a warning, a changed
  one is the new look, and an entity without a file is untouched by the
  files the renderer holds; a missing texture is a warning for each slot
  and the slot's default (the picture equals the untextured one within
  1/255); a changed texture is drawn at the next frame; double-sided; the
  identity view cuts what the mask cuts; broken texture data is refused.
- `wish_render/tests/models.rs`: a `Material` without a file leaves a model
  byte for byte as it was; a material file replaces its materials (flat, and
  with a texture on the model's own coordinates); a model without texture
  coordinates is drawn in one colour with `material_needs_uvs`.
- `wish_render/tests/extract.rs`: the file is extracted beside the numbers;
  a look built from numbers that are not numbers is cleaned.
- `wish_asset/tests/materials.rs`: round trip and canonical form; decoding
  with defaults; 23 hostile files (each a diagnostic with a file, a line and
  a hint); the file as a document and an asset; the project check (missing
  texture, not a PNG, a missing or wrong material file in a scene, a
  broken, an uncanonical and an oversized file); the two "unused" warnings;
  moving a texture, a material file and a folder; the ending that a move
  must keep.
- `wish_control/tests/material_tools.rs`: `asset_create` (dry run, exact
  text, overwrite, 11 refusals with their place, a missing texture as a
  warning), `asset_list`, `asset_info`, `check`, the activity feed; the
  type in `schema_get`, its page and the search; a textured cube drawn by
  `capture_frame`, following a changed texture, a changed material file (by
  the tool and by hand), a broken one and its repair, with the warnings;
  `describe_view` and a session; `asset_move`; an export; a replay across a
  changed look.
- `wish_control/tests/recipes.rs`: the new recipe "A texture on a box" is
  run: its calls, its operations, a clean `check`, a picture without
  warnings whose ground shows more than one colour.
- `wish_cli/tests/project.rs`: `wen fmt` (with `--check`, with a file
  named), `wen check`, `wen migrate`, `wen capture` and `wen watch` on a
  project with a material file, good and broken.
- `wish_editor/tests/parity_ops.rs`: a material file is made from the
  browser's menu, two of its fields are changed by hand (each one call by
  `human`), no `file` field is offered among them, and an entity names the
  file in the inspector.

## 7. Result

Measured on 19 November 2026 on the code as it is now (the design of 3.1),
on this machine: the hardware adapter is an NVIDIA RTX 3060 Ti on Vulkan,
the software adapter WARP on Dx12.

**Off is exact.**

- *The examples, both adapters.* Before any code changed, `wen` was built
  from `68f4d85` and drew the main scene of each of the six example
  projects (bricks, collect, first-pixel, hop, islands, quiz) at 480x270 in
  the lit, identity, depth and normals views on both adapters: 48
  pictures. The `wen` of this package drew the same 48: **all 48
  byte-identical**. That the hardware adapter repeats itself was checked
  first, with the old binary: its 24 pictures drawn a second time were
  byte-identical to the first.
- *Textured glTF models, both adapters.* The Khronos gallery
  (`tests/khronos.rs`: Box, Duck, DamagedHelmet, MetalRoughSpheres,
  NormalTangentTest, from `C:\wish-build\khronos-samples`) drawn by the test
  binary of `68f4d85` and of this package: **all 10 pictures
  byte-identical**.
- *Golden images.* `cargo test --release -p wish_render` with
  `UPDATE_GOLDEN=1`: every test passed and `git status` shows no modified
  `.png`. The editor's golden images (`inspector.png` among them) compared
  equal without a refresh: the `Material` box gained a row, but the golden
  inspector does not show a `Material`.
- *Files.* `wen fmt --check` on every example project: 0 files need
  formatting (old scene files load and save byte for byte); the
  `wish_asset` round-trip tests do the same for material files.

**Tests** (each with `CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=3`, below
normal priority on four cores):

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean (one finding fixed: `contains` in `schema_get`) |
| `cargo test -p wish_types -p wish_asset` | 34 passed (`MaterialFile` and `AlphaMode` added to `tests/wire.rs`) |
| `cargo test -p wish_schema --test compile_fail` | passed, no `.stderr` changed |
| `cargo test --release -p wish_render` (`UPDATE_GOLDEN=1`) | 174 passed, 5 ignored (measurements, the gallery); `materials` 11 of 11 |
| `cargo test -p wish_render -p wish_sdk` (debug) | 174 + 4 passed, 5 ignored |
| `cargo test -p wish_control` | 186 passed (`material_tools` 8, `recipes` 2); after the guide was shortened, `recipes`, `material_tools` and `diagnostics_catalog` again: 12 passed |
| `cargo test -p wish_cli -p wish_player -p wish_mcp` | 67 passed, 1 ignored (the package build) |
| `cargo test -p wish_editor` | 127 passed, 2 ignored (`parity_ops` 17, `inspector` 8, `languages` 4); no golden image changed |
| `wish-evals selftest` | 41 of 41 |

Three failures were found and fixed on the way:

- `wish_mcp` `resources_and_prompts`: the guide (`wish://docs/guide`) grew
  to 12,380 bytes, over its limit of 12,000. The paragraph on surfaces
  became two lines; the recipe carries the rest.
- `wish_editor` `a_material_file_is_made_and_changed_by_hand` (the failure
  the first design had too): the browser's detail column had been put into
  a scroll area, which lays its contents out as its parent does, and the
  parent lays them side by side. The fields of a material file went off
  the right edge of the window, where no click reaches. The detail is a
  column again (`ui.vertical` inside the scroll area); the test scrolls to
  `double_sided`.
- `wish_editor` `notes_are_written_and_cleared` typed its note into the new
  `Material.file` box: its helper took the first text box of the inspector.
  It takes the notes box's own role now (`MultilineTextInput`).

**The package works** (by test, section 6): textures in all five slots,
UV scale and offset, `Opaque` and `Mask` with the cutoff, double-sided, on
the cube, the plane and the sphere and on a model; a mask cuts its shadow
and the identity view; a missing or broken texture or material file draws
the fallback with a warning naming the file (and the slot); hostile files
are diagnostics with a code, a place and a hint; `asset_move` rewrites
references to a material file and the textures inside it; a saved file is
in the next picture; an export takes material files and their textures.

**Pictures to look at** (in `C:/wish-build/look-4/`, not in the
repository; drawn on the RTX 3060 Ti at 960x540):

- `before.png`: `examples/first-pixel` as it is: flat colours.
- `after.png`: the same scene after the calls in `calls.sh` (output in
  `calls-output.txt`): the cube is bricks with a normal map (the mortar
  lines are lit from the sun's side), the ground is a tile repeated 12 by
  12, and a new cube of a masked grate (round holes, double-sided) stands
  at the right with the holes in its shadow. The orb and the lamp keep
  their plain `Material`. The bricks, their normal map and the grate were
  generated by `generate_textures.py` (Python, nothing downloaded); the
  tile is a placeholder made by `asset_create`. `check` was clean and the
  picture had no warning.

**Not verified:**

- By eye, beyond the two pictures above: the textured shapes on the
  software adapter, and a normal-mapped model whose material is replaced
  by a material file (both are covered by structural tests only).
- Hot reload in a running `wen play` and `wen view` window (the window loop
  does not run in tests; reloading is tested through `capture_frame`).
- `wen package` and the exported game on a clean machine with material
  files (the export is tested in process).
- No real model run of the AI scorecard (package 10) and no `wish-evals
  run` (it spends quota).

### 7.1 Review

One independent reader went through the finished package (read only: the
code, the tests, this note). One likely-high finding, two medium, three
low-medium or low, and two to write down. What was done with each:

1. **High: a checkpoint could not be restored in a project with a material
   file.** `wish_asset::index::documents` lists material files, so a
   checkpoint keeps them, but restoring sent every kept document through
   the scene loader, which refused `material` with `unknown_key` and
   stopped the whole restore. Meta files had the same latent fault. This
   mattered because the guides tell people to save a checkpoint before a
   large change to a material file, which has no undo. Fixed: only scenes
   and prefabs are brought back by operations in their history; every
   other document is compared as text and written back with the project's
   atomic writer, and a dry run lists it in `restored` or `unchanged`
   (`checkpoints.rs`). Test `material_tools::a_checkpoint_brings_material_and_meta_files_back`
   (a material file, a meta file and a scene changed, then a dry run, a
   restore and a second dry run): it failed before the fix with the
   reader's `unknown_key` and passes after it.
2. **Medium: the material caches were never let go.** `load_materials`
   only added to what it had read and to the renderer's textures, and the
   2^28-pixel budget counted everything ever loaded, so a long editor or
   `wen play` session that swaps textures would end in a false
   `texture_not_loaded` and hold GPU memory. Fixed: material files and
   textures the current scene no longer names are dropped from the stamps
   and from the renderer (`Renderer::forget_texture`). Test
   `material_cache::textures_the_scene_no_longer_names_are_let_go` (six
   textures swapped in under a budget of two, then a scene without the
   file). One consequence, accepted: a `Drawing` that draws two scenes in
   turn reads the other scene's textures again each time.
3. **Medium-low: a failed texture was read and decoded again on every
   picture** (every frame in `wen play`), and a picture over the budget was
   decoded before it was refused. Fixed: a material file or texture that
   could not be used is remembered with its stamp and not read again until
   it changes (its warning is given again each picture); the budget is
   checked from the PNG header before anything is decoded, and a budget
   refusal is not remembered, because it depends on the other textures.
   `MaterialStamps` gained `with_pixel_budget` (so a test reaches the
   budget with small pictures), `files_read` and `textures_held`. Tests
   `material_cache::a_file_that_failed_is_not_read_again_until_it_changes`
   and `material_cache::a_picture_over_the_budget_is_refused_from_its_header`.
4. **Low-medium: the editor could overwrite an agent's change to a material
   file.** It wrote the file from the panel's possibly stale `written` plus
   the edited field. Fixed: what the file writes is read again
   (`asset_info`) right before the one field is merged in. The by-hand test
   now changes the file from outside between two edits and requires that
   the change is kept.
5. **Low, done:**
   - A `uv_scale` of 0, or a `uv_scale` or `uv_offset` number beyond 4096
     either way, is a `check` warning, `material_uv_unusable`, at the
     number's line (it used to be turned into 1 or clamped silently). Test
     `materials::tiling_the_renderer_cannot_use_as_written_is_a_warning`.
   - When the GPU will not bind a material's textures (`texture_set`
     returns `None`), the picture now warns `texture_not_loaded` once for
     the file, instead of falling back without a word.
   - The warnings one picture collects about materials are capped at 32,
     the rest counted in one `warnings_cut_short`; what has been said is a
     `BTreeSet`, not a list searched for every draw. Test
     `materials::a_picture_lists_at_most_32_material_warnings_and_counts_the_rest`.
6. **Written down, not changed:**
   - The renderer reads material textures without checking for links, as
     `load_ui_pictures` reads the interface's pictures; `check` flags links
     through the asset index. A link inside the project that points outside
     it would be read for a picture.
   - `material_unused_on_model` fires on old scenes that have a `Model`
     with a plain `Material`. That `Material` never did anything; the
     warning says so for the first time, and the scene still loads and
     draws as before.

After the fixes: `cargo fmt --all --check` and `cargo clippy --workspace
--all-targets -- -D warnings` clean; `cargo test -p wish_asset -p
wish_render -p wish_control -p wish_cli -p wish_player -p wish_editor`
(debug): 570 passed, 0 failed, 8 ignored (`wish_asset` 31, `wish_render`
175, `wish_control` 190, `wish_editor` 127, `wish_cli` 45, `wish_player`
2); no `.png` modified. The renderer's change here is warnings only, so
the release render run and the picture comparisons of section 7 were not
repeated.

## History

The package was built twice. The first build put the textures and options
on the `Material` component itself (option 3 of 3.1), so that a texture on
a box was one `edit_apply`. It passed its render tests, but it made
`Material` 17 fields: in the inspector that box pushed `Transform` and the
component buttons below the fold at ordinary window sizes (13 of the
editor's tests failed, 7 of the inspector's 8), and every entity with a
material carried 96 bytes instead of 40, for a second way to have textures
whose 16 fields meant nothing whenever `file` was set. It was taken back
to the design of 3.1: `Material` gains only `file`, and the look with
textures is a material file of type `MaterialFile`. The agent that built
it was stopped before it ran the tests on the second design; a second
agent ran them, fixed the three failures above and made the measurements.
