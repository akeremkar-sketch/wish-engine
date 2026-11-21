//! Players (release 1.1, package 2; design note `docs/design/1.1-players.md`):
//! a session with several players, each with their own input through
//! `sim_step` and scenario tests, game code reading another player's
//! actions, two-player replays in format 2, and single-player replays that
//! stay format 1 and still play, old ones included.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_script::build::{SCRIPTS_ENTRY, Toolchain, pack_sdk};

fn toolchain() -> &'static Toolchain {
    static TOOLCHAIN: OnceLock<Toolchain> = OnceLock::new();
    TOOLCHAIN.get_or_init(|| {
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
        let sdk = tmp.join("sdk-pack-players");
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

/// Each avatar walks with its own player's `move`; `Seen` says how many
/// players the game code saw.
const GAME: &str = r#"use wish_sdk::prelude::*;

/// Whose avatar this is.
#[component]
pub struct Avatar {
    /// The player who walks it.
    #[schema(range = 0..=15)]
    pub player: PlayerId,
}

/// What the game code saw on its last tick.
#[resource]
pub struct Seen {
    /// How many players PlayerActions had.
    pub players: u32,
}

/// Walks every avatar with its player's input, 2 m/s.
#[system]
fn walk(players: PlayerActions, mut avatars: Query<(&Avatar, &mut Transform)>, mut seen: ResMut<Seen>, time: Res<Time>) {
    seen.players = players.count();
    for (avatar, transform) in avatars.iter_mut() {
        let step = players.of(avatar.player).axis2("move");
        transform.translation += Vec3::new(step.x, 0.0, -step.y) * 2.0 * time.delta;
    }
}

wish_sdk::export! {
    components: [Avatar],
    resources: [Seen],
    systems: [walk],
}
"#;

/// A project with the game above, built, and two avatars: `red` (player 0)
/// at x = -2 and `blue` (player 1) at x = 2.
fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("players-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Players", "wen").unwrap();
    std::fs::write(dir.join(SCRIPTS_ENTRY), GAME).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_toolchain(toolchain().clone());
    let agent = parse_actor("agent");
    let built = ws.call("script_build", &json!({}), &agent);
    assert_eq!(built["passed"], true, "{built}");
    let edited = ws.call(
        "edit_apply",
        &json!({ "ops": [
            { "op": "spawn", "id": "red", "components": {
                "Transform": { "translation": [-2, 0, 0] }, "Avatar": { "player": 0 } } },
            { "op": "spawn", "id": "blue", "components": {
                "Transform": { "translation": [2, 0, 0] }, "Avatar": { "player": 1 } } }
        ] }),
        &agent,
    );
    assert_eq!(edited["ok"], true, "{edited}");
    (dir, ws)
}

fn at(ws: &mut Workspace, session: &str, id: &str) -> [f64; 3] {
    let got = ws.call(
        "entity_get",
        &json!({ "session": session, "ids": [id] }),
        &parse_actor("agent"),
    );
    let t = &got["entities"][0]["components"]["Transform"]["translation"];
    [0, 1, 2].map(|i| t[i].as_f64().unwrap())
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

fn resources(ws: &mut Workspace, session: &str) -> Value {
    ws.call(
        "entity_get",
        &json!({ "session": session, "resources": ["Seen", "Players"] }),
        &parse_actor("agent"),
    )
}

fn codes(answer: &Value) -> Vec<String> {
    answer["diagnostics"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|d| d["code"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn two_players_move_on_their_own_and_game_code_reads_each() {
    let (_dir, mut ws) = project("sessions");
    let agent = parse_actor("agent");

    let started = ws.call("sim_start", &json!({ "players": 2 }), &agent);
    assert_eq!(started["players"], 2, "{started}");
    let stepped = ws.call(
        "sim_step",
        &json!({ "session": "s1", "frames": 30,
                 "input": { "move": [0, 1] }, "players": { "1": { "move": [1, 0] } } }),
        &agent,
    );
    assert_eq!(stepped["ok"], true, "{stepped}");
    // Half a second at 2 m/s: player 0 walked forward, player 1 right.
    assert!(near(at(&mut ws, "s1", "red"), [-2.0, 0.0, -1.0]));
    assert!(near(at(&mut ws, "s1", "blue"), [3.0, 0.0, 0.0]));
    let seen = resources(&mut ws, "s1");
    assert_eq!(seen["resources"]["Seen"]["players"], 2, "{seen}");
    assert_eq!(seen["resources"]["Players"]["count"], 2, "{seen}");

    // Player 0 may also be given by id; players not named are at rest.
    ws.call(
        "sim_step",
        &json!({ "session": "s1", "frames": 30, "players": { "0": { "move": [-1, 0] } } }),
        &agent,
    );
    assert!(near(at(&mut ws, "s1", "red"), [-3.0, 0.0, -1.0]));
    assert!(near(at(&mut ws, "s1", "blue"), [3.0, 0.0, 0.0]));

    // A fork keeps the players.
    ws.call(
        "sim_snapshot",
        &json!({ "session": "s1", "label": "a" }),
        &agent,
    );
    let forked = ws.call(
        "sim_restore",
        &json!({ "session": "s1", "snapshot": "a", "fork": true }),
        &agent,
    );
    let fork = forked["session"].as_str().unwrap().to_owned();
    ws.call(
        "sim_step",
        &json!({ "session": fork, "frames": 30, "players": { "1": { "move": [0, -1] } } }),
        &agent,
    );
    assert!(near(at(&mut ws, &fork, "blue"), [3.0, 0.0, 1.0]));

    // Mistakes are explained where they are.
    let unknown = ws.call(
        "sim_step",
        &json!({ "session": "s1", "players": { "2": { "move": [1, 0] } } }),
        &agent,
    );
    assert_eq!(unknown["error"]["code"], "unknown_player", "{unknown}");
    assert_eq!(unknown["error"]["location"]["path"], "/players/2");
    let twice = ws.call(
        "sim_step",
        &json!({ "session": "s1", "input": { "jump": true }, "players": { "0": { "jump": true } } }),
        &agent,
    );
    assert_eq!(twice["error"]["code"], "invalid_parameter", "{twice}");
    let pointer = ws.call(
        "sim_step",
        &json!({ "session": "s1", "players": { "1": { "pointer": [10, 10] } } }),
        &agent,
    );
    assert_eq!(pointer["error"]["code"], "invalid_parameter", "{pointer}");
    assert_eq!(pointer["error"]["location"]["path"], "/players/1/pointer");
    let action = ws.call(
        "sim_step",
        &json!({ "session": "s1", "players": { "1": { "jmup": true } } }),
        &agent,
    );
    assert_eq!(action["error"]["code"], "unknown_action", "{action}");
    assert_eq!(action["error"]["location"]["path"], "/players/1/jmup");

    // Alone: one player, the same game code, nothing new in the answer.
    let alone = ws.call("sim_start", &json!({}), &agent);
    let solo = alone["session"].as_str().unwrap().to_owned();
    assert!(alone.get("players").is_none(), "{alone}");
    ws.call(
        "sim_step",
        &json!({ "session": solo, "frames": 30, "input": { "move": [0, 1] } }),
        &agent,
    );
    assert!(near(at(&mut ws, &solo, "red"), [-2.0, 0.0, -1.0]));
    assert!(near(at(&mut ws, &solo, "blue"), [2.0, 0.0, 0.0]));
    let seen = resources(&mut ws, &solo);
    assert_eq!(seen["resources"]["Seen"]["players"], 1, "{seen}");
    let lonely = ws.call(
        "sim_step",
        &json!({ "session": solo, "players": { "1": { "move": [1, 0] } } }),
        &agent,
    );
    assert_eq!(lonely["error"]["code"], "unknown_player", "{lonely}");
    let too_many = ws.call("sim_start", &json!({ "players": 17 }), &agent);
    assert_eq!(too_many["error"]["code"], "invalid_parameter", "{too_many}");
}

#[test]
fn a_scenario_test_plays_two_players() {
    let (dir, mut ws) = project("scenario");
    let agent = parse_actor("agent");
    std::fs::write(
        dir.join("tests/two.test.json"),
        r#"{
  "format": 1,
  "scene": "scenes/main.scene.json",
  "seed": 1,
  "players": 2,
  "steps": [
    { "input": { "move": [0, 1] }, "players": { "1": { "move": [1, 0] } }, "frames": 30 },
    { "expect": "Players.count == 2" },
    { "expect": "Transform.translation.z < -0.99 && Transform.translation.x < -1.99", "entity": "red" },
    { "expect": "Transform.translation.x > 2.99 && Transform.translation.z > -0.01", "entity": "blue" },
    { "players": { "1": { "move": [-1, 0] } }, "until": "Transform.translation.x < 2.5", "max_frames": 60 }
  ]
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("tests/stranger.test.json"),
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "players": 2,
  "steps": [ { "players": { "3": { "move": [1, 0] } }, "frames": 1 } ] }
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("tests/crowd.test.json"),
        r#"{ "format": 1, "scene": "scenes/main.scene.json", "players": 40, "steps": [ { "frames": 1 } ] }
"#,
    )
    .unwrap();
    let ran = ws.call(
        "test_run",
        &json!({ "paths": ["tests/two.test.json", "tests/boots.test.json"] }),
        &agent,
    );
    assert_eq!(ran["passed"], true, "{ran}");
    let stranger = ws.call(
        "test_run",
        &json!({ "paths": ["tests/stranger.test.json"] }),
        &agent,
    );
    let failure = &stranger["results"][0]["failure"];
    assert_eq!(failure["kind"], "input", "{stranger}");
    assert_eq!(
        failure["diagnostic"]["code"], "unknown_player",
        "{stranger}"
    );
    let crowd = ws.call(
        "test_run",
        &json!({ "paths": ["tests/crowd.test.json"] }),
        &agent,
    );
    let failure = &crowd["results"][0]["failure"];
    assert_eq!(
        failure["diagnostic"]["code"], "invalid_test_file",
        "{crowd}"
    );
}

fn replay_text(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(format!("replays/{name}.replay.jsonl"))).unwrap()
}

fn header(text: &str) -> Value {
    serde_json::from_str(text.lines().next().unwrap()).unwrap()
}

#[test]
fn a_two_player_replay_is_format_2_and_plays_back_bit_for_bit() {
    let (dir, mut ws) = project("replay-two");
    let agent = parse_actor("agent");
    ws.call("sim_start", &json!({ "players": 2, "seed": 4 }), &agent);
    let recording = ws.call("replay", &json!({ "action": "record" }), &agent);
    assert_eq!(recording["ok"], true, "{recording}");
    for (input, other, frames) in [
        (json!({ "move": [0, 1] }), json!({ "move": [1, 0] }), 20),
        (json!({ "move": [0, 1] }), json!({ "jump": true }), 10),
        (json!({}), json!({ "move": [-1, 0] }), 15),
    ] {
        let stepped = ws.call(
            "sim_step",
            &json!({ "frames": frames, "input": input, "players": { "1": other } }),
            &agent,
        );
        assert_eq!(stepped["ok"], true, "{stepped}");
    }
    let end = at(&mut ws, "s1", "blue");
    let saved = ws.call(
        "replay",
        &json!({ "action": "save", "name": "duo" }),
        &agent,
    );
    assert_eq!(saved["players"], 2, "{saved}");
    // Changes: player 0 at tick 0, player 1 at 0, 20 and 30, player 0 at 30.
    assert_eq!(saved["input_changes"], 5, "{saved}");

    let text = replay_text(&dir, "duo");
    let head = header(&text);
    assert_eq!(head["format"], 2);
    assert_eq!(head["players"], 2);
    assert_eq!(head["engine"], env!("CARGO_PKG_VERSION"));
    assert_eq!(head["executor"], wish_script::EXECUTOR);
    let inputs: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["kind"] == "input")
        .collect();
    assert!(inputs.iter().all(|line| line.get("player").is_some()));
    assert_eq!(
        inputs
            .iter()
            .map(|l| (l["tick"].as_u64().unwrap(), l["player"].as_u64().unwrap()))
            .collect::<Vec<_>>(),
        [(0, 0), (0, 1), (20, 1), (30, 0), (30, 1)]
    );

    let ran = ws.call(
        "replay",
        &json!({ "action": "run", "name": "duo", "keep": true }),
        &agent,
    );
    assert_eq!(ran["passed"], true, "{ran}");
    assert_eq!(ran["players"], 2, "{ran}");
    assert!(codes(&ran).is_empty(), "{ran}");
    let again = ran["session"].as_str().unwrap().to_owned();
    assert!(near(at(&mut ws, &again, "blue"), end));

    // A format-2 file naming a player it does not have is refused.
    let broken = text.replacen("\"player\":1", "\"player\":5", 1);
    std::fs::write(dir.join("replays/broken.replay.jsonl"), broken).unwrap();
    let refused = ws.call(
        "replay",
        &json!({ "action": "run", "name": "broken" }),
        &agent,
    );
    assert_eq!(refused["error"]["code"], "invalid_replay_file", "{refused}");
}

#[test]
fn single_player_replays_stay_format_1_and_old_ones_still_play() {
    let (dir, mut ws) = project("replay-one");
    let agent = parse_actor("agent");
    ws.call("sim_start", &json!({ "seed": 2 }), &agent);
    ws.call("replay", &json!({ "action": "record" }), &agent);
    ws.call(
        "sim_step",
        &json!({ "frames": 12, "input": { "move": [1, 1] } }),
        &agent,
    );
    ws.call("sim_step", &json!({ "frames": 8 }), &agent);
    let saved = ws.call(
        "replay",
        &json!({ "action": "save", "name": "solo" }),
        &agent,
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert!(saved.get("players").is_none(), "{saved}");
    let text = replay_text(&dir, "solo");
    let head = header(&text);
    assert_eq!(head["format"], 1);
    assert!(head.get("players").is_none());
    assert_eq!(head["engine"], env!("CARGO_PKG_VERSION"));
    assert!(!text.contains("\"player\""), "{text}");
    let ran = ws.call(
        "replay",
        &json!({ "action": "run", "name": "solo" }),
        &agent,
    );
    assert_eq!(ran["passed"], true, "{ran}");
    assert!(codes(&ran).is_empty(), "{ran}");

    // The shape 1.0 wrote (no engine, no executor) still plays bit for bit,
    // and says that it does not say which engine made it.
    let mut old = header(&text);
    let fields = old.as_object_mut().unwrap();
    fields.remove("engine");
    fields.remove("executor");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines[0] = old.to_string();
    std::fs::write(
        dir.join("replays/old.replay.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let ran = ws.call("replay", &json!({ "action": "run", "name": "old" }), &agent);
    assert_eq!(ran["passed"], true, "{ran}");
    assert_eq!(codes(&ran), ["replay_engine_changed"], "{ran}");

    // Another engine and executor are said, and the replay still plays.
    let mut other = header(&text);
    other["engine"] = json!("1.0.0");
    other["executor"] = json!("web");
    lines[0] = other.to_string();
    std::fs::write(
        dir.join("replays/other.replay.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let ran = ws.call(
        "replay",
        &json!({ "action": "run", "name": "other" }),
        &agent,
    );
    assert_eq!(ran["passed"], true, "{ran}");
    assert_eq!(
        codes(&ran),
        ["replay_engine_changed", "replay_executor_changed"],
        "{ran}"
    );

    // A format-1 file cannot claim players, and a newer format is refused.
    let mut claims = header(&text);
    claims["players"] = json!(2);
    lines[0] = claims.to_string();
    std::fs::write(
        dir.join("replays/claims.replay.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let refused = ws.call(
        "replay",
        &json!({ "action": "run", "name": "claims" }),
        &agent,
    );
    assert_eq!(refused["error"]["code"], "invalid_replay_file", "{refused}");
    let mut newer = header(&text);
    newer["format"] = json!(3);
    lines[0] = newer.to_string();
    std::fs::write(
        dir.join("replays/newer.replay.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
    let refused = ws.call(
        "replay",
        &json!({ "action": "run", "name": "newer" }),
        &agent,
    );
    assert_eq!(refused["error"]["code"], "invalid_replay_file", "{refused}");
}
