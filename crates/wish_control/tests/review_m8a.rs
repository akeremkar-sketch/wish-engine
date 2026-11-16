//! Regression tests of the M8a closing review that go through the tools
//! (the editor's sit in wish_editor/tests, the HTTP transport's in
//! wish_mcp/tests/http.rs).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("review-m8a-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Review", "wen").unwrap();
    dir
}

fn open(dir: &Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent"))
}

fn ok(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = call(ws, tool, args);
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn refused(ws: &mut Workspace, tool: &str, args: Value, code: &str) -> Value {
    let answer = call(ws, tool, args);
    assert_eq!(answer["ok"], false, "{tool}: {answer}");
    assert_eq!(answer["error"]["code"], code, "{answer}");
    assert!(
        !answer["error"]["hint"].as_str().unwrap_or("").is_empty(),
        "{answer}"
    );
    answer
}

const LAMP: &str = "prefabs/lamp.prefab.json";

fn with_lamp(name: &str) -> (PathBuf, Workspace) {
    let dir = project(name);
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join(LAMP),
        r#"{ "format": 1, "entities": {
            "root": { "components": { "Transform": {} } },
            "bulb": { "parent": "root", "components": {
                "Transform": {}, "Primitive": { "shape": "Sphere" }, "Material": {} } } } }"#,
    )
    .unwrap();
    let mut ws = open(&dir);
    ok(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "instantiate", "id": "lamp_1", "prefab": LAMP }] }),
    );
    (dir, ws)
}

/// Finding (control plane 1): an edit that would leave a prefab file no
/// longer a prefab is refused, and the file stays as it was.
#[test]
fn a_prefab_edit_must_leave_a_prefab() {
    let (dir, mut ws) = with_lamp("prefab-rules");
    let before = std::fs::read_to_string(dir.join(LAMP)).unwrap();
    for op in [
        json!({ "op": "spawn", "id": "loose", "components": { "Transform": {} } }),
        json!({ "op": "despawn", "id": "root" }),
        json!({ "op": "rename", "id": "root", "to": "base" }),
        json!({ "op": "set_resource", "name": "Exposure", "value": {} }),
    ] {
        refused(
            &mut ws,
            "edit_apply",
            json!({ "scene": LAMP, "ops": [op] }),
            "prefab_edit_invalid",
        );
    }
    assert_eq!(std::fs::read_to_string(dir.join(LAMP)).unwrap(), before);
    // A good edit still works, and the scene still loads.
    ok(
        &mut ws,
        "edit_apply",
        json!({ "scene": LAMP, "ops": [{ "op": "spawn", "id": "shade", "parent": "root",
            "components": { "Transform": {} } }] }),
    );
    ok(&mut ws, "scene_get", json!({}));
}

/// Finding (control plane 2): a prefab entity an instance overrides is not
/// renamed or removed under it (the scene would no longer load).
#[test]
fn an_overridden_prefab_entity_is_not_pulled_away() {
    let (dir, mut ws) = with_lamp("prefab-orphans");
    ok(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set_override", "id": "lamp_1", "entity": "bulb",
            "component": "Material", "value": { "metallic": 1.0 } }] }),
    );
    let answer = refused(
        &mut ws,
        "edit_apply",
        json!({ "scene": LAMP, "ops": [{ "op": "rename", "id": "bulb", "to": "light" }] }),
        "prefab_override_orphaned",
    );
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("lamp_1/bulb"),
        "{answer}"
    );
    // The override dropped first, the rename works, and the scene loads
    // in a new workspace too.
    ok(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "remove_override", "id": "lamp_1", "entity": "bulb",
            "component": "Material" }] }),
    );
    ok(
        &mut ws,
        "edit_apply",
        json!({ "scene": LAMP, "ops": [{ "op": "rename", "id": "bulb", "to": "light" }] }),
    );
    let mut again = open(&dir);
    ok(&mut again, "check", json!({}));
    let got = ok(&mut again, "entity_get", json!({ "ids": ["lamp_1/light"] }));
    assert_eq!(got["entities"].as_array().unwrap().len(), 1);
}

/// Finding (control plane 3): live edits refuse what the engine writes
/// itself, and a sibling order a running game cannot honour.
#[test]
fn live_edits_refuse_engine_written_components_and_orders() {
    let dir = project("live");
    let mut ws = open(&dir);
    let session = ok(&mut ws, "sim_start", json!({}))["session"].clone();
    for op in [
        json!({ "op": "remove", "id": "ground", "component": "SceneId" }),
        json!({ "op": "set", "id": "ground", "component": "SceneId", "value": { "id": "sun" } }),
        json!({ "op": "set", "id": "ground", "component": "GlobalTransform", "value": {} }),
        json!({ "op": "spawn", "id": "x", "components": { "SceneId": { "id": "y" } } }),
    ] {
        refused(
            &mut ws,
            "edit_apply",
            json!({ "session": session, "ops": [op] }),
            "managed_component",
        );
    }
    refused(
        &mut ws,
        "edit_apply",
        json!({ "session": session, "ops": [{ "op": "reparent", "id": "sun", "parent": "ground", "order": 2 }] }),
        "live_edit_unsupported",
    );
    // The world is as it was: both still found by name.
    let got = ok(
        &mut ws,
        "entity_get",
        json!({ "session": session, "ids": ["ground", "sun"] }),
    );
    assert_eq!(got["entities"].as_array().unwrap().len(), 2);
}

/// Findings (control plane 4–6, 8): the default scene is stored exactly as
/// spelled on disk; the input map is written in its own canonical form; a
/// broken input.json does not hide the project's name.
#[test]
fn project_settings_keep_one_spelling_and_one_form() {
    let dir = project("settings");
    let mut ws = open(&dir);
    refused(
        &mut ws,
        "project_settings",
        json!({ "action": "set", "scene": "Scenes/Main.scene.json" }),
        "path_case_mismatch",
    );
    ok(
        &mut ws,
        "project_settings",
        json!({ "action": "set", "input": { "actions": { "jump": { "keys": ["Space"], "kind": "button" } }, "format": 1 } }),
    );
    let written = std::fs::read_to_string(dir.join("input.json")).unwrap();
    let canonical = wish_input::InputMap::parse(&written, "input.json")
        .unwrap()
        .to_canonical()
        .unwrap();
    assert_eq!(written, canonical);
    std::fs::write(dir.join("input.json"), "{ broken").unwrap();
    let got = ok(&mut ws, "project_settings", json!({}));
    assert_eq!(got["project"]["name"], "Review");
    assert_eq!(got["input_problem"]["code"], "invalid_input_file", "{got}");
}

/// Finding (editor 11): a bad save from a text editor, undone there, does
/// not leave the scene refusing every change.
#[test]
fn a_bad_outside_save_undone_by_hand_is_forgotten() {
    let dir = project("disk");
    let mut ws = open(&dir);
    ok(&mut ws, "scene_get", json!({}));
    let path = dir.join("scenes/main.scene.json");
    let good = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, "{ not a scene").unwrap();
    ok(&mut ws, "scene_get", json!({}));
    refused(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "rename", "id": "sun", "to": "star" }] }),
        "scene_file_invalid",
    );
    std::fs::write(&path, &good).unwrap();
    ok(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "rename", "id": "sun", "to": "star" }] }),
    );
}
