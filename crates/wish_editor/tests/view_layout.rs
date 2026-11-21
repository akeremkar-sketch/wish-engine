//! The views' layout (a user request after the beta self-test): the line
//! between the scene view and the game view is dragged to share the middle
//! (a double click centres it), a view is maximized to fill the editor and
//! restored (a button, or Shift+Space over it), and the game view closes
//! from its own corner.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn app(name: &str) -> EditorApp {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-layout-{name}"));
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

/// A window wide enough for the game view beside the scene view.
fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1800, 900], "steps": steps })).unwrap();
}

fn named(app: &EditorApp, text: &str) -> Option<Rect> {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text))
        .map(|w| w.rect)
}

fn click(app: &mut EditorApp, text: &str) {
    let rect = named(app, text).unwrap_or_else(|| panic!("nothing called '{text}'"));
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 3 }]),
    );
}

#[test]
fn the_line_between_the_views_is_dragged_and_centred_again() {
    let mut app = app("split");
    let game = app
        .game_view_area()
        .expect("a wide window shows the game view");
    let line = (app.viewport().max.x + game.min.x) * 0.5;
    let y = game.center().y;
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [line, y] }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": [line + 60.0, y] }] },
            { "events": [{ "pointer": [line + 200.0, y] }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 3 }
        ]),
    );
    let moved = app.game_view_area().unwrap();
    assert!(moved.min.x > game.min.x + 150.0, "{game:?} -> {moved:?}");
    assert!(app.viewport().width() > 150.0 + game.width() * 0.5);
    // A double click on the line shares the middle evenly again.
    let line = (app.viewport().max.x + moved.min.x) * 0.5;
    play(
        &mut app,
        json!([
            { "events": [{ "click": [line, y] }] },
            { "events": [{ "click": [line, y] }] },
            { "idle": 3 }
        ]),
    );
    let back = app.game_view_area().unwrap();
    assert!(
        (back.min.x - game.min.x).abs() < 2.0,
        "{game:?} -> {back:?}"
    );
}

#[test]
fn a_view_is_maximized_and_restored() {
    let mut app = app("maximize");
    let normal = app.viewport();
    assert!(named(&app, "camera").is_some(), "the hierarchy shows");
    click(&mut app, "Maximize the view");
    // The scene view fills the middle; the panels and the game view step aside.
    assert!(
        app.viewport().width() > normal.width() * 1.8,
        "{:?}",
        app.viewport()
    );
    assert!(app.game_view_area().is_none());
    assert!(named(&app, "camera").is_none(), "the hierarchy is hidden");
    click(&mut app, "Restore the view");
    assert_eq!(app.viewport(), normal);
    // Shift+Space over the game view maximizes the game view.
    let game = app.game_view_area().unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [game.center().x, game.center().y] }] },
            { "events": [{ "key": "Space", "shift": true }] },
            { "idle": 3 }
        ]),
    );
    let full = app
        .game_view_area()
        .expect("the game view fills the middle");
    assert!(full.width() > game.width() * 1.8, "{full:?}");
    play(
        &mut app,
        json!([{ "events": [{ "key": "Space", "shift": true }] }, { "idle": 3 }]),
    );
    assert_eq!(app.game_view_area(), Some(game), "restored");
}

#[test]
fn the_game_view_closes_from_its_corner() {
    let mut app = app("close");
    assert!(app.game_view_area().is_some());
    click(&mut app, "Close the game view");
    assert!(app.game_view_area().is_none());
    // And comes back from the scene view's corner.
    click(&mut app, "Game view");
    assert!(app.game_view_area().is_some());
}
