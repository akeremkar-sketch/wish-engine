//! The bottom panel's tabs and the code buttons (M8a WP8): console,
//! problems, performance, "Edit code", "Build code".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-x-{name}"));
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

fn find(app: &EditorApp, starts: &str) -> Option<Rect> {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun")
        .find(|w| {
            w.label.as_deref().is_some_and(|l| l.starts_with(starts))
                || w.value.as_deref().is_some_and(|v| v.starts_with(starts))
        })
        .map(|w| w.rect)
}

fn click(app: &mut EditorApp, starts: &str) {
    let rect = find(app, starts).unwrap_or_else(|| panic!("nothing called '{starts}'"));
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
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
fn the_console_shows_what_was_logged() {
    let (mut app, _dir) = app("console");
    click(&mut app, "Console");
    click(&mut app, "Play");
    play(&mut app, json!([{ "idle": 40 }]));
    assert!(
        app.console().iter().any(|e| e["code"] == "session_started"),
        "{:?}",
        app.console()
    );
    assert!(
        find(&app, "t0 [session] session_started").is_some(),
        "shown in the tab"
    );
    click(&mut app, "Clear");
    assert!(app.console().is_empty());
}

#[test]
fn a_problem_is_listed_and_a_click_selects_its_entity() {
    let (mut app, _dir) = app("problems");
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "speaker", "components": {
            "Transform": {}, "AudioSource": { "clip": "assets/sounds/missing.wav" } } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    click(&mut app, "Problems");
    assert!(!app.problems().is_empty(), "check finds the missing sound");
    let text = app.problems()[0]["code"].as_str().unwrap().to_owned();
    click(&mut app, &text);
    assert_eq!(app.selection(), ["speaker"]);
}

#[test]
fn the_performance_tab_shows_frame_times() {
    let (mut app, _dir) = app("performance");
    click(&mut app, "Performance");
    play(
        &mut app,
        json!([{ "events": [{ "pointer": [600, 300] }] }, { "events": [{ "pointer": [610, 300] }] }]),
    );
    assert!(find(&app, "interface ").is_some());
    assert!(find(&app, "scene picture ").is_some());
}

#[test]
fn edit_code_opens_the_game_code_outside() {
    let (mut app, dir) = app("edit-code");
    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&opened);
    app.set_opener(Box::new(move |path| {
        seen.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }));
    project_menu(&mut app, "Edit code");
    assert_eq!(*opened.lock().unwrap(), [dir.join("scripts/src/lib.rs")]);
    assert_eq!(app.status(), "opened scripts/src/lib.rs");
}

#[test]
fn a_build_that_fails_shows_its_problems() {
    let (mut app, _dir) = app("build");
    // No compiler is set up in this test: the build fails, and says so.
    project_menu(&mut app, "Build code");
    assert!(
        app.status().starts_with("game code not built"),
        "{}",
        app.status()
    );
    assert!(!app.problems().is_empty());
    assert!(find(&app, "Problems (").is_some());
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
