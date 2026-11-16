//! The editor's shell, headless (M8a WP2): what it shows of a project, how
//! changes made elsewhere reach it, how a click becomes a control-plane call
//! by `human`, and how the view's camera moves.
//!
//! Needs a software adapter for the scene's picture; run these in release
//! too (`cargo test --release -p wish_editor`), as the render tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_editor::raster::Canvas;
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    (EditorApp::new(workspace), dir)
}

fn play(app: &mut EditorApp, steps: Value) -> Canvas {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps }))
        .unwrap()
        .canvas
}

/// The pixel count that differs between two pictures.
fn differing(a: &Canvas, b: &Canvas) -> usize {
    a.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .filter(|(x, y)| x != y)
        .count()
}

/// Clicks an item of the project menu (the menu bar's first button, named
/// after the project).
#[allow(dead_code)]
fn project_menu(app: &mut EditorApp, item: &str) {
    let menu = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some("Editor Test"))
        .map(|w| w.rect)
        .expect("the project menu");
    run_script(
        app,
        &json!({ "size": [1280, 720], "steps": [
            { "events": [{ "click": [menu.center().x, menu.center().y] }] }, { "idle": 2 }] }),
    )
    .unwrap();
    let entry = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(item))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no '{item}' in the project menu"));
    run_script(
        app,
        &json!({ "size": [1280, 720], "steps": [
            { "events": [{ "click": [entry.center().x, entry.center().y] }] }, { "idle": 2 }] }),
    )
    .unwrap();
}

/// Clicks a button of the menu bar by its label.
#[allow(dead_code)]
fn menu_bar(app: &mut EditorApp, label: &str) {
    let rect = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(label) && w.rect.max.y < 30.0)
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no '{label}' in the menu bar"));
    run_script(
        app,
        &json!({ "size": [1280, 720], "steps": [
            { "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }] }),
    )
    .unwrap();
}

#[test]
fn the_hierarchy_and_the_scene_picture_show_the_project() {
    let (mut app, _dir) = app("shows");
    let canvas = play(&mut app, json!([{ "idle": 2 }]));
    assert_eq!(app.scene(), "scenes/main.scene.json");
    let names = ids(app.rows());
    for expected in ["camera", "ground", "sun"] {
        assert!(names.contains(expected), "{expected} is listed: {names:?}");
    }
    // The viewport is the middle of the window and holds the scene's picture
    // (the sky and the ground are not the backdrop colour).
    let view = app.viewport();
    assert!(view.width() > 500.0 && view.height() > 400.0, "{view:?}");
    let centre = (view.center().y as usize * canvas.width as usize + view.center().x as usize) * 4;
    assert_ne!(&canvas.pixels[centre..centre + 4], &[24, 24, 27, 255]);
    assert_eq!(app.status(), "", "nothing is wrong: {}", app.status());
}

#[test]
fn a_change_made_elsewhere_reaches_the_hierarchy_and_the_picture() {
    let (mut app, dir) = app("elsewhere");
    let before = play(&mut app, json!([{ "idle": 2 }]));
    // An agent adds a cube through the control plane (the same workspace
    // here; another process editing the file has the same effect).
    let added = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": {
            "Transform": { "translation": [0, 0.6, 0], "scale": [2, 2, 2] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
        &parse_actor("agent"),
    );
    assert_eq!(added["ok"], true, "{added}");
    // The editor looks at the project every 0.4 s.
    let after = play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert!(ids(app.rows()).contains("crate"), "{:?}", app.rows());
    assert!(
        differing(&before, &after) > 1000,
        "the red cube is in the picture"
    );
    // And a hand edit of the file does too.
    let path = dir.join("scenes/main.scene.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("\"crate\"", "\"box\"")).unwrap();
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let names = ids(app.rows());
    assert!(
        names.contains("box") && !names.contains("crate"),
        "{names:?}"
    );
}

#[test]
fn the_undo_button_is_a_control_plane_call_by_a_human() {
    let (mut app, _dir) = app("undo");
    play(&mut app, json!([{ "idle": 2 }]));
    let done = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "pillar", "components": {
            "Transform": {}, "Primitive": { "shape": "Cube" } } }] }),
    );
    assert_eq!(done["ok"], true, "{done}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert!(ids(app.rows()).contains("pillar"));
    // Undo is the second button of the menu bar.
    menu_bar(&mut app, "Undo");
    assert!(
        !ids(app.rows()).contains("pillar"),
        "undone: {:?}",
        app.rows()
    );
    assert_eq!(app.status(), "undo: done");
    // It went into the one history, as a human's change.
    let history = app.call("edit_history", &json!({ "action": "list" }));
    assert_eq!(history["can_redo"], true, "{history}");
    assert!(
        history.to_string().contains("human"),
        "the history says who: {history}"
    );
}

