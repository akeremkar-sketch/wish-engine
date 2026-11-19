//! Runs the `wen` binary the way people and agents do.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use serde_json::Value;

fn wen(arguments: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args(arguments)
        .output()
        .expect("wen runs");
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn wen_json(arguments: &[&str]) -> (bool, Value) {
    let (ok, stdout, _) = wen(arguments);
    (
        ok,
        serde_json::from_str(stdout.trim()).expect("stdout is one JSON document"),
    )
}

#[test]
fn run_is_deterministic_and_reports_hashes() {
    let arguments = [
        "run",
        "--example",
        "bounce",
        "--frames",
        "300",
        "--seed",
        "7",
        "--hash-every",
        "100",
        "--json",
    ];
    let (ok, first) = wen_json(&arguments);
    assert!(ok);
    assert_eq!(first["ok"], true);
    assert_eq!(first["ticks"], 300);
    assert_eq!(first["seed"], 7);
    assert!(first["entities"].as_u64().unwrap() > 0);
    assert_eq!(first["world_hash"].as_str().unwrap().len(), 32);
    assert_eq!(first["checkpoints"].as_array().unwrap().len(), 3);
    assert!(first["failures"].as_array().unwrap().is_empty());

    let (_, second) = wen_json(&arguments);
    for key in ["world_hash", "app_hash", "entities", "checkpoints"] {
        assert_eq!(first[key], second[key], "{key} differs between runs");
    }

    let (_, other_seed) = wen_json(&[
        "run",
        "--example",
        "bounce",
        "--frames",
        "300",
        "--seed",
        "8",
        "--json",
    ]);
    assert_ne!(first["app_hash"], other_seed["app_hash"]);
}

#[test]
fn text_output_for_people() {
    let (ok, stdout, _) = wen(&["run", "--example", "bounce", "--frames", "20"]);
    assert!(ok);
    assert!(
        stdout.starts_with("bounce - seed 0 - 20 ticks in "),
        "{stdout}"
    );
    assert!(stdout.contains("world hash: "));
    let (ok, stdout, _) = wen(&["examples"]);
    assert!(ok);
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["bounce", "stress"]);
}

#[test]
fn bench_reports_timings_and_a_deterministic_hash() {
    let arguments = [
        "bench",
        "--entities",
        "2000",
        "--ticks",
        "30",
        "--seed",
        "3",
        "--json",
    ];
    let (ok, first) = wen_json(&arguments);
    assert!(ok, "{first}");
    assert_eq!(first["entities"], 2000);
    assert_eq!(first["ticks"], 30);
    for key in ["mean", "median", "p95", "max"] {
        assert!(first["tick_ms"][key].as_f64().unwrap() >= 0.0, "{key}");
    }
    let (_, second) = wen_json(&arguments);
    assert_eq!(first["world_hash"], second["world_hash"]);
}

#[test]
fn schema_lists_every_registered_type() {
    let (ok, document) = wen_json(&["schema", "--example", "bounce", "--json"]);
    assert!(ok);
    assert_eq!(document["ok"], true);
    let body = &document["components"]["Body"];
    assert_eq!(body["properties"]["restitution"]["maximum"], 1.0);
    assert_eq!(body["properties"]["velocity"]["x-unit"], "m/s");
    assert!(
        document["components"]["Hierarchy"].is_object(),
        "built-in types are listed"
    );
    assert!(document["resources"]["Time"].is_object());
    assert!(document["events"]["Bounced"].is_object());

    let (_, bare) = wen_json(&["schema", "--json"]);
    assert!(
        bare["components"]["Body"].is_null(),
        "without an example only built-in types"
    );
}

