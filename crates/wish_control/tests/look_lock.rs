//! The look lock (M12 package 9): named looks applied with `edit_apply`
//! `apply_look`; the recorded defaults of the file format, kept equal to the
//! engine's (ADR-027, rule 1, the guard); and the migration a change of the
//! default look needs, tried with each candidate look as the new defaults on
//! every scene of the repository and on generated ones. Design:
//! `docs/design/M12-look-lock.md`.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_scene::migrate::{Defaults, Kind, Step};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("look-lock-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "L", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn ok(answer: &Value) -> &Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn record_path() -> PathBuf {
    repo().join(format!(
        "crates/wish_scene/src/defaults/format-{}.json",
        wish_scene::FORMAT
    ))
}

fn recorded() -> Defaults {
    Defaults::parse(wish_scene::migrate::FORMAT_1_DEFAULTS).unwrap()
}

// --- Named looks -----------------------------------------------------------

#[test]
fn a_look_is_applied_in_one_step_and_the_answer_says_what_it_set() {
    let (dir, mut ws) = project("apply");
    let me = parse_actor("claude-code");
    let size = json!({ "width": 96, "height": 54 });
    let before = ok(&ws.call("capture_frame", &size, &me)).clone();
    let file_before = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();

    // A dry run says what would change and changes nothing.
    let dry = ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "natural" }], "dry_run": true }),
        &me,
    ))
    .clone();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["changes"]["count"], 6, "{dry}");
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file_before
    );

    let done = ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "natural" }] }),
        &me,
    ))
    .clone();
    let look = wish_control::looks::find("natural").unwrap();
    // The answer has every look resource as it now is: no read-back needed.
    let now = done["now_resources"].as_object().unwrap();
    assert_eq!(
        now.keys().collect::<Vec<_>>(),
        [
            "Ambient",
            "Bloom",
            "ColorGrading",
            "Fog",
            "RenderSettings",
            "Sky"
        ]
    );
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    for (name, fields) in &look.resources {
        for (field, value) in fields {
            assert_eq!(&now[name][field], value, "{name}.{field}");
            // Written out in the file, whatever the defaults are.
            assert_eq!(&file["resources"][name][field], value, "{name}.{field}");
        }
    }
    // Fields a look does not own keep the scene's (and are shown).
    assert_eq!(now["RenderSettings"]["shadow_map_size"], 2048);
    assert_eq!(now["Sky"]["luminance"], 20000.0);

    let after = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(
        after["stats"]["effects"],
        json!([
            "sky_light",
            "sun_disc",
            "fog",
            "bloom",
            "color_grading",
            "fxaa"
        ])
    );
    assert_eq!(after["stats"]["shadow_cascades"], 4);
    // One change of the history, undone in one step.
    ok(&ws.call("edit_history", &json!({ "action": "undo" }), &me));
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file_before
    );
    let undone = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(undone["image"]["data"], before["image"]["data"]);

    // Plain is the picture of the defaults, to the byte.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "plain" }] }),
        &me,
    ));
    let plain = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(plain["image"]["data"], before["image"]["data"]);
    assert_eq!(plain["stats"]["effects"], json!([]));

    // An unknown look is refused at its own index, with the names there are.
    let wrong = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "ground" }, { "op": "apply_look", "look": "vivd" }] }),
        &me,
    );
    assert_eq!(wrong["error"]["code"], "unknown_look", "{wrong}");
    assert_eq!(wrong["error"]["location"]["op"], 1);
    assert_eq!(wrong["error"]["hint"], "Did you mean 'vivid'?");
    // A problem after a look points at the operation that was written, not
    // at the look's sixth resource.
    let late = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "vivid" }, { "op": "despawn", "id": "nobody" }] }),
        &me,
    );
    assert_eq!(late["error"]["code"], "unknown_entity", "{late}");
    assert_eq!(late["error"]["location"]["op"], 1);

    // In a running game it is a live change.
    let session = ok(&ws.call("sim_start", &json!({}), &me))["session"].clone();
    let live = ok(&ws.call(
        "edit_apply",
        &json!({ "session": session, "ops": [{ "op": "apply_look", "look": "vivid" }] }),
        &me,
    ))
    .clone();
    assert_eq!(live["changes"]["count"], 6, "{live}");
}

