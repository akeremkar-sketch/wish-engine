# M12 package 9: the look lock

**Status:** switched on 19 November 2026 (not committed). The user chose
**Natural** as the default look of 1.0 by eye, from the comparison pictures
of section 7. The defaults of the look are Natural's, the scene and prefab
file format is 2, a file of format 1 is read as it was meant and keeps its
picture to the byte, and the examples and templates have the new look.
What was done, what it costs and what is not verified: **section 10,
"Switched"**. Sections 1 to 9 are the preparation as it was written before
the switch (part 1 of this package: named looks, the recorded defaults,
the migration, the candidates); where they say "today" they mean the
engine before the switch.

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

## 9. Review (of the preparation)

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

## 10. Switched

19 November 2026, part 2 of this package. The user chose Natural. The work
list was section 3.5; this section says what was done with each item, the
decisions the list left open, what changed in pictures, what the new
default costs, and what is not verified. Nothing is committed.

### 10.1 What was done

| Item of 3.5 | Done |
|---|---|
| 1. Defaults | The fourteen attributes in `crates/wish_types/src/render.rs`, with Natural's values read from `looks.json` by the script that made the edit; the field documentation says what the default now is. `plain`, `natural` and `vivid` stay as looks. A test holds that the record of format 1 with Natural's values in place is the engine's whole table of defaults, so `natural` is the defaults exactly and no other default moved (`look_lock.rs::every_look_is_a_full_set_of_clean_values_plain_is_format_1_and_natural_the_defaults`). |
| 2. Format | `wish_scene::FORMAT` is 2; `STEPS` is `[Step { from: 1, defaults: FORMAT_1_DEFAULTS }]`. |
| 3. The record | `crates/wish_scene/src/defaults/format-2.json`, written by the guard test with `UPDATE_DEFAULTS=1`: it differs from `format-1.json` in the format number and the fourteen values, nothing else. `format-1.json` is unchanged and is never edited again. The guard now holds format 2's. |
| 4. Reading format 1 in memory | Built (10.2). |
| 5. The examples | Format 2, not pinned: they take the new look on purpose (10.4). |
| 6. The sun disc under an orthographic camera | A rule of the renderer (10.3). |
| 7. Tests that leaned on "the default is off" | Decided test by test (10.5). |
| 8. Literal formats | The fixtures that draw stay format 1 (they are the old-format corpus and keep their pictures); the fixtures of tests that assert a clean, silent load moved to format 2 (10.5). |
| 9. Game code | The SDK pack is rebuilt (`wen sdk`); the tests build their own. One line in the release note (10.8). |

### 10.2 Reading format 1 without a migration first

A scene or prefab file of format 1 is read by every reader, because they
all go through `wish_scene::document::load_with`:

- **The loader** (`wish_scene/src/document.rs`). The file is validated as
  it is written, so every problem still points at the file's own lines.
  The format number may be the current one or an older one that every
  step exists for (`migrate::can_upgrade_from`). An older file's document
  is then filled in (`fill_in_older`): `migrate::upgrade` runs on the
  document, with the document's own prefabs saying what an instance takes
  from its prefab, and the patches it changed are taken back. The document
  is the file as format 2 holds it: every value it relied on whose default
  changed is written in, the six look resources included when the scene
  did not set them.
- **Only the engine's own types are filled in.** Pinning goes by type
  name, and a registry can hold someone else's type of the same name (the
  tests of `wish_scene` and `wish_asset` have their own `Camera`, `Sky` and
  `Collider`). `Defaults::agreeing_with` keeps only the types whose
  defaults in the registry are the record's of the current format, which
  the engine's are (the guard test); into any other type nothing is
  written.
- **What the loader answers.** `LoadResult.older_format` (the file's
  format, when older) and `LoadResult.canonical`: the file's own text in
  canonical form **in the format it is in**. A warning `old_format`, at
  the file's format number: "This file is format 1; this engine writes
  format 2. It is read as format 1 meant it...", with the hint "Run `wen
  migrate`...". `not_canonical` is judged against the file's own format.
