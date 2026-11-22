//! The operations an agent had and the editor lacked until M8a WP9, done by
//! hand: prefab overrides reset and reverted, notes, the scene's resources,
//! the history, placeholder files, pictures, replays and tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn project(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-q-{name}"));
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
    find(app, text).unwrap_or_else(|| {
        let all: Vec<_> = app
            .widgets()
            .iter()
            .map(|w| (&w.role, &w.label, &w.value))
            .collect();
        panic!("nothing called '{text}': {all:?}")
    })
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

fn right_click(app: &mut EditorApp, rect: Rect) {
    let at = [rect.center().x, rect.center().y];
    play(
        app,
        json!([
            { "events": [{ "pointer": at }] },
            { "events": [{ "press": "secondary" }] },
            { "events": [{ "release": "secondary" }] },
            { "idle": 2 }
        ]),
    );
}

/// Opens the project menu (named after the project) and clicks `item`; a
/// submenu is opened by pointing at it first.
fn project_menu(app: &mut EditorApp, path: &[&str]) {
    let menu = named(app, top_menu(path[0]));
    click(app, menu);
    for item in path {
        let rect = named(app, item);
        play(
            app,
            json!([{ "events": [{ "pointer": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
        );
        click(app, rect);
    }
}

fn has_text(app: &EditorApp, part: &str) -> bool {
    app.widgets().iter().any(|w| {
        w.value.as_deref().is_some_and(|v| v.contains(part))
            || w.label.as_deref().is_some_and(|l| l.contains(part))
    })
}

fn scene_file(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap())
        .unwrap()
}

#[test]
fn an_override_is_reset_and_reverted_to_the_prefab() {
    let dir = project("override");
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(
        dir.join("prefabs/lamp.prefab.json"),
        r#"{ "format": 1, "entities": {
            "root": { "components": { "Transform": {} } },
            "bulb": { "parent": "root", "components": {
                "Transform": {}, "Primitive": { "shape": "Sphere" },
                "Material": { "roughness": 0.9 } } } } }"#,
    )
    .unwrap();
    let path = dir.join("scenes/main.scene.json");
    let mut scene = scene_file(&dir);
    scene["entities"]["lamp_1"] = json!({ "prefab": "prefabs/lamp.prefab.json",
        "overrides": { "bulb": { "components": { "Material": { "metallic": 0.5 } } } } });
    std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
    let mut app = open(&dir);
    app.select(&["lamp_1/bulb"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let material = |app: &mut EditorApp| {
        app.call(
            "entity_get",
            &json!({ "ids": ["lamp_1/bulb"], "components": ["Material"] }),
        )["entities"][0]["components"]["Material"]
            .clone()
    };
    assert_eq!(material(&mut app)["metallic"], 0.5);
    // Paste: the ground's material replaces the part's (an override that
    // replaces: metallic is not kept).
    app.select(&["ground"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let header = named(&app, "Material");
    right_click(&mut app, header);
    let copy = named(&app, "Copy values");
    click(&mut app, copy);
    app.select(&["lamp_1/bulb"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let header = named(&app, "Material");
    right_click(&mut app, header);
    assert!(
        find(&app, "Reset to defaults").is_none(),
        "a part reverts instead"
    );
    let paste = named(&app, "Paste values");
    click(&mut app, paste);
    let now = material(&mut app);
    assert_eq!(now["metallic"], 0.0, "{now}");
    assert_eq!(now["base_color"], json!([0.45, 0.45, 0.42, 1.0]), "{now}");
    // Revert: the override is gone, the prefab's values are back.
    play(&mut app, json!([{ "idle": 2 }]));
    let revert = named(&app, "Revert to prefab");
    click(&mut app, revert);
    let back = material(&mut app);
    assert_eq!(back["roughness"], 0.9, "{back}");
    assert!(
        scene_file(&dir)["entities"]["lamp_1"]
            .get("overrides")
            .is_none(),
        "{}",
        scene_file(&dir)
    );
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(find(&app, "Revert to prefab").is_none());
}

#[test]
fn notes_are_written_and_cleared() {
    let dir = project("notes");
    let mut app = open(&dir);
    app.select(&["ground"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let boxes = app_widgets_multiline(&app);
    assert!(!boxes.is_empty(), "a notes box");
    let rect = boxes[0];
    let heading = app
        .widgets()
        .iter()
        .find(|w| {
            w.value
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("Inspector"))
        })
        .map(|w| w.rect)
        .unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "click": [rect.center().x, rect.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": "the floor; keep it flat" }] },
            { "idle": 1 },
            { "events": [{ "click": [heading.center().x, heading.center().y] }] },
            { "idle": 2 }
        ]),
    );
    let notes = |app: &mut EditorApp| {
        app.call("entity_get", &json!({ "ids": ["ground"] }))["entities"][0]["notes"].clone()
    };
    assert_eq!(
        notes(&mut app),
        "the floor; keep it flat",
        "{}",
        app.status()
    );
    // Emptied: the notes are gone.
    let rect = app_widgets_multiline(&app)[0];
    play(
        &mut app,
        json!([
            { "events": [{ "click": [rect.center().x, rect.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "key": "Backspace" }] },
            { "idle": 1 },
            { "events": [{ "click": [heading.center().x, heading.center().y] }] },
            { "idle": 2 }
        ]),
    );
    assert!(notes(&mut app).is_null(), "{}", app.status());
}

fn app_widgets_multiline(app: &EditorApp) -> Vec<Rect> {
    app.widgets()
        .iter()
        .filter(|w| w.role == "MultilineTextInput")
        .filter(|w| w.rect.min.x > 880.0)
        .map(|w| w.rect)
        .collect()
}

#[test]
fn scene_resources_are_edited_added_reset_and_removed() {
    let dir = project("resources");
    let mut app = open(&dir);
    // Nothing selected: the scene's resources.
    assert!(has_text(&app, "Scene resources"));
    let add = named(&app, "Add resource...");
    click(&mut app, add);
    play(
        &mut app,
        json!([{ "events": [{ "text": "Exposure" }] }, { "idle": 2 }]),
    );
    let choice = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some("Exposure"))
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, choice);
    assert!(
        scene_file(&dir)["resources"].get("Exposure").is_some(),
        "{}",
        scene_file(&dir)
    );
    // A field: the box right of its label.
    let label = app
        .widgets()
        .iter()
        .find(|w| w.role == "Label" && w.value.as_deref() == Some("ev100"))
        .map(|w| w.rect)
        .unwrap();
    let field = app
        .widgets()
        .iter()
        .filter(|w| w.role == "SpinButton")
        .filter(|w| (w.rect.center().y - label.center().y).abs() < 8.0)
        .map(|w| w.rect)
        .next()
        .unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "12" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(scene_file(&dir)["resources"]["Exposure"]["ev100"], 12.0);
    // Reset to defaults, then remove.
    let header = named(&app, "Exposure");
    right_click(&mut app, header);
    let reset = named(&app, "Reset to defaults");
    click(&mut app, reset);
    assert_eq!(
        scene_file(&dir)["resources"]["Exposure"],
        json!({}),
        "{}",
        scene_file(&dir)
    );
    let remove = named(&app, "Remove resource");
    click(&mut app, remove);
    assert!(
        scene_file(&dir)
            .get("resources")
            .is_none_or(|all| all.get("Exposure").is_none()),
        "{}",
        scene_file(&dir)
    );
    // Each was one change by a human.
    let history = app.call("edit_history", &json!({ "action": "list" }));
    assert_eq!(history["entries"].as_array().unwrap().len(), 4, "{history}");
    assert!(history.to_string().contains("human"));
}

#[test]
fn the_history_tab_lists_who_changed_what() {
    let dir = project("history");
    let mut app = open(&dir);
    let agent = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "robot_made", "components": { "Transform": {} } }] }),
        &parse_actor("agent:helper"),
    );
    assert_eq!(agent["ok"], true, "{agent}");
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "hand_made", "components": { "Transform": {} } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let tab = named(&app, "History");
    click(&mut app, tab);
    assert!(has_text(&app, "hand_made"), "the human's change is listed");
    assert!(has_text(&app, "robot_made"), "the agent's change is listed");
    assert!(has_text(&app, "helper"), "with who made it");
    // Undo from the tab (its own button, below the view).
    let undo = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Undo"))
        .find(|w| w.rect.min.y > 500.0)
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, undo);
    assert!(
        !wish_editor::ids(app.rows()).contains("hand_made"),
        "{}",
        app.status()
    );
}

