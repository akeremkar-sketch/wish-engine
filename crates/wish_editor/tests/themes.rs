//! Themes (M10a WP2), headless: the engine's themes come from its
//! packages, one is chosen from the Project menu, the editor is drawn in it
//! and the choice is kept; a theme package installed into the project is
//! offered too.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

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

fn click(app: &mut EditorApp, text: &str) {
    let rect = named(app, text);
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

fn hover(app: &mut EditorApp, text: &str) {
    let rect = named(app, text);
    play(
        app,
        json!([{ "events": [{ "pointer": [rect.center().x, rect.center().y] }] }, { "idle": 3 }]),
    );
}

/// The colour of the canvas's top-left pixel (the backdrop, under the
/// menu bar's panel... the bottom-left corner is the status bar's panel).
fn pixel(app: &EditorApp, x: u32, y: u32) -> [u8; 4] {
    let canvas = app.canvas();
    let at = ((y * canvas.width + x) * 4) as usize;
    canvas.pixels[at..at + 4].try_into().unwrap()
}

#[test]
fn a_theme_is_chosen_drawn_and_kept() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-themes");
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Theme Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    let prefs = dir.join("editor.json");
    std::fs::write(&prefs, r#"{ "language": "en" }"#).unwrap();
    app.use_preferences(prefs.clone());
    play(&mut app, json!([{ "idle": 2 }]));
    let names: Vec<String> = app.themes().into_iter().map(|(_, name)| name).collect();
    assert_eq!(names[0], "Atelier", "the default comes first: {names:?}");
    assert!(names.contains(&"Midnight".to_owned()), "{names:?}");
    assert!(names.contains(&"High contrast".to_owned()), "{names:?}");
    let atelier = pixel(&app, 2, 715);
    // Project menu → Theme → Midnight.
    click(&mut app, "View");
    hover(&mut app, "Theme");
    click(&mut app, "Midnight");
    assert_eq!(
        app.theme_key(),
        "wish.theme.midnight/midnight.json",
        "{}",
        app.status()
    );
    play(&mut app, json!([{ "idle": 2 }]));
    let midnight = pixel(&app, 2, 715);
    assert_ne!(
        atelier, midnight,
        "the status bar is drawn in the new theme"
    );
    assert_eq!(midnight, [18, 21, 30, 255], "Midnight's panel colour");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["theme"], "wish.theme.midnight/midnight.json");
    // A theme package installed into the project is offered too.
    let package = dir.join("ember");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({ "format": 1, "id": "com.example.ember", "name": "Ember", "version": "1.0.0",
            "engine": ">=0.1", "license": "MIT", "contents": { "themes": ["ember.json"] } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        package.join("ember.json"),
        r##"{ "format": 1, "name": "Ember", "colors": { "panel": "#301010" } }"##,
    )
    .unwrap();
    let installed = app.call(
        "packages",
        &json!({ "action": "install", "path": package.display().to_string() }),
    );
    assert_eq!(installed["ok"], true, "{installed}");
    // The editor reads the themes again when asked for one it does not know.
    assert!(
        app.set_theme("com.example.ember/ember.json"),
        "{}",
        app.status()
    );
    play(&mut app, json!([{ "idle": 2 }]));
    assert_eq!(pixel(&app, 2, 715), [48, 16, 16, 255]);
    // A theme that is not there leaves the theme as it is and says why.
    assert!(!app.set_theme("com.example.none/none.json"));
    assert!(
        app.status().contains("could not be used"),
        "{}",
        app.status()
    );
    assert_eq!(app.theme_key(), "com.example.ember/ember.json");
}

#[test]
fn a_language_package_adds_a_language() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-language-package");
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("game");
    create_project(&root, "Language Test", "wen").unwrap();
    let package = dir.join("svenska");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("wish-package.json"),
        json!({ "format": 1, "id": "com.example.svenska", "name": "Svenska", "version": "1.0.0",
            "engine": ">=0.1", "license": "MIT", "contents": { "languages": ["sv.json"] } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        package.join("sv.json"),
        json!({ "format": 1, "code": "sv", "name": "Svenska", "texts": { "Undo": "Ångra", "Redo": "Gör om" } })
            .to_string(),
    )
    .unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: root.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    assert!(!app.set_language("sv"), "no Swedish before the package");
    let installed = app.call(
        "packages",
        &json!({ "action": "install", "path": package.display().to_string() }),
    );
    assert_eq!(installed["ok"], true, "{installed}");
    assert!(app.set_language("sv"), "the package brings it");
    play(&mut app, json!([{ "idle": 3 }]));
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Ångra")),
        "the toolbar speaks Swedish"
    );
    // What the package does not translate stays English.
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.label.as_deref() == Some("Play"))
    );
    // A package cannot take a built-in language's code.
    assert!(!wish_editor::i18n::register(
        "tr",
        "Fake",
        Default::default()
    ));
}
