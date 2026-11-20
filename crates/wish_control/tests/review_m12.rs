#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Regression tests for the 1.0 closing review (M12): checkpoints that
//! name paths outside the project, "before" pictures drawn with the
//! material files and prefabs the checkpoint kept, restores that are all
//! or nothing and keep what they replace, and material files read with
//! their size cap everywhere.

#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{PictureSource, Workspace, WorkspaceOptions, parse_actor};

/// A fresh folder `review-m12-<name>` holding a project in `game/` (so that
/// a path that climbs out of the project still lands inside the folder).
fn folder(name: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("review-m12-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Review", "wen").unwrap();
    // No game code: these projects are about documents and looks.
    std::fs::remove_dir_all(root.join("scripts")).unwrap();
    (dir, root)
}

fn open(root: &Path) -> Workspace {
    let mut ws = Workspace::open(WorkspaceOptions {
        root: root.to_path_buf(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    ws
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("human"))
}

fn ok(answer: Value) -> Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

const MATERIAL: &str = "assets/materials/ground.mat.json";
const LAMP: &str = "prefabs/lamp.prefab.json";
const GREEN: [f64; 4] = [0.0, 1.0, 0.0, 1.0];
const RED: [f64; 4] = [1.0, 0.0, 0.0, 1.0];

fn color_material(ws: &mut Workspace, color: [f64; 4]) {
    ok(call(
        ws,
        "asset_create",
        json!({ "path": MATERIAL, "overwrite": true, "material": { "base_color": color } }),
    ));
}

/// The ground of the new project's scene, drawn with the material file.
fn ground_uses_the_material(ws: &mut Workspace) {
    ok(call(
        ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "ground", "component": "Material",
            "value": { "file": MATERIAL } }] }),
    ));
}

/// A prefab with a sphere, placed once in the scene as `lamp_1`.
fn with_lamp(root: &Path, ws: &mut Workspace) {
    std::fs::create_dir_all(root.join("prefabs")).unwrap();
    std::fs::write(
        root.join(LAMP),
        r#"{ "format": 1, "entities": {
            "root": { "components": { "Transform": {} } },
            "bulb": { "parent": "root", "components": {
                "Transform": {}, "Primitive": { "shape": "Sphere" }, "Material": {} } } } }"#,
    )
    .unwrap();
    ok(call(
        ws,
        "edit_apply",
        json!({ "ops": [{ "op": "instantiate", "id": "lamp_1", "prefab": LAMP }] }),
    ));
}

fn capture(ws: &mut Workspace, checkpoint: Option<&str>) -> Value {
    let mut args = json!({ "width": 64, "height": 36 });
    if let Some(name) = checkpoint {
        args["checkpoint"] = json!(name);
    }
    let answer = ok(call(ws, "capture_frame", args));
    answer["image"]["data"].clone()
}

/// Finding 1 (high): a checkpoint's description names the files it brings
/// back; a path in it that is not a document of the project (outside it,
/// absolute, another kind of file) refuses the whole restore, dry run
/// included, and nothing is read or written.
#[test]
fn a_checkpoint_cannot_write_outside_the_project() {
    let (dir, root) = folder("escape");
    let mut ws = open(&root);
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "evil" }),
    ));
    let kept = root.join(".wish/checkpoints/evil");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(kept.join("checkpoint.json")).unwrap())
            .unwrap();
    // A change the refused restore must not take back.
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "later", "components": { "Transform": {} } }] }),
    ));
    let payload = "{\n  \"format\": 1\n}\n";
    // What the paths would read, where the old code looked for them.
    std::fs::write(kept.join("x.mat.json"), payload).unwrap();
    std::fs::create_dir_all(kept.join("files/assets")).unwrap();
    std::fs::write(kept.join("files/assets/x.cmd"), payload).unwrap();
    std::fs::write(kept.join("files/assets/y.mat.json"), payload).unwrap();
    for bad in [
        "../x.mat.json",
        "assets/../../x.mat.json",
        "C:/x.mat.json",
        "/etc/x.mat.json",
        "//server/share/x.mat.json",
        "assets/x.cmd",
        "assets\\y.mat.json",
        "./assets/y.mat.json",
        ".wish/x.mat.json",
        "",
    ] {
        let mut evil = manifest.clone();
        let mut documents = evil["documents"].as_array().unwrap().clone();
        documents.push(json!(bad));
        evil["documents"] = json!(documents);
        std::fs::write(kept.join("checkpoint.json"), evil.to_string()).unwrap();
        for dry_run in [true, false] {
            let answer = call(
                &mut ws,
                "checkpoint",
                json!({ "action": "restore", "name": "evil", "dry_run": dry_run }),
            );
            assert_eq!(answer["ok"], false, "{bad}: {answer}");
            assert_eq!(answer["error"]["code"], "checkpoint_damaged", "{answer}");
            assert!(
                !answer["error"]["hint"].as_str().unwrap_or("").is_empty(),
                "{answer}"
            );
        }
    }
    assert!(!dir.join("x.mat.json").exists(), "nothing outside");
    assert!(!root.join("assets/x.cmd").exists());
    assert!(!root.join("assets/y.mat.json").exists());
    // The scene was not taken back either: all or nothing.
    ok(call(&mut ws, "entity_get", json!({ "ids": ["later"] })));
    // The description as it was still works.
    std::fs::write(kept.join("checkpoint.json"), manifest.to_string()).unwrap();
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "evil" }),
    ));
}

