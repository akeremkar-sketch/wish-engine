//! Tools added while building the showcase game (M7).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("m7-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "M7", "wen").unwrap();
    let ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent"))
}

/// F5: placeholder sounds and pictures, made with the tools.
#[test]
fn placeholder_sounds_and_pictures_are_made_and_used() {
    let (dir, mut ws) = project("create");
    let jump = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sounds/jump.wav", "sound": {
            "wave": "square", "frequency": 300, "end_frequency": 700, "seconds": 0.15, "release": 0.1 } }),
    );
    assert_eq!(jump["ok"], true, "{jump}");
    let bytes = std::fs::read(dir.join("assets/sounds/jump.wav")).unwrap();
    let clip = wish_audio::decode("assets/sounds/jump.wav", &bytes).unwrap();
    assert!((clip.seconds() - 0.15).abs() < 0.01);
    assert!(
        clip.samples.iter().any(|s| s.abs() > 0.3),
        "it is not silent"
    );
    // The same numbers make the same file.
    call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sounds/again.wav", "sound": {
            "wave": "square", "frequency": 300, "end_frequency": 700, "seconds": 0.15, "release": 0.1 } }),
    );
    assert_eq!(
        std::fs::read(dir.join("assets/sounds/again.wav")).unwrap(),
        bytes
    );

    let dot = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/ui/gem.png", "picture": {
            "width": 32, "height": 32, "shape": "diamond", "color": [0.3, 0.9, 1, 1] } }),
    );
    assert_eq!(dot["ok"], true, "{dot}");
    let picture = wish_import::load_png(
        &std::fs::read(dir.join("assets/ui/gem.png")).unwrap(),
        "gem.png",
    )
    .unwrap();
    assert_eq!((picture.width, picture.height), (32, 32));
    let at = |x: u32, y: u32| &picture.rgba[((y * 32 + x) * 4) as usize..][..4];
    assert_eq!(at(16, 16), &[77, 230, 255, 255], "the middle is the colour");
    assert_eq!(at(0, 0)[3], 0, "the corner is transparent");
    // The asset list knows them, and a session reads the sound's length.
    let listed = call(&mut ws, "asset_list", json!({}));
    assert!(
        listed.to_string().contains("assets/sounds/jump.wav"),
        "{listed}"
    );

    // Mistakes: an existing file, an unknown wave, a format it does not make.
    let again = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sounds/jump.wav", "sound": {} }),
    );
    assert_eq!(again["error"]["code"], "output_exists");
    let wave = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/x.wav", "sound": { "wave": "sqare" } }),
    );
    assert_eq!(wave["error"]["location"]["path"], "/sound/wave", "{wave}");
    assert!(wave["error"]["hint"].as_str().unwrap().contains("square"));
    let mp3 = call(&mut ws, "asset_create", json!({ "path": "assets/x.mp3" }));
    assert_eq!(mp3["error"]["location"]["path"], "/path");
    let outside = call(
        &mut ws,
        "asset_create",
        json!({ "path": "../outside.wav", "sound": {} }),
    );
    assert_eq!(outside["ok"], false);
    assert!(!dir.join("../outside.wav").exists());
}

/// F7: resources are read with entity_get, in the scene and in a session;
/// asking for one as a component says how.
#[test]
fn resources_are_read_with_entity_get() {
    let (_dir, mut ws) = project("resources");
    let scene = call(&mut ws, "entity_get", json!({ "resources": ["Sky"] }));
    assert_eq!(scene["ok"], true, "{scene}");
    assert!(
        scene["resources"]["Sky"]["luminance"].is_number(),
        "{scene}"
    );
    assert!(scene.get("entities").is_none());
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 30 }));
    let live = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["camera"], "resources": ["Time"] }),
    );
    assert_eq!(live["resources"]["Time"]["tick"], 29, "{live}");
    assert_eq!(live["entities"][0]["id"], "camera");
    let wrong = call(&mut ws, "entity_query", json!({ "with": ["Time"] }));
    assert_eq!(wrong["error"]["code"], "unknown_component");
    assert!(
        wrong["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("resources"),
        "{wrong}"
    );
    let unknown = call(&mut ws, "entity_get", json!({ "resources": ["Scroe"] }));
    assert_eq!(unknown["error"]["location"]["path"], "/resources/0");
    let nothing = call(&mut ws, "entity_get", json!({}));
    assert_eq!(nothing["error"]["code"], "missing_parameter");
}

