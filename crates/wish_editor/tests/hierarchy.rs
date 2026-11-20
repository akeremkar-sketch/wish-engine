//! The hierarchy and selection (M8a WP3): clicking, adding, deleting,
//! renaming, dragging to change a parent, and picking in the view. Every one
//! is a control-plane call by `human`, and one entry of the one history.
//!
//! The coordinates are the layout of a 1280x720 window over a new project
//! with a one-row top bar: hierarchy rows at y = 93 (camera), 119 (ground),
//! 144 (sun); the "+" button at (156, 55) and "Delete" at (195, 55). The
//! redesign's menu bar and toolbar move everything down by the same amount:
//! [`dy`] measures it from where the camera's row is, and [`pos`] adds it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

const CAMERA: [f32; 2] = [58.0, 93.0];
const GROUND: [f32; 2] = [58.0, 119.0];
const SUN: [f32; 2] = [50.0, 144.0];
const ADD: [f32; 2] = [156.0, 55.0];
const DELETE: [f32; 2] = [195.0, 55.0];

/// How far below the one-row layout the hierarchy is now.
fn dy(app: &EditorApp) -> f32 {
    let row = app
        .widgets()
        .iter()
        .find(|w| w.label.as_deref() == Some("camera") && w.rect.min.x < 220.0)
        .map(|w| w.rect)
        .expect("the camera's row");
    row.center().y - CAMERA[1]
}

/// A place of the one-row layout, where it is now.
fn pos(at: [f32; 2], dy: f32) -> [f32; 2] {
    [at[0], at[1] + dy]
}

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-h-{name}"));
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

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn click(at: [f32; 2]) -> Value {
    json!({ "events": [{ "click": at }] })
}

/// A click with Ctrl held (a frame to take hold, one to click, one to let go).
fn ctrl_click(at: [f32; 2]) -> Value {
    json!([
        { "events": [{ "modifiers": { "ctrl": true } }] },
        { "events": [{ "click": at }] },
        { "events": [{ "modifiers": {} }] },
        { "idle": 1 }
    ])
}

fn history(app: &mut EditorApp) -> Value {
    app.call("edit_history", &json!({ "action": "list" }))
}

/// Clicks an item of the project menu (the menu bar's first button, named
/// after the project).
#[allow(dead_code)]
fn project_menu(app: &mut EditorApp, item: &str) {
    let menu = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(top_menu(item)))
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
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(label) && w.rect.max.y < 72.0)
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
fn clicking_rows_selects_and_ctrl_adds() {
    let (mut app, _dir) = app("select");
    play(&mut app, json!([{ "idle": 2 }]));
    assert_eq!(
        app.rows().iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["camera", "ground", "sun"]
    );
    let dy = dy(&app);
    play(&mut app, json!([click(pos(GROUND, dy)), { "idle": 1 }]));
    assert_eq!(app.selection(), ["ground"]);
    play(&mut app, ctrl_click(pos(SUN, dy)));
    assert_eq!(app.selection(), ["ground", "sun"]);
    // Ctrl-click again takes it out; a plain click on another replaces.
    play(&mut app, ctrl_click(pos(SUN, dy)));
    assert_eq!(app.selection(), ["ground"]);
    play(&mut app, json!([click(pos(CAMERA, dy)), { "idle": 1 }]));
    assert_eq!(app.selection(), ["camera"]);
    // The empty part of the panel selects nothing. Selecting does not touch
    // the scene: no history.
    play(&mut app, json!([click([100.0, 400.0]), { "idle": 1 }]));
    assert!(app.selection().is_empty());
    assert_eq!(history(&mut app)["can_undo"], false);
}

#[test]
fn add_and_delete_are_single_undoable_calls_by_a_human() {
    let (mut app, _dir) = app("add");
    play(&mut app, json!([{ "idle": 2 }]));
    let dy = dy(&app);
    play(&mut app, json!([click(pos(ADD, dy)), { "idle": 1 }]));
    assert!(ids(app.rows()).contains("entity_1"), "{:?}", app.rows());
    assert_eq!(app.selection(), ["entity_1"]);
    assert_eq!(app.status(), "added entity_1");
    // A second one gets the next free name.
    play(&mut app, json!([click(pos(ADD, dy)), { "idle": 1 }]));
    assert!(ids(app.rows()).contains("entity_2"));
    // Delete removes the selected one.
    play(&mut app, json!([click(pos(DELETE, dy)), { "idle": 1 }]));
    assert!(!ids(app.rows()).contains("entity_2"), "{:?}", app.rows());
    assert!(ids(app.rows()).contains("entity_1"));
    assert!(app.selection().is_empty());
    // Three changes, three entries, all by the human; undo takes back one.
    let list = history(&mut app);
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{list}");
    assert!(
        entries.iter().all(|e| e.to_string().contains("human")),
        "{list}"
    );
    menu_bar(&mut app, "Undo");
    assert!(ids(app.rows()).contains("entity_2"), "undo brought it back");
}