#[test]
fn every_look_is_a_full_set_of_clean_values_and_plain_is_the_format_1_defaults() {
    let record = recorded();
    let looks = wish_control::looks::looks().unwrap();
    let (_dir, mut ws) = project("clean");
    let me = parse_actor("claude-code");
    for look in &looks {
        for (name, fields) in &look.resources {
            let defaults = record.resources[name].as_object().unwrap();
            for (field, value) in fields {
                assert!(defaults.contains_key(field), "{}: {name}.{field}", look.id);
                // Every number is finite and in its range: the scene check
                // refuses anything else, and the renderer's own cleaning
                // (look::clean) then has nothing to change.
                if let Some(number) = value.as_f64() {
                    assert!(number.is_finite());
                }
            }
        }
        ok(&ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "apply_look", "look": look.id }], "dry_run": true }),
            &me,
        ));
    }
    let plain = looks.iter().find(|look| look.id == "plain").unwrap();
    for (name, fields) in &plain.resources {
        for (field, value) in fields {
            assert_eq!(
                value.as_f64().map_or(value.clone(), |n| json!(n)),
                record.resources[name][field]
                    .as_f64()
                    .map_or(record.resources[name][field].clone(), |n| json!(n)),
                "plain {name}.{field}"
            );
        }
    }
}

// --- The guard: recorded defaults ------------------------------------------

/// Every leaf that differs between `old` and `new` (a value, or a field one
/// of them lacks), as `path: old -> new`.
fn differences(
    old: &Value,
    new: &Value,
    at: &str,
    changed: &mut Vec<String>,
    added: &mut Vec<String>,
) {
    match (old.as_object(), new.as_object()) {
        (Some(old), Some(new)) => {
            for (key, was) in old {
                match new.get(key) {
                    Some(now) => differences(was, now, &format!("{at}/{key}"), changed, added),
                    None => changed.push(format!("{at}/{key}: removed")),
                }
            }
            for key in new.keys().filter(|key| !old.contains_key(*key)) {
                added.push(format!("{at}/{key}"));
            }
        }
        _ if old != new => changed.push(format!("{at}: {old} -> {new}")),
        _ => {}
    }
}

/// ADR-027, rule 1: a default is part of the file format. The defaults of
/// every engine type are recorded for the current format; a change of one
/// fails here, unless it is made in a planned window, with a new format
/// number and a migration step (`docs/design/M12-look-lock.md`). A new type
/// or field is an addition: `UPDATE_DEFAULTS=1` records it.
#[test]
fn the_recorded_defaults_are_the_engines() {
    let now = wish_control::migrate::engine_defaults().unwrap();
    let text = wish_scene::json::to_canonical(&now.to_value(wish_scene::FORMAT)).unwrap();
    let path = record_path();
    let old_text = std::fs::read_to_string(&path).unwrap_or_default();
    if old_text == text {
        return;
    }
    let old: Value = serde_json::from_str(&old_text).unwrap_or(json!({}));
    let new: Value = serde_json::from_str(&text).unwrap();
    let (mut changed, mut added) = (Vec::new(), Vec::new());
    differences(&old, &new, "", &mut changed, &mut added);
    changed.retain(|line| !line.starts_with("/format"));
    assert!(
        changed.is_empty(),
        "defaults of format {} changed outside a format window (ADR-027, rule 1): {changed:#?}\n\
         A default changes only with a new format number and a migration step: see \
         docs/design/M12-look-lock.md, \"The switch\".",
        wish_scene::FORMAT
    );
    if std::env::var_os("UPDATE_DEFAULTS").is_some() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    panic!(
        "new types or fields since the record of format {} (additions keep old files' meaning): \
         {added:#?}\nRecord them with UPDATE_DEFAULTS=1 and look at the diff.",
        wish_scene::FORMAT
    );
}

