# M8a — Editor foundation (manual editing)

Status: design (16 November 2026). Branch: `m8a-editor`. Related decisions: ADR-018 (hybrid interface), ADR-009 (single control plane), ADR-003 (schema first), invariants 4, 6, 8, 9. Before this: M7 is done. Everything the editor will rely on exists (control plane, sessions, `describe_view`, undo/redo, window).

## 1. Goal and boundary

With no AI connected, one person builds a part of the showcase game (`examples/islands`) **only by hand**, plays it, debugs it and gets it ready for export. The editor is a **thin face of the control plane**. Every action in the interface is a `Workspace::call`. There can be no capability that is in the interface but not in a tool (invariant 4). The cockpit (conversation, "show and tell", activity feed) is in M8b. M8a only prepares the place where it will sit and the data path (`actor`) of the activity feed.

**Exit criteria (ROADMAP):**
1. Without AI, only by hand: a part of the showcase game is built, played and prepared for export.
2. Parity check: every scene/entity operation in the control plane has a manual counterpart in the interface (automatic list, test).
3. Every action in the interface is a control plane operation. Undo/redo works on every action.
4. A component newly defined in Wasm can be edited in the inspector without writing extra interface code.

## 2. Decision: interface technology (the "temporary part" of ADR-018 is settled)

**Decision: in-process `egui` 0.37 (`egui` + `epaint`, MIT OR Apache-2.0), with our own CPU rasterizer. It does not touch the GPU and follows the existing `wish_window` path.**

Options:

| Option | Pro | Con |
|---|---|---|
| A. `egui-wgpu` (draw on the GPU) | Standard path, fast | It forces invariant 6 (wgpu only in `wish_render`): either `wish_render` knows egui or the editor touches wgpu. Window surface/device management breaks the "draw off-screen + softbuffer" decision of M3 WP8. Headless testing is hard. |
| B. `egui` tessellation + **our own CPU rasterizer** (chosen) | The window path stays the same (the renderer draws off-screen, the UI is laid over it on the CPU, `softbuffer` shows it). **Headless**: the UI is also drawn to an image and tested with golden images (invariant 8). Deterministic. No new native dependency. | We write the rasterizer (~400 lines: textured colored triangles, clipping, alpha). CPU cost in a big window (measured in WP1). |
| C. `egui_software_backend` 0.0.3 | Ready made | Not mature (6.7 thousand downloads, 0.0.x). No guarantee of determinism/testing. |
| D. HTTP + web interface (webview) | Strong for rich text | Hard to embed the view window. Webview dependency. The cockpit design does not exist yet. |

Reason: B protects both the architecture (invariant 6) and testability (invariant 8), and the rest of the engine is not affected. If the technology changes later, the editor shell changes and the core does not. **Risk:** CPU cost. Mitigation: the WP1 proof work measures the frame time at 1920×1080. The budget is ≤ 6 ms per frame (UI). If it is exceeded, the way back to option A stays open through a narrow "overlay" extension in `wish_render` (with an ADR update in the design note).

This decision is written into ADR-018 in `docs/DECISIONS.md` (in WP1). The dependency licenses are in the allowed list in `deny.toml` (MIT/Apache-2.0). When they are added, they are checked with `cargo metadata`.

## 3. Architecture

New crates (dependency direction is downward; the editor uses the control plane and the window/renderer, and they do not know the editor):

- **`wish_editor`** — library: `EditorState` (selection, open panels, camera, layout), a `Panel` trait, each panel has `fn ui(&mut self, ctx, &mut Workspace)`. No panel writes directly to the scene file or the world. They only use `Workspace::call` (tool name + argument). Headless use: `Editor::run_frame(input) -> Frame` (egui output + tessellation), `render_frame(&Frame) -> RgbaImage` (CPU rasterizer). Tests inject input (mouse/keyboard events) and read the frame.
- **`wish_editor_raster`** (or a `raster` module inside `wish_editor`): a deterministic rasterizer that draws the epaint mesh to an image.
- **`wen edit [--root DIR]`** — opens the windowed editor (`wish_window` is extended: renderer view + UI overlay + input forwarding). `wen edit --headless-frame <input.json> --out frame.png` is headless capture (for tests and inspection).

Data flow: UI event → panel → **one** `Workspace::call(tool, args, actor = "human")` → result/diagnostic to the UI. A scene change enters the history with `edit_apply` (the same undo stack). A change from outside/an agent arrives with `changed_by_others` and the panels are refreshed. View: the scene is drawn through the `capture_frame`/render path (the same `Renderer`), and **while playing** the session frames are drawn.

**Automatic inspector from the schema:** `schema_get` gives the fields, type, unit, range and doc comment of each component. The inspector makes widgets from this data (number → drag box with unit and range, `bool` → checkbox, `enum` → dropdown, `Vec3`/`Quat`/`Color` → special widgets, entity/entity reference → picker, `AssetRef` → asset picker). A component defined in Wasm also shows up in `schema_get`, so it is edited **without writing extra code** (criterion 4).