/// Finding 2 (high): the "before" picture of a checkpoint is drawn with the
/// material files the checkpoint kept, not the ones on disk now; and
/// drawing it does not leave the kept look in place for "now".
#[test]
fn a_checkpoint_picture_uses_the_material_files_it_kept() {
    let (_dir, root) = folder("picture-material");
    let mut ws = open(&root);
    color_material(&mut ws, GREEN);
    ground_uses_the_material(&mut ws);
    let green = capture(&mut ws, None);
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "green" }),
    ));
    color_material(&mut ws, RED);
    let red = capture(&mut ws, None);
    assert_ne!(green, red);
    let then = capture(&mut ws, Some("green"));
    assert_eq!(then, green, "the checkpoint's own material file");
    let now = capture(&mut ws, None);
    assert_eq!(now, red, "the picture of now is drawn with the file of now");
    // Through the editor's way of drawing too: the ground below the middle
    // of the picture is green in the checkpoint and red now.
    let view = wish_render::View::parse("lit").unwrap();
    let pixel = |ws: &mut Workspace, source: PictureSource<'_>| {
        let picture = ws.render_picture(source, None, 64, 36, view).unwrap();
        let at = ((34 * 64 + 32) * 4) as usize;
        let rgba = &picture.image.pixels[at..at + 4];
        (rgba[0], rgba[1])
    };
    let (r, g) = pixel(&mut ws, PictureSource::Checkpoint(None, "green"));
    assert!(g > r, "green ground in the checkpoint: {r} {g}");
    let (r, g) = pixel(&mut ws, PictureSource::Scene(None));
    assert!(r > g, "red ground now: {r} {g}");
}

/// Finding 3 (medium): a restore checks every kept document before it
/// writes anything, with the prefabs the checkpoint kept: a prefab deleted
/// since comes back together with the instance that places it.
#[test]
fn a_restore_brings_back_a_deleted_prefab_and_its_instance() {
    let (_dir, root) = folder("restore-prefab");
    let mut ws = open(&root);
    with_lamp(&root, &mut ws);
    color_material(&mut ws, GREEN);
    let material_then = std::fs::read_to_string(root.join(MATERIAL)).unwrap();
    let lamp_then = std::fs::read_to_string(root.join(LAMP)).unwrap();
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "x" }),
    ));
    color_material(&mut ws, RED);
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "despawn", "id": "lamp_1" }] }),
    ));
    std::fs::remove_file(root.join(LAMP)).unwrap();
    // The engine has seen the prefab go.
    ok(call(&mut ws, "scene_get", json!({})));
    let back = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "x" }),
    ));
    assert_eq!(
        std::fs::read_to_string(root.join(LAMP)).unwrap(),
        lamp_then,
        "{back}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(MATERIAL)).unwrap(),
        material_then
    );
    ok(call(&mut ws, "entity_get", json!({ "ids": ["lamp_1"] })));
}

