//! Regression tests of the M6 closing review that go through the tools.
//! (Others sit with their crates: wish_physics/tests/world.rs,
//! wish_particles/tests/particles.rs, wish_anim/tests/anim.rs,
//! wish_import/tests/animation.rs, wish_script/tests/hostile_game.rs,
//! wish_control/tests/physics_tools.rs.)

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn open(dir: &std::path::Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

/// Finding 12: a replay notices that a file the simulation reads (a clip's
/// length decides when its sound ends) changed since it was recorded.
#[test]
fn a_replay_says_when_the_files_the_game_reads_changed() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("review-m6-assets");
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Review", "wen").unwrap();
    std::fs::create_dir_all(dir.join("assets/sounds")).unwrap();
    let clip = dir.join("assets/sounds/beep.wav");
    std::fs::write(&clip, wish_audio::sine_wav(440.0, 0.5, 22_050)).unwrap();
    let mut ws = open(&dir);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "beeper", "components": {
            "AudioSource": { "clip": "assets/sounds/beep.wav", "playing": true } } }] }),
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
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 60 }));
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "beep" }),
    );
    let same = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "beep" }),
    );
    assert_eq!(same["passed"], true, "{same}");
    assert!(
        !same.to_string().contains("replay_assets_changed"),
        "{same}"
    );

    // A longer clip: the sound now ends later.
    std::fs::write(&clip, wish_audio::sine_wav(440.0, 2.0, 22_050)).unwrap();
    let changed = ws.call(
        "replay",
        &json!({ "action": "run", "name": "beep" }),
        &parse_actor("agent"),
    );
    assert!(
        changed.to_string().contains("replay_assets_changed"),
        "{changed}"
    );
}
