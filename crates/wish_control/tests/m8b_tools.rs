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
