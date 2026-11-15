//! `capture_frame` and `describe_view`, on the software adapter.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-view-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "V", "wen").unwrap();
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

#[test]
fn a_picture_comes_back_as_a_png_and_can_be_written_to_a_file() {
    let (dir, mut ws) = project("capture");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
            "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
        &me,
    ));
    let shot = ok(&ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert_eq!(shot["image"]["mime"], "image/png");
    assert_eq!(
        (
            shot["image"]["width"].as_u64(),
            shot["image"]["height"].as_u64()
        ),
        (Some(160), Some(90))
    );
    assert!(
        shot["image"]["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo"),
        "a PNG header"
    );
    assert_eq!(shot["camera"], "camera");
    assert_eq!(shot["drawn"], 2);
    assert_eq!(shot["adapter"]["software"], true);
    assert_eq!(shot["view"], "lit");
    // Same call, same bytes (software adapter).
    let again = ws.call("capture_frame", &json!({ "width": 160, "height": 90 }), &me);
    assert_eq!(again["image"]["data"], shot["image"]["data"]);
    // Every view draws; written to a file instead of returned.
    for view in ["ids", "depth", "normals", "wireframe"] {
        let answer = ok(&ws.call(
            "capture_frame",
            &json!({ "width": 64, "height": 36, "view": view, "out": format!("captures/{view}.png") }),
            &me,
        ))
        .clone();
        assert!(answer["image"].get("data").is_none());
        let png = std::fs::read(dir.join(format!("captures/{view}.png"))).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
    // Mistakes.
    let bad_view = ws.call("capture_frame", &json!({ "view": "wireframes" }), &me);
    assert_eq!(
        bad_view["error"]["hint"]
            .as_str()
            .unwrap()
            .split(' ')
            .next(),
        Some("Did")
    );
    let bad_camera = ws.call("capture_frame", &json!({ "camera": "camra" }), &me);
    assert_eq!(bad_camera["error"]["hint"], "Did you mean 'camera'?");
    let not_a_camera = ws.call("capture_frame", &json!({ "camera": "ball" }), &me);
    assert_eq!(not_a_camera["error"]["code"], "not_a_camera");
    for out in [
        "../escape.png",
        "scenes/main.scene.json",
        "wish.json",
        "/abs.png",
        ".hidden/x.png",
    ] {
        let refused = ws.call("capture_frame", &json!({ "out": out }), &me);
        assert_eq!(refused["error"]["code"], "invalid_output_path", "{out}");
    }
    let huge = ws.call("capture_frame", &json!({ "width": 5000 }), &me);
    assert_eq!(huge["error"]["code"], "invalid_parameter");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn describe_view_says_what_is_visible_and_what_is_hidden() {
    let (dir, mut ws) = project("describe");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "wall", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 2, 0], "scale": [4, 3, 1] } } },
            { "op": "spawn", "id": "behind", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [0, 2, -3], "scale": [0.5, 0.5, 0.5] } } },
            { "op": "spawn", "id": "far_away", "components": { "Primitive": { "shape": "Cube" },
                "Transform": { "translation": [500, 1, 0] } } }
        ] }),
        &me,
    ));
    let seen = ok(&ws.call("describe_view", &json!({ "width": 160, "height": 90 }), &me)).clone();
    let names: Vec<&str> = seen["visible"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["entity"].as_str().unwrap())
        .collect();
    assert_eq!(names[0], "wall", "nearest first: {names:?}");
    assert!(names.contains(&"ground"));
    assert_eq!(seen["visible"][0]["visible_percent"], 100.0);
    assert_eq!(seen["hidden"], json!(["behind", "far_away"]));
    assert_eq!(seen["camera"], "camera");
    // limit lists the nearest and says how many are left out.
    let one = ok(&ws.call(
        "describe_view",
        &json!({ "width": 160, "height": 90, "limit": 1 }),
        &me,
    ))
    .clone();
    assert_eq!(one["visible"].as_array().unwrap().len(), 1);
    assert!(one["visible_not_listed"].as_u64().unwrap() >= 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_session_can_be_looked_at_while_it_runs() {
    let (dir, mut ws) = project("session");
    let me = parse_actor("claude-code");
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "ball", "components": {
        "Primitive": { "shape": "Sphere" }, "Transform": { "translation": [0, 1, 0] } } }] }),
        &me,
    ));
    ok(&ws.call("sim_start", &json!({}), &me));
    let live = ok(&ws.call(
        "capture_frame",
        &json!({ "session": "s1", "width": 64, "height": 36 }),
        &me,
    ))
    .clone();
    assert_eq!(live["session"], "s1");
    assert_eq!(live["tick"], 0);
    // The session is a copy: removing the ball from the scene does not change it.
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "ball" }] }),
        &me,
    ));
    let still = ok(&ws.call(
        "describe_view",
        &json!({ "session": "s1", "width": 160, "height": 90 }),
        &me,
    ))
    .clone();
    assert!(
        still["visible"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["entity"] == "ball")
    );
    let scene = ok(&ws.call("describe_view", &json!({ "width": 160, "height": 90 }), &me)).clone();
    assert!(
        !scene["visible"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["entity"] == "ball")
    );
    let wrong = ws.call("capture_frame", &json!({ "session": "s9" }), &me);
    assert_eq!(wrong["error"]["code"], "unknown_session");
    std::fs::remove_dir_all(&dir).unwrap();
}
