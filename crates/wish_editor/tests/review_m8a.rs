//! Regression tests of the M8a closing review (the editor's findings).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

fn project(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-r-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    dir
}

fn open(dir: &Path) -> EditorApp {
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    app
}

fn play(app: &mut EditorApp, steps: Value) {
    run_script(app, &json!({ "size": [1280, 720], "steps": steps })).unwrap();
}

fn find(app: &EditorApp, text: &str) -> Option<Rect> {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .find(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
}

fn named(app: &EditorApp, text: &str) -> Rect {
    find(app, text).unwrap_or_else(|| panic!("nothing called '{text}' ({})", app.status()))
}

fn at(rect: Rect) -> [f32; 2] {
    [rect.center().x, rect.center().y]
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": at(rect) }] }, { "idle": 2 }]),
    );
}

fn right_click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([
            { "events": [{ "pointer": at(rect) }] },
            { "events": [{ "press": "secondary" }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
}

fn ctrl(app: &mut EditorApp, key: &str) {
    play(
        app,
        json!([
            { "events": [{ "pointer": [600, 300] }, { "modifiers": { "ctrl": true } }] },
            { "events": [{ "key": key, "ctrl": true }] },
            { "events": [{ "key": key, "ctrl": true, "up": true }, { "modifiers": {} }] },
            { "idle": 2 }
        ]),
    );
}

/// Finding 1: a file whose name has capitals (or starts with a digit) is
/// placed under an id the scene accepts.
#[test]
fn a_file_with_capitals_in_its_name_is_placed() {
    let dir = project("capitals");
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join("prefabs/2D Big-Lamp.prefab.json"),
        r#"{ "format": 1, "entities": { "root": { "components": { "Transform": {} } } } }"#,
    )
    .unwrap();
    let mut app = open(&dir);
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    let row = named(&app, "prefabs/2D Big-Lamp.prefab.json");
    play(
        &mut app,
        json!([{ "events": [{ "click": at(row) }] }, { "events": [{ "click": at(row) }] }, { "idle": 3 }]),
    );
    assert!(
        ids(app.rows())
            .iter()
            .any(|id| id.starts_with("e_2d_big_lamp")),
        "{} {:?}",
        app.status(),
        ids(app.rows())
    );
}

/// Findings 2 and 3: while the game runs, Undo, Redo, another scene and
/// "Make prefab" are not offered (they are about the files).
#[test]
fn while_playing_the_file_actions_wait() {
    let dir = project("playing");
    let mut app = open(&dir);
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "pillar", "components": { "Transform": {} } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let file = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    ctrl(&mut app, "P");
    assert!(app.playing());
    ctrl(&mut app, "Z");
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file,
        "undo did not touch the file while playing"
    );
    let pillar = named(&app, "pillar");
    right_click(&mut app, pillar);
    assert!(find(&app, "Make prefab").is_none());
    play(
        &mut app,
        json!([{ "events": [{ "key": "Escape" }] }, { "idle": 2 }]),
    );
    ctrl(&mut app, "P");
    assert!(!app.playing());
    ctrl(&mut app, "Z");
    assert!(!ids(app.rows()).contains("pillar"), "undo works again");
}

/// Finding 4: another scene's resources are not shown (and edited) after a
/// switch.
#[test]
fn the_scene_resources_follow_the_scene() {
    let dir = project("resources");
    std::fs::write(
        dir.join("scenes/other.scene.json"),
        r#"{ "format": 1, "entities": {} }"#,
    )
    .unwrap();
    let mut app = open(&dir);
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "set_resource", "name": "Exposure", "value": { "ev100": 9.0 } }] }),
    );
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    assert!(find(&app, "Exposure").is_some(), "shown for main");
    let picker = named(&app, "scenes/main.scene.json");
    click(&mut app, picker);
    let other = app
        .widgets()
        .iter()
        .filter(|w| {
            w.label.as_deref() == Some("scenes/other.scene.json")
                || w.value.as_deref() == Some("scenes/other.scene.json")
        })
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .map(|w| w.rect)
        .next()
        .unwrap();
    click(&mut app, other);
    assert_eq!(app.scene(), "scenes/other.scene.json");
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(
        find(&app, "Exposure").is_none(),
        "main's Exposure is not shown"
    );
}

