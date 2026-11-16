//! Physics through the tools (M6 WP2): a scene built with edit_apply falls
//! and settles in a session; snapshots, forks and replays of a physics
//! scene are bit for bit (M6 exit criterion 2).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("physics-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Physics", "wen").unwrap();
    let ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

/// A floor and a stack of crates, built with the tools.
fn stack(ws: &mut Workspace) {
    let mut ops = vec![json!({ "op": "spawn", "id": "floor", "components": {
        "Transform": { "translation": [0, -0.5, 0], "scale": [20, 1, 20] },
        "Primitive": { "shape": "Cube" },
        "Collider": {} } })];
    for i in 0..6 {
        ops.push(
            json!({ "op": "spawn", "id": format!("crate_{i}"), "components": {
            "Transform": { "translation": [(i % 2) as f64 * 0.3, 0.6 + i as f64 * 1.2, 0] },
            "Primitive": { "shape": "Cube" },
            "Collider": { "friction": 0.8 },
            "RigidBody": { "kind": "Dynamic" } } }),
        );
    }
    call(ws, "edit_apply", json!({ "ops": ops }));
}

fn y(ws: &mut Workspace, session: &str, id: &str) -> f64 {
    let answer = call(ws, "entity_get", json!({ "session": session, "ids": [id] }));
    answer["entities"][0]["components"]["Transform"]["translation"][1]
        .as_f64()
        .unwrap()
}

#[test]
fn a_scene_built_with_the_tools_falls_and_settles() {
    let (_dir, mut ws) = project("settle");
    stack(&mut ws);
    // The scene says where things start; physics is only in sessions.
    call(&mut ws, "check", json!({}));
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    // The floor's top is at y = 0 (a 1 m high cube scaled 20 x 1 x 20).
    let fell = call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "until": "matches(id, \"crate_5\") && Transform.translation.y < 6", "max_frames": 600 }),
    );
    assert_eq!(fell["stopped_by"], "until", "{fell}");
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 600 }));
    let low = y(&mut ws, &s, "crate_0");
    assert!(
        (low - 0.5).abs() < 0.05,
        "the bottom crate rests on the floor: {low}"
    );
    let speed = call(
        &mut ws,
        "entity_query",
        json!({ "session": s, "where": "length(BodyVelocity.linear) > 0.05" }),
    );
    assert_eq!(
        speed["entities"],
        json!([]),
        "everything is at rest: {speed}"
    );
    // The scene file did not change.
    let scene = call(&mut ws, "entity_get", json!({ "ids": ["crate_5"] }));
    assert_eq!(
        scene["entities"][0]["components"]["Transform"]["translation"][1],
        6.6
    );
}

#[test]
fn snapshots_forks_and_replays_of_physics_are_bit_for_bit() {
    let (_dir, mut ws) = project("replay");
    stack(&mut ws);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 45 }));
    call(
        &mut ws,
        "sim_snapshot",
        json!({ "session": s, "label": "mid" }),
    );
    let straight =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 200 }))["world_hash"].clone();
    // Back to the snapshot and on again: the same world.
    call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "mid" }),
    );
    let again =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 200 }))["world_hash"].clone();
    assert_eq!(again, straight);
    // A fork from the snapshot gets there too.
    let fork = call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "mid", "fork": true }),
    )["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let forked = call(
        &mut ws,
        "sim_step",
        json!({ "session": fork, "frames": 200 }),
    )["world_hash"]
        .clone();
    assert_eq!(forked, straight);
    // The recording (245 ticks, through a restore) plays back exactly.
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 355 }));
    let saved = call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "pile" }),
    );
    assert_eq!(saved["ticks"], 600, "{saved}");
    let ran = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "pile" }),
    );
    assert_eq!(ran["passed"], true, "{ran}");
}

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-physics");
        // The folder is the test's own (a cache may have left anything in it).
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let sysroot = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let rustc = sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        Toolchain { sdk, rustc }
    })
}

/// A platformer character in game code: walks with `move`, jumps with
/// `jump` when on the ground, gravity added by the code itself.
const RUNNER: &str = r#"
use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Runner {
    /// Walking speed.
    #[schema(unit = "m/s", default = 3.0)]
    pub speed: f32,
}

#[system]
fn run(
    actions: Actions,
    time: Res<Time>,
    mut runners: Query<(&Runner, &mut CharacterMotion, &CharacterState)>,
) {
    let go = actions.axis2("move");
    for (runner, motion, state) in runners.iter_mut() {
        let mut vy = motion.velocity.y - 9.81 * time.delta;
        if state.grounded {
            vy = if actions.pressed("jump") { 5.0 } else { -1.0 };
        }
        motion.velocity = Vec3::new(go.x * runner.speed, vy, -go.y * runner.speed);
    }
}