/// M7 closing review: asset_create writes nothing on a dry run or a failed
/// call, and refuses what it would otherwise get silently wrong.
#[test]
fn asset_create_leaves_no_trace_and_refuses_unknown_fields() {
    let (dir, mut ws) = project("create-review");
    let dry = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/new/deep/x.wav", "dry_run": true }),
    );
    assert_eq!(dry["ok"], true, "{dry}");
    assert!(!dir.join("assets/new").exists(), "a dry run made folders");

    let typo = call(
        &mut ws,
        "asset_create",
        json!({ "path": "made/here/x.wav", "sound": { "wave": "sqare" } }),
    );
    assert_eq!(typo["ok"], false);
    assert!(!dir.join("made").exists(), "a failed call made folders");

    // A field it does not know, with the near name as a hint.
    let unknown = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sounds/long.wav", "sound": { "secons": 2 } }),
    );
    assert_eq!(unknown["error"]["code"], "invalid_parameter", "{unknown}");
    assert_eq!(unknown["error"]["location"]["path"], "/sound/secons");
    assert!(
        unknown["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("seconds"),
        "{unknown}"
    );
    // A name that is not a string.
    let number = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/ui/x.png", "picture": { "shape": 3 } }),
    );
    assert_eq!(
        number["error"]["location"]["path"], "/picture/shape",
        "{number}"
    );
    // The spec of the other kind.
    let crossed = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/ui/x.png", "sound": { "wave": "sine" } }),
    );
    assert_eq!(crossed["error"]["location"]["path"], "/sound", "{crossed}");
    assert!(!dir.join("assets/ui/x.png").exists());

    // A name that exists with other letter case.
    let made = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/sounds/jump.wav" }),
    );
    assert_eq!(made["ok"], true, "{made}");
    let cased = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/Sounds/Jump.wav", "overwrite": true }),
    );
    assert_eq!(cased["error"]["code"], "invalid_output_path", "{cased}");
    assert!(
        cased["error"]["message"]
            .as_str()
            .unwrap()
            .contains("assets/sounds/jump.wav"),
        "{cased}"
    );
}

/// M7 closing review: a resource read from its defaults says so.
#[test]
fn entity_get_says_which_resources_are_defaults() {
    let (_dir, mut ws) = project("resources-review");
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "resources": ["PhysicsSettings"] }),
    );
    assert_eq!(got["ok"], true, "{got}");
    assert!(got["resources"]["PhysicsSettings"].is_object(), "{got}");
    assert_eq!(
        got["resources_defaulted"],
        json!(["PhysicsSettings"]),
        "{got}"
    );
}

