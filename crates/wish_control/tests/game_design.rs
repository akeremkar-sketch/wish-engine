//! `game_design` (M9c): the game card, decisions, plan, play-test notes and
//! questions kept in `design/game.json`.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{DESIGN_FILE, Workspace, WorkspaceOptions, design_summary, parse_actor};

fn open(name: &str) -> (Workspace, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("game-design-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Design", "wen").unwrap();
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    (ws, dir)
}

fn call(ws: &mut Workspace, actor: &str, args: Value) -> Value {
    ws.call("game_design", &args, &parse_actor(actor))
}

#[test]
fn an_empty_project_has_an_empty_design_and_no_file() {
    let (mut ws, dir) = open("empty");
    let got = call(&mut ws, "human", json!({}));
    assert_eq!(got["ok"], true, "{got}");
    assert_eq!(got["design"]["brief"]["pitch"], "");
    assert_eq!(got["design"]["brief"]["must"], json!([]));
    assert_eq!(got["design"]["next_id"], 1);
    assert!(!dir.join(DESIGN_FILE).exists());
    assert_eq!(design_summary(&got["design"]), None);
}

#[test]
fn the_card_decisions_plan_notes_and_questions_are_kept() {
    let (mut ws, dir) = open("kept");
    let set = call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": {
            "title": "Hop", "pitch": "Hop between floating islands",
            "must": ["double jump", " ", "three islands"] } }),
    );
    assert_eq!(set["ok"], true, "{set}");
    assert_eq!(
        set["design"]["brief"]["must"],
        json!(["double jump", "three islands"])
    );
    // Only the given fields change.
    call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": { "genre": "platformer" } }),
    );
    let decided = call(
        &mut ws,
        "agent:claude-code",
        json!({ "action": "decide", "text": "Jump height 2.5 m" }),
    );
    assert_eq!(decided["id"], 1, "{decided}");
    let step = call(
        &mut ws,
        "agent:claude-code",
        json!({ "action": "plan_add", "text": "Add the second island" }),
    );
    assert_eq!(step["id"], 2);
    let marked = call(
        &mut ws,
        "human",
        json!({ "action": "plan_set", "id": 2, "status": "doing" }),
    );
    assert_eq!(marked["design"]["plan"][0]["status"], "doing", "{marked}");
    let note = call(
        &mut ws,
        "human",
        json!({ "action": "note", "text": "The gap is too wide", "scene": "scenes/main.scene.json", "tick": 120 }),
    );
    assert_eq!(note["id"], 3, "{note}");
    let asked = call(
        &mut ws,
        "agent:claude-code",
        json!({ "action": "ask", "text": "How many enemies?", "options": ["3", "5", "10"] }),
    );
    assert_eq!(asked["id"], 4, "{asked}");
    let answered = call(
        &mut ws,
        "human",
        json!({ "action": "answer", "id": 4, "answer": "5", "remember": true }),
    );
    assert_eq!(answered["decision"], 5, "{answered}");
    let design = &answered["design"];
    assert_eq!(design["brief"]["title"], "Hop");
    assert_eq!(design["brief"]["genre"], "platformer");
    assert_eq!(design["decisions"][0]["by"], "agent:claude-code");
    assert_eq!(design["decisions"][1]["text"], "How many enemies? → 5");
    assert_eq!(design["decisions"][1]["by"], "human");
    assert_eq!(design["questions"][0]["answer"], "5");
    assert_eq!(design["notes"][0]["tick"], 120);
    // The file is canonical and read back the same.
    let text = std::fs::read_to_string(dir.join(DESIGN_FILE)).unwrap();
    let reopened = call(&mut ws, "human", json!({ "action": "get" }));
    assert_eq!(&reopened["design"], design);
    let again = call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": {} }),
    );
    assert_eq!(again["ok"], true);
    assert_eq!(
        std::fs::read_to_string(dir.join(DESIGN_FILE)).unwrap(),
        text
    );
    // The summary an agent gets.
    let summary = design_summary(design).unwrap();
    assert!(
        summary.contains("- pitch: Hop between floating islands"),
        "{summary}"
    );
    assert!(
        summary.contains("- must: double jump; three islands"),
        "{summary}"
    );
    assert!(summary.contains("[1] Jump height 2.5 m"), "{summary}");
    assert!(summary.contains("[2] Add the second island"), "{summary}");
    assert!(summary.contains("The gap is too wide"), "{summary}");
    assert!(summary.contains("How many enemies? → 5"), "{summary}");
    // Done steps and notes leave the summary; a forgotten decision is gone.
    call(
        &mut ws,
        "human",
        json!({ "action": "plan_set", "id": 2, "status": "done" }),
    );
    call(&mut ws, "human", json!({ "action": "note_done", "id": 3 }));
    let forgot = call(&mut ws, "human", json!({ "action": "forget", "id": 1 }));
    let summary = design_summary(&forgot["design"]).unwrap();
    assert!(!summary.contains("second island"), "{summary}");
    assert!(!summary.contains("gap is too wide"), "{summary}");
    assert!(!summary.contains("Jump height"), "{summary}");
    let removed = call(
        &mut ws,
        "human",
        json!({ "action": "plan_remove", "id": 2 }),
    );
    assert_eq!(removed["design"]["plan"], json!([]));
    // Ids are never given twice.
    let next = call(
        &mut ws,
        "human",
        json!({ "action": "decide", "text": "Pastel colours" }),
    );
    assert_eq!(next["id"], 6);
}

