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
