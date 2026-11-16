//! The asset browser and prefab editing (M8a WP6), driven through the window
//! like a person: widgets are found by name, files are dragged, menus opened
//! with the right button.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::{Pos2, Rect};
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-a-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    // A sound, a picture and a prefab (made with the tools, as an agent would).
    for (path, args) in [
        (
            "assets/sounds/jump.wav",
            json!({ "sound": { "wave": "square", "seconds": 0.1 } }),
        ),
        (
            "assets/ui/ring.png",
            json!({ "picture": { "width": 40, "height": 40, "shape": "ring" } }),
        ),
    ] {
        let mut call = args;
        call["path"] = json!(path);
        assert_eq!(app.call("asset_create", &call)["ok"], true);
    }
    let prefab = app.call(
        "edit_apply",
        &json!({ "scene": "prefabs/lamp.prefab.json", "create": true, "ops": [
            { "op": "spawn", "id": "root", "components": { "Transform": {} } },
            { "op": "spawn", "id": "bulb", "parent": "root", "components": {
                "Transform": { "translation": [0, 1, 0] },
                "Primitive": { "shape": "Sphere" }, "Material": {} } }] }),
    );
    assert_eq!(prefab["ok"], true, "{prefab}");
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
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
        .unwrap_or_else(|| {
            let all: Vec<_> = app
                .widgets()
                .iter()
                .filter(|w| w.role == "Button")
                .map(|w| &w.label)
                .collect();
            panic!("nothing called '{text}': {all:?}")
        })
}

