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
        std::fs::write(self.0.join(file), text).unwrap();
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
    assert_eq!(report["ok"], false);
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
