//! The AI mode (M9c), headless: the switch changes the editor's face and is
//! remembered; the game card, decisions, plan, play-test notes and answers
//! to the agent's questions are made by hand (the parity test of
//! `game_design`); the agent gets the design with every task, continues
//! its conversation, and goes on after a question is answered.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::Rect;
use serde_json::{Value, json};
use wish_agent::scripted::Scripted;
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::EditorApp;
use wish_editor::script::run_script;

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-ai-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn app(name: &str) -> (EditorApp, PathBuf) {
    let dir = folder(name);
    create_project(&dir, "Island Hop", "wen").unwrap();
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
    run_script(app, &json!({ "size": [1600, 900], "steps": steps })).unwrap();
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
        let names: Vec<String> = app
            .widgets()
            .iter()
            .filter_map(|w| w.label.clone())
            .collect();
        panic!("nothing called '{text}': {names:?}")
    })
}

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
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

fn picture(app: &EditorApp, name: &str) {
    let canvas = app.canvas();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(file, canvas.width, canvas.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&canvas.pixels)
        .unwrap();
}

fn wait_for_agent(app: &mut EditorApp) {
    let started = Instant::now();
    play(app, json!([{ "idle": 2 }]));
    while app.agent_running() && started.elapsed() < Duration::from_secs(60) {
        play(app, json!([{ "idle": 2 }]));
    }
    play(app, json!([{ "idle": 3 }]));
    assert!(!app.agent_running(), "{:?}", app.transcript());
}

#[test]
fn the_switch_changes_the_face_and_is_remembered() {
    let (mut app, dir) = app("switch");
    let prefs = dir.join("editor.json");
    app.use_preferences(prefs.clone());
    play(&mut app, json!([{ "idle": 2 }]));
    assert!(!app.ai_mode());
    assert!(find(&app, "Move").is_some(), "the manual tools are there");
    let with_ai = named(&app, "With AI");
    click(&mut app, with_ai);
    assert!(app.ai_mode());
    assert!(find(&app, "Move").is_none(), "the manual tools step aside");
    assert!(find(&app, "Save card").is_some());
    assert!(find(&app, "Send").is_some());
    assert!(find(&app, "Do the next step").is_some());
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["ai_mode"], true);
    picture(&app, "ai-mode-empty");
    // The game view shows the game; the scene view can be asked for.
    assert!(app.game_view_area().is_some());
    let scene = named(&app, "Scene");
    click(&mut app, scene);
    assert!(app.game_view_area().is_none());
    let manual = named(&app, "Manual");
    click(&mut app, manual);
    assert!(!app.ai_mode());
    assert!(find(&app, "Move").is_some());
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["ai_mode"], false);
}

#[test]
fn the_design_is_made_by_hand() {
    let (mut app, dir) = app("by-hand");
    app.set_ai_mode(true);
    play(&mut app, json!([{ "idle": 2 }]));
    // The game card.
    type_into(
        &mut app,
        "In one sentence",
        "Hop the islands and light the lighthouses",
    );
    type_into(&mut app, "Genre", "platformer");
    let save = named(&app, "Save card");
    click(&mut app, save);
    assert_eq!(
        app.design()["brief"]["pitch"],
        "Hop the islands and light the lighthouses",
        "{}",
        app.status()
    );
    assert_eq!(app.design()["brief"]["genre"], "platformer");
    // A decision.
    let decisions = app
        .widgets()
        .iter()
        .find(|w| {
            w.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Decisions ("))
        })
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, decisions);
    type_into(&mut app, "A new decision", "Pastel colours only");
    let add = named(&app, "Add decision");
    click(&mut app, add);
    assert_eq!(app.design()["decisions"][0]["text"], "Pastel colours only");
    assert_eq!(app.design()["decisions"][0]["by"], "human");
    // A plan step, marked as it goes, then removed.
    type_into(&mut app, "A new step", "Make the second island");
    let add = named(&app, "Add step");
    click(&mut app, add);
    assert_eq!(app.design()["plan"][0]["status"], "todo");
    let mark = named(&app, "to do");
    click(&mut app, mark);
    assert_eq!(app.design()["plan"][0]["status"], "doing");
    // A play-test note, with its picture.
    let notes = app
        .widgets()
        .iter()
        .find(|w| {
            w.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Play-test notes ("))
        })
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, notes);
    type_into(&mut app, "What did you notice?", "The jump feels floaty");
    let take = named(&app, "Take a note");
    click(&mut app, take);
    let note = &app.design()["notes"][0];
    assert_eq!(note["text"], "The jump feels floaty", "{}", app.status());
    let picture_file = note["picture"].as_str().expect("a picture").to_owned();
    assert!(dir.join(&picture_file).is_file());
    picture(&app, "ai-mode-filled");
    let done = named(&app, "Done");
    click(&mut app, done);
    assert_eq!(app.design()["notes"][0]["status"], "done");
    // Everything is in the file, by the person.
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("design/game.json")).unwrap())
            .unwrap();
    assert_eq!(file["brief"]["genre"], "platformer");
    assert_eq!(file["plan"][0]["by"], "human");
}