#[test]
fn a_placeholder_sound_and_picture_are_made() {
    let dir = project("create");
    let mut app = open(&dir);
    let new = named(&app, "New...");
    click(&mut app, new);
    let jump = named(&app, "Jump sound");
    click(&mut app, jump);
    assert_eq!(app.status(), "made assets/sounds/sound.wav");
    let sound = std::fs::read(dir.join("assets/sounds/sound.wav")).unwrap();
    assert_eq!(&sound[..4], b"RIFF");
    let new = named(&app, "New...");
    click(&mut app, new);
    let circle = named(&app, "Circle picture");
    click(&mut app, circle);
    let picture = std::fs::read(dir.join("assets/images/picture.png")).unwrap();
    assert_eq!(&picture[1..4], b"PNG");
    // A second one does not replace the first.
    let new = named(&app, "New...");
    click(&mut app, new);
    let jump = named(&app, "Jump sound");
    click(&mut app, jump);
    assert!(
        dir.join("assets/sounds/sound_2.wav").exists(),
        "{}",
        app.status()
    );
}

/// The 1.0 closing review: a material file that someone broke after the
/// browser showed it is not made again from the one field changed by hand
/// (that would wipe everything else it wrote); the change is refused and
/// the file stays as it is.
#[test]
fn a_broken_material_file_is_not_overwritten_by_hand() {
    let dir = project("material-broken");
    let mut app = open(&dir);
    let new = named(&app, "New...");
    click(&mut app, new);
    let material = named(&app, "Material file");
    click(&mut app, material);
    let path = "assets/materials/material.mat.json";
    type_number(&mut app, "alpha_cutoff", "0.25");
    assert_eq!(app.status(), format!("set alpha_cutoff in {path}"));
    // Someone half-way through editing the file by hand.
    let broken = "{\n  \"format\": 1,\n  \"material\": {\n    \"alpha_cutoff\": 0.25,\n    \"roughness\": 0.8,\n";
    std::fs::write(dir.join(path), broken).unwrap();
    type_number(&mut app, "alpha_cutoff", "0.5");
    assert_eq!(std::fs::read_to_string(dir.join(path)).unwrap(), broken);
    assert!(
        app.status()
            .starts_with(&format!("could not make {path}: ")),
        "{}",
        app.status()
    );
}

