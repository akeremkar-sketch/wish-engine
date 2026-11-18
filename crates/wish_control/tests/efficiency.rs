//! The AI efficiency work (design note `ai-efficiency.md`): answers that
//! save an agent a turn. edit_apply answers the changed entities as they
//! are now; sim_step reports watched entities at the end; a wrong name
//! among the components to show is a warning, not a failed call; a type
//! is found by the words people use for it.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn workspace(name: &str) -> Workspace {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("efficiency-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    create_project(&root, "Efficiency", "wen").unwrap();
    std::fs::remove_dir_all(root.join("scripts")).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })
    .unwrap();
    let ops = json!([
        { "op": "spawn", "id": "floor", "components": {
            "Transform": { "translation": [0, -0.1, 0], "scale": [20, 0.2, 20] },
            "Collider": { "shape": "Box" }, "RigidBody": { "kind": "Fixed" } } },
        { "op": "spawn", "id": "car", "components": { "Transform": { "translation": [5, 0.5, 0] } } },
        { "op": "spawn", "id": "box", "components": {
            "Transform": { "translation": [0, 4, 0] }, "Collider": { "shape": "Box" } } }
    ]);
    let made = call(&mut ws, "edit_apply", json!({ "ops": ops }));
    assert_eq!(made["ok"], true, "{made}");
    ws
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent:test"))
}

fn near(value: &Value, want: [f64; 3]) -> bool {
    value.as_array().is_some_and(|a| {
        a.iter()
            .zip(want)
            .all(|(v, w)| (v.as_f64().unwrap() - w).abs() < 0.01)
    })
}

#[test]
fn edit_apply_answers_the_changed_entities_as_they_are_now() {
    let mut ws = workspace("now");
    // A spawn shows every component and where it is.
    let spawned = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "wheel", "parent": "car",
            "components": { "Transform": { "translation": [0.5, -0.2, 0] } } }] }),
    );
    assert_eq!(spawned["ok"], true, "{spawned}");
    let wheel = &spawned["now"]["wheel"];
    assert!(wheel["components"]["Transform"].is_object(), "{spawned}");
    assert!(
        near(&wheel["world_position"], [5.5, 0.3, 0.0]),
        "under car: {spawned}"
    );
    // A set shows only the component it touched; a moved parent moves the
    // world position.
    let painted = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set", "id": "box", "component": "Material", "value": { "base_color": [1, 0, 0, 1] } },
            { "op": "set", "id": "car", "component": "Transform", "value": { "translation": [6, 0.5, 0] } }
        ] }),
    );
    let now = &painted["now"];
    assert_eq!(
        now["box"]["components"]["Material"]["base_color"],
        json!([1.0, 0.0, 0.0, 1.0]),
        "{painted}"
    );
    assert!(now["box"]["components"].get("Transform").is_none());
    assert!(
        now["box"].get("world_position").is_none(),
        "not moved: {painted}"
    );
    assert!(near(&now["car"]["world_position"], [6.0, 0.5, 0.0]));
    // A despawned one is not shown; a dry run shows nothing as now.
    let gone = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "despawn", "id": "wheel" }] }),
    );
    assert!(gone.get("now").is_none(), "{gone}");
    let dry = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "despawn", "id": "car" }], "dry_run": true }),
    );
    assert!(dry.get("now").is_none(), "{dry}");
}

#[test]
fn sim_step_reports_watched_entities_at_the_end() {
    let mut ws = workspace("watch");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "box", "component": "RigidBody", "value": { "kind": "Dynamic" } }] }),
    );
    let started = call(&mut ws, "sim_start", json!({}));
    let session = started["session"].as_str().unwrap().to_owned();
    let ran = call(
        &mut ws,
        "sim_step",
        json!({ "session": session, "frames": 180, "watch": ["box", "floor"] }),
    );
    assert_eq!(ran["ok"], true, "{ran}");
    let box_at = &ran["watched"]["box"]["world_position"];
    assert!(near(box_at, [0.0, 0.5, 0.0]), "came to rest: {ran}");
    assert!(ran["watched"]["box"]["velocity"].is_array(), "{ran}");
    assert!(near(
        &ran["watched"]["floor"]["world_position"],
        [0.0, -0.1, 0.0]
    ));
    // A wrong id is refused before anything runs.
    let wrong = call(
        &mut ws,
        "sim_step",
        json!({ "session": session, "frames": 10, "watch": ["boxx"] }),
    );
    assert_eq!(wrong["error"]["code"], "unknown_entity", "{wrong}");
    assert!(wrong["error"]["hint"].as_str().unwrap().contains("box"));
    let after = call(&mut ws, "sim_step", json!({ "session": session }));
    assert_eq!(after["tick"], 181, "the refused call ran nothing: {after}");
}

#[test]
fn a_wrong_component_to_show_is_a_warning() {
    let mut ws = workspace("lenient");
    let found = call(
        &mut ws,
        "entity_query",
        json!({ "name": "box", "components": ["Transform", "MeshMaterial"] }),
    );
    assert_eq!(found["ok"], true, "{found}");
    assert!(
        found["entities"][0]["components"]["Transform"].is_object(),
        "{found}"
    );
    assert!(
        found["diagnostics"][0].get("candidates").is_none(),
        "short: {found}"
    );
    let warning = &found["diagnostics"][0];
    assert_eq!(warning["code"], "unknown_component", "{found}");
    assert_eq!(warning["severity"], "warning");
    // Filtering by a wrong name still fails: the answer would be wrong.
    let wrong = call(&mut ws, "entity_query", json!({ "with": ["MeshMaterial"] }));
    assert_eq!(wrong["error"]["code"], "unknown_component");
    // entity_get: a partly wrong list is a warning, an all-wrong one fails.
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "ids": ["box"], "components": ["Collider", "Hitbox"] }),
    );
    assert!(
        got["entities"][0]["components"]["Collider"].is_object(),
        "{got}"
    );
    assert_eq!(got["diagnostics"][0]["code"], "unknown_component");
    let none = call(
        &mut ws,
        "entity_get",
        json!({ "ids": ["box"], "components": ["Hitbox"] }),
    );
    assert_eq!(none["error"]["code"], "unknown_component", "{none}");
}

#[test]
fn types_are_found_by_the_words_people_use() {
    let mut ws = workspace("search");
    let names = |answer: &Value| -> Vec<String> {
        answer["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    };
    for (word, wanted) in [
        ("mesh", "Primitive"),
        ("position", "Transform"),
        ("colour", "Material"),
        ("gravity", "RigidBody"),
        ("lamp", "PointLight"),
    ] {
        let found = call(&mut ws, "schema_get", json!({ "search": word }));
        assert!(
            names(&found).iter().any(|n| n == wanted),
            "{word} -> {:?}",
            names(&found)
        );
    }
}
