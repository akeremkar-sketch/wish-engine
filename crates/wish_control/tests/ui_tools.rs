//! The in-game interface through the tools (M6 WP7): a tool clicks a button
//! with the pointer, sees the interface in describe_view, replays the
//! clicks; fonts are checked and the inspector names interface mistakes.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ui-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Ui", "wen").unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "start", "components": {
                "UiNode": { "position": "Absolute", "offset": [540, 300],
                    "width": { "unit": "Px", "value": 200 }, "height": { "unit": "Px", "value": 80 },
                    "padding": [20, 20, 20, 20], "background": [0.1, 0.3, 0.9, 1] },
                "UiButton": {} } },
            { "op": "spawn", "id": "start_label", "parent": "start", "components": {
                "UiNode": {}, "UiText": { "text": "Start", "size": 32 } } }
        ] }),
    );
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn components(ws: &mut Workspace, session: &str, id: &str) -> Value {
    call(ws, "entity_get", json!({ "session": session, "ids": [id] }))["entities"][0]["components"]
        .clone()
}

fn start(ws: &mut Workspace) -> String {
    call(ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_tool_clicks_a_button_with_the_pointer() {
    let (_dir, mut ws) = project("click");
    let s = start(&mut ws);
    // Laid out before the first tick.
    let layout = components(&mut ws, &s, "start")["UiLayout"].clone();
    assert_eq!(layout["position"], json!([540.0, 300.0]), "{layout}");
    assert_eq!(layout["size"], json!([200.0, 80.0]));

    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340], "click": true } }),
    );
    let held = components(&mut ws, &s, "start")["UiInteraction"].clone();
    assert_eq!(
        (held["hovered"].clone(), held["pressed"].clone()),
        (json!(true), json!(true))
    );
    let released = call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340] } }),
    );
    assert_eq!(released["events"]["ButtonClicked"], 1, "{released}");
    let state = components(&mut ws, &s, "start")["UiInteraction"].clone();
    assert_eq!(state["clicks"], 1);
    assert_eq!(state["pressed"], false);

    // A click on the label inside counts for the button; a click elsewhere
    // does not.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [100, 100], "click": true } }),
    );
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [100, 100] } }),
    );
    assert_eq!(
        components(&mut ws, &s, "start")["UiInteraction"]["clicks"],
        1
    );

    // The pointer resource is in the world for game code and conditions.
    let until = call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "until": "Pointer.present", "max_frames": 1,
                "input": { "pointer": [560, 320] } }),
    );
    assert_eq!(until["stopped_by"], "until", "{until}");
}

#[test]
fn describe_view_lists_the_interface_with_its_text() {
    let (_dir, mut ws) = project("describe");
    // The scene file, with no session.
    let scene = call(
        &mut ws,
        "describe_view",
        json!({ "width": 640, "height": 360 }),
    );
    let interface = scene["interface"].as_array().unwrap();
    let label = interface
        .iter()
        .find(|e| e["entity"] == "start_label")
        .unwrap();
    assert_eq!(label["text"], "Start");
    let button = interface.iter().find(|e| e["entity"] == "start").unwrap();
    assert_eq!(button["bounding_box"], json!([270, 150, 370, 190]));
    assert_eq!(button["button"]["hovered"], false);
    // A session, with the pointer over the button.
    let s = start(&mut ws);
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340] } }),
    );
    let live = call(
        &mut ws,
        "describe_view",
        json!({ "session": s, "width": 640, "height": 360 }),
    );
    let button = live["interface"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["entity"] == "start")
        .unwrap()
        .clone();
    assert_eq!(button["button"]["hovered"], true, "{live}");
    // The picture shows it.
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "session": s, "width": 128, "height": 72 }),
    );
    assert!(picture.get("diagnostics").is_none(), "{picture}");
}

#[test]
fn clicks_replay_bit_for_bit() {
    let (_dir, mut ws) = project("replay");
    let s = start(&mut ws);
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    for input in [
        json!({ "pointer": [10, 10] }),
        json!({ "pointer": [600, 340] }),
        json!({ "pointer": [600, 340], "click": true }),
        json!({ "pointer": [610, 345] }),
        json!({}),
    ] {
        call(
            &mut ws,
            "sim_step",
            json!({ "session": s, "frames": 3, "input": input }),
        );
    }
    assert_eq!(
        components(&mut ws, &s, "start")["UiInteraction"]["clicks"],
        1
    );
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "click" }),
    );
    let ran = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "click" }),
    );
    // Every tick's world hash matched the recording's.
    assert_eq!(ran["passed"], true, "{ran}");
}

#[test]
fn pointer_input_mistakes_are_explained() {
    let (_dir, mut ws) = project("mistakes");
    let s = start(&mut ws);
    for (input, path) in [
        (json!({ "pointer": [1, 2, 3] }), "/input/pointer"),
        (json!({ "pointer": "middle" }), "/input/pointer"),
        (json!({ "click": 1 }), "/input/click"),
        (json!({ "click": true }), "/input/click"),
    ] {
        let answer = ws.call(
            "sim_step",
            &json!({ "session": s, "frames": 1, "input": input }),
            &parse_actor("agent"),
        );
        assert_eq!(answer["ok"], false, "{input}: {answer}");
        assert_eq!(answer["error"]["location"]["path"], path, "{answer}");
        assert!(!answer["error"]["hint"].as_str().unwrap().is_empty());
    }
    // Once the pointer is somewhere, a click alone clicks there.
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340] } }),
    );
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "click": true } }),
    );
    assert_eq!(
        components(&mut ws, &s, "start")["UiInteraction"]["pressed"],
        true
    );
}