- **`check` and `wen check`: a warning, never an error.** A project of
  format 1 passes, with one `old_format` per file. Decision: not refused
  (it works) and not silent (the person should know the file will be
  rewritten when it is next saved).
- **`wen fmt` never migrates.** It writes a file's canonical form in the
  file's own format: a loose format-1 file becomes a tidy format-1 file,
  and a tidy one passes `wen fmt --check`. The files of an older format
  are named in its answer (`older_format`, a line each in the text form).
  Decision: a formatter that added six resources to a file would change
  more than a formatter may; `wen migrate` does it, on purpose.
- **Tool calls do not repeat it.** `documents.rs` drops `old_format` from
  the notes of a tool call, as it already drops `not_canonical`: how a
  file is written is `check`'s to say.
- **The documents of the control plane** needed no change of mechanism,
  and that is the point that was to be checked: an open document keeps the
  file's text as read (`disk_text`), and "changed outside the engine" is
  judged by that text, never by comparing the file with the document. So
  the difference between a format-1 file and its pinned document is not
  taken for an outside change, does not enter the history and does not
  rewrite the file: looking at a format-1 scene any number of times
  (`scene_get`, `capture_frame`, `entity_get`, a play session) leaves
  revision 0, an empty history, no `changed_by_others` and the file's bytes
  as they were. A real outside edit of the format-1 file is one change by
  the file that touches what was edited (both sides of the comparison are
  pinned). The first change made through the engine writes the document:
  format 2, with the values written in. Undone, the file is exactly what
  `wen migrate` writes for it.