#[test]
fn the_agent_asks_the_person_answers_and_it_goes_on() {
    let (mut app, _dir) = app("questions");
    app.set_ai_mode(true);
    app.call(
        "game_design",
        &json!({ "action": "set_brief", "fields": { "pitch": "Hop the islands" } }),
    );
    app.call(
        "game_design",
        &json!({ "action": "decide", "text": "Jump height 2.5 m" }),
    );
    // The agent asks, and stops.
    app.set_agent(Box::new(Scripted {
        name: "scripted".to_owned(),
        says: "One question first.".to_owned(),
        calls: vec![(
            "game_design".to_owned(),
            json!({ "action": "ask", "text": "How many islands?", "options": ["3", "5", "10"] }),
        )],
    }));
    play(&mut app, json!([{ "idle": 2 }]));
    type_into(
        &mut app,
        "Tell the agent what to make or change",
        "make the level",
    );
    let send = named(&app, "Send");
    click(&mut app, send);
    wait_for_agent(&mut app);
    let task = app.last_task().unwrap().clone();
    let design = task.design.clone().expect("the design goes with the task");
    assert!(design.contains("Hop the islands"), "{design}");
    assert!(design.contains("Jump height 2.5 m"), "{design}");
    assert!(task.resume.is_none(), "a new conversation");
    let instructions = app.last_instructions().unwrap();
    assert!(instructions.contains("game_design ask"), "{instructions}");
    // The question waits in the conversation; the person picks an answer.
    play(&mut app, json!([{ "idle": 2 }]));
    picture(&app, "ai-mode-question");
    let five = named(&app, "5");
    click(&mut app, five);
    wait_for_agent(&mut app);
    assert_eq!(app.design()["questions"][0]["answer"], "5");
    assert_eq!(
        app.design()["decisions"][1]["text"],
        "How many islands? → 5",
        "kept as a decision"
    );
    // The agent went on, in the same conversation, with the answer.
    let task = app.last_task().unwrap();
    assert!(
        task.request.contains("How many islands?"),
        "{}",
        task.request
    );
    assert!(task.request.contains('5'));
    assert_eq!(task.resume.as_deref(), Some("scripted-conversation"));
    assert!(
        task.design
            .as_deref()
            .unwrap()
            .contains("How many islands? → 5")
    );
    // Each task saved a checkpoint first: its card can be undone.
    assert!(find(&app, "Undo this task").is_some());
}

#[test]
fn the_plan_notes_and_problems_become_tasks() {
    let (mut app, _dir) = app("tasks");
    app.set_ai_mode(true);
    app.set_agent(Box::new(Scripted {
        name: "scripted".to_owned(),
        says: "Done.".to_owned(),
        calls: Vec::new(),
    }));
    app.call(
        "game_design",
        &json!({ "action": "plan_add", "text": "Add a second island" }),
    );
    app.call(
        "game_design",
        &json!({ "action": "note", "text": "The gap is too wide", "tick": 120 }),
    );
    // Changes made elsewhere show at the next look at the project.
    play(&mut app, json!([{ "seconds": 2.5 }, { "idle": 3 }]));
    let next = named(&app, "Do the next step");
    click(&mut app, next);
    wait_for_agent(&mut app);
    let request = app.last_task().unwrap().request.clone();
    assert!(request.contains("[1] Add a second island"), "{request}");
    let notes = app
        .widgets()
        .iter()
        .find(|w| {
            w.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Play-test notes ("))
        })
        .map(|w| w.rect)
        .unwrap();
    click(&mut app, notes);
    let send = named(&app, "Send the notes to the agent");
    click(&mut app, send);
    wait_for_agent(&mut app);
    let request = app.last_task().unwrap().request.clone();
    assert!(
        request.contains("The gap is too wide (tick 120)"),
        "{request}"
    );
    // A ready request is written into the box, not sent.
    let ready = named(&app, "Find bugs");
    click(&mut app, ready);
    assert!(!app.agent_running());
    assert!(find(&app, "Send").is_some());
}

#[test]
fn the_ai_settings_go_with_the_task_and_are_kept() {
    let (mut app, dir) = app("settings");
    let prefs = dir.join("editor.json");
    app.use_preferences(prefs.clone());
    app.set_ai_mode(true);
    app.set_agent(Box::new(Scripted {
        name: "scripted".to_owned(),
        says: "Done.".to_owned(),
        calls: Vec::new(),
    }));
    play(&mut app, json!([{ "idle": 2 }]));
    let open = named(&app, "AI settings");
    click(&mut app, open);
    picture(&app, "ai-mode-settings");
    let sonnet = named(&app, "Sonnet");
    click(&mut app, sonnet);
    let files = named(&app, "The agent may change the game code itself");
    click(&mut app, files);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(saved["agent"]["model"], "sonnet");
    assert_eq!(saved["agent"]["files"], false);
    type_into(&mut app, "Tell the agent what to make or change", "hello");
    let send = named(&app, "Send");
    click(&mut app, send);
    wait_for_agent(&mut app);
    let task = app.last_task().unwrap();
    assert_eq!(task.model.as_deref(), Some("sonnet"));
    assert!(!task.files);
    // A new conversation forgets the agent's.
    let fresh = named(&app, "New conversation");
    click(&mut app, fresh);
    type_into(&mut app, "Tell the agent what to make or change", "again");
    let send = named(&app, "Send");
    click(&mut app, send);
    wait_for_agent(&mut app);
    assert!(app.last_task().unwrap().resume.is_none());
}

#[test]
fn the_window_icon_is_the_mark_on_a_dark_tile() {
    let (pixels, width, height) = wish_editor::window_icon(64);
    assert_eq!((width, height), (64, 64));
    assert_eq!(pixels.len(), 64 * 64 * 4);
    let at = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
    // The corner is outside the rounded tile; the middle of the tile is dark.
    assert_eq!(at(0, 0)[3], 0);
    assert_eq!(at(4, 32), &[29, 28, 25, 255]);
    // The bone's keel, bone-coloured, and the winning arm, orange.
    let orange = (0..64)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = at(x, y);
            p[0] > 230 && p[1] < 140 && p[2] < 90
        })
        .count();
    assert!(orange > 20, "{orange}");
}
