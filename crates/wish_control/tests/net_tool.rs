//! Online play in the control plane (release 1.1, package 14; design note
//! `docs/design/1.1-net-tool.md`): the `net` tool hosts a match in a
//! session, bots play it over a simulated (lossy) network, a player's
//! machine is looked at through `player`, a scenario test plays a six-player
//! match, the host's match log is a replay that verifies, the same calls
//! give the same match, mistakes are explained, and a match runs over
//! localhost UDP too.

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
        let sdk = tmp.join("sdk-pack-net");
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

/// The host walks every avatar with its player's `move` (2 m/s); a player's
/// machine only shows what the host sends.
const GAME: &str = r#"use wish_sdk::prelude::*;

/// A player's body.
#[component]
pub struct Avatar {
    /// The player who walks it.
    #[schema(range = 0..=15)]
    pub player: PlayerId,
}

/// A number only its owner may see.
#[component]
pub struct Secret {
    /// The number.
    pub number: u32,
}

/// Walks every avatar with its player's input, on the host.
#[system]
fn walk(players: PlayerActions, local: Res<LocalPlayer>, mut avatars: Query<(&Avatar, &mut Transform)>, time: Res<Time>) {
    if !local.host {
        return;
    }
    for (avatar, transform) in avatars.iter_mut() {
        let step = players.of(avatar.player).axis2("move");
        transform.translation += Vec3::new(step.x, 0.0, -step.y) * 2.0 * time.delta;
    }
}

wish_sdk::export! {
    components: [Avatar, Secret],
    systems: [walk],
}
"#;

/// A built project with six avatars `a0`..`a5` (at x = 2k, everyone sees
/// them) and six secrets `s0`..`s5` (only their owner sees them), and a
/// `net` section that replicates them.
fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("net-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Net test", "wen").unwrap();
    std::fs::write(dir.join(SCRIPTS_ENTRY), GAME).unwrap();
    let file = dir.join("wish.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    config["net"] = json!({
        "replicate": ["Transform", "NetOwner", "Avatar", "Secret"],
        "lobby": { "roles": [{ "name": "crew" }, { "name": "saboteur", "count": 1 }] }
    });
    std::fs::write(&file, serde_json::to_string_pretty(&config).unwrap()).unwrap();
    let mut ws = open(&dir);
    let agent = parse_actor("agent");
    let built = ws.call("script_build", &json!({}), &agent);
    assert_eq!(built["passed"], true, "{built}");
    let mut ops = Vec::new();
    for k in 0..6 {
        ops.push(
            json!({ "op": "spawn", "id": format!("a{k}"), "components": {
            "Transform": { "translation": [2 * k, 0, 0] }, "Avatar": { "player": k },
            "NetOwner": { "player": k }, "Replicated": {} } }),
        );
        ops.push(
            json!({ "op": "spawn", "id": format!("s{k}"), "components": {
            "Secret": { "number": 1000 + k }, "NetOwner": { "player": k },
            "Replicated": { "visibility": "Owner" } } }),
        );
    }
    let edited = ws.call("edit_apply", &json!({ "ops": ops }), &agent);
    assert_eq!(edited["ok"], true, "{edited}");
    (dir, ws)
}

fn open(dir: &Path) -> Workspace {
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.to_path_buf(),
        example: None,
    })
    .unwrap();
    ws.set_toolchain(toolchain().clone());
    ws
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], true, "{tool} {args}: {answer}");
    answer
}