- **A newer file is still refused** (`unsupported_format`, "update the
  engine"), and so is a format no engine wrote (0).
- Checkpoints, export, the player, sessions, hot reload (`wen watch`) and
  asset moves read through the same loader or copy the file's text as it
  is; none of them rewrites a format-1 file.

Tests: `look_lock.rs::a_format_1_project_opens_as_it_was_and_is_saved_migrated_on_its_next_change`
(all of the above through the tools, including the equal bytes with `wen
migrate`), `a_format_1_scene_keeps_its_picture_to_the_byte` (three scenes:
the format-1 file, its migrated form and Plain applied to its content draw
the same bytes; the same content as format 2 draws the default look),
`a_project_is_migrated_with_the_switchs_step`;
`wish_scene/tests/document.rs::a_file_of_format_1_is_read_as_the_current_format_with_a_warning`;
`wish_cli/tests/project.rs::a_project_of_format_1_is_read_as_it_is_and_migrated_on_request`
(`check`, `run`, `fmt`, `fmt --check`, `migrate` on the command line).

Limits: the equal bytes hold for a file in canonical form (what the engine
writes and `wen fmt` gives): `wen migrate` works on the file's JSON and
keeps an empty `"components": {}` that the document form drops. A scene's
world differs from the one the old engine made of the same file: the
pinned resources are resources of the world (section 3.4), so its world
hash is another, and a replay recorded before the switch diverges at tick
0. Measured on the examples (240 ticks with input): `hop`, whose scene sets
no look resource, has the same hash on the old engine and on the new one as
format 2; `islands` and `bricks` set `Sky` colours, so their `Sky` now
holds a sun size of 3 and their hash is another; every format-1 copy read
by the new engine has another hash than on the old. No hash or replay is
recorded in the repository for a scene (the recorded hashes of `wish_ecs`
and `wish_physics` did not move; `wish_core`'s neither).

### 10.3 Rules of the renderer, not defaults

Checked with pictures (`C:/wish-build/look-switch/`): each default in a 2D
scene (`bricks`, and `bricks` with the default sky), an interface-only
scene (`quiz`) and the 3D examples.

- **The sun's disc is never drawn through an orthographic camera**
  (`look::sun_disc`). Such a camera looks one way from every pixel, so the
  disc would fill the background or not show; with a default of 3 degrees
  a 2D scene whose camera faces the sun would have lost its sky. Test:
  `look.rs::an_orthographic_camera_never_shows_the_sun_disc` (the picture
  with a sun size of 3 and of 30 is the picture without a disc, and
  `effects` leaves `sun_disc` out).
- **`sun_disc_without_light` is given only for a size someone changed.**
  With a disc by default, a scene without a directional light (an
  interior, a menu, a 2D scene) is no mistake and has no disc; the
  warning remains for a `Sky.sun_size_deg` other than the default in a
  scene without the light, and never under an orthographic camera. (So a
  `sun_size_deg: 3` written out in such a scene is silent too: the
  renderer sees the value, not whether it was written; section 11.)
- **A value that is not a number is "no effect" where a field has one, and
  the default where it does not.** The renderer's cleaning (`look::clean`):
  a broken value written by game code never turns an effect on. Three
  fallbacks were stronger than the new defaults and were changed in the
  review (section 11, item 3).
- **Nothing else needed a rule.** Sprites, particles and the interface are
  drawn after the look: `bricks` and `quiz` do not change (no pixel
  differs by more than 2/255 at 1280x720). What the look touches in such a
  scene is the sky behind the sprites: with the default sky it gets
  slightly brighter in the middle and a faint vignette (7 levels from the
  middle to a corner against 5 before); the fog hazes it towards the
  horizon's colour, which is what an orthographic camera shows anyway.
  Cascades work under an orthographic camera (`cascades.rs`, package 6)
  and are not drawn at all in a scene without a directional light.
- **One class of scene is worse off and it is not fixed here:** pixel art
  shown on lit, textured shapes (not on `Sprite`s) is softened by FXAA.
  The field's documentation says so and `antialiasing: "Off"` is one
  field. A 2D game made of sprites is not affected.
- **Cost in scenes that show nothing of it:** `bricks` and `quiz` draw 19
  and 18 passes where they drew 4 and 3 (bloom, FXAA and the look's sky
  pass over a sky nobody sees much of). Not skipped by a rule: the sky can
  bloom, and a rule that guesses "nothing to do" would change bytes.

### 10.4 The examples, the templates and a new project

All six example scenes are format 2 with nothing pinned; `wen new`, the
templates (they are the examples) and `edit_apply` with `create` make
format 2. Their scenario tests pass at the same ticks as before (`islands`'
`win` at 889, `checkpoint` at 294). No example needed a choice of its own.

| Example | Under Natural |
|---|---|
| `islands` | Deeper colours, smoothed edges, a faint vignette. The sun is behind the camera. Better. |
| `hop` | The grey band under the horizon is gone (the haze); the platform's edges are smooth. Much better. The gold coins are darker. |
| `collect` | The vignette shows on the white ground; **the gold coins are darker and duller**: a rough metal now reflects the sky's mean light, not the bright horizon. This is the change the candidate pictures showed for Natural; it is the one thing here that someone could call worse. |
| `first-pixel` | The haze joins ground and sky; the metal ball is darker for the same reason. |
| `bricks` (2D) | The same picture (no pixel differs by more than 2/255). |
| `quiz` (interface) | The same picture. |

### 10.5 Tests: what moved and what was decided

The first full run after the switch: 917 passed, 30 failed. With the
in-memory reading of format 1, **every golden image of `wish_render` is
unchanged** (their fixtures are format 1: `basic`, `lights`, `many_lights`,
`probe`, `sprites`, `ui`, every `look_*`, `bloom_*` and view): "off is
exact" now reads "Plain is exact", and the tests that prove it prove it
with format-1 fixtures as they did, and with `apply_look plain` in the
control plane's tests. The 30, by cause:

- **A clean load is no longer silent for a format-1 fixture** (the
  `old_format` warning), 15 tests in `wish_asset` (`materials`,
  `project`), `wish_cli` (`project`) and `wish_scene` (`document`), and
  `scene_tools::an_agent_builds_a_scene_and_the_file_follows` (the file
  listed six resources after its first change). These tests are about
  loading, checking and editing, not about the look: their scene and
  prefab fixtures are now format 2. `wish_scene/tests/document.rs` moved
  whole (38 fixtures) and got a test of its own for format 1.
- **Tests that set look effects from "nothing"** (`view_tools.rs`: the
  look, bloom, cascades, the cost of a picture; the editor's
  `parity_ops.rs`: the same four by hand). They now say Plain where they
  measure a setting (`apply_look plain` first, then the settings one by
  one, and Plain again at the end gives the first picture to the byte),
  and they assert the default where the test is about the default: a new
  scene answers all six effects and four cascades, and removing the look's
  resources gives the default picture back to the byte. The by-hand tests
  change values in the other direction where the old value is now the
  default (anti-aliasing switched off, the ambient light to `Color`, two
  cascades), and one switches bloom off by hand.
- **Counts of a default frame** (`wen bench --render`'s test; two unit
  tests of `shadow.rs` that read `RenderSettings::default()`): accepted,
  with the new numbers explained.
- **What a tool call keeps in memory.** The four cascade maps of the
  default look are 64 MiB, which was the whole budget
  (`KEPT_AFTER_A_TOOL_CALL`): every default `capture_frame` would have made
  them again. The budget is 128 MiB (the maps, and a 1280x720 picture with
  its bloom and FXAA targets); the test follows, and still shows that a
  larger target goes when the call ends.
- **Golden pictures of the default look**: the two example goldens and the
  editor's window (10.6).
- One catalogue text that did not end in a full stop.

### 10.6 Every picture that changed

Refreshed with the new engine, each one looked at. Nine files:

| File | What changed |
|---|---|
| `examples/islands/tests/golden/win.png` | The showcase's win screen under Natural: deeper colours, smoothed edges, the sky hazed; the interface on it is the same. 78% of the pixels differ by more than 2. Right. |
| `examples/bricks/tests/golden/won.png` | No pixel differs by more than 2 (the old golden still passed); rewritten so that it is the engine's picture. Right. |
| `examples/islands/.template/thumbnail.png` | As the scene's picture above. |
| `examples/hop/.template/thumbnail.png` | The grey band under the horizon is gone; darker coins. |
| `examples/collect/.template/thumbnail.png` | A faint vignette on the white ground; darker coins. |
| `examples/bricks/.template/thumbnail.png` | Nothing visible (at most 2/255). |
| `examples/quiz/.template/thumbnail.png` | Nothing visible (at most 1/255). |
| `crates/wish_editor/assets/empty-project.png` | The welcome screen's picture of an empty project: the starting scene under Natural (the haze joins ground and sky). |
| `crates/wish_editor/tests/golden/editor_window.png` | Only the scene picture inside the window: the starting scene under Natural. The interface around it is pixel for pixel the same. |

The thumbnails are scene captures at 640x360 on the hardware adapter, the
way the old ones were made (checked: the old engine redraws the old ones
to within a few levels). Not changed: every golden image of `wish_render`,
and the editor's `inspector.png`.

### 10.7 What the new default costs

`wen bench --render` (a frame drawn and read back; release build, four
cores at low priority), the same commands before the switch (the old
`wen`, whose default is now called Plain) and after it. The benchmark's
scene sets nothing of the look, so it measures the default.

| Adapter | Scene | Size | Before | After |
|---|---|---|---|---|
| WARP (software), Dx12 | 20 surfaces | 512x288 | 49 ms | **129 ms** |
| WARP (software), Dx12 | 20 surfaces | 1280x720 | 241 ms | **704 ms** |
| WARP (software), Dx12 | 2500 surfaces, 16 lamps | 512x288 | 324 ms | 376 ms |
| WARP (software), Dx12 | 2500 surfaces, 16 lamps | 1280x720 | 649 ms | 1070 ms |
| RTX 3060 Ti, Vulkan | 20 surfaces | 512x288 | 0.67 ms | 1.05 ms |
| RTX 3060 Ti, Vulkan | 20 surfaces | 1280x720 | 2.07 ms | 2.92 ms |
| RTX 3060 Ti, Vulkan | 2500 surfaces, 16 lamps | 512x288 | 4.24 ms | 5.19 ms |
| RTX 3060 Ti, Vulkan | 2500 surfaces, 16 lamps | 1280x720 | 5.20 ms | 7.05 ms |

(Medians of 30 frames. Reproduce: `wen bench --render --entities 20
--ticks 30 --size 512x288 --software`, and the other rows by their
numbers; without `--software` for the graphics card.)

**On the software adapter the default picture is about 2.7 times as
expensive in a small scene: 80 ms more for a default 512x288
`capture_frame`, almost half a second more at 1280x720.** That is the
price CI and every agent on a machine without a graphics card pay for
each lit picture of a scene with the default look. Most of it is bloom
(about 60 ms and 360 ms of the two, by the bloom note's own measurement);
the rest is FXAA, the look's sky and tone mapping passes and three more
shadow maps. It grows with the picture, not with the scene. On a graphics
card it is under a millisecond at 1280x720 in a small scene, about two in
a large one. The debug views, `describe_view`'s identity pass and
everything in a format-1 or Plain scene cost what they did.

Memory: the default look's shadow maps are 64 MiB of GPU memory (four of
2048 texels) where one was 16 MiB, and a tool server keeps up to 128 MiB
of targets between calls (was 64).

The whole test suite took 39 minutes on this machine in the first run
after the switch, most of it building; there is no timing of the suite
before the switch from the same conditions to compare with.

### 10.8 The release note

> **The default look changed, and scene files are format 2.** A new scene
> now has the look "Natural": the sky lights the scene, a sun disc, a
> light haze, a soft glow on bright light, gentle colour grading, smoothed
> edges (FXAA) and four shadow cascades. **Your existing scenes keep the
> look they had.** Scene and prefab files written before this release are
> format 1; the engine reads them as they were meant, so they look and
> play as before, and `check` names them with the warning `old_format`.
> A file is rewritten as format 2, with the values it relied on written
> into it, the first time you change it in the editor or through a tool,
> or all at once with `wen migrate`. To give an old scene the new look,
> apply it: *Scene resources*, *Add resource...*, *Apply a look...*,
> *Natural* (or `edit_apply` with `{ "op": "apply_look", "look":
> "natural" }`); `plain` turns every effect off. **Record replays again**:
> a replay made before a scene was read by this version no longer matches
> it. Game code that builds a `Fog`, `Bloom`, `ColorGrading`, `Sky`,
> `Ambient` or `RenderSettings` from `Default::default()` gets the new
> values when it is rebuilt. Pictures of a default scene take longer on a
> machine without a graphics card (about 130 ms instead of 50 ms for a
> 512x288 capture); a test that only needs to see where things are can
> use `describe_view` or a debug view, or apply `plain`. An engine older
> than this one cannot read a format-2 file.

### 10.9 Result of the switch

Measured on this machine (Windows, WARP for the pictures; four cores at
low priority), 19 November 2026.

- **The switch as built, before the review's fixes:** `cargo fmt --all
  --check` clean; `cargo clippy --workspace --all-targets -- -D warnings`
  clean; `cargo test --workspace --no-fail-fast` 949 passed, **1 failed**
  (`wish_editor/tests/manual_only.rs`, section 11, item 1); `cargo test
  --release -p wish_render` 191 passed.
- **After the review's fixes (section 11):** fmt and clippy (whole
  workspace, every target) clean; `cargo test -p wish_render -p
  wish_asset` 222 passed, 0 failed; `cargo test --release -p wish_render`
  191 passed, 0 failed; `cargo test -p wish_control -p
  wish_cli -p wish_mcp -p wish_evals` 271 passed, 0 failed (the new tests
  `an_ambient_colour_the_sky_light_does_not_use_is_said` and
  `the_loader_pins_every_type_the_engine_registers` among them); `cargo
  test -p wish_editor --test manual_only` 1 passed (172 s); `wish-evals
  selftest` 41/41 (task 18 with its corrected setup included). The full
  workspace gate is run again before the commit.
- **Pictures:** the nine files of 10.6, each looked at; no golden image of
  `wish_render` moved.
- **The examples:** every scenario test passes at the tick it passed at
  before (10.4); no recorded world hash of `wish_ecs`, `wish_core` or
  `wish_physics` moved.
- **Cost:** 10.7 (on the software adapter a default 512x288 picture takes
  about 2.7 times as long; on a graphics card under a millisecond more in
  a small scene).

### 10.10 Not done, not verified

- **Linux and lavapipe, for this switch.** Not run yet: nothing of the
  switch is pushed. The refreshed `examples/islands/tests/golden/win.png`
  now has FXAA and bloom in it and was made on WARP; it is compared in CI
  at 2/255. CI (`.github/workflows/ci.yml`) runs on pushes to `main` and to
  the milestone branches (`m[0-9]*`) and on pull requests, on Windows and
  on Linux with lavapipe; every push of `m9-product` so far ran there and
  was green (the branch's Actions run history, as read on 19 November
  2026), the golden images of packages 4 to 8 (fog, grading, FXAA, bloom,
  cascades, blending) included. So the effects themselves agree
  between WARP and lavapipe; the default-look goldens of this switch are
  checked by the first push.
- **Open question for the user: how strongly the sky is reflected under
  sky light.** In the sky-light mode (`Ambient.source: Sky`) the opaque
  pass multiplies both what the sky gives a surface, its diffuse light and
  its *reflection*, by `sky_strength`
  (`crates/wish_render/src/shaders/opaque.wgsl`, the `frame.sky_light`
  branch: `strength` scales the `sky_reflection` term). With Natural's 0.3,
  every metal reflects 0.3 of the sky; in the `Color` mode the reflection
  is at full strength (`sky_radiance(reflected)`, times occlusion only).
  That is why the gold coins of `hop` and `collect` and the ball of
  `first-pixel` are darker under Natural (10.4). Scaling only the diffuse
  part by `sky_strength` would brighten metals under Natural, but it
  changes the look the user approved (and every default-look picture), so
  it is the user's to decide; not changed. If changed, it is a change of
  the default picture inside format 2 (no file's values change), so it
  should come before 1.0 or with a format step.
- **The pictures were looked at by the implementer, not signed off by the
  user.** The user chose Natural from the candidate sheets; the nine
  refreshed files and the before/after sheets in
  `C:/wish-build/look-switch/` are for the user to look at.
- `wen migrate` does not set the `engine` key of `wish.json` (ADR-027,
  rule 3: a follow-up of the public-face package), and `opens_from` does
  not exist. An older engine refuses a format-2 file with
  `unsupported_format`.
- A real project migrated by a person: the migration ran on the examples'
  copies, a new project and the test corpus only.
- The editor showing `old_format` in its *Problems* tab was not looked at
  in the window (the tab lists what `check` answers).
- Tiers, a render scale and looks from packages stay where section 8 put
  them.

## 11. Review (of the switch)

One independent reader went through the switch (read only), and the first
full run of the gate on it had one failing test. No high finding. What was
done with each:

1. **The one failing test: `wish_editor/tests/manual_only.rs`** (M8a's exit
   criterion, the showcase built by hand). It expected the project's tests
   to fail once, because a new island far off the course made the one
   shadow map cover more and moved the winning picture's shadows; the
   person then took the picture as the new golden one by hand. With four
   cascades fitted to the camera by default, the island no longer moves
   the picture, and all four tests passed. It is the only test that uses
   *Update golden pictures* by hand, so the expectation was not just
   changed: the person now changes the look by hand (nothing selected,
   *Add resource...*, *Plain*), which the winning picture shows; its test
   fails and says "golden"; *Update golden pictures*, *Run tests*: all
   four pass; `check` finds no problems; every history entry, the look's
   included, is the human's; the saved scene holds Plain's values. The
   golden image changed is the one in the test's own copy of the project.
2. **Medium: an `Ambient.color` or `intensity` did nothing, silently.**
   `source` is `Sky` by default, and the sky light uses neither. A new
   warning, `ambient_color_unused` (in `docs::DIAGNOSTICS`): when the
   source is `Sky` and `color` or `intensity` differs from its default,
   naming the first of them. `check` gives it from the scene as loaded
   (`wish_asset::project`, located at the field's line in the file; a file
   of format 1 is judged with its own default, a flat light, so only a
   `source: "Sky"` written out there is warned about); the lit picture
   gives it (`capture_frame`, `describe_view`; `RenderScene.ambient_unused`
   from `extract`, located at the field by `view.rs`). Its hint says to
   set `source: "Color"` or apply `plain`, or to remove the two fields and
   use `sky_strength`. Test:
   `view_tools.rs::an_ambient_colour_the_sky_light_does_not_use_is_said`
   (both tools, both fields, the picture unchanged by the setting, quiet
   with `Color` and after `plain`, the two format-1 cases, the catalogue).
   The evaluation task 18 (a lamp at night, `m6_tasks.rs`) set only
   `intensity: 0` to take the fill light away: under the sky light that
   did nothing (the scene stayed dark only because its sky's luminance is
   0 too). It now sets `source: "Color"` with it, which is what it meant,
   and gives no warning.
3. **Low: three fallbacks of the renderer's cleaning were stronger than
   the new defaults** (`look.rs`): `Fog.max_opacity` 1 (default 0.9),
   `Bloom.spread` 0.5 (default 0.4), `Ambient.sky_strength` 1 (default
   0.3); `Fog.height_falloff`'s 0 (a fog that does not thin out with
   height, default 0.05) was the same kind and was changed too. Now a
   field that has a value switching its effect off takes it (`max_opacity`
   0 is no fog, as a broken `density` already was), and a field without
   one takes its type's default, read from the type
   (`Ambient::default().sky_strength`, `Fog::default().height_falloff`,
   `Bloom::default().spread`). The module's comment says exactly which
   field does what. Tests: `look.rs::settings_that_are_not_numbers_...`
   (a broken limit is no fog; a broken falloff is the default's picture;
   a broken sky strength is 0.3's) and `bloom.rs`'s (a broken spread is
   0.4's). `shadow.rs`'s header no longer calls one map the default.
4. **Low: `Defaults::agreeing_with` left a type out silently** when the
   registry's defaults and the record disagree: a field added to `Fog`
   without `UPDATE_DEFAULTS=1` would stop format-1 files being pinned for
   `Fog`, and old scenes would change look with no diagnostic. The guard
   test fails in that case already, but it reads the record from disk;
   the loader uses the record built into it. New test
   `look_lock.rs::the_loader_pins_every_type_the_engine_registers`: with
   the built-in record, `agreeing_with` keeps every component and resource
   the engine registers, and every type of the format-1 record is among
   them. No warning at run time (the test makes drift a failing build, not
   a user's changed scene).
5. **Low, a note: checkpoints keep two forms of a format-1 file.** A
   checkpoint stores an open document as the engine holds it (format 2,
   pinned) and a document nobody opened as the file's own text (format 1).
   Both mean the same picture and the same world: a restore loads the kept
   text through the same loader (pinned on reading) and compares documents,
   not texts, so nothing is restored that did not change; a document
   deleted since is written back as the kept text, in whichever of the two
   formats it was kept. Harmless; not changed.
6. **Docs.** 10.10 said that CI runs only on pushes to `main`; it runs on
   the milestone branches too, on Windows and on Linux with lavapipe
   (corrected). 10.9 has the measured result.
7. **An open question for the user** (10.10): the sky's reflection is
   scaled by `sky_strength` too, which is why metals are darker under
   Natural. Not changed.

Noted, not done:

- **The cost in CI.** About 150 captures of the default look on the
  software adapter in the test suites (the reader's estimate: the tests
  that draw a format-2 scene without a look). At roughly tens of
  milliseconds more each (10.7 measured 80 ms more at 512x288, and most
  test pictures are smaller), that is a few seconds to under a minute per
  operating system. Not measured. Ways to cut it if it
  matters: tests that only need positions ask `describe_view` or a debug
  view, or apply `plain`; smaller pictures; bloom skipped when no texel is
  above the threshold (would need a rule that changes no byte).
- **`sun_disc_without_light` is silent for a written `sun_size_deg: 3`**
  in a scene without a directional light. The renderer sees the value,
  not whether a file wrote it, and the value is the default; telling the
  two apart would need the scene file, for a warning about a disc that is
  drawn nowhere. Documented (10.3), not changed.
