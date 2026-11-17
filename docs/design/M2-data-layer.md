# M2 Design Note — Data Layer

Scope: `wish_scene` (scene and prefab documents, canonical format, validation, operations, undo), `wish_asset` (asset files, watching, `*.meta.json`), `wen check / fmt / mv / migrate`.
Exit criteria: the M2 section of [ROADMAP.md](../ROADMAP.md). Related decisions: ADR-007 (file formats), ADR-008 (files are the source of truth), ADR-018 (hybrid: one history, `actor`).

This note fixes **how** M2 is built. As in M1: a decision that turns out wrong during implementation is corrected here first, then in the code.

## 1. Core decision: the document and the world are separate

```
 DISK                        MEMORY
 main.scene.json  ⇄  SceneDocument (revision N, undo stack)   ← editing: agent, editor, file
                          │ instantiate (one way)
                          ▼
                       World  (play session: deterministic, does not write to disk)
```

- **`SceneDocument`** is the editable form of the scene: entities are kept by **scene id** (`"player"`), components as **JSON values**. Type information comes from `wish_ecs::Registry` (from the schemas); the document does not contain a `World`.
- **Why do we not edit in the ECS world?** The world is for run time: entity handles are unstable, there are change ticks and events, and the order depends on the archetype layout. The document is what humans and agents see, diff and undo. The canonical format is defined on the document.
- **Transfer to the world is one way:** `scene.instantiate(&mut world) -> SceneMap` (scene id ⇄ `Entity`). Taking a value in a play session back into the document is an explicit operation (in M4, `edit_apply` + reading from the session).

## 2. File format (`*.scene.json`, format 1)

```json
{
  "format": 1,
  "entities": {
    "camera": {
      "parent": "player",
      "components": {
        "Camera": { "fov_y_deg": 60 },
        "Transform": { "translation": [0, 2, 6] }
      }
    },
    "player": {
      "components": {
        "Transform": { "translation": [0, 1, 0] }
      },
      "notes": "Moved by the input system."
    }
  },
  "resources": {
    "Gravity": { "value": [0, -9.81, 0] }
  }
}
```

| Rule | Why |
|---|---|
| Entities are an **object keyed by id**, not an array | Patches and diffs are addressed by id, not by index (ADR-007) |
| Scene id: `[a-z0-9_]`, starts with a letter, ≤ 64 characters | Used without escaping in file paths, JSON pointers and on the command line |
| `parent` is the only source of structure | A child list is not kept separately; two sources of truth would not stay in sync |
| Sibling order: `order` (integer, default 0), ties broken by id | Id order is enough in most scenes; when order matters, `order` is written explicitly |
| Component values are in **patch** form: only fields that differ from the default are written | Files stay short; if a default changes, the scene gets the current default |
| `notes` (optional text) | JSON does not support comments; a field for explanation for agents and humans |
| `resources` section | Scene-specific resource values (gravity etc.) |
| The hierarchy (`Hierarchy`) is **not written** in the file | It is a managed component; derived from `parent` + `order` |
| Unknown top-level key → diagnostic | A "silently ignored field" is the worst kind of error for an agent |

### Canonical format (`wen fmt`)

- 2-space indent, LF line endings, a single newline at the end of the file, UTF-8 (no BOM).
- Object keys in **byte order** (at the top level too: `entities`, `format`, `resources`).
- Numbers: integers without a decimal point; fractional numbers by the type in the schema (`f32` fields with the shortest `f32` form — the `to_json` rule from M1).
- Empty objects are `{}`; array items are on a single line if all are numbers (`[0, 1, 0]`), otherwise one per line.
- **Invariant:** loading a canonical file and saving it gives a byte-identical file. A non-canonical file can be loaded; `wen fmt` makes it canonical, `wen check` gives a warning.

We write our own JSON writer (the formatter of `serde_json` does not support the array rule and the number rule).

### Located diagnostics

`wen check` gives **file + line + column + JSON pointer**, like a compiler. `serde_json` gives line/column for syntax errors but not for semantic errors (unknown field, out of range). **Decision:** we write a small JSON parser (`wish_scene::json`) that records the source span of every value (JSON pointer → line/column). The `apply_json` diagnostics of M1 already carry a JSON pointer; the pointer is converted to line/column from this table. The parser is strict (duplicate key → diagnostic; `serde_json` would silently take the last one).

## 3. Prefabs (`*.prefab.json`)

