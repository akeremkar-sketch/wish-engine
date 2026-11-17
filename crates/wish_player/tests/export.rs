//! M9 WP1: an exported game runs without the engine, and plays exactly as
//! the project does.
//!
//! The showcase (`examples/islands`, with game code) is copied, built and
//! exported; the exported program (the player, renamed) runs headless in its
//! own folder; the world's hash after the same ticks is the project's own,
//! bit for bit. Builds game code (the SDK pack, ~1 min the first time).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-player");
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = Command::new("rustc")
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

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        if entry.file_name() == ".wish" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

fn open(dir: &Path) -> Workspace {
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    workspace.set_player_path(PathBuf::from(env!("CARGO_BIN_EXE_wish_player")));
    workspace
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("human"))
}

/// The hash of the project's own game after `frames` ticks.
fn project_hash(ws: &mut Workspace, frames: u64) -> String {
    let started = call(ws, "sim_start", json!({}));
    assert_eq!(started["ok"], true, "{started}");
    let stepped = call(ws, "sim_step", json!({ "frames": frames }));
    call(ws, "sim_stop", json!({}));
    stepped["world_hash"].as_str().unwrap().to_owned()
}

/// Runs the exported program headless, in its own folder.
fn exported_hash(program: &Path, frames: u64) -> String {
    let out = Command::new(program)
        .args(["--headless", "--frames", &frames.to_string()])
        .current_dir(program.parent().unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    answer["world_hash"].as_str().unwrap().to_owned()
}

#[test]
fn the_showcase_exported_plays_as_the_project_does() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("export-islands");
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/islands"),
        &dir,
    );
    let mut ws = open(&dir);
    ws.set_toolchain(toolchain().clone());
    // Not built yet: refused, with how to fix it.
    let refused = call(&mut ws, "project_export", json!({}));
    assert_eq!(refused["error"]["code"], "export_needs_build", "{refused}");
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    // What would go out.
    let dry = call(&mut ws, "project_export", json!({ "dry_run": true }));
    assert_eq!(dry["ok"], true, "{dry}");
    let listed: Vec<&str> = dry["would_copy"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(listed.contains(&"scenes/main.scene.json"), "{listed:?}");
    assert!(listed.contains(&".wish/build/game.wasm"), "{listed:?}");
    assert!(listed.iter().any(|p| p.ends_with(".wav")), "{listed:?}");
    assert!(
        !listed
            .iter()
            .any(|p| p.starts_with("scripts/") || p.starts_with("tests/")),
        "no source, no tests: {listed:?}"
    );
    let exported = call(&mut ws, "project_export", json!({}));
    assert_eq!(exported["ok"], true, "{exported}");
    let folder = dir.join(exported["folder"].as_str().unwrap());
    let program = dir.join(exported["program"].as_str().unwrap());
    assert!(program.is_file(), "{exported}");
    let licenses = std::fs::read_to_string(folder.join("LICENSES.txt")).unwrap();
    assert!(licenses.contains("MIT") && licenses.contains("Apache License"));
    assert!(licenses.contains("# Third-party libraries"));
    // The exported game is not a project of its own: its copies are not the
    // project's scenes or assets.
    let assets = call(&mut ws, "asset_list", json!({}));
    assert!(!assets.to_string().contains("exports/"), "{assets}");
    // Again: refused unless asked.
    let again = call(&mut ws, "project_export", json!({}));
    assert_eq!(again["error"]["code"], "export_exists");
    // It plays exactly as the project does (the crystals' game code runs).
    let expected = project_hash(&mut ws, 300);
    assert_eq!(exported_hash(&program, 300), expected);
    // And as the project after more play: still the same.
    let later = project_hash(&mut ws, 900);
    assert_eq!(exported_hash(&program, 900), later);
}

#[test]
fn a_project_with_errors_or_a_bad_name_is_not_exported() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("export-refused");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Refused", "wen").unwrap();
    // A project without game code.
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut ws = open(&dir);
    let bad = call(&mut ws, "project_export", json!({ "name": "Bad Name" }));
    assert_eq!(bad["error"]["code"], "invalid_export_name");
    std::fs::write(
        dir.join("scenes/main.scene.json"),
        r#"{ "format": 1, "entities": { "x": { "components": { "Nonsense": {} } } } }"#,
    )
    .unwrap();
    let mut ws = open(&dir);
    let errors = call(&mut ws, "project_export", json!({}));
    assert_eq!(errors["error"]["code"], "export_has_errors", "{errors}");
    assert!(!errors["error"]["hint"].as_str().unwrap().is_empty());
}
