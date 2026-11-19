# M12 package 9: the look lock

**Status:** prepared, not switched. Built and tested: named looks (the
`edit_apply` operation `apply_look`, its answer `now_resources`, the
editor's "Apply a look..."), the recorded defaults of file format 1 with a
guard test, the migration a change of defaults needs, and comparison
pictures of three candidate default looks. **Not done, by design: the
switch itself.** The user picks the default look by eye from the pictures
(section 7); the switch is then the small change of section 3.5.

Recommended model · effort for the switch and for finishing this package:
Opus 5.5 · high (it touches golden images and many tests, but every decision
is made here).

Plan: [M12-road-to-1-0.md](M12-road-to-1-0.md) §0 item 9 ("the look lock:
presets and the one switch of the default look, signed off by the user by
eye; every new setting is editable by hand in the editor, invariant 9").
Rules: [DECISIONS.md](../DECISIONS.md) ADR-027 rule 1 (schema defaults are
part of the file format; a default changes only in a planned window, with a
migration). The look packages before it:
[look 1](M12-look-1-sky-fog-grading-fxaa.md),
[bloom](M12-look-2-bloom.md), [cascades](M12-look-3-cascaded-shadows.md),
[materials](M12-materials.md), [blended pass](M12-blended-pass.md).

## 1. Problem

Packages 4 to 8 added the look of 1.0, every setting off by default so that
no picture changed (their rule). A new scene therefore still looks as it
did in M3: flat bluish ambient light, no anti-aliasing, a grey band under
the horizon, one shadow map for everything (blocky or missing shadows in a
large level). The plan ends that once, at the look lock, signed off by the
user by eye. Before that can happen:

1. A person or an agent needs a way to give a scene a whole look in one
   step (presets), as a capability of the control plane first (invariant
   4), reachable by hand (invariant 9).
2. The user needs candidates to choose from, as pictures of real scenes.
3. A default is part of the file format (ADR-027 rule 1): every scene that
   leaves a field out depends on it. Changing the default look needs a
   migration that keeps old files' pictures, and nothing for it existed:
   `wen migrate` had no step and refused anything that was not format 1.
4. Every new look setting must be editable by hand (invariant 9): audited
   here.

Scope (§0 of the plan, the short path): presets and the one switch. **Not
built: Low/Medium/High tiers and a render scale.** They are in the full
scope (plan, Phase 2) and in release 1.3 by ADR-012 as amended. I do not
think 1.0 needs them: a tier is a preset of cost settings, and the cost
settings that exist (`shadows`, `shadow_map_size`, `shadow_cascades`,
`antialiasing`) are already one field each; a render scale needs an
upscaling pass and a decision about the interface's resolution. *If the
user wants a "Low" preset at 1.0 anyway, it is one more entry of
`looks.json` (shadows off or one small map, no FXAA, no bloom): this grows
the 1.0 scope, slightly.*

## 2. What the code says today

Every look setting, its type and field, default, range and what reads it.
All are resources of the scene except the last block. A resource the scene
does not set is drawn with its defaults (`wish_render::extract`,
`unwrap_or_default`).

| Type | Field | Default | Range / choices | Unit | Package | In a look? |
|---|---|---|---|---|---|---|
| `Ambient` | `source` | `Color` | `Color`, `Sky` | | 4 | yes |
| `Ambient` | `sky_strength` | 1 | 0 to 10 | | 4 | yes |
| `Ambient` | `color` | (0.5, 0.6, 0.8) | colour | | M3 | no: the scene's |
| `Ambient` | `intensity` | 20 000 | 0 to 200 000 | lux | M3 | no: the scene's |
| `Sky` | `sun_size_deg` | 0 | 0 to 45 | deg | 4 | yes |
| `Sky` | `zenith`, `horizon`, `ground` | (0.18, 0.36, 0.75), (0.65, 0.75, 0.85), (0.25, 0.22, 0.20) | colours | | M3 | no: the scene's |
| `Sky` | `luminance` | 20 000 | 0 to 100 000 | cd/m² | M3 | no: the scene's |
| `Fog` | `tint` | white | colour | | 4 | yes |
| `Fog` | `density` | 0 | 0 to 1 | 1/m | 4 | yes |
| `Fog` | `height_falloff` | 0 | 0 to 10 | 1/m | 4 | yes |
| `Fog` | `base_height` | 0 | -100 000 to 100 000 | m | 4 | yes |
| `Fog` | `start` | 0 | 0 to 100 000 | m | 4 | yes |
| `Fog` | `max_opacity` | 1 | 0 to 1 | | 4 | yes |
| `ColorGrading` | `contrast` | 1 | 0 to 2 | | 4 | yes |
| `ColorGrading` | `saturation` | 1 | 0 to 2 | | 4 | yes |
| `ColorGrading` | `temperature` | 0 | -1 to 1 | | 4 | yes |
| `ColorGrading` | `tint` | 0 | -1 to 1 | | 4 | yes |
| `ColorGrading` | `vignette` | 0 | 0 to 1 | | 4 | yes |
| `Bloom` | `strength` | 0 | 0 to 1 | | 5 | yes |
| `Bloom` | `spread` | 0.5 | 0 to 1 | | 5 | yes |
| `Bloom` | `threshold` | 0 | 0 to 64 | (1 = white) | 5 | yes |
| `RenderSettings` | `antialiasing` | `Off` | `Off`, `Fxaa` | | 4 | yes |
| `RenderSettings` | `shadow_cascades` | 1 | 1 to 4 | | 6 | yes |
| `RenderSettings` | `shadow_distance` | 100 | 0.1 to 10 000 | m | 6 | yes |
| `RenderSettings` | `shadows` | true | | | 1 | no: a cost choice |
| `RenderSettings` | `shadow_map_size` | 2048 | 64 to 4096 | px | 1 | no: a cost choice |
| `Exposure` | `ev100` | 14 | -10 to 30 | EV | M3 | no: the scene's camera |
| `Material` (component) | `file` | empty | asset | | 7 | no: per surface |
| `Material` (component) | `base_color` alpha | 1 | 0 to 1 | | 8 | no: per surface |
| `MaterialFile` (file) | 16 fields (M12-materials.md §3.2) | the look of a shape without a material | | | 7, 8 | no: per surface |

Every number a shader sees is cleaned on the CPU first (`look::clean`,
`shadow::count`, `shadow::distance`): not a number becomes the default,
out of range becomes the range's end.

## 3. Decisions

### 3.1 What a look owns

A look is a full set of values for **twenty fields**: `Ambient.source` and
`sky_strength`, `Sky.sun_size_deg`, all six of `Fog`, all five of
`ColorGrading`, all three of `Bloom`, and `RenderSettings.antialiasing`,
`shadow_cascades` and `shadow_distance`. Those are the "yes" rows above.

It does not own the sky's colours and luminance, the flat ambient light's
colour and strength, the exposure or the two cost choices of
`RenderSettings`. Those are a scene's own art and a machine's budget: a
look that reset the showcase's sky to the default blue, or turned shadows
back on on a slow machine, would be wrong. (`Ambient.color` and
`intensity` are not used once `source` is `Sky`, so Natural and Vivid need
nothing from them; Plain uses them as the scene has them, which is today's
picture.)

### 3.2 Presets: `apply_look`

Options for applying one:

1. **Write every owned value explicitly into the scene's resources.**
2. A named field (`RenderSettings.look: Natural`) that the renderer
   resolves.
3. A tool of its own (`look { action: list | apply }`).

Decision: **1, as an operation of `edit_apply`**:
`{ "op": "apply_look", "look": "natural" }`.

- **ADR-027.** With 1 the file says what it looks like, whatever the
  defaults become: a scene that applied a look before the switch keeps its
  picture after it, with nothing to migrate. With 2 the named field's
  meaning is a second set of defaults hidden in the renderer: changing a
  preset's numbers would change every scene that names it, which is exactly
  the problem rule 1 exists to stop, and a person could not see or change
  one value of the look without leaving the preset.
- **Tokens.** An operation is a few words in `edit_apply`'s description;
  a tool would be one more entry of the list every agent session pays for.
  The operation also composes: a look and other edits in one transaction.
- **The answer makes a read-back unnecessary.** `edit_apply` now answers
  `now_resources`: every resource the change touched, as it is after it,
  every field filled in (for any resource change, not only looks). After
  `apply_look` that is the whole look.
- **It is an ordinary edit.** `apply_look` is read (`wish_control::ops`) as
  one `set_resource` per look resource, in name order, with the owned
  values written out (`set_resource` merges, so the fields a look does not
  own stay). So it is one entry of the history, undone in one step, checked
  by `dry_run`, refused all or nothing, allowed on a running game
  (`session`), and recorded as the caller's (`actor`). A problem is
  reported at the `apply_look`'s own index: `parse_ops_traced` keeps which
  operation each edit came from, and `edit_apply` turns `location.op` back
  (without that, a mistake in the operation after a look would point six
  operations too far).
- **Data.** The looks are `crates/wish_control/src/looks.json`
  (`wish_control::looks`): id, English name and description, and the owned
  values. Every value is finite and in its range (the edit's schema check
  refuses anything else, so the renderer's own cleaning has nothing to
  change); a test checks that every look owns the same twenty fields and
  that Plain is exactly the format-1 defaults. An unknown name is
  `unknown_look` with the names there are.
- **By hand.** The scene resources' "Add resource..." menu (the inspector
  with nothing selected) starts with "Apply a look...": the looks by name
  in the interface's language, each with its description on hover; a click
  is one `edit_apply` by `human`. In the menu, not as a button beside it:
  a button changed 8 586 pixels of the editor's golden window
  (`app::the_whole_editor_window_matches_its_golden_image`), and this
  package changes no golden image. New interface text in all eleven language files. A `PARITY` row
  (`edit_apply:apply_look`) and a by-hand test
  (`parity_ops::a_look_is_applied_by_hand`).
- **Command line.** For free: `wen call edit_apply --ops '[{"op":
  "apply_look", "look": "natural"}]'`. No new command.
- **Discoverable.** The guide (`wish://docs/guide`, "Game systems") names
  the operation; a concept hint answers people who look for a
  `PostProcessVolume`, `Volume`, `WorldEnvironment` or "preset" component.

Looks from packages (a `look` kind beside themes and workspaces) were left
out: the M10 package kinds would take them later without a format change,
since a look is only data of the same shape.

### 3.3 The candidates

Three looks, each a full set of the twenty values, picked by drawing nine
scenes with each and adjusting (section 7; two rounds: the first Natural
had contrast 1.08, sky light 0.35 and fog 0.002 / 0.03, which washed out
the white ground of `collect` and `first-pixel`; the first Vivid's
temperature 0.1 turned white ground yellow).

| | Plain | Natural | Vivid |
|---|---|---|---|
| `Ambient.source` / `sky_strength` | `Color` / 1 | `Sky` / 0.3 | `Sky` / 0.3 |
| `Sky.sun_size_deg` | 0 | 3 | 4 |
| `Fog.density` / `height_falloff` / `max_opacity` | 0 / 0 / 1 | 0.0015 / 0.05 / 0.9 | 0.004 / 0.06 / 0.95 |
| `Fog.tint` / `base_height` / `start` | white / 0 / 0 | white / 0 / 0 | white / 0 / 0 |
| `Bloom.strength` / `spread` / `threshold` | 0 / 0.5 / 0 | 0.05 / 0.4 / 0 | 0.12 / 0.35 / 0 |
| `ColorGrading` contrast / saturation / temperature / tint / vignette | 1 / 1 / 0 / 0 / 0 | 1.05 / 1.1 / 0.03 / 0 / 0.1 | 1.25 / 1.3 / 0.06 / 0 / 0.25 |
| `RenderSettings` antialiasing / cascades / distance | `Off` / 1 / 100 | `Fxaa` / 4 / 100 | `Fxaa` / 4 / 100 |

- **Plain** is today's picture, to the byte (drawn both ways in every
  scene of section 7 and in the tests).