#[test]
fn right_drag_and_keys_move_the_view_camera() {
    let (mut app, _dir) = app("fly");
    // Something to fly towards: an empty ground and sky barely change when
    // the camera moves.
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "marker", "components": {
            "Transform": { "translation": [0.5, 1.0, 1.0] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
    );
    let start = play(&mut app, json!([{ "idle": 2 }]));
    let view = app.viewport();
    let (x, y) = (view.center().x, view.center().y);
    // Look around with the right button held over the view, then fly.
    let looked = play(
        &mut app,
        json!([
            { "events": [{ "pointer": [x, y] }, { "press": "secondary" }] },
            { "events": [{ "mouse_delta": [120, 0] }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 1 }
        ]),
    );
    assert!(differing(&start, &looked) > 5000, "the view turned");
    let moved = play(
        &mut app,
        json!([
            { "events": [{ "pointer": [x, y] }, { "press": "secondary" }] },
            { "events": [{ "key": "W" }] },
            { "idle": 20 },
            { "events": [{ "key": "W", "up": true }, { "release": "secondary" }] },
            { "idle": 1 }
        ]),
    );
    assert!(differing(&looked, &moved) > 5000, "the view moved forward");
    // Reset camera goes back to the scene's own camera.
    menu_bar(&mut app, "Reset camera");
    let reset = play(&mut app, json!([{ "idle": 1 }]));
    assert!(differing(&moved, &reset) > 5000);
    assert!(differing(&start, &reset) < differing(&start, &moved));
}

#[test]
fn a_bad_script_is_a_diagnostic() {
    let (mut app, _dir) = app("badscript");
    let error = run_script(
        &mut app,
        &json!({ "steps": [{ "events": [{ "wiggle": 1 }] }] }),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "invalid_editor_script");
    assert!(!error.hint.is_empty());
}

fn png_bytes(canvas: &Canvas) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, canvas.width, canvas.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&canvas.pixels).unwrap();
    }
    out
}

fn read_png(bytes: &[u8]) -> Canvas {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    Canvas {
        width: info.width,
        height: info.height,
        pixels,
    }
}

/// The whole window: the interface is exact (it is drawn on the CPU); the
/// scene's picture, drawn by the GPU, may differ by the renderer's own
/// tolerance between adapters (1/255 per channel, here 3 to be safe).
#[test]
fn the_whole_editor_window_matches_its_golden_image() {
    let (mut app, _dir) = app("golden");
    let canvas = play(&mut app, json!([{ "idle": 2 }]));
    let view = app.viewport();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("editor_window.png");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, png_bytes(&canvas)).unwrap();
        return;
    }
    let expected =
        read_png(&std::fs::read(&path).expect("missing golden; run with UPDATE_GOLDEN=1"));
    assert_eq!(
        (expected.width, expected.height),
        (canvas.width, canvas.height)
    );
    let (mut ui_bytes, mut view_bytes) = (0, 0);
    for y in 0..canvas.height as usize {
        for x in 0..canvas.width as usize {
            let inside = view.contains(egui::pos2(x as f32 + 0.5, y as f32 + 0.5));
            let at = (y * canvas.width as usize + x) * 4;
            for k in 0..4 {
                let (a, b) = (canvas.pixels[at + k], expected.pixels[at + k]);
                if inside {
                    if a.abs_diff(b) > 3 {
                        view_bytes += 1;
                    }
                } else if a != b {
                    ui_bytes += 1;
                }
            }
        }
    }
    assert_eq!(ui_bytes, 0, "the interface differs from the golden image");
    assert_eq!(
        view_bytes, 0,
        "the scene picture differs by more than 3/255"
    );
}
