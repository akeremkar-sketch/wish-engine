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
