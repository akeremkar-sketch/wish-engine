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
    assert_eq!(stdout.trim(), "bounce");
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