// --- The migration, tried with each candidate as the new defaults ----------

/// `defaults` with a look's values as the new defaults: what the switch
/// would make them.
fn with_look(defaults: &Defaults, look: &str) -> Defaults {
    let mut out = defaults.clone();
    for (name, fields) in &wish_control::looks::find(look).unwrap().resources {
        let value = out.resources.get_mut(name).unwrap();
        for (field, v) in fields {
            value[field] = v.clone();
        }
    }
    out
}

/// Every default of every type changed (numbers moved, switches flipped,
/// texts and choices renamed): the hardest case for the migration.
fn everything_changed(defaults: &Defaults) -> Defaults {
    fn shift(value: &Value) -> Value {
        match value {
            Value::Number(n) => json!(n.as_f64().unwrap_or(0.0) + 0.25),
            Value::Bool(b) => json!(!b),
            Value::String(s) => json!(format!("{s}x")),
            Value::Array(items) => Value::Array(items.iter().map(shift).collect()),
            Value::Object(fields) => {
                Value::Object(fields.iter().map(|(k, v)| (k.clone(), shift(v))).collect())
            }
            Value::Null => Value::Null,
        }
    }
    Defaults {
        components: defaults
            .components
            .iter()
            .map(|(k, v)| (k.clone(), shift(v)))
            .collect(),
        resources: defaults
            .resources
            .iter()
            .map(|(k, v)| (k.clone(), shift(v)))
            .collect(),
    }
}

fn merged(base: &Value, patch: &Value) -> Value {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            let mut out = base.clone();
            for (key, value) in patch {
                let next = match out.get(key) {
                    Some(old) => merged(old, value),
                    None => value.clone(),
                };
                out.insert(key.clone(), next);
            }
            Value::Object(out)
        }
        (_, patch) => patch.clone(),
    }
}

/// What a file means under `defaults`: every engine resource (a missing one
/// is its defaults, as the engine uses it) and every engine component of
/// every entity and prefab part, all fields filled in.
fn meaning(file: &Value, defaults: &Defaults, prefabs: &BTreeMap<String, Value>) -> Value {
    let mut out = Map::new();
    for (name, base) in &defaults.resources {
        let value = match file.pointer(&format!("/resources/{name}")) {
            Some(patch) => merged(base, patch),
            None => base.clone(),
        };
        out.insert(format!("resource {name}"), value);
    }
    let component = |prefab: Option<&str>, part: &str, name: &str, patch: &Value| {
        let base = defaults.components.get(name)?;
        let from_prefab = prefab.and_then(|prefab| {
            let prefab_file = prefabs.get(prefab)?;
            let inner = prefab_file.pointer(&format!("/entities/{part}/components/{name}"))?;
            Some(meaning_of_prefab_part(inner, base))
        });
        Some(merged(&from_prefab.unwrap_or_else(|| base.clone()), patch))
    };
    if let Some(entities) = file.get("entities").and_then(Value::as_object) {
        for (id, entity) in entities {
            let prefab = entity.get("prefab").and_then(Value::as_str);
            for (name, patch) in entity
                .get("components")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if let Some(value) = component(prefab, "root", name, patch) {
                    out.insert(format!("{id} {name}"), value);
                }
            }
            for (part, patch) in entity
                .get("overrides")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                for (name, patch) in patch
                    .get("components")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                {
                    if let Some(value) = component(prefab, part, name, patch) {
                        out.insert(format!("{id}/{part} {name}"), value);
                    }
                }
            }
        }
    }
    Value::Object(out)
}

fn meaning_of_prefab_part(patch: &Value, base: &Value) -> Value {
    merged(base, patch)
}

