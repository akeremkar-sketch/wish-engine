//! The editor's finishing touches (M8a WP8): shortcuts, a scene that
//! cannot be shown says why and what to do, the menu bar in a narrow window.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-p-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    (app, dir)
}

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn shortcut(app: &mut EditorApp, key: &str, shift: bool) {
    play(
        app,
        json!([
            { "events": [{ "pointer": [600, 300] }, { "modifiers": { "ctrl": true, "shift": shift } }] },
            { "events": [{ "key": key, "ctrl": true, "shift": shift }] },
            { "events": [{ "key": key, "ctrl": true, "shift": shift, "up": true }, { "modifiers": {} }] },
            { "idle": 2 }
        ]),
    );
}

fn named(app: &EditorApp, label: &str) -> Option<Rect> {
    app.widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(label))
        .map(|w| w.rect)
}

#[test]
fn ctrl_z_undoes_ctrl_y_redoes_and_ctrl_p_plays() {
    let (mut app, _dir) = app("keys");
    let done = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "pillar", "components": {
            "Transform": {}, "Primitive": { "shape": "Cube" } } }] }),
    );
    assert_eq!(done["ok"], true, "{done}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert!(ids(app.rows()).contains("pillar"));
    shortcut(&mut app, "Z", false);
    assert!(!ids(app.rows()).contains("pillar"), "{}", app.status());
    shortcut(&mut app, "Y", false);
    assert!(ids(app.rows()).contains("pillar"), "{}", app.status());
    shortcut(&mut app, "Z", false);
    shortcut(&mut app, "Z", true);
    assert!(
        ids(app.rows()).contains("pillar"),
        "Ctrl+Shift+Z redoes: {}",
        app.status()
    );
    shortcut(&mut app, "P", false);
    assert!(app.playing(), "{}", app.status());
    shortcut(&mut app, "P", false);
    assert!(!app.playing(), "{}", app.status());
}

#[test]
fn a_scene_that_needs_unbuilt_game_code_says_so_and_offers_to_build() {
    // The scene uses a component of the game code, which is not built (as
    // in a project just copied or cloned).
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-p-unbuilt");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let path = dir.join("scenes/main.scene.json");
    let mut scene: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    scene["entities"]["spinner"] = json!({ "components": { "Spin": { "speed": 1.0 } } });
    std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let texts: Vec<String> = app
        .widgets()
        .iter()
        .filter_map(|w| w.value.clone().or(w.label.clone()))
        .collect();
    assert!(
        texts.iter().any(|t| t == "This scene cannot be shown"),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Spin")),
        "the message: {texts:?}"
    );
    let build = named(&app, "Build code").expect("a Build code button on the card");
    assert!(app.viewport().contains(build.center()));
    // The problems tab counts what the status bar counts.
    assert!(!app.problems().is_empty());
    let show = named(&app, "Show problems").unwrap();
    play(
        &mut app,
        json!([{ "events": [{ "click": [show.center().x, show.center().y] }] }, { "idle": 2 }]),
    );
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.value.as_deref().is_some_and(|v| v.contains("Spin"))
                || w.label.as_deref().is_some_and(|l| l.contains("Spin"))),
        "the problem is listed"
    );
    // Fixed by hand: the card goes away.
    std::fs::write(
        &path,
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"Spin\"", "\"Transform\"")
            .replace("\"speed\": 1.0", "\"translation\": [0, 1, 0]"),
    )
    .unwrap();
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert!(named(&app, "Show problems").is_none(), "{}", app.status());
    assert!(ids(app.rows()).contains("spinner"));
}

#[test]
fn the_menu_bar_wraps_in_a_narrow_window() {
    let (mut app, _dir) = app("narrow");
    run_script(
        &mut app,
        &json!({ "size": [760, 600], "steps": [{ "idle": 3 }] }),
    )
    .unwrap();
    let off: Vec<_> = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.rect.max.x > 760.0)
        .map(|w| (w.label.clone(), w.rect))
        .collect();
    assert!(off.is_empty(), "off the window: {off:?}");
    for label in ["Undo", "Snap"] {
        assert!(named(&app, label).is_some(), "{label} is shown");
    }
}
