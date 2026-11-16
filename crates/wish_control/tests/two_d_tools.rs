//! 2D and particles through the tools (M6 WP8): an orthographic camera,
//! sprites and an emitter set up with edit_apply, seen by describe_view and
//! counted in a session.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn project(name: &str) -> Workspace {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("two-d-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "TwoD", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir,
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "replace", "id": "camera", "component": "Camera",
              "value": { "projection": "Orthographic", "ortho_height": 10 } },
            { "op": "replace", "id": "camera", "component": "Transform",
              "value": { "translation": [0, 0, 10] } },
            { "op": "spawn", "id": "crate", "components": {
                "Transform": { "translation": [2, 1, 0] },
                "Sprite": { "rect": [0, 0, 100, 100], "color": [0.8, 0.5, 0.2, 1], "layer": 2 } } },
            { "op": "spawn", "id": "smoke", "components": {
                "Transform": { "translation": [-2, -2, 0] },
                "ParticleEmitter": { "rate": 20, "lifetime": 1 } } }
        ] }),
    );
    ws
}

#[test]
fn sprites_and_particles_are_seen_and_counted() {
    let mut ws = project("seen");
    // In the scene file, no session: the box of the crate (1 m at (2, 1):
    // 36 pixels a meter in a 640 x 360 picture).
    let scene = call(
        &mut ws,
        "describe_view",
        json!({ "width": 640, "height": 360 }),
    );
    let sprite = scene["sprites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["entity"] == "crate")
        .unwrap()
        .clone();
    assert_eq!(
        sprite["bounding_box"],
        json!([374, 126, 410, 162]),
        "{scene}"
    );
    assert_eq!(sprite["layer"], 2);
    assert_eq!(scene["particles"][0]["entity"], "smoke");

    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 60 }));
    let smoke = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["smoke"] }),
    );
    assert_eq!(
        smoke["entities"][0]["components"]["ParticleState"]["alive"], 20,
        "rate x lifetime: {smoke}"
    );
    let live = call(
        &mut ws,
        "describe_view",
        json!({ "session": s, "width": 640, "height": 360 }),
    );
    let particles = &live["particles"][0];
    assert_eq!(particles["alive"], 20, "{live}");
    assert!(particles["bounding_box"].is_array());
    // A condition on the count stops a run.
    let until = call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 5, "until": "ParticleState.alive == 20", "max_frames": 5 }),
    );
    assert_eq!(until["stopped_by"], "until", "{until}");
    // Pictures draw the 2D scene.
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "session": s, "width": 128, "height": 72 }),
    );
    assert!(picture.get("diagnostics").is_none(), "{picture}");
}

#[test]
fn a_missing_sprite_picture_is_a_warning_and_the_rest_draws() {
    let mut ws = project("missing");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "ghost", "components": {
            "Transform": {}, "Sprite": { "image": "sprites/none.png" } } }] }),
    );
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "width": 128, "height": 72 }),
    );
    let codes: Vec<&str> = picture["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(codes, ["ui_image_not_loaded"], "{picture}");
}