- A prefab is a scene fragment whose root has the id `"root"` (the same format, no `resources`).
- An instance in a scene: `{ "prefab": "prefabs/enemy.prefab.json", "components": { … } }`. `components` is a **patch on top of** the prefab root.
- The non-root entities of the prefab are opened under the instance as `instance_id/prefab_id` (`enemy_1/weapon`). Patches can be written in the scene for these derived ids: `"overrides": { "weapon": { "components": { … } } }`.
- **M2 scope:** a single level of prefab. A nested prefab (a prefab inside a prefab) is rejected with a diagnostic in M2; the need is measured in M7 (the showcase game).
- A prefab cycle and a missing prefab file produce a located diagnostic.

## 4. Operations, revisions, undo

```rust
pub enum EditOp {
    Spawn { id, parent: Option<SceneId>, components: Map<String, Value> },
    Despawn { id },                         // with the subtree
    Set { id, component, value },           // patch (M1 apply_json meaning)
    Remove { id, component },
    Reparent { id, parent: Option<SceneId>, order: Option<i64> },
    Rename { id, to },                      // parents that refer to it are updated too
    Instantiate { id, prefab, parent },
    SetResource { name, value },
}
pub struct Transaction { pub actor: Actor, pub base_revision: Option<u64>, pub ops: Vec<EditOp> }
```

- **Atomic:** the whole transaction is applied to a copy and validated; if even a single operation is rejected, the document does not change, and the diagnostic points to that operation with `location.op`.
- **Optimistic concurrency:** if `base_revision` is given and the document changed in the meantime, a `stale_revision` diagnostic (with the current revision).
- **Undo:** every successful transaction enters the stack together with a transaction made of inverse operations. Undo/redo are transactions too (the revision goes up). The stack depth is limited (default 1000).
- **`actor`** (`Human` | `Agent(String)` | `File`): who did it. The basis of the hybrid parity invariant (ADR-018); the activity feed will show this in M8b.
- **Diff:** every transaction returns the list of JSON pointers that changed (the `diff` field of the `edit_apply` reply in M4).
- Direct editing of the file (ADR-008): the file watcher re-reads the file and records the difference between the new document and the old one in the history as an **`actor: File`** transaction. Invalid file → diagnostic; the last valid document in memory is kept.

## 5. `wish_asset` (M2 scope)

