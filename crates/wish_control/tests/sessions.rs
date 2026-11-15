//! Play sessions: stepping, determinism, snapshots and forks, `until`, the
//! inspector and the log, on the built-in `bounce` and `stress` examples.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str, example: &str, scene: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wish-sessions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("wish.json"),
        format!(
            "{{\n  \"engine\": \"0.1.0\",\n  \"format\": 1,\n  \"name\": \"S\",\n  \"scene\": \"scenes/main.scene.json\",\n  \"types\": {{\n    \"example\": \"{example}\"\n  }}\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("scenes/main.scene.json"), scene).unwrap();
    dir
}

fn open(dir: &Path) -> Workspace {
    Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap()
}

fn ok(answer: &Value) -> &Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

/// Bounce: bodies spawn at random, fall and bounce; `Stats` counts.
const BOUNCE: &str = r#"{ "format": 1, "entities": {
    "cam": { "components": { "Camera": {}, "Transform": { "translation": [0, 5, 20] } } }
} }"#;

const STRESS: &str = r#"{ "format": 1, "entities": {
    "mover": { "components": { "Transform": { "translation": [0, 1, 0] }, "Position": { "value": [0, 1, 0] }, "Velocity": { "value": [0, -1, 0] } } },
    "fast": { "components": { "Position": { "value": [0, 0, 0] }, "Velocity": { "value": [30, 0, 0] } } }
} }"#;