/// A small deterministic random sequence for the generated scenes.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Scenes with a random choice of look resources, each with a random choice
/// of fields written (at values other than the defaults), and entities with
/// engine components that leave fields out.
fn generated_scenes(record: &Defaults, count: usize) -> Vec<Value> {
    let mut dice = Dice(0x9e37_79b9_7f4a_7c15);
    let look_resources = [
        "Ambient",
        "Bloom",
        "ColorGrading",
        "Exposure",
        "Fog",
        "RenderSettings",
        "Sky",
    ];
    (0..count)
        .map(|_| {
            let mut resources = Map::new();
            for name in look_resources {
                if !dice.chance(50) {
                    continue;
                }
                let mut patch = Map::new();
                for (field, value) in record.resources[name].as_object().unwrap() {
                    if dice.chance(40) {
                        let written = match value {
                            Value::Number(n) if n.is_f64() => {
                                json!(n.as_f64().unwrap() * 0.5 + 0.001)
                            }
                            other => other.clone(),
                        };
                        patch.insert(field.clone(), written);
                    }
                }
                resources.insert(name.to_owned(), Value::Object(patch));
            }
            let mut entities = Map::new();
            for (n, name) in ["Camera", "DirectionalLight", "Material", "Transform"]
                .iter()
                .enumerate()
            {
                if dice.chance(70) {
                    entities.insert(format!("e{n}"), json!({ "components": { *name: {} } }));
                }
            }
            let mut file = Map::new();
            file.insert("format".to_owned(), json!(1));
            if !entities.is_empty() {
                file.insert("entities".to_owned(), Value::Object(entities));
            }
            if !resources.is_empty() {
                file.insert("resources".to_owned(), Value::Object(resources));
            }
            Value::Object(file)
        })
        .collect()
}

/// Files by name with their kind and JSON, and prefabs by path.
type Corpus = (Vec<(String, Kind, Value)>, BTreeMap<String, Value>);

/// Every scene and prefab file of the repository (the examples), the scene
/// a new project starts with, a prefab and a scene that places it, and
/// generated scenes. Prefabs by their project-relative path.
fn corpus(record: &Defaults) -> Corpus {
    let mut files = Vec::new();
    let mut prefabs = BTreeMap::new();
    let examples = repo().join("examples");
    let mut walk = vec![examples];
    while let Some(dir) = walk.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                walk.push(path);
            } else if name.ends_with(".scene.json") || name.ends_with(".prefab.json") {
                let value =
                    wish_scene::json::parse(&std::fs::read_to_string(&path).unwrap(), &name)
                        .unwrap()
                        .to_value();
                let kind = if name.ends_with(".scene.json") {
                    Kind::Scene
                } else {
                    Kind::Prefab
                };
                files.push((name, kind, value));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(files.len() >= 6, "the examples' scenes: {}", files.len());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("look-lock-corpus");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "C", "wen").unwrap();
    files.push((
        "new project".to_owned(),
        Kind::Scene,
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap(),
    ));
    let lamp = json!({
        "format": 1,
        "entities": {
            "root": { "components": { "Material": { "roughness": 0.2 }, "Primitive": {} } },
            "bulb": { "parent": "root", "components": { "PointLight": { "range": 4.0 } } }
        }
    });
    prefabs.insert("prefabs/lamp.prefab.json".to_owned(), lamp.clone());
    files.push(("prefabs/lamp.prefab.json".to_owned(), Kind::Prefab, lamp));
    files.push((
        "a scene with lamps".to_owned(),
        Kind::Scene,
        json!({
            "format": 1,
            "entities": {
                "lamp_1": { "prefab": "prefabs/lamp.prefab.json" },
                "lamp_2": { "prefab": "prefabs/lamp.prefab.json",
                            "components": { "Material": { "metallic": 1.0 }, "Camera": {} },
                            "overrides": { "bulb": { "components": { "PointLight": { "intensity_lm": 50.0 }, "SpotLight": {} } } } }
            },
            "resources": { "Fog": {}, "Sky": { "zenith": [0.1, 0.2, 0.3, 1.0] } }
        }),
    ));
    for (n, scene) in generated_scenes(record, 64).into_iter().enumerate() {
        files.push((format!("generated {n}"), Kind::Scene, scene));
    }
    (files, prefabs)
}

