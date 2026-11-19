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
    let menu = named(app, "Editor Test");
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
        .find(|w| w.rect.min.y < 30.0)
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
        .find(|w| w.rect.min.y < 30.0)
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

/// The look settings of M12 (sky light, sun disc, fog, colour grading,
/// anti-aliasing) are resources with a schema, so a person sets every one of
/// them in the inspector, with no editor code written for them (invariant 9).
#[test]
fn the_look_of_a_scene_is_set_by_hand() {
    let dir = project("look");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!([]), "{before}");
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
    choose(&mut app, "antialiasing", "Fxaa");
    fold(&mut app, "RenderSettings");
    add_resource(&mut app, "Ambient");
    choose(&mut app, "source", "Sky");
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
            "Ambient": { "sky_strength": 0.5, "source": "Sky" },
            "ColorGrading": { "contrast": 1.2, "saturation": 0.8, "temperature": 0.25, "vignette": 0.4 },
            "Fog": { "density": 0.02, "height_falloff": 0.1 },
            "RenderSettings": { "antialiasing": "Fxaa" },
            "Sky": { "sun_size_deg": 4.5 }
        }),
        "{}",
        scene_file(&dir)
    );
    // The picture uses all of them.
    let after = app.call("capture_frame", &size);
    assert_eq!(
        after["stats"]["effects"],
        json!(["sky_light", "sun_disc", "fog", "color_grading", "fxaa"]),
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
/// look into the scene, and the picture uses all of it. Plain gives the
/// first picture back, to the byte (invariant 9).
#[test]
fn a_look_is_applied_by_hand() {
    let dir = project("apply-look");
    let mut app = open(&dir);
    let size = json!({ "width": 96, "height": 54 });
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!([]), "{before}");

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
    assert_eq!(
        after["stats"]["effects"],
        json!([
            "sky_light",
            "sun_disc",
            "fog",
            "bloom",
            "color_grading",
            "fxaa"
        ]),
        "{after}"
    );
    assert_eq!(after["stats"]["shadow_cascades"], 4, "{after}");

    // Plain, by hand too: the picture of the defaults again, to the byte.
    let menu = named(&app, "Add resource...");
    click(&mut app, menu);
    let plain = named(&app, "Plain");
    click(&mut app, plain);
    assert_eq!(app.status(), "applied the look Plain");
    let again = app.call("capture_frame", &size);
    assert_eq!(again["stats"]["effects"], json!([]), "{again}");
    assert_eq!(again["image"]["data"], before["image"]["data"]);

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
    let before = app.call("capture_frame", &size);
    assert_eq!(before["stats"]["effects"], json!([]), "{before}");

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
    assert_eq!(after["stats"]["effects"], json!(["bloom"]), "{after}");
    assert_ne!(after["image"]["data"], before["image"]["data"]);
    // Every step was one change by a human: the resource added, three
    // fields set.
    let history = app.call("edit_history", &json!({ "action": "list", "limit": 50 }));
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4, "{history}");
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
    assert_eq!(before["stats"]["shadow_cascades"], 1, "{before}");

    add_resource(&mut app, "RenderSettings");
    type_number(&mut app, "shadow_cascades", "4");
    type_number(&mut app, "shadow_distance", "60");

    assert_eq!(
        scene_file(&dir)["resources"],
        json!({ "RenderSettings": { "shadow_cascades": 4, "shadow_distance": 60.0 } }),
        "{}",
        scene_file(&dir)
    );
    // The picture uses them.
    let after = app.call("capture_frame", &size);
    assert_eq!(after["stats"]["shadow_cascades"], 4, "{after}");
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
