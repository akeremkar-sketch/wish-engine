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
    click(&mut app, "File");
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

fn type_into(app: &mut EditorApp, field: &str, text: &str) {
    let rect = named(app, field);
    play(
        app,
        json!([
            { "events": [{ "click": [rect.center().x, rect.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": text }] },
            { "idle": 2 }
        ]),
    );
}

#[test]
fn packages_are_found_in_the_index_added_and_updated_by_hand() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-package-index");
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Index Test", "wen").unwrap();
    // An index: a folder with two versions of one theme package.
    for version in ["1.0.0", "1.1.0"] {
        let package = dir.join("index").join(format!("dusk-{version}"));
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("wish-package.json"),
            json!({ "format": 1, "id": "com.example.dusk", "name": "Dusk", "version": version,
                "engine": ">=0.1", "license": "MIT", "description": "Evening colours.",
                "contents": { "themes": ["dusk.json"] } })
            .to_string(),
        )
        .unwrap();
        std::fs::write(package.join("dusk.json"), r#"{ "format": 1 }"#).unwrap();
    }
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    click(&mut app, "File");
    click(&mut app, "Packages...");
    type_into(
        &mut app,
        "The index folder (a folder of packages; empty: WISH_PACKAGE_INDEX)",
        &dir.join("index").display().to_string(),
    );
    type_into(
        &mut app,
        "Words to search for (empty: everything)",
        "evening",
    );
    click(&mut app, "Search");
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Dusk") || w.value.as_deref() == Some("Dusk")),
        "the index's package is listed"
    );
    click(&mut app, "Add to the project");
    let version = |root: &std::path::Path| -> String {
        let text =
            std::fs::read_to_string(root.join("packages/com.example.dusk/wish-package.json"))
                .unwrap_or_default();
        serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)["version"]
            .as_str()
            .unwrap_or("")
            .to_owned()
    };
    assert_eq!(version(&root), "1.1.0", "the newest: {}", app.status());
    assert!(root.join("wish.lock").is_file());
    // An older one installed by hand; the index offers the update.
    let older = dir.join("index/dusk-1.0.0").display().to_string();
    let answer = app.call(
        "packages",
        &json!({ "action": "install", "path": older, "replace": true }),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    click(&mut app, "Search");
    click(&mut app, "Update");
    assert_eq!(version(&root), "1.1.0", "{}", app.status());
    // The searches are not in the activity feed; the add and update are, as the person's.
    let feed = app.call("activity_get", &json!({}));
    let actions: Vec<String> = feed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["tool"] == "packages")
        .map(|e| e["summary"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(!feed.to_string().contains("\"search"), "{actions:?}");
    assert!(
        feed["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["tool"] == "packages")
            .all(|e| e["actor"] == "human"),
        "{feed}"
    );
    assert!(actions.len() >= 3, "add, install, update: {actions:?}");
}