#[test]
fn a_migrated_file_means_under_the_new_defaults_what_it_meant_under_the_old() {
    let record = recorded();
    let (files, prefabs) = corpus(&record);
    let prefab_has = |prefab: &str, entity: &str, component: &str| {
        prefabs
            .get(prefab)
            .and_then(|file| file.pointer(&format!("/entities/{entity}/components/{component}")))
            .is_some()
    };
    let candidates = [
        ("natural", with_look(&record, "natural")),
        ("vivid", with_look(&record, "vivid")),
        ("plain", with_look(&record, "plain")),
        ("everything changed", everything_changed(&record)),
    ];
    let mut written_total = BTreeMap::new();
    for (label, new) in &candidates {
        let mut written = 0;
        for (name, kind, file) in &files {
            let mut pinned = file.clone();
            written +=
                wish_scene::migrate::pin(&mut pinned, *kind, &record, new, &prefab_has).len();
            // The new prefab files: an instance leans on the migrated prefab.
            let new_prefabs: BTreeMap<String, Value> = prefabs
                .iter()
                .map(|(path, prefab)| {
                    let mut prefab = prefab.clone();
                    wish_scene::migrate::pin(&mut prefab, Kind::Prefab, &record, new, &prefab_has);
                    (path.clone(), prefab)
                })
                .collect();
            let mut now = meaning(&pinned, new, &new_prefabs);
            let mut was = meaning(file, &record, &prefabs);
            if *kind == Kind::Prefab {
                // A prefab file has no resources: only its entities count.
                assert!(pinned.get("resources").is_none(), "{label}: {name}");
                for map in [&mut now, &mut was] {
                    map.as_object_mut()
                        .unwrap()
                        .retain(|key, _| !key.starts_with("resource "));
                }
            }
            assert_eq!(
                now, was,
                "{label}: {name} means something else after the migration: {pinned}"
            );
            // Nothing more to write the second time.
            let mut again = pinned.clone();
            assert!(
                wish_scene::migrate::pin(&mut again, *kind, &record, new, &prefab_has).is_empty(),
                "{label}: {name}"
            );
        }
        written_total.insert(*label, written);
    }
    // Plain is today's defaults: nothing to write, every file as it was.
    assert_eq!(written_total["plain"], 0, "{written_total:?}");
    assert!(
        written_total["natural"] > 0
            && written_total["everything changed"] > written_total["natural"]
    );
    // With no changed default (today), every example's file is left
    // byte for byte as it is.
    for (name, kind, file) in &files {
        if !name.contains("/examples/") {
            continue;
        }
        let mut same = file.clone();
        assert!(
            wish_scene::migrate::pin(&mut same, *kind, &record, &record, &prefab_has).is_empty()
        );
        let source = std::fs::read_to_string(name).unwrap();
        assert_eq!(
            wish_scene::json::to_canonical(&same).unwrap(),
            source,
            "{name}"
        );
    }
}

/// Writes the resources whose default differs between `record` and `new`
/// in full, as they are under `new`: drawn by today's engine (whose defaults
/// are `record`), the file then looks as it would under `new`.
fn as_drawn_under(file: &Value, record: &Defaults, new: &Defaults) -> Value {
    let mut out = file.clone();
    let meaning = meaning(file, new, &BTreeMap::new());
    for (name, value) in &new.resources {
        if record.resources.get(name) != Some(value) {
            out["resources"][name] = meaning[format!("resource {name}")].clone();
        }
    }
    out
}

/// Draws `file` as a new scene file of the project (a new name each time,
/// so that no change is missed).
fn picture(dir: &Path, ws: &mut Workspace, file: &Value) -> Value {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scene = format!("scenes/probe_{n}.scene.json");
    std::fs::write(
        dir.join(&scene),
        wish_scene::json::to_canonical(file).unwrap(),
    )
    .unwrap();
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "capture_frame",
        &json!({ "scene": scene, "width": 160, "height": 90 }),
        &me,
    ))["image"]["data"]
        .clone()
}