/// The drop-down list showing `text` (the scene picker).
fn combo(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .find(|w| w.role == "ComboBox" && w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no list showing '{text}'"))
}

/// The entry `text` of the list that opened under `list` (not a widget of
/// the same name elsewhere, such as a row of the browser).
fn popup_entry(app: &EditorApp, text: &str, list: Rect) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .filter(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .filter(|w| w.rect.min.y >= list.max.y && w.rect.min.y < list.max.y + 300.0)
        .map(|w| w.rect)
        .next()
        .unwrap_or_else(|| panic!("no '{text}' under the list"))
}

fn has(app: &EditorApp, text: &str) -> bool {
    app.widgets().iter().any(|w| {
        w.role != "TextRun"
            && (w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
    })
}

fn at(p: Pos2) -> Value {
    json!([p.x, p.y])
}

fn click_at(rect: Rect) -> Value {
    json!({ "events": [{ "click": at(rect.center()) }] })
}

/// Opens the context menu of a widget (right click).
fn right_click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([
            { "events": [{ "pointer": at(rect.center()) }] },
            { "events": [{ "press": "secondary" }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
}

fn entries(app: &mut EditorApp, scene: &str) -> usize {
    app.call("edit_history", &json!({ "scene": scene, "action": "list" }))["entries"]
        .as_array()
        .map_or(0, Vec::len)
}

fn translation(app: &mut EditorApp, id: &str) -> [f64; 3] {
    let got = app.call(
        "entity_get",
        &json!({ "ids": [id], "components": ["Transform"] }),
    );
    let t = &got["entities"][0]["components"]["Transform"]["translation"];
    [
        t[0].as_f64().unwrap(),
        t[1].as_f64().unwrap(),
        t[2].as_f64().unwrap(),
    ]
}

#[test]
fn the_browser_lists_files_and_prefabs_and_filters_them() {
    let (mut app, _dir) = app("list");
    for path in [
        "assets/sounds/jump.wav",
        "assets/ui/ring.png",
        "prefabs/lamp.prefab.json",
    ] {
        assert!(has(&app, path), "{path} is listed");
    }
    // Pictures get a thumbnail, not a badge.
    assert!(
        app.widgets().iter().any(|w| w.role == "Image"),
        "the picture has a thumbnail"
    );
    // Type into the filter: only what matches stays.
    let filter = app
        .widgets()
        .iter()
        .find(|w| w.role == "TextInput" && w.rect.min.y > 480.0)
        .map(|w| w.rect)
        .expect("the filter box");
    play(
        &mut app,
        json!([click_at(filter), { "idle": 1 }, { "events": [{ "text": "jump" }] }, { "idle": 2 }]),
    );
    assert!(has(&app, "assets/sounds/jump.wav"));
    assert!(!has(&app, "assets/ui/ring.png"));
    // A kind button filters by kind.
    play(
        &mut app,
        json!([{ "events": [{ "key": "A", "ctrl": true }] }, { "events": [{ "key": "Backspace" }] }, { "idle": 1 }]),
    );
    let prefabs = named(&app, "Prefabs");
    play(&mut app, json!([click_at(prefabs), { "idle": 2 }]));
    assert!(has(&app, "prefabs/lamp.prefab.json") && !has(&app, "assets/sounds/jump.wav"));
}

#[test]
fn selecting_a_file_shows_what_the_tools_know_about_it() {
    let (mut app, _dir) = app("detail");
    let row = named(&app, "assets/sounds/jump.wav");
    play(&mut app, json!([click_at(row), { "idle": 2 }]));
    // asset_info's answer: its kind and who uses it.
    assert!(
        app.widgets()
            .iter()
            .any(|w| w.value.as_deref() == Some("used_by:")),
        "the detail shows what uses it"
    );
}

#[test]
fn double_clicking_a_prefab_puts_an_instance_in_the_scene() {
    let (mut app, _dir) = app("place");
    let row = named(&app, "prefabs/lamp.prefab.json");
    let before = entries(&mut app, "scenes/main.scene.json");
    play(
        &mut app,
        json!([click_at(row), click_at(row), { "idle": 3 }]),
    );
    let rows: Vec<_> = app
        .rows()
        .iter()
        .map(|r| (r.id.clone(), r.prefab.clone()))
        .collect();
    assert!(
        rows.contains(&(
            "lamp_1".to_owned(),
            Some("prefabs/lamp.prefab.json".to_owned())
        )),
        "{rows:?}"
    );
    assert_eq!(app.selection(), ["lamp_1"]);
    assert_eq!(entries(&mut app, "scenes/main.scene.json"), before + 1);
    // Put where the view looks at the ground.
    assert!(translation(&mut app, "lamp_1")[1].abs() < 1e-3);
}

#[test]
fn dragging_a_file_onto_the_view_places_it_under_the_pointer() {
    let (mut app, _dir) = app("drag");
    let row = named(&app, "prefabs/lamp.prefab.json");
    let view = app.viewport();
    let to = Pos2::new(view.center().x + 60.0, view.max.y - 60.0);
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": at(row.center()) }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": [row.center().x + 30.0, row.center().y - 40.0] }] },
            { "events": [{ "pointer": [(row.center().x + to.x) / 2.0, (row.center().y + to.y) / 2.0] }] },
            { "events": [{ "pointer": at(to) }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 3 }
        ]),
    );
    let placed = app
        .rows()
        .iter()
        .find(|r| r.prefab.is_some())
        .map(|r| r.id.clone())
        .expect("an instance was made");
    let at_ground = translation(&mut app, &placed);
    assert!(at_ground[1].abs() < 1e-3, "on the ground: {at_ground:?}");
    // Right of the middle and nearer the camera than the middle.
    assert!(at_ground[0] > 0.0, "{at_ground:?}");
    assert!(at_ground[2] > 0.0, "{at_ground:?}");
}

#[test]
fn a_file_dropped_on_an_asset_field_sets_it() {
    let (mut app, _dir) = app("field");
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "speaker", "components": {
            "Transform": {}, "AudioSource": {} } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["speaker"]);
    play(&mut app, json!([{ "idle": 2 }]));
    // The `clip` field's text box, to the right of its label.
    let label = app
        .widgets()
        .iter()
        .find(|w| w.role == "Label" && w.value.as_deref() == Some("clip") && w.rect.min.x > 900.0)
        .map(|w| w.rect)
        .expect("the clip field");
    let boxed = app
        .widgets()
        .iter()
        .filter(|w| w.role == "TextInput" && (w.rect.center().y - label.center().y).abs() < 8.0)
        .map(|w| w.rect)
        .next()
        .expect("its text box");
    let row = named(&app, "assets/sounds/jump.wav");
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": at(row.center()) }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": [row.center().x + 40.0, row.center().y - 60.0] }] },
            { "events": [{ "pointer": at(boxed.center()) }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 3 }
        ]),
    );
    let got = app.call(
        "entity_get",
        &json!({ "ids": ["speaker"], "components": ["AudioSource"] }),
    );
    assert_eq!(
        got["entities"][0]["components"]["AudioSource"]["clip"],
        "assets/sounds/jump.wav"
    );
}

