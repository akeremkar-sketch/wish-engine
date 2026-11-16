//! Sound through the tools (M6 WP4): a project with a clip and an
//! `AudioSource` in its scene; `logs_get kinds: ["audio"]` says what played.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("audio-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Audio", "wen").unwrap();
    std::fs::create_dir_all(dir.join("assets/sounds")).unwrap();
    std::fs::write(
        dir.join("assets/sounds/jump.wav"),
        wish_audio::sine_wav(440.0, 0.5, 8000),
    )
    .unwrap();
    let ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

#[test]
fn an_agent_hears_a_scene_through_the_log() {
    let (_dir, mut ws) = project("log");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "ears", "components": {
                "Transform": {}, "AudioListener": {} } },
            { "op": "spawn", "id": "door", "components": {
                "Transform": { "translation": [4, 0, 0] },
                "AudioSource": { "clip": "assets/sounds/jump.wav", "playing": true,
                                 "spatial": true } } },
            { "op": "spawn", "id": "ghost", "components": {
                "Transform": {},
                "AudioSource": { "clip": "assets/sounds/missing.wav", "playing": true } } }
        ] }),
    );
    // The scene is checked: the missing file is a reference problem.
    let check = call(&mut ws, "check", json!({}));
    let text = check.to_string();
    assert!(text.contains("missing.wav"), "{check}");
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 60 }));
    let log = call(&mut ws, "logs_get", json!({ "kinds": ["audio"] }));
    let messages: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("jump.wav")
            && m.contains("started on 'door'")
            && m.contains("-12.0 dB")
            && m.contains("right 100%")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("jump.wav") && m.contains("ended on 'door'")),
        "{messages:?}"
    );
    let codes: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"audio_clip_unknown"), "{codes:?}");
    // The sound's state is on the entity.
    let door = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["door"] }),
    );
    assert_eq!(
        door["entities"][0]["components"]["AudioSource"]["playing"],
        false
    );
    // Playing it again is one command: set playing.
    call(
        &mut ws,
        "entity_query",
        json!({ "session": s, "where": "has(AudioPlayback)" }),
    );
}

#[test]
fn replays_and_snapshots_of_a_sound_scene_are_bit_for_bit() {
    let (_dir, mut ws) = project("replay");
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "spawn", "id": "ears", "components": { "Transform": {}, "AudioListener": {} } },
            { "op": "spawn", "id": "loop", "components": {
                "Transform": { "translation": [0, 0, -3] },
                "AudioSource": { "clip": "assets/sounds/jump.wav", "playing": true,
                                 "looping": true, "spatial": true } } }
        ] }),
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 40 }));
    call(
        &mut ws,
        "sim_snapshot",
        json!({ "session": s, "label": "a" }),
    );
    let straight =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 100 }))["world_hash"].clone();
    call(
        &mut ws,
        "sim_restore",
        json!({ "session": s, "snapshot": "a" }),
    );
    let again =
        call(&mut ws, "sim_step", json!({ "session": s, "frames": 100 }))["world_hash"].clone();
    assert_eq!(again, straight);
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "loop" }),
    );
    let ran = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "loop" }),
    );
    assert_eq!(ran["passed"], true, "{ran}");
}
