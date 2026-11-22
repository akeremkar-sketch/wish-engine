//! The cockpit (M8b WP5), headless: the person asks, pointing at things;
//! a scripted agent works through the editor's own MCP endpoint; its change
//! is in the one history and the feed, under its name; a checkpoint was
//! saved first; the person's own edit while it works is kept.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use egui::Rect;
use serde_json::{Value, json};
use wish_agent::scripted::Scripted;
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions};
use wish_editor::script::run_script;
use wish_editor::{EditorApp, ids};

fn app(name: &str) -> EditorApp {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("editor-c-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Editor Test", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir,
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
fn an_agent_works_from_the_cockpit_and_everything_is_kept() {
    let mut app = app("agent");
    app.set_agent(Box::new(Scripted {
        name: "scripted".to_owned(),
        says: "Adding a crate.".to_owned(),
        calls: vec![
            (
                "edit_apply".to_owned(),
                json!({ "ops": [{ "op": "spawn", "id": "crate", "components": {
                    "Transform": { "translation": [0, 1, 0] }, "Primitive": { "shape": "Cube" } } }],
                    "reason": "a crate to jump on" }),
            ),
            ("describe_view".to_owned(), json!({})),
        ],
    }));
    let toggle = named(&app, "Agent");
    click(&mut app, toggle);
    // Show and tell: the selection, and something pointed at in the view.
    app.select(&["sun"]);
    play(&mut app, json!([{ "idle": 2 }]));
    let add = named(&app, "Add selection");
    click(&mut app, add);
    assert_eq!(app.agent_context().len(), 1, "{:?}", app.agent_context());
    let point = named(&app, "Point in the view");
    click(&mut app, point);
    let view = app.viewport();
    play(
        &mut app,
        json!([{ "events": [{ "click": [view.center().x, view.max.y - 40.0] }] }, { "idle": 3 }]),
    );
    assert_eq!(
        app.agent_context().len(),
        2,
        "the ground was pointed at: {:?}",
        app.agent_context()
    );
    assert!(
        app.agent_context()[1].contains("`ground`"),
        "{:?}",
        app.agent_context()
    );
    assert_eq!(app.selection(), ["sun"], "pointing does not select");
    // The request.
    let send = named(&app, "Send");
    let request = app
        .widgets()
        .iter()
        .filter(|w| w.role == "MultilineTextInput")
        .filter(|w| w.rect.max.y <= send.min.y + 2.0 && (send.min.y - w.rect.max.y) < 60.0)
        .map(|w| w.rect)
        .next()
        .unwrap();
    play(
        &mut app,
        json!([
            { "events": [{ "click": [request.center().x, request.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": "put a crate on the ground" }] },
            { "idle": 2 }
        ]),
    );
    let send = named(&app, "Send");
    click(&mut app, send);
    assert!(
        app.agent_running() || !app.transcript().is_empty(),
        "{}",
        app.status()
    );
    // While it works, the person edits something else.
    app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "lamp", "components": { "Transform": {} } }] }),
    );
    for _ in 0..200 {
        if !app.agent_running() {
            break;
        }
        play(&mut app, json!([{ "seconds": 0.05 }, { "idle": 1 }]));
    }
    assert!(!app.agent_running(), "{:?}", app.transcript());
    let transcript = app.transcript();
    assert_eq!(
        transcript[0], "You: put a crate on the ground",
        "{transcript:?}"
    );
    assert!(
        transcript.contains(&"Adding a crate.".to_owned()),
        "{transcript:?}"
    );
    assert!(
        transcript.contains(&"edit_apply: done".to_owned()),
        "{transcript:?}"
    );
    assert_eq!(
        transcript.last().map(String::as_str),
        Some("Done."),
        "{transcript:?}"
    );
    // Its change and the person's are both there; the agent's is its own.
    // (A gate under load once lost the person's lamp, 22 November 2026: the
    // history and the feed say which write replaced it.)
    let history = app
        .call("edit_history", &json!({ "limit": 50 }))
        .to_string();
    let activity = app
        .call("activity_get", &json!({ "limit": 50 }))
        .to_string();
    let names = ids(app.rows());
    assert!(
        names.contains("crate") && names.contains("lamp"),
        "{names:?}
history: {history}
activity: {activity}"
    );
    let history = app.call("edit_history", &json!({}));
    assert!(history.to_string().contains("agent:scripted"), "{history}");
    // What it was told: the request and what was pointed at.
    let told = app.last_instructions().unwrap();
    assert!(
        told.contains("`sun`") && told.contains("`ground`"),
        "{told}"
    );
    assert!(told.ends_with("put a crate on the ground"));
    // A way back was saved first.
    let checkpoints = app.call("checkpoint", &json!({ "action": "list" }));
    assert!(
        checkpoints
            .to_string()
            .contains("before: put a crate on the ground"),
        "{checkpoints}"
    );
    // The feed shows the agent.
    let feed = app.call("activity_get", &json!({ "actor": "agents" }));
    assert!(
        feed["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["reason"] == "a crate to jump on"),
        "{feed}"
    );
    let tab = named(&app, "Activity");
    click(&mut app, tab);
    play(&mut app, json!([{ "seconds": 0.6 }, { "idle": 2 }]));
    keep_picture(&mut app, "cockpit.png");
}

/// The last picture of a test, kept for people to look at
/// (`<target tmp>/cockpit.png`).
#[allow(dead_code)]
fn keep_picture(app: &mut EditorApp, name: &str) {
    let canvas = run_script(
        app,
        &json!({ "size": [1280, 720], "steps": [{ "idle": 2 }] }),
    )
    .unwrap()
    .canvas;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, canvas.width, canvas.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&canvas.pixels).unwrap();
    }
    std::fs::write(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name), out).unwrap();
}