#[test]
fn the_delete_key_deletes_the_selection() {
    let (mut app, _dir) = app("delete-key");
    play(&mut app, json!([{ "idle": 2 }]));
    let dy = dy(&app);
    play(&mut app, json!([click(pos(GROUND, dy)), { "idle": 1 }]));
    play(
        &mut app,
        json!([{ "events": [{ "key": "Delete" }] }, { "idle": 1 }]),
    );
    assert!(!ids(app.rows()).contains("ground"), "{:?}", app.rows());
}

#[test]
fn double_click_renames() {
    let (mut app, _dir) = app("rename");
    play(&mut app, json!([{ "idle": 2 }]));
    let dy = dy(&app);
    play(
        &mut app,
        json!([
            click(pos(GROUND, dy)),
            click(pos(GROUND, dy)),
            { "idle": 2 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "floor" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    let names = ids(app.rows());
    assert!(
        names.contains("floor") && !names.contains("ground"),
        "{names:?}"
    );
    assert_eq!(app.selection(), ["floor"]);
    assert_eq!(app.status(), "renamed ground to floor");
}

#[test]
fn dragging_a_row_onto_another_changes_its_parent() {
    let (mut app, _dir) = app("drag");
    play(&mut app, json!([{ "idle": 2 }]));
    let dy = dy(&app);
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": pos(SUN, dy) }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": pos([50.0, 136.0], dy) }] },
            { "events": [{ "pointer": pos([52.0, 124.0], dy) }] },
            { "events": [{ "pointer": pos([55.0, 108.0], dy) }] },
            { "events": [{ "pointer": pos([58.0, 95.0], dy) }] },
            { "events": [{ "pointer": pos([58.0, 93.0], dy) }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 2 }
        ]),
    );
    // The hierarchy is read again on the editor's next look at the scene.
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let sun = app.rows().iter().find(|r| r.id == "sun").unwrap();
    assert_eq!(sun.parent.as_deref(), Some("camera"), "{:?}", app.rows());
    assert_eq!(sun.depth, 1);
    // Dropping on the empty part takes it out again.
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": pos([72.0, 119.0], dy) }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": pos([72.0, 130.0], dy) }] },
            { "events": [{ "pointer": pos([70.0, 200.0], dy) }] },
            { "events": [{ "pointer": [80, 400] }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 2 },
            { "seconds": 0.5 },
            { "idle": 2 }
        ]),
    );
    let sun = app.rows().iter().find(|r| r.id == "sun").unwrap();
    assert_eq!(sun.parent, None, "{:?}", app.rows());
}

#[test]
fn clicking_in_the_view_picks_what_is_under_the_pointer() {
    let (mut app, _dir) = app("pick");
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "marker", "components": {
            "Transform": { "translation": [0.0, 1.0, 1.0] },
            "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] } } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    // Where the marker is: select it by name and read the box drawn around it.
    app.select(&["marker"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let outline = *app.outlines().first().expect("the marker is in view");
    let centre = outline.center();
    app.select(&[]);
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(app.outlines().is_empty());
    // A click there picks it; the panels say so and the box is drawn.
    play(
        &mut app,
        json!([{ "events": [{ "click": [centre.x, centre.y] }] }, { "idle": 2 }]),
    );
    assert_eq!(app.selection(), ["marker"]);
    assert_eq!(app.outlines().len(), 1);
    // A click on the sky picks nothing and clears the selection.
    let view = app.viewport();
    play(
        &mut app,
        json!([{ "events": [{ "click": [view.center().x, view.min.y + 20.0] }] }, { "idle": 2 }]),
    );
    assert!(app.selection().is_empty(), "{:?}", app.selection());
}

#[test]
fn a_change_made_since_the_editor_looked_is_not_overwritten() {
    let (mut app, _dir) = app("stale");
    play(&mut app, json!([{ "idle": 2 }]));
    // An agent renames the ground between two looks of the editor...
    let renamed = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "rename", "id": "ground", "to": "terrain" }] }),
        &parse_actor("agent"),
    );
    assert_eq!(renamed["ok"], true, "{renamed}");
    // ...and the person, still looking at the old hierarchy, deletes
    // "ground" (selected before the agent's change).
    app.select(&["ground"]);
    let dy = dy(&app);
    play(&mut app, json!([click(pos(DELETE, dy))]));
    // Nothing the person did overwrote the agent's work: "terrain" is there,
    // and the person is told what happened.
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let names = ids(app.rows());
    assert!(names.contains("terrain"), "{names:?}");
    assert!(
        app.status().contains("failed") || !app.status().starts_with("deleted"),
        "the person is told: {}",
        app.status()
    );
}

/// The menu bar's menu that holds `item` (the editor redesign, R2: File,
/// Edit, Create, View, Game, AI, Help replaced the project menu).
#[allow(dead_code)]
fn top_menu(item: &str) -> &'static str {
    match item {
        "Settings..." | "Packages..." | "Export game" | "Save picture" | "New scene" | "Quit" => {
            "File"
        }
        "Checkpoints" | "Duplicate" | "Rename" | "Undo" | "Redo" => "Edit",
        "Permissions" | "Connect an outside agent..." => "AI",
        "Theme" | "Language" | "Reset camera" | "Shading" => "View",
        _ => "Game",
    }
}
