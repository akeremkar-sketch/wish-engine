//! Regression tests for the M4 closing review's findings (design note §14,
//! "M4 closing"). Each test is the review's reproduction.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-review-m4-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "R", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn scene(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap()
}

fn hand_write(dir: &Path, text: &str) {
    let part = dir.join("scenes/main.scene.json.part");
    std::fs::write(&part, text).unwrap();
    std::fs::rename(&part, dir.join("scenes/main.scene.json")).unwrap();
}

#[test]
fn h1_a_picture_never_replaces_a_document_or_an_existing_file() {
    let (dir, mut ws) = project("h1");
    let agent = parse_actor("agent");
    let before = scene(&dir);
    let project_file = std::fs::read_to_string(dir.join("wish.json")).unwrap();
    std::fs::write(dir.join("assets/texture.png"), b"not really a png").unwrap();
    for out in [
        "scenes/main.scene.json.",
        "scenes/main.scene.json ",
        "WISH.JSON",
        "wish.json",
        "CLAUDE.md",
        "assets/model.glb",
        "con.png",
        "captures/NUL.png",
        ".hidden/x.png",
        "captures/./x.png",
        "../outside.png",
        "C:/x.png",
    ] {
        let answer = ws.call(
            "capture_frame",
            &json!({ "width": 16, "height": 16, "out": out }),
            &agent,
        );
        assert_eq!(
            answer["error"]["code"], "invalid_output_path",
            "{out}: {answer}"
        );
    }
    let existing = ws.call(
        "capture_frame",
        &json!({ "width": 16, "height": 16, "out": "assets/texture.png" }),
        &agent,
    );
    assert_eq!(existing["error"]["code"], "output_exists");
    assert_eq!(
        std::fs::read(dir.join("assets/texture.png")).unwrap(),
        b"not really a png"
    );
    // A new file, then the same name again only with overwrite.
    assert_eq!(
        ws.call(
            "capture_frame",
            &json!({ "width": 16, "height": 16, "out": "captures/a.png" }),
            &agent
        )["ok"],
        true
    );
    assert_eq!(
        ws.call(
            "capture_frame",
            &json!({ "width": 16, "height": 16, "out": "captures/a.png" }),
            &agent
        )["ok"],
        false
    );
    assert_eq!(
        ws.call(
            "capture_frame",
            &json!({ "width": 16, "height": 16, "out": "captures/a.png", "overwrite": true }),
            &agent
        )["ok"],
        true
    );
    assert_eq!(scene(&dir), before);
    assert_eq!(
        std::fs::read_to_string(dir.join("wish.json")).unwrap(),
        project_file
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn h2_a_change_whose_write_failed_is_not_kept() {
    let (dir, mut ws) = project("h2");
    let agent = parse_actor("agent");
    ws.call("scene_get", &json!({}), &agent);
    // The temporary file the atomic write needs cannot be made.
    let blocker = dir.join("scenes/main.scene.json.wish-tmp");
    std::fs::create_dir_all(&blocker).unwrap();
    let failed = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ghost" }] }),
        &agent,
    );
    assert_eq!(failed["error"]["code"], "file_unwritable", "{failed}");
    let ghost = ws.call("entity_query", &json!({ "name": "ghost" }), &agent);
    assert_eq!(
        ghost["matched"], 0,
        "the failed change must not be in memory"
    );
    std::fs::remove_dir_all(&blocker).unwrap();
    assert_eq!(
        ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "spawn", "id": "real" }] }),
            &agent
        )["ok"],
        true
    );
    let text = scene(&dir);
    assert!(
        text.contains("\"real\"") && !text.contains("ghost"),
        "{text}"
    );
    // The retry of the failed change works (no duplicate id).
    assert_eq!(
        ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "spawn", "id": "ghost" }] }),
            &agent
        )["ok"],
        true
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn m1_a_refused_edit_does_not_use_up_what_others_changed() {
    let (dir, mut ws) = project("m1");
    let agent = parse_actor("agent");
    ws.call("scene_get", &json!({}), &agent);
    hand_write(
        &dir,
        &scene(&dir).replace("\"roughness\": 0.9", "\"roughness\": 0.5"),
    );
    let refused = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "nobody" }] }),
        &agent,
    );
    assert_eq!(refused["ok"], false);
    let next = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "x" }] }),
        &agent,
    );
    assert_eq!(
        next["changed_by_others"]["entities"],
        json!(["ground"]),
        "{next}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn m2_a_change_the_operations_cannot_express_keeps_revisions_counting() {
    let (dir, mut ws) = project("m2");
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join("prefabs/box.prefab.json"),
        r#"{ "format": 1, "entities": { "root": { "components": { "Primitive": { "shape": "Cube" } } } } }"#,
    )
    .unwrap();
    let agent = parse_actor("agent");
    for id in ["x1", "x2"] {
        ws.call(
            "edit_apply",
            &json!({ "ops": [{ "op": "spawn", "id": id }] }),
            &agent,
        );
    }
    let seen = ws.call("scene_get", &json!({}), &agent);
    let revision = seen["revision"].as_u64().unwrap();
    // A person turns x1 into a prefab instance (and renames nothing else).
    let mut value: Value = serde_json::from_str(&scene(&dir)).unwrap();
    value["entities"]["x1"] = json!({ "prefab": "prefabs/box.prefab.json" });
    hand_write(&dir, &serde_json::to_string_pretty(&value).unwrap());
    let after = ws.call("scene_get", &json!({}), &agent);
    assert!(after["revision"].as_u64().unwrap() > revision, "{after}");
    assert!(
        after["changed_by_others"]["entities"]
            .as_array()
            .unwrap()
            .contains(&json!("x1")),
        "{after}"
    );
    // A base revision from before is refused, not accepted by a recount.
    let stale = ws.call(
        "edit_apply",
        &json!({ "base_revision": revision, "ops": [{ "op": "spawn", "id": "y" }] }),
        &agent,
    );
    assert_eq!(stale["error"]["code"], "stale_revision");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn m3_restoring_a_snapshot_reports_the_same_problems_again() {
    let (dir, mut ws) = project("m3");
    let agent = parse_actor("agent");
    ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "faller", "components": { "Transform": { "translation": [0, -5000, 0] } } }] }),
        &agent,
    );
    ws.call("sim_start", &json!({}), &agent);
    ws.call("sim_snapshot", &json!({ "label": "start" }), &agent);
    let first = ws.call("sim_step", &json!({}), &agent);
    assert_eq!(first["diagnostics"][0]["code"], "entity_out_of_bounds");
    ws.call("sim_restore", &json!({ "snapshot": "start" }), &agent);
    let again = ws.call("sim_step", &json!({}), &agent);
    assert_eq!(again["world_hash"], first["world_hash"]);
    assert_eq!(
        again["diagnostics"], first["diagnostics"],
        "same state, same steps, same reports"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn m5_one_file_is_one_document() {
    let (dir, mut ws) = project("m5");
    let agent = parse_actor("agent");
    let dot = ws.call(
        "scene_get",
        &json!({ "scene": "scenes/./main.scene.json" }),
        &agent,
    );
    assert_eq!(dot["error"]["code"], "invalid_scene_path");
    let case = ws.call(
        "scene_get",
        &json!({ "scene": "SCENES/Main.scene.json" }),
        &agent,
    );
    assert_eq!(case["error"]["code"], "path_case_mismatch");
    assert!(
        case["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("scenes/main.scene.json")
    );
    let hidden = ws.call("edit_apply", &json!({ "scene": ".git/x.scene.json", "create": true, "ops": [{ "op": "spawn", "id": "a" }] }), &agent);
    assert_eq!(hidden["error"]["code"], "invalid_scene_path");
    assert!(!dir.join(".git/x.scene.json").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn m6_moving_the_default_scene_is_the_agents_change_and_wish_json_follows() {
    let (dir, mut ws) = project("m6");
    let agent = parse_actor("agent");
    ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "a" }] }),
        &agent,
    );
    let moved = ws.call(
        "asset_move",
        &json!({ "from": "scenes", "to": "levels" }),
        &agent,
    );
    assert_eq!(moved["ok"], true, "{moved}");
    assert_eq!(moved["diagnostics"][0]["code"], "default_scene_moved");
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("wish.json")).unwrap()).unwrap();
    assert_eq!(config["scene"], "levels/main.scene.json");
    // Working on goes to the new file; the old one does not come back.
    let next = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "b" }] }),
        &agent,
    );
    assert_eq!(next["scene"], "levels/main.scene.json", "{next}");
    assert!(next.get("changed_by_others").is_none(), "{next}");
    assert!(!dir.join("scenes/main.scene.json").exists());
    assert!(
        std::fs::read_to_string(dir.join("levels/main.scene.json"))
            .unwrap()
            .contains("\"b\"")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