wish_sdk::export! {
    components: [Runner],
    systems: [run],
}
"#;

#[test]
fn game_code_drives_a_physics_character() {
    let (dir, mut ws) = project("runner");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(dir.join(SCRIPTS_ENTRY), RUNNER).unwrap();
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    stack(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "hero", "components": {
            "Transform": { "translation": [-5, 0.95, 0] },
            "Collider": { "shape": "Capsule", "size": [0.6, 1.8, 0.6] },
            "RigidBody": { "kind": "Kinematic" },
            "CharacterController": {},
            "CharacterMotion": {},
            "CharacterState": {},
            "Runner": {} } }] }),
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 20 }));
    let hero = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["hero"] }),
    );
    assert_eq!(
        hero["entities"][0]["components"]["CharacterState"]["grounded"], true,
        "{hero}"
    );
    // Walk right for a second: about 3 m.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 60, "input": { "move": [1, 0] } }),
    );
    let x = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["hero"] }),
    )["entities"][0]["components"]["Transform"]["translation"][0]
        .as_f64()
        .unwrap();
    assert!((x - -2.0).abs() < 0.3, "walked to {x}");
    // Jump: up, then back on the ground.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "jump": true } }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 15 }));
    let up = y(&mut ws, &s, "hero");
    assert!(up > 1.5, "in the air: {up}");
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 90 }));
    let down = y(&mut ws, &s, "hero");
    assert!((down - 0.92).abs() < 0.05, "landed: {down}");
}

#[test]
fn physics_query_answers_in_scenes_and_sessions() {
    let (_dir, mut ws) = project("query");
    stack(&mut ws);
    // In the scene file: the ray straight down from above the stack hits
    // the top crate where the scene puts it (y = 6.6, top face at 7.1).
    let down = call(
        &mut ws,
        "physics_query",
        json!({ "ray": { "from": [0.3, 20, 0], "dir": [0, -1, 0] } }),
    );
    assert_eq!(down["hit"]["id"], "crate_5", "{down}");
    assert!((down["hit"]["distance"].as_f64().unwrap() - 12.9).abs() < 1e-3);
    // In a session after the stack settled, lower.
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 300 }));
    let settled = call(
        &mut ws,
        "physics_query",
        json!({ "session": s, "ray": { "from": [0.3, 20, 0], "dir": [0, -1, 0], "ignore": "crate_5" } }),
    );
    assert_eq!(settled["hit"]["id"], "crate_4", "{settled}");
    let touching = call(
        &mut ws,
        "physics_query",
        json!({ "session": s, "touching": "crate_0" }),
    );
    assert!(
        touching["ids"]
            .as_array()
            .unwrap()
            .contains(&json!("floor")),
        "{touching}"
    );
    let near = call(
        &mut ws,
        "physics_query",
        json!({ "session": s, "overlap": { "sphere": 0.2, "at": [0, 0.5, 0] } }),
    );
    assert_eq!(near["ids"], json!(["crate_0"]), "{near}");
    // Mistakes are explained.
    let agent = parse_actor("agent");
    let two = ws.call(
        "physics_query",
        &json!({ "touching": "floor", "ray": { "from": [0, 1, 0], "dir": [0, -1, 0] } }),
        &agent,
    );
    assert_eq!(two["error"]["code"], "invalid_parameter");
    let unknown = ws.call("physics_query", &json!({ "touching": "flor" }), &agent);
    assert_eq!(unknown["error"]["code"], "entity_not_found");
    assert_eq!(unknown["error"]["hint"], "Did you mean 'floor'?");
    let bad = ws.call(
        "physics_query",
        &json!({ "ray": { "from": [0, 1], "dir": [0, -1, 0] } }),
        &agent,
    );
    assert_eq!(bad["error"]["location"]["path"], "/ray/from");
}

/// Game code that measures the height above whatever is below.
const PROBE: &str = r#"
use wish_sdk::prelude::*;

/// Measures how high it floats.
#[component]
pub struct Probe {
    /// Distance to what is below, -1 if nothing.
    pub below: f32,
    /// Ray casts so far.
    pub casts: u32,
}

#[system]
fn measure(mut probes: Query<(Entity, &Transform, &mut Probe)>) {
    for (entity, transform, probe) in probes.iter_mut() {
        let ray = Ray {
            from: transform.translation,
            dir: Vec3::new(0.0, -1.0, 0.0),
            max: 100.0,
            mask: u32::MAX,
            ignore: Some(entity),
        };
        probe.below = match wish_sdk::physics::cast(ray) {
            Some(hit) => hit.distance,
            None => -1.0,
        };
        probe.casts += 1;
    }
}