fn x_of(ws: &mut Workspace, session: &str, player: Option<u32>, id: &str) -> f64 {
    let mut args =
        json!({ "session": session, "with": ["Avatar"], "components": ["Avatar", "Transform"] });
    if let Some(player) = player {
        args["player"] = json!(player);
    }
    let found = call(ws, "entity_query", args);
    let k: u64 = id[1..].parse().unwrap();
    found["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["components"]["Avatar"]["player"] == k)
        .map(|e| {
            e["components"]["Transform"]["translation"][0]
                .as_f64()
                .unwrap()
        })
        .unwrap_or_else(|| panic!("no avatar {id}: {found}"))
}

/// Hosts a three-seat match with two bots over a lossy network and plays
/// it: the calls of the tests below.
fn play(ws: &mut Workspace) -> (String, Value) {
    let hosted = call(
        ws,
        "net",
        json!({ "action": "host", "players": 3, "bots": 2, "auto_start": true, "seed": 1,
                "network": { "latency_ms": 50, "jitter_ms": 10, "loss": 0.02, "seed": 5 } }),
    );
    let session = hosted["session"].as_str().unwrap().to_owned();
    assert_eq!(hosted["bots"], json!(["Bot 1", "Bot 2"]), "{hosted}");
    assert_eq!(hosted["network"]["kind"], "simulated");
    let started = call(
        ws,
        "net",
        json!({ "action": "step", "session": session, "until": "Lobby.phase == 'Running'", "max_frames": 600 }),
    );
    assert_eq!(started["stopped_by"], "until", "{started}");
    // Seat 1 walks right for a second while the host walks forward.
    call(
        ws,
        "net",
        json!({ "action": "step", "session": session, "frames": 60,
                "input": { "move": [0, 1] }, "inputs": { "1": { "move": [1, 0] } } }),
    );
    let settled = call(
        ws,
        "net",
        json!({ "action": "step", "session": session, "frames": 60 }),
    );
    (session, settled)
}

#[test]
fn bots_play_a_match_over_a_lossy_network_and_each_sees_what_it_may() {
    let (_dir, mut ws) = project("match");
    let (session, settled) = play(&mut ws);
    assert_eq!(settled["lobby"]["phase"], "Running", "{settled}");

    let status = call(
        &mut ws,
        "net",
        json!({ "action": "status", "session": session }),
    );
    let seats = status["seats"].as_array().unwrap();
    assert_eq!(seats.len(), 3, "{status}");
    assert_eq!(seats[0]["machine"], "host");
    for seat in &seats[1..] {
        assert_eq!(seat["machine"], "bot", "{seat}");
        assert_eq!(seat["ready"], true);
        assert!(seat["rtt_ms"].as_u64().unwrap() >= 100, "{seat}");
        assert!(seat["bytes_received"].as_u64().unwrap() > 0);
    }
    assert!(status["host"]["states_sent"].as_u64().unwrap() > 0);
    assert_eq!(status["network"]["latency_ms"], 50);

    // The host's world: seat 1 walked right 2 m (input over the network,
    // every frame applied once), the host forward.
    let x1 = x_of(&mut ws, &session, None, "a1");
    assert!((x1 - 4.0).abs() < 1e-3, "a1 at {x1}");
    // What player 2's machine was sent: the same avatars, and of the
    // secrets only its own.
    let x1_seen = x_of(&mut ws, &session, Some(2), "a1");
    assert!(
        (x1_seen - 4.0).abs() < 1e-3,
        "player 2 sees a1 at {x1_seen}"
    );
    let secrets = call(
        &mut ws,
        "entity_query",
        json!({ "session": session, "player": 2, "with": ["Secret"], "components": ["Secret"] }),
    );
    assert_eq!(secrets["matched"], 1, "{secrets}");
    assert_eq!(
        secrets["entities"][0]["components"]["Secret"]["number"],
        1002
    );
    let on_host = call(
        &mut ws,
        "entity_query",
        json!({ "session": session, "with": ["Secret"] }),
    );
    assert_eq!(on_host["matched"], 6);
    let mine = call(
        &mut ws,
        "net",
        json!({ "action": "status", "session": session, "player": 2 }),
    );
    assert_eq!(mine["player"]["local_player"]["player"], 2, "{mine}");
    assert_eq!(mine["player"]["local_player"]["joined"], true);

    // A picture of what player 1's machine has.
    ws.set_software(true);
    let picture = call(
        &mut ws,
        "describe_view",
        json!({ "session": session, "player": 1, "width": 64, "height": 36 }),
    );
    assert_eq!(picture["player"], 1, "{picture}");

    // The match is its replay.
    let saved = call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": session, "name": "match" }),
    );
    assert_eq!(saved["format"], 3, "{saved}");
    let run = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "match" }),
    );
    assert_eq!(run["passed"], true, "{run}");
    assert_eq!(run["ticks"], saved["ticks"]);

    // A player leaves; the host hears it; the match is stopped for all.
    call(
        &mut ws,
        "net",
        json!({ "action": "leave", "session": session, "player": 2 }),
    );
    let after = call(
        &mut ws,
        "net",
        json!({ "action": "step", "session": session, "frames": 30 }),
    );
    // Its seat is no longer one of the match's players.
    let seated: Vec<&Value> = after["seats"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["player"] == 2 && s["left"] != true)
        .collect();
    assert!(seated.is_empty(), "{after}");
    let stopped = call(
        &mut ws,
        "net",
        json!({ "action": "stop", "session": session }),
    );
    assert_eq!(stopped["open_sessions"], json!([]), "{stopped}");
}