/// M8a WP5: a preview draws a scene with edits applied on a copy: what it
/// would look like, with the file, the history and the revision untouched.
#[test]
fn a_preview_shows_a_change_without_making_it() {
    use wish_control::PictureSource;
    let (dir, mut ws) = project("preview");
    ws.set_software(true);
    let added = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "marker", "components": {
            "Transform": { "translation": [0.0, 1.0, 0.0] }, "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    let file = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    let before = call(&mut ws, "edit_history", json!({ "action": "list" }));
    let still = ws
        .render_picture(
            PictureSource::Scene(None),
            None,
            320,
            180,
            wish_render::View::Lit,
        )
        .unwrap();
    // Move the cube far to the right on a copy.
    let ops = json!([{ "op": "set", "id": "marker", "component": "Transform",
                       "value": { "translation": [3.0, 1.0, 0.0] } }]);
    let moved = ws
        .render_picture(
            PictureSource::Preview(None, &ops),
            None,
            320,
            180,
            wish_render::View::Lit,
        )
        .unwrap();
    let differing = still
        .image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(moved.image.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 500,
        "the preview shows the move ({differing} pixels differ)"
    );
    // Nothing was made: same file, same history, and a plain picture is
    // still the old one.
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file
    );
    assert_eq!(
        call(&mut ws, "edit_history", json!({ "action": "list" })),
        before
    );
    let again = ws
        .render_picture(
            PictureSource::Scene(None),
            None,
            320,
            180,
            wish_render::View::Lit,
        )
        .unwrap();
    assert_eq!(again.image.pixels, still.image.pixels);
    // A preview that cannot be made is a diagnostic, not a panic.
    let bad = json!([{ "op": "set", "id": "nobody", "component": "Transform", "value": {} }]);
    let error = ws
        .render_picture(
            PictureSource::Preview(None, &bad),
            None,
            320,
            180,
            wish_render::View::Lit,
        )
        .err()
        .unwrap();
    assert!(!error.hint.is_empty());
}

/// M8a WP6: a prefab is made and edited with `edit_apply` like a scene; the
/// scenes that hold instances of it see the change at once.
#[test]
fn a_prefab_is_made_and_edited_and_its_instances_follow() {
    let (dir, mut ws) = project("prefab-edit");
    let made = call(
        &mut ws,
        "edit_apply",
        json!({ "scene": "prefabs/lamp.prefab.json", "create": true, "ops": [
            { "op": "spawn", "id": "root", "components": { "Transform": {} } },
            { "op": "spawn", "id": "bulb", "parent": "root", "components": {
                "Transform": { "translation": [0, 1, 0] },
                "Primitive": { "shape": "Sphere" }, "Material": {} } }] }),
    );
    assert_eq!(made["ok"], true, "{made}");
    assert!(dir.join("prefabs/lamp.prefab.json").exists());
    let placed = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "instantiate", "id": "lamp_1", "prefab": "prefabs/lamp.prefab.json" }] }),
    );
    assert_eq!(placed["ok"], true, "{placed}");
    let read = |ws: &mut Workspace| {
        call(
            ws,
            "entity_get",
            json!({ "ids": ["lamp_1/bulb"], "components": ["Material"] }),
        )["entities"][0]["components"]["Material"]["metallic"]
            .clone()
    };
    assert_eq!(read(&mut ws), 0.0);
    // Edit the prefab: the instance follows, with no call to refresh.
    let edited = call(
        &mut ws,
        "edit_apply",
        json!({ "scene": "prefabs/lamp.prefab.json", "ops": [
            { "op": "set", "id": "bulb", "component": "Material", "value": { "metallic": 0.8 } }] }),
    );
    assert_eq!(edited["ok"], true, "{edited}");
    assert_eq!(read(&mut ws), 0.8);
    // An instance's own override wins over the prefab, and survives the edit.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set_override", "id": "lamp_1", "entity": "bulb",
                          "component": "Material", "value": { "metallic": 0.2 } }] }),
    );
    call(
        &mut ws,
        "edit_apply",
        json!({ "scene": "prefabs/lamp.prefab.json", "ops": [
            { "op": "set", "id": "bulb", "component": "Material", "value": { "roughness": 0.9 } }] }),
    );
    assert_eq!(read(&mut ws), 0.2);
    // The prefab has its own history: undo takes the last edit back.
    let undone = call(
        &mut ws,
        "edit_history",
        json!({ "scene": "prefabs/lamp.prefab.json", "action": "undo" }),
    );
    assert_eq!(undone["ok"], true, "{undone}");
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "ids": ["lamp_1/bulb"], "components": ["Material"] }),
    );
    assert_eq!(
        got["entities"][0]["components"]["Material"]["roughness"],
        0.5
    );
    // `check` still passes.
    assert_eq!(call(&mut ws, "check", json!({}))["passed"], true);
}