**Parity check (criterion 2):** one table that names the UI counterpart for every tool/operation (`wish_editor::parity::TABLE`: tool name → panel/action id). The test checks that every scene/entity operation in the tool list of `wish_control` is in the table, and that every table entry is bound to a real action id (if a new tool is added, the test breaks; the same rule as the M4 parity test).

## 4. Scope and work packages

Recommended model · effort follows the ROADMAP: Sonnet 5.5 · high; Opus 5.5 · high for interaction/design decisions.

| WP | Work | Model · effort |
|---|---|---|
| WP1 | **Proof work:** egui headless (tessellation) + CPU rasterizer; golden image; 1920×1080 frame time measurement; ADR-018 update; `wish_editor` skeleton, `wen edit --headless-frame` | Opus 5.5 · high |
| WP2 | Shell: window (UI overlay + view), layout, camera movement (`FlyCamera`), open/save project, `changed_by_others`, `wen edit` | Sonnet 5.5 · high |
| WP3 | Hierarchy (`scene_get`, `entity_query`; add/delete/rename/reparent, drag-and-drop) + selection (by clicking in the view: `IdBuffer`) | Sonnet 5.5 · high |
| WP4 | Inspector: widgets made from the schema, unit/range, add/remove component, multi-select, prefab overrides; undo/redo (`edit_history`) | Opus 5.5 · high (schema→widget design), Sonnet 5.5 · high |
| WP5 | Gizmos: move/rotate/scale (local/world, snapping); a drag is one `edit_apply` (a single operation on release, preview while dragging) | Sonnet 5.5 · high |
| WP6 | Asset browser (`asset_list/info/move`, thumbnails, drag-and-drop to the scene/inspector), material editor, prefab editing (carried over from M4: prefab editing becomes a tool) | Sonnet 5.5 · high |
| WP7 | Play: play/pause/step/rewind, `sim_snapshot` timeline, **applying `edit_apply` to a live session** (carried over from M5), input map editor (`input.json`) | Opus 5.5 · high |
| WP8 | Console and diagnostics (`logs_get`, `check`), performance panel (`profile_get`), project settings, "open in external editor", **multilingual interface** ("one language = one file": no fixed text in code, a missing translation falls back to English and is listed in a test; for 1.0, 10–12 languages with Latin/Cyrillic/Greek alphabets, with Roboto; CJK/Arabic/Hindi in M11 — user decision, 16 November 2026), visual polish, **Streamable HTTP transport** (carried over from M4; editor and agent in the same process), export readiness check | Sonnet 5.5 · high |
| WP9 | Parity check table and test, the "manual only" exit scenario, independent closing review, M8a result, merge | Opus 5.5 · high |

## 5. Test plan (invariant 8: headless)