#[test]
fn a_material_file_is_made_and_changed_by_hand() {
    let dir = project("material");
    let mut app = open(&dir);
    let new = named(&app, "New...");
    click(&mut app, new);
    let material = named(&app, "Material file");
    click(&mut app, material);
    let path = "assets/materials/material.mat.json";
    assert_eq!(app.status(), format!("made {path}"));
    assert_eq!(
        std::fs::read_to_string(dir.join(path)).unwrap(),
        "{
  \"format\": 1
}
",
        "the default look"
    );
    // The new file is selected, and its fields are where the browser shows
    // it: a number is typed, a box is ticked. Each is one call by a human
    // that writes the file again, the earlier change kept.
    type_number(&mut app, "alpha_cutoff", "0.25");
    assert_eq!(app.status(), format!("set alpha_cutoff in {path}"));
    // The fields are a column under the file's name: scrolled to the box.
    let first = field_of(&app, "alpha_cutoff", "SpinButton");
    for _ in 0..20 {
        if field_of(&app, "double_sided", "CheckBox").center().y < 680.0 {
            break;
        }
        play(
            &mut app,
            json!([{ "events": [{ "pointer": [first.center().x, first.center().y] }, { "scroll": [0, -40] }] },
                   { "seconds": 0.2 }, { "idle": 2 }]),
        );
    }
    // Meanwhile someone else changes the file (an agent, a text editor):
    // the next change by hand keeps what they wrote.
    std::fs::write(
        dir.join(path),
        "{
  \"format\": 1,
  \"material\": {
    \"alpha_cutoff\": 0.25,
    \"roughness\": 0.8
  }
}
",
    )
    .unwrap();
    let tick = field_of(&app, "double_sided", "CheckBox");
    click(&mut app, tick);
    assert_eq!(app.status(), format!("set double_sided in {path}"));
    assert_eq!(
        std::fs::read_to_string(dir.join(path)).unwrap(),
        "{
  \"format\": 1,
  \"material\": {
    \"alpha_cutoff\": 0.25,
    \"double_sided\": true,
    \"roughness\": 0.8
  }
}
"
    );
    // The file's colour, its first number typed where the browser shows it.
    let red = browser_box(&mut app, "base_color", Pick::First);
    play(
        &mut app,
        json!([
            { "events": [{ "click": [red.center().x, red.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "0.5" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), format!("set base_color in {path}"));
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap();
    let colour: Vec<f64> = written["material"]["base_color"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert_eq!(colour, [0.5, 1.0, 1.0, 1.0], "{written}");
    assert_eq!(
        written["material"]["roughness"], 0.8,
        "the agent's change is kept"
    );
    let activity = app.call("activity_get", &json!({}));
    let made: Vec<&Value> = activity["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["tool"] == "asset_create")
        .collect();
    assert_eq!(made.len(), 4, "{activity}");
    assert!(made.iter().all(|entry| entry["actor"] == "human"));
    // A material file cannot name another one: no such field is offered.
    assert!(
        !app.widgets()
            .iter()
            .any(|w| w.role == "Label" && w.value.as_deref() == Some("file")),
        "no 'file' among a material file's fields"
    );
    // Clicking the menu's item made the file and picked nothing behind it.
    assert!(app.selection().is_empty(), "{:?}", app.selection());
    // An entity takes the look by naming the file in its Material: the
    // inspector's field, as any other.
    let ground = named(&app, "ground");
    click(&mut app, ground);
    let file = field_of(&app, "file", "TextInput");
    play(
        &mut app,
        json!([
            { "events": [{ "click": [file.center().x, file.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": path }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), "set Material.file");
    assert_eq!(
        scene_file(&dir)["entities"]["ground"]["components"]["Material"]["file"],
        path
    );
}

/// Something in the asset panel that `find` finds, scrolled into view
/// first (the panel shows a few rows at a time; see [`browser_widget`]).
fn browser_scrolled(app: &mut EditorApp, what: &str, find: impl Fn(&EditorApp) -> Rect) -> Rect {
    let panel = app
        .widgets()
        .iter()
        .find(|w| w.role == "ScrollBar" && w.rect.min.y > 500.0)
        .map(|w| w.rect)
        .expect("the asset panel's scroll bar");
    for _ in 0..40 {
        let found = find(app);
        let y = found.center().y;
        if y > panel.min.y + 12.0 && y < panel.max.y - 12.0 {
            return found;
        }
        let step = if y >= panel.max.y - 12.0 { -10.0 } else { 10.0 };
        play(
            app,
            json!([{ "events": [{ "pointer": [found.min.x + 4.0, panel.center().y] }, { "scroll": [0, step] }] },
                   { "seconds": 0.2 }, { "idle": 2 }]),
        );
    }
    panic!("{what} does not come into view")
}

/// A button of the asset panel, scrolled into view.
fn browser_button(app: &mut EditorApp, name: &str) -> Rect {
    browser_scrolled(app, name, |app| named(app, name))
}

/// Types `text` into the box at `field` and presses Enter.
fn type_into(app: &mut EditorApp, field: Rect, text: &str) {
    play(
        app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": text }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
}

fn written(dir: &Path, path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap()
}

#[test]
fn a_sprite_sheet_is_made_and_changed_by_hand() {
    let dir = project("sheet");
    // A picture of two 32 x 32 frames side by side.
    std::fs::create_dir_all(dir.join("assets/sprites")).unwrap();
    let picture = wish_render::RgbaImage {
        width: 64,
        height: 32,
        pixels: vec![200; 64 * 32 * 4],
    };
    std::fs::write(
        dir.join("assets/sprites/hero.png"),
        picture.to_png().unwrap(),
    )
    .unwrap();
    let mut app = open(&dir);
    // New... > Sprite sheet, with the picture selected: one frame the
    // size of the picture.
    let row = named(&app, "assets/sprites/hero.png");
    click(&mut app, row);
    let new = named(&app, "New...");
    click(&mut app, new);
    let sheet = named(&app, "Sprite sheet");
    click(&mut app, sheet);
    let path = "assets/sprites/hero.sheet.json";
    assert_eq!(app.status(), format!("made {path}"));
    assert_eq!(
        written(&dir, path),
        json!({ "format": 1, "image": "assets/sprites/hero.png",
                "grid": { "frame": [64, 32], "columns": 1, "rows": 1 } })
    );
    // Cut in two: the frame's width typed, the columns typed. Each is one
    // call by a human; numbers stay whole numbers.
    let width = browser_box(&mut app, "frame", Pick::First);
    type_into(&mut app, width, "32");
    assert_eq!(app.status(), format!("set grid in {path}"));
    let columns = browser_box(&mut app, "columns", Pick::First);
    type_into(&mut app, columns, "2");
    assert_eq!(
        written(&dir, path)["grid"],
        json!({ "frame": [32, 32], "columns": 2, "rows": 1 })
    );
    // A clip: its name typed beside "Add clip".
    let add = browser_button(&mut app, "Add clip");
    let name_box = app
        .widgets()
        .iter()
        .filter(|w| w.role == "TextInput")
        .filter(|w| (w.rect.center().y - add.center().y).abs() < 8.0 && w.rect.max.x < add.min.x)
        .map(|w| w.rect)
        .next()
        .expect("the new clip's name box");
    play(
        &mut app,
        json!([
            { "events": [{ "click": [name_box.center().x, name_box.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": "walk" }] },
            { "idle": 2 }
        ]),
    );
    let add = browser_button(&mut app, "Add clip");
    click(&mut app, add);
    assert_eq!(app.status(), format!("set clips/walk in {path}"));
    assert_eq!(
        written(&dir, path)["clips"],
        json!({ "walk": { "frames": [0] } })
    );
    // Its frames and speed, where the file is shown.
    let frames = browser_widget(&mut app, "frames", "TextInput", Pick::First);
    type_into(&mut app, frames, "[0, 1]");
    let fps = browser_box(&mut app, "fps", Pick::First);
    type_into(&mut app, fps, "8");
    assert_eq!(
        written(&dir, path)["clips"]["walk"],
        json!({ "frames": [0, 1], "fps": 8 })
    );
    // Meanwhile someone else adds a clip: the next change by hand keeps it.
    let mut theirs = written(&dir, path);
    theirs["clips"]["idle"] = json!({ "frames": [1] });
    std::fs::write(
        dir.join(path),
        serde_json::to_string_pretty(&theirs).unwrap() + "\n",
    )
    .unwrap();
    let looping = browser_widget(&mut app, "loop", "CheckBox", Pick::First);
    click(&mut app, looping);
    let clips = written(&dir, path)["clips"].clone();
    assert_eq!(clips["idle"], json!({ "frames": [1] }), "{clips}");
    assert_eq!(clips["walk"]["loop"], false, "{clips}");
    // Cut by rectangles instead, and back to a grid.
    let switch = browser_button(&mut app, "Switch to rectangles");
    click(&mut app, switch);
    let now = written(&dir, path);
    assert_eq!(
        now["frames"],
        json!([[0, 0, 32, 32], [32, 0, 32, 32]]),
        "{now}"
    );
    assert!(now.get("grid").is_none());
    let switch = browser_button(&mut app, "Switch to a grid");
    click(&mut app, switch);
    let now = written(&dir, path);
    assert_eq!(
        now["grid"],
        json!({ "frame": [32, 32], "columns": 2, "rows": 1 }),
        "{now}"
    );
    // A clip removed: the Remove on its row, under the clips' fields.
    let remove_idle = |app: &EditorApp| {
        let widgets = app.widgets();
        let row = widgets
            .iter()
            .filter(|w| w.value.as_deref() == Some("idle") || w.label.as_deref() == Some("idle"))
            .map(|w| w.rect)
            .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
            .expect("the idle clip's row");
        widgets
            .iter()
            .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Remove"))
            .filter(|w| (w.rect.center().y - row.center().y).abs() < 8.0)
            .map(|w| w.rect)
            .next()
            .expect("the idle clip's Remove")
    };
    let remove = browser_scrolled(&mut app, "the idle clip's Remove", remove_idle);
    click(&mut app, remove);
    assert!(written(&dir, path)["clips"].get("idle").is_none());
    // Every change is the person's, in the activity feed.
    let activity = app.call("activity_get", &json!({}));
    let made: Vec<&Value> = activity["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["tool"] == "asset_create")
        .collect();
    assert!(made.len() >= 9, "{activity}");
    assert!(made.iter().all(|entry| entry["actor"] == "human"));
    // An entity plays the sheet: a SpriteAnimator added in the inspector,
    // its sheet and clip typed in (on an entity with nothing else, so that
    // its fields are at the top of the inspector).
    let made = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "hero", "components": {} }] }),
    );
    assert_eq!(made["ok"], true, "{made}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["hero"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let add = named(&app, "Add component...");
    click(&mut app, add);
    play(
        &mut app,
        json!([{ "events": [{ "text": "SpriteAnimator" }] }, { "idle": 2 }]),
    );
    let choice = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("SpriteAnimator"))
        .map(|w| w.rect)
        .next()
        .expect("SpriteAnimator is offered");
    click(&mut app, choice);
    let sheet_field = field_of(&app, "sheet", "TextInput");
    type_into(&mut app, sheet_field, path);
    let clip_field = field_of(&app, "clip", "TextInput");
    type_into(&mut app, clip_field, "walk");
    let animator = scene_file(&dir)["entities"]["hero"]["components"]["SpriteAnimator"].clone();
    assert_eq!(
        animator,
        json!({ "sheet": path, "clip": "walk" }),
        "{animator}"
    );
}

#[test]
fn the_view_is_saved_as_a_picture() {
    let dir = project("picture");
    let mut app = open(&dir);
    project_menu(&mut app, &["Save picture"]);
    assert_eq!(app.status(), "picture saved as captures/main_1.png");
    let bytes = std::fs::read(dir.join("captures/main_1.png")).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    project_menu(&mut app, &["Save picture"]);
    assert!(dir.join("captures/main_2.png").exists(), "{}", app.status());
}

#[test]
fn a_run_is_recorded_and_played_back() {
    let dir = project("replay");
    let mut app = open(&dir);
    let record = named(&app, "Record");
    click(&mut app, record);
    let start = named(&app, "Play");
    click(&mut app, start);
    assert!(app.playing());
    assert!(has_text(&app, "REC"));
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 3 }]));
    let stop = named(&app, "Stop");
    click(&mut app, stop);
    assert!(
        app.status().contains("replay take_1 saved"),
        "{}",
        app.status()
    );
    assert!(dir.join("replays/take_1.replay.jsonl").exists());
    assert_eq!(app.replays(), ["take_1"]);
    project_menu(&mut app, &["Replays", "take_1"]);
    assert!(app.status().contains("plays the same"), "{}", app.status());
}

#[test]
fn the_tests_run_from_the_menu() {
    let dir = project("tests");
    let mut app = open(&dir);
    project_menu(&mut app, &["Run tests"]);
    assert_eq!(app.status(), "tests: 1 passed, 0 failed");
    std::fs::write(
        dir.join("tests/fails.test.json"),
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "seed": 1, "steps": [
            { "frames": 2 },
            { "expect": "count(Camera) == 5", "message": "five cameras" } ] }"#,
    )
    .unwrap();
    project_menu(&mut app, &["Run tests"]);
    assert_eq!(app.status(), "tests: 1 passed, 1 failed");
    assert!(
        app.problems()
            .iter()
            .any(|p| p["code"] == "test_failed" && p["message"].as_str().unwrap().contains("fails")),
        "{:?}",
        app.problems()
    );
}

/// M8b WP1: a card of the history is taken back on its own; the person's
/// later change stays (the hand-off of exit criterion 3).
#[test]
fn one_change_is_taken_back_from_the_history_tab() {
    let dir = project("revert");
    let mut app = open(&dir);
    let agent = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } }],
            "reason": "a crate to jump on" }),
        &parse_actor("agent:helper"),
    );
    assert_eq!(agent["ok"], true, "{agent}");
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "lamp", "components": { "Transform": {} } }] }),
    );
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let tab = named(&app, "History");
    click(&mut app, tab);
    assert!(
        has_text(&app, "a crate to jump on"),
        "the reason is on the card"
    );
    // Two cards can be taken back; the agent's is the lower (older) one.
    let buttons: Vec<Rect> = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Undo this"))
        .map(|w| w.rect)
        .collect();
    assert_eq!(buttons.len(), 2, "{}", app.status());
    let older = *buttons
        .iter()
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap();
    click(&mut app, older);
    assert!(app.status().starts_with("took back r"), "{}", app.status());
    let names = wish_editor::ids(app.rows());
    assert!(
        !names.contains("crate") && names.contains("lamp"),
        "{names:?}"
    );
    // The take-back is itself in the history, and Undo brings the crate back.
    let undo = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Undo"))
        .find(|w| w.rect.min.y < 72.0)
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, undo);
    assert!(wish_editor::ids(app.rows()).contains("crate"));
}