#[test]
fn a_dry_run_changes_nothing() {
    let (mut ws, dir) = open("dry");
    let dry = call(
        &mut ws,
        "agent:a",
        json!({ "action": "decide", "text": "No violence", "dry_run": true }),
    );
    assert_eq!(dry["ok"], true, "{dry}");
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["would_be"]["decisions"][0]["text"], "No violence");
    assert!(!dir.join(DESIGN_FILE).exists());
}

#[test]
fn bad_calls_say_what_is_wrong() {
    let (mut ws, dir) = open("bad");
    let unknown = call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": { "pich": "x" } }),
    );
    assert_eq!(unknown["error"]["code"], "invalid_design", "{unknown}");
    assert!(
        unknown["error"]["hint"].as_str().unwrap().contains("pitch"),
        "{unknown}"
    );
    let list = call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": { "must": "double jump" } }),
    );
    assert_eq!(list["error"]["code"], "invalid_design", "{list}");
    let one = call(
        &mut ws,
        "agent:a",
        json!({ "action": "ask", "text": "Colour?", "options": ["red"] }),
    );
    assert_eq!(one["error"]["code"], "invalid_design", "{one}");
    let missing = call(&mut ws, "human", json!({ "action": "decide" }));
    assert_eq!(missing["error"]["code"], "missing_parameter", "{missing}");
    let nobody = call(&mut ws, "human", json!({ "action": "forget", "id": 9 }));
    assert_eq!(nobody["error"]["code"], "design_item_not_found", "{nobody}");
    let long = call(
        &mut ws,
        "human",
        json!({ "action": "decide", "text": "x".repeat(3000) }),
    );
    assert_eq!(long["error"]["code"], "invalid_design", "{long}");
    let picture = call(
        &mut ws,
        "human",
        json!({ "action": "note", "text": "look", "picture": "../outside.png" }),
    );
    assert_eq!(picture["error"]["code"], "invalid_output_path", "{picture}");
    let absent = call(
        &mut ws,
        "human",
        json!({ "action": "note", "text": "look", "picture": "design/notes/none.png" }),
    );
    assert_eq!(absent["error"]["code"], "file_not_found", "{absent}");
    assert!(!dir.join(DESIGN_FILE).exists());
}

#[test]
fn a_broken_design_file_is_never_overwritten() {
    let (mut ws, dir) = open("broken");
    std::fs::create_dir_all(dir.join("design")).unwrap();
    std::fs::write(dir.join(DESIGN_FILE), "{ \"format\": 1, oops").unwrap();
    let got = call(&mut ws, "human", json!({}));
    assert_eq!(got["error"]["code"], "design_file_invalid", "{got}");
    let write = call(&mut ws, "human", json!({ "action": "decide", "text": "x" }));
    assert_eq!(write["error"]["code"], "design_file_invalid", "{write}");
    assert_eq!(
        std::fs::read_to_string(dir.join(DESIGN_FILE)).unwrap(),
        "{ \"format\": 1, oops"
    );
}

