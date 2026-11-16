//! Game code through the tools: `wen new`'s starting code builds, its
//! component is usable in scenes, sessions run it, a rebuilt module takes
//! over a running session without losing its state, a broken build keeps
//! the old code, and a reopened project still knows the game's types.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-control");
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

fn open(dir: &std::path::Path) -> Workspace {
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    ws.set_toolchain(toolchain().clone());
    ws
}

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("script-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Game", "wen").unwrap();
    let ws = open(&dir);
    (dir, ws)
}

/// The rotation angle around Y of a quaternion `[x, y, z, w]`.
fn yaw(rotation: &Value) -> f64 {
    let y = rotation[1].as_f64().unwrap();
    let w = rotation[3].as_f64().unwrap();
    2.0 * y.atan2(w)
}

fn top_yaw(ws: &mut Workspace) -> f64 {
    let agent = parse_actor("agent");
    let top = ws.call(
        "entity_get",
        &json!({ "session": "s1", "ids": ["top"] }),
        &agent,
    );
    yaw(&top["entities"][0]["components"]["Transform"]["rotation"])
}

#[test]
fn game_code_is_built_used_in_scenes_run_and_reloaded() {
    let (dir, mut ws) = project("flow");
    let agent = parse_actor("agent");
    let info = ws.call("project_info", &json!({}), &agent);
    assert_eq!(info["scripts"]["sources"], SCRIPTS_ENTRY);
    assert!(info["scripts"]["module"].is_null());

    // Build the starting code.
    let built = ws.call("script_build", &json!({}), &agent);
    assert_eq!(built["ok"], true, "{built}");
    assert_eq!(built["passed"], true, "{built}");
    assert_eq!(built["systems"], json!(["spin"]));
    let info = ws.call("project_info", &json!({}), &agent);
    assert_eq!(info["scripts"]["module"]["types"], json!(["Spin"]));

    // The game's component is a scene component now, documented like any.
    let schema = ws.call(
        "schema_get",
        &json!({ "names": ["Spin"], "detail": "normal" }),
        &agent,
    );
    assert!(schema.to_string().contains("rad/s"), "{schema}");
    let edited = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "top", "components": {
            "Transform": { "translation": [0, 1, 0] }, "Spin": { "speed": 2.0 } } }] }),
        &agent,
    );
    assert_eq!(edited["ok"], true, "{edited}");
    assert_eq!(ws.call("check", &json!({}), &agent)["passed"], true);

    // It runs: 30 ticks at 2 rad/s is half a second, one radian.
    ws.call("sim_start", &json!({}), &agent);
    ws.call("sim_step", &json!({ "frames": 30 }), &agent);
    assert!(
        (top_yaw(&mut ws) - 1.0).abs() < 1e-3,
        "{}",
        top_yaw(&mut ws)
    );

    // New code, same session: it turns back now, from where it was.
    let source = std::fs::read_to_string(dir.join(SCRIPTS_ENTRY)).unwrap();
    let reversed = source
        .replace("spin.speed * time.delta", "-spin.speed * time.delta")
        .replace(
            "    for (transform, spin)",
            "    wish_sdk::info!(\"turning back\");\n    for (transform, spin)",
        );
    std::fs::write(dir.join(SCRIPTS_ENTRY), &reversed).unwrap();
    let rebuilt = ws.call("script_build", &json!({}), &agent);
    assert_eq!(rebuilt["passed"], true, "{rebuilt}");
    assert_eq!(
        rebuilt["diagnostics"][0]["code"], "script_reloaded",
        "{rebuilt}"
    );
    let stepped = ws.call("sim_step", &json!({ "frames": 30 }), &agent);
    assert!(
        stepped
            .get("diagnostics")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "{stepped}"
    );
    assert!(top_yaw(&mut ws).abs() < 1e-3, "{}", top_yaw(&mut ws));
    let logs = ws.call("logs_get", &json!({ "kinds": ["script"] }), &agent);
    assert!(logs.to_string().contains("[spin] turning back"), "{logs}");

    // A broken build: the error on its line, the old code keeps running.
    let broken = reversed.replace(
        "-spin.speed * time.delta",
        "-spin.speed * time.delta + true",
    );
    std::fs::write(dir.join(SCRIPTS_ENTRY), &broken).unwrap();
    let failed = ws.call("script_build", &json!({}), &agent);
    assert_eq!(failed["ok"], true);
    assert_eq!(failed["passed"], false);
    let first = &failed["diagnostics"][0];
    assert_eq!(first["code"], "script_compile_error", "{failed}");
    assert_eq!(first["location"]["file"], SCRIPTS_ENTRY);
    let line = broken.lines().position(|l| l.contains("+ true")).unwrap() + 1;
    assert_eq!(first["location"]["line"], line);
    ws.call("sim_step", &json!({ "frames": 30 }), &agent);
    assert!(
        (top_yaw(&mut ws) + 1.0).abs() < 1e-3,
        "the old code still runs"
    );

    // dry_run compiles and changes nothing.
    std::fs::write(dir.join(SCRIPTS_ENTRY), &source).unwrap();
    let before =
        ws.call("project_info", &json!({}), &agent)["scripts"]["module"]["fingerprint"].clone();
    let checked = ws.call("script_build", &json!({ "dry_run": true }), &agent);
    assert_eq!(checked["passed"], true, "{checked}");
    let after =
        ws.call("project_info", &json!({}), &agent)["scripts"]["module"]["fingerprint"].clone();
    assert_eq!(before, after);

    // A project opened again knows the game's types from its last build.
    let mut reopened = open(&dir);
    let spin = reopened.call("schema_get", &json!({ "names": ["Spin"] }), &agent);
    assert_eq!(spin["ok"], true, "{spin}");
    reopened.call("sim_start", &json!({}), &agent);
    reopened.call("sim_step", &json!({ "frames": 30 }), &agent);
    assert!(
        (top_yaw(&mut reopened) + 1.0).abs() < 1e-3,
        "the last good build runs"
    );
}

