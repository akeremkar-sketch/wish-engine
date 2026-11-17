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
        .find(|w| w.value.as_deref() == Some("Inspector"))
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
        .filter(|w| w.role == "MultilineTextInput" || w.role == "TextInput")
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
