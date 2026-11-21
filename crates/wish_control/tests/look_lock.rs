//! The look lock (M12 package 9): named looks applied with `edit_apply`
//! `apply_look`; the recorded defaults of the file format, kept equal to the
//! engine's (ADR-027, rule 1, the guard); the migration the change of the
//! default look needed, tried with each look as the new defaults on every
//! scene of the repository and on generated ones; and the switch itself:
//! the defaults are the look "Natural", the scene format is 2, and a file of
//! format 1 is read as it was meant, keeps its picture to the byte, and
//! becomes its migrated form when it is next saved. Design:
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

/// The defaults of format 1: the engine's before the look lock.
fn recorded() -> Defaults {
    Defaults::parse(wish_scene::migrate::FORMAT_1_DEFAULTS).unwrap()
}

/// [`recorded`] with the types the engine added since format 1 (release
/// 1.1 on: `Players`, `PlayerActionState`, ...) at their defaults now. A
/// file written before a type existed cannot hold it, so the type means
/// there what its defaults say: a format changes by addition (ADR-028,
/// rule 5), and the record of format 1 is never edited.
fn recorded_with_additions() -> Defaults {
    let mut record = recorded();
    let engine = wish_control::migrate::engine_defaults().unwrap();
    for (name, value) in engine.components {
        record.components.entry(name).or_insert(value);
    }
    for (name, value) in engine.resources {
        record.resources.entry(name).or_insert(value);
    }
    record
}

const MAIN: &str = "scenes/main.scene.json";

/// Relabels the project's scene as a file of format 1: what a project made
/// before the look lock holds (the starting scene's content did not change
/// with the switch, only the number).
fn as_format_1(dir: &Path) -> String {
    let now = std::fs::read_to_string(dir.join(MAIN)).unwrap();
    let old = now.replace("\"format\": 2", "\"format\": 1");
    assert_ne!(old, now, "a new project is format 2: {now}");
    std::fs::write(dir.join(MAIN), &old).unwrap();
    old
}

const ALL_EFFECTS: [&str; 6] = [
    "sky_light",
    "sun_disc",
    "fog",
    "bloom",
    "color_grading",
    "fxaa",
];

// --- Named looks -----------------------------------------------------------