/// The proof by picture: each scene drawn as it is today, and drawn as the
/// migrated file would be under a candidate's defaults, gives the same
/// bytes; without the migration the picture changes.
#[test]
fn a_migrated_scene_keeps_its_picture_to_the_byte() {
    let record = recorded();
    let (dir, mut ws) = project("pictures");
    let start: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
            .unwrap();
    let first_pixel: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("examples/first-pixel/scenes/main.scene.json"))
            .unwrap(),
    )
    .unwrap();
    let mut partly = start.clone();
    partly["resources"] = json!({
        "Fog": { "density": 0.0 },
        "RenderSettings": { "shadow_map_size": 1024 },
        "Sky": { "zenith": [0.3, 0.2, 0.6, 1.0] },
        "Ambient": { "intensity": 12000.0 }
    });
    let no_prefabs = |_: &str, _: &str, _: &str| false;
    for look in ["natural", "vivid"] {
        let new = with_look(&record, look);
        for (label, file) in [
            ("new project", &start),
            ("first-pixel", &first_pixel),
            ("partly set", &partly),
        ] {
            let today = picture(&dir, &mut ws, file);
            let mut pinned = file.clone();
            wish_scene::migrate::pin(&mut pinned, Kind::Scene, &record, &new, &no_prefabs);
            let migrated = picture(&dir, &mut ws, &as_drawn_under(&pinned, &record, &new));
            assert_eq!(
                migrated, today,
                "{look}: {label}: the migrated file draws differently"
            );
            let unmigrated = picture(&dir, &mut ws, &as_drawn_under(file, &record, &new));
            assert_ne!(
                unmigrated, today,
                "{look}: {label}: the new defaults change nothing?"
            );
        }
    }
}

/// `wen migrate`'s capability, with a step as the switch would add it: the
/// project's scene files are rewritten in canonical form with the values
/// they relied on, a second run finds them current, and a dry run writes
/// nothing.
#[test]
fn a_project_is_migrated_with_the_switchs_step() {
    let record = recorded();
    let (dir, _ws) = project("migrate");
    let new = with_look(&record, "natural");
    const STEP: Step = Step {
        from: 1,
        defaults: wish_scene::migrate::FORMAT_1_DEFAULTS,
    };
    let scene = dir.join("scenes/main.scene.json");
    let before = std::fs::read_to_string(&scene).unwrap();
    let dry = wish_control::migrate::migrate_project(&dir, &[], &[STEP], 2, &new, false).unwrap();
    assert_eq!(dry.migrated.len(), 1, "{dry:?}");
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), before);
    let done = wish_control::migrate::migrate_project(&dir, &[], &[STEP], 2, &new, true).unwrap();
    assert!(done.diagnostics.is_empty(), "{:?}", done.diagnostics);
    let (name, written) = &done.migrated[0];
    assert_eq!(name, "scenes/main.scene.json");
    assert!(
        written.contains(&"/resources/Ambient/source".to_owned()),
        "{written:?}"
    );
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&scene).unwrap()).unwrap();
    assert_eq!(after["format"], 2);
    assert_eq!(after["resources"]["Ambient"]["source"], "Color");
    assert_eq!(after["resources"]["RenderSettings"]["shadow_cascades"], 1);
    assert_eq!(
        wish_scene::json::to_canonical(&after).unwrap(),
        std::fs::read_to_string(&scene).unwrap(),
        "written in canonical form"
    );
    let again = wish_control::migrate::migrate_project(&dir, &[], &[STEP], 2, &new, true).unwrap();
    assert!(again.migrated.is_empty());
    assert_eq!(again.up_to_date, done.up_to_date + 1);
    // Today's engine: format 1 is current, nothing to migrate.
    let (dir, _ws) = project("migrate-today");
    let today = wish_control::migrate::migrate_project(
        &dir,
        &[],
        wish_scene::migrate::STEPS,
        wish_scene::FORMAT,
        &wish_control::migrate::engine_defaults().unwrap(),
        true,
    )
    .unwrap();
    assert!(today.migrated.is_empty() && today.diagnostics.is_empty());
}