#[test]
fn a_project_without_game_code_says_so() {
    let (dir, mut ws) = project("none");
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let agent = parse_actor("agent");
    let answer = ws.call("script_build", &json!({}), &agent);
    assert_eq!(answer["error"]["code"], "script_sources_missing");
    assert!(ws.call("project_info", &json!({}), &agent)["scripts"]["sources"].is_null());
}

#[test]
fn a_component_that_gains_a_field_keeps_its_values_in_a_running_game() {
    let (dir, mut ws) = project("migrate");
    let agent = parse_actor("agent");
    assert_eq!(ws.call("script_build", &json!({}), &agent)["passed"], true);
    ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "top", "components": {
            "Transform": {}, "Spin": { "speed": 2.0 } } }] }),
        &agent,
    );
    ws.call("sim_start", &json!({}), &agent);
    ws.call("sim_step", &json!({ "frames": 30 }), &agent);
    let before = top_yaw(&mut ws);

    // Spin gets a second field; the speed of the running top is kept, the
    // new field starts at its default, and the game goes on from there.
    let source = std::fs::read_to_string(dir.join(SCRIPTS_ENTRY)).unwrap();
    let grown = source.replace(
        "    pub speed: f32,\n}",
        "    pub speed: f32,\n    /// How many turns it made.\n    #[schema(default = 0.0)]\n    pub turns: f32,\n}",
    );
    assert_ne!(grown, source);
    std::fs::write(dir.join(SCRIPTS_ENTRY), &grown).unwrap();
    let rebuilt = ws.call("script_build", &json!({}), &agent);
    assert_eq!(rebuilt["passed"], true, "{rebuilt}");
    let codes: Vec<&str> = rebuilt["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&"script_type_changed"), "{rebuilt}");
    assert!(codes.contains(&"script_reloaded"), "{rebuilt}");
    let top = ws.call(
        "entity_get",
        &json!({ "session": "s1", "ids": ["top"] }),
        &agent,
    );
    assert_eq!(
        top["entities"][0]["components"]["Spin"],
        json!({ "speed": 2.0, "turns": 0.0 }),
        "{top}"
    );
    ws.call("sim_step", &json!({ "frames": 30 }), &agent);
    assert!((top_yaw(&mut ws) - before - 1.0).abs() < 1e-3);
    // The scene document accepts the new field too.
    let set = ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set", "id": "top", "component": "Spin", "value": { "turns": 3.0 } }] }),
        &agent,
    );
    assert_eq!(set["ok"], true, "{set}");
}

const WALKER: &str = r#"
use wish_sdk::prelude::*;

/// Something the player walks.
#[component]
pub struct Walker {
    /// How fast it walks.
    #[schema(unit = "m/s", default = 4.0)]
    pub speed: f32,
}

#[system]
fn walk(actions: Actions, mut walkers: Query<(&mut Transform, &Walker)>, time: Res<Time>) {
    let go = actions.axis2("move");
    for (transform, walker) in walkers.iter_mut() {
        transform.translation += Vec3::new(go.x, 0.0, -go.y) * walker.speed * time.delta;
        if actions.pressed("jump") {
            transform.translation.y += 1.0;
        }
    }
}

wish_sdk::export! {
    components: [Walker],
    systems: [walk],
}
"#;

fn hero_at(ws: &mut Workspace) -> Vec<f64> {
    let agent = parse_actor("agent");
    let hero = ws.call(
        "entity_get",
        &json!({ "session": "s1", "ids": ["hero"] }),
        &agent,
    );
    hero["entities"][0]["components"]["Transform"]["translation"]
        .as_array()
        .unwrap_or_else(|| panic!("no hero position in {hero}"))
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect()
}

