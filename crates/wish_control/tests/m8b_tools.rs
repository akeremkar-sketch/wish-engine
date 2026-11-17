//! M8b tools: reasons on changes and taking one change back on its own.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn open(name: &str) -> Workspace {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("m8b-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "M8b", "wen").unwrap();
    Workspace::open(WorkspaceOptions {
        root: dir,
        example: None,
    })
    .unwrap()
}

fn call(ws: &mut Workspace, actor: &str, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor(actor))
}

#[test]
fn a_change_is_taken_back_on_its_own_with_tools() {
    let mut ws = open("revert");
    let made = call(
        &mut ws,
        "agent:helper",
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } }],
            "reason": "a crate to jump on" }),
    );
    let crate_revision = made["revision"].as_u64().unwrap();
    // The person moves the crate: the agent's card cannot be taken back
    // without undoing that.
    call(
        &mut ws,
        "human",
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "crate", "component": "Transform",
            "value": { "translation": [2, 0, 0] } }] }),
    );
    let list = call(&mut ws, "human", "edit_history", json!({}));
    let card = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["revision"] == crate_revision)
        .unwrap()
        .clone();
    assert_eq!(card["reason"], "a crate to jump on", "{card}");
    assert_eq!(card["revertible"], false);
    assert_eq!(card["not_revertible"], "revert_conflict");
    let refused = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "revert", "revision": crate_revision }),
    );
    assert_eq!(refused["error"]["code"], "revert_conflict", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("by human")
    );
    // The person's move taken back first (newest first), then the crate.
    let moved = list["entries"][0]["revision"].as_u64().unwrap();
    let back = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "revert", "revision": moved }),
    );
    assert_eq!(back["ok"], true, "{back}");
    let gone = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "revert", "revision": crate_revision, "dry_run": true }),
    );
    assert_eq!(gone["ok"], true, "{gone}");
    let gone = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "revert", "revision": crate_revision }),
    );
    assert_eq!(gone["ok"], true, "{gone}");
    let got = call(&mut ws, "human", "entity_get", json!({ "ids": ["crate"] }));
    assert_eq!(got["error"]["code"], "unknown_entity");
    let list = call(&mut ws, "human", "edit_history", json!({}));
    assert_eq!(list["entries"][0]["action"], "revert");
    assert_eq!(list["entries"][0]["reverts"], crate_revision);
    let missing = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "revert" }),
    );
    assert_eq!(missing["error"]["code"], "missing_parameter");
}

#[test]
fn the_activity_log_keeps_every_change_and_what_agents_do() {
    let mut ws = open("activity");
    // A person looking is not activity; a person changing is.
    call(&mut ws, "human", "scene_get", json!({}));
    call(
        &mut ws,
        "human",
        "edit_apply",
        json!({ "ops": [{ "op": "rename", "id": "sun", "to": "star" }] }),
    );
    // An agent looking and changing, with a reason; a failed call too.
    call(&mut ws, "agent:helper", "scene_get", json!({}));
    let edit = call(
        &mut ws,
        "agent:helper",
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } },
                         { "op": "set", "id": "crate", "component": "Transform", "value": {} }],
            "reason": "a crate to jump on" }),
    );
    call(
        &mut ws,
        "agent:helper",
        "edit_apply",
        json!({ "ops": [{ "op": "despawn", "id": "nobody" }] }),
    );
    let all = call(&mut ws, "human", "activity_get", json!({}));
    let entries = all["entries"].as_array().unwrap();
    let tools: Vec<(&str, &str)> = entries
        .iter()
        .map(|e| (e["actor"].as_str().unwrap(), e["tool"].as_str().unwrap()))
        .collect();
    assert_eq!(
        tools,
        [
            ("human", "edit_apply"),
            ("agent:helper", "scene_get"),
            ("agent:helper", "edit_apply"),
            ("agent:helper", "edit_apply"),
        ],
        "{all}"
    );
    let card = &entries[2];
    assert_eq!(card["reason"], "a crate to jump on");
    assert_eq!(card["revision"], edit["revision"]);
    assert_eq!(card["summary"], "spawn crate, set crate.Transform");
    assert_eq!(entries[3]["ok"], false);
    assert_eq!(entries[3]["error"]["code"], "unknown_entity");
    // Only what is new, only agents.
    let last = all["last_seq"].as_u64().unwrap();
    let newer = call(&mut ws, "human", "activity_get", json!({ "since": last }));
    assert_eq!(newer["entries"].as_array().unwrap().len(), 0);
    let agents = call(
        &mut ws,
        "human",
        "activity_get",
        json!({ "actor": "agents" }),
    );
    assert_eq!(agents["entries"].as_array().unwrap().len(), 3);
    // activity_get itself is not activity.
    let again = call(&mut ws, "agent:helper", "activity_get", json!({}));
    assert_eq!(again["last_seq"], last);
}