/// M8a WP7 (from M5): `edit_apply` with `session` changes the running game,
/// all or nothing, and leaves the scene file and its history alone.
#[test]
fn edit_apply_changes_a_running_game_and_not_the_file() {
    let (dir, mut ws) = project("live-edit");
    let file = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    let started = call(&mut ws, "sim_start", json!({}));
    let session = started["session"].as_str().unwrap().to_owned();
    call(
        &mut ws,
        "sim_step",
        json!({ "session": session, "frames": 5 }),
    );
    let live = |ws: &mut Workspace, ops: Value| {
        call(ws, "edit_apply", json!({ "session": session, "ops": ops }))
    };
    // Set, spawn (with a parent), rename: one call, all applied.
    let done = live(
        &mut ws,
        json!([
            { "op": "set", "id": "ground", "component": "Material", "value": { "metallic": 1.0 } },
            { "op": "spawn", "id": "box", "parent": "ground", "components": {
                "Transform": { "translation": [0, 2, 0] }, "Primitive": { "shape": "Cube" } } },
            { "op": "rename", "id": "sun", "to": "lamp" }
        ]),
    );
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(done["changes"]["count"], 4);
    let got = call(
        &mut ws,
        "entity_get",
        json!({ "session": session, "ids": ["ground", "box", "lamp"] }),
    );
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(
        got["entities"][0]["components"]["Material"]["metallic"],
        1.0
    );
    assert_eq!(
        got["entities"][1]["components"]["Transform"]["translation"],
        json!([0.0, 2.0, 0.0])
    );
    // The world goes on with it.
    let stepped = call(
        &mut ws,
        "sim_step",
        json!({ "session": session, "frames": 3 }),
    );
    assert_eq!(stepped["ok"], true, "{stepped}");
    // The file and the scene's history are untouched.
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file
    );
    assert_eq!(
        call(&mut ws, "edit_history", json!({ "action": "list" }))["can_undo"],
        false
    );
    // The session's log says who changed what.
    let logs = call(
        &mut ws,
        "logs_get",
        json!({ "session": session, "kinds": ["edit"] }),
    );
    assert!(logs.to_string().contains("live_edit"), "{logs}");
    // All or nothing: the second operation fails, the first is not kept.
    let refused = live(
        &mut ws,
        json!([
            { "op": "despawn", "id": "box" },
            { "op": "set", "id": "nobody", "component": "Transform", "value": {} }
        ]),
    );
    assert_eq!(refused["error"]["code"], "unknown_entity");
    assert_eq!(refused["error"]["location"]["op"], 1);
    let still = call(
        &mut ws,
        "entity_get",
        json!({ "session": session, "ids": ["box"] }),
    );
    assert_eq!(still["ok"], true, "box is still there: {still}");
    // A dry run changes nothing.
    let dry = call(
        &mut ws,
        "edit_apply",
        json!({ "session": session, "dry_run": true, "ops": [{ "op": "despawn", "id": "box" }] }),
    );
    assert_eq!(dry["dry_run"], true, "{dry}");
    assert_eq!(
        call(
            &mut ws,
            "entity_get",
            json!({ "session": session, "ids": ["box"] })
        )["ok"],
        true
    );
    // What only means something in a file is refused, with a hint.
    let placed = live(
        &mut ws,
        json!([{ "op": "set_notes", "id": "box", "notes": "x" }]),
    );
    assert_eq!(placed["error"]["code"], "live_edit_unsupported");
    // A recording session refuses live edits.
    call(&mut ws, "sim_stop", json!({ "session": session }));
    let fresh = call(&mut ws, "sim_start", json!({}));
    let fresh = fresh["session"].as_str().unwrap().to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": fresh }),
    );
    let recording = call(
        &mut ws,
        "edit_apply",
        json!({ "session": fresh, "ops": [{ "op": "despawn", "id": "sun" }] }),
    );
    assert_eq!(recording["error"]["code"], "live_edit_while_recording");
}