/// Finding 4 (medium): what a restore writes over (material, meta and
/// project files, which have no history) is kept in the project's trash,
/// and the answer says where; a dry run keeps nothing and says what it
/// would write back (finding 9).
#[test]
fn a_restore_keeps_the_texts_it_replaces() {
    let (_dir, root) = folder("restore-trash");
    let mut ws = open(&root);
    color_material(&mut ws, GREEN);
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "a" }),
    ));
    color_material(&mut ws, RED);
    let newer = std::fs::read_to_string(root.join(MATERIAL)).unwrap();
    ok(call(
        &mut ws,
        "project_settings",
        json!({ "action": "set", "name": "Renamed" }),
    ));
    let newer_settings = std::fs::read_to_string(root.join("wish.json")).unwrap();
    let dry = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "a", "dry_run": true }),
    ));
    let restored = dry["restored"].as_array().unwrap();
    let material = restored.iter().find(|r| r["path"] == MATERIAL).unwrap();
    assert_eq!(material["would_write_back"], true, "{dry}");
    assert!(material.get("written_back").is_none(), "{dry}");
    assert!(dry.get("replaced_kept_in").is_none(), "{dry}");
    assert!(!root.join(".wish/trash/checkpoints").exists());
    let back = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "a" }),
    ));
    let kept = back["replaced_kept_in"].as_str().unwrap();
    assert!(kept.starts_with(".wish/trash/checkpoints/a-"), "{back}");
    assert_eq!(
        std::fs::read_to_string(root.join(kept).join("files").join(MATERIAL)).unwrap(),
        newer
    );
    assert_eq!(
        std::fs::read_to_string(root.join(kept).join("files/wish.json")).unwrap(),
        newer_settings
    );
    // A second restore keeps its own copy beside the first.
    color_material(&mut ws, RED);
    let again = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "a" }),
    ));
    assert_ne!(
        again["replaced_kept_in"], back["replaced_kept_in"],
        "{again}"
    );
    // The trash is not part of the project's assets.
    let listed = ok(call(&mut ws, "asset_list", json!({ "kind": "material" })));
    assert_eq!(listed["matched"], 1, "{listed}");
}

/// Finding 6 (low): a process whose first call draws a checkpoint of a
/// scene with prefab instances draws it, with the prefab as the
/// checkpoint kept it.
#[test]
fn a_fresh_workspace_draws_a_checkpoint_with_its_own_prefabs() {
    let (_dir, root) = folder("picture-prefab");
    let then = {
        let mut ws = open(&root);
        with_lamp(&root, &mut ws);
        let then = capture(&mut ws, None);
        ok(call(
            &mut ws,
            "checkpoint",
            json!({ "action": "save", "name": "lamp" }),
        ));
        // The prefab changes after the checkpoint.
        ok(call(
            &mut ws,
            "edit_apply",
            json!({ "scene": LAMP, "ops": [{ "op": "set", "id": "bulb", "component": "Material",
                "value": { "base_color": [1, 0, 0, 1] } }] }),
        ));
        assert_ne!(capture(&mut ws, None), then);
        then
    };
    let mut fresh = open(&root);
    assert_eq!(capture(&mut fresh, Some("lamp")), then);
}

/// Finding 7 (low): a restore that brings back a material file naming a
/// texture that is no longer there (moved since) says so.
#[test]
fn a_restore_names_the_textures_it_leaves_missing() {
    let (_dir, root) = folder("restore-moved");
    let mut ws = open(&root);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/textures/a.png",
            "picture": { "width": 8, "height": 8, "shape": "fill", "color": GREEN } }),
    ));
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": MATERIAL, "material": { "base_color_texture": "assets/textures/a.png" } }),
    ));
    ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "before" }),
    ));
    ok(call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/textures/a.png", "to": "assets/textures/b.png" }),
    ));
    let back = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "before" }),
    ));
    let warnings = back["diagnostics"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w["code"] == "asset_not_found"
            && w["location"]["file"] == MATERIAL
            && w["message"]
                .as_str()
                .unwrap()
                .contains("assets/textures/a.png")),
        "{back}"
    );
}

/// Finding 8 (low): a material file larger than any material can be is
/// refused wherever it is read whole: asset_info, a checkpoint, a migration.
#[test]
fn material_files_are_read_with_their_size_cap() {
    let (_dir, root) = folder("material-cap");
    let mut ws = open(&root);
    let path = "assets/materials/huge.mat.json";
    let mut text = String::from("{\n  \"format\": 1,\n  \"material\": {}");
    text.push_str(&" ".repeat(300 * 1024));
    text.push_str("\n}\n");
    std::fs::create_dir_all(root.join("assets/materials")).unwrap();
    std::fs::write(root.join(path), &text).unwrap();
    let info = ok(call(&mut ws, "asset_info", json!({ "path": path })));
    assert!(info.get("written").is_none(), "{info}");
    assert!(
        info.to_string().contains("material_file_too_large"),
        "{info}"
    );
    let saved = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "huge" }),
    ));
    assert!(
        saved.to_string().contains("material_file_too_large"),
        "{saved}"
    );
    assert!(
        !root
            .join(".wish/checkpoints/huge/files")
            .join(path)
            .exists()
    );
    let report = wish_control::migrate::migrate_project(
        &root,
        &[],
        wish_scene::migrate::STEPS,
        wish_scene::FORMAT,
        &wish_control::migrate::engine_defaults().unwrap(),
        false,
    )
    .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| &*d.code == "material_file_too_large"),
        "{:?}",
        report.diagnostics
    );
}
