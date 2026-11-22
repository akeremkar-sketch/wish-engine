//! The pixel-perfect camera, sorting layers and the pointer in the world
//! through the tools (release 1.1, package 6, design note
//! `docs/design/1.1-editor-2d.md` §2 to §4): `Pointer.world` in a scenario
//! test and through a window's picture (`play_step`), `describe_view` saying
//! what is under a point and naming sorting layers, and a letterboxing
//! camera's bars showing nothing.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::project::ProjectKind;
use wish_control::scaffold::create_project_of_kind;
use wish_control::{PointerAt, Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("pixel-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project_of_kind(&dir, "Pixel", "wen", ProjectKind::D2).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

/// The camera at (2, 1) with a pixel camera of 320 × 180 game pixels at 16
/// a meter (20 × 11.25 m), and two plain sprites a meter wide: `a_low` at
/// (4, 0), `b_high` at (4, 0.5) over it, both on the named, y-sorted layer
/// "actors"; and `front` with order 1 at (8, 2).
fn pixel_scene(ws: &mut Workspace) {
    call(
        ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set", "id": "camera", "component": "Transform",
              "value": { "translation": [2.0, 1.0, 10.0] } },
            { "op": "set", "id": "camera", "component": "PixelCamera",
              "value": { "resolution": [320, 180], "pixels_per_unit": 16.0 } },
            { "op": "spawn", "id": "actors_layer", "components": {
                "SortingLayer": { "name": "actors", "layer": 0, "y_sort": true } } },
            { "op": "spawn", "id": "a_low", "components": {
                "Transform": { "translation": [4.0, 0.0, 0.0] },
                "Sprite": { "color": [1.0, 0.0, 0.0, 1.0] } } },
            { "op": "spawn", "id": "b_high", "components": {
                "Transform": { "translation": [4.0, 0.5, 0.0] },
                "Sprite": { "color": [0.0, 0.0, 1.0, 1.0] } } },
            { "op": "spawn", "id": "front", "components": {
                "Transform": { "translation": [8.0, 2.0, 0.0] },
                "Sprite": { "order": 1 } } }
        ] }),
    );
}

fn write_test(dir: &Path, name: &str, steps: Value) {
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join(format!("tests/{name}.test.json")),
        serde_json::to_string_pretty(&json!({
            "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": steps
        }))
        .unwrap(),
    )
    .unwrap();
}

fn near(expr: &str, value: f32) -> String {
    format!("{expr} > {} && {expr} < {}", value - 0.001, value + 0.001)
}

/// A scenario test points at the middle of the interface's screen and at
/// its top right corner: game code reads where that is in the world.
#[test]
fn a_scenario_test_reads_the_pointer_s_place_in_the_world() {
    let (dir, mut ws) = project("scenario");
    pixel_scene(&mut ws);
    write_test(
        &dir,
        "pointer",
        json!([
            { "input": { "pointer": [640, 360] }, "frames": 1 },
            { "expect": "Pointer.in_world == true", "message": "a pointer over the screen is in the world" },
            { "expect": format!("{} && {}", near("Pointer.world.x", 2.0), near("Pointer.world.y", 1.0)),
              "message": "the middle of the screen is the camera's place" },
            { "input": { "pointer": [1280, 0] }, "frames": 1 },
            { "expect": format!("{} && {}", near("Pointer.world.x", 12.0), near("Pointer.world.y", 6.625)),
              "message": "the top right corner is 160 and 90 game pixels away, 16 a meter" },
            { "input": { "pointer": [0, 720] }, "frames": 1 },
            { "expect": format!("{} && {}", near("Pointer.world.x", -8.0), near("Pointer.world.y", -4.625)),
              "message": "the bottom left corner" }
        ]),
    );
    let ran = call(&mut ws, "test_run", json!({}));
    assert_eq!(ran["passed"], true, "{ran}");
}

/// Without a pointer there is no place in the world; a plain orthographic
/// camera (no pixel camera) maps the pointer too.
#[test]
fn the_pointer_s_world_place_under_a_plain_camera_and_without_a_pointer() {
    let (_, mut ws) = project("plain");
    let started = call(&mut ws, "sim_start", json!({ "seed": 1 }));
    let id = started["session"].as_str().unwrap().to_owned();
    let pointer = |ws: &mut Workspace| {
        call(
            ws,
            "entity_get",
            json!({ "session": id.clone(), "resources": ["Pointer"] }),
        )["resources"]["Pointer"]
            .clone()
    };
    // The 2D project's camera is 10 m tall at (0, 0): the virtual screen's
    // right edge is 10 × 16 / 9 / 2 m from the middle.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": id.clone(), "frames": 1, "input": { "pointer": [1280, 360] } }),
    );
    let got = pointer(&mut ws);
    assert_eq!(got["in_world"], true, "{got}");
    let x = got["world"][0].as_f64().unwrap();
    assert!((x - 10.0 * 16.0 / 9.0 / 2.0).abs() < 1e-3, "{got}");
    assert!(got["world"][1].as_f64().unwrap().abs() < 1e-3, "{got}");
    // The pointer leaves: it is not in the world any more.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": id.clone(), "frames": 1 }),
    );
    let got = pointer(&mut ws);
    assert_eq!(got["present"], false, "{got}");
    assert_eq!(got["in_world"], false, "{got}");
}

