//! The 2D physics profile through the tools (release 1.1, package 4,
//! design note 1.1-physics-2d): `physics_query` asks in the plane, a
//! session runs a top-down room bit for bit, and `check` warns about what
//! only 3D physics uses.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> Workspace {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("physics-2d-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Physics 2D", "wen").unwrap();
    Workspace::open(WorkspaceOptions {
        root: dir,
        example: None,
    })
    .unwrap()
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn fails(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], false, "{tool}: {answer}");
    answer["error"].clone()
}

/// The ids of an answer, sorted by name (answers are in entity order).
fn sorted(ids: &Value) -> Vec<String> {
    let mut names: Vec<String> = ids
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn warnings<'a>(answer: &'a Value, code: &str) -> Vec<&'a Value> {
    answer["diagnostics"]
        .as_array()
        .map(|all| all.iter().filter(|d| d["code"] == code).collect())
        .unwrap_or_default()
}

/// A top-down room: four walls placed 2 m behind the plane, a crate and a
/// character in front of it.
fn room(ws: &mut Workspace) {
    let wall = |id: &str, at: [f64; 3], size: [f64; 3]| {
        json!({ "op": "spawn", "id": id, "components": {
            "Transform": { "translation": at },
            "Collider": { "size": size } } })
    };
    call(
        ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set_resource", "name": "PhysicsSettings", "value": { "profile": "TopDown2D" } },
            wall("north", [0.0, 4.5, -2.0], [12.0, 1.0, 1.0]),
            wall("south", [0.0, -4.5, -2.0], [12.0, 1.0, 1.0]),
            wall("east", [5.5, 0.0, -2.0], [1.0, 8.0, 1.0]),
            wall("west", [-5.5, 0.0, -2.0], [1.0, 8.0, 1.0]),
            { "op": "spawn", "id": "crate", "components": {
                "Transform": { "translation": [-2.0, 2.0, 1.0] },
                "Collider": { "shape": "Sphere" },
                "RigidBody": { "kind": "Dynamic", "linear_damping": 3.0 } } },
            { "op": "spawn", "id": "hero", "components": {
                "Transform": { "translation": [0.0, 0.0, 1.0] },
                "Collider": { "shape": "Sphere", "size": [0.6, 0.6, 0.6] },
                "RigidBody": { "kind": "Kinematic" },
                "CharacterController": {},
                "CharacterMotion": { "velocity": [3.0, 0.0, 0.0] } } }
        ] }),
    );
}

#[test]
fn physics_query_asks_in_the_plane() {
    let mut ws = project("query");
    room(&mut ws);
    // In the scene file: a ray given as [x, y], at no depth, meets the east
    // wall 2 m behind; the answer names the profile.
    let hit = call(
        &mut ws,
        "physics_query",
        json!({ "ray": { "from": [0, 0], "dir": [1, 0], "ignore": "hero" } }),
    );
    assert_eq!(hit["profile"], "TopDown2D", "{hit}");
    assert_eq!(hit["hit"]["id"], "east", "{hit}");
    let distance = hit["hit"]["distance"].as_f64().unwrap();
    assert!((distance - 5.0).abs() < 1e-4, "{hit}");
    assert_eq!(hit["hit"]["point"][2], 0.0, "{hit}");
    // A circle is a sphere in the plane; a box may be [hx, hy].
    let found = call(
        &mut ws,
        "physics_query",
        json!({ "overlap": { "circle": 0.3, "at": [-2, 2] } }),
    );
    assert_eq!(found["ids"], json!(["crate"]), "{found}");
    let found = call(
        &mut ws,
        "physics_query",
        json!({ "overlap": { "box": [6.5, 0.2], "at": [0, 3.9] } }),
    );
    assert_eq!(sorted(&found["ids"]), ["east", "north", "west"], "{found}");
    // A ray along Z has no direction in the plane.
    let error = fails(
        &mut ws,
        "physics_query",
        json!({ "ray": { "from": [0, 0, 5], "dir": [0, 0, -1] } }),
    );
    assert_eq!(error["code"], "invalid_parameter", "{error}");
    assert!(
        error["hint"].as_str().unwrap().contains("x or y"),
        "{error}"
    );
    // One number is not a position, in 2D either.
    let error = fails(
        &mut ws,
        "physics_query",
        json!({ "overlap": { "circle": 1, "at": [3] } }),
    );
    assert_eq!(error["location"]["path"], "/overlap/at", "{error}");
    assert!(
        error["hint"].as_str().unwrap().contains("[x, y]"),
        "{error}"
    );

    // In a session: the character walks into the east wall and stops; it
    // touches it, and the touch is found in the plane.
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 120 }));
    let hero = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["hero"] }),
    );
    let components = &hero["entities"][0]["components"];
    let x = components["Transform"]["translation"][0].as_f64().unwrap();
    assert!(x > 4.6 && x < 4.71, "{hero}");
    assert_eq!(components["Transform"]["translation"][2], 1.0, "{hero}");
    assert_eq!(components["CharacterState"]["blocked"], true, "{hero}");
    assert_eq!(components["CharacterState"]["grounded"], false, "{hero}");
    let touching = call(
        &mut ws,
        "physics_query",
        json!({ "session": s, "overlap": { "circle": 0.4, "at": [x, 0] } }),
    );
    assert_eq!(sorted(&touching["ids"]), ["east", "hero"], "{touching}");
    // Seen from above nothing falls: the crate is where it was.
    let crate_ = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["crate"] }),
    );
    assert_eq!(
        crate_["entities"][0]["components"]["Transform"]["translation"],
        json!([-2.0, 2.0, 1.0]),
        "{crate_}"
    );
}

