//! The menu bar of the redesign (R2): the Create menu, Duplicate and New
//! scene, done by hand with clicks and keys. Each is one control-plane call
//! signed `human` and one entry of the one history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-menus-{name}"));
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
        .find(|w| w.label.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

/// Opens a menu of the menu bar and clicks one of its items.
fn menu(app: &mut EditorApp, top: &str, item: &str) {
    let menu = named(app, top);
    click(app, menu);
    let entry = named(app, item);
    click(app, entry);
}

fn history_count(app: &mut EditorApp) -> usize {
    app.call("edit_history", &json!({ "action": "list" }))["entries"]
        .as_array()
        .map_or(0, Vec::len)
}

fn components(app: &mut EditorApp, id: &str) -> Value {
    app.call("entity_get", &json!({ "ids": [id] }))["entities"][0]["components"].clone()
}

#[test]
fn the_create_menu_makes_ready_entities_by_hand() {
    let (mut app, _dir) = app("create");
    menu(&mut app, "Create", "Cube");
    assert!(ids(app.rows()).contains("cube_1"), "{:?}", app.rows());
    assert_eq!(app.selection(), ["cube_1"]);
    assert_eq!(app.status(), "added cube_1");
    assert_eq!(components(&mut app, "cube_1")["Primitive"]["shape"], "Cube");
    // It stands on the ground (a metre across, half a metre up).
    assert_eq!(
        components(&mut app, "cube_1")["Transform"]["translation"],
        json!([0.0, 0.5, 0.0])
    );
    menu(&mut app, "Create", "Point light");
    assert!(
        components(&mut app, "point_light_1")
            .get("PointLight")
            .is_some()
    );
    menu(&mut app, "Create", "Camera");
    assert!(components(&mut app, "camera_1").get("Camera").is_some());
    // Each is one change of the one history, by the person.
    let list = app.call("edit_history", &json!({ "action": "list" }));
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{list}");
    assert!(
        entries.iter().all(|e| e.to_string().contains("human")),
        "{list}"
    );
}

#[test]
fn duplicate_copies_an_entity_with_its_children_in_one_change() {
    let (mut app, _dir) = app("duplicate");
    let made = app.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "crate_1", "components": {
                "Transform": { "translation": [1.0, 0.5, 0.0] },
                "Primitive": { "shape": "Cube" } } },
            { "op": "spawn", "id": "lid", "parent": "crate_1", "components": {
                "Transform": { "translation": [0.0, 0.6, 0.0] },
                "Primitive": { "shape": "Plane" } } }
        ] }),
    );
    assert_eq!(made["ok"], true, "{made}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let before = history_count(&mut app);
    app.select(&["crate_1"]);
    play(&mut app, json!([{ "idle": 2 }]));
    menu(&mut app, "Edit", "Duplicate");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let names = ids(app.rows());
    assert!(
        names.contains("crate_2") && names.contains("lid_1"),
        "{names:?}"
    );
    assert_eq!(app.selection(), ["crate_2"]);
    assert_eq!(app.status(), "duplicated crate_2");
    let lid = app.rows().iter().find(|r| r.id == "lid_1").unwrap();
    assert_eq!(lid.parent.as_deref(), Some("crate_2"));
    assert_eq!(
        components(&mut app, "crate_2"),
        components(&mut app, "crate_1"),
        "the copy is the same"
    );
    assert_eq!(history_count(&mut app), before + 1, "one change");
    // Ctrl+D does the same.
    play(
        &mut app,
        json!([{ "events": [{ "key": "D", "ctrl": true }] }, { "idle": 2 }]),
    );
    assert!(ids(app.rows()).contains("crate_3"), "{:?}", app.rows());
}

#[test]
fn new_scene_makes_a_scene_with_a_camera_and_opens_it() {
    let (mut app, dir) = app("new-scene");
    menu(&mut app, "File", "New scene");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert_eq!(app.scene(), "scenes/scene_1.scene.json", "{}", app.status());
    assert_eq!(app.status(), "new scene scenes/scene_1.scene.json");
    assert!(dir.join("scenes/scene_1.scene.json").exists());
    let names = ids(app.rows());
    assert!(
        ["camera", "ground", "sun"]
            .iter()
            .all(|n| names.contains(n)),
        "{names:?}"
    );
    // A second one takes the next free name.
    menu(&mut app, "File", "New scene");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert_eq!(app.scene(), "scenes/scene_2.scene.json");
}

/// A 2D project (release 1.1, package 1): the Create menu lists sprites
/// first, a camera made by hand is orthographic, and a new scene starts
/// with an orthographic camera and nothing else. The kind is read from the
/// project (`project_info`), nowhere else.
#[test]
fn a_2d_project_lists_sprites_first_and_starts_flat() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-menus-2d");
    let _ = std::fs::remove_dir_all(&dir);
    wish_control::scaffold::create_project_of_kind(
        &dir,
        "Flat",
        "wen",
        wish_control::project::ProjectKind::D2,
    )
    .unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));

    // The menu: Sprite above Empty above Cube (a 3D project: Empty first).
    let create = named(&app, "Create");
    click(&mut app, create);
    let y = |app: &EditorApp, text: &str| named(app, text).min.y;
    assert!(y(&app, "Sprite") < y(&app, "Empty"), "sprites first");
    assert!(y(&app, "Empty") < y(&app, "Cube"));
    let sprite = named(&app, "Sprite");
    click(&mut app, sprite);
    assert!(ids(app.rows()).contains("sprite_1"), "{:?}", app.rows());

    // A camera made by hand looks at the flat scene head on.
    menu(&mut app, "Create", "Camera");
    assert_eq!(
        components(&mut app, "camera_1")["Camera"]["projection"],
        "Orthographic"
    );

    // A new scene: an orthographic camera, no ground, no sun.
    menu(&mut app, "File", "New scene");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    assert_eq!(app.scene(), "scenes/scene_1.scene.json", "{}", app.status());
    let names: Vec<String> = app.rows().iter().map(|row| row.id.clone()).collect();
    assert_eq!(names, ["camera"], "{names:?}");
    assert_eq!(
        components(&mut app, "camera")["Camera"]["projection"],
        "Orthographic"
    );
}