- **Golden image:** for every panel and layout, golden PNGs made with `Editor::run_frame` + the CPU rasterizer (there is no WARP/software inconsistency: the UI is pure CPU, and the aim is to be bit-identical. The view region is excluded, or handled separately with the renderer's golden tolerance).
- **Input tests:** mouse/keyboard are injected ("click `crate` in the hierarchy, type 2 into `Transform.translation.y` in the inspector"), then the control plane value is checked with `entity_get`. Undo returns to the old value.
- **Drag = one operation:** a gizmo drag leaves **one** record in the history. Cancelling (Esc) leaves no record.
- **Parity test:** §3. **Schema test:** a widget can be made for every `Plain` type in `schema_get` (an unknown type → a "raw JSON" fallback widget and a warning; the test checks that none of the engine's types falls back).
- **Manual end to end:** a script like `tests/editor_manual.test.json` (a recorded input stream): a level is built using only UI input, played, and `test_run` passes. The window loop does not run in CI (locally `wen edit --exit-after`), but the headless frame path is in CI.

## 6. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| The CPU rasterizer is slow | The editor feels heavy | Measure in WP1; 6 ms budget; dirty-region drawing; in the worst case a narrow return to option A |
| The T1 mockups were never made | The layout/flow may turn out unusable | At the end of WP2 the real layout, captured headless, is shown to the user (a working skeleton instead of a mockup). Objections are handled before WP3. The cockpit mockups (a precondition of M8b) stay separate. |
| The inspector is not enough for the variety of schemas | Extra code is needed (criterion 4 breaks) | A raw-JSON fallback widget. A special widget per type only for `Vec3/Quat/Color/AssetRef/Entity` |
| Manual and agent changes collide | Lost work | The existing `base_revision` + `changed_by_others`. The editor sends `base_revision` on every write |
| Scope creep (specialist tools) | 1.0 is late | No animation timeline, shader graph, terrain or painting (ADR-018) |

## 7. Decisions that belong to the user

None (the technology decision is explained in §2). The only thing to ask of the user is to look at the layout of the working skeleton at the end of WP2 and object (instead of T1).

## 8. Implementation notes

### WP1 — proof work: headless egui + CPU rasterizer

- **Crate `wish_editor`** (dependencies: `egui` 0.37 with default features off, `wish_core`, `wish_text` only for the Roboto bytes). License scan (`cargo metadata`, 70+ packages): all MIT/Apache-2.0/BSD/Zlib/BSL/Unicode, in the allowed list. The default font packages (OFL/Ubuntu) are turned off, so they do not come in.
- **`raster`**: `Canvas` (RGBA8, sRGB, premultiplied alpha), `Textures` (egui texture deltas, applied in order), `paint` (triangles). Vertices are rounded to 1/256 pixel. Coverage is decided with **integer** edge functions and the top-left rule: on a shared edge there is no double painting or gap (no seam in semi-transparent shapes). Linear/nearest texture sampling. "Flat" triangles (one color, one texture point) are drawn on a fast path without interpolation. The mesh floating point operations are `+ - * /` and rounding, with no machine-dependent function (sin/exp).
- **`Editor`**: `layout` (egui runs, input events are given, no clock: the time comes from the caller) → `Frame` (triangles), `paint(frame, canvas)`, `run_frame` does both. Font: only Roboto, in two families.
- **Tests (`tests/raster.rs`):** a golden image (`inspector.png`: title, drag box, checkbox, text field, slider, button; **byte-for-byte equal**), two editors draw the same image, a mouse click changes the checkbox, frame time measurement (`--ignored`, release).
- **Measurement (release, 1920×1080, 60 labels + 8 inspector panels, 6,524 triangles):** layout 0.6 ms, paint 4.3 ms (the first version was 11.7 ms; the flat triangle path and incremental edge functions gained 2.7×), canvas allocation 2.4 ms (it is reused in practice). The budget (layout + paint ≤ 6 ms) holds. Dirty-region drawing in WP2 if needed.
- **egui 0.37 notes:** `SidePanel` → `Panel::left/right(..).show(ui, ..)`; `Context::run_ui`; the applied `TexturesDelta` must be emptied with `clear()` (in debug, `Drop` checks it).

### WP2 — shell: window, view, `wen edit`

- **`wish_control::Workspace::render_picture(source, camera, w, h, view) -> Picture`** (+ `PictureSource`, `Picture`): the **same** preparation and drawing path as `capture_frame`. It takes the picture itself instead of a PNG, and takes the camera as a function of the first camera (the fly camera). The editor's view is drawn with this. `prepare`/`drawing` were split into cores with no arguments (the tools did not change).
- **`wish_window::run_shell`** (`shell.rs`): a general shell loop that does not know egui. Plain `ShellEvent`s come in (pointer, button, wheel, key name in egui's spelling, text, raw mouse delta). `Shell::frame(size, time, events) -> Option<Picture>` returns ready RGBA (`None`: unchanged). The window shows it with `softbuffer`. It is called every 16 ms and after input.
- **`wish_editor::EditorApp`** (`app.rs`, a `Shell`): top menu (project, scene picker, Undo/Redo, view mode, reset camera, Check, Exit), left hierarchy (read-only for now; WP3), right inspector (placeholder; WP4), bottom status bar (error/warning count + last message), and in the middle an area that egui **does not paint**: the scene picture shows through there (drawn in the order background → scene → panels). While the right button is held, mouse in the view = look, W A S D E Q fly, Shift fast.
- **Every action is a control plane call, the actor is `human`** (`EditorApp::call`): Undo/Redo `edit_history`, Check `check`; reads `project_info`/`scene_get`/`edit_history`. **Someone else's change:** every 0.4 s the `scene_get` revision is checked. An agent's or a manual file edit shows in the hierarchy and the picture (the same single history as M4's `changed_by_others` mechanism).
- **Headless:** `wish_editor::script` (input script: `pointer`, `press/release`, `click`, `key`, `text`, `scroll`, `mouse_delta`; steps `events`/`idle`/`seconds`), `wen edit --headless-frame script.json --out frame.png [--software]`. Windowed: `wen edit [--root K] [--size WxH] [--software] [--exit-after N]` (tried locally: it opens, 5 frames, closes; there is no window in CI).
- **Tests (`wish_editor/tests/app.rs`, pass in release and debug):** hierarchy + scene picture, someone else's change (control plane *and* a manual file edit) shows in the hierarchy/picture, the Undo button enters the single history signed `human`, camera movement with right drag/keys, a bad script is a diagnostic, **golden image of the whole window** (interface byte-for-byte, scene region ≤3/255).
- **Known limits (WP3+):** the hierarchy cannot be clicked, there is no selection in the view, the inspector is empty. No dirty-region drawing (a full frame on every change, ~5 ms at full HD + the scene). The layout is to be shown to the user: [M8a-wp2-editor-window.png](M8a-wp2-editor-window.png).

### WP3 — hierarchy and selection

- **Selection model:** `EditorApp::selection()` (ordered; the first is the "main" one), `select(&[ids])`. In the hierarchy, click = select, **Ctrl+click** = add to/remove from the selection, click on an empty area = clear the selection. In the view, click = select the entity at that pixel (Ctrl: add/remove), click on empty = clear. Selection does not touch the scene: it does not enter the history. If someone else deletes or renames an entity, the selection is pruned accordingly.
- **Hierarchy actions (each is one `edit_apply`, actor `human`, with `base_revision`):** "+" add (`entity_N`, with `Transform`; right click "Add child" adds under it), "Delete" or the **Delete key** (everything selected in one call), **double click / F2 / right click "Rename"** an inline text box (a `rename` operation; Enter applies, Esc/losing focus cancels; the selection moves to the new name), **drag-and-drop** onto another row = make it a child, onto the empty area of the panel = move to root (`reparent`; dropping on itself is ignored, a cycle is rejected with the engine's diagnostic and shown in the status bar). Each action is **one** record in the history. Undo reverses it in one step.
- **Conflict:** an edit is sent with the revision the person saw (`base_revision`). If someone else changed it in the meantime, the call is rejected, the person is told "failed: ...", and the hierarchy is refreshed — the person does not overwrite the agent's work (tested).
- **Selection in the view:** `Workspace::render_ids` (`IdPicture`: `IdBuffer` + names; the same camera as `render_picture`). The screen box of the selected entity is taken from the id buffer (scanning every 2 pixels) and an **orange 2-pixel frame** is drawn over the view (not tied to `describe_view` but to the fly camera).
- **Shell improvements:** a **second layout pass** after an action or selection (the panel says what you did in the same frame: the Delete button, the inspector title); `ShellEvent::Modifiers` → egui `ModifiersChanged` (the Ctrl held state; in a script `{ "modifiers": { "ctrl": true } }`). In a `wen edit` script, clicks use the same path.
- **Tests (`wish_editor/tests/hierarchy.rs`, release+debug):** click/Ctrl-click selection and the empty area, add/delete/undo (3 records, all `human`), the Delete key, rename by double click, reparenting by drag-and-drop and moving to root, selecting by clicking in the view + frame + clearing by clicking on the sky, a stale revision does not overwrite the agent's work. The golden image was renewed (new buttons).
- **Limit (WP4+):** the inspector is still empty (it only writes the selected name). Shared editing in multi-select is in WP4.

### WP4 — inspector (from the schema, no extra code)

- **`wish_editor::inspector`:** the components of the selection are read with `entity_get` (effective values). Widgets are made from the JSON Schema of `schema_get` (`detail: full`) with **general** rules: number → a drag box with unit and range (no decimals for integers), bool → checkbox, `oneOf` → dropdown (the doc of each option in the tooltip), fixed-length number/bool array → x/y/z/w boxes (`Color`: + a color picker, `Quat`: **degrees** — in the order of `edit_apply`'s `euler_deg`, Y then X then Z), `Entity` → a list of the entities in the scene, `AssetRef` → text + a list of project files, `Text` → text box, nested object → recursive grid, object array → folded recursion, anything else → raw JSON (with a warning). The tooltip of a field name is the doc comment + the unit. **Tested: no field of the engine's 59 types falls back to raw JSON** (`has_widget`).
- **When it is sent:** when a drag is released (until then only a preview in the inspector; **one history record**), typing on Enter/losing focus, a click immediately. All are `set` (for every selected entity, **one** `edit_apply`, `human`, `base_revision`). Add component (a searchable menu: it filters as you type) = `set {}` (defaults), remove = `remove`. Multi-select: the common components, the values of the first are shown, a change goes to all.
- **Prefab part** (`instance/part`, selected from the view): a change becomes a `set_override`. The prefab file does not change. No add/remove. It stays in the selection while the instance exists. (Editing the prefab file itself is WP6.)
- **Wasm component (exit criterion 4):** every 2 s the editor looks at `project_info`. When the fingerprint of the game code changes, it reads the schemas again. Test: while the editor is open, game code that defines `Spin { speed: f32 (deg/s), backwards: bool }` is built → it appears in "Add component" → it is added → `speed` is typed with its unit, `backwards` is clicked — **there is not one line of code in the editor for this component**.
- **Shell:** egui's "redraw now" request is obeyed (dropdown menus open on the next frame; before, they did not open without input — this was a bug in a real window too). The **accessibility tree** (AccessKit, ready in egui) was turned on: `Editor::widgets()` / `EditorApp::widgets()` give the role, label, value and place of every widget. Tests find widgets **by name** (not tied to pixels). It is also a base for screen readers and for an agent to "see" the interface later.
- **Tests (`wish_editor/tests/inspector.rs`, 8):** every field has a widget; typing a number is one `human` call; a drag is not sent until it is released and is one record; dropdown + add component/checkbox/remove; multi-select is one call; rotation in degrees; the Wasm component; prefab part override. Total 24 tests in the editor, release+debug.

### WP5 — gizmos (move / rotate / scale)

- **`wish_editor::gizmo`** (pure math, no window and no scene, 8 unit tests): `Eye` (the camera's view: `project` world→window, `ray` window→ray; perspective and orthographic), a **fixed screen length** (110 px, independent of distance), `hit_test` (the nearest handle, within 9 px), `Drag` (start/measure/transform). A drag is read from the **ray**, so the pointer is followed exactly at every distance and angle. Move = the point of the axis line closest to the ray. Rotate = the angle of the point where the ray hits the plane of the ring (shortest way, right-hand rule). Scale = the ratio of distances along the axis, and at the center the ratio of the pointer's distance from the center (guarded against division by zero).
- **Parent and axes:** the drag is computed in world space. Since `Transform` is local, it is passed through the **parent's world matrix** (`GlobalTransform * local⁻¹`; moving with a parent is tested). The axes are **World/Local** (a button in the menu), **Snap** (0.5 m, 15°, 10% step) is in the menu.
- **Choosing the modes:** Move/Rotate/Scale buttons in the menu and the **W/E/R** keys (while the right button is held they stay as fly keys; the mode does not change while typing).
- **Preview while dragging, one operation on release:** `PictureSource::Preview(scene, ops)` — the operations are applied on a **copy** of the document and it is drawn that way (`Workspace::build_preview`; the file, history and revision are not touched; tested, `wish_control/tests/m7_tools.rs`). On release **one** `edit_apply` (`set Transform {only the changed field}`, `human`, `base_revision`): one record in the history. **Esc** undoes it all (no record, the state is "cancelled").
- **View improvements:** the gizmos are on the **main** one (the first) of the selected entities. There is none on a prefab part (the override it would write is WP6 scope). Until a drag is released the selection box is not recalculated. The gizmos follow the camera (the last drawn camera record) and, during a drag, the place in the preview.
- **Tests (`wish_editor/tests/gizmo.rs`, 7):** drawing and fixed length; drag the X handle → one record `human`; the scene does not change until release but the picture does; Esc undoes; snapping 0.5 m; E key + Z ring quarter turn; R key + the center handle scales all three axes; no gizmo without a selection. Total 32 tests in the editor, release+debug.

### WP6 — asset browser, prefab editing, material

- **Control plane (carried over from M4: prefab editing):** `edit_apply` now also edits `.prefab.json` files (the `prefab_read_only` rejection is gone and was removed from the catalog). A `scene` path may be a prefab, and `create: true` opens a new prefab. A prefab has its own history (undo works). After a write the **library is refreshed immediately** (`prefab_stamp` is reset, so an edit made in the same time slice is not missed) and the scenes that have instances are rebuilt. The instance's own override still wins after a prefab edit (tested, `m7_tools.rs`). When `capture_frame`/`render_picture` draws a prefab and there is no camera or light, it adds a **default scene** (a camera from the front/above + daylight). `PictureSource::Preview` for `Workspace::render_picture`/`render_ids` is from WP5.
- **Asset browser (`wish_editor::browser`, bottom panel, resizable):** `asset_list` + prefabs in one list. A filter (text) + type buttons (Images/Models/Sounds/Prefabs). A **thumbnail** for PNGs (decoded once, above 8 MB it is skipped), a type badge for the others. Selecting shows the `asset_info` detail (including who uses it). **Double click or right click "Add to scene"** (prefab → `instantiate`, model → an entity with `Model`; at the ground in the middle of the view). **Drag-and-drop**: drop on the view = place at the ground (y=0) point of the pointer (if it does not see the ground, 6 m along the ray). Drop on an `AssetRef` box in the inspector = sets the value. **Right click "Rename / move..."** = `asset_move` (the scene and prefab references that use it are updated, and the status bar says how many files were updated).
- **Prefab editing:** the scene picker also lists prefabs under the scenes. When you select a prefab, the hierarchy/inspector/gizmos/view work in the **same** way as with a scene (a shared path), and when you save, the instances follow immediately. **Right click "Make prefab" in the hierarchy:** the selected entity and its children become `prefabs/<name>.prefab.json` (the root is `root`, the root's Transform is neutral), and an **instance** is put in its place in the scene with the same id, parent and Transform. Two files, two histories (the prefab file + the scene): the scene goes back to its old state with one undo, and the prefab file stays. An entity that has another prefab instance inside it is rejected (the status bar says so).
- **Material (scope note):** in this engine a material is not a separate asset file but the `Material` component. Its editing is already in the inspector (color picker, metallic/roughness boxes with ranges, emissive). Added in WP6: **right click on the component title → Copy values / Paste values / Reset to defaults** (for every component; for the whole selection; an override on a prefab part). A preview sphere/material library was **not made** (a library needs a file format; to the M11 list).
- **Bug found and fixed:** the periodic project poll (2 s) updated the file/prefab/problem counts but that frame was not redrawn (a new file showed only after an interaction). `refresh_project` now returns "did what is shown change".
- **Tests:** `wish_editor/tests/assets.rs` (9: list+filter+thumbnail, detail, double click placement, dragging and dropping on the view, dropping on a field, move + the user is updated, Make prefab + undo, edit the prefab in the editor + the instance follows, copy/paste/reset); `wish_control/tests/m7_tools.rs` (create/edit prefab/instance/override/undo); `scene_tools.rs` (a prefab is now editable). Total 41 tests in the editor, release+debug.

### WP7 — play, timeline, editing in a live session

- **Control plane (carried over from M5): `edit_apply` + `session`** (`wish_control::live`): the operations are applied to the world of that running game — not to the file and its history, and they stay as long as the session lasts. All or nothing (if one fails, it is restored from a snapshot), `dry_run`, the same `location.op` diagnostic. Supported: spawn (with parent), despawn (with children), set, replace, remove, reparent, rename, resource operations. File-specific ones (instantiate, overrides, notes) give `live_edit_unsupported` + a hint. **A session that is making a replay record rejects it** (`live_edit_while_recording`: an edit is not input, so the record would not play the same). Every live edit enters the session log with kind `edit` (`logs_get kinds: ["edit"]`, who changed what). After an edit `GlobalTransform` is updated immediately (the picture/queries do not wait). New diagnostics are in the catalog.
- **Play in the editor:** **Play** in the menu (disabled while a prefab is open) → `sim_start`; **Pause/Resume**, **Step** (one tick while paused), **Stop** (`sim_stop`, says "live changes were not kept"); a `tick N` indicator. Real time is turned into 60 Hz ticks (at most 8 per frame, no race on a slow frame). The view is drawn from the game's **own camera**. The selection frame and selection by clicking come from the session world.
- **Input:** while playing and the pointer is over the view, keys (with the game's names: arrows `Up/Down/Left/Right`), Shift/Ctrl/Alt and mouse buttons (`MouseLeft`...) go to the game. The pointer position goes as `pointer` (the pixel inside the picture). Editor shortcuts (W/E/R, Delete, F2) are off while playing. While typing in a text field the keys do not go to the game.
- **Timeline:** an automatic `sim_snapshot` every 2 s of game time (`t<tick>`, at most 30; the session limit is 32). When you release the slider in the menu, it **rewinds** to that moment (`sim_restore`, paused). The slider value waits until it is released (both click and drag).
- **Editing while playing:** the inspector shows the live values of the session (it reads every 0.25 s), and changes become `edit_apply`+`session` (the status bar says "live: not kept"). Add/delete in the hierarchy is also live. Gizmos are off while playing (the preview was for the scene).
- **The input map editor moved to WP8:** editing `input.json` by hand needs a control plane capability (invariant 4). It will be done together with the project settings (`wish.json`) in a single settings tool.
- **Tests:** `wish_control/tests/m7_tools.rs` (live editing: set/spawn/rename, the world goes on, file and history are not touched, log, all-or-nothing, dry_run, unsupported operation, record rejection); `wish_editor/tests/play.rs` (5: play → box falls → stop, file the same; pause + step; return to the start with the timeline + the box in place; live edit from the inspector while playing, gone after stop; Space and the pointer go to the game). Total 46 tests in the editor, release+debug.

### WP8 — settings, console/problems/performance, code buttons, multilingual interface, polish, Streamable HTTP

- **`project_settings` (26th tool):** the project's name and default scene (`wish.json`) and the input map (`input.json`) are read/changed. The name is 1–80 characters (`invalid_project_name`), the scene must exist, the input map is checked with the engine's own parser (the closest real name to a wrong key name becomes the hint), `dry_run` (`would_change`). 4 parity examples. This way the input map editor obeys invariant 4 (the agent uses the same tool too).
- **Editor:** a project menu (the project's name: Settings..., Check, Edit code, Build code, Language, Exit) — the crowded bar had overflowed. A settings window (name, default scene, actions/types/keys; add/remove; save = a tool call). Bottom panel tabs: Assets, **Console** (`logs_get`, in order, Clear), **Problems** (`check`; clicking selects the entity of the problem; the count follows the project's error count), **Performance** (per-frame interface/scene picture/game times; a 16.7 ms line — the editor's own time, not the simulation). **Edit code** opens `scripts/src/lib.rs` outside (`WISH_EDITOR` or the system's program; a replaceable `Opener` in tests), **Build code** calls `script_build` and shows its problems in Problems. "Open outside" in the asset browser.
- **Multilingual interface (`wish_editor::i18n`):** every text in the code goes through `t("...")`/`tf("...", names)`, and the key is the English text itself. Each language is one file (`crates/wish_editor/lang/<code>.json`; a translator works without knowing Rust, `lang/README.md`). 12 languages: English, Turkish, German, French, Spanish, Portuguese, Italian, Dutch, Polish, Russian, Ukrainian, Greek. A missing text shows in English. `tests/languages.rs` checks the missing/extra items of every file, that the `{name}` placeholders match, that the font has every letter, and that the menu bar fits in 1280 px in every language. The language is per thread (tests running side by side do not break each other). Choice: project menu → Language. It is stored in the person's preferences (`WISH_CONFIG_DIR`, `%APPDATA%\Wish\editor.json`, `~/.config/wish/editor.json`). Also `WISH_LANG`, the system's `LANG`, `wen edit --lang` (`unknown_language`). A headless picture (`--headless-frame`) depends only on its arguments (the preferences file is not read). The engine's diagnostic messages stay English (an M11 note).
- **Polish:** a scene that cannot be shown (for example the types of game code that was not built in a freshly cloned project) shows a card in the view with the reason, the hint and the "Build code"/"Show problems" buttons — before, the view was empty and Problems was (0) (found in the copy of the showcase game). The status bar is one line (all of it on hover). Shortcuts: Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z, Ctrl+P (not while typing). **Bug fixed:** when the window was resized it was not redrawn until something else happened. In a narrow window the menu bar goes to a second row. View names are translated (the tool name on hover).
- **Streamable HTTP (`wish_mcp::http`, carried over from M4):** `wen mcp --http <port or address>`. One endpoint `/mcp`, `POST` (request → `200` JSON, notification/response → `202`), `DELETE` ends the session, `GET` → `405` (server stream is M11). `Mcp-Session-Id` in the `initialize` response. After that, `400` if it is missing, `404` if it is unknown/ended. Security (the specification's recommendations): default `127.0.0.1`. A non-loopback address without a token is rejected (`http_needs_token`; `WISH_MCP_TOKEN` → `Authorization: Bearer`). `403` if `Origin` is another site (DNS rebinding; `WISH_MCP_ORIGINS`), a `MCP-Protocol-Version` check, body 8 MiB / headers 64 KiB, 30 s timeout, at most 64 connections, chunked transfer `501`. No tokio: `std::net`, a thread per connection. Calls run in order on the workspace's thread. `notifications/cancelled` is handled right away on the connection thread (the shared `Busy` with stdio). Tests `wish_mcp/tests/http.rs` (4: session lifecycle, reasoned rejections, cancel from another connection, token) + unit tests. `wen mcp --http` was tried with curl. The HTTP trial with MCP Inspector was not done (Inspector had been deleted for disk; reinstalling needs a download).
- **Tests:** in the editor `settings.rs` (2), `panels.rs` (5), `languages.rs` (4), `polish.rs` (3) + i18n unit tests. Total editor tests 70 (release).

### WP9 — parity check, the "manual only" exit scenario, closing review

- **Parity table (`wish_editor::parity::PARITY`) and its test (`tests/parity.rs`):** one row for every tool in the catalog, every operation of `edit_apply` (`wish_control::ops::OPS`), `edit_apply` + `session`, and the list/undo/redo actions of `edit_history`: where the person does this in the editor and the name of the test that does it by hand (`file::function`). The test breaks if a new tool or operation is added without a row. It looks at the source to check that each row's test really exists. 30 operations + 12 queries (queries — `scene_get`, `describe_view`, `physics_query`... — are met by what the editor shows).
- **Gaps found in the check and how they were closed** (invariant 9): removing an override on a prefab (`remove_override` → "Revert to prefab" in the inspector; `entity_get` now tells the `overridden` components of a part), `replace_override` on a part ("Paste values" now replaces the component; there is no "Reset to defaults" on a part), entity notes (`set_notes` → "Notes" above the inspector), **scene resources** (`set/replace/remove_resource` → when nothing is selected the inspector shows the scene's resources; "Add resource...", live while playing), placeholder files (`asset_create` → "New..." in the browser: jump/coin/thud/beep sound, circle/square/diamond picture), the history list (`edit_history list` → a **History** tab in the bottom panel: who, what, how many changes; Undo/Redo), `test_run` ("Run tests" in the project menu — failures in Problems — and "Update golden images"), `replay` (a **Record** switch in the menu bar → play/stop saves the record as `replays/take_N`; the "Replays" list in the project menu → plays it back and says "the game plays the same" or the first diverging tick), `capture_frame` ("Save picture" → `captures/<scene>_N.png`). Their tests are `tests/parity_ops.rs` (8).
- **Exit criterion 1, end to end (`tests/manual_only.rs`):** a copy of the showcase game is opened as if freshly cloned (no built code). From the card in the view, **Build code**. A new island is added (+, name with F2, Primitive and Collider with "Add component", scale and position from the inspector). A **prefab** is made from it, and a second instance is placed from the browser and moved. **Ctrl+Z / Ctrl+Y**. It is played with **Record** + Ctrl+P, stopped, and the record is **played back** ("the game plays the same"). **Run tests**: because the new island makes the scene bigger, the sun's shadow map covers a wider area and the shadows of the win picture shift a little (1 test remains, the hint is "look at the picture") → the person does **Update golden images** → 4/4 pass. **Save picture**. **Check** → no problems. Every record in the history is `human`. The test itself calls no changing tool (only for reading). About 11 s in release.
- **Finding (engine behavior, documented):** because the single shadow map is fitted to the scene, adding a far-away object changes the sharpness of the visible shadows (an M3 WP5 decision). Cascaded shadow maps are in the advanced rendering work in M11.
- **Independent closing review (3 agents: editor, control plane, HTTP) — 35 findings, all addressed.** Regression tests: `wish_control/tests/review_m8a.rs` (5), `wish_editor/tests/review_m8a.rs` (7), `wish_mcp/tests/http.rs` (+5).
  - *Control plane:*
    - Prefab editing is now checked with the prefab rules (`prefab_edit_invalid`): the root cannot be deleted or renamed; an entity without a parent, a resource and an instance cannot be added.
    - A prefab entity that a scene overrides cannot be deleted or renamed (`prefab_override_orphaned`; it says which scene and instance). Undo/redo also pass the same check (`commit_document`).
    - Live editing rejects `SceneId` and `GlobalTransform` and a given `order`.
    - `project_settings` keeps the default scene with its spelling on disk (`path_case_mismatch`; the default scene also passes `exact_case`).
    - `project_settings` writes `input.json` first, and if `wish.json` cannot be written it puts it back. It writes the input map in its own canonical form. With a broken `input.json`, `get` still shows the project's name (`input_problem`).
    - A scene that was broken from outside and then fixed by hand no longer rejects every edit.
  - *Editor:*
    - Ids made from a file name follow the scene rule: lowercase, starts with a letter, at most 56 characters + `_N`.
    - While playing, Undo/Redo, switching scenes and "Make prefab" are disabled (they concern files).
    - When the scene changes, the inspector and gizmo caches are reset; the resources of another scene are not shown.
    - Notes go to the entity they were written for.
    - A gizmo drag sends the revision it started at as `base_revision`; a change the agent made in between is not overwritten.
    - When you rewind, the abandoned part of the timeline is deleted; labels are not repeated.
    - A comma can be typed in the key list.
    - When a problem in another file is clicked, that file is opened.
    - F2 and Delete do nothing on a prefab part.
    - After a failed move the selection stays in place.
    - The texture releases of passes that were not edited are not lost.
    - Badges, degrees and slot names are translated.
    - An entity created in the game after Stop does not stay selected.
    - Left for later: the starting revision of a number drag in the inspector.
  - *HTTP:*
    - Every session works with its own client name (`actor`) and protocol version; with two clients connected, the history now writes the right person.
    - Cancellations are keyed by (session, id) and do not go stale: a late cancel of a finished call does not stop the next call with the same id; a session cannot stop another's call.
    - The timeout is per request; slow-sending clients cannot hold the 64 slots.
    - Headers are checked before the body is read; the body is not copied.
    - The pending call limit is 16; cancellations and notifications always pass, and notifications get `202` right away.
    - `Host` check (DNS rebinding).
    - The token is compared in constant time; `Bearer` is case-insensitive.
    - A double `Content-Length` or `Host` is rejected; the header scan is linear.
    - The connection slot is given back on a panic too.
    - Session ids are random; at most 32 sessions.
    - A batch message gets `400`; `--http` is only for `wen mcp`.
    - **Conformance with ADR-022:** `wen mcp --http` now always needs a token. If `WISH_MCP_TOKEN` is missing, it makes a random token and shows it at start.
    - No direct connection from a browser (CORS): an M11 note.

## 9. M8a result (17 November 2026)

**Exit criteria:**
1. *Without AI, only by hand: a part of the showcase game is built, played and prepared for export.* ✅ Evidence `wish_editor/tests/manual_only.rs` (WP9):
   1. the unbuilt copy is opened, Build code;
   2. a new island, its prefab and a second instance are added;
   3. Ctrl+Z / Ctrl+Y;
   4. it is played while recording and the record is played back ("the game plays the same");
   5. the tests run, the golden image is updated: 4/4;
   6. the picture is saved; Check: "no problems".

   The whole history is `human`. The export itself (`wen export`) is in M9.
2. *Parity check (automatic list, test).* ✅ `wish_editor::parity::PARITY` + `tests/parity.rs`: the 26 tools of the catalog, 15 edit operations, live editing and history actions. Every row has a test that does it by hand. A new tool or operation cannot be added without a row.
3. *Every action is a control plane operation; undo/redo works on every action.* ✅
   - Every editor action is a `Workspace::call` (`human`).
   - Reading files directly is only for display: the replay list, thumbnails, empty name checks, the preferences file.
   - A drag is one record in the history.
   - Undo/redo works from the menu, with the shortcut and in the History tab (disabled while playing).
4. *A component newly defined in Wasm is edited in the inspector without extra interface code.* ✅ `tests/inspector.rs::a_component_defined_in_game_code_is_edited_without_editor_code` (WP4).

**Numbers:**
- Editor tests: 22 files, release and debug.
- The full gate is clean: fmt, clippy `-D warnings`, `cargo test --workspace`.
- Interface in 12 languages.
- MCP: 26 tools; stdio + Streamable HTTP.

**Left open (for the user or later work):**
- CI does not run because of the GitHub billing problem: only verified locally, M8a was not verified on Linux.
- The HTTP trial with MCP Inspector was not done (Inspector must be reinstalled; it needs a download).
- A manual trial in the window loop (`wen edit`).
- The starting revision of a number drag in the inspector.
- HTTP directly from a browser (CORS) and the server stream (M11).