- **An asset reference** is a path relative to the project root + an optional sub-object: `"assets/models/crate.glb#Mesh0"`. An **`AssetRef`** type is added to the M1 schema types (a handle in the component, like `Text`; it points into the world's text table). Since it is a separate type in the schema, the JSON Schema and validation carry the information "this is a file path".
- **Asset server (M2):** file discovery, reading/writing `*.meta.json`, change watching (`notify`), reference validation ("file not found" → located diagnostic + a similar-file suggestion). **Import (glTF, texture) is in M3.**
- **`wen mv <old> <new>`:** moves the file and updates all scene/prefab references in the project in the same operation.
- **Determinism of file watching:** watcher events do not enter the simulation directly; they are queued as document-level operations.

## 6. `SceneDocument` → `World`

- One `Entity` for every scene entity; type names are resolved from the registry, values are written with M1 `set_json`. Unknown component name → already a diagnostic when the document is loaded.
- The hierarchy is built from `parent` + `order` (`set_parent`, with sibling order).
- **Scene id in the world:** a managed `SceneId { id: Text }` component. The agent can address `"player"` in a play session too; `entity_query(name)` uses this.
- **The transfer is deterministic:** entities are created parent first, siblings in order, roots in id order. The same document → the same world hash (test).

## 7. Command line

| Command | What it does |
|---|---|
| `wen check [paths…] [--json]` | The project (or the given files): syntax, schema, references, canonical format. Diagnostics with file/line/column/pointer |
| `wen fmt [paths…] [--check]` | Makes files canonical; `--check` only reports (for CI) |
| `wen mv <old> <new>` | Moves an asset/scene file, updates references |
| `wen migrate [paths…]` | Upgrades old `format` versions (in M2 only the infrastructure; format 1 is the only version) |
| `wen run <scene>` | Loads the scene, runs it with no systems (systems will come from Wasm in M5); reports the hash |

All of them give output in the MCP envelope with `--json`, as in M1.

## 8. Inherited from M1

- Compile-error tests (`trybuild`) and the repeated error messages of the macro for a field that is not plain data.
- The move to `syn` 3 (macro).
- ~~The first real run of CI~~ — done on 15 November 2026: a private GitHub repository was opened, and CI is green on Windows and Linux.

## 9. Test plan (with the exit criteria)

| Exit criterion | Test |
|---|---|
| Load → save is byte-identical | Example scenes + property test: random document → write canonical → read → write = same bytes |
| Each of 30 broken files gets a located diagnostic with a hint | `tests/broken/*.scene.json` + golden files of the expected diagnostic (code, line, column, pointer) |
| When a file changes, the world updates in < 1 s; an invalid file does not corrupt the last valid state | Watcher test (temporary folder) |
| 100 random operations + 100 undos = the starting file | Property test (model-based) |
| The same document → the same world hash | Transfer test |

## 10. Work packages and recommended model

| # | Work package | Model · effort |
|---|---|---|
| WP1 | `wish_scene::json`: a strict parser that records spans + a canonical writer | Sonnet 5.5 · high — **done** |
| WP2 | `SceneDocument`: loading, validation (schema, id, parent, cycle), located diagnostics, 30 broken files test | Sonnet 5.5 · high — **done** |
| WP3 | Operations, revisions, undo/redo, `actor`, diff; model-based test | Opus 5.5 · high — **done** |
| WP4 | Prefabs (single level, patches, derived ids) | Opus 5.5 · high — **done** |
| WP5 | `SceneId`, document → world transfer, `wen run <scene>` | Sonnet 5.5 · medium — **done** |
| WP6 | `wish_asset`: the `AssetRef` schema type, discovery, `*.meta.json`, `wen mv` (watching to WP7) | Sonnet 5.5 · high — **done** |
| WP7 | `wen check / fmt / migrate`, hot reload with the file watcher | Sonnet 5.5 · medium — **done** |
| WP8 | M1 carry-overs (`trybuild`, `syn` 3), closing review | Opus 5.5 · high — **done** |

## 11. Open questions

1. **`order` or a child list?** `parent` + `order` was chosen (single source). If sorting by drag and drop in the editor needs many `order` renumberings, it is reconsidered in M8a.
2. **The rule of writing only the fields that differ from the default** silently changes the meaning of the scene when a default changes. This is a deliberate choice (the wanted behavior for prefabs). If it is not wanted, `wen fmt --explicit` writes all the fields.
3. **Number format:** component values are formatted by the schema, but there is no free-form JSON other than `notes`; if a number without a schema appears (e.g. an unknown component), a diagnostic is given, so no format problem arises.

## 12. Implementation notes

### WP1 — `wish_scene::json`

- **Tree:** `parse(source, file) -> Node`. Every `Node` carries the `Span` where its value starts (line, column; from 1, in **characters**); object members also carry the position of the key. `Node::locate(pointer)` converts a JSON pointer (the schema diagnostics of M1 carry these) into `Located { key, value }`: the two are separate so that an unknown field can point to the key and an out-of-range number can point to the value.
- **Strictness:** a duplicate key is `json_duplicate_key`; a trailing comma, a comment, single quotes, a leading zero, `NaN`/`Infinity`, a raw control character, an invalid escape, a lone surrogate half, and extra content at the end of the file are each a separate located diagnostic that carries a hint. A BOM is read but the file is not counted as canonical. The nesting limit is 128 (both when reading and when writing).
- **Canonical writer:** `to_canonical(&Value) -> Result<String>`. Keys are sorted **explicitly** in UTF-8 byte order (we did not rely on the order of `serde_json::Map`, because it depends on a feature flag). Arrays made only of numbers are on a single line; empty `[]`/`{}`; in strings only `"`, `\` and control characters are escaped. Numbers use the shortest form of `serde_json::Number` (`1e2` → `100.0`, `0.10` → `0.1`). `is_canonical(source)`: byte-level equality.
- **Verification:** random `Value` → canonical → read → the same value and the same bytes (a fixed point); every position really points to the first character of that value; on random text the parser does not crash, and the error always carries a position and a hint.
- **The bug the test caught:** when a high surrogate half came with an invalid second half, the combining arithmetic overflowed (a crash on malicious input). The arithmetic was moved to after the range is validated; two examples were added as tests.
- **Error hints must be sufficient on their own:** the tests force the hint to contain the relevant term (e.g. "surrogate", "four"); the message and the hint cannot stand in for each other.

### WP2 — `SceneDocument` and validation

- **`load(source, file, &Registry) -> LoadResult { document, diagnostics }`.** On a syntax error it stops at the first error (the parser is strict); after the syntax passes, it collects **all** semantic problems and gives them in file order: so that `wen check` can show everything in a single run. The document is returned only if there is no error; warnings do not block loading.
- **Every diagnostic carries file + line + column + JSON pointer.** Schema diagnostics (`from_json` of M1) give their own pointer; `Node::locate` converts it to a position. `unknown_field` points to the key, the others (range, type, length, unknown variant) point to the value.
- **New diagnostic codes:** `missing_format`, `unsupported_format`, `unknown_key` (a top-level or an entity key; with "did you mean"), `invalid_scene_id`, `unknown_parent`, `hierarchy_cycle` (the cycle once, at the `parent` value of the smallest id; the message writes the cycle as `a -> b -> c -> a`), `unknown_component` (the candidates do not include managed components), `managed_component` (`Hierarchy` cannot be written in a file), `unknown_resource`, `not_canonical` (warning). The existing `type_mismatch` and schema codes are used as they are.
- **Component values are stored as JSON, as they are in the file** (patch form; defaults are not added). For validation, a copy is checked with `from_json`; texts are written into a table that is thrown away.
- **The canonical format is a fixed point at the document level too:** `to_canonical` does not write empty sections (`entities`, `resources`, empty `components`, `order` of 0). The `not_canonical` warning is a comparison "source = the canonical text produced from the document"; a file that writes empty sections explicitly gets a warning too. Tests: a loose file → warning + document; the canonical text that was written → zero diagnostics and the same bytes.
- **Test:** 35 broken files (each with a single mistake; the expected code, line, column and the keyword in the hint); a cycle + 3 independent errors in the same file give four diagnostics instead of five; pointers; loading with no file path.
- **Deliberate limits:** until WP4, the `prefab`/`overrides` keys give `unknown_key`; scene ids are ASCII only (`[a-z][a-z0-9_]*`, ≤ 64).

### WP3 — operations, undo, activity log

- **Operations (`EditOp`):** `Spawn` (parent, order, components, notes), `Despawn` (with the subtree), `Set` (merge: the named fields change), `Replace` (the full value), `Remove`, `Reparent` (order is optional), `Rename` (the `parent` of the children is updated too), `SetNotes`, `SetResource` / `ReplaceResource` / `RemoveResource`. Added relative to the §4 draft: `Replace` and `ReplaceResource` (needed for the exact inverse in undo; agents can use them too), `SetNotes`. `Instantiate` (prefab) in WP4.
- **The `Set` merge** has the same meaning as `apply_json` of M1: objects merge key by key, arrays and other values are replaced entirely.
- **All or nothing:** the transaction is applied in order to a copy of the document; if any step is rejected, the original document is not touched, and the diagnostic shows the step with `location.op` and the JSON pointer with `location.path`. Component and resource values go through the same schema check as loading (the shared `schema_check` module: the same error gets the same diagnostic).
- **Inverse operations are computed at the time of application**, and undo applies them in reverse. A deleted subtree comes back in full (components, notes, order), parent first; a rename moves the children back too.
- **Revisions:** every successful transaction, undo and redo raises the revision by 1. If `base_revision` does not match, `stale_revision`; an empty transaction gives `empty_transaction`. `preview` gives the diff list without changing anything (`dry_run` in M4).
- **Diff list:** every transaction returns the changed JSON pointers in order.
- **Who did it:** `Actor::Human` / `Agent(name)` / `File`. The activity log (`log()`, at most 1000) records every edit, undo and redo with the revision, actor and changes: the document-level counterpart of the hybrid parity invariant (one history, `actor`). The undo stack is at most 1000 transactions; a new edit clears the redo stack.
- **Document equality** compares only the content (entities and resources), not the revision and history.
- **New diagnostic codes:** `unknown_entity`, `entity_exists`, `component_not_found`, `resource_not_found`, `stale_revision`, `empty_transaction`.
- **M2 exit criterion met (undo):** a model-based test applies up to 100 random operations in 128 scenarios (some deliberately invalid: out-of-range value, cycle, nonexistent component). After every successful step, the document is reloaded from its canonical text with **zero errors**; every rejected step leaves the document byte-identical. Then all of them are undone: the starting file comes back byte for byte. All are redone: the final file comes back byte for byte.

### WP4 — prefabs

- **Prefab library:** `PrefabLibrary` (path → prefab document, sorted). `load_prefab(source, file, &Registry)` runs the scene validator in "prefab" mode; the extra rules are in the same pass, located: `prefab_without_root`, `prefab_root_with_parent`, `prefab_entity_outside_root` (every entity other than the root must have a parent, that is, everything is under the root), `prefab_with_resources`, `nested_prefab` (a single level; §3).
- **The scene document shares the library with `Arc`** (`load_with_prefabs`, `set_prefabs`, `prefabs()`); it is not part of equality. `load` works with an empty library: a scene that has an instance gives `prefab_not_found` ("No prefabs are loaded"). Discovery of prefab files is in WP6 (`wish_asset`), hot reload of a changed prefab in WP7.
- **Instance:** an entity that has a `prefab` key. `components` is a patch on the components of the prefab root, `overrides.<prefab_entity>.components` is a patch on those of the other prefab entities. Patches are validated on their own by the schema: the merge (the `apply_json` meaning of M1) produces a valid patch from two valid patches, so there is no need to check the combined value too.
- **New diagnostic codes (loading):** `prefab_not_found` ("did you mean" for the path), `overrides_without_prefab`, `override_on_root` (changes to the root are written in the instance's own `components`: one place), `unknown_prefab_entity`. A key other than `components` inside an override is `unknown_key`.
- **Decision — an unknown override target is an error, not a warning.** If overrides written for an entity were silently ignored when that entity is deleted or renamed in the prefab, data would be lost quietly (and undo could not write back a target that is no longer valid). Instead, when the scene is loaded, it says which override is invalid and why, with its position; the hint says "rename or delete". Re-validating dependent scenes when a prefab changes and automatic renaming are the job of WP7 (hot reload).
- **Expansion:** `SceneDocument::expand() -> Vec<ExpandedEntity { id, parent, order, components }>`. Instance X becomes the prefab root (components: root ⊕ the instance patch); prefab entity p → `X/p`, its parent is X if the parent is `root`, otherwise `X/<parent>`; components p ⊕ `overrides[p]`. Since scene ids cannot contain `/`, derived ids cannot collide with those in the scene. The order is stable: parent first, siblings in `(order, id)` order, the prefab entities of an instance right after the instance, then the instance's children in the scene. WP5 (document → world) will use this.
- **Operations:** `Instantiate` (id, prefab, parent, order, root patches, overrides, notes), `SetOverride` (merge), `ReplaceOverride`, `RemoveOverride` (an override entry that becomes empty is removed from the file too). The inverse of deleting an instance is `Instantiate` with the full state. `Set/Replace/Remove` on an instance changes only the instance's patch: `Remove` brings back the prefab's value. New codes: `not_an_instance`, `override_not_found`.
- **Test (`tests/prefabs.rs`, 10 tests):** expansion and derived ids; the canonical round trip (zero diagnostics, the same bytes); 5 prefab rules + 8 instance errors (code, line, column, keyword in the hint); operations + undo/redo; 8 rejected operations (the right code, `location.op`, document unchanged); a model-based property test (96 scenarios, up to 60 random instantiate/override/delete/rename steps, some deliberately invalid): after every successful step it is reloaded and expanded with zero diagnostics, undoing all gives the starting file and redoing all gives the final file, byte for byte.
- **Deliberate limits (WP4):** no nested prefabs (§3: the need is measured in M7), so a prefab cycle cannot form either. An entity in the scene cannot be attached under a derived entity (`enemy_1/weapon`); it can be attached only to the instance itself. There is no "unpack" (turning an instance into ordinary entities). If a prefab document is touched with operations, the prefab rules are checked only when the file is reloaded (prefab edit mode in M8a).

### WP5 — document → world, `SceneId`, `wen run --scene`

- **`SceneDocument::instantiate(&mut World) -> SceneEntities`** (`wish_scene::instantiate`): for every entity in the `expand()` output, an `Entity`, `SceneId { id: Text }`, components with `set_json`, then the hierarchy (`World::set_parent`; since parents come first and siblings are added in `order`, the sibling order is kept), then the scene resources. The returned map goes from scene id (derived `enemy_1/weapon` included) to `Entity`. The transfer is not transactional (on a rejected value the world can stay partly filled); a document that was `load`ed with the same registry must not fail here.
- **Decision — `SceneId` is a reserved component, not a `managed` one.** Design §6 said managed; but the managed flag would have needed a new write/registration path inside `wish_ecs` (the crate with unsafe, Miri). Instead, `wish_scene` registers it as an ordinary component (`instantiate::register`) and `schema_check::component` rejects writing it in a scene file with the `managed_component` code; it is also removed from the candidate list. For the user the result is the same: it cannot be written in a file, and every entity gets it from its key. `wish_ecs` was not touched.
- **`wen run --example <name> --scene <file> [--root <folder>]`:** with `--scene`, the example's own entities and systems are not loaded; the example provides only the **component/resource types** (`wish_examples::register_types`), because there are no game modules (Wasm) until M5. All the `*.prefab.json` files under `--root` (default: the current folder) are loaded into the library, sorted by path, with project-relative paths using `/`; the scene path is relative to it too. `scene`, `scene_entities` and `warnings` (e.g. `not_canonical`) are added to the output.
- **Error behavior:** if there is an error in a scene or prefab file, `wen run` returns the first error (file, line, column, hint); a note "N more problems" is added to the hint (seeing all of them at once comes with `wen check` in WP7). Unreadable file: `file_unreadable`.
- **Test:** `tests/instantiate.rs` (3): ids/hierarchy/sibling order/overrides/resource values, the same document → the same world hash (different from an empty scene), rejecting `SceneId` in a file. `wish_cli/tests/wen.rs` (+2): running a scene + prefab in a temporary project folder (two runs, the same hash), a located error and a missing file.
- **Deliberate limit:** the prefab children of an instance and its scene children are not mixed with `order` under the same parent; the prefab entities come first, then the scene children.

### WP6 — `AssetRef` and `wish_asset`

- **`AssetRef` is a schema type** (`TypeKind::AssetRef`, fingerprint tag 23; the existing tags did not change, the recorded hashes held). Storage is the same as `Text` (a 4-byte handle into the text table; `wish_core::AssetRef` is `repr(transparent)` over `Text`, an empty path = "no asset"), but when entering JSON the path form is checked (`invalid_asset_ref`, with a hint): if not empty, it must be relative, `/`-separated, with no `.`/`..`/empty part, no backslash, no drive letter and no control character, at most one `#sub_object`, ≤ 512 bytes. The JSON Schema output carries `"x-asset": true`, so tools and agents can offer a file picker/validation. `contains_text` and the world text count cover `AssetRef` too (the same table).
- **Decision — watching to WP7:** `notify` is a new external dependency; since it will be used together with hot reload (WP7) anyway, it will be added there and its license checked. WP6 scope: discovery, meta, reference checking, `wen mv`.
- **`wish_asset`:** depends on `wish_core`, `wish_ecs`, `wish_scene`, `wish_schema` (a layer on top of scene documents; `wish_scene` does not know about asset files).
  - `AssetIndex::scan(root)`: all files other than hidden folders (starting with `.`) and engine documents (`*.scene.json`, `*.prefab.json`, `*.meta.json`), sorted. `check(reference)`: form → `invalid_asset_ref`, file missing → `asset_not_found` ("did you mean" or an empty-project hint). `#sub_object` is not checked: its content is known only after import (M3).
  - `*.meta.json`: `{ "format": 1, "import": { … } }`, parsing with positions and hints (`parse_meta`), canonical writing, `read_meta`/`write_meta`. The content of `import` will be defined by the importers (M3); for now it is kept as it is. An asset with no meta file uses the default settings.
  - Finding references is done **by the schema**, not by string similarity: in scene/prefab JSON, entity components, the `overrides` components of instances and `resources` are walked by the component schema (struct fields, arrays). `check_source` gives every bad reference with file/line/column and JSON pointer; a syntax error returns as a single diagnostic.
  - **`wen mv <old> <new> --example <name> [--root] [--dry-run]`:** moves a file or a folder; updates the references in all scene/prefab documents (`#sub_object` is kept, folder prefixes are mapped, a folder with a similar prefix is not affected) in the same operation; the file's `.meta.json` goes with it. Rejected: a nonexistent source (`asset_not_found`, with a suggestion), an existing target (`asset_exists`, nothing is overwritten), moving a folder into itself, the same path, an invalid path, a document with a syntax error (it stops at the planning stage, nothing is moved). **All or nothing:** first a plan (does not touch the disk), then the application; if a step fails, what was done is undone (tested with a read-only scene). Changed documents are rewritten as canonical JSON (key order and layout; the meaning does not change). Document-level editing like dropping `"order": 0` is the job of `wen fmt`. (After the WP8 review, prefab references, folders that contain documents, and letter case were also covered; see WP8.)
  - `--example` is required because the component types (which field is an `AssetRef`) come from there; when game modules (M5) arrive, the project will bring its own types. Only a scene-only registered `Model { mesh, textures }` component was added to the `stress` example (it does not enter the benchmark world/hashes).
- **Miri:** in WP6 the `wish_ecs`/`Plain` declaration was touched; `cargo +nightly miri test -p wish_ecs` is clean.
- **Hybrid parity:** `wen mv` is a file-level human operation (when the scene document is not open); connecting it to the operation history (`actor`) of open documents comes with the editor (M8a) and hot reload (WP7).
- **Test:** `wish_asset/tests/assets.rs` (9): schema and form checking (`set_json` rejection, `x-asset`), index, meta (round trip + 5 error positions), finding references by schema, a located missing file, moving a file (+ meta, + all references resolve after the move), moving a folder, 6 rejected moves, undo. `wen mv` CLI test (+1). `wish_core::asset` unit tests (3).

### WP7 — `wen check / fmt / migrate / watch`, hot reload

- **Decision — the watcher uses polling, without `notify`.** `Watcher::poll()` compares the project folder (hidden folders excluded) by modification time + size and gives the added/changed/deleted paths. Reason: zero dependencies (no license/download), the same behavior on every platform, cheap at project size; if needed, a native notification backend can be put behind the same `poll()` interface. Limit: a change with the same size and the same timestamp is missed (file system time resolution). It reads the file system clock, and does not touch the simulation clock.
- **`load_project(root, &Registry)` (`wish_asset::project`):** all prefabs (first), scenes and meta files are loaded and checked together; prefab rules, scene validation, existence of asset files and meta format in a single pass; all diagnostics in path order, valid documents and each file's canonical text (`None` if it has errors) are returned. `wen check` and `wen fmt` are built on this.
- **`wen check [files…] --example <name> [--root] [--json]`:** gives all problems with file/line/column/pointer and hint; it exits with failure only if there is an error (a warning, e.g. `not_canonical`, does not count as a failure). The output is `{ok, files, errors, warnings, diagnostics}`. If file names are given, only their diagnostics are reported (the others are still loaded: prefabs are needed).
- **`wen fmt [files…] [--check]`:** loads the document and writes it with `to_canonical` (empty sections are dropped, keys are sorted). It does not touch files with errors, reports them as `skipped` and exits with failure. `--check` changes nothing, lists the non-canonical ones, and exits with failure if there are any (for CI).
- **`wen migrate`:** since format 1 is the only version, this is infrastructure: it counts the up-to-date files, gives `unsupported_format` for a newer one ("update the engine"), `no_migration` for an older one, and `missing_format` if `format` is missing. When format 2 comes, the steps will be added here.
- **Hot reload `HotScene` (`wish_asset::hot`):** `start` loads the project (if it is not valid, it stops with the first error: there is no earlier good state to go back to) and transfers the scene to the world; `poll(&mut World)` reloads the project if a file changed. **If the project is valid**, the entities of the scene are destroyed and the new ones are built (`applied: true`); **if it is invalid**, the world is not touched, the last good scene keeps running, and the problems return with `Reload.diagnostics` (`applied: false`). A missing asset file blocks the reload too. Entities outside the scene are not touched. The world changes only inside `poll`, between ticks.
- **Deliberate limit — the scene is replaced as a whole.** The state that the scene's entities gained while running is lost; a per-entity diff application (writing only the changed components with an id mapping) will be measured and added in M3/M8a. The content of the reloaded world is the same as a fresh load but **the world hash is not the same**: renewed entities raise the generation count of their slots (so that old handles cannot point to the new entities); this is correct behavior, and the test compares the content per entity.
- **`wen watch --example <name> --scene <file> [--root] [--seconds n] [--json]`:** loads the scene and polls every 100 ms; with `--json` every event is a JSON line (`loaded`, `reloaded`, `rejected`, and at the end `{ok, reloads, rejected}`). This is the observable surface of hot reload from the control plane (the event stream in MCP comes in M4). The update delay is at most one polling interval (exit criterion: < 1 s).
- **Exit status:** when `check`/`fmt`/`migrate` find a problem, they print their own output (`ok: false` + `diagnostics`) and exit with failure; the other commands give a single `{ok:false,error}` diagnostic.
- **Test:** `wish_asset/tests/project.rs` (4): a multi-file project with many errors, the watcher, hot reload (valid → applied, invalid → rejected and the hash is the same, missing asset, after the fix the content equals a fresh load), startup errors. `wish_cli/tests/project.rs` (4): `check`, `fmt` (+ `--check`), `migrate`, `watch` (wait for the first `loaded` line, then edit: `loaded → reloaded → rejected`).

### WP8 — M1 carry-overs and the M2 closing

- **Repeated errors from the macro:** for a field that is not plain data, the errors went down from 5 to 2. Every `impl` the macro generates (`Plain`, `Default`, the marker trait) carries a `where FieldType: Plain` clause with the position of the field type; the bodies assume the bound, and the error appears only once, at that position (rustc merges the same message at the same position). The remaining second message is rustc's own note "this field is not `Copy`"; it points to the same field. The empty enum error now shows the name of the enum, not the attribute line.
- **`trybuild` compile-error tests** (`wish_schema/tests/compile_fail.rs`, 13 cases): a field that is not plain data, a tuple struct, a generic type, a union, `derive`/`repr`/`Default` conflicts, a half-open range, an unknown option, a variant with data, an empty enum, an argument, a default of the wrong type. The expected outputs are in the repo as `tests/ui/*.stderr`; after a deliberate change they are regenerated with `TRYBUILD=overwrite` and the diff is reviewed. (Since the list of types that implement `Plain` appears in rustc's help note, adding a new `Plain` type changes this file.)
- **`syn` 3:** the macro crate moved to `syn` 3, no API change was needed; since `serde_derive` already uses 3, `syn` 2 left the dependency tree. `trybuild` (test only) was added; the licenses in its tree are in the allow list.
- **Closing review:** the M2 code was reviewed by an independent reading that does not depend on my decisions (a separate agent, given only the code and the design note). All findings were fixed, and a regression test was added for each:

| # | Finding | Fix |
|---|---|---|
| 1 | `wen mv` did not update prefab references (`"prefab": …`); moving a prefab broke scenes | The move plan maps the `prefab` keys of entities too; tests for moving a prefab file and a prefab folder |
| 2 | Moving a folder that contained a document with references always failed (the document was written to its old path) | Every document is written to its place after the move (`target`); on undo, the empty folders created by the move are deleted too |
| 3 | On a case-insensitive file system (Windows), `LEVEL1/TEX.png` moved the file and said "success" without updating references | The source must exist **byte for byte** in the project list (a file or a folder prefix); the hint mentions letter case too |
| 4 | Hot reload did not reset resources: a resource or field removed from the file stayed at its old value | `HotScene` stores the pre-scene value of every resource the scene touches; on reload it puts that back first (or removes it if there was none), then applies the new patch. `World::remove_resource_json` was added for this |
| 5 | A non-matching file name given to `wen check/fmt/migrate` (a typo, `.\`, a backslash) was silently ignored and passed with "0 errors" | Arguments are normalized (`\` → `/`, a leading `./`); a name that matches no document fails with `file_not_found` + a suggestion |
| 6 | A single unreadable document (not UTF-8, locked) or a folder deleted during the scan stopped the whole `check` and `watch` | An unreadable document becomes a diagnostic tied to that file, the rest is checked; `fmt` puts it in the skipped list; a deleted subfolder is skipped; `watch` reports the error as an `error` event and keeps watching |
| 7 | The folder walker followed symbolic links/junctions (infinite in a loop); `wen run --scene` also loaded prefabs in hidden folders | Links are not followed; `wen run --scene` uses the same walker as `wen check` |
| 8 | `wen mv` accepted moving to a hidden folder or to an extension that changes the kind (`a.png` → `a.scene.json`) | Rejected with `hidden_destination` and `kind_change`; the claim of "canonical rewrite" was corrected in the doc (only at the JSON level) |
| 9 | `Reparent` reported `/parent` even if only the order changed | Whatever changed (`/parent`, `/order` or both) is what is reported |

- **What the review found sound:** the strictness and depth limit of the JSON parser, the canonical text being a fixed point, the correctness of the inverses of undo/redo (subtree, rename, instance and override included), the cycle check, `validate_asset_ref` stopping escape from the project root, the determinism of the `expand`/`instantiate` order, the `where` change of the macro producing no warnings.
- **Miri:** clean for `wish_ecs` after the macro change and `remove_resource_json`.

### M2 exit criteria

| Criterion | Evidence |
|---|---|
| Load → save is byte-identical | `wish_scene/tests/json.rs` property tests (random value → canonical → read → the same bytes), `tests/document.rs` and `tests/prefabs.rs` round trips; `wen check` gives zero diagnostics on a canonical file |
| Each of 30 broken files gets a located diagnostic with a hint | `tests/document.rs`: 35 broken scenes (code, line, column, keyword in the hint); in addition 5 prefab rules + 8 instance errors, meta and asset reference errors |
| When a file changes, the world updates in < 1 s; an invalid file does not corrupt the last valid state | `wish_asset/tests/project.rs` (on an invalid file the world hash stays the same) and the `wen watch` test (100 ms polling; `loaded → reloaded → rejected`) |
| 100 random operations + 100 undos = the starting file | the `tests/edit.rs` model test (128 scenarios, up to 100 operations) and `tests/prefabs.rs` with prefab operations (96 scenarios) |
| The same document → the same world hash | `tests/instantiate.rs`, the `wen run --scene` two-run test |