#[test]
fn moving_a_file_updates_what_uses_it() {
    let (mut app, dir) = app("move");
    // A scene entity uses the sound.
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "speaker", "components": {
            "Transform": {}, "AudioSource": { "clip": "assets/sounds/jump.wav" } } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let row = named(&app, "assets/sounds/jump.wav");
    right_click(&mut app, row);
    let rename = named(&app, "Rename / move...");
    play(
        &mut app,
        json!([
            click_at(rename),
            { "idle": 2 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "assets/sounds/hop.wav" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 3 }
        ]),
    );
    assert!(dir.join("assets/sounds/hop.wav").exists());
    assert!(!dir.join("assets/sounds/jump.wav").exists());
    let got = app.call(
        "entity_get",
        &json!({ "ids": ["speaker"], "components": ["AudioSource"] }),
    );
    assert_eq!(
        got["entities"][0]["components"]["AudioSource"]["clip"],
        "assets/sounds/hop.wav"
    );
    assert!(
        app.status()
            .starts_with("moved assets/sounds/jump.wav to assets/sounds/hop.wav"),
        "{}",
        app.status()
    );
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    assert!(has(&app, "assets/sounds/hop.wav") && !has(&app, "assets/sounds/jump.wav"));
}

#[test]
fn making_a_prefab_of_an_entity_and_its_children() {
    let (mut app, dir) = app("make");
    app.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "tower", "components": {
                "Transform": { "translation": [2.0, 0.0, -1.0] },
                "Primitive": { "shape": "Cube" } } },
            { "op": "spawn", "id": "flag", "parent": "tower", "components": {
                "Transform": { "translation": [0.0, 2.0, 0.0] },
                "Primitive": { "shape": "Sphere" } } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let tower = named(&app, "tower");
    right_click(&mut app, tower);
    let make = named(&app, "Make prefab");
    play(&mut app, json!([click_at(make), { "idle": 3 }]));
    assert_eq!(app.status(), "made prefab prefabs/tower.prefab.json");
    assert!(dir.join("prefabs/tower.prefab.json").exists());
    // The scene now holds an instance where the tower was, in the same place.
    let row = app.rows().iter().find(|r| r.id == "tower").unwrap();
    assert_eq!(row.prefab.as_deref(), Some("prefabs/tower.prefab.json"));
    assert_eq!(translation(&mut app, "tower"), [2.0, 0.0, -1.0]);
    // Its child is a part of the instance.
    let part = app.call(
        "entity_get",
        &json!({ "ids": ["tower/flag"], "components": ["Primitive"] }),
    );
    assert_eq!(
        part["entities"][0]["components"]["Primitive"]["shape"],
        "Sphere"
    );
    // The prefab file: a neutral root and the child under it.
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("prefabs/tower.prefab.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file["entities"]["flag"]["parent"], "root");
    assert!(
        file["entities"]["root"]["components"]["Transform"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    // The browser lists it.
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 2 }]));
    assert!(has(&app, "prefabs/tower.prefab.json"));
    // One undo puts the scene back as it was (the prefab file stays).
    let undo = named(&app, "Undo");
    play(&mut app, json!([click_at(undo), { "idle": 3 }]));
    let row = app.rows().iter().find(|r| r.id == "tower").unwrap();
    assert_eq!(row.prefab, None);
    assert!(
        app.rows()
            .iter()
            .any(|r| r.id == "flag" && r.parent.as_deref() == Some("tower"))
    );
}

