//! The scene view of the redesign (R3): the eye orbits with Alt and the
//! left button, slides with the middle button, zooms with the wheel and
//! frames the selection with F; the controls floating on the view take
//! their own clicks (a click on them picks nothing in the scene).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::raster::Canvas;
use wish_editor::script::run_script;

fn app(name: &str) -> EditorApp {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-eye-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 3 }]));
    app
}

fn play(app: &mut EditorApp, steps: Value) -> Canvas {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps }))
        .unwrap()
        .canvas
}

/// The pixels of the scene view's middle, where the overlays are not.
fn middle(app: &EditorApp, canvas: &Canvas) -> Vec<u8> {
    let view = app.viewport();
    let mut out = Vec::new();
    let (x0, x1) = (view.min.x as u32 + 60, view.max.x as u32 - 60);
    let (y0, y1) = (view.min.y as u32 + 60, view.max.y as u32 - 60);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * canvas.width + x) * 4) as usize;
            out.extend_from_slice(&canvas.pixels[at..at + 4]);
        }
    }
    out
}

fn named(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

#[test]
fn orbit_pan_zoom_and_frame_move_the_eye() {
    let mut app = app("moves");
    let view = app.viewport();
    let centre = [view.center().x, view.center().y];
    let start = play(&mut app, json!([{ "idle": 1 }]));
    let start = middle(&app, &start);

    // The wheel zooms.
    let zoomed = play(
        &mut app,
        json!([{ "events": [{ "pointer": centre }] }, { "events": [{ "scroll": [0.0, 96.0] }] }, { "idle": 2 }]),
    );
    let zoomed = middle(&app, &zoomed);
    assert_ne!(start, zoomed, "the wheel moved the eye nearer");

    // Alt and the left button orbit, and select nothing.
    let orbited = play(
        &mut app,
        json!([
            { "events": [{ "modifiers": { "alt": true } }] },
            { "events": [{ "pointer": centre }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "mouse_delta": [80.0, 0.0] }] },
            { "events": [{ "release": "primary" }] },
            { "events": [{ "modifiers": {} }] },
            { "idle": 2 }
        ]),
    );
    let orbited = middle(&app, &orbited);
    assert_ne!(zoomed, orbited, "the eye turned around the middle");
    assert!(app.selection().is_empty(), "an orbit is not a click");

    // The middle button slides.
    let panned = play(
        &mut app,
        json!([
            { "events": [{ "pointer": centre }] },
            { "events": [{ "press": "middle" }] },
            { "events": [{ "mouse_delta": [0.0, 60.0] }] },
            { "events": [{ "release": "middle" }] },
            { "idle": 2 }
        ]),
    );
    assert_ne!(orbited, middle(&app, &panned), "the eye slid");

    // F frames the selection (and with nothing selected says so).
    play(
        &mut app,
        json!([{ "events": [{ "key": "F" }] }, { "idle": 2 }]),
    );
    assert_eq!(app.status(), "Select an entity to frame it");
    app.select(&["sun"]);
    let before = play(&mut app, json!([{ "idle": 2 }]));
    let framed = play(
        &mut app,
        json!([{ "events": [{ "key": "F" }] }, { "idle": 2 }]),
    );
    assert_ne!(
        middle(&app, &before),
        middle(&app, &framed),
        "F moved the eye"
    );
}

#[test]
fn the_floating_tools_take_their_own_clicks() {
    let mut app = app("overlay");
    app.select(&["ground"]);
    play(&mut app, json!([{ "idle": 2 }]));
    // The tools float on the view.
    let snap = named(&app, "Snap");
    assert!(app.viewport().contains(snap.center()), "{snap:?}");
    play(
        &mut app,
        json!([{ "events": [{ "click": [snap.center().x, snap.center().y] }] }, { "idle": 2 }]),
    );
    // The click switched snapping on and did not reach the scene: the
    // selection is as it was.
    assert_eq!(app.selection(), ["ground"]);
    let rotate = named(&app, "Rotate");
    play(
        &mut app,
        json!([{ "events": [{ "click": [rotate.center().x, rotate.center().y] }] }, { "idle": 2 }]),
    );
    assert_eq!(app.handle_mode(), "rotate");
    assert_eq!(app.selection(), ["ground"]);
}