/// M8b WP2: the Activity tab shows what an agent did, with its reason, and
/// takes its edit back on its own.
#[test]
fn the_activity_tab_shows_what_an_agent_did() {
    let dir = project("activity");
    let mut app = open(&dir);
    app.workspace()
        .call("scene_get", &json!({}), &parse_actor("agent:helper"));
    let made = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } }],
            "reason": "a crate to jump on" }),
        &parse_actor("agent:helper"),
    );
    assert_eq!(made["ok"], true, "{made}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    let tab = named(&app, "Activity");
    click(&mut app, tab);
    play(&mut app, json!([{ "seconds": 0.6 }, { "idle": 2 }]));
    assert!(has_text(&app, "agent:helper"), "{:?}", app.activity());
    assert!(has_text(&app, "spawn crate"));
    assert!(has_text(&app, "a crate to jump on"));
    let only = named(&app, "Agents only");
    click(&mut app, only);
    let undo = named(&app, "Undo this");
    click(&mut app, undo);
    assert!(app.status().starts_with("took back r"), "{}", app.status());
    assert!(!wish_editor::ids(app.rows()).contains("crate"));
}

/// M8b WP3: a checkpoint saved from the menu, compared with now in the
/// before/after window, gone back to; Undo brings now back.
#[test]
fn a_checkpoint_is_saved_compared_and_gone_back_to() {
    let dir = project("checkpoint");
    let mut app = open(&dir);
    project_menu(&mut app, &["Checkpoints", "Save checkpoint"]);
    assert_eq!(app.status(), "checkpoint cp_1 saved", "{}", app.status());
    assert_eq!(app.checkpoint_names(), ["cp_1"]);
    let made = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": {
            "Transform": { "translation": [0, 1, 0] }, "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [1, 0, 0, 1] } } }] }),
        &parse_actor("agent:helper"),
    );
    assert_eq!(made["ok"], true);
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    project_menu(
        &mut app,
        &[
            "Checkpoints",
            "cp_1: saved in the editor",
            "Compare with now",
        ],
    );
    assert_eq!(app.comparing(), Some("cp_1"), "{}", app.status());
    assert!(has_text(&app, "Before (cp_1) and now"));
    let back = named(&app, "Go back to this");
    click(&mut app, back);
    assert!(
        app.status().starts_with("back to checkpoint cp_1"),
        "{}",
        app.status()
    );
    assert!(app.comparing().is_none());
    assert!(!wish_editor::ids(app.rows()).contains("crate"));
    let undo = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Undo"))
        .find(|w| w.rect.min.y < 72.0)
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, undo);
    assert!(
        wish_editor::ids(app.rows()).contains("crate"),
        "now is back"
    );
}

/// M8b WP4: in "Only suggest", an agent's changes wait in the Activity tab;
/// one is accepted (made, as the agent's), one rejected.
#[test]
fn an_agents_proposal_is_accepted_and_rejected_by_hand() {
    let dir = project("proposals");
    let mut app = open(&dir);
    project_menu(&mut app, &["Permissions", "Only suggest"]);
    assert_eq!(app.permission_mode(), "suggest", "{}", app.status());
    let agent = parse_actor("agent:helper");
    let held = app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } }],
            "reason": "a crate to jump on" }),
        &agent,
    );
    assert_eq!(held["proposed"], 1, "{held}");
    app.workspace().call(
        "edit_apply",
        &json!({ "ops": [{ "op": "despawn", "id": "sun" }] }),
        &agent,
    );
    let tab = named(&app, "Activity");
    click(&mut app, tab);
    play(&mut app, json!([{ "seconds": 0.6 }, { "idle": 2 }]));
    assert!(
        has_text(&app, "Waiting for you: 2"),
        "{:?}",
        app.waiting_proposals()
    );
    assert!(has_text(&app, "a crate to jump on"));
    assert!(
        !wish_editor::ids(app.rows()).contains("crate"),
        "not made yet"
    );
    // The first Accept (the top one) is the crate's: oldest first.
    let accept = app
        .widgets()
        .iter()
        .filter(|w| w.role == "Button" && w.label.as_deref() == Some("Accept"))
        .map(|w| w.rect)
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap();
    click(&mut app, accept);
    assert_eq!(app.status(), "proposal 1 accepted and made");
    assert!(wish_editor::ids(app.rows()).contains("crate"));
    let reject = named(&app, "Reject");
    click(&mut app, reject);
    assert_eq!(app.status(), "proposal 2 rejected");
    assert!(wish_editor::ids(app.rows()).contains("sun"));
    assert!(app.waiting_proposals().is_empty());
}

