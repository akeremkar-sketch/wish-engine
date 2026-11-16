//! The project settings window (M8a WP8): name, default scene and the input
//! map, saved through `project_settings`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-s-{name}"));
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

fn named(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

/// The text box on the same row as the label `text`, to its right.
fn text_box_after(app: &EditorApp, text: &str) -> Rect {
    let label = app
        .widgets()
        .iter()
        .find(|w| (w.role == "Label" || w.role == "TextRun") && w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no label '{text}'"));
    // The nearest text box to its right (a grid row can hold a column of
    // boxes, with the label in its middle).
    let all: Vec<_> = app
        .widgets()
        .iter()
        .filter(|w| w.role == "TextInput")
        .map(|w| w.rect)
        .collect();
    all.iter()
        .filter(|r| r.min.x > label.max.x && (r.center().y - label.center().y).abs() < 30.0)
        .min_by(|a, b| {
            (a.center().y - label.center().y)
                .abs()
                .total_cmp(&(b.center().y - label.center().y).abs())
        })
        .copied()
        .or_else(|| {
            eprintln!("label {label:?}, text boxes {all:?}");
            None
        })
        .unwrap_or_else(|| panic!("no text box after '{text}'"))
}

fn type_into(app: &mut EditorApp, rect: Rect, text: &str) {
    play(
        app,
        json!([
            { "events": [{ "click": [rect.center().x, rect.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": text }] },
            { "idle": 1 }
        ]),
    );
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
fn the_input_map_is_edited_and_saved_with_a_hint_for_mistakes() {
    let (mut app, dir) = app("input");
    project_menu(&mut app, "Settings...");
    // The new project's actions are listed.
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.value.as_deref() == Some("jump"))
    );
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.value.as_deref() == Some("move"))
    );
    // Jump on Space and Enter.
    let keys = text_box_after(&app, "jump");
    type_into(&mut app, keys, "Space, Enter");
    let save = named(&app, "Save input map");
    click(&mut app, save);
    assert_eq!(app.status(), "settings saved (input)", "{}", app.status());
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(file["actions"]["jump"]["keys"], json!(["Space", "Enter"]));
    // A key the engine does not know: nothing is written, the hint says why.
    let keys = text_box_after(&app, "jump");
    type_into(&mut app, keys, "Spacebar");
    let save = named(&app, "Save input map");
    click(&mut app, save);
    assert!(
        app.status().starts_with("settings not saved"),
        "{}",
        app.status()
    );
    assert!(
        app.status().contains("Space"),
        "the hint names the right key: {}",
        app.status()
    );
    let still: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(still["actions"]["jump"]["keys"], json!(["Space", "Enter"]));
}

#[test]
fn a_new_action_is_added_and_the_project_renamed() {
    let (mut app, dir) = app("add");
    project_menu(&mut app, "Settings...");
    let new_action = app
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput" && w.label.is_none() && w.value.as_deref() == Some(""))
        .map(|w| w.rect);
    // The "new action" box: the empty one next to the kind list.
    let add = named(&app, "Add action");
    let new_action = new_action
        .filter(|r| (r.center().y - add.center().y).abs() < 10.0)
        .unwrap_or_else(|| {
            app.widgets()
                .iter()
                .filter(|w| {
                    w.role == "TextInput" && (w.rect.center().y - add.center().y).abs() < 10.0
                })
                .map(|w| w.rect)
                .next()
                .unwrap()
        });
    type_into(&mut app, new_action, "fire");
    let add = named(&app, "Add action");
    click(&mut app, add);
    let keys = text_box_after(&app, "fire");
    type_into(&mut app, keys, "F, MouseLeft");
    let save = named(&app, "Save input map");
    click(&mut app, save);
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(
        file["actions"]["fire"],
        json!({ "kind": "button", "keys": ["F", "MouseLeft"] }),
        "{file}"
    );
    // The project's name.
    let name = text_box_after(&app, "Name");
    type_into(&mut app, name, "Island Hopper");
    let save = named(&app, "Save project");
    click(&mut app, save);
    let project: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("wish.json")).unwrap()).unwrap();
    assert_eq!(project["name"], "Island Hopper");
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.role == "Button" && w.label.as_deref() == Some("Island Hopper"))
    );
}
