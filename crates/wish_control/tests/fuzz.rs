//! Every invalid call gets a structured error with a hint, and nothing an
//! agent can send takes the control plane down (M4 exit criterion).
//!
//! For every tool in the catalogue, arguments are derived from its parameter
//! table and broken in every way the table allows: missing required ones,
//! each parameter with every wrong JSON type, out-of-range numbers, unknown
//! choices, an unknown parameter, a non-object. Then references that do not
//! exist (scene, entity, session, snapshot, asset). Every answer must be
//! `ok: false` with a non-empty hint and a code that has a catalogue page;
//! `internal_error` (a bug) is never acceptable. Answers that succeed must
//! only carry catalogued diagnostic codes too.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Kind, Param, Workspace, WorkspaceOptions, catalog, parse_actor};

fn catalogued() -> BTreeSet<&'static str> {
    wish_control::docs::DIAGNOSTICS
        .iter()
        .map(|(code, _)| *code)
        .collect()
}

/// A valid-looking value for a parameter kind.
fn good(param: &Param) -> Value {
    match param.kind {
        Kind::String => match param.name {
            "scene" => json!("scenes/main.scene.json"),
            "path" | "from" => json!("assets/none.glb"),
            "to" => json!("assets/other.glb"),
            "query" => json!("camera"),
            _ => json!("x"),
        },
        Kind::Integer { min, max } => json!(min.max(0).min(max)),
        Kind::Number { min, .. } => json!(min.max(0.0)),
        Kind::Boolean => json!(false),
        Kind::StringList => json!([]),
        Kind::Object => json!({}),
        Kind::Array => json!([]),
        Kind::Choice(choices) => json!(choices[0]),
    }
}

/// Values of the wrong JSON type for a kind.
fn wrong(kind: Kind) -> Vec<Value> {
    let all = [
        json!(null),
        json!(true),
        json!(7),
        json!(2.5),
        json!("text"),
        json!([1]),
        json!({ "a": 1 }),
    ];
    all.into_iter()
        .filter(|value| {
            let fits = match kind {
                Kind::String | Kind::Choice(_) => value.is_string(),
                Kind::Integer { .. } => value.is_i64(),
                Kind::Number { .. } => value.is_number(),
                Kind::Boolean => value.is_boolean(),
                Kind::StringList => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string)),
                Kind::Object => value.is_object(),
                Kind::Array => value.is_array(),
            };
            // null means "left out", which is fine for optional parameters.
            !fits && !value.is_null()
        })
        .collect()
}

fn base_args(params: &[Param]) -> Map<String, Value> {
    params
        .iter()
        .filter(|p| p.required)
        .map(|p| (p.name.to_owned(), good(p)))
        .collect()
}

/// Every broken argument object for a tool.
fn broken(params: &[Param]) -> Vec<Value> {
    let mut cases = vec![json!([1, 2]), json!("args"), json!(3)];
    let base = base_args(params);
    let mut unknown = base.clone();
    unknown.insert("definitely_not_a_parameter".to_owned(), json!(1));
    cases.push(Value::Object(unknown));
    for param in params {
        if param.required {
            let mut missing = base.clone();
            missing.remove(param.name);
            cases.push(Value::Object(missing));
        }
        for value in wrong(param.kind) {
            let mut args = base.clone();
            args.insert(param.name.to_owned(), value);
            cases.push(Value::Object(args));
        }
        let mut extreme = Vec::new();
        match param.kind {
            Kind::Integer { min, max } => {
                if min > i64::MIN {
                    extreme.push(json!(min - 1));
                }
                if max < i64::MAX {
                    extreme.push(json!(max + 1));
                }
                extreme.push(json!(1.5));
            }
            Kind::Choice(_) => extreme.push(json!("no_such_choice")),
            _ => {}
        }
        for value in extreme {
            let mut args = base.clone();
            args.insert(param.name.to_owned(), value);
            cases.push(Value::Object(args));
        }
    }
    cases
}