- **Natural** is today's picture finished: the sky lights the scene, a
  small sun disc, a haze that blends the far ground into the horizon (the
  grey band under the horizon is gone), smooth edges, a faint vignette,
  sharp shadows near the camera in a large level. Metal looks darker,
  because a rough metal now reflects the sky's mean light instead of the
  grey horizon.
- **Vivid** is Natural stronger: punchier, warmer, more saturated, thicker
  haze, a clear glow, darker corners. Right for a stylised game; as a
  default it tints white grounds and darkens every corner.

**Recommendation: Natural** as the default of 1.0. Sky strength 0.3 rather
than 1: the default sky is about three times a real clear sky (look-1 §3.1),
so at 1 a top face gets 17 to 48 klux where the flat ambient gives 10 to 16;
at 0.3 the brightness of a scene stays close to today's. Four cascades to
100 m: the outdoor level shows they are the largest single improvement, and
in a small scene they cost three more shadow passes and change little.

### 3.4 The migration

ADR-027 rule 1 asks that a changed default comes with a migration that
says, for each changed default, what happens to existing files: they keep
their meaning (the old value is written into every file that left it out),
or they take the new value on purpose. What is built:

- **The recorded defaults** (`crates/wish_scene/src/defaults/format-1.json`,
  `wish_scene::migrate::FORMAT_1_DEFAULTS`): the full default value of
  every engine component (32) and resource (12) as files of format 1 rely
  on it. This is the guard ADR-027 asked for in Phase 0: the test
  `wish_control/tests/look_lock.rs::the_recorded_defaults_are_the_engines`
  fails when a default changes while the format is still 1, and says how
  to do it right. A new type or field is an addition (it cannot change an
  old file's meaning, by the same rule) and is recorded with
  `UPDATE_DEFAULTS=1`, like a golden image. **This is a new rule for every
  later package: adding an engine type or field now needs
  `UPDATE_DEFAULTS=1 cargo test -p wish_control --test look_lock` and a
  look at the diff.**
- **Pinning** (`wish_scene::migrate::pin`): given the old defaults and the
  new, it writes into a scene or prefab file every value the file relied on
  whose default changed:
  - a component or resource that leaves a changed field out gets the old
    value (nested objects field by field);
  - a resource the scene does not set at all, which the engine draws at
    its defaults, is **added** with the old values of its changed fields;
  - a prefab instance's patch rides on its prefab's value, which is pinned
    in the prefab's own file, so it is pinned only where the prefab does not
    give that entity the component;
  - nothing else: a field the file sets keeps its value, unchanged defaults
    stay out, a field the new engine no longer has is never written (a
    removal needs its own step), and with no changed default a file is not
    touched at all.
- **Steps** (`wish_scene::migrate::Step`, `STEPS`, `upgrade`): format `n`
  to `n + 1` with format `n`'s recorded defaults, the newest defaults being
  the running engine's (`Defaults::of_registry`). `STEPS` is empty today.
- **`wen migrate`** now runs `wish_control::migrate::migrate_project`
  (invariant 4: the capability is in the control plane, the command line
  calls it): scene and prefab files through the steps, written in
  canonical form; meta and material files checked as before. Today it
  answers exactly what it answered before (format 1 is current; older and
  newer files are refused with the same codes).

Why the hash moves, and why that is accepted: a resource added to a scene
is a resource in the world, so a migrated scene's world hash differs from
the old one (resource presence is hashed; defaults are not part of the
type fingerprint, so a default change alone moves no hash of a scene that
lacks the resource). A replay recorded on that scene before the switch
then differs from tick 0. The repository has no committed replays (CI
records and replays the examples in one run), and 0.x promises no replay
compatibility; the release notes of the switch say "re-record replays".
The alternative, inserting every look resource into every world at its
defaults so that presence is uniform, would move the hash of every scene
and every replay instead.

### 3.5 The switch, ready but not thrown

The one change, after the user picks (the numbers below are Natural's):

1. **Defaults** in `crates/wish_types/src/render.rs` (fourteen
   `#[schema(default = ...)]` attributes; enum fields get one too, since an
   enum's default is otherwise its first variant):
   `Ambient.source = AmbientSource::Sky`, `Ambient.sky_strength = 0.3`,
   `Sky.sun_size_deg = 3.0`, `Fog.density = 0.0015`,
   `Fog.height_falloff = 0.05`, `Fog.max_opacity = 0.9`,
   `ColorGrading.contrast = 1.05`, `saturation = 1.1`,
   `temperature = 0.03`, `vignette = 0.1`, `Bloom.strength = 0.05`,
   `Bloom.spread = 0.4`, `RenderSettings.antialiasing =
   Antialiasing::Fxaa`, `RenderSettings.shadow_cascades = 4`. The field
   documentation that says "(the default)" moves with them.
2. **Format** `wish_scene::FORMAT` 1 → 2, and
   `STEPS = &[Step { from: 1, defaults: FORMAT_1_DEFAULTS }]`.
3. **The record of format 2**: `UPDATE_DEFAULTS=1 cargo test -p
   wish_control --test look_lock` writes `defaults/format-2.json`;
   `format-1.json` stays as it is, forever.
4. **Reading format 1 without a migration first.** Not built. Today a
   file of another format is refused (`unsupported_format`, "run `wen
   migrate`"). For a smooth upgrade the loaders should read a format-1
   file through `upgrade` in memory and save it as format 2 on its next
   change: `wish_scene::document` (`check_format`) and the two callers that
   read project files (`wish_asset::load_project`, `wish_control`'s
   documents). The point to check there: `documents.rs` notices files
   changed outside the engine, and must not take the difference between the
   file on disk (format 1) and the document (pinned) for such a change.
   The fallback is to refuse and say "run `wen migrate`", which works but
   is not smooth.
5. **The examples**: either `wen migrate` them (they keep today's look and
   their golden images; their templates start new projects Plain), or,
   recommended, let them take the new look on purpose: set `"format": 2`
   by hand and refresh their golden images and template thumbnails
   (`examples/islands/tests/golden/win.png`,
   `examples/bricks/tests/golden/won.png`,
   `examples/*/.template/thumbnail.png`) and look at them.
6. **The sun disc under an orthographic camera.** The disc fills the
   background or does not show (look-1 §3.2); with a default of 3 degrees,
   every 2D scene whose camera looks towards the sun would show it. The
   switch should make the sky pass skip the disc for an orthographic
   camera (one condition in `look::sun_disc`; no picture of today changes,
   since no scene has a disc). In `bricks` the disc does not show today.
7. **Tests that say "the default is off".** Expected from reading (the
   first run of the switch lists them exactly): the golden images of every
   lit picture in `crates/wish_render/tests/golden/` (`basic`, `lights`,
   `many_lights`, `probe`, `sprites`, `ui`, `view_colliders`,
   `view_lights`, every `look_*` and `bloom_*`; the debug views `view-ids`,
   `view-depth`, `view-normals` should not move), the editor's
   `editor_window.png` and `inspector.png` and its `empty-project.png`
   asset; the render tests that measure "the defaults are the old picture"
   (`look.rs`, `bloom.rs`, the cascade tests, `foundation.rs`'s statistics
   of a plain frame), `wish_control/tests/view_tools.rs` (`"effects": []`
   of a default scene), and the editor's by-hand tests that check a new
   project draws no effect before the person sets one (`parity_ops.rs`,
   including this package's `a_look_is_applied_by_hand`, whose first
   capture would then show Natural's effects). Each of those should name
   Plain explicitly (the scene applies `plain` first) rather than lean on
   the default: that keeps what they measure. The recorded world hashes of
   `wish_ecs`, `wish_core` and `wish_physics` do not move (no render type
   is registered there).
8. **Find every literal scene format.** Before the switch, the code that
   wrote a format-1 scene by hand (the starting scene of `wen new`, the
   `wen bench --render` scene, an evaluation task's prefab, the loader's
   and the JSON parser's hints, the catalogue's `missing_format` text) was
   changed to `wish_scene::FORMAT` (`wish_control::SCENE_FORMAT` for
   clients without `wish_scene`), so it follows the switch. What is left
   are test fixtures: about 130 literal `"format": 1` scenes and prefabs in
   about 38 test files (`wish_scene/tests/document.rs` 36,
   `wish_render/tests/*` about 45, `wish_cli/tests/project.rs` 8,
   `wish_control/tests/scene_tools.rs` 6, `wish_scene/tests/prefabs.rs` 9,
   and a few in each of the others; counted with a search for `"format": 1`
   near `entities` or `resources`, so give or take a few). Decision: they
   are left as format 1 on purpose. With the in-memory upgrade of item 4
   they load as before **and keep today's meaning** (they are pinned to the
   format-1 defaults), which is what most of them measure; they become the
   old-format corpus the format window of release 1.2 asks for. A fixture
   that should see the new defaults says `wish_scene::FORMAT`. Without item
   4 every one of them is refused, so item 4 is not optional. Before
   switching, search again: `rg '"format\\?":\s*1' crates` and look at
   every hit that is a scene or a prefab (the scenario test format, the
   material, meta, package, theme, input and project formats are other
   numbers and stay).
9. **Game code** built against the SDK takes the new `Default` of these
   types when rebuilt (`Fog::default()` in a game becomes Natural's fog):
   one line in the release notes.

An alternative, considered and not recommended: **keep the type defaults
at Plain and give every new scene the chosen look explicitly** (`wen new`'s
starting scene, every template, the editor's new scene all apply
`natural`). No default changes, no format change, no migration, almost no
golden image moves. But a scene made any other way (`edit_apply` with
`create`, a file written by hand or by an agent, a world built in code)
would still be Plain, so a project's second level would look different from
its first unless someone remembers; the default look would be a habit of
the templates, not of the engine. It is the fallback if the user prefers
the smaller change.

### 3.6 Invariant 9: every look setting by hand

| Setting | Where in the editor | By-hand test |
|---|---|---|
| `Ambient.source`, `sky_strength` | Inspector, nothing selected: Add resource... `Ambient`; a list, a number box | `parity_ops::the_look_of_a_scene_is_set_by_hand` |
| `Sky.sun_size_deg` | the same, `Sky` | the same |
| `Fog.density`, `height_falloff` | the same, `Fog` | the same |
| `Fog.tint`, `base_height`, `start`, `max_opacity` | the same, `Fog`: colour boxes, number boxes | **new:** `parity_ops::the_other_look_fields_are_set_by_hand` |
| `ColorGrading.contrast`, `saturation`, `temperature`, `vignette` | the same, `ColorGrading` | `the_look_of_a_scene_is_set_by_hand` |
| `ColorGrading.tint` | the same | **new:** `the_other_look_fields_are_set_by_hand` |
| `Bloom.strength`, `spread`, `threshold` | the same, `Bloom` | `parity_ops::bloom_is_set_by_hand` |
| `RenderSettings.antialiasing` | the same, `RenderSettings`: a list | `the_look_of_a_scene_is_set_by_hand` |
| `RenderSettings.shadow_cascades`, `shadow_distance` | the same: number boxes; the view list's "Shadow cascades" | `parity_ops::cascaded_shadows_are_set_by_hand` |
| `RenderSettings.shadows`, `shadow_map_size` | the same: a check box, a number box | **new:** `the_other_look_fields_are_set_by_hand` |
| A whole look | Scene resources: Add resource..., Apply a look | **new:** `parity_ops::a_look_is_applied_by_hand` |
| `Material.file` | Inspector: the text box and the project's files | `parity_ops::a_material_file_is_made_and_changed_by_hand`, `a_surface_is_made_see_through_by_hand` |
| `Material.base_color` alpha | Inspector: the fourth box of `base_color` | `a_surface_is_made_see_through_by_hand` |
| `MaterialFile.base_color`, `alpha_mode`, `alpha_cutoff`, `double_sided` | Browser: select the file, its fields under it | `a_material_file_is_made_and_changed_by_hand`, `a_surface_is_made_see_through_by_hand` |
| `MaterialFile.base_color_texture`, `uv_scale`, `normal_scale` | the same | **new:** `the_other_look_fields_are_set_by_hand` |
| `MaterialFile.metallic`, `roughness`, `emissive`, `metallic_roughness_texture`, `normal_texture`, `occlusion_texture`, `occlusion_strength`, `emissive_texture`, `uv_offset` | the same widgets (number boxes, colour/vector boxes, asset text boxes) | a widget exists for every field: `inspector::every_engine_field_has_a_widget`; not typed one by one |

Gaps found: **none in the editor** (every field has a widget made from its
schema, and every widget writes through the control plane). The gap was in
the tests: seven fields of the look and three kinds of material file field
had never been typed by hand; the new test does it. The asset panel is
short at 1280x720 (package 8's finding): the test scrolls each row into the
band between the panel's tabs and the status bar, as a person would
(`browser_widget`, the old `browser_box` for any widget role).

## 4. What it adds

- No engine type, no field and no default changes: no world hash, golden
  image, template thumbnail or recorded value moves.
- The tool surface: one `edit_apply` operation (`apply_look`) and one answer
  key (`now_resources`); two diagnostics (`unknown_look`,
  `looks_invalid`) and one for the migration (`defaults_record_invalid`),
  in `docs::DIAGNOSTICS`. `MCP_API.md` and the guide say so.
- Files: `crates/wish_control/src/looks.json` and `looks.rs`,
  `crates/wish_control/src/migrate.rs`,
  `crates/wish_scene/src/migrate.rs`,
  `crates/wish_scene/src/defaults/format-1.json`,
  `crates/wish_control/tests/look_lock.rs`; changes in `ops.rs`,
  `tools/scene.rs`, `docs.rs`, `concepts.rs`, `wish_cli`'s
  `project_cmds.rs`, the editor's inspector, app, parity table, tests and
  language files.

## 5. Tests

| What | Where |
|---|---|
| `apply_look` reads as six `set_resource`s; problems point at the operation written; unknown names | `wish_control::ops` unit tests |
| Every look owns the same twenty fields; an unknown look names the others | `wish_control::looks` unit tests |
| Applied by a tool: a dry run changes nothing; the answer's `now_resources` has every look resource; the file has every value; the picture uses all six effects and four cascades; undone in one step to the same bytes; Plain draws the first picture to the byte; a mistake after a look is reported at its own index; a live session takes it | `look_lock.rs::a_look_is_applied_in_one_step_and_the_answer_says_what_it_set` |
| Every look's values are accepted by the schema check; Plain equals the format-1 record | `look_lock.rs::every_look_is_a_full_set_of_clean_values_and_plain_is_the_format_1_defaults` |
| The guard | `look_lock.rs::the_recorded_defaults_are_the_engines` |
| The migration's meaning, on a corpus: every scene of the repository (the six examples), the scene a new project starts with, a prefab and a scene placing it with overrides, and 64 generated scenes with random look resources and fields; under four sets of new defaults (Natural, Vivid, Plain, and every default of every engine type changed); each file means under the new defaults what it meant under the old; a second pass writes nothing; with Plain nothing is written; with no change every example file stays byte for byte | `look_lock.rs::a_migrated_file_means_under_the_new_defaults_what_it_meant_under_the_old` |
| The migration's picture: three scenes (a new project, `first-pixel`, one that sets some look fields), Natural and Vivid as the new defaults: drawn as migrated under the new defaults, the same bytes as today; drawn without the migration, different | `look_lock.rs::a_migrated_scene_keeps_its_picture_to_the_byte` |
| A project migrated with the switch's step: dry run writes nothing; format 2 in canonical form with the old values; a second run finds it current; today's engine has nothing to migrate | `look_lock.rs::a_project_is_migrated_with_the_switchs_step` |
| Pinning rules (only changed defaults, nested fields, instances lean on their prefab, prefab files get no resources, steps, refusals) | `wish_scene::migrate` unit tests |
| `wen migrate` answers as before | `wish_cli/tests/project.rs::migrate_accepts_current_files_and_refuses_what_it_cannot_upgrade` (unchanged) |
| By hand | `parity_ops::a_look_is_applied_by_hand`, `parity_ops::the_other_look_fields_are_set_by_hand`; the parity table (`tests/parity.rs`) and the language files (`tests/languages.rs`) |

How "new defaults" are tried without changing `wish_types`: the
migration takes the old and new defaults as data, so the tests make the
new ones from the record with a candidate's values in place. The picture
test cannot change the engine's defaults, so it draws the migrated file
with the changed resources written out in full at their values under the
new defaults: today's engine then draws exactly what the switched engine
would.

## 6. Result

Prepared on 19 November 2026 on the development machine (Windows 10;
pictures on the RTX 3060 Ti, tests on the software adapter). Nothing is
committed; the switch is not thrown.

- `cargo fmt --all --check`: clean. `cargo clippy --workspace
  --all-targets -- -D warnings`: clean.
- `cargo test -p wish_control -p wish_scene`: 254 passed, 0 failed
  (including the six of `look_lock.rs`); after the last changes,
  `wish_control --lib`, `look_lock`, `diagnostics_catalog`, `catalog`: 40
  passed.
- `cargo test -p wish_cli`: 45 passed, 0 failed (`wen migrate` answers as
  before).
- `cargo test -p wish_editor`: the first run 128 passed and 2 failed: the
  editor window's golden image (8 586 pixels: the "Apply a look..." button
  beside "Add resource...") and the first by-hand look test (a submenu's
  click selected an entity behind it). Both were fixed by moving the looks
  into the "Add resource..." menu; the rerun of `app`, `parity_ops`,
  `parity`, `languages`, `inspector`: 39 passed, 0 failed. No golden image
  changed (`git status` shows no modified `.png`).
- Plain drawn by `apply_look` is today's picture to the byte in all nine
  scenes of section 7 (hardware) and in the tests (software).
- Render code is untouched, so `cargo test --release -p wish_render` was
  not needed.

Not verified: Linux and lavapipe (nothing here is adapter-specific
except the pictures); the full workspace gate; the editor's menu was
driven by its test, not looked at.

## 7. To look at

On the development machine, not in the repository:
`C:/wish-build/look-lock/`, with a `README.txt`. Every picture 1280x720 on
the hardware adapter (NVIDIA GeForce RTX 3060 Ti, Vulkan), drawn with the
engine's own tools (`wen do` of `edit_apply` and `capture_frame`) on copies
of the projects. Nine scenes: `islands` (the showcase from its own camera),
`islands-sun` (a camera looking back at the sun), `hop`, `collect`,
`bricks` (2D), `quiz` (interface only), `first-pixel`, `outdoor-level` (the
400 m level of the cascades package) and `glass` (the see-through scene of
the blended-pass package). For each: `<scene>-today.png` (also exactly
Plain), `<scene>-natural.png`, `<scene>-vivid.png`, and
`sheet-<scene>.png` with the three side by side and the values.

What the pictures show: the outdoor level's shadows are the largest change
(one blocky map against four cascades); the sun disc and its glow show
only from `islands-sun`; the interface (`quiz`) does not change; `bricks`
changes only at its edges (FXAA).

## 8. Not done, handed on

- **The switch** (section 3.5), after the user's choice.
- Reading format 1 in memory (3.5, item 4).
- The sun disc under an orthographic camera (3.5, item 6).
- Tiers and a render scale: release 1.3 (ADR-012 as amended).
- Looks from packages (a `look` kind).
- `wen check` does not suggest a look; the guides (`docs/user`) do not
  mention looks yet (the "public face" package).

## 9. Review

One independent reader went through the preparation (read only). No high
finding; four medium, five low. What was done with each:

1. **Medium: the switch would break code that writes a format-1 scene by
   hand.** The starting scene of `wen new` (`scaffold.rs`), the
   `wen bench --render` scene, an evaluation task's prefab
   (`wish_evals/src/tasks.rs`), the hints of `wish_scene::document` and
   `wish_scene::json`, and the catalogue's `missing_format` text now use
   `wish_scene::FORMAT` (`wish_control::SCENE_FORMAT` re-exports it). No
   behaviour changes today. The test fixtures (about 130) are counted and
   decided in section 3.5, item 8.
2. **Medium: `migrate_project` wrote with a plain `fs::write`, and its `?`
   lost the report of a half-migrated project.** It now checks the path
   (`inside_project`) and writes through the project's atomic writer
   (`write_project_file`: side file, then rename). A file that cannot be
   written or migrated is a located diagnostic and the others go on; the
   report lists both. Test:
   `look_lock::a_migration_goes_on_past_a_file_it_cannot_write_and_takes_prefabs_along`
   (the side file's place is taken by a folder, which fails on every
   system; a read-only flag does not stop a rename on Linux).
3. **Medium (unverified): the MCP guide's size limit** (`text.len() <
   12_000`, `wish_mcp/tests/protocol.rs`). It failed: the guide was 12 218
   bytes. The look's sentence is now one short line ("All at once:
   `edit_apply` `apply_look`."; the tool's description lists the looks)
   and the cascades' sentence lost a clause; `protocol` passes.
4. **Low-medium: `now_resources` had no bound, and one unknown resource
   dropped the whole map.** A resource whose JSON is over 4 KiB is named in
   `now_resources_omitted` instead (no engine resource is that large: the
   largest default is `ActionState`, 2.7 KB; not tested with a large game
   resource); an unknown one is skipped. `MCP_API.md` and the tool's
   description say so.
5. **Low: a record of defaults without its tables read as "no defaults".**
   `Defaults::parse` refuses it (`defaults_record_invalid`); unit test.
6. **Low, in pinning and prefabs.**
   - Entity ids in JSON pointers: `pin` escaped them already; the
     control plane's prefab lookup now does too
     (`wish_scene::migrate::escape` is public).
   - A non-object `resources` was overwritten: `pin` leaves it, and
     `upgrade` refuses a file whose `entities` or `resources` is not an
     object (`migration_refused`, at `/resources`); unit test.
   - A missing prefab, or one nested too deep: a located warning
     (`migration_prefab_unresolved`) says the value was written as if the
     prefab gave none. A prefab placed inside a prefab is followed (its
     root's components count), up to 16 deep. Warnings do not fail
     `wen migrate`.
   - `wen migrate <scene>` alone now migrates the prefabs the scene places
     (and theirs) with it.
7. **Low: concept hints.** `volume` (sound has volumes) and `preset` were
   taken out of the look's hint.
8. **Low, noted.**
   - The refusals of `wen migrate` for a newer or older file have the same
     codes and hints as before; the message now reads "'name': The file is
     format 9, ..." instead of "'name' is format 9, ...".
   - The guard covers the registry's defaults only: `MaterialFile`, the
     input map and `wish.json` have defaults of their own that it does not
     record. The switch does not touch them; a later change to one of them
     needs its own record.
   - The by-hand test types only the first channel of `Fog.tint` and the
     first axis of `uv_scale`; the other boxes are the same widget.
