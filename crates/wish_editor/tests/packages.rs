//! The Packages window (M10a), headless: a package is installed from a
//! folder and removed by hand, as `packages` calls by the person.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1600, 900], "steps": steps })).unwrap();
}

fn named(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(app: &mut EditorApp, text: &str) {
    let rect = named(app, text);
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

#[test]
fn packages_are_installed_and_removed_by_hand() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-packages");
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Package Test", "wen").unwrap();
    // A theme package in a folder of its own.
    let package = dir.join("dusk");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({ "format": 1, "id": "com.example.dusk", "name": "Dusk", "version": "1.0.0",
            "engine": ">=0.1", "license": "MIT", "contents": { "themes": ["dusk.json"] } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(package.join("dusk.json"), r#"{ "format": 1 }"#).unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    click(&mut app, "Package Test");
    click(&mut app, "Packages...");
    let field = named(&app, "The package's folder (with wish-package.json)");
    play(
        &mut app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": package.display().to_string() }] },
            { "idle": 2 }
        ]),
    );
    click(&mut app, "Install into the project");
    assert!(
        root.join("packages/com.example.dusk/dusk.json").is_file(),
        "{}",
        app.status()
    );
    assert!(root.join("wish.lock").is_file());
    // The window lists it, locked; removing it moves it to the trash.
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("locked") || w.value.as_deref() == Some("locked"))
    );
    click(&mut app, "Remove");
    assert!(
        !root.join("packages/com.example.dusk").exists(),
        "{}",
        app.status()
    );
    assert!(
        root.join(".wish/trash/packages/com.example.dusk-1")
            .is_dir()
    );
    // Both are the person's, in the activity feed.
    let feed = app.call("activity_get", &json!({}));
    let mine = feed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["tool"] == "packages" && e["actor"] == "human")
        .count();
    assert_eq!(mine, 2, "{feed}");
}