#[test]
fn a_checkpoint_brings_the_project_back_without_losing_anything() {
    let mut ws = open("checkpoint");
    ws.set_software(true);
    let saved = call(
        &mut ws,
        "human",
        "checkpoint",
        json!({ "action": "save", "note": "before the agent" }),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["checkpoint"]["name"], "cp_1");
    let before = call(
        &mut ws,
        "human",
        "capture_frame",
        json!({ "width": 64, "height": 36 }),
    );
    // The agent works: a crate, the sun renamed, a new scene, a new name.
    call(
        &mut ws,
        "agent:helper",
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "crate", "components": { "Transform": { "translation": [0, 1, 0] },
                "Primitive": { "shape": "Cube" }, "Material": { "base_color": [1, 0, 0, 1] } } },
            { "op": "rename", "id": "sun", "to": "star" }] }),
    );
    call(
        &mut ws,
        "agent:helper",
        "edit_apply",
        json!({ "scene": "scenes/extra.scene.json", "create": true,
            "ops": [{ "op": "spawn", "id": "thing", "components": { "Transform": {} } }] }),
    );
    call(
        &mut ws,
        "agent:helper",
        "project_settings",
        json!({ "action": "set", "name": "Renamed" }),
    );
    // Before and after: the checkpoint's picture is the earlier one.
    let then = call(
        &mut ws,
        "human",
        "capture_frame",
        json!({ "width": 64, "height": 36, "checkpoint": "cp_1" }),
    );
    let now = call(
        &mut ws,
        "human",
        "capture_frame",
        json!({ "width": 64, "height": 36 }),
    );
    assert_eq!(then["image"]["data"], before["image"]["data"], "{then}");
    assert_ne!(then["image"]["data"], now["image"]["data"]);
    // A dry run changes nothing.
    let dry = call(
        &mut ws,
        "human",
        "checkpoint",
        json!({ "action": "restore", "name": "cp_1", "dry_run": true }),
    );
    assert_eq!(dry["restored"].as_array().unwrap().len(), 1, "{dry}");
    let got = call(&mut ws, "human", "entity_get", json!({ "ids": ["crate"] }));
    assert_eq!(got["ok"], true);
    // Back.
    let back = call(
        &mut ws,
        "human",
        "checkpoint",
        json!({ "action": "restore", "name": "cp_1" }),
    );
    assert_eq!(back["ok"], true, "{back}");
    assert_eq!(back["kept_newer"], json!(["scenes/extra.scene.json"]));
    assert_eq!(back["settings_restored"], json!(["wish.json"]));
    let got = call(&mut ws, "human", "entity_get", json!({ "ids": ["sun"] }));
    assert_eq!(got["ok"], true, "{got}");
    let gone = call(&mut ws, "human", "entity_get", json!({ "ids": ["crate"] }));
    assert_eq!(gone["error"]["code"], "unknown_entity");
    let info = call(&mut ws, "human", "project_info", json!({}));
    assert_eq!(info["project"]["name"], "M8b");
    // Nothing is lost: going back is one change in the history, undone.
    let history = call(&mut ws, "human", "edit_history", json!({}));
    assert_eq!(
        history["entries"][0]["reason"], "back to checkpoint cp_1",
        "{history}"
    );
    let undone = call(
        &mut ws,
        "human",
        "edit_history",
        json!({ "action": "undo" }),
    );
    assert_eq!(undone["ok"], true, "{undone}");
    let again = call(
        &mut ws,
        "human",
        "entity_get",
        json!({ "ids": ["crate", "star"] }),
    );
    assert_eq!(again["ok"], true, "{again}");
    // List, refusals, delete.
    let list = call(&mut ws, "human", "checkpoint", json!({ "action": "list" }));
    assert_eq!(list["checkpoints"][0]["note"], "before the agent");
    for (args, code) in [
        (
            json!({ "action": "save", "name": "cp_1" }),
            "checkpoint_exists",
        ),
        (
            json!({ "action": "save", "name": "Bad Name" }),
            "invalid_checkpoint_name",
        ),
        (
            json!({ "action": "restore", "name": "nope" }),
            "checkpoint_not_found",
        ),
        (json!({ "action": "restore" }), "missing_parameter"),
    ] {
        let refused = call(&mut ws, "human", "checkpoint", args);
        assert_eq!(refused["error"]["code"], code, "{refused}");
    }
    let deleted = call(
        &mut ws,
        "human",
        "checkpoint",
        json!({ "action": "delete", "name": "cp_1" }),
    );
    assert_eq!(deleted["ok"], true);
    let list = call(&mut ws, "human", "checkpoint", json!({ "action": "list" }));
    assert_eq!(list["checkpoints"], json!([]));
}