#[test]
fn a_2d_session_is_bit_for_bit_after_a_snapshot() {
    let mut ws = project("snapshot");
    room(&mut ws);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 30 }));
    call(
        &mut ws,
        "sim_snapshot",
        json!({ "session": s, "label": "mid" }),
    );
    let straight =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 150 }))["world_hash"].clone();
    call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "mid" }),
    );
    let again =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 150 }))["world_hash"].clone();
    assert_eq!(again, straight);
}

#[test]
fn check_warns_about_what_only_3d_physics_uses() {
    let mut ws = project("check");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "drum", "components": {
                "Transform": {}, "Collider": { "shape": "Cylinder" } } },
            { "op": "set_resource", "name": "PhysicsSettings", "value": { "gravity": [0, -9.81, 2] } }
        ] }),
    );
    // In 3D all of that is fine.
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        warnings(&checked, "physics_3d_in_2d").is_empty(),
        "{checked}"
    );

    // Seen from the side: the cylinder and the gravity's z part.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set_resource", "name": "PhysicsSettings",
            "value": { "gravity": [0, -9.81, 2], "profile": "SideView2D" } }] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    assert_eq!(checked["passed"], true, "warnings, not errors: {checked}");
    let found = warnings(&checked, "physics_3d_in_2d");
    let paths: Vec<&str> = found
        .iter()
        .map(|d| d["location"]["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/resources/PhysicsSettings/gravity/2",
            "/entities/drum/components/Collider/shape"
        ],
        "{checked}"
    );
    for warning in &found {
        assert_eq!(warning["severity"], "warning");
        assert!(warning["location"]["line"].as_u64().is_some(), "{warning}");
        assert!(!warning["hint"].as_str().unwrap().is_empty());
    }

    // Seen from above: any gravity written is unused.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set_resource", "name": "PhysicsSettings",
            "value": { "gravity": [0, -20, 0], "profile": "TopDown2D" } }] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    let found = warnings(&checked, "physics_3d_in_2d");
    assert_eq!(found.len(), 2, "{checked}");
    assert_eq!(
        found[0]["location"]["path"],
        "/resources/PhysicsSettings/gravity"
    );
    assert!(
        found[0]["hint"].as_str().unwrap().contains("SideView2D"),
        "{checked}"
    );
    // The code is documented.
    let docs = call(
        &mut ws,
        "docs_search",
        json!({ "query": "physics_3d_in_2d" }),
    );
    assert!(docs.to_string().contains("physics_3d_in_2d"), "{docs}");
}
