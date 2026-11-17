//! Regression tests for the M9 closing review: exported copies are not the
//! project's files, an export is replaced whole and never through a link,
//! names Windows keeps are refused, and a new project never replaces files.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::{create_from_template, create_project, reserved_on_windows};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("review-m9-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A project without game code, with a sound that its scene uses.
fn project(name: &str) -> (Workspace, PathBuf) {
    let dir = folder(name);
    create_project(&dir, "Review", "wen").unwrap();
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    // Any file stands for the player here (wish_player's tests run it).
    ws.set_player_path(std::env::current_exe().unwrap());
    let made = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/beep.wav", "sound": { "wave": "sine", "frequency": 440, "seconds": 0.1 } }),
    );
    assert_eq!(made["ok"], true, "{made}");
    let spawned = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "speaker", "components": { "AudioSource": { "clip": "assets/beep.wav" } } }] }),
    );
    assert_eq!(spawned["ok"], true, "{spawned}");
    (ws, dir)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent:review"))
}

#[test]
fn exported_copies_are_not_checked_moved_or_rewritten() {
    let (mut ws, dir) = project("copies");
    let exported = call(&mut ws, "project_export", json!({}));
    assert_eq!(exported["ok"], true, "{exported}");
    let copy = dir.join("exports/review/game/scenes/main.scene.json");
    let before = std::fs::read_to_string(&copy).unwrap();
    // A move rewrites the project's scene, not the exported copy.
    let moved = call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/beep.wav", "to": "assets/sounds/beep.wav" }),
    );
    assert_eq!(moved["ok"], true, "{moved}");
    assert!(!moved.to_string().contains("exports/"), "{moved}");
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), before);
    // The old copy no longer matches the project, and that is fine: check
    // looks at the project only, so exporting again works.
    assert_eq!(call(&mut ws, "check", json!({}))["passed"], true);
    let info = call(
        &mut ws,
        "asset_info",
        json!({ "path": "assets/sounds/beep.wav" }),
    );
    assert_eq!(info["used_by"], json!(["scenes/main.scene.json"]), "{info}");
    let again = call(&mut ws, "project_export", json!({ "overwrite": true }));
    assert_eq!(again["ok"], true, "{again}");
    assert!(
        std::fs::read_to_string(&copy)
            .unwrap()
            .contains("assets/sounds/beep.wav")
    );
    // Nothing of the work is left beside it.
    let left: Vec<String> = std::fs::read_dir(dir.join("exports"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["review"]);
}

#[test]
fn an_export_is_replaced_whole_and_names_windows_keeps_are_refused() {
    let (mut ws, dir) = project("replace");
    assert_eq!(call(&mut ws, "project_export", json!({}))["ok"], true);
    std::fs::write(dir.join("exports/review/extra.txt"), "x").unwrap();
    let refused = call(&mut ws, "project_export", json!({}));
    assert_eq!(refused["error"]["code"], "export_exists");
    assert!(dir.join("exports/review/extra.txt").is_file());
    let replaced = call(&mut ws, "project_export", json!({ "overwrite": true }));
    assert_eq!(replaced["ok"], true, "{replaced}");
    assert!(!dir.join("exports/review/extra.txt").exists());
    assert!(dir.join("exports/review/LICENSES.txt").is_file());
    for name in ["con", "nul", "com1", "lpt9"] {
        let bad = call(&mut ws, "project_export", json!({ "name": name }));
        assert_eq!(bad["error"]["code"], "invalid_export_name", "{name}");
    }
    assert!(reserved_on_windows("CON.txt") && reserved_on_windows("Aux"));
    assert!(!reserved_on_windows("console") && !reserved_on_windows("com10"));
}

#[cfg(windows)]
#[test]
fn an_export_is_never_written_through_a_link() {
    let (mut ws, dir) = project("link");
    let elsewhere = folder("link-target");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("keep.txt"), "mine").unwrap();
    // A junction needs no special right on Windows.
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dir.join("exports").display().to_string().replace('/', "\\"))
        .arg(elsewhere.display().to_string().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let refused = call(&mut ws, "project_export", json!({ "overwrite": true }));
    assert_eq!(
        refused["error"]["code"], "export_outside_project",
        "{refused}"
    );
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 1);
    std::fs::remove_dir(dir.join("exports")).unwrap();
}

#[test]
fn a_new_project_never_replaces_files_that_are_there() {
    let template = folder("template");
    create_project(&template, "Template", "wen").unwrap();
    std::fs::create_dir_all(template.join(".git")).unwrap();
    std::fs::write(template.join(".git/config"), "x").unwrap();
    std::fs::write(template.join("scenes/.hidden.json"), "x").unwrap();
    let dir = folder("occupied");
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(dir.join("scenes/main.scene.json"), "mine").unwrap();
    std::fs::write(dir.join("README.md"), "mine").unwrap();
    let refused = create_from_template(&dir, "New", "wen", &template).unwrap_err();
    assert_eq!(refused.code, "project_files_exist");
    assert!(refused.message.contains("scenes/main.scene.json"));
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        "mine"
    );
    assert!(!dir.join("wish.json").exists());
    let refused = create_project(&dir, "New", "wen").unwrap_err();
    assert_eq!(refused.code, "project_files_exist");
    // Other files are fine; hidden entries of a template stay behind.
    std::fs::remove_file(dir.join("scenes/main.scene.json")).unwrap();
    create_from_template(&dir, "New", "wen", &template).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("README.md")).unwrap(),
        "mine"
    );
    assert!(!dir.join(".git").exists() && !dir.join("scenes/.hidden.json").exists());
}
