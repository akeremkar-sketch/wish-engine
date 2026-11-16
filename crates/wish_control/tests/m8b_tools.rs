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