#[test]
fn fonts_are_checked_and_interface_mistakes_are_named() {
    let (dir, mut ws) = project("fonts");
    std::fs::create_dir_all(dir.join("assets/fonts")).unwrap();
    std::fs::write(dir.join("assets/fonts/bad.ttf"), b"this is not a font").unwrap();
    std::fs::write(dir.join("assets/fonts/copy.ttf"), wish_text::DEFAULT_FONT).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    let codes: Vec<&str> = checked["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&"font_invalid"), "{checked}");
    assert_eq!(checked["passed"], false);
    std::fs::remove_file(dir.join("assets/fonts/bad.ttf")).unwrap();
    assert_eq!(call(&mut ws, "check", json!({}))["passed"], true);

    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "lost_text", "components": { "UiText": { "text": "nowhere" } } },
            { "op": "set", "id": "start_label", "component": "UiText",
              "value": { "font": "assets/fonts/copy.ttf" } },
            { "op": "spawn", "id": "odd_font", "components": {
                "UiNode": {}, "UiText": { "text": "x", "font": "assets/fonts/none.ttf" } } }
        ] }),
    );
    let s = start(&mut ws);
    let stepped = call(&mut ws, "sim_step", json!({ "session": s, "frames": 1 }));
    let found: Vec<(String, String)> = stepped["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap().to_owned(),
                d["message"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        found
            .iter()
            .any(|(code, message)| code == "ui_without_node" && message.contains("lost_text")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(code, message)| code == "ui_font_missing" && message.contains("none.ttf")),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(_, message)| message.contains("start_label")),
        "a font the project has is fine: {found:?}"
    );
}

#[test]
fn pointer_and_click_are_not_action_names() {
    let (dir, mut ws) = project("reserved");
    std::fs::write(
        dir.join("input.json"),
        r#"{ "format": 1, "actions": { "click": { "kind": "button", "keys": ["MouseLeft"] } } }"#,
    )
    .unwrap();
    let checked = call(&mut ws, "check", json!({}));
    let problem = &checked["diagnostics"][0];
    assert_eq!(problem["code"], "invalid_input_file", "{checked}");
    assert!(problem["message"].as_str().unwrap().contains("pointer"));
}

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-ui");
        // The folder is the test's own (a cache may have left anything in it).
        let _ = std::fs::remove_dir_all(&sdk);
        pack_sdk(&cargo, &manifest, &tmp.join("sdk-target"), &sdk).unwrap();
        let out = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap();
        let sysroot = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
        let rustc = sysroot
            .join("bin")
            .join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        Toolchain { sdk, rustc }
    })
}

/// A clicker: every click of the button adds a point, and the label says
/// how many, written by game code as text.
const CLICKER: &str = r#"
use wish_sdk::prelude::*;

/// Points so far.
#[resource]
pub struct Score {
    /// Points.
    pub points: u32,
}

/// The text that shows the score.
#[component]
pub struct ScoreLabel {
    /// The score the text shows now.
    pub shown: u32,
}

#[system]
fn count(clicks: EventReader<ButtonClicked>, mut score: ResMut<Score>) {
    for _ in clicks.read() {
        score.points += 1;
    }
}

#[system]
fn show(score: Res<Score>, mut labels: Query<(&mut ScoreLabel, &mut UiText)>) {
    for (label, words) in labels.iter_mut() {
        if label.shown != score.points || words.text == Text::EMPTY {
            words.text = text(&format!("Score: {}", score.points));
            label.shown = score.points;
        }
        // Reading a text back gives the same string.
        assert_eq!(text_of(words.text), format!("Score: {}", score.points));
    }
}

wish_sdk::export! {
    components: [ScoreLabel],
    resources: [Score],
    systems: [count, show],
}
"#;

#[test]
fn game_code_counts_clicks_and_writes_the_label() {
    let (dir, mut ws) = project("clicker");
    ws.set_toolchain(toolchain().clone());
    std::fs::write(dir.join(SCRIPTS_ENTRY), CLICKER).unwrap();
    let built = call(&mut ws, "script_build", json!({}));
    assert_eq!(built["passed"], true, "{built}");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "score", "components": {
            "UiNode": { "position": "Absolute", "offset": [20, 20] },
            "UiText": { "size": 40 },
            "ScoreLabel": {} } }] }),
    );
    let s = start(&mut ws);
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 2 }));
    let label = |ws: &mut Workspace| components(ws, &s, "score")["UiText"]["text"].clone();
    assert_eq!(label(&mut ws), "Score: 0");
    for _ in 0..3 {
        call(
            &mut ws,
            "sim_step",
            json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340], "click": true } }),
        );
        call(
            &mut ws,
            "sim_step",
            json!({ "session": s, "frames": 2, "input": { "pointer": [600, 340] } }),
        );
    }
    assert_eq!(label(&mut ws), "Score: 3");
    // The label grew with its text, and the picture says so.
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "session": s, "width": 640, "height": 360 }),
    );
    let shown = seen["interface"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["entity"] == "score")
        .unwrap()
        .clone();
    assert_eq!(shown["text"], "Score: 3", "{seen}");
    // Snapshots and replays keep the texts game code made.
    call(
        &mut ws,
        "sim_snapshot",
        json!({ "session": s, "label": "three" }),
    );
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 1, "input": { "pointer": [600, 340], "click": true } }),
    );
    call(
        &mut ws,
        "sim_step",
        json!({ "session": s, "frames": 2, "input": { "pointer": [600, 340] } }),
    );
    assert_eq!(label(&mut ws), "Score: 4");
    call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "three" }),
    );
    assert_eq!(label(&mut ws), "Score: 3");
}
