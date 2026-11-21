//! Playing the game in the editor (M8a WP7): play, pause, step, stop, the
//! timeline, live edits and the keys going to the game.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

/// A project with a box that falls onto the ground when the game runs.
fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-p-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    let added = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "box", "components": {
            "Transform": { "translation": [0.0, 3.0, 0.0] }, "Primitive": { "shape": "Cube" },
            "Material": { "base_color": [0.9, 0.1, 0.1, 1] },
            "RigidBody": { "kind": "Dynamic" }, "Collider": { "shape": "Box" } } }] }),
    );
    assert_eq!(added["ok"], true, "{added}");
    play(&mut app, json!([{ "seconds": 0.5 }, { "idle": 2 }]));
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
        .unwrap_or_else(|| panic!("nothing called '{text}'"))
}

fn click(app: &mut EditorApp, text: &str) {
    let rect = named(app, text);
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 1 }]),
    );
}

/// Real time passing: `seconds` in frames of a sixtieth.
fn wait(app: &mut EditorApp, seconds: f64) {
    let frames = (seconds * 60.0).round() as u64;
    play(app, json!([{ "idle": frames }]));
}

fn box_height(app: &mut EditorApp) -> f64 {
    let (session, ..) = app.play_state().expect("playing");
    let session = session.to_owned();
    let got = app.call(
        "entity_get",
        &json!({ "session": session, "ids": ["box"], "components": ["Transform"] }),
    );
    got["entities"][0]["components"]["Transform"]["translation"][1]
        .as_f64()
        .unwrap()
}

#[test]
fn play_runs_the_game_and_stop_brings_the_scene_back() {
    let (mut app, dir) = app("run");
    let file = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    click(&mut app, "Play");
    assert!(app.playing());
    wait(&mut app, 1.0);
    let (_, tick, paused) = app.play_state().unwrap();
    assert!(!paused);
    assert!((55..=70).contains(&tick), "about a second of ticks: {tick}");
    assert!(box_height(&mut app) < 2.5, "the box fell");
    click(&mut app, "Stop");
    assert!(!app.playing());
    assert!(app.status().starts_with("stopped"), "{}", app.status());
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file
    );
}

#[test]
fn pause_holds_the_game_and_step_runs_one_tick() {
    let (mut app, _dir) = app("pause");
    click(&mut app, "Play");
    wait(&mut app, 0.5);
    click(&mut app, "Pause");
    let (_, held, paused) = app.play_state().unwrap();
    assert!(paused);
    wait(&mut app, 0.5);
    assert_eq!(
        app.play_state().unwrap().1,
        held,
        "nothing runs while paused"
    );
    click(&mut app, "Step");
    assert_eq!(app.play_state().unwrap().1, held + 1, "one tick");
    click(&mut app, "Resume");
    wait(&mut app, 0.3);
    assert!(app.play_state().unwrap().1 > held + 5);
}

#[test]
fn the_timeline_goes_back_to_an_earlier_moment() {
    let (mut app, _dir) = app("timeline");
    click(&mut app, "Play");
    wait(&mut app, 5.0);
    let timeline = app.timeline();
    assert!(timeline.len() >= 3, "a snapshot every 2 s: {timeline:?}");
    assert_eq!(timeline[0].1, 0, "the first is at the start");
    let fallen = box_height(&mut app);
    assert!(fallen < 1.0, "{fallen}");
    // The slider's left end is the start.
    let slider = app
        .widgets()
        .iter()
        .find(|w| w.role == "Slider")
        .map(|w| w.rect)
        .expect("the timeline");
    // Press at the left end, let go: back to the start.
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [slider.min.x + 2.0, slider.center().y] }] },
            { "events": [{ "press": "primary" }] },
            { "events": [{ "release": "primary" }] },
            { "idle": 2 }
        ]),
    );
    let (_, tick, paused) = app.play_state().unwrap();
    assert_eq!(tick, 0, "back at the start");
    assert!(paused, "paused there");
    assert!(
        (box_height(&mut app) - 3.0).abs() < 1e-3,
        "the box is up again"
    );
}