#[test]
fn design_changes_reach_the_activity_feed_and_looking_does_not() {
    let (mut ws, _dir) = open("activity");
    call(&mut ws, "human", json!({ "action": "get" }));
    call(&mut ws, "human", json!({}));
    call(
        &mut ws,
        "human",
        json!({ "action": "decide", "text": "Short levels" }),
    );
    let feed = ws.call("activity_get", &json!({}), &parse_actor("human"));
    let tools: Vec<&str> = feed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["tool"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["game_design"], "{feed}");
}

#[test]
fn play_test_pictures_are_not_assets() {
    let (mut ws, dir) = open("pictures");
    std::fs::create_dir_all(dir.join("design/notes")).unwrap();
    std::fs::write(dir.join("design/notes/1.png"), b"not really a png").unwrap();
    let note = call(
        &mut ws,
        "human",
        json!({ "action": "note", "text": "here", "picture": "design/notes/1.png" }),
    );
    assert_eq!(
        note["design"]["notes"][0]["picture"], "design/notes/1.png",
        "{note}"
    );
    let assets = ws.call("asset_list", &json!({}), &parse_actor("human"));
    assert!(!assets.to_string().contains("design/"), "{assets}");
    let check = ws.call("check", &json!({}), &parse_actor("human"));
    assert_eq!(check["ok"], true, "{check}");
}

#[test]
fn a_workspace_brings_its_own_card_and_keeps_every_value() {
    let (mut ws, _dir) = open("workspace");
    call(
        &mut ws,
        "human",
        json!({ "action": "set_brief", "fields": { "pitch": "A ghost story", "genre": "adventure" } }),
    );
    let card = json!([
        { "key": "pitch", "label": "In one sentence" },
        { "key": "setting", "label": "Setting" },
        { "key": "characters", "label": "Characters", "list": true }
    ]);
    let set = call(
        &mut ws,
        "human",
        json!({ "action": "set_workspace", "workspace": "wish.workspace.story/story.json", "card": card }),
    );
    assert_eq!(set["ok"], true, "{set}");
    assert_eq!(
        set["design"]["workspace"],
        "wish.workspace.story/story.json"
    );
    assert_eq!(set["design"]["brief"]["characters"], json!([]));
    // The new fields are set; the old ones are no longer on the card.
    let filled = call(
        &mut ws,
        "agent:a",
        json!({ "action": "set_brief", "fields": { "characters": ["Mira", "the keeper"], "setting": "a lighthouse" } }),
    );
    assert_eq!(filled["ok"], true, "{filled}");
    let gone = call(
        &mut ws,
        "agent:a",
        json!({ "action": "set_brief", "fields": { "genre": "x" } }),
    );
    assert_eq!(gone["error"]["code"], "invalid_design", "{gone}");
    let summary = design_summary(&filled["design"]).unwrap();
    assert!(
        summary.contains("- characters: Mira; the keeper"),
        "{summary}"
    );
    assert!(summary.contains("- setting: a lighthouse"), "{summary}");
    assert!(
        !summary.contains("adventure"),
        "only the card's fields: {summary}"
    );
    // Back to the default card: every value is still there.
    let back = call(&mut ws, "human", json!({ "action": "set_workspace" }));
    assert_eq!(back["ok"], true, "{back}");
    assert!(back["design"].get("card").is_none() && back["design"].get("workspace").is_none());
    assert_eq!(back["design"]["brief"]["genre"], "adventure");
    assert_eq!(
        back["design"]["brief"]["characters"],
        json!(["Mira", "the keeper"])
    );
    // A bad card is explained.
    for (card, why) in [
        (json!([]), "empty"),
        (json!([{ "key": "Bad Key", "label": "X" }]), "key"),
        (
            json!([{ "key": "a", "label": "A" }, { "key": "a", "label": "B" }]),
            "twice",
        ),
        (json!([{ "key": "a", "label": "" }]), "label"),
    ] {
        let got = call(
            &mut ws,
            "human",
            json!({ "action": "set_workspace", "workspace": "x/y.json", "card": card }),
        );
        assert_eq!(got["error"]["code"], "invalid_design", "{why}: {got}");
    }
    let no_card = call(
        &mut ws,
        "human",
        json!({ "action": "set_workspace", "workspace": "x/y.json" }),
    );
    assert_eq!(no_card["error"]["code"], "missing_parameter", "{no_card}");
}