/// A window wider and taller than four times the game's picture: the game
/// is drawn at four, with bars around it; the pointer is mapped through the
/// game's picture, so the middle of the game is the camera's place and the
/// window's size goes into the input (and so into replays).
#[test]
fn a_window_s_pointer_is_mapped_through_the_game_s_picture() {
    let (_, mut ws) = project("window");
    pixel_scene(&mut ws);
    let started = call(&mut ws, "sim_start", json!({ "seed": 1 }));
    let id = started["session"].as_str().unwrap().to_owned();
    // 1400 × 800: scale 4, the game's 1280 × 720 at (60, 40).
    let at = |x: f32, y: f32| PointerAt {
        x,
        y,
        width: 1400.0,
        height: 800.0,
    };
    ws.play_step(&id, 1, &BTreeSet::new(), Some(at(700.0, 400.0)))
        .unwrap();
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "session": id.clone(), "resources": ["Pointer"] }),
    )["resources"]["Pointer"]
        .clone();
    let position = got["position"].as_array().unwrap();
    assert!(
        (position[0].as_f64().unwrap() - 640.0).abs() < 1e-3,
        "{got}"
    );
    assert!(
        (position[1].as_f64().unwrap() - 360.0).abs() < 1e-3,
        "{got}"
    );
    assert!(
        (got["world"][0].as_f64().unwrap() - 2.0).abs() < 1e-3,
        "{got}"
    );
    assert!(
        (got["world"][1].as_f64().unwrap() - 1.0).abs() < 1e-3,
        "{got}"
    );
    // One game pixel (four window pixels) to the right is a sixteenth of a
    // meter.
    ws.play_step(&id, 1, &BTreeSet::new(), Some(at(704.0, 400.0)))
        .unwrap();
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "session": id.clone(), "resources": ["Pointer"] }),
    )["resources"]["Pointer"]
        .clone();
    assert!(
        (got["world"][0].as_f64().unwrap() - 2.0625).abs() < 1e-3,
        "{got}"
    );
}

/// `describe_view` with a point: where it is in the world, the sprites
/// under it front first (the y-sorted layer puts the lower sprite in
/// front), the layer's name, an order; a point in the bars shows nothing.
#[test]
fn describe_view_says_what_is_under_a_point_in_2d() {
    let (_, mut ws) = project("describe");
    pixel_scene(&mut ws);
    // 640 × 360: scale 2, no bars; the camera at (320, 180); a meter is 32
    // pixels. The sprites overlap between y = 0 and 0.5 at x = 4: the
    // point (4, 0.25) is pixel (320 + 64, 180 + 24).
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "width": 640, "height": 360, "point": [384, 204] }),
    );
    let under = &seen["under_point"];
    assert_eq!(under["sprites"], json!(["a_low", "b_high"]), "{seen}");
    let world = under["world"].as_array().unwrap();
    assert!((world[0].as_f64().unwrap() - 4.0).abs() < 1e-3, "{seen}");
    assert!((world[1].as_f64().unwrap() - 0.25).abs() < 1e-3, "{seen}");
    let sprites = seen["sprites"].as_array().unwrap();
    let entry = |name: &str| {
        sprites
            .iter()
            .find(|entry| entry["entity"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(entry("a_low")["sorting_layer"], "actors", "{seen}");
    assert_eq!(entry("front")["order"], 1, "{seen}");
    assert!(entry("a_low").get("order").is_none(), "{seen}");
    // Not y-sorted: the later entity is drawn over the earlier.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [ { "op": "set", "id": "actors_layer", "component": "SortingLayer",
                           "value": { "y_sort": false } } ] }),
    );
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "width": 640, "height": 360, "point": [384, 204] }),
    );
    assert_eq!(
        seen["under_point"]["sprites"],
        json!(["b_high", "a_low"]),
        "{seen}"
    );
    // A backdrop larger than the world the camera shows. 700 × 400 is
    // still scale 2 (640 × 360 at (30, 20)): a point in the bars has no
    // sprite under it, a point inside the game's picture has the backdrop.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [ { "op": "spawn", "id": "backdrop", "components": {
            "Transform": { "translation": [2.0, 1.0, 0.0], "scale": [60.0, 60.0, 1.0] },
            "Sprite": { "layer": -10 } } } ] }),
    );
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "width": 700, "height": 400, "point": [5, 5] }),
    );
    assert_eq!(seen["under_point"]["sprites"], json!([]), "{seen}");
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "width": 700, "height": 400, "point": [40, 30] }),
    );
    assert_eq!(
        seen["under_point"]["sprites"],
        json!(["backdrop"]),
        "{seen}"
    );
}