#[test]
fn a_look_is_applied_in_one_step_and_the_answer_says_what_it_set() {
    let (dir, mut ws) = project("apply");
    let me = parse_actor("claude-code");
    let size = json!({ "width": 96, "height": 54 });
    // A new scene sets nothing of the look: it is drawn with the defaults,
    // which are the look "Natural".
    let before = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(before["stats"]["effects"], json!(ALL_EFFECTS), "{before}");
    assert_eq!(before["stats"]["shadow_cascades"], 4);
    let file_before = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    assert!(!file_before.contains("resources"), "{file_before}");

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
    assert_eq!(after["stats"]["effects"], json!(ALL_EFFECTS));
    assert_eq!(after["stats"]["shadow_cascades"], 4);
    // Natural written out is the picture of the defaults, to the byte: the
    // look and the defaults are the same twenty values.
    assert_eq!(after["image"]["data"], before["image"]["data"]);
    // One change of the history, undone in one step.
    ok(&ws.call("edit_history", &json!({ "action": "undo" }), &me));
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file_before
    );
    let undone = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_eq!(undone["image"]["data"], before["image"]["data"]);

    // Vivid is another picture, with the same effects.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "vivid" }] }),
        &me,
    ));
    let vivid = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_ne!(vivid["image"]["data"], before["image"]["data"]);
    assert_eq!(vivid["stats"]["effects"], json!(ALL_EFFECTS));

    // Plain turns every effect off: the picture the engine drew before the
    // look settings existed, which is also, to the byte, the picture of
    // the same scene as a file of format 1.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "apply_look", "look": "plain" }] }),
        &me,
    ));
    let plain = ok(&ws.call("capture_frame", &size, &me)).clone();
    assert_ne!(plain["image"]["data"], before["image"]["data"]);
    assert_eq!(plain["stats"]["effects"], json!([]));
    assert_eq!(plain["stats"]["shadow_cascades"], 1);
    std::fs::write(
        dir.join("scenes/old.scene.json"),
        file_before.replace("\"format\": 2", "\"format\": 1"),
    )
    .unwrap();
    let mut old_size = size.clone();
    old_size["scene"] = json!("scenes/old.scene.json");
    let old = ok(&ws.call("capture_frame", &old_size, &me)).clone();
    assert_eq!(old["image"]["data"], plain["image"]["data"]);
    assert_eq!(old["stats"]["effects"], json!([]));

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
fn every_look_is_a_full_set_of_clean_values_plain_is_format_1_and_natural_the_defaults() {
    let record = recorded_with_additions();
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
    // Natural is the engine's defaults, exactly: the record of format 1
    // with Natural's values in place is the engine's whole table of
    // defaults (so no other default moved with the switch), and that table
    // is the record of format 2.
    let engine = wish_control::migrate::engine_defaults().unwrap();
    assert_eq!(with_look(&record, "natural"), engine);
    assert_eq!(
        Defaults::parse(wish_scene::migrate::FORMAT_2_DEFAULTS).unwrap(),
        engine
    );
    // What changed between the two formats: fourteen values of six
    // resources, and nothing of any component.
    assert_eq!(record.components, engine.components);
    let (mut changed, mut added) = (Vec::new(), Vec::new());
    differences(
        &record.to_value(1)["resources"],
        &engine.to_value(1)["resources"],
        "",
        &mut changed,
        &mut added,
    );
    assert!(added.is_empty(), "{added:?}");
    assert_eq!(changed.len(), 14, "{changed:#?}");
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

/// The loader reads an older file with the defaults of the registry's types
/// that agree with the record of the current format
/// (`Defaults::agreeing_with`), so that someone else's type of an engine
/// type's name is never pinned. A type whose defaults drift from the record
/// (a field added without `UPDATE_DEFAULTS=1`) would be left out silently,
/// and every file of format 1 would then lose its pinned values for that
/// type and change its look with no diagnostic. So every type the engine
/// registers must be kept, and every type the record of format 1 knows must
/// still be one of them (a type taken away needs a migration step of its
/// own) — checked against the record built into the loader, not the file
/// on disk (review of the switch).
#[test]
fn the_loader_pins_every_type_the_engine_registers() {
    let engine = wish_control::migrate::engine_defaults().unwrap();
    let current = Defaults::parse(wish_scene::migrate::CURRENT_DEFAULTS).unwrap();
    let kept = engine.agreeing_with(&current);
    let left_out = |all: &BTreeMap<String, Value>, kept: &BTreeMap<String, Value>| {
        all.keys()
            .filter(|name| !kept.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        left_out(&engine.components, &kept.components),
        Vec::<String>::new(),
        "engine components whose defaults are not the record of format {} (run \
         UPDATE_DEFAULTS=1 cargo test -p wish_control --test look_lock)",
        wish_scene::FORMAT
    );
    assert_eq!(
        left_out(&engine.resources, &kept.resources),
        Vec::<String>::new(),
        "engine resources whose defaults are not the record of format {} (run \
         UPDATE_DEFAULTS=1 cargo test -p wish_control --test look_lock)",
        wish_scene::FORMAT
    );
    assert_eq!(kept, engine);
    let old = recorded();
    assert_eq!(
        left_out(&old.components, &kept.components),
        Vec::<String>::new()
    );
    assert_eq!(
        left_out(&old.resources, &kept.resources),
        Vec::<String>::new()
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
    let record = recorded_with_additions();
    let (files, prefabs) = corpus(&record);
    let prefab_has = |prefab: &str, entity: &str, component: &str| {
        prefabs
            .get(prefab)
            .and_then(|file| file.pointer(&format!("/entities/{entity}/components/{component}")))
            .is_some()
    };
    let candidates = [
        (
            "the engine's",
            wish_control::migrate::engine_defaults().unwrap(),
        ),
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
    // Plain is format 1's defaults: nothing to write, every file as it was.
    assert_eq!(written_total["plain"], 0, "{written_total:?}");
    assert!(
        written_total["natural"] > 0
            && written_total["everything changed"] > written_total["natural"]
    );
    assert_eq!(written_total["the engine's"], written_total["natural"]);
    // With no changed default, every example's file is left byte for byte
    // as it is.
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

/// Draws `file` as a new scene file of the project (a new name each time,
/// so that no change is missed). Returns the picture and the effects.
fn picture(dir: &Path, ws: &mut Workspace, file: &Value) -> (Value, Value) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scene = format!("scenes/probe_{n}.scene.json");
    std::fs::write(
        dir.join(&scene),
        wish_scene::json::to_canonical(file).unwrap(),
    )
    .unwrap();
    let me = parse_actor("claude-code");
    let answer = ok(&ws.call(
        "capture_frame",
        &json!({ "scene": scene, "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    (
        answer["image"]["data"].clone(),
        answer["stats"]["effects"].clone(),
    )
}

/// The proof by picture, with the real switch: a scene file of format 1
/// drawn as it is (read through the migration in memory), drawn after the
/// migration wrote it as format 2, and drawn with the look Plain applied to
/// its unmigrated content give the same bytes; the same content labelled
/// format 2 without the migration is the new default look, another picture.
#[test]
fn a_format_1_scene_keeps_its_picture_to_the_byte() {
    let (dir, mut ws) = project("pictures");
    let start: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(MAIN)).unwrap()).unwrap();
    let first_pixel: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("examples/first-pixel/scenes/main.scene.json"))
            .unwrap(),
    )
    .unwrap();
    let mut partly = start.clone();
    partly["resources"] = json!({
        "Fog": { "start": 2.0 },
        "RenderSettings": { "shadow_map_size": 1024 },
        "Sky": { "zenith": [0.3, 0.2, 0.6, 1.0] },
        "Ambient": { "intensity": 12000.0 }
    });
    let no_prefabs = |_: &str, _: &str, _: &str| false;
    let engine = wish_control::migrate::engine_defaults().unwrap();
    let me = parse_actor("claude-code");
    for (label, file) in [
        ("new project", &start),
        ("first-pixel", &first_pixel),
        ("partly set", &partly),
    ] {
        let mut old = file.clone();
        old["format"] = json!(1);
        let (as_it_is, effects) = picture(&dir, &mut ws, &old);
        assert_eq!(
            effects,
            json!([]),
            "{label}: a format-1 file has no effect on"
        );
        let mut migrated = old.clone();
        let written = wish_scene::migrate::upgrade(
            &mut migrated,
            Kind::Scene,
            wish_scene::FORMAT,
            wish_scene::migrate::STEPS,
            &engine,
            &no_prefabs,
        )
        .unwrap();
        assert!(!written.is_empty(), "{label}");
        assert_eq!(migrated["format"], 2);
        let (after, effects) = picture(&dir, &mut ws, &migrated);
        assert_eq!(
            after, as_it_is,
            "{label}: the migrated file draws differently"
        );
        assert_eq!(effects, json!([]));
        // The same content as a file of format 2: the new defaults.
        let mut new = file.clone();
        new["format"] = json!(2);
        let (unmigrated, effects) = picture(&dir, &mut ws, &new);
        assert_ne!(
            unmigrated, as_it_is,
            "{label}: the new defaults change nothing?"
        );
        assert_eq!(effects, json!(ALL_EFFECTS), "{label}");
        // And Plain applied to that is the old picture again.
        let scene = format!("scenes/plain_{}.scene.json", label.replace([' ', '-'], "_"));
        std::fs::write(
            dir.join(&scene),
            wish_scene::json::to_canonical(&new).unwrap(),
        )
        .unwrap();
        ok(&ws.call(
            "edit_apply",
            &json!({ "scene": scene, "ops": [{ "op": "apply_look", "look": "plain" }] }),
            &me,
        ));
        let plain = ok(&ws.call(
            "capture_frame",
            &json!({ "scene": scene, "width": 160, "height": 90 }),
            &me,
        ))["image"]["data"]
            .clone();
        assert_eq!(
            plain, as_it_is,
            "{label}: Plain is not the format-1 picture"
        );
    }
}

/// `wen migrate`'s capability with the engine's own step: a project of
/// format 1 is rewritten in canonical form with the values it relied on, a
/// second run finds it current, and a dry run writes nothing.
#[test]
fn a_project_is_migrated_with_the_switchs_step() {
    let (dir, _ws) = project("migrate");
    let engine = wish_control::migrate::engine_defaults().unwrap();
    let steps = wish_scene::migrate::STEPS;
    let to = wish_scene::FORMAT;
    assert_eq!((steps.len(), to), (1, 2));
    // A new project is current already.
    let fresh =
        wish_control::migrate::migrate_project(&dir, &[], steps, to, &engine, true).unwrap();
    assert!(
        fresh.migrated.is_empty() && fresh.diagnostics.is_empty(),
        "{fresh:?}"
    );
    let before = as_format_1(&dir);
    let scene = dir.join(MAIN);
    let dry = wish_control::migrate::migrate_project(&dir, &[], steps, to, &engine, false).unwrap();
    assert_eq!(dry.migrated.len(), 1, "{dry:?}");
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), before);
    let done = wish_control::migrate::migrate_project(&dir, &[], steps, to, &engine, true).unwrap();
    assert!(done.diagnostics.is_empty(), "{:?}", done.diagnostics);
    let (name, written) = &done.migrated[0];
    assert_eq!(name, MAIN);
    assert_eq!(written.len(), 14, "{written:?}");
    assert!(
        written.contains(&"/resources/Ambient/source".to_owned()),
        "{written:?}"
    );
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&scene).unwrap()).unwrap();
    assert_eq!(after["format"], 2);
    assert_eq!(after["resources"]["Ambient"]["source"], "Color");
    assert_eq!(after["resources"]["RenderSettings"]["shadow_cascades"], 1);
    assert_eq!(after["resources"]["Sky"]["sun_size_deg"], 0.0);
    assert_eq!(
        wish_scene::json::to_canonical(&after).unwrap(),
        std::fs::read_to_string(&scene).unwrap(),
        "written in canonical form"
    );
    let again =
        wish_control::migrate::migrate_project(&dir, &[], steps, to, &engine, true).unwrap();
    assert!(again.migrated.is_empty());
    assert_eq!(again.up_to_date, done.up_to_date + 1);
}