#[test]
fn the_same_calls_give_the_same_match() {
    let (_a, mut first) = project("same-a");
    let (_b, mut second) = project("same-b");
    let (s1, one) = play(&mut first);
    let (s2, two) = play(&mut second);
    assert_eq!(one["world_hash"], two["world_hash"]);
    let status = |ws: &mut Workspace, session: &str| {
        call(
            ws,
            "net",
            json!({ "action": "status", "session": session, "player": 1 }),
        )
    };
    let (a, b) = (status(&mut first, &s1), status(&mut second, &s2));
    assert_eq!(a["network"]["delivery_hash"], b["network"]["delivery_hash"]);
    assert_eq!(a["player"]["world_hash"], b["player"]["world_hash"]);
    assert_eq!(a.to_string(), b.to_string());
}

#[test]
fn a_scenario_test_plays_six_players_over_a_bad_network() {
    let (dir, mut ws) = project("scenario");
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests/six.test.json"),
        json!({
            "format": 1, "scene": "scenes/main.scene.json", "seed": 4,
            "net": { "players": 6, "bots": 5, "auto_start": true,
                     "network": { "latency_ms": 100, "jitter_ms": 20, "loss": 0.05, "seed": 9 } },
            "steps": [
                { "until": "Lobby.phase == 'Running'", "max_frames": 1500 },
                { "players": { "3": { "move": [1, 0] } }, "frames": 60 },
                { "frames": 90 },
                { "expect": "Transform.translation.x > 7.99 && Transform.translation.x < 8.01", "entity": "a3",
                  "message": "seat 3 walked 2 m to the right" },
                { "expect": "count(Secret) == 1", "player": 4, "message": "player 4 sees only its own secret" },
                { "expect": "LocalPlayer.joined == true && LocalPlayer.player == 4", "player": 4 },
                { "expect": "count(Secret) == 6", "message": "the host has every secret" }
            ]
        })
        .to_string(),
    )
    .unwrap();
    // A test that expects what is not so fails at that step.
    std::fs::write(
        dir.join("tests/wrong.test.json"),
        json!({
            "format": 1, "scene": "scenes/main.scene.json", "seed": 4,
            "net": { "players": 2, "bots": 1, "auto_start": true },
            "steps": [
                { "until": "Lobby.phase == 'Running'", "max_frames": 600 },
                { "expect": "count(Secret) == 6", "player": 1 }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let ran = ws.call(
        "test_run",
        &json!({ "paths": ["tests/six.test.json"] }),
        &parse_actor("agent"),
    );
    assert_eq!(ran["passed"], true, "{ran}");
    assert_eq!(ran["results"][0]["replay_verified"], true, "{ran}");
    let again = ws.call(
        "test_run",
        &json!({ "paths": ["tests/six.test.json"] }),
        &parse_actor("agent"),
    );
    assert_eq!(ran.to_string(), again.to_string(), "the same match twice");
    let wrong = ws.call(
        "test_run",
        &json!({ "paths": ["tests/wrong.test.json"] }),
        &parse_actor("agent"),
    );
    assert_eq!(wrong["passed"], false);
    assert_eq!(wrong["results"][0]["failure"]["step"], 1, "{wrong}");
    assert_eq!(wrong["results"][0]["failure"]["kind"], "expect");
}

#[test]
fn mistakes_on_networked_sessions_are_explained() {
    let (dir, mut ws) = project("mistakes");
    let agent = parse_actor("agent");
    let plain = call(&mut ws, "sim_start", json!({}));
    let plain = plain["session"].as_str().unwrap().to_owned();
    let not_net = ws.call(
        "net",
        &json!({ "action": "status", "session": plain }),
        &agent,
    );
    assert_eq!(not_net["error"]["code"], "net_not_networked", "{not_net}");
    let looked = ws.call(
        "entity_query",
        &json!({ "session": plain, "player": 1, "with": ["Avatar"] }),
        &agent,
    );
    assert_eq!(looked["error"]["code"], "net_not_networked", "{looked}");
    let no_session = ws.call("entity_query", &json!({ "player": 1 }), &agent);
    assert_eq!(no_session["error"]["location"]["path"], "/player");

    let hosted = call(
        &mut ws,
        "net",
        json!({ "action": "host", "players": 2, "bots": 1, "ready": false }),
    );
    let net = hosted["session"].as_str().unwrap().to_owned();
    call(
        &mut ws,
        "net",
        json!({ "action": "step", "session": net, "frames": 30 }),
    );
    for (tool, args) in [
        ("sim_snapshot", json!({ "session": net })),
        ("replay", json!({ "action": "record", "session": net })),
        (
            "edit_apply",
            json!({ "session": net, "ops": [{ "op": "despawn", "id": "a0" }] }),
        ),
    ] {
        let refused = ws.call(tool, &args, &agent);
        assert_eq!(
            refused["error"]["code"], "net_session_unsupported",
            "{tool}: {refused}"
        );
    }
    let nobody = ws.call(
        "net",
        &json!({ "action": "status", "session": net, "player": 5 }),
        &agent,
    );
    assert_eq!(nobody["error"]["code"], "net_player_unknown", "{nobody}");
    let not_ready = ws.call("net", &json!({ "action": "start", "session": net }), &agent);
    assert_eq!(not_ready["error"]["code"], "lobby_not_ready", "{not_ready}");
    let bad_network = ws.call(
        "net",
        &json!({ "action": "host", "network": { "lag": 5 } }),
        &agent,
    );
    assert_eq!(bad_network["error"]["location"]["path"], "/network/lag");
    let bad_address = ws.call(
        "net",
        &json!({ "action": "join", "session": net, "address": "not an address" }),
        &agent,
    );
    assert_eq!(bad_address["error"]["code"], "net_join_failed");

    // wish.json's net section is checked with the place of the mistake.
    let file = dir.join("wish.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    config["net"]["lobby"]["min_players"] = json!(40);
    std::fs::write(&file, config.to_string()).unwrap();
    let mut reopened = open(&dir);
    let checked = reopened.call("check", &json!({ "paths": ["wish.json"] }), &agent);
    let problem = &checked["diagnostics"][0];
    assert_eq!(problem["code"], "invalid_project_file", "{checked}");
    assert_eq!(problem["location"]["path"], "/net/lobby/min_players");
}

#[test]
fn a_discovered_host_is_joined_over_the_simulator() {
    let (_dir, mut ws) = project("discover");
    let hosted = call(
        &mut ws,
        "net",
        json!({ "action": "host", "players": 2, "auto_start": true }),
    );
    let session = hosted["session"].as_str().unwrap().to_owned();
    call(
        &mut ws,
        "net",
        json!({ "action": "join", "session": session, "address": "discover", "name": "Ada" }),
    );
    let started = call(
        &mut ws,
        "net",
        json!({ "action": "step", "session": session, "until": "Lobby.phase == 'Running'", "max_frames": 600 }),
    );
    assert_eq!(started["stopped_by"], "until", "{started}");
    let ada = started["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["player"] == 1)
        .unwrap();
    assert_eq!(ada["name"], "Ada");
    assert_eq!(ada["machine"], "local");
}

#[test]
fn a_match_runs_over_localhost_udp() {
    let (_dir, mut ws) = project("udp");
    let hosted = call(
        &mut ws,
        "net",
        json!({ "action": "host", "players": 2, "bots": 1, "auto_start": true, "port": 0 }),
    );
    let session = hosted["session"].as_str().unwrap().to_owned();
    assert_eq!(hosted["network"]["kind"], "udp", "{hosted}");
    let started = call(
        &mut ws,
        "net",
        json!({ "action": "step", "session": session, "until": "Lobby.phase == 'Running'", "max_frames": 3000 }),
    );
    assert_eq!(started["stopped_by"], "until", "{started}");
    call(
        &mut ws,
        "net",
        json!({ "action": "step", "session": session, "frames": 120 }),
    );
    let secrets = call(
        &mut ws,
        "entity_query",
        json!({ "session": session, "player": 1, "with": ["Secret"] }),
    );
    assert_eq!(secrets["matched"], 1, "{secrets}");
    call(
        &mut ws,
        "net",
        json!({ "action": "stop", "session": session }),
    );
}