/// M9 WP1: the game exported from the Project menu.
#[test]
fn the_game_is_exported_from_the_menu() {
    let dir = project("export");
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut app = open(&dir);
    // The player program: any file stands for it here (the player's own
    // tests run the real one).
    app.workspace()
        .set_player_path(std::env::current_exe().unwrap());
    project_menu(&mut app, &["Export game"]);
    assert!(
        app.status()
            .starts_with("game exported: exports/editor_test/"),
        "{}",
        app.status()
    );
    assert!(
        dir.join("exports/editor_test/game/scenes/main.scene.json")
            .is_file()
    );
    assert!(dir.join("exports/editor_test/LICENSES.txt").is_file());
    // The export is not part of the project.
    assert!(
        !app.call("asset_list", &json!({}))
            .to_string()
            .contains("exports/")
    );
    // The exported copies are not checked as the project's own files.
    std::fs::write(
        dir.join("exports/editor_test/game/scenes/main.scene.json"),
        "not json",
    )
    .unwrap();
    assert_eq!(app.call("check", &json!({}))["passed"], true);
    // Exporting again asks before the earlier export is replaced.
    std::fs::write(dir.join("exports/editor_test/mine.txt"), "mine").unwrap();
    project_menu(&mut app, &["Export game"]);
    assert!(
        app.status().starts_with("an earlier export"),
        "{}",
        app.status()
    );
    let cancel = named(&app, "Cancel");
    click(&mut app, cancel);
    assert!(dir.join("exports/editor_test/mine.txt").is_file());
    project_menu(&mut app, &["Export game"]);
    let replace = named(&app, "Replace");
    click(&mut app, replace);
    assert!(
        app.status().starts_with("game exported: "),
        "{}",
        app.status()
    );
    assert!(!dir.join("exports/editor_test/mine.txt").exists());
    assert!(
        std::fs::read_to_string(dir.join("exports/editor_test/game/scenes/main.scene.json"))
            .unwrap()
            .contains("\"format\"")
    );
}

/// "Add resource...", type a name into its search box, click the match.
fn add_resource(app: &mut EditorApp, name: &str) {
    let add = named(app, "Add resource...");
    click(app, add);
    play(
        app,
        json!([{ "events": [{ "text": name }] }, { "idle": 2 }]),
    );
    let choice = app
        .widgets()
        .iter()
        .find(|w| w.role == "Button" && w.label.as_deref() == Some(name))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("'{name}' is not offered"));
    click(app, choice);
}

/// Which number box of a row.
#[derive(Clone, Copy)]
enum Pick {
    First,
    Last,
}

/// A number box on the row of the material file's field `label` in the
/// asset browser, after scrolling the row into view. The browser's label
/// column is left of the inspector's, which can show a field of the same
/// name. The asset panel shows a few rows at a time, between its tabs and
/// the status bar (at 1280x720 about 90 pixels): a row below that band is
/// under the status bar and takes no click, one above it is under the tabs,
/// so the panel is scrolled until the row is inside, as a person would.
fn browser_box(app: &mut EditorApp, label: &str, pick: Pick) -> Rect {
    browser_widget(app, label, "SpinButton", pick)
}

/// [`browser_box`] for a widget of any role (`TextInput` for a texture).
fn browser_widget(app: &mut EditorApp, label: &str, role: &str, pick: Pick) -> Rect {
    let row_of = |app: &EditorApp| {
        let widgets = app.widgets();
        let row = widgets
            .iter()
            .filter(|w| w.role == "Label" && w.value.as_deref() == Some(label))
            .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
            .map(|w| w.rect)
            .unwrap_or_else(|| panic!("no '{label}' row in the browser"));
        let boxes = widgets.iter().filter(|w| w.role == role).filter(|w| {
            (w.rect.center().y - row.center().y).abs() < 8.0 && w.rect.min.x > row.max.x
        });
        let field = match pick {
            Pick::First => boxes.min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x)),
            Pick::Last => boxes.max_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x)),
        }
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no {role} on the '{label}' row"));
        (row, field)
    };
    let panel = app
        .widgets()
        .iter()
        .find(|w| w.role == "ScrollBar" && w.rect.min.y > 500.0)
        .map(|w| w.rect)
        .expect("the asset panel's scroll bar");
    for _ in 0..40 {
        let (row, _) = row_of(app);
        let y = row.center().y;
        if y > panel.min.y + 12.0 && y < panel.max.y - 12.0 {
            break;
        }
        let step = if y >= panel.max.y - 12.0 { -10.0 } else { 10.0 };
        play(
            app,
            json!([{ "events": [{ "pointer": [row.min.x + 20.0, panel.center().y] }, { "scroll": [0, step] }] },
                   { "seconds": 0.2 }, { "idle": 2 }]),
        );
    }
    let (row, field) = row_of(app);
    assert!(
        row.center().y > panel.min.y && row.center().y < panel.max.y,
        "the '{label}' row is in view: {row:?} in {panel:?}"
    );
    field
}

/// The widget of `role` on the row of the inspector's field `label`.
fn field_of(app: &EditorApp, label: &str, role: &str) -> Rect {
    let widgets = app.widgets();
    let row = widgets
        .iter()
        .find(|w| w.role == "Label" && w.value.as_deref() == Some(label))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no field '{label}' in the inspector"));
    widgets
        .iter()
        .filter(|w| w.role == role)
        .filter(|w| (w.rect.center().y - row.center().y).abs() < 8.0 && w.rect.min.x > row.max.x)
        .min_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
        .map(|w| w.rect)
        .unwrap_or_else(|| panic!("no {role} next to '{label}'"))
}

/// Clicks a number box, types a value and presses Enter.
fn type_number(app: &mut EditorApp, label: &str, text: &str) {
    let field = field_of(app, label, "SpinButton");
    play(
        app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": text }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
}

/// Opens a field's list of choices and clicks one.
fn choose(app: &mut EditorApp, label: &str, choice: &str) {
    let list = field_of(app, label, "ComboBox");
    click(app, list);
    let item = named(app, choice);
    click(app, item);
}

/// Every look effect of the default look, in the order a frame draws them.
const ALL_EFFECTS: [&str; 6] = [
    "sky_light",
    "sun_disc",
    "fog",
    "bloom",
    "color_grading",
    "fxaa",
];

/// The look settings of M12 (sky light, sun disc, fog, colour grading,
/// anti-aliasing) are resources with a schema, so a person sets every one of
/// them in the inspector, with no editor code written for them (invariant 9).
/// A new scene has the default look; here a person changes each part of it,
/// and switches two of them off.
#[test]
fn the_look_of_a_scene_is_set_by_hand() {
    let dir = project("look");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!(ALL_EFFECTS), "{before}");
    assert!(has_text(&app, "Scene resources"));

    // Each resource is added from the list, one of its fields is set, and
    // its section is folded away again to keep the next one in view.
    let fold = |app: &mut EditorApp, name: &str| {
        let header = named(app, name);
        click(app, header);
    };
    add_resource(&mut app, "Fog");
    type_number(&mut app, "density", "0.02");
    type_number(&mut app, "height_falloff", "0.1");
    fold(&mut app, "Fog");
    add_resource(&mut app, "RenderSettings");
    choose(&mut app, "antialiasing", "Off");
    fold(&mut app, "RenderSettings");
    add_resource(&mut app, "Ambient");
    choose(&mut app, "source", "Color");
    type_number(&mut app, "sky_strength", "0.5");
    fold(&mut app, "Ambient");
    add_resource(&mut app, "ColorGrading");
    type_number(&mut app, "contrast", "1.2");
    type_number(&mut app, "saturation", "0.8");
    type_number(&mut app, "temperature", "0.25");
    type_number(&mut app, "vignette", "0.4");
    fold(&mut app, "ColorGrading");
    add_resource(&mut app, "Sky");
    type_number(&mut app, "sun_size_deg", "4.5");

    assert_eq!(
        scene_file(&dir)["resources"],
        json!({
            "Ambient": { "sky_strength": 0.5, "source": "Color" },
            "ColorGrading": { "contrast": 1.2, "saturation": 0.8, "temperature": 0.25, "vignette": 0.4 },
            "Fog": { "density": 0.02, "height_falloff": 0.1 },
            "RenderSettings": { "antialiasing": "Off" },
            "Sky": { "sun_size_deg": 4.5 }
        }),
        "{}",
        scene_file(&dir)
    );
    // The picture follows: the sky's light and the smoothing are off, the
    // rest is drawn with the new numbers.
    let after = app.call("capture_frame", &size);
    assert_eq!(
        after["stats"]["effects"],
        json!(["sun_disc", "fog", "bloom", "color_grading"]),
        "{after}"
    );
    assert_ne!(after["image"]["data"], before["image"]["data"]);
    // Every step was one change by a human: five resources added, ten
    // fields set.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 50 }));
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 15, "{history}");
    assert!(
        entries
            .iter()
            .all(|entry| entry.to_string().contains("human")),
        "{history}"
    );
}