wish_sdk::export! {
    components: [Probe],
    systems: [measure],
}
"#;

#[test]
fn game_code_casts_rays_into_the_physics_world() {
    let (dir, mut ws) = project("probe");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(dir.join(SCRIPTS_ENTRY), PROBE).unwrap();
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    stack(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "probe", "components": {
                "Transform": { "translation": [5, 3, 0] }, "Probe": {} } },
            { "op": "spawn", "id": "over_crates", "components": {
                "Transform": { "translation": [0.3, 20, 0] }, "Probe": {} } }
        ] }),
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let probe = |ws: &mut Workspace, id: &str| {
        call(ws, "entity_get", json!({ "session": s, "ids": [id] }))["entities"][0]["components"]
            ["Probe"]
            .clone()
    };
    // On the very first tick the scene is already there.
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 1 }));
    let first = probe(&mut ws, "probe");
    assert!(
        (first["below"].as_f64().unwrap() - 3.0).abs() < 1e-3,
        "{first}"
    );
    let over = probe(&mut ws, "over_crates");
    assert!(
        (over["below"].as_f64().unwrap() - 12.9).abs() < 1e-3,
        "the top crate where the scene puts it: {over}"
    );
    // Later, the crates have settled lower.
    let straight =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 300 }))["world_hash"].clone();
    let later = probe(&mut ws, "over_crates");
    assert!(later["below"].as_f64().unwrap() > 13.5, "{later}");
    assert_eq!(later["casts"], 301);
    // The answers are part of a deterministic run: a replay of it holds.
    let again = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let rerun = call(
        &mut ws,
        "sim_step",
        json!({ "session": again, "frames": 301 }),
    )["world_hash"]
        .clone();
    assert_eq!(rerun, straight);
}

#[test]
fn the_inspector_explains_physics_mistakes() {
    let (_dir, mut ws) = project("inspect");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "ghost", "components": {
                "Transform": { "translation": [0, 5, 0] }, "RigidBody": { "kind": "Dynamic" } } },
            { "op": "spawn", "id": "walker", "components": {
                "Transform": {}, "Collider": { "shape": "Capsule" },
                "RigidBody": { "kind": "Dynamic" }, "CharacterController": {} } },
            { "op": "spawn", "id": "faller", "components": {
                "Transform": { "translation": [9, 2, 9] }, "Collider": {},
                "RigidBody": { "kind": "Dynamic" } } }
        ] }),
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let stepped = call(&mut ws, "sim_step", json!({ "session": s, "frames": 1200 }));
    let codes: Vec<&str> = stepped["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&"rigid_body_without_collider"), "{stepped}");
    assert!(codes.contains(&"character_not_kinematic"), "{stepped}");
    let fell = stepped["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "entity_out_of_bounds")
        .unwrap();
    assert!(
        fell["hint"].as_str().unwrap().contains("Collider"),
        "{fell}"
    );
}

#[test]
fn the_colliders_and_lights_views_come_through_the_tools() {
    let (_dir, mut ws) = project("views");
    ws.set_software(true);
    stack(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "bulb", "components": {
            "Transform": { "translation": [2, 2, 0] },
            "PointLight": { "intensity_lm": 800, "range": 4 } } }] }),
    );
    for view in ["colliders", "lights"] {
        let picture = call(
            &mut ws,
            "capture_frame",
            json!({ "view": view, "width": 128, "height": 72 }),
        );
        assert!(
            picture["image"]["data"].as_str().unwrap().len() > 100,
            "{view}"
        );
    }
    let agent = parse_actor("agent");
    let wrong = ws.call("capture_frame", &json!({ "view": "collider" }), &agent);
    assert_eq!(wrong["error"]["code"], "invalid_parameter", "{wrong}");
    assert!(wrong.to_string().contains("colliders"), "{wrong}");
}

/// A game system ordered after an engine system is refused with an
/// explanation, not left to a cycle at session start (M6 closing review).
#[test]
fn game_code_cannot_ask_to_run_after_physics() {
    let (dir, mut ws) = project("after-physics");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(
        dir.join(SCRIPTS_ENTRY),
        r#"
use wish_sdk::prelude::*;

#[system(after = "physics")]
fn late(_time: Res<Time>) {}

wish_sdk::export! {
    systems: [late],
}
"#,
    )
    .unwrap();
    let built = ws.call("script_build", &json!({}), &parse_actor("agent"));
    let error = if built["ok"] == true {
        built["diagnostics"][0].clone()
    } else {
        built["error"].clone()
    };
    assert_eq!(error["code"], "script_after_engine_system", "{built}");
    assert!(error["hint"].as_str().unwrap().contains("next tick"));
}