/// Calls that are well-formed but name things that do not exist.
fn dangling() -> Vec<(&'static str, Value)> {
    vec![
        ("scene_get", json!({ "scene": "scenes/missing.scene.json" })),
        ("scene_get", json!({ "root": "nobody" })),
        ("scene_get", json!({ "scene": "../escape.scene.json" })),
        ("entity_get", json!({ "ids": ["nobody"] })),
        ("entity_get", json!({ "ids": [] })),
        (
            "entity_get",
            json!({ "ids": ["camera"], "components": ["Nope"] }),
        ),
        ("entity_get", json!({ "ids": ["camera"], "session": "s9" })),
        ("entity_query", json!({ "where": "((((" })),
        ("entity_query", json!({ "where": "Nope.x > 1" })),
        ("entity_query", json!({ "with": ["Nope"] })),
        ("entity_query", json!({ "near": { "point": "here" } })),
        ("entity_query", json!({ "cursor": "after:nobody" })),
        ("edit_apply", json!({ "ops": [] })),
        ("edit_apply", json!({ "ops": [{}] })),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "despawn", "id": "nobody" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "set", "id": "camera", "component": "Camera", "value": { "fov_y_deg": 1000 } }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "set", "id": "camera", "component": "Camera", "value": { "fvo": 1 } }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "spawn", "id": "camera" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "spawn", "id": "bad id!" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "reparent", "id": "camera", "parent": "camera" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "instantiate", "id": "x", "prefab": "prefabs/none.prefab.json" }] }),
        ),
        (
            "edit_apply",
            json!({ "base_revision": 99, "ops": [{ "op": "spawn", "id": "x" }] }),
        ),
        (
            "edit_apply",
            json!({ "scene": "scenes/missing.scene.json", "ops": [{ "op": "spawn", "id": "x" }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "set", "id": "camera", "component": "Transform", "value": { "euler_deg": "up" } }] }),
        ),
        (
            "edit_apply",
            json!({ "ops": [{ "op": "set", "id": "camera", "component": "GlobalTransform", "value": {} }] }),
        ),
        ("asset_info", json!({ "path": "assets/none.glb" })),
        (
            "asset_move",
            json!({ "from": "assets/none.glb", "to": "assets/x.glb" }),
        ),
        (
            "asset_move",
            json!({ "from": "scenes", "to": "scenes/inner" }),
        ),
        ("asset_move", json!({ "from": "../outside", "to": "x" })),
        ("check", json!({ "paths": ["nope.scene.json"] })),
        ("schema_get", json!({ "names": ["Nope"] })),
        ("sim_step", json!({})),
        ("sim_step", json!({ "session": "s9" })),
        ("sim_snapshot", json!({ "session": "s9" })),
        ("sim_restore", json!({ "snapshot": "x" })),
        ("sim_stop", json!({ "session": "s9" })),
        ("sim_start", json!({ "scene": "scenes/missing.scene.json" })),
        ("capture_frame", json!({ "camera": "nobody" })),
        ("capture_frame", json!({ "camera": "ground" })),
        ("capture_frame", json!({ "out": "../x.png" })),
        ("capture_frame", json!({ "session": "s9" })),
        ("describe_view", json!({ "camera": "nobody" })),
        ("logs_get", json!({ "kinds": ["nope"] })),
    ]
}

fn assert_good_error(tool: &str, args: &Value, answer: &Value, known: &BTreeSet<&str>) {
    assert_eq!(answer["ok"], false, "{tool} {args} should fail: {answer}");
    let error = &answer["error"];
    let code = error["code"].as_str().unwrap_or_default();
    assert_ne!(code, "internal_error", "{tool} {args}: a bug: {answer}");
    assert!(
        known.contains(code),
        "{tool} {args}: the code '{code}' has no catalogue page"
    );
    assert!(
        !error["hint"].as_str().unwrap_or_default().trim().is_empty(),
        "{tool} {args}: no hint: {answer}"
    );
    assert!(!error["message"].as_str().unwrap_or_default().is_empty());
    assert_eq!(error["docs"], format!("wish://docs/diagnostics/{code}"));
}

fn assert_catalogued_notes(tool: &str, answer: &Value, known: &BTreeSet<&str>) {
    for note in answer["diagnostics"].as_array().into_iter().flatten() {
        let code = note["code"].as_str().unwrap_or_default();
        assert!(
            known.contains(code),
            "{tool}: the note code '{code}' has no catalogue page"
        );
    }
}

#[test]
fn every_broken_call_is_explained_and_nothing_breaks() {
    let dir = std::env::temp_dir().join(format!("wish-fuzz-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Fuzz", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    let actor = parse_actor("fuzzer");
    let known = catalogued();
    let before = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();

    let mut cases = 0;
    for tool in catalog() {
        for args in broken(tool.params) {
            let answer = ws.call(tool.name, &args, &actor);
            assert_good_error(tool.name, &args, &answer, &known);
            cases += 1;
        }
    }
    for (tool, args) in dangling() {
        let answer = ws.call(tool, &args, &actor);
        assert_good_error(tool, &args, &answer, &known);
        cases += 1;
    }
    // Unknown tools, too.
    for name in ["", "entity", "edit_aply", "DROP TABLE"] {
        let answer = ws.call(name, &json!({}), &actor);
        assert_good_error(name, &json!({}), &answer, &known);
    }
    assert!(cases > 300, "only {cases} cases");
    // Nothing a failed call did changed the scene.
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        before
    );

    // And successful calls only carry catalogued notes.
    for (tool, args) in [
        ("project_info", json!({})),
        ("schema_get", json!({ "search": "zzzz" })),
        ("docs_search", json!({ "query": "zzzz" })),
        ("check", json!({})),
        ("edit_history", json!({ "action": "undo" })),
        ("sim_start", json!({})),
        (
            "sim_step",
            json!({ "until": "Transform.translation.y > 100000", "max_frames": 3 }),
        ),
        ("capture_frame", json!({ "width": 32, "height": 18 })),
    ] {
        let answer = ws.call(tool, &args, &actor);
        assert_eq!(answer["ok"], true, "{tool}: {answer}");
        assert_catalogued_notes(tool, &answer, &known);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
