//! `wen check`, `fmt`, `migrate` and `watch` on a project folder.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, Read};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

fn wen(arguments: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args(arguments)
        .output()
        .expect("wen runs");
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

fn wen_json(arguments: &[&str]) -> (bool, Value) {
    let (ok, stdout) = wen(arguments);
    (
        ok,
        serde_json::from_str(stdout.trim()).expect("stdout is one JSON document"),
    )
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("wen-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/a.glb"), "data").unwrap();
        Self(root)
    }

    fn write(&self, file: &str, text: &str) {
        // Atomic, like an editor's save: a watcher must never see a half-written file.
        let path = self.0.join(file);
        let part = self.0.join(format!("{file}.part"));
        std::fs::write(&part, text).unwrap();
        std::fs::rename(&part, &path).unwrap();
    }

    fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.0.join(file)).unwrap()
    }

    fn arg(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const GOOD: &str = "{\n  \"entities\": {\n    \"a\": {\n      \"components\": {\n        \"MeshRef\": {\n          \"mesh\": \"assets/a.glb\"\n        }\n      }\n    }\n  },\n  \"format\": 1\n}\n";
const LOOSE: &str = r#"{ "format": 1, "entities": { "a": { "components": { "MeshRef": { "mesh": "assets/a.glb" } } } } }"#;

#[test]
fn check_reports_every_problem_and_fails_only_on_errors() {
    let dir = Dir::new("check");
    dir.write("good.scene.json", GOOD);
    let args = [
        "check",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ];
    let (ok, report) = wen_json(&args);
    assert!(ok, "{report}");
    assert_eq!(report["files"], 1);
    assert_eq!(report["errors"], 0);

    // A warning (not canonical) does not fail; two errors in two files do.
    dir.write("loose.scene.json", LOOSE);
    let (ok, report) = wen_json(&args);
    assert!(ok, "{report}");
    assert_eq!(report["warnings"], 1);

    dir.write("missing.scene.json", &GOOD.replace("a.glb", "b.glb"));
    dir.write("typo.scene.json", &GOOD.replace("MeshRef", "MeshRf"));
    let (ok, report) = wen_json(&args);
    assert!(!ok);
    assert_eq!(report["passed"], false);
    assert_eq!(report["errors"], 2);
    let codes: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"asset_not_found") && codes.contains(&"unknown_component"),
        "{codes:?}"
    );
    assert!(report["diagnostics"][0]["location"]["line"].is_number());

    // A file argument that names no document fails instead of checking
    // nothing; Windows-style and ./ paths are understood.
    let (ok, report) = wen_json(&[
        "check",
        "typo.scene.jsn",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "file_not_found");
    assert_eq!(report["error"]["hint"], "Did you mean 'typo.scene.json'?");
    let (ok, report) = wen_json(&[
        "check",
        ".\\typo.scene.json",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(report["errors"], 1, "{report}");

    // Naming files limits what is reported.
    let (ok, report) = wen_json(&[
        "check",
        "good.scene.json",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(ok, "{report}");
    assert_eq!(report["files"], 1);
}

#[test]
fn render_scenes_are_checked_and_run_like_any_other() {
    let dir = Dir::new("render");
    let scene = "{\n  \"entities\": {\n    \"camera\": {\n      \"components\": {\n        \"Camera\": {\n          \"fov_y_deg\": 50\n        },\n        \"Transform\": {\n          \"translation\": [0, 2, 8]\n        }\n      }\n    },\n    \"cube\": {\n      \"components\": {\n        \"Material\": {\n          \"metallic\": 1\n        },\n        \"Primitive\": {\n          \"shape\": \"Cube\"\n        }\n      }\n    }\n  },\n  \"format\": 1,\n  \"resources\": {\n    \"Exposure\": {\n      \"ev100\": 12\n    }\n  }\n}\n";
    dir.write("main.scene.json", scene);
    let (ok, report) = wen_json(&[
        "check",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(ok, "{report}");
    assert_eq!(report["errors"], 0);
    assert_eq!(report["warnings"], 0, "{report}");

    let (ok, run) = wen_json(&[
        "run",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        dir.arg(),
        "--frames",
        "3",
        "--json",
    ]);
    assert!(ok, "{run}");
    assert_eq!(run["scene_entities"], 2);

    // Mistakes in render components get the usual positioned diagnostics.
    dir.write(
        "bad.scene.json",
        &scene.replace("50", "500").replace("\"Cube\"", "\"Cubee\""),
    );
    let (ok, report) = wen_json(&[
        "check",
        "bad.scene.json",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(!ok);
    let codes: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["value_out_of_range", "unknown_variant"], "{report}");
    assert_eq!(report["diagnostics"][0]["location"]["line"], 6);

    // The engine's types are in the schema listing.
    let (_, schema) = wen_json(&["schema", "--json"]);
    for name in [
        "Transform",
        "Camera",
        "DirectionalLight",
        "Primitive",
        "Material",
        "Model",
        "SceneId",
    ] {
        assert!(schema["components"][name].is_object(), "{name}");
    }
    assert!(schema["resources"]["Sky"].is_object());
    assert_eq!(
        schema["components"]["Camera"]["properties"]["fov_y_deg"]["maximum"],
        179.0
    );
}

#[test]
fn capture_draws_a_scene_file_into_a_png() {
    let dir = Dir::new("capture");
    dir.write(
        "main.scene.json",
        r#"{ "format": 1, "entities": {
            "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 1, 5] } } },
            "box": { "components": { "Primitive": { "shape": "Cube" }, "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } },
            "sun": { "components": { "DirectionalLight": {}, "Transform": { "rotation": [-0.38, 0, 0, 0.92] } } }
        } }"#,
    );
    let out = dir.0.join("shot.png");
    let out_arg = out.to_str().unwrap();
    let base = [
        "capture",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        dir.arg(),
        "--software",
        "--size",
        "160x120",
        "--out",
        out_arg,
        "--json",
    ];
    let (ok, report) = wen_json(&base);
    assert!(ok, "{report}");
    assert_eq!(report["camera"], "cam");
    assert_eq!(report["drawn"], 1);
    assert_eq!(
        (
            report["image"]["width"].as_u64(),
            report["image"]["height"].as_u64()
        ),
        (Some(160), Some(120))
    );
    assert_eq!(report["image"]["path"], out_arg);
    assert_eq!(report["adapter"]["software"], true);
    let png = std::fs::read(&out).unwrap();
    assert_eq!(&png[1..4], b"PNG");

    // Mistakes are explained.
    let with = |extra: &[&str]| {
        let mut arguments = base.to_vec();
        arguments.extend_from_slice(extra);
        wen_json(&arguments)
    };
    let (ok, report) = with(&["--camera", "cma"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "unknown_entity");
    assert_eq!(report["error"]["hint"], "Did you mean 'cam'?");
    let (ok, report) = with(&["--camera", "box"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "not_a_camera");
    let (ok, report) = with(&["--view", "idz"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_parameter");
    assert!(
        report["error"]["hint"]
            .as_str()
            .unwrap()
            .starts_with("Did you mean 'ids'?")
    );
    // Every debug view draws.
    for view in ["ids", "depth", "normals", "wireframe"] {
        let (ok, report) = with(&["--view", view]);
        assert!(ok, "{view}: {report}");
        assert_eq!(report["view"], view);
    }
    let (ok, report) = with(&["--size", "big"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_arguments");
    let (ok, report) = with(&["--size", "0x10"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_parameter");
}

#[test]
fn describe_lists_what_a_camera_sees() {
    let dir = Dir::new("describe");
    dir.write(
        "main.scene.json",
        r#"{ "format": 1, "entities": {
            "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 1, 8] } } },
            "wall": { "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1, 0], "scale": [3, 3, 1] } } },
            "hidden_box": { "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [0, 1, -3], "scale": [0.5, 0.5, 0.5] } } },
            "far_box": { "components": { "Primitive": { "shape": "Cube" }, "Transform": { "translation": [200, 1, 0] } } }
        } }"#,
    );
    let base = [
        "describe",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        dir.arg(),
        "--software",
        "--size",
        "160x90",
    ];
    let mut json_args = base.to_vec();
    json_args.push("--json");
    let (ok, report) = wen_json(&json_args);
    assert!(ok, "{report}");
    assert_eq!(report["camera"], "cam");
    let visible = report["visible"].as_array().unwrap();
    assert_eq!(visible.len(), 1, "{report}");
    assert_eq!(visible[0]["entity"], "wall");
    assert_eq!(visible[0]["visible_percent"], 100.0);
    assert!((visible[0]["distance"].as_f64().unwrap() - 8.0).abs() < 0.01);
    let hidden: Vec<&str> = report["hidden"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|name| name.as_str())
        .collect();
    assert_eq!(hidden, ["far_box", "hidden_box"]);

    // The text form says the same thing in words.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wen"))
        .args(base)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 visible, 2 hidden"), "{text}");
    assert!(text.contains("wall: "), "{text}");
    assert!(text.contains("far_box, hidden_box"), "{text}");

    // Mistakes are explained.
    let (ok, report) = wen_json(&[
        "describe",
        "--example",
        "stress",
        "--root",
        dir.arg(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_arguments");
    let mut wrong_camera = json_args.clone();
    wrong_camera.extend(["--camera", "cma"]);
    let (ok, report) = wen_json(&wrong_camera);
    assert!(!ok);
    assert_eq!(report["error"]["hint"], "Did you mean 'cam'?");
}

/// A one-triangle GLB with a red material, built by hand.
fn triangle_glb() -> Vec<u8> {
    let positions: Vec<u8> = [-1.0f32, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.5, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": 36 }],
        "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1.5, 0] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
        "materials": [{ "doubleSided": true, "pbrMetallicRoughness": { "baseColorFactor": [0.9, 0.1, 0.1, 1.0], "metallicFactor": 0.0 } }],
        "nodes": [{ "mesh": 0 }],
        "scenes": [{ "nodes": [0] }]
    });
    let mut text = serde_json::to_vec(&json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + positions.len();
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&positions);
    out
}

#[test]
fn capture_draws_models_from_the_projects_assets_and_reports_the_ones_it_cannot() {
    let dir = Dir::new("capture-model");
    std::fs::write(dir.0.join("assets/tri.glb"), triangle_glb()).unwrap();
    std::fs::write(dir.0.join("assets/broken.glb"), b"not a model").unwrap();
    dir.write(
        "main.scene.json",
        r#"{ "format": 1, "entities": {
            "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 0, 6] } } },
            "sun": { "components": { "DirectionalLight": {} } },
            "tri": { "components": { "Model": { "asset": "assets/tri.glb" } } },
            "bad": { "components": { "Model": { "asset": "assets/broken.glb" } } }
        } }"#,
    );
    let out = dir.0.join("model.png");
    let (ok, report) = wen_json(&[
        "capture",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        dir.arg(),
        "--software",
        "--size",
        "160x120",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(ok, "{report}");
    assert_eq!(report["drawn"], 1, "the good model is drawn: {report}");
    let warnings: Vec<&serde_json::Value> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["code"] != "not_canonical")
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "one report per broken model: {warnings:?}"
    );
    assert_eq!(warnings[0]["code"], "model_not_loaded");
    assert_eq!(warnings[0]["location"]["file"], "assets/broken.glb");
    assert!(warnings[0]["hint"].as_str().unwrap().contains("glTF"));
    assert!(out.exists());
}

#[test]
fn fmt_rewrites_to_canonical_form_and_check_mode_only_reports() {
    let dir = Dir::new("fmt");
    dir.write("loose.scene.json", LOOSE);
    dir.write("broken.scene.json", "{ \"format\": ");
    let base = ["fmt", "--example", "stress", "--root", dir.arg(), "--json"];

    let mut check = base.to_vec();
    check.push("--check");
    let (ok, report) = wen_json(&check);
    assert!(!ok);
    assert_eq!(report["changed"][0], "loose.scene.json");
    assert_eq!(report["skipped"][0], "broken.scene.json");
    assert_eq!(
        dir.read("loose.scene.json"),
        LOOSE,
        "--check changes nothing"
    );

    let (ok, report) = wen_json(&base);
    assert!(
        !ok,
        "the broken file is skipped and fails the run: {report}"
    );
    assert_eq!(dir.read("loose.scene.json"), GOOD);
    assert_eq!(
        dir.read("broken.scene.json"),
        "{ \"format\": ",
        "files with errors are left alone"
    );

    std::fs::remove_file(dir.0.join("broken.scene.json")).unwrap();
    let (ok, report) = wen_json(&check);
    assert!(ok, "now everything is canonical: {report}");
}

#[test]
fn migrate_accepts_current_files_and_refuses_what_it_cannot_upgrade() {
    let dir = Dir::new("migrate");
    dir.write("a.scene.json", GOOD);
    let (ok, report) = wen_json(&["migrate", "--root", dir.arg(), "--json"]);
    assert!(ok, "{report}");
    assert_eq!(report["up_to_date"], 1);
    assert_eq!(report["current_format"], 1);

    dir.write("old.scene.json", "{ \"format\": 0 }");
    dir.write("new.scene.json", "{ \"format\": 9 }");
    let (ok, report) = wen_json(&["migrate", "--root", dir.arg(), "--json"]);
    assert!(!ok);
    let codes: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["unsupported_format", "no_migration"]);
}

#[test]
fn watch_reloads_a_valid_change_and_keeps_the_scene_on_an_invalid_one() {
    let dir = Dir::new("watch");
    dir.write("main.scene.json", GOOD);
    let mut child = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args([
            "watch",
            "--example",
            "stress",
            "--scene",
            "main.scene.json",
            "--root",
            dir.arg(),
            "--seconds",
            "3",
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("wen runs");
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    // Edit only once the scene is loaded, so the edits are changes to it.
    let mut stdout = String::new();
    reader.read_line(&mut stdout).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let two = GOOD.replace(
        "\n  },\n  \"format\"",
        ",\n    \"b\": {}\n  },\n  \"format\"",
    );
    dir.write("main.scene.json", &two);
    std::thread::sleep(Duration::from_millis(700));
    dir.write("main.scene.json", "{ not json at all, really");
    reader.read_to_string(&mut stdout).unwrap();
    let ok = child.wait().unwrap().success();
    assert!(ok, "{stdout}");
    let events: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let names: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
    assert_eq!(names, ["loaded", "reloaded", "rejected"], "{stdout}");
    assert_eq!(events[0]["entities"], 1);
    assert_eq!(events[1]["entities"], 2);
    assert_eq!(events[2]["entities"], 2, "the last good scene stays");
    assert_eq!(events[2]["diagnostics"][0]["code"], "json_syntax");
    let last = events.last().unwrap();
    assert_eq!(last["ok"], true);
    assert_eq!(
        (last["reloads"].as_u64(), last["rejected"].as_u64()),
        (Some(1), Some(1))
    );
}

#[test]
fn shaders_are_checked_and_a_folder_of_edits_can_be_drawn_with() {
    let dir = Dir::new("shaders");
    let (ok, report) = wen_json(&["shaders", "--json"]);
    assert!(ok, "{report}");
    assert_eq!(report["problems"].as_array().unwrap().len(), 0);
    assert!(report["checked"].as_array().unwrap().len() >= 5);

    // A developer's copy with a missing semicolon.
    std::fs::write(
        dir.0.join("sky_pass.wgsl"),
        "@fragment\nfn fs_main() -> @location(0) vec4<f32> {\n    let x = 1.0\n    return vec4<f32>(x);\n}\n",
    )
    .unwrap();
    let (ok, report) = wen_json(&["shaders", "--dir", dir.arg(), "--json"]);
    assert!(!ok);
    assert_eq!(report["replaced"][0], "sky_pass");
    let problem = &report["problems"][0];
    assert_eq!(problem["code"], "shader_syntax");
    assert!(
        problem["location"]["file"]
            .as_str()
            .unwrap()
            .ends_with("sky_pass.wgsl")
    );
    assert!(problem["location"]["line"].as_u64().unwrap() >= 3);
    assert!(!problem["hint"].as_str().unwrap().is_empty());

    // Capture refuses to draw with it and says why.
    dir.write(
        "main.scene.json",
        r#"{ "format": 1, "entities": { "cam": { "components": { "Camera": {} } } } }"#,
    );
    let out = dir.0.join("x.png");
    let (ok, report) = wen_json(&[
        "capture",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--root",
        dir.arg(),
        "--shaders",
        dir.arg(),
        "--software",
        "--size",
        "32x24",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "shader_syntax");
}

#[test]
fn view_checks_its_arguments_before_opening_a_window() {
    let (ok, report) = wen_json(&["view", "--example", "stress", "--json"]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "invalid_arguments");
    let (ok, report) = wen_json(&[
        "view",
        "--example",
        "stress",
        "--scene",
        "main.scene.json",
        "--view",
        "idz",
        "--json",
    ]);
    assert!(!ok);
    assert_eq!(report["error"]["code"], "unknown_view");
    assert_eq!(report["error"]["hint"], "Did you mean 'ids'?");
}

#[test]
fn without_a_usable_graphics_adapter_capture_explains_instead_of_crashing() {
    let dir = Dir::new("no-gpu");
    dir.write(
        "main.scene.json",
        r#"{ "format": 1, "entities": { "cam": { "components": { "Camera": {} } } } }"#,
    );
    let out = dir.0.join("never.png");
    // Asking for a backend this platform does not have leaves no adapter.
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .env("WGPU_BACKEND", "metal")
        .args([
            "capture",
            "--example",
            "stress",
            "--scene",
            "main.scene.json",
            "--root",
            dir.arg(),
            "--size",
            "32x24",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"]["code"], "no_gpu_adapter");
    assert!(
        report["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--software")
    );
    assert!(!out.exists());
}

/// A template a package brings is offered by `wen new --template`, as
/// `<package id>/<folder>`, picture and all (M10a WP5).
#[test]
fn a_package_template_makes_a_new_project() {
    let base = std::env::temp_dir().join(format!("wen-package-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // An engine packages folder with one template package.
    let package = base.join("engine/com.example.kit");
    std::fs::create_dir_all(package.join("tiny/scenes")).unwrap();
    std::fs::create_dir_all(package.join("tiny/.template")).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        r#"{ "format": 1, "id": "com.example.kit", "name": "Kit", "version": "1.0.0", "engine": ">=0.1",
            "license": "MIT", "contents": { "templates": ["tiny"] } }"#,
    )
    .unwrap();
    std::fs::write(
        package.join("tiny/wish.json"),
        r#"{ "format": 1, "name": "Tiny", "scene": "scenes/main.scene.json" }"#,
    )
    .unwrap();
    std::fs::write(
        package.join("tiny/scenes/main.scene.json"),
        r#"{ "format": 1, "entities": {} }"#,
    )
    .unwrap();
    std::fs::write(package.join("tiny/.template/thumbnail.png"), "png").unwrap();
    let out = base.join("game");
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args([
            "new",
            out.to_str().unwrap(),
            "--template",
            "com.example.kit/tiny",
            "--json",
        ])
        .env("WISH_ENGINE_PACKAGES", base.join("engine"))
        .output()
        .unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    assert_eq!(
        answer["ok"],
        true,
        "{answer} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.join("scenes/main.scene.json").is_file());
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("wish.json")).unwrap()).unwrap();
    assert_eq!(config["name"], "game");
    // The template's picture is part of the package (not of the project).
    let files = wish_control::packages::package_files(&package).unwrap();
    assert!(
        files.contains(&"tiny/.template/thumbnail.png".to_owned()),
        "{files:?}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pkg_searches_adds_updates_and_removes_from_a_folder_index() {
    let base = std::env::temp_dir().join(format!("wen-pkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let theme = |folder: &str, version: &str| {
        let package = base.join("index").join(folder);
        std::fs::create_dir_all(package.join("themes")).unwrap();
        std::fs::write(
            package.join("wish-package.json"),
            format!(
                r#"{{ "format": 1, "id": "com.example.dusk", "name": "Dusk", "version": "{version}", "engine": ">=0.1",
                    "license": "MIT", "description": "Evening colours.", "contents": {{ "themes": ["themes/dusk.json"] }} }}"#
            ),
        )
        .unwrap();
        std::fs::write(package.join("themes/dusk.json"), r#"{ "format": 1 }"#).unwrap();
    };
    theme("dusk-1", "1.0.0");
    theme("dusk-2", "1.2.0");
    let game = base.join("game");
    wish_control::scaffold::create_project(&game, "Pkg", "wen").unwrap();
    let pkg = |words: &[&str]| -> (bool, Value) {
        let mut arguments = vec!["pkg"];
        arguments.extend_from_slice(words);
        arguments.extend_from_slice(&["--index", "../index", "--json"]);
        let output = Command::new(env!("CARGO_BIN_EXE_wen"))
            .args(&arguments)
            .current_dir(&game)
            .output()
            .unwrap();
        (
            output.status.success(),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        )
    };
    let (ok, found) = pkg(&["search", "evening"]);
    assert!(ok, "{found}");
    assert_eq!(found["packages"][0]["latest"], "1.2.0", "{found}");
    let (ok, added) = pkg(&["add", "com.example.dusk", "--version", "=1.0.0"]);
    assert!(ok, "{added}");
    let (ok, updated) = pkg(&["update"]);
    assert!(ok, "{updated}");
    assert_eq!(updated["updated"][0]["to"], "1.2.0", "{updated}");
    assert!(game.join("wish.lock").is_file());
    let (ok, removed) = pkg(&["remove", "com.example.dusk"]);
    assert!(ok, "{removed}");
    assert!(!game.join("packages/com.example.dusk").exists());
    assert!(!game.join("wish.lock").exists(), "no packages, no lock");
    // People read text.
    let output = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args(["pkg", "search", "--index", "../index"])
        .current_dir(&game)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("com.example.dusk 1.2.0"), "{text}");
    let (ok, wrong) = wen(&["pkg", "fetch"]);
    assert!(!ok, "{wrong}");
    let _ = std::fs::remove_dir_all(&base);
}