#[test]
fn the_same_seed_gives_the_same_run_and_another_seed_a_different_one() {
    let dir = project("determinism", "bounce", BOUNCE);
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    let run = |ws: &mut Workspace, seed: i64| -> String {
        let started = ok(&ws.call("sim_start", &json!({ "seed": seed }), &me)).clone();
        let id = started["session"].as_str().unwrap().to_owned();
        let stepped = ws.call("sim_step", &json!({ "session": id, "frames": 240 }), &me);
        ok(&stepped);
        assert_eq!(stepped["tick"], 240);
        assert_eq!(stepped["stopped_by"], "frames");
        let hash = stepped["world_hash"].as_str().unwrap().to_owned();
        ok(&ws.call("sim_stop", &json!({ "session": id }), &me));
        hash
    };
    let a = run(&mut ws, 7);
    let b = run(&mut ws, 7);
    let c = run(&mut ws, 8);
    assert_eq!(a, b, "deterministic");
    assert_ne!(a, c, "the seed matters");
    assert_eq!(a.len(), 32);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn events_are_counted_and_the_world_can_be_looked_at_while_it_runs() {
    let dir = project("events", "bounce", BOUNCE);
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    let started = ok(&ws.call("sim_start", &json!({ "seed": 3 }), &me)).clone();
    assert_eq!(started["tick"], 0);
    assert!(
        started["systems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "transform_propagation")
    );
    // Only one session: the id may be left out.
    let stepped = ok(&ws.call("sim_step", &json!({ "frames": 300 }), &me)).clone();
    assert!(
        stepped["events"]["Bounced"].as_u64().unwrap() > 0,
        "{stepped}"
    );
    let bodies = ok(&ws.call(
        "entity_query",
        &json!({ "session": "s1", "with": ["Body"], "limit": 3 }),
        &me,
    ))
    .clone();
    assert!(bodies["matched"].as_u64().unwrap() > 0, "{bodies}");
    assert_eq!(bodies["tick"], 300);
    let first = bodies["entities"][0]["id"].as_str().unwrap();
    assert!(first.starts_with('#'), "made while running: {first}");
    // The scene's own entity keeps its name; resources can be used in conditions.
    let cam = ok(&ws.call(
        "entity_get",
        &json!({ "session": "s1", "ids": ["cam", first] }),
        &me,
    ))
    .clone();
    assert_eq!(cam["entities"][0]["id"], "cam");
    assert!(cam["entities"][1]["components"]["Body"].is_object());
    let counted = ok(&ws.call(
        "entity_query",
        &json!({ "session": "s1", "where": "Stats.bounces > 0 && id == \"cam\"" }),
        &me,
    ))
    .clone();
    assert_eq!(counted["entities"][0]["id"], "cam");
    // The file was never touched, and the scene still has one entity.
    let scene = ok(&ws.call("scene_get", &json!({}), &me)).clone();
    assert_eq!(scene["entities"], 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn until_stops_right_after_the_tick_it_becomes_true() {
    let dir = project("until", "stress", STRESS);
    let mut ws = open(&dir);
    let me = parse_actor("human");
    ok(&ws.call("sim_start", &json!({}), &me));
    // `fast` runs +30 m/s along x: x > 10 after 20 ticks (30 / 60 per tick = 0.5 m).
    let stepped = ok(&ws.call(
        "sim_step",
        &json!({ "until": "Position.value.x > 10" }),
        &me,
    ))
    .clone();
    assert_eq!(stepped["stopped_by"], "until");
    assert_eq!(stepped["matched"], json!(["fast"]));
    assert_eq!(stepped["ticks_run"], 21);
    // Never true: stops at max_frames with a warning that says what to do.
    let never = ok(&ws.call(
        "sim_step",
        &json!({ "until": "Position.value.y > 1000", "max_frames": 30 }),
        &me,
    ))
    .clone();
    assert_eq!(never["stopped_by"], "max_frames");
    assert_eq!(never["ticks_run"], 30);
    assert_eq!(never["diagnostics"][0]["code"], "until_not_reached");
    // A mistake in the condition is explained before anything runs.
    let bad = ws.call("sim_step", &json!({ "until": "Position.vlaue.x > 1" }), &me);
    assert_eq!(bad["error"]["code"], "expr_unknown_name");
    assert_eq!(bad["error"]["location"]["path"], "/until");
    let tick = ok(&ws.call("sim_step", &json!({ "frames": 1 }), &me))["tick"].clone();
    assert_eq!(tick, 52, "the bad call ran nothing");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn snapshots_restore_exactly_and_forks_run_side_by_side() {
    let dir = project("fork", "bounce", BOUNCE);
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    ok(&ws.call("sim_start", &json!({ "seed": 11 }), &me));
    ok(&ws.call("sim_step", &json!({ "frames": 100 }), &me));
    let saved = ok(&ws.call("sim_snapshot", &json!({ "label": "mid" }), &me)).clone();
    assert_eq!(saved["tick"], 100);
    let continued = ok(&ws.call("sim_step", &json!({ "frames": 100 }), &me))["world_hash"].clone();
    // Restore and run again: exactly the same.
    let restored = ok(&ws.call("sim_restore", &json!({ "snapshot": "mid" }), &me)).clone();
    assert_eq!(restored["tick"], 100);
    assert_eq!(restored["world_hash"], saved["world_hash"]);
    assert_eq!(
        ok(&ws.call("sim_step", &json!({ "frames": 100 }), &me))["world_hash"],
        continued
    );
    // Fork: a second session from the snapshot; the first is left alone.
    let fork = ok(&ws.call(
        "sim_restore",
        &json!({ "snapshot": "mid", "fork": true }),
        &me,
    ))
    .clone();
    let forked = fork["session"].as_str().unwrap().to_owned();
    assert_eq!(fork["forked_from"], "s1");
    assert_ne!(forked, "s1");
    assert_eq!(fork["world_hash"], saved["world_hash"]);
    // Two sessions now: naming one is required, and the error lists them.
    let ambiguous = ws.call("sim_step", &json!({ "frames": 1 }), &me);
    assert_eq!(ambiguous["error"]["code"], "session_required");
    assert_eq!(ambiguous["error"]["candidates"], json!(["s1", "s2"]));
    let from_fork = ok(&ws.call(
        "sim_step",
        &json!({ "session": forked, "frames": 100 }),
        &me,
    ))["world_hash"]
        .clone();
    assert_eq!(
        from_fork, continued,
        "the fork continues exactly like the original did"
    );
    // Listing, duplicates, mistakes.
    let list = ok(&ws.call(
        "sim_snapshot",
        &json!({ "session": "s1", "action": "list" }),
        &me,
    ))
    .clone();
    assert_eq!(list["snapshots"][0]["label"], "mid");
    let auto = ok(&ws.call("sim_snapshot", &json!({ "session": "s1" }), &me)).clone();
    assert_eq!(auto["label"], "snap1");
    let again = ws.call(
        "sim_snapshot",
        &json!({ "session": "s1", "label": "mid" }),
        &me,
    );
    assert_eq!(again["error"]["code"], "snapshot_exists");
    let missing = ws.call(
        "sim_restore",
        &json!({ "session": "s1", "snapshot": "mdi" }),
        &me,
    );
    assert_eq!(missing["error"]["hint"], "Did you mean 'mid'?");
    let wrong = ws.call("sim_step", &json!({ "session": "s3" }), &me);
    assert_eq!(wrong["error"]["code"], "unknown_session");
    assert_eq!(wrong["error"]["candidates"], json!(["s1", "s2"]));
    let stopped = ok(&ws.call("sim_stop", &json!({ "session": "s2" }), &me)).clone();
    assert_eq!(stopped["open_sessions"], json!(["s1"]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_inspector_and_the_log_say_what_looks_wrong() {
    let dir = project("inspect", "stress", STRESS);
    let mut ws = open(&dir);
    let me = parse_actor("claude-code");
    ok(&ws.call("sim_start", &json!({}), &me));
    // `mover` has a Transform but nothing moves it; give the scene one that
    // starts far below the world, and one that is not finite is refused at
    // load (so only the first can be tested here).
    ok(&ws.call(
        "edit_apply",
        &json!({ "ops": [{ "op": "spawn", "id": "faller", "components": { "Transform": { "translation": [0, -5000, 0] } } }] }),
        &me,
    ));
    let started = ok(&ws.call("sim_start", &json!({}), &me)).clone();
    let id = started["session"].as_str().unwrap().to_owned();
    let stepped = ok(&ws.call("sim_step", &json!({ "session": id, "frames": 2 }), &me)).clone();
    let codes: Vec<&str> = stepped["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["entity_out_of_bounds"], "{stepped}");
    assert!(
        stepped["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("faller")
    );
    // Reported once, not at every step.
    let again = ok(&ws.call("sim_step", &json!({ "session": id, "frames": 2 }), &me)).clone();
    assert!(again.get("diagnostics").is_none(), "{again}");
    // The log kept it, with the session and tick.
    let log = ok(&ws.call("logs_get", &json!({ "severity": "warning" }), &me)).clone();
    assert_eq!(log["entries"].as_array().unwrap().len(), 1);
    assert_eq!(log["entries"][0]["code"], "entity_out_of_bounds");
    assert_eq!(log["entries"][0]["kind"], "inspector");
    assert_eq!(log["entries"][0]["session"], id.as_str());
    let everything = ok(&ws.call("logs_get", &json!({ "kinds": ["session"] }), &me)).clone();
    assert!(everything["entries"].as_array().unwrap().len() >= 2);
    let last = everything["last_seq"].as_u64().unwrap();
    let none_new = ok(&ws.call("logs_get", &json!({ "since": last }), &me)).clone();
    assert_eq!(none_new["entries"], json!([]));
    let bad = ws.call("logs_get", &json!({ "kinds": ["sistem"] }), &me);
    assert_eq!(bad["error"]["hint"], "Did you mean 'system'?");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sessions_have_limits_and_need_a_scene() {
    let dir = project("limits", "stress", STRESS);
    let mut ws = open(&dir);
    let me = parse_actor("human");
    let none = ws.call("sim_step", &json!({}), &me);
    assert_eq!(none["error"]["code"], "no_session");
    let missing = ws.call(
        "sim_start",
        &json!({ "scene": "scenes/other.scene.json" }),
        &me,
    );
    assert_eq!(missing["error"]["code"], "scene_not_found");
    for _ in 0..8 {
        ok(&ws.call("sim_start", &json!({}), &me));
    }
    let ninth = ws.call("sim_start", &json!({}), &me);
    assert_eq!(ninth["error"]["code"], "too_many_sessions");
    assert!(
        ninth["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("sim_stop")
    );
    let too_many = ws.call(
        "sim_step",
        &json!({ "session": "s1", "frames": 5_000_000 }),
        &me,
    );
    assert_eq!(too_many["error"]["code"], "invalid_parameter");
    std::fs::remove_dir_all(&dir).unwrap();
}