/// The review of this package: `wen migrate` writes each file atomically
/// and goes on past one it cannot write, listing both; naming one scene
/// takes the prefabs it places along; a prefab it cannot read is a located
/// warning, not a silent guess.
#[test]
fn a_migration_goes_on_past_a_file_it_cannot_write_and_takes_prefabs_along() {
    let record = recorded();
    let (dir, _ws) = project("migrate-review");
    let mut new = with_look(&record, "natural");
    new.components.get_mut("Material").unwrap()["roughness"] = json!(0.9);
    const STEP: Step = Step {
        from: 1,
        defaults: wish_scene::migrate::FORMAT_1_DEFAULTS,
    };
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join("prefabs/lamp.prefab.json"),
        "{ \"format\": 1, \"entities\": { \"root\": { \"components\": { \"Material\": {} } } } }",
    )
    .unwrap();
    std::fs::write(
        dir.join("scenes/level.scene.json"),
        "{ \"format\": 1, \"entities\": {
            \"lamp\": { \"prefab\": \"prefabs/lamp.prefab.json\", \"components\": { \"Material\": {} } },
            \"lost\": { \"prefab\": \"prefabs/gone.prefab.json\", \"components\": { \"Material\": {} } } } }",
    )
    .unwrap();
    std::fs::write(dir.join("scenes/locked.scene.json"), "{ \"format\": 1 }").unwrap();
    // The side file the atomic write needs is in the way: the file cannot
    // be written (the same on every system, unlike a read-only flag).
    std::fs::create_dir_all(dir.join("scenes/locked.scene.json.wish-tmp")).unwrap();

    // One scene named: its prefab comes along, the other scenes do not.
    let one = wish_control::migrate::migrate_project(
        &dir,
        &["scenes/level.scene.json".to_owned()],
        &[STEP],
        2,
        &new,
        true,
    )
    .unwrap();
    let names: Vec<&str> = one.migrated.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        ["prefabs/lamp.prefab.json", "scenes/level.scene.json"],
        "{one:?}"
    );
    // The instance of the lamp leans on the prefab (pinned there); the one
    // whose prefab is gone is pinned, with a warning that says so.
    let level: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("scenes/level.scene.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        level["entities"]["lamp"]["components"]["Material"],
        json!({})
    );
    assert_eq!(
        level["entities"]["lost"]["components"]["Material"]["roughness"],
        0.5
    );
    let lamp: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("prefabs/lamp.prefab.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        lamp["entities"]["root"]["components"]["Material"]["roughness"],
        0.5
    );
    assert_eq!(lamp["format"], 2);
    let warnings: Vec<&str> = one.diagnostics.iter().map(|d| d.code.as_ref()).collect();
    assert_eq!(warnings, ["migration_prefab_unresolved"], "{one:?}");
    assert!(!one.diagnostics[0].is_error());
    assert_eq!(
        one.diagnostics[0].location.file.as_deref(),
        Some("scenes/level.scene.json")
    );

    // The whole project: the locked scene fails, located; the rest is
    // migrated and listed.
    let all = wish_control::migrate::migrate_project(&dir, &[], &[STEP], 2, &new, true).unwrap();
    let failed: Vec<(&str, Option<&str>)> = all
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.code.as_ref(), d.location.file.as_deref()))
        .collect();
    assert_eq!(
        failed,
        [("file_unwritable", Some("scenes/locked.scene.json"))],
        "{all:?}"
    );
    let names: Vec<&str> = all.migrated.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["scenes/main.scene.json"], "{all:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/locked.scene.json")).unwrap(),
        "{ \"format\": 1 }",
        "a file that could not be written is as it was"
    );
}
