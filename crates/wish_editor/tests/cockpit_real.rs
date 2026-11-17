//! The cockpit with the real Claude Code (M8b exit criterion, the part a
//! script cannot stand in for). It uses the person's own Claude account and
//! spends its quota, so it runs only by hand:
//!
//! ```text
//! cargo test -p wish_editor --test cockpit_real -- --ignored --nocapture
//! ```
//!
//! Claude Code is found as the welcome screen finds it (`WISH_CLAUDE`, the
//! PATH, `~/.local/bin`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

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

fn click(app: &mut EditorApp, rect: Rect) {
    play(
        app,
        json!([{ "events": [{ "click": [rect.center().x, rect.center().y] }] }, { "idle": 2 }]),
    );
}

#[test]
#[ignore = "runs the real Claude Code on the person's account (spends quota)"]
fn the_real_claude_code_works_from_the_cockpit() {
    let program = wish_agent::claude::find().expect("Claude Code is installed");
    println!("Claude Code: {}", program.display());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cockpit-real");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Cockpit Real", "wen").unwrap();
    // Plain scene work: no game code to build.
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    workspace.set_software(true);
    let mut app = EditorApp::new(workspace);
    play(&mut app, json!([{ "idle": 2 }]));
    let toggle = named(&app, "Agent");
    click(&mut app, toggle);
    let send = named(&app, "Send");
    let request = app
        .widgets()
        .iter()
        .filter(|w| w.role == "MultilineTextInput")
        .filter(|w| w.rect.max.y <= send.min.y + 2.0 && (send.min.y - w.rect.max.y) < 60.0)
        .map(|w| w.rect)
        .next()
        .unwrap();
    let asked = "Add a red cube with the id crate, resting on the ground two metres in front of the camera. Do nothing else.";
    play(
        &mut app,
        json!([
            { "events": [{ "click": [request.center().x, request.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": asked }] },
            { "idle": 2 }
        ]),
    );
    let started = Instant::now();
    let send = named(&app, "Send");
    click(&mut app, send);
    while app.agent_running() && started.elapsed() < Duration::from_secs(600) {
        play(&mut app, json!([{ "seconds": 0.05 }, { "idle": 1 }]));
        std::thread::sleep(Duration::from_millis(100));
    }
    let seconds = started.elapsed().as_secs_f64();
    let transcript = app.transcript();
    println!("--- transcript ({seconds:.1} s) ---");
    for line in &transcript {
        println!("{line}");
    }
    println!("--- status: {}", app.status());
    assert!(
        !app.agent_running(),
        "the agent did not finish in 10 minutes"
    );
    // The change is in the scene, in the one history under the agent's
    // name, in the activity feed; a checkpoint was saved first.
    let crate_ = app.call("entity_get", &json!({ "ids": ["crate"] }));
    println!("--- crate: {crate_}");
    assert_eq!(crate_["ok"], true, "{crate_}");
    let history = app.call("edit_history", &json!({}));
    let by_agent = history["entries"].as_array().unwrap().iter().any(|entry| {
        entry["actor"]
            .as_str()
            .is_some_and(|a| a.starts_with("agent"))
    });
    assert!(by_agent, "{history}");
    let activity = app.call("activity_get", &json!({}));
    assert!(
        activity.to_string().contains("edit_apply"),
        "the agent's change is in the feed: {activity}"
    );
    // Its last message is said once, not twice.
    let said = transcript.last().unwrap();
    assert_eq!(
        transcript.iter().filter(|line| *line == said).count(),
        1,
        "{transcript:?}"
    );
    assert!(
        !app.checkpoint_names().is_empty(),
        "a checkpoint before the task"
    );
    println!("--- done in {seconds:.1} s");
}