#[test]
fn a_game_is_played_through_actions_the_tool_gives_it() {
    let (dir, mut ws) = project("input");
    let agent = parse_actor("agent");
    std::fs::write(dir.join(SCRIPTS_ENTRY), WALKER).unwrap();
    let built = ws.call("script_build", &json!({}), &agent);
    assert_eq!(built["passed"], true, "{built}");
    ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "hero", "components": { "Transform": {}, "Walker": {} } }] }),
        &agent,
    );
    ws.call("sim_start", &json!({}), &agent);

    // Held for the ticks of the call: forward is -Z, 4 m/s for half a second.
    let walked = ws.call(
        "sim_step",
        &json!({ "frames": 30, "input": { "move": [0, 1] } }),
        &agent,
    );
    assert_eq!(walked["ok"], true, "{walked}");
    let at = hero_at(&mut ws);
    assert!((at[2] + 2.0).abs() < 1e-3 && at[0].abs() < 1e-6, "{at:?}");

    // Released afterwards: the next call without input stands still.
    ws.call("sim_step", &json!({ "frames": 10 }), &agent);
    assert!((hero_at(&mut ws)[2] + 2.0).abs() < 1e-3);

    // "pressed" is true on the first tick only, however long it is held.
    ws.call(
        "sim_step",
        &json!({ "frames": 5, "input": { "jump": true } }),
        &agent,
    );
    assert!(
        (hero_at(&mut ws)[1] - 1.0).abs() < 1e-6,
        "{:?}",
        hero_at(&mut ws)
    );
    ws.call("sim_step", &json!({ "frames": 1 }), &agent);
    ws.call(
        "sim_step",
        &json!({ "frames": 5, "input": { "jump": true } }),
        &agent,
    );
    assert!(
        (hero_at(&mut ws)[1] - 2.0).abs() < 1e-6,
        "pressed again after a release"
    );

    // The same input gives the same world, in a session of its own.
    let hash = ws.call("sim_step", &json!({ "frames": 1 }), &agent)["world_hash"].clone();
    ws.call(
        "sim_start",
        &json!({ "scene": "scenes/main.scene.json" }),
        &agent,
    );
    ws.call(
        "sim_step",
        &json!({ "session": "s2", "frames": 30, "input": { "move": [0, 1] } }),
        &agent,
    );
    ws.call(
        "sim_step",
        &json!({ "session": "s2", "frames": 10 }),
        &agent,
    );
    ws.call(
        "sim_step",
        &json!({ "session": "s2", "frames": 5, "input": { "jump": true } }),
        &agent,
    );
    ws.call("sim_step", &json!({ "session": "s2", "frames": 1 }), &agent);
    ws.call(
        "sim_step",
        &json!({ "session": "s2", "frames": 5, "input": { "jump": true } }),
        &agent,
    );
    let again =
        ws.call("sim_step", &json!({ "session": "s2", "frames": 1 }), &agent)["world_hash"].clone();
    assert_eq!(again, hash);

    // Mistakes are explained.
    let unknown = ws.call(
        "sim_step",
        &json!({ "session": "s1", "input": { "jmup": true } }),
        &agent,
    );
    assert_eq!(unknown["error"]["code"], "unknown_action");
    assert_eq!(unknown["error"]["hint"], "Did you mean 'jump'?");
    let wrong = ws.call(
        "sim_step",
        &json!({ "session": "s1", "input": { "move": "up" } }),
        &agent,
    );
    assert_eq!(wrong["error"]["code"], "invalid_parameter");

    // A broken input.json is reported by check, with its line.
    std::fs::write(
        dir.join("input.json"),
        "{ \"format\": 1, \"actions\": { \"jump\": { \"kind\": \"button\", \"keys\": [\"Spcae\"] } } }",
    )
    .unwrap();
    let checked = ws.call("check", &json!({}), &agent);
    assert_eq!(checked["passed"], false, "{checked}");
    assert_eq!(checked["diagnostics"][0]["code"], "invalid_input_file");
    assert_eq!(checked["diagnostics"][0]["hint"], "Did you mean 'Space'?");
}

#[test]
fn held_keys_drive_a_session_the_way_the_play_window_does() {
    let (dir, mut ws) = project("play");
    let agent = parse_actor("agent");
    std::fs::write(dir.join(SCRIPTS_ENTRY), WALKER).unwrap();
    assert_eq!(ws.call("script_build", &json!({}), &agent)["passed"], true);
    ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "hero", "components": { "Transform": {}, "Walker": {}, "Primitive": { "shape": "Cube" } } }
        ] }),
        &agent,
    );
    let started = ws.call("sim_start", &json!({}), &agent);
    let id = started["session"].as_str().unwrap().to_owned();
    let keys = |names: &[&str]| -> std::collections::BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    };
    // The window passes the keys it sees held; W is "move up" in input.json.
    assert!(ws.play_step(&id, 30, &keys(&["W"])).unwrap().is_empty());
    let at = hero_at(&mut ws);
    assert!((at[2] + 2.0).abs() < 1e-3, "{at:?}");
    // Keys the map does not know are ignored; no key is rest.
    ws.play_step(&id, 10, &keys(&["Q", "F9"])).unwrap();
    assert!((hero_at(&mut ws)[2] + 2.0).abs() < 1e-3);
    // What is drawn is the session's own world: the cube moved with it.
    let scene = ws.play_scene(&id).unwrap();
    assert_eq!(scene.draws.len(), 2, "the ground and the hero");
    assert_eq!(scene.cameras.len(), 1);
}