/// A throwaway project folder with a prefab and a scene using the `stress`
/// example's types.
fn project(name: &str, scene: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("wen-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(root.join("prefabs")).unwrap();
    std::fs::write(
        root.join("prefabs/mover.prefab.json"),
        r#"{ "format": 1, "entities": { "root": { "components": { "Velocity": { "value": [1, 0, 0] } } }, "tag": { "parent": "root", "components": { "Spin": { "rate": 2 } } } } }"#,
    )
    .unwrap();
    std::fs::write(root.join("main.scene.json"), scene).unwrap();
    root
}

#[test]
fn run_loads_a_scene_with_its_prefabs_deterministically() {
    let root = project(
        "ok",
        r#"{ "format": 1, "entities": { "a": { "components": { "Position": { "value": [0, 1, 0] } } }, "m": { "prefab": "prefabs/mover.prefab.json" } } }"#,
    );
    let arguments = [
        "run",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        root.to_str().unwrap(),
        "--frames",
        "5",
        "--json",
    ];
    let (ok, first) = wen_json(&arguments);
    assert!(ok, "{first}");
    assert_eq!(first["scene"], "main.scene.json");
    assert_eq!(first["scene_entities"], 3);
    assert_eq!(first["entities"], 3);
    assert_eq!(first["ticks"], 5);
    let (_, second) = wen_json(&arguments);
    assert_eq!(first["world_hash"], second["world_hash"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn run_reports_the_first_scene_error_with_its_position() {
    let root = project(
        "broken",
        "{\n  \"format\": 1,\n  \"entities\": { \"a\": { \"components\": { \"Positon\": {} } } }\n}\n",
    );
    let (ok, document) = wen_json(&[
        "run",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(document["error"]["code"], "unknown_component");
    assert_eq!(document["error"]["location"]["file"], "main.scene.json");
    assert_eq!(document["error"]["location"]["line"], 3);
    assert!(
        document["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Position")
    );

    let (ok, document) = wen_json(&[
        "run",
        "--example",
        "stress",
        "--scene",
        "missing.scene.json",
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(document["error"]["code"], "file_unreadable");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mv_moves_an_asset_and_updates_the_scenes_that_use_it() {
    let root = project(
        "mv",
        r#"{ "format": 1, "entities": { "a": { "components": { "MeshRef": { "mesh": "assets/old.glb#Mesh0" } } } } }"#,
    );
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/old.glb"), "data").unwrap();
    let path = root.to_str().unwrap();
    let base = [
        "mv",
        "assets/old.glb",
        "assets/new/moved.glb",
        "--example",
        "stress",
        "--root",
        path,
        "--json",
    ];

    let mut dry = base.to_vec();
    dry.push("--dry-run");
    let (ok, report) = wen_json(&dry);
    assert!(ok, "{report}");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["documents"][0]["file"], "main.scene.json");
    assert_eq!(report["documents"][0]["references"], 1);
    assert!(
        root.join("assets/old.glb").exists(),
        "a dry run moves nothing"
    );

    let (ok, report) = wen_json(&base);
    assert!(ok, "{report}");
    assert!(root.join("assets/new/moved.glb").exists());
    assert!(!root.join("assets/old.glb").exists());
    let scene = std::fs::read_to_string(root.join("main.scene.json")).unwrap();
    assert!(scene.contains("\"assets/new/moved.glb#Mesh0\""), "{scene}");

    let (ok, report) = wen_json(&base);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "asset_not_found");
    let (ok, report) = wen_json(&["mv", "only_one", "--example", "stress", "--json"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_arguments");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mistakes_come_back_as_diagnostics() {
    let (ok, document) = wen_json(&["rn", "--json"]);
    assert!(!ok);
    assert_eq!(document["ok"], false);
    assert_eq!(document["error"]["code"], "invalid_arguments");
    assert_eq!(document["error"]["hint"], "Did you mean 'run'?");

    let (ok, document) = wen_json(&["run", "--example", "bounse", "--json"]);
    assert!(!ok);
    assert_eq!(document["error"]["code"], "unknown_example");
    assert_eq!(document["error"]["hint"], "Did you mean 'bounce'?");

    let (ok, stdout, stderr) = wen(&["run", "--exmaple", "bounce"]);
    assert!(!ok);
    assert!(stdout.is_empty());
    assert!(stderr.contains("error[invalid_arguments]"), "{stderr}");
    assert!(
        stderr.contains("hint: Did you mean '--example'?"),
        "{stderr}"
    );
}

#[test]
fn bench_render_times_frames_and_reports_their_counts() {
    // Small, on the software adapter (the same everywhere): 40 surfaces.
    let (ok, report) = wen_json(&[
        "bench",
        "--render",
        "--software",
        "--entities",
        "40",
        "--ticks",
        "3",
        "--size",
        "160x90",
        "--json",
    ]);
    assert!(ok, "{report}");
    assert_eq!(report["workload"], "render");
    assert_eq!(report["adapter"]["software"], true);
    assert_eq!(report["frames"], 3);
    // The benchmark scene has the default look. Every surface is drawn into
    // the picture (40) and into the cascades that it can shadow (four
    // cascades; a surface counts once for each one it is drawn into); the
    // sky, tone mapping and FXAA are a draw call each, and bloom is two for
    // each of its four levels in a picture 90 pixels high.
    let casters = report["stats"]["shadow_casters"].as_u64().unwrap();
    assert!((40..=160).contains(&casters), "{report}");
    assert_eq!(
        report["stats"]["draw_calls"].as_u64().unwrap(),
        40 + casters + 3 + 8
    );
    assert_eq!(report["stats"]["width"], 160);
    assert_eq!(report["stats"]["height"], 90);
    assert!(report["stats"]["point_lights"].as_u64().unwrap() >= 1);
    assert!(report["frame_ms"]["mean"].as_f64().unwrap() > 0.0);
    assert!(report["first_frame_ms"].as_f64().unwrap() > 0.0);
    // The frames after the first reuse its targets and buffers.
    assert_eq!(
        report["created_after_first_frame"],
        serde_json::json!({ "textures": 0, "buffers": 0 })
    );
    // The text form says the same, for people.
    let (ok, text, _) = wen(&[
        "bench",
        "--render",
        "--software",
        "--entities",
        "10",
        "--ticks",
        "2",
        "--size",
        "96x64",
    ]);
    assert!(ok, "{text}");
    assert!(
        text.contains("render - 10 draws - 96x64 - 2 frames"),
        "{text}"
    );
    // Ten surfaces in the picture and each in all four cascades, the sky,
    // tone mapping, FXAA, and eight passes of bloom.
    assert!(text.contains("61 draw calls"), "{text}");
    assert!(text.contains("40 shadow casters"), "{text}");
    assert!(
        text.contains("created after the first frame: 0 textures, 0 buffers"),
        "{text}"
    );
    // One measurement at a time.
    let (ok, _, stderr) = wen(&["bench", "--render", "--script"]);
    assert!(!ok);
    assert!(stderr.contains("--script or --render"), "{stderr}");
}