/// M8b exit criterion 5: an outside agent (connected to the editor's
/// endpoint, as `claude mcp add` would) and the cockpit's agent work at the
/// same time, and the person too; nothing is lost, each change is its
/// maker's.
#[test]
fn an_outside_agent_and_the_cockpit_agent_work_together() {
    use wish_agent::Adapter;
    let mut app = app("together");
    // Project menu > Connect an outside agent...
    let menu = named(&app, "AI");
    click(&mut app, menu);
    let connect = named(&app, "Connect an outside agent...");
    click(&mut app, connect);
    let (url, token) = app.mcp_endpoint().expect("the endpoint is open");
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    let spawn = |id: &str, reason: &str| {
        (
            "edit_apply".to_owned(),
            json!({ "ops": [{ "op": "spawn", "id": id, "components": { "Transform": {} } }],
                "reason": reason }),
        )
    };
    let outside = Scripted {
        name: "outside".to_owned(),
        says: String::new(),
        calls: vec![
            spawn("tower", "from outside"),
            spawn("wall", "from outside"),
        ],
    };
    let mut outside_run = outside
        .start(&wish_agent::Task::new(
            "",
            url,
            token,
            std::env::temp_dir(),
            "scenes/main.scene.json",
        ))
        .unwrap();
    app.set_agent(Box::new(Scripted {
        name: "cockpit".to_owned(),
        says: "On it.".to_owned(),
        calls: vec![
            spawn("crate", "from the cockpit"),
            spawn("barrel", "from the cockpit"),
        ],
    }));
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
    play(
        &mut app,
        json!([
            { "events": [{ "click": [request.center().x, request.center().y] }] },
            { "idle": 1 },
            { "events": [{ "text": "two things" }] },
            { "idle": 2 }
        ]),
    );
    let send = named(&app, "Send");
    click(&mut app, send);
    // The person's own edit while both agents work. Its answer is kept: a
    // run under heavy load once lost this rename (21 November 2026), and the
    // answer says whether it was refused or never applied.
    let renamed = app.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "rename", "id": "sun", "to": "star" }] }),
    );
    assert_eq!(renamed["ok"], true, "the person's rename: {renamed}");
    for _ in 0..300 {
        outside_run.poll();
        if !app.agent_running() && outside_run.is_over() {
            break;
        }
        play(&mut app, json!([{ "seconds": 0.05 }, { "idle": 1 }]));
    }
    assert!(outside_run.is_over() && !app.agent_running());
    let history = app
        .call("edit_history", &json!({ "limit": 50 }))
        .to_string();
    let activity = app
        .call("activity_get", &json!({ "limit": 50 }))
        .to_string();
    let names = ids(app.rows());
    for id in ["tower", "wall", "crate", "barrel", "star"] {
        assert!(
            names.contains(id),
            "{id} is there: {names:?}
rename: {renamed}
history: {history}
activity: {activity}"
        );
    }
    for who in ["agent:outside", "agent:cockpit", "human"] {
        assert!(history.contains(who), "{who} in {history}");
    }
}