/// A named look (M12 package 9, the look lock) is applied by hand from the
/// scene resources: one change by a human that writes every value of the
/// look into the scene, and the picture uses all of it. Natural is the look
/// a new scene has already, so it gives the first picture, to the byte;
/// Plain switches every effect off (invariant 9).
#[test]
fn a_look_is_applied_by_hand() {
    let dir = project("apply-look");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!(ALL_EFFECTS), "{before}");
    assert!(scene_file(&dir).get("resources").is_none());

    let menu = named(&app, "Add resource...");
    click(&mut app, menu);
    let natural = named(&app, "Natural");
    click(&mut app, natural);
    assert_eq!(app.status(), "applied the look Natural");
    let written = scene_file(&dir);
    let look = wish_control::looks::find("natural").unwrap();
    for (name, fields) in &look.resources {
        for (field, value) in fields {
            assert_eq!(
                &written["resources"][name][field], value,
                "{name}.{field} in {written}"
            );
        }
    }
    let after = app.call("capture_frame", &size);
    assert_eq!(after["stats"]["effects"], json!(ALL_EFFECTS), "{after}");
    assert_eq!(after["stats"]["shadow_cascades"], 4, "{after}");
    // The default look written out: the same picture, to the byte.
    assert_eq!(after["image"]["data"], before["image"]["data"]);

    // Plain, by hand too: no effect is left, and one shadow map.
    let menu = named(&app, "Add resource...");
    click(&mut app, menu);
    let plain = named(&app, "Plain");
    click(&mut app, plain);
    assert_eq!(app.status(), "applied the look Plain");
    let again = app.call("capture_frame", &size);
    assert_eq!(again["stats"]["effects"], json!([]), "{again}");
    assert_eq!(again["stats"]["shadow_cascades"], 1, "{again}");
    assert_ne!(again["image"]["data"], before["image"]["data"]);

    // Each look was one change by a human.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 50 }));
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{history}");
    assert!(
        entries
            .iter()
            .all(|entry| entry.to_string().contains("human")),
        "{history}"
    );
}