#[test]
fn a_prefab_is_edited_in_the_editor_and_its_instances_follow() {
    let (mut app, dir) = app("prefab-edit");
    app.call("edit_apply", &json!({ "ops": [{ "op": "instantiate", "id": "lamp_a", "prefab": "prefabs/lamp.prefab.json" }] }));
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    // Open the prefab from the scene picker.
    let picker = combo(&app, "scenes/main.scene.json");
    play(&mut app, json!([click_at(picker), { "idle": 2 }]));
    let prefab_entry = popup_entry(&app, "prefabs/lamp.prefab.json", picker);
    play(&mut app, json!([click_at(prefab_entry), { "idle": 3 }]));
    assert_eq!(app.scene(), "prefabs/lamp.prefab.json");
    let ids: Vec<_> = app.rows().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["root", "bulb"]);
    // The view shows it (the default stage: a camera and a sun).
    assert_eq!(app.status(), "", "{}", app.status());
    // Select the bulb and change the roughness in the inspector.
    app.select(&["bulb"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let label = app
        .widgets()
        .iter()
        .find(|w| {
            w.role == "Label" && w.value.as_deref() == Some("roughness") && w.rect.min.x > 900.0
        })
        .map(|w| w.rect)
        .expect("roughness");
    let boxed = app
        .widgets()
        .iter()
        .find(|w| {
            w.role == "SpinButton"
                && (w.rect.center().y - label.center().y).abs() < 8.0
                && w.rect.min.x > label.max.x
        })
        .map(|w| w.rect)
        .unwrap();
    play(
        &mut app,
        json!([
            click_at(boxed),
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "0.9" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    let file = std::fs::read_to_string(dir.join("prefabs/lamp.prefab.json")).unwrap();
    assert!(file.contains("\"roughness\": 0.9"), "{file}");
    assert_eq!(entries(&mut app, "prefabs/lamp.prefab.json"), 2); // the setup's and this one
    // Back in the scene, the instance has it.
    let picker = combo(&app, "prefabs/lamp.prefab.json");
    play(&mut app, json!([click_at(picker), { "idle": 2 }]));
    let main = popup_entry(&app, "scenes/main.scene.json", picker);
    play(&mut app, json!([click_at(main), { "idle": 3 }]));
    assert_eq!(app.scene(), "scenes/main.scene.json");
    let got = app.call(
        "entity_get",
        &json!({ "ids": ["lamp_a/bulb"], "components": ["Material"] }),
    );
    assert_eq!(
        got["entities"][0]["components"]["Material"]["roughness"],
        0.9
    );
}

#[test]
fn component_values_are_copied_pasted_and_reset() {
    let (mut app, _dir) = app("copy");
    app.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "gold", "components": { "Transform": {}, "Primitive": { "shape": "Cube" },
                "Material": { "metallic": 1.0, "roughness": 0.2 } } },
            { "op": "spawn", "id": "clay", "components": { "Transform": {}, "Primitive": { "shape": "Cube" },
                "Material": {} } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let component = |app: &mut EditorApp, id: &str| {
        app.call(
            "entity_get",
            &json!({ "ids": [id], "components": ["Material"] }),
        )["entities"][0]["components"]["Material"]
            .clone()
    };
    app.select(&["gold"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let header = named(&app, "Material");
    right_click(&mut app, header);
    let copy = named(&app, "Copy values");
    play(&mut app, json!([click_at(copy), { "idle": 2 }]));
    assert_eq!(app.status(), "copied Material");
    app.select(&["clay"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let header = named(&app, "Material");
    right_click(&mut app, header);
    let paste = named(&app, "Paste values");
    play(&mut app, json!([click_at(paste), { "idle": 3 }]));
    let pasted = component(&mut app, "clay");
    assert_eq!(pasted["metallic"], 1.0);
    assert_eq!(pasted["roughness"], 0.2);
    // Reset puts the defaults back.
    let header = named(&app, "Material");
    right_click(&mut app, header);
    let reset = named(&app, "Reset to defaults");
    play(&mut app, json!([click_at(reset), { "idle": 3 }]));
    let back = component(&mut app, "clay");
    assert_eq!(back["metallic"], 0.0);
    assert_eq!(back["roughness"], 0.5);
    assert_eq!(app.status(), "reset Material");
}
