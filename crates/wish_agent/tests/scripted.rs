//! A scripted agent works through an endpoint whose calls run on the
//! owner's thread, as in the editor: its changes are in the one history,
//! under its name.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use wish_agent::scripted::Scripted;
use wish_agent::{Adapter, Event, Task};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_mcp::Server;
use wish_mcp::http::{Endpoint, HttpOptions};

#[test]
fn a_scripted_agent_changes_the_scene_through_the_endpoint() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("agent-scripted");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Agent", "wen").unwrap();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    let mut endpoint = Endpoint::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        HttpOptions {
            token: Some("secret".to_owned()),
            ..HttpOptions::default()
        },
    )
    .unwrap();
    let agent = Scripted {
        name: "scripted".to_owned(),
        says: "Adding a crate.".to_owned(),
        calls: vec![
            (
                "edit_apply".to_owned(),
                json!({ "ops": [{ "op": "spawn", "id": "crate", "components": { "Transform": {} } }],
                    "reason": "a crate to jump on" }),
            ),
            ("describe_view".to_owned(), json!({})),
        ],
    };
    let task = Task {
        request: "a crate".to_owned(),
        context: Vec::new(),
        endpoint: format!("http://{}/mcp", endpoint.address()),
        token: "secret".to_owned(),
        project: dir.clone(),
        scene: "scenes/main.scene.json".to_owned(),
    };
    let mut run = agent.start(&task).unwrap();
    let mut events = Vec::new();
    let started = Instant::now();
    while !run.is_over() {
        // The owner's loop: its own calls and the agent's, one at a time.
        endpoint.serve_pending(&mut Server::new(&mut workspace), Duration::from_millis(10));
        events.extend(run.poll());
        // A person's edit in between is kept too.
        if events.len() == 2
            && workspace.call(
                "entity_get",
                &json!({ "ids": ["lamp"] }),
                &parse_actor("human"),
            )["ok"]
                != true
        {
            workspace.call(
                "edit_apply",
                &json!({ "ops": [{ "op": "spawn", "id": "lamp", "components": { "Transform": {} } }] }),
                &parse_actor("human"),
            );
        }
        assert!(started.elapsed() < Duration::from_secs(60), "{events:?}");
    }
    assert_eq!(events[0], Event::Said("Adding a crate.".to_owned()));
    assert!(
        events.contains(&Event::Answered {
            tool: "edit_apply".to_owned(),
            ok: true
        }),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(Event::Finished { ok: true, .. })),
        "{events:?}"
    );
    let history = workspace.call("edit_history", &json!({}), &parse_actor("human"));
    let actors: Vec<&str> = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["actor"].as_str().unwrap())
        .collect();
    assert!(actors.contains(&"agent:scripted"), "{history}");
    let both = workspace.call(
        "entity_get",
        &json!({ "ids": ["crate", "lamp"] }),
        &parse_actor("human"),
    );
    assert_eq!(both["ok"], true, "nothing is lost: {both}");
}

#[test]
fn an_agent_that_cannot_reach_the_editor_says_so() {
    let task = Task {
        request: String::new(),
        context: Vec::new(),
        endpoint: "http://127.0.0.1:1/mcp".to_owned(),
        token: "x".to_owned(),
        project: PathBuf::from("."),
        scene: String::new(),
    };
    let agent = Scripted {
        name: "scripted".to_owned(),
        says: String::new(),
        calls: Vec::new(),
    };
    let mut run = agent.start(&task).unwrap();
    let started = Instant::now();
    let mut events = Vec::new();
    while !run.is_over() && started.elapsed() < Duration::from_secs(30) {
        events.extend(run.poll());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(events.as_slice(), [Event::Failed(_)]),
        "{events:?}"
    );
}

#[test]
fn the_instructions_carry_the_request_and_what_was_pointed_at() {
    let task = Task {
        request: "make it bigger".to_owned(),
        context: vec!["the entity `crate`".to_owned()],
        endpoint: String::new(),
        token: String::new(),
        project: PathBuf::from("."),
        scene: "scenes/main.scene.json".to_owned(),
    };
    let text = wish_agent::instructions(&task);
    assert!(text.ends_with("make it bigger"));
    assert!(text.contains("- the entity `crate`"));
    assert!(text.contains("reason"));
    assert!(text.contains("scenes/main.scene.json"));
}