fn open(dir: &Path) -> Workspace {
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    ws
}

/// A project of format 1 is opened without a migration: read as it was
/// meant, never rewritten by looking at it, never taken for a change made
/// outside the engine; the first change saves it as its migrated form, the
/// bytes `wen migrate` writes; `check` says so with a warning, not an error.
#[test]
fn a_format_1_project_opens_as_it_was_and_is_saved_migrated_on_its_next_change() {
    let (dir, ws) = project("format-1");
    drop(ws);
    let old = as_format_1(&dir);
    let scene = dir.join(MAIN);
    let me = parse_actor("claude-code");
    let mut ws = open(&dir);

    // check: one warning that names the file and the remedy; it passes.
    let checked = ok(&ws.call("check", &json!({}), &me)).clone();
    assert_eq!(checked["passed"], true, "{checked}");
    let warnings: Vec<&Value> = checked["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "old_format")
        .collect();
    assert_eq!(warnings.len(), 1, "{checked}");
    assert_eq!(warnings[0]["severity"], "warning");
    assert_eq!(warnings[0]["location"]["file"], MAIN);
    assert!(
        warnings[0]["hint"]
            .as_str()
            .unwrap()
            .contains("wen migrate"),
        "{checked}"
    );

    // Looking at it, again and again: the old picture, the values the file
    // relied on, no history, nothing "changed by others", and the file as
    // it was.
    for _ in 0..2 {
        let shot =
            ok(&ws.call("capture_frame", &json!({ "width": 96, "height": 54 }), &me)).clone();
        assert_eq!(shot["stats"]["effects"], json!([]), "{shot}");
        assert_eq!(shot["stats"]["shadow_cascades"], 1);
        assert_eq!(shot["revision"], 0);
        assert!(shot.get("diagnostics").is_none(), "{shot}");
        let got = ok(&ws.call("scene_get", &json!({}), &me)).clone();
        assert_eq!(got["revision"], 0, "{got}");
        assert!(got.get("changed_by_others").is_none(), "{got}");
        let history = ok(&ws.call("edit_history", &json!({}), &me)).clone();
        assert_eq!(history["revision"], 0, "{history}");
        assert!(
            history["entries"].as_array().is_none_or(Vec::is_empty),
            "{history}"
        );
        assert_eq!(std::fs::read_to_string(&scene).unwrap(), old);
    }
    let fog = ok(&ws.call(
        "entity_get",
        &json!({ "resources": ["Fog", "Ambient"] }),
        &me,
    ))
    .clone();
    assert_eq!(fog["resources"]["Fog"]["density"], 0.0, "{fog}");
    assert_eq!(fog["resources"]["Ambient"]["source"], "Color");
    // A game started from it plays, and the file is still as it was.
    let session = ok(&ws.call("sim_start", &json!({}), &me))["session"].clone();
    ok(&ws.call("sim_step", &json!({ "session": session, "frames": 3 }), &me));
    ok(&ws.call("sim_stop", &json!({ "session": session }), &me));
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), old);

    // A change made outside the engine to the format-1 file is one change
    // by the file that touches what was changed, nothing else.
    let mut by_hand: Value = serde_json::from_str(&old).unwrap();
    by_hand["entities"]["ground"]["components"]["Material"]["roughness"] = json!(0.5);
    let edited = wish_scene::json::to_canonical(&by_hand).unwrap();
    assert_ne!(edited, old, "{old}");
    std::fs::write(&scene, &edited).unwrap();
    let got = ok(&ws.call("scene_get", &json!({}), &me)).clone();
    assert_eq!(got["revision"], 1, "{got}");
    assert_eq!(got["changed_by_others"]["by"], json!(["file"]), "{got}");
    assert_eq!(
        got["changed_by_others"]["entities"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{got}"
    );
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), edited);

    // The first change through the engine saves the file as format 2, with
    // the values it relied on written in. Undone, the file is the migrated
    // form of what it was: the bytes `wen migrate` writes for the same file.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set", "id": "camera", "component": "Camera", "value": { "fov_y_deg": 40.0 } }] }),
        &me,
    ));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&scene).unwrap()).unwrap();
    assert_eq!(saved["format"], 2, "{saved}");
    assert_eq!(saved["resources"]["Fog"]["density"], 0.0);
    assert_eq!(saved["resources"]["Bloom"]["strength"], 0.0);
    assert_eq!(
        saved["entities"]["camera"]["components"]["Camera"]["fov_y_deg"],
        40.0
    );
    let shot = ok(&ws.call("capture_frame", &json!({ "width": 96, "height": 54 }), &me)).clone();
    assert_eq!(shot["stats"]["effects"], json!([]), "{shot}");
    ok(&ws.call("edit_history", &json!({ "action": "undo" }), &me));
    let by_the_engine = std::fs::read_to_string(&scene).unwrap();

    let (other, ws2) = project("format-1-migrated");
    drop(ws2);
    std::fs::write(other.join(MAIN), &edited).unwrap();
    let report = wish_control::migrate::migrate_project(
        &other,
        &[],
        wish_scene::migrate::STEPS,
        wish_scene::FORMAT,
        &wish_control::migrate::engine_defaults().unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(report.migrated.len(), 1, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(other.join(MAIN)).unwrap(),
        by_the_engine
    );
    // The migrated file is current: it loads without a note, and loading
    // and saving it gives the same bytes (invariant 3).
    let mut ws2 = open(&other);
    let checked = ok(&ws2.call("check", &json!({}), &me)).clone();
    assert_eq!(checked["warnings"], 0, "{checked}");
    ok(&ws2.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_notes", "id": "camera", "notes": "x" }] }),
        &me,
    ));
    ok(&ws2.call("edit_history", &json!({ "action": "undo" }), &me));
    assert_eq!(
        std::fs::read_to_string(other.join(MAIN)).unwrap(),
        by_the_engine
    );

    // A file newer than the engine is still refused, and so is a format no
    // engine wrote.
    for (format, word) in [(3, "newer"), (0, "No engine")] {
        std::fs::write(
            dir.join("scenes/odd.scene.json"),
            format!("{{\n  \"format\": {format}\n}}\n"),
        )
        .unwrap();
        let refused = ws.call(
            "scene_get",
            &json!({ "scene": "scenes/odd.scene.json" }),
            &me,
        );
        assert_eq!(refused["error"]["code"], "unsupported_format", "{refused}");
        assert!(
            refused["error"]["hint"].as_str().unwrap().contains(word),
            "{refused}"
        );
    }
}

/// The review of this package: `wen migrate` writes each file atomically
/// and goes on past one it cannot write, listing both; naming one scene
/// takes the prefabs it places along; a prefab it cannot read is a located
/// warning, not a silent guess.
#[test]
fn a_migration_goes_on_past_a_file_it_cannot_write_and_takes_prefabs_along() {
    let record = recorded();
    let (dir, _ws) = project("migrate-review");
    as_format_1(&dir);
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