/// Finding 5: notes typed for one entity stay with it when the next click
/// selects another.
#[test]
fn notes_stay_with_the_entity_they_were_typed_for() {
    let dir = project("notes");
    let mut app = open(&dir);
    app.select(&["ground"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let notes = app
        .widgets()
        .iter()
        .find(|w| w.role == "MultilineTextInput")
        .map(|w| w.rect)
        .unwrap();
    let sun = named(&app, "sun");
    play(
        &mut app,
        json!([
            { "events": [{ "click": at(notes) }] },
            { "idle": 1 },
            { "events": [{ "text": "the floor" }] },
            { "idle": 1 },
            { "events": [{ "click": at(sun) }] },
            { "idle": 2 }
        ]),
    );
    let got = app.call("entity_get", &json!({ "ids": ["ground", "sun"] }));
    assert_eq!(got["entities"][0]["notes"], "the floor", "{got}");
    assert!(got["entities"][1]["notes"].is_null(), "{got}");
}

/// Finding 7: going back on the timeline drops the moments of the run left
/// behind.
#[test]
fn rewinding_cuts_the_timeline() {
    let dir = project("timeline");
    let mut app = open(&dir);
    ctrl(&mut app, "P");
    play(&mut app, json!([{ "idle": 300 }]));
    let before = app.timeline().len();
    assert!(before >= 3, "{:?}", app.timeline());
    // The slider's first notch: drag it to the left end.
    let slider = app
        .widgets()
        .iter()
        .find(|w| w.role == "Slider")
        .map(|w| w.rect)
        .unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [slider.min.x + 2.0, slider.center().y] }, { "press": "primary" }] },
            { "events": [{ "pointer": [slider.min.x - 20.0, slider.center().y] }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 3 }
        ]),
    );
    assert_eq!(app.timeline().len(), 1, "{:?}", app.timeline());
    // Played on, new moments follow without clashing labels.
    play(&mut app, json!([{ "idle": 2 }]));
    let resume = named(&app, "Resume");
    click(&mut app, resume);
    play(&mut app, json!([{ "idle": 150 }]));
    assert!(app.timeline().len() >= 2, "{:?}", app.timeline());
}

/// Finding 8: a comma typed in a key list stays while the next key is typed.
#[test]
fn a_comma_can_be_typed_in_a_key_list() {
    let dir = project("comma");
    let mut app = open(&dir);
    let menu = named(&app, "File");
    click(&mut app, menu);
    let settings = named(&app, "Settings...");
    click(&mut app, settings);
    let label = app
        .widgets()
        .iter()
        .find(|w| (w.role == "Label" || w.role == "TextRun") && w.value.as_deref() == Some("jump"))
        .map(|w| w.rect)
        .unwrap();
    let field = app
        .widgets()
        .iter()
        .filter(|w| w.role == "TextInput" && w.rect.min.x > label.max.x)
        .filter(|w| (w.rect.center().y - label.center().y).abs() < 30.0)
        .map(|w| w.rect)
        .next()
        .unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "click": at(field) }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "Space," }] },
            { "idle": 2 },
            { "events": [{ "text": " Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.role == "TextInput" && w.value.as_deref() == Some("Space, Enter")),
        "{:?}",
        app.widgets()
            .iter()
            .filter(|w| w.role == "TextInput")
            .map(|w| w.value.clone())
            .collect::<Vec<_>>()
    );
}

/// Finding 10: Delete on a part of a prefab instance does nothing (the part
/// belongs to the prefab), instead of failing.
#[test]
fn a_prefab_part_is_not_deleted_by_the_delete_key() {
    let dir = project("part");
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join("prefabs/lamp.prefab.json"),
        r#"{ "format": 1, "entities": {
            "root": { "components": { "Transform": {} } },
            "bulb": { "parent": "root", "components": { "Transform": {} } } } }"#,
    )
    .unwrap();
    let mut app = open(&dir);
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "instantiate", "id": "lamp_1", "prefab": "prefabs/lamp.prefab.json" }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["lamp_1/bulb"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let entries = |app: &mut EditorApp| {
        app.call("edit_history", &json!({ "action": "list" }))["entries"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let before = entries(&mut app);
    play(
        &mut app,
        json!([{ "events": [{ "pointer": [100, 300] }, { "key": "Delete" }] }, { "idle": 2 }]),
    );
    assert_eq!(entries(&mut app), before);
    assert!(!app.status().contains("failed"), "{}", app.status());
    assert!(ids(app.rows()).contains("lamp_1"));
}