#[test]
fn a_change_made_while_playing_is_live_and_not_kept() {
    let (mut app, dir) = app("live");
    let file = std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap();
    click(&mut app, "Play");
    wait(&mut app, 0.2);
    click(&mut app, "Pause");
    app.select(&["box"]);
    play(&mut app, json!([{ "idle": 2 }]));
    // The inspector shows the running game's values: change the box's
    // material there (scrolled into sight: a running game has more
    // components, and the list is longer).
    let find = |app: &EditorApp| {
        let label = app
            .widgets()
            .iter()
            .find(|w| {
                w.role == "Label" && w.value.as_deref() == Some("metallic") && w.rect.min.x > 900.0
            })
            .map(|w| w.rect)
            .expect("metallic");
        app.widgets()
            .iter()
            .find(|w| {
                w.role == "SpinButton"
                    && (w.rect.center().y - label.center().y).abs() < 8.0
                    && w.rect.min.x > label.max.x
            })
            .map(|w| w.rect)
            .unwrap()
    };
    let mut field = find(&app);
    for _ in 0..10 {
        if field.max.y < 480.0 {
            break;
        }
        play(
            &mut app,
            // (Scrolling is animated: wait for it to settle.)
            json!([{ "events": [{ "pointer": [1100, 300] }, { "scroll": [0, -120] }] }, { "idle": 30 }]),
        );
        field = find(&app);
    }
    play(
        &mut app,
        json!([
            { "events": [{ "click": [field.center().x, field.center().y] }] },
            { "idle": 1 },
            { "events": [{ "key": "A", "ctrl": true }] },
            { "events": [{ "text": "0.7" }] },
            { "events": [{ "key": "Enter" }] },
            { "idle": 2 }
        ]),
    );
    assert!(
        app.status().contains("live"),
        "status: {} / field {field:?} / widgets near: {:?}",
        app.status(),
        app.widgets()
            .iter()
            .filter(|w| (w.rect.center().y - field.center().y).abs() < 10.0)
            .map(|w| (&w.role, &w.value))
            .collect::<Vec<_>>()
    );
    let (session, ..) = app.play_state().unwrap();
    let session = session.to_owned();
    let got = app.call(
        "entity_get",
        &json!({ "session": session, "ids": ["box"], "components": ["Material"] }),
    );
    assert_eq!(
        got["entities"][0]["components"]["Material"]["metallic"],
        0.7
    );
    // Not in the file, and gone with the game.
    assert_eq!(
        std::fs::read_to_string(dir.join("scenes/main.scene.json")).unwrap(),
        file
    );
    click(&mut app, "Stop");
    let after = app.call(
        "entity_get",
        &json!({ "ids": ["box"], "components": ["Material"] }),
    );
    assert_eq!(
        after["entities"][0]["components"]["Material"]["metallic"],
        0.0
    );
}

#[test]
fn keys_and_the_pointer_over_the_view_go_to_the_game() {
    let (mut app, _dir) = app("keys");
    click(&mut app, "Play");
    let view = app.viewport();
    let (x, y) = (view.center().x, view.center().y);
    let held = |app: &mut EditorApp| {
        let (session, ..) = app.play_state().unwrap();
        let session = session.to_owned();
        let got = app.call(
            "entity_get",
            &json!({ "session": session, "resources": ["ActionState", "Pointer"] }),
        );
        let any_held = got["resources"]["ActionState"]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|slot| slot["held"] == true);
        (any_held, got["resources"]["Pointer"].clone())
    };
    // Space (jump in a new project's input map) held over the view.
    play(
        &mut app,
        json!([
            { "events": [{ "pointer": [x, y] }] },
            { "events": [{ "key": "Space" }] },
            { "idle": 6 }
        ]),
    );
    let (pressed, pointer) = held(&mut app);
    assert!(pressed, "the game sees the key held");
    assert_eq!(
        pointer["present"], true,
        "the game sees the pointer: {pointer}"
    );
    play(
        &mut app,
        json!([{ "events": [{ "key": "Space", "up": true }] }, { "idle": 6 }]),
    );
    assert!(!held(&mut app).0, "and let go");
}

/// The beta's first finding: with the game view beside the scene view (a
/// wide window), keys pressed with the pointer over the game view went
/// nowhere. The game view is where a person plays.
#[test]
fn keys_over_the_game_view_go_to_the_game() {
    let (mut app, _dir) = app("keys-game-view");
    let wide = |app: &mut EditorApp, steps: Value| {
        run_script(app, &json!({ "size": [1800, 900], "steps": steps })).unwrap();
    };
    wide(&mut app, json!([{ "idle": 3 }]));
    let start = app
        .widgets()
        .iter()
        .find(|w| w.label.as_deref() == Some("Play"))
        .map(|w| w.rect.center())
        .unwrap();
    wide(
        &mut app,
        json!([{ "events": [{ "click": [start.x, start.y] }] }, { "idle": 3 }]),
    );
    let game = app
        .game_view_area()
        .expect("a wide window shows the game view");
    assert!(!app.viewport().contains(game.center()));
    wide(
        &mut app,
        json!([
            { "events": [{ "pointer": [game.center().x, game.center().y] }] },
            { "events": [{ "key": "Space" }] },
            { "idle": 6 }
        ]),
    );
    let (session, ..) = app.play_state().unwrap();
    let session = session.to_owned();
    let got = app.call(
        "entity_get",
        &json!({ "session": session, "resources": ["ActionState"] }),
    );
    assert!(
        got["resources"]["ActionState"]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|slot| slot["held"] == true),
        "the game sees the key held over the game view: {got}"
    );
}