/// The look lock's audit of invariant 9 (M12 package 9): the look fields no
/// other by-hand test types are set by hand too. In the scene resources,
/// `Fog`'s colour, height, start and opacity, `ColorGrading`'s tint, and
/// whether and how finely shadows are drawn; in the browser, a material
/// file's texture, tiling and bump strength. Each is one call by a human.
#[test]
fn the_other_look_fields_are_set_by_hand() {
    let dir = project("look-rest");
    let mut app = open(&dir);
    let fold = |app: &mut EditorApp, name: &str| {
        let header = named(app, name);
        click(app, header);
    };
    add_resource(&mut app, "Fog");
    type_number(&mut app, "tint", "0.5");
    type_number(&mut app, "base_height", "2");
    type_number(&mut app, "start", "5");
    type_number(&mut app, "max_opacity", "0.8");
    fold(&mut app, "Fog");
    add_resource(&mut app, "RenderSettings");
    let shadows = field_of(&app, "shadows", "CheckBox");
    click(&mut app, shadows);
    type_number(&mut app, "shadow_map_size", "1024");
    fold(&mut app, "RenderSettings");
    add_resource(&mut app, "ColorGrading");
    type_number(&mut app, "tint", "0.3");
    assert_eq!(
        scene_file(&dir)["resources"],
        json!({
            "ColorGrading": { "tint": 0.3 },
            "Fog": { "base_height": 2.0, "max_opacity": 0.8, "start": 5.0, "tint": [0.5, 1.0, 1.0, 1.0] },
            "RenderSettings": { "shadow_map_size": 1024, "shadows": false }
        }),
        "{}",
        scene_file(&dir)
    );

    // A picture, a material file, and the file's texture, tiling and bumps
    // typed where the browser shows the file.
    let new = named(&app, "New...");
    click(&mut app, new);
    let circle = named(&app, "Circle picture");
    click(&mut app, circle);
    let new = named(&app, "New...");
    click(&mut app, new);
    let material = named(&app, "Material file");
    click(&mut app, material);
    let path = "assets/materials/material.mat.json";
    let texture = browser_widget(&mut app, "base_color_texture", "TextInput", Pick::First);
    play(
        &mut app,
        json!([
            { "events": [{ "click": [texture.center().x, texture.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": "assets/images/picture.png" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), format!("set base_color_texture in {path}"));
    for (label, text) in [("uv_scale", "4"), ("normal_scale", "2")] {
        let field = browser_box(&mut app, label, Pick::First);
        play(
            &mut app,
            json!([
                { "events": [{ "click": [field.center().x, field.center().y] }] },
                { "idle": 1 },
                { "events": [{ "key": "A", "ctrl": true }] },
                { "events": [{ "text": text }] },
                { "events": [{ "key": "Enter" }] },
                { "idle": 2 }
            ]),
        );
        assert_eq!(app.status(), format!("set {label} in {path}"));
    }
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap();
    assert_eq!(
        written["material"],
        json!({
            "base_color_texture": "assets/images/picture.png",
            "normal_scale": 2.0,
            "uv_scale": [4.0, 1.0]
        }),
        "{written}"
    );
    // Every change was a human's.
    let activity = app.call("activity_get", &json!({ "limit": 50 }));
    let entries = activity["entries"].as_array().unwrap();
    assert!(entries.len() >= 12, "{activity}");
    assert!(
        entries.iter().all(|entry| entry["actor"] == "human"),
        "{activity}"
    );
}

/// Bloom (M12, the second look package) is a resource with a schema like
/// the rest of the look, so a person adds it and sets its three numbers in
/// the inspector, with no editor code written for it (invariant 9).
#[test]
fn bloom_is_set_by_hand() {
    let dir = project("bloom");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    // A new scene has the default look's faint glow.
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!(ALL_EFFECTS), "{before}");

    add_resource(&mut app, "Bloom");
    type_number(&mut app, "strength", "0.2");
    type_number(&mut app, "spread", "0.7");
    type_number(&mut app, "threshold", "0.5");

    assert_eq!(
        scene_file(&dir)["resources"],
        json!({ "Bloom": { "spread": 0.7, "strength": 0.2, "threshold": 0.5 } }),
        "{}",
        scene_file(&dir)
    );
    // The picture uses it.
    let after = app.call("capture_frame", &size);
    assert_eq!(after["stats"]["effects"], json!(ALL_EFFECTS), "{after}");
    assert_ne!(after["image"]["data"], before["image"]["data"]);
    // And it is switched off by hand: a strength of 0.
    type_number(&mut app, "strength", "0");
    let off = app.call("capture_frame", &size);
    assert_eq!(
        off["stats"]["effects"],
        json!(["sky_light", "sun_disc", "fog", "color_grading", "fxaa"]),
        "{off}"
    );
    // Every step was one change by a human: the resource added, four
    // fields set.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 50 }));
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5, "{history}");
    assert!(
        entries
            .iter()
            .all(|entry| entry.to_string().contains("human")),
        "{history}"
    );
}

/// Cascaded shadows (M12, the third look package) are two fields of
/// `RenderSettings`, so a person sets them in the inspector, and the view
/// that shows the cascades is in the view list beside the others
/// (invariant 9).
#[test]
fn cascaded_shadows_are_set_by_hand() {
    let dir = project("cascades");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["shadow_cascades"], 4, "{before}");

    add_resource(&mut app, "RenderSettings");
    type_number(&mut app, "shadow_cascades", "2");
    type_number(&mut app, "shadow_distance", "60");

    assert_eq!(
        scene_file(&dir)["resources"],
        json!({ "RenderSettings": { "shadow_cascades": 2, "shadow_distance": 60.0 } }),
        "{}",
        scene_file(&dir)
    );
    // The picture uses them.
    let after = app.call("capture_frame", &size);
    assert_eq!(after["stats"]["shadow_cascades"], 2, "{after}");
    // Every step was one change by a human: the resource added, two fields
    // set.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 50 }));
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{history}");
    assert!(
        entries
            .iter()
            .all(|entry| entry.to_string().contains("human")),
        "{history}"
    );

    // The view of the cascades is picked from the view list in the top bar,
    // under its own name.
    let views = named(&app, "Shaded");
    click(&mut app, views);
    let cascades = named(&app, "Shadow cascades");
    click(&mut app, cascades);
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(
        find(&app, "Shadow cascades").is_some(),
        "the view list shows it"
    );
    assert!(
        find(&app, "Shaded").is_none(),
        "and no longer the shaded view"
    );
}

/// See-through surfaces (M12 package 8) are asked for by hand both ways: a
/// plain `Material`'s alpha typed in the inspector (the fourth box of its
/// `base_color`), and a material file whose `alpha_mode` is set to `Blend`
/// in the browser, put on the entity through the inspector's `file` field.
/// The file's own alpha is then typed in the browser. Each step is one call
/// by a human, and the count of see-through surfaces and the picture follow
/// each one (invariant 9).
#[test]
fn a_surface_is_made_see_through_by_hand() {
    let dir = project("see-through");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["blended_surfaces"], 0, "{before}");

    // 1. The ground's own Material at half alpha: see-through. Its alpha is
    // the last number box of base_color.
    let ground = named(&app, "ground");
    click(&mut app, ground);
    let widgets = app.widgets();
    let row = widgets
        .iter()
        .find(|w| w.role == "Label" && w.value.as_deref() == Some("base_color"))
        .map(|w| w.rect)
        .expect("the base_color row");
    let alpha = widgets
        .iter()
        .filter(|w| w.role == "SpinButton")
        .filter(|w| (w.rect.center().y - row.center().y).abs() < 8.0 && w.rect.min.x > row.max.x)
        .max_by(|a, b| a.rect.min.x.total_cmp(&b.rect.min.x))
        .map(|w| w.rect)
        .expect("the alpha box");
    play(
        &mut app,
        json!([
            { "events": [{ "click": [alpha.center().x, alpha.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "0.5" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), "set Material.base_color");
    assert_eq!(
        scene_file(&dir)["entities"]["ground"]["components"]["Material"]["base_color"][3],
        0.5
    );
    let plain = app.call("capture_frame", &size);
    assert_eq!(plain["stats"]["blended_surfaces"], 1, "{plain}");
    assert_ne!(plain["image"]["data"], before["image"]["data"]);

    // 2. A new material file (solid by default) named in the ground's
    // Material by hand: the file is the whole look, so the ground is solid
    // again.
    let new = named(&app, "New...");
    click(&mut app, new);
    let material = named(&app, "Material file");
    click(&mut app, material);
    let path = "assets/materials/material.mat.json";
    let ground = named(&app, "ground");
    click(&mut app, ground);
    let file = field_of(&app, "file", "TextInput");
    play(
        &mut app,
        json!([
            { "events": [{ "click": [file.center().x, file.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": path }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), "set Material.file");
    let solid = app.call("capture_frame", &size);
    assert_eq!(solid["stats"]["blended_surfaces"], 0, "{solid}");
    assert_ne!(solid["image"]["data"], plain["image"]["data"]);

    // 3. The file's alpha mode chosen from the list in the browser: the
    // ground is see-through again, through the file.
    choose(&mut app, "alpha_mode", "Blend");
    assert_eq!(app.status(), format!("set alpha_mode in {path}"));
    assert_eq!(
        std::fs::read_to_string(dir.join(path)).unwrap(),
        "{
  \"format\": 1,
  \"material\": {
    \"alpha_mode\": \"Blend\"
  }
}
"
    );
    let blended = app.call("capture_frame", &size);
    assert_eq!(blended["stats"]["blended_surfaces"], 1, "{blended}");

    // 4. The file's own alpha, typed in the browser: the last box of its
    // base_color row (the browser's label column is left of the
    // inspector's). The asset panel shows a few rows at a time between its
    // tabs and the status bar, so the row is scrolled into that band first,
    // as a person would.
    let field = browser_box(&mut app, "base_color", Pick::Last);
    play(
        &mut app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "0.3" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(app.status(), format!("set base_color in {path}"));
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap();
    assert_eq!(written["material"]["alpha_mode"], "Blend", "{written}");
    assert_eq!(written["material"]["base_color"][3], 0.3, "{written}");
    let fainter = app.call("capture_frame", &size);
    assert_eq!(fainter["stats"]["blended_surfaces"], 1, "{fainter}");
    assert_ne!(fainter["image"]["data"], blended["image"]["data"]);

    // 5. Back to solid by hand: with `Opaque` the file's alpha does
    // nothing, and the picture is the solid one to the byte.
    choose(&mut app, "alpha_mode", "Opaque");
    let again = app.call("capture_frame", &size);
    assert_eq!(again["stats"]["blended_surfaces"], 0, "{again}");
    assert_eq!(again["image"]["data"], solid["image"]["data"]);

    // Every step was one call by a human: the ground's alpha, the file
    // made and named, its mode set twice and its alpha once.
    let activity = app.call("activity_get", &json!({}));
    let by_hand: Vec<&Value> = activity["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["tool"] == "asset_create" || entry["tool"] == "edit_apply")
        .collect();
    assert_eq!(by_hand.len(), 6, "{activity}");
    assert!(by_hand.iter().all(|entry| entry["actor"] == "human"));
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

/// The lowest widget on the window (the inspector's palette is under the
/// component's fields) whose label or value is `text`, other than text,
/// scrolled into the inspector's view.
fn lowest(app: &mut EditorApp, text: &str) -> Rect {
    let panel = app
        .widgets()
        .iter()
        .filter(|w| w.role == "ScrollBar" && w.rect.min.x > 1200.0 && w.rect.min.y < 300.0)
        .map(|w| w.rect)
        .next();
    for _ in 0..40 {
        let found = lowest_now(app, text);
        let Some(panel) = panel else {
            return found;
        };
        let y = found.center().y;
        if y > panel.min.y + 8.0 && y < panel.max.y - 8.0 {
            return found;
        }
        let step = if y >= panel.max.y - 8.0 { -10.0 } else { 10.0 };
        play(
            app,
            json!([{ "events": [{ "pointer": [panel.min.x - 40.0, panel.center().y] }, { "scroll": [0, step] }] },
                   { "seconds": 0.2 }, { "idle": 2 }]),
        );
    }
    panic!("{text} does not come into view")
}

fn lowest_now(app: &EditorApp, text: &str) -> Rect {
    app.widgets()
        .iter()
        .filter(|w| w.role != "TextRun" && w.role != "Label")
        .filter(|w| w.label.as_deref() == Some(text) || w.value.as_deref() == Some(text))
        .map(|w| w.rect)
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

/// Presses on the view at cell `from` of the map, moves to cell `to` and
/// lets go: one stroke.
fn stroke(app: &mut EditorApp, from: (i32, i32), to: (i32, i32)) {
    // The 2D project's camera shows 10 m from top to bottom; the map's
    // origin is at (-4, -3).
    let area = app.scene_view_area();
    let meter = area.height() / 10.0;
    let at = |(x, y): (i32, i32)| {
        [
            area.center().x + (-4.0 + x as f32 + 0.5) * meter,
            area.center().y - (-3.0 + y as f32 + 0.5) * meter,
        ]
    };
    play(
        app,
        json!([
            { "events": [{ "pointer": at(from) }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "pointer": at(to) }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 2 }
        ]),
    );
}

/// The tiles of a layer of the map file in a rectangle, rows top first.
fn tiles(app: &mut EditorApp, map: &str, layer: &str, from: [i32; 2], to: [i32; 2]) -> Value {
    let got = app.call(
        "tilemap_edit",
        &json!({ "tilemap": map, "action": "get", "from": from, "to": to }),
    );
    assert_eq!(got["ok"], true, "{got}");
    got["rows"][layer].clone()
}

#[test]
fn a_tilemap_is_painted_by_hand() {
    use wish_control::project::ProjectKind;
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("editor-q-tilemap");
    let _ = std::fs::remove_dir_all(&dir);
    wish_control::scaffold::create_project_of_kind(&dir, "Editor Test", "wen", ProjectKind::D2)
        .unwrap();
    // A picture of two 16 x 16 tiles: grey, then orange.
    std::fs::create_dir_all(dir.join("assets/tiles")).unwrap();
    let mut pixels = Vec::new();
    for _ in 0..16 {
        for x in 0..32 {
            pixels.extend_from_slice(&if x < 16 {
                [120, 120, 130, 255]
            } else {
                [230, 140, 40, 255]
            });
        }
    }
    let picture = wish_render::RgbaImage {
        width: 32,
        height: 16,
        pixels,
    };
    std::fs::write(
        dir.join("assets/tiles/station.png"),
        picture.to_png().unwrap(),
    )
    .unwrap();
    let mut app = open(&dir);
    // New... > Tileset of the selected picture, New... > Tilemap of it.
    let row = named(&app, "assets/tiles/station.png");
    click(&mut app, row);
    let new = named(&app, "New...");
    click(&mut app, new);
    let tileset = named(&app, "Tileset");
    click(&mut app, tileset);
    let set = "assets/tiles/station.tileset.json";
    assert_eq!(app.status(), format!("made {set}"));
    assert_eq!(
        written(&dir, set),
        json!({ "format": 1, "image": "assets/tiles/station.png",
                "grid": { "frame": [16, 16], "columns": 2, "rows": 1 } })
    );
    let new = named(&app, "New...");
    click(&mut app, new);
    let tilemap = named(&app, "Tilemap");
    click(&mut app, tilemap);
    let map = "maps/map.tilemap.json";
    assert_eq!(app.status(), format!("made {map}"));
    // An entity shows it (made as any entity is).
    let done = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "map", "components": {
            "Transform": { "translation": [-4, -3, 0] }, "Tilemap": { "map": map } } }] }),
    );
    assert_eq!(done["ok"], true, "{done}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
    app.select(&["map"]);
    play(&mut app, json!([{ "idle": 3 }]));
    // The palette: the brush, tile 1, made solid.
    let brush = lowest(&mut app, "Brush");
    click(&mut app, brush);
    let one = lowest(&mut app, "1");
    click(&mut app, one);
    let solid = lowest(&mut app, "Solid tile");
    click(&mut app, solid);
    assert_eq!(
        written(&dir, set)["tiles"],
        json!({ "1": { "solid": true } }),
        "{}",
        app.status()
    );
    // A brush stroke on the walls layer (the last, chosen first): one call.
    stroke(&mut app, (0, 0), (3, 0));
    assert_eq!(app.status(), format!("painted 4 cell(s) of {map}"));
    assert_eq!(
        tiles(&mut app, map, "walls", [0, 0], [3, 0]),
        json!([[1, 1, 1, 1]])
    );
    // A rectangle.
    let rectangle = lowest(&mut app, "Rectangle");
    click(&mut app, rectangle);
    stroke(&mut app, (0, 1), (2, 3));
    assert_eq!(
        tiles(&mut app, map, "walls", [0, 1], [3, 3]),
        json!([[1, 1, 1, -1], [1, 1, 1, -1], [1, 1, 1, -1]])
    );
    // The floor, filled with tile 0 inside the map's painted part.
    let layer = lowest(&mut app, "walls");
    click(&mut app, layer);
    let floor = named(&app, "floor");
    click(&mut app, floor);
    let zero = lowest(&mut app, "0");
    click(&mut app, zero);
    let fill = lowest(&mut app, "Fill");
    click(&mut app, fill);
    stroke(&mut app, (1, 1), (1, 1));
    assert_eq!(
        tiles(&mut app, map, "floor", [0, 0], [3, 3]),
        json!([[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]])
    );
    // The eraser on the walls.
    let layer = lowest(&mut app, "floor");
    click(&mut app, layer);
    let walls = named(&app, "walls");
    click(&mut app, walls);
    let erase = lowest(&mut app, "Erase");
    click(&mut app, erase);
    stroke(&mut app, (0, 0), (1, 0));
    assert_eq!(
        tiles(&mut app, map, "walls", [0, 0], [3, 0]),
        json!([[-1, -1, 1, 1]])
    );
    // The picker takes a cell's tile and hands back the brush.
    let picker = lowest(&mut app, "Picker");
    click(&mut app, picker);
    stroke(&mut app, (2, 2), (2, 2));
    assert_eq!(app.status(), "picked tile 1");
    // Ctrl+Z while painting takes the last stroke back: the eraser's.
    play(
        &mut app,
        json!([
            { "events": [{ "modifiers": { "ctrl": true } }] },
            { "events": [{ "key": "Z", "ctrl": true }] },
            { "events": [{ "key": "Z", "ctrl": true, "up": true }, { "modifiers": {} }] },
            { "idle": 2 }
        ]),
    );
    assert_eq!(
        tiles(&mut app, map, "walls", [0, 0], [3, 0]),
        json!([[1, 1, 1, 1]])
    );
    // Every stroke and the undo are the person's, in the map's history.
    let history = app.call("edit_history", &json!({ "scene": map }));
    let actions: Vec<(String, String)> = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap().to_owned(),
                e["actor"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected: Vec<(String, String)> = ["undo", "edit", "edit", "edit", "edit"]
        .iter()
        .map(|action| ((*action).to_owned(), "human".to_owned()))
        .collect();
    assert_eq!(actions, expected, "{history}");
}

#[test]
fn an_online_match_is_played_with_two_players_by_hand() {
    let dir = project("online");
    let mut app = open(&dir);
    project_menu(&mut app, &["Players", "2 players"]);
    assert!(
        app.status().contains("online match of 2 players"),
        "{}",
        app.status()
    );
    let start = named(&app, "Play");
    click(&mut app, start);
    assert!(app.playing());
    // The bot joins, is ready, and the match starts by itself.
    play(&mut app, json!([{ "seconds": 2.0 }, { "idle": 3 }]));
    let status = app
        .network_status()
        .cloned()
        .expect("the network panel reads the match");
    assert_eq!(status["seats"].as_array().unwrap().len(), 2, "{status}");
    assert_eq!(status["lobby"]["phase"], "Running", "{status}");
    assert_eq!(status["seats"][1]["machine"], "bot", "{status}");
    // The switcher shows the bot's machine in the game view, then the host's.
    let bot = named(&app, "Player 1");
    click(&mut app, bot);
    assert_eq!(app.watched_player(), Some(1));
    let host = named(&app, "Host");
    click(&mut app, host);
    assert_eq!(app.watched_player(), None);
    let stop = named(&app, "Stop");
    click(&mut app, stop);
    assert!(!app.playing());
    assert!(app.network_status().is_none());
}
