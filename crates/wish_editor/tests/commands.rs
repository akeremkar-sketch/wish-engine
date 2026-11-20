//! Commands (M10a WP4), headless: the command palette finds a command by
//! its name and runs it; shortcuts come from the command table and the
//! person remaps them in the preferences.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn key(app: &mut EditorApp, name: &str) {
    play(
        app,
        json!([{ "events": [{ "key": name, "ctrl": true }, { "key": name, "ctrl": true, "up": true }] }, { "idle": 2 }]),
    );
}

fn palette_open(app: &EditorApp) -> bool {
    app.widgets()
        .iter()
        .any(|w| w.label.as_deref() == Some("Type a command"))
}

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-commands-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Commands", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    (app, dir)
}

#[test]
fn the_palette_finds_and_runs_a_command() {
    let (mut app, _dir) = app("palette");
    assert!(!palette_open(&app));
    key(&mut app, "K");
    assert!(palette_open(&app), "Ctrl+K opens the palette");
    // Typing narrows the list; Enter runs the first.
    play(
        &mut app,
        json!([{ "events": [{ "text": "with ai" }] }, { "idle": 2 }]),
    );
    let lines: Vec<String> = app
        .widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .filter_map(|w| w.label.clone())
        .collect();
    assert!(lines.contains(&"With AI".to_owned()), "{lines:?}");
    assert!(
        !lines.contains(&"Run tests".to_owned()),
        "filtered: {lines:?}"
    );
    play(
        &mut app,
        json!([{ "events": [{ "key": "Enter" }, { "key": "Enter", "up": true }] }, { "idle": 2 }]),
    );
    assert!(app.ai_mode(), "the command ran");
    assert!(!palette_open(&app), "and the palette closed");
    // Escape closes it without running anything.
    key(&mut app, "K");
    assert!(palette_open(&app));
    play(
        &mut app,
        json!([{ "events": [{ "key": "Escape" }, { "key": "Escape", "up": true }] }, { "idle": 2 }]),
    );
    assert!(!palette_open(&app));
    assert!(app.ai_mode());
}

#[test]
fn shortcuts_come_from_the_table_and_can_be_remapped() {
    let (mut app, dir) = app("keys");
    key(&mut app, "M");
    assert!(app.ai_mode(), "Ctrl+M switches the mode");
    key(&mut app, "M");
    assert!(!app.ai_mode());
    // The person's own shortcut.
    let prefs = dir.join("editor.json");
    std::fs::write(
        &prefs,
        r#"{ "language": "en", "keys": { "mode.switch": "Ctrl+J" } }"#,
    )
    .unwrap();
    app.use_preferences(prefs);
    play(&mut app, json!([{ "idle": 2 }]));
    key(&mut app, "M");
    assert!(!app.ai_mode(), "Ctrl+M is no longer the switch");
    key(&mut app, "J");
    assert!(app.ai_mode(), "Ctrl+J is");
}
