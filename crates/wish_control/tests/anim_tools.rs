//! Animation through the tools (M6 WP6): a model with a clip, played in a
//! session; finishing, pictures and replays.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str, looping: bool) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("anim-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Anim", "wen").unwrap();
    std::fs::create_dir_all(dir.join("assets/models")).unwrap();
    std::fs::write(
        dir.join("assets/models/bar.gltf"),
        wish_import::samples::bending_bar(),
    )
    .unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "bar", "components": {
            "Transform": {},
            "Model": { "asset": "assets/models/bar.gltf" },
            "AnimationPlayer": { "clip": "bend", "looping": looping } } }] }),
    );
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn player(ws: &mut Workspace, session: &str) -> Value {
    call(
        ws,
        "entity_get",
        json!({ "session": session, "ids": ["bar"] }),
    )["entities"][0]["components"]["AnimationPlayer"]
        .clone()
}

#[test]
fn a_clip_plays_finishes_and_is_drawn() {
    let (_dir, mut ws) = project("finish", false);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 30 }));
    let half = player(&mut ws, &s);
    assert!(
        (half["time"].as_f64().unwrap() - 0.5).abs() < 1e-3,
        "{half}"
    );
    assert_eq!(half["playing"], true);
    let done = call(&mut ws, "sim_step", json!({ "session": s, "frames": 60 }));
    assert_eq!(done["events"]["AnimationFinished"], 1, "{done}");
    let end = player(&mut ws, &s);
    assert_eq!(end["time"], 1.0);
    assert_eq!(end["playing"], false);
    // The posed model is drawn in the session.
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "session": s, "width": 64, "height": 48 }),
    );
    assert!(
        picture.get("diagnostics").is_none_or(|d| d == &json!([])),
        "{picture}"
    );
    assert_eq!(picture["drawn"], 2, "the bar and the ground");
}

#[test]
fn a_looping_clip_replays_bit_for_bit() {
    let (_dir, mut ws) = project("loop", true);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 150 }));
    let time = player(&mut ws, &s)["time"].as_f64().unwrap();
    assert!((time - 0.5).abs() < 1e-3, "2.5 s of a 1 s clip: {time}");
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "loop" }),
    );
    let ran = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "loop" }),
    );
    assert_eq!(ran["passed"], true, "{ran}");
}
