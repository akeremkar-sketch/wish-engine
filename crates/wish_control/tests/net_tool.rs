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

/// The game of an online game made for prediction (design note
/// `1.1-net-tool.md` §12): every machine walks every avatar with its
/// player's `move` (2 m/s), so a player's machine predicts its own avatar
/// with the host's own system.
const PREDICTED_GAME: &str = r#"use wish_sdk::prelude::*;

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

/// Walks every avatar with its player's input, on every machine.
#[system]
fn walk(players: PlayerActions, mut avatars: Query<(&Avatar, &mut Transform)>, time: Res<Time>) {
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
    project_with(name, GAME, &["Transform", "NetOwner", "Avatar", "Secret"])
}

/// [`project`] with another game and replicated types.
fn project_with(name: &str, game: &str, replicate: &[&str]) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("net-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "Net test", "wen").unwrap();
    std::fs::write(dir.join(SCRIPTS_ENTRY), game).unwrap();
    let file = dir.join("wish.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    config["net"] = json!({
        "replicate": replicate,
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
            "NetOwner": { "player": k }, "Replicated": {}, "Primitive": { "shape": "Cube" } } }),
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
    // Player 1's own machine predicted a1 with a game that walks only on
    // the host: every confirmation of the walk corrected it, and once the
    // walk was over it ended where the host says, nothing left to smooth.
    let x1_own = x_of(&mut ws, &session, Some(1), "a1");
    assert!((x1_own - 4.0).abs() < 1e-3, "player 1 has a1 at {x1_own}");
    let one = call(
        &mut ws,
        "net",
        json!({ "action": "status", "session": session, "player": 1 }),
    );
    let prediction = &one["player"]["prediction"];
    assert!(prediction["corrections"].as_u64().unwrap() > 0, "{one}");
    assert!(
        prediction["largest_correction_mm"].as_u64().unwrap() > 0,
        "{one}"
    );
    assert_eq!(prediction["smoothing_mm"], 0, "{one}");
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

/// The types a predicting game replicates (the avatars' cube included, so
/// that a player's machine draws them).
const PREDICTED_TYPES: &[&str] = &["Transform", "NetOwner", "Avatar", "Secret", "Primitive"];

/// The bad network of the prediction tests: 100 ms each way, jitter, 5 %
/// loss.
fn bad_network() -> Value {
    json!({ "latency_ms": 100, "jitter_ms": 10, "loss": 0.05, "seed": 9 })
}

/// A scenario test of a match over 100 ms and 5 % loss in which a player's
/// avatar answers its input on the very tick in its own view (while the
/// host has not heard of it yet), and both end where the host says (design
/// note `1.1-net-tool.md` §12).
#[test]
fn a_player_sees_its_own_avatar_answer_at_once_over_a_bad_network() {
    let (dir, mut ws) = project_with("predicted", PREDICTED_GAME, PREDICTED_TYPES);
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests/predicted.test.json"),
        json!({
            "format": 1, "scene": "scenes/main.scene.json", "seed": 4,
            "net": { "players": 3, "bots": 2, "auto_start": true, "network": bad_network() },
            "steps": [
                { "until": "Lobby.phase == 'Running'", "max_frames": 1500 },
                { "frames": 120 },
                { "players": { "1": { "move": [1, 0] } }, "frames": 1 },
                { "expect": "Avatar.player == 1 && Transform.translation.x > 2.03 && Transform.translation.x < 2.07",
                  "player": 1, "message": "player 1's own avatar moved on the tick it pressed" },
                { "expect": "Transform.translation.x == 2", "entity": "a1",
                  "message": "the host has not heard of it yet" },
                { "players": { "1": { "move": [1, 0] } }, "frames": 59 },
                { "frames": 120 },
                { "expect": "Transform.translation.x > 3.99 && Transform.translation.x < 4.01", "entity": "a1",
                  "message": "the host walked a1 one second" },
                { "expect": "Avatar.player == 1 && Transform.translation.x > 3.99 && Transform.translation.x < 4.01",
                  "player": 1, "message": "player 1's prediction ends where the host says" },
                { "expect": "Avatar.player == 1 && Transform.translation.x > 3.99 && Transform.translation.x < 4.01",
                  "player": 2, "message": "player 2 sees a1 there too" }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let ran = ws.call(
        "test_run",
        &json!({ "paths": ["tests/predicted.test.json"] }),
        &parse_actor("agent"),
    );
    assert_eq!(ran["passed"], true, "{ran}");
    assert_eq!(ran["results"][0]["replay_verified"], true, "{ran}");
    let again = ws.call(
        "test_run",
        &json!({ "paths": ["tests/predicted.test.json"] }),
        &parse_actor("agent"),
    );
    assert_eq!(ran.to_string(), again.to_string(), "the same match twice");
}

/// The person plays a seat as a predicting player (`play_as: "player"`):
/// its own avatar answers at once in its own view and in its picture,
/// corrections stay rare and end, the host hears its frames in time, and two
/// workspaces give the same answers byte for byte.
#[test]
fn the_person_plays_a_predicted_seat_and_corrections_stay_rare() {
    let run = |name: &str| {
        let (_dir, mut ws) = project_with(name, PREDICTED_GAME, PREDICTED_TYPES);
        let hosted = call(
            &mut ws,
            "net",
            json!({ "action": "host", "players": 3, "bots": 2, "auto_start": true, "seed": 4,
                    "play_as": "player", "network": bad_network() }),
        );
        assert_eq!(hosted["host_plays"], false, "{hosted}");
        assert_eq!(hosted["you"], "You", "{hosted}");
        let session = hosted["session"].as_str().unwrap().to_owned();
        let mut answers = Vec::new();
        let started = call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "until": "Lobby.phase == 'Running'", "max_frames": 1500 }),
        );
        assert_eq!(started["stopped_by"], "until", "{started}");
        answers.push(started.to_string());
        let settled = call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 120 }),
        );
        answers.push(settled.to_string());
        let seats = settled["seats"].as_array().unwrap().clone();
        assert_eq!(seats.len(), 3, "{settled}");
        let you = seats
            .iter()
            .find(|s| s["machine"] == "you")
            .and_then(|s| s["player"].as_u64())
            .unwrap_or_else(|| panic!("no seat of the person: {settled}"));
        for seat in &seats {
            assert_eq!(seat["predicted"], true, "{seat}");
            assert!(
                seat["lead_ticks"].as_i64().unwrap() > 6,
                "the lead covers the trip: {seat}"
            );
        }
        let avatar = format!("a{you}");
        let start = x_of(&mut ws, &session, None, &avatar);
        // One tick of the person's input (the session's own input).
        call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 1, "input": { "move": [1, 0] } }),
        );
        let own = x_of(&mut ws, &session, Some(you as u32), &avatar);
        // The frame predicts one tick (two when tick sync catches up).
        let moved = (own - start) * 30.0;
        assert!(
            (moved - 1.0).abs() < 1e-3 || (moved - 2.0).abs() < 1e-3,
            "the person's avatar answers on the tick: {start} -> {own}"
        );
        let on_host = x_of(&mut ws, &session, None, &avatar);
        assert_eq!(on_host, start, "the host has not heard of it yet");
        // The picture the person sees draws it there too.
        let seen = ws.play_scene(&session).unwrap();
        let drawn: Vec<f32> = seen.draws.iter().map(|draw| draw.world.w_axis.x).collect();
        assert!(
            drawn.iter().any(|x| (f64::from(*x) - own).abs() < 1e-4),
            "the person's picture draws its avatar where it predicted it ({own}): {drawn:?}"
        );
        // A second of walking, then rest.
        answers.push(
            call(
                &mut ws,
                "net",
                json!({ "action": "step", "session": session, "frames": 59, "input": { "move": [1, 0] } }),
            )
            .to_string(),
        );
        answers.push(
            call(
                &mut ws,
                "net",
                json!({ "action": "step", "session": session, "frames": 180 }),
            )
            .to_string(),
        );
        let host_x = x_of(&mut ws, &session, None, &avatar);
        let own_x = x_of(&mut ws, &session, Some(you as u32), &avatar);
        assert!(
            (host_x - start - 2.0).abs() < 1e-3,
            "the host walked it 2 m: {host_x}"
        );
        assert_eq!(own_x, host_x, "the prediction ends where the host says");
        let mine = call(
            &mut ws,
            "net",
            json!({ "action": "status", "session": session, "player": you }),
        );
        let prediction = &mine["player"]["prediction"];
        let confirmations = prediction["confirmations"].as_u64().unwrap();
        let corrections = prediction["corrections"].as_u64().unwrap();
        assert!(confirmations > 100, "{mine}");
        assert!(
            corrections * 50 <= confirmations,
            "corrections are rare (at most 2 % of the confirmations): {mine}"
        );
        assert_eq!(
            prediction["smoothing_mm"], 0,
            "nothing is left to smooth: {mine}"
        );
        assert!(
            prediction["late_frames"].as_u64().unwrap() <= 2,
            "the host hears the person's frames in time: {mine}"
        );
        answers.push(mine.to_string());
        answers
    };
    let first = run("predicted-a");
    let second = run("predicted-b");
    assert_eq!(first, second, "the same calls give the same answers");
}

/// The seat whose name is `name` in a `net status` (or `net step`) answer.
fn seat_named(status: &Value, name: &str) -> Value {
    status["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no seat '{name}': {status}"))
}

/// A host behind a home router takes players on its local network and
/// through a relay on the same socket (design note `1.1-net-tool.md` §13):
/// over the simulator with a relay node the session runs itself, a host,
/// two direct bots and two players through the relay play one match; the
/// status says which seat came how and how the relay stands; a relayed
/// player's own avatar answers on its input tick while the host's has not
/// moved, and ends where the host says; the same seed gives the same
/// answers.
#[test]
fn a_host_takes_direct_and_relayed_players_in_one_match() {
    let run = |name: &str| {
        let (_dir, mut ws) = project_with(name, PREDICTED_GAME, PREDICTED_TYPES);
        let mut answers = Vec::new();
        let hosted = call(
            &mut ws,
            "net",
            json!({ "action": "host", "players": 5, "bots": 2, "auto_start": true, "seed": 3,
                    "relay": "simulated",
                    "network": { "latency_ms": 40, "jitter_ms": 5, "loss": 0.02, "seed": 3 } }),
        );
        assert_eq!(hosted["relay"]["simulated"], true, "{hosted}");
        let session = hosted["session"].as_str().unwrap().to_owned();
        answers.push(hosted.to_string());
        for far in ["Far 1", "Far 2"] {
            let joined = call(
                &mut ws,
                "net",
                json!({ "action": "join", "session": session, "name": far, "relay": "simulated" }),
            );
            answers.push(joined.to_string());
        }
        let started = call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "until": "Lobby.phase == 'Running'", "max_frames": 1500 }),
        );
        assert_eq!(started["stopped_by"], "until", "{started}");
        let settled = call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 120 }),
        );
        answers.push(settled.to_string());
        let status = call(
            &mut ws,
            "net",
            json!({ "action": "status", "session": session }),
        );
        assert_eq!(status["seats"].as_array().unwrap().len(), 5, "{status}");
        for bot in ["Bot 1", "Bot 2"] {
            assert_eq!(seat_named(&status, bot)["via"], "direct", "{status}");
        }
        for far in ["Far 1", "Far 2"] {
            let seat = seat_named(&status, far);
            assert_eq!(seat["via"], "relay", "{status}");
            assert_eq!(seat["predicted"], true, "{status}");
        }
        let relay = &status["relay"];
        assert_eq!(relay["state"], "ready", "{status}");
        assert_eq!(relay["role"], "host", "{status}");
        assert_eq!(relay["rooms"], 1, "{status}");
        assert_eq!(relay["clients"], 2, "{status}");
        assert!(relay["forwarded"].as_u64().unwrap() > 100, "{status}");
        answers.push(status.to_string());
        // Far 1 walks: its own machine answers on the tick, the host later.
        let seat = seat_named(&status, "Far 1")["player"].as_u64().unwrap() as u32;
        let avatar = format!("a{seat}");
        let start = x_of(&mut ws, &session, None, &avatar);
        let walk = json!({ seat.to_string(): { "move": [1, 0] } });
        call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 1, "inputs": walk }),
        );
        let own = x_of(&mut ws, &session, Some(seat), &avatar);
        assert!(
            own > start + 0.03,
            "the relayed player's avatar answers on the tick: {start} -> {own}"
        );
        assert_eq!(
            x_of(&mut ws, &session, None, &avatar),
            start,
            "the host has not heard of it yet"
        );
        call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 59, "inputs": walk }),
        );
        call(
            &mut ws,
            "net",
            json!({ "action": "step", "session": session, "frames": 180 }),
        );
        let host_x = x_of(&mut ws, &session, None, &avatar);
        let own_x = x_of(&mut ws, &session, Some(seat), &avatar);
        assert!(
            (host_x - start - 2.0).abs() < 1e-3,
            "the host walked it 2 m: {host_x}"
        );
        assert_eq!(own_x, host_x, "the prediction ends where the host says");
        let mine = call(
            &mut ws,
            "net",
            json!({ "action": "status", "session": session, "player": seat }),
        );
        answers.push(mine.to_string());
        answers
    };
    let first = run("relay-sim-a");
    let second = run("relay-sim-b");
    assert_eq!(first, second, "the same seed gives the same answers");
}

/// What `relay` cannot name is explained, with its location.
#[test]
fn a_relay_that_cannot_be_used_is_explained() {
    let (_dir, mut ws) = project("relay-mistakes");
    let agent = parse_actor("agent");
    let real_over_sim = ws.call(
        "net",
        &json!({ "action": "host", "relay": "203.0.113.7:47620" }),
        &agent,
    );
    assert_eq!(
        real_over_sim["error"]["code"], "net_config_invalid",
        "{real_over_sim}"
    );
    assert_eq!(
        real_over_sim["error"]["location"]["path"], "/relay",
        "{real_over_sim}"
    );
    let hosted = call(&mut ws, "net", json!({ "action": "host" }));
    let session = hosted["session"].as_str().unwrap().to_owned();
    let no_relay = ws.call(
        "net",
        &json!({ "action": "join", "session": session, "relay": "simulated" }),
        &agent,
    );
    assert_eq!(
        no_relay["error"]["code"], "net_config_invalid",
        "{no_relay}"
    );
    call(
        &mut ws,
        "net",
        json!({ "action": "stop", "session": session }),
    );
    let simulated_over_udp = ws.call(
        "net",
        &json!({ "action": "join", "relay": "simulated", "room_code": "ABC234" }),
        &agent,
    );
    assert_eq!(
        simulated_over_udp["error"]["code"], "net_config_invalid",
        "{simulated_over_udp}"
    );
    let no_room = ws.call(
        "net",
        &json!({ "action": "join", "relay": "127.0.0.1:47620" }),
        &agent,
    );
    assert_eq!(
        no_room["error"]["code"], "net_room_code_invalid",
        "{no_room}"
    );
}

/// This process joins a host elsewhere through a real relay over localhost
/// UDP, and its own world is a prediction (design note `1.1-net-tool.md`
/// §13): its own avatar answers on the call that gives the input, before the
/// host has stepped; after a walk both end where the host says.
#[test]
fn a_process_joined_through_a_relay_predicts_its_own_player() {
    let (dir, mut host) = project_with("relay-udp", PREDICTED_GAME, PREDICTED_TYPES);
    let mut far = open(&dir);
    let socket = wish_net::UdpTransport::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut relay = wish_net::relay::Relay::new(
        socket,
        wish_net::relay::RelayConfig::default(),
        wish_net::Entropy::os().unwrap(),
    )
    .unwrap();
    let relay_addr = relay.local_addr().to_string();
    let hosted = call(
        &mut host,
        "net",
        json!({ "action": "host", "players": 2, "auto_start": true, "port": 0, "relay": relay_addr }),
    );
    let hs = hosted["session"].as_str().unwrap().to_owned();
    let room = hosted["room_code"].as_str().unwrap().to_owned();
    assert_eq!(hosted["relay"]["simulated"], false, "{hosted}");
    let joined = call(
        &mut far,
        "net",
        json!({ "action": "join", "relay": relay_addr, "room_code": room, "name": "Far" }),
    );
    let fs = joined["session"].as_str().unwrap().to_owned();
    // The two machines and the relay take turns, as three computers would.
    let mut clock = 0u64;
    let mut turn = |host: &mut Workspace, far: &mut Workspace, input: Option<Value>| {
        clock += 16;
        relay.update(clock);
        call(
            host,
            "net",
            json!({ "action": "step", "session": hs, "frames": 1 }),
        );
        relay.update(clock);
        let mut step = json!({ "action": "step", "session": fs, "frames": 1 });
        if let Some(input) = input {
            step["input"] = input;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
        call(far, "net", step)
    };
    let mut seat = None;
    for _ in 0..1500 {
        let answer = turn(&mut host, &mut far, None);
        let mine = answer["seats"]
            .as_array()
            .and_then(|seats| seats.iter().find(|s| s["machine"] == "you").cloned());
        if answer["lobby"]["phase"] == "Running"
            && let Some(mine) = mine
        {
            seat = mine["player"].as_u64();
            break;
        }
    }
    let seat = seat.expect("the process is seated through the relay and the match runs") as u32;
    for _ in 0..120 {
        turn(&mut host, &mut far, None);
    }
    let status = call(
        &mut far,
        "net",
        json!({ "action": "status", "session": fs }),
    );
    assert_eq!(status["role"], "player", "{status}");
    assert_eq!(status["relay"]["state"], "ready", "{status}");
    let mine = seat_named(&status, "Far");
    assert_eq!(mine["machine"], "you", "{status}");
    assert_eq!(mine["via"], "relay", "{status}");
    assert_eq!(mine["predicted"], true, "{status}");
    let host_status = call(
        &mut host,
        "net",
        json!({ "action": "status", "session": hs }),
    );
    assert_eq!(
        seat_named(&host_status, "Far")["via"],
        "relay",
        "{host_status}"
    );
    assert_eq!(host_status["relay"]["state"], "ready", "{host_status}");
    // The session's own world (no player named) is the prediction.
    let avatar = format!("a{seat}");
    let start = x_of(&mut far, &fs, None, &avatar);
    assert_eq!(x_of(&mut host, &hs, None, &avatar), start);
    let answer = turn(&mut host, &mut far, Some(json!({ "move": [1, 0] })));
    let own = x_of(&mut far, &fs, None, &avatar);
    assert!(
        own > start + 0.03,
        "its own avatar answers on the call: {start} -> {own}: {answer}"
    );
    assert_eq!(
        x_of(&mut host, &hs, None, &avatar),
        start,
        "the host has not heard of it yet"
    );
    assert_eq!(
        x_of(&mut far, &fs, Some(seat), &avatar),
        own,
        "player names the same world"
    );
    for _ in 0..59 {
        turn(&mut host, &mut far, Some(json!({ "move": [1, 0] })));
    }
    for _ in 0..180 {
        turn(&mut host, &mut far, None);
    }
    let host_x = x_of(&mut host, &hs, None, &avatar);
    let own_x = x_of(&mut far, &fs, None, &avatar);
    assert!(
        (host_x - start - 2.0).abs() < 1e-3,
        "the host walked it 2 m: {host_x}"
    );
    assert!(
        (own_x - host_x).abs() < 1e-4,
        "the prediction ends where the host says: {own_x} vs {host_x}"
    );
    let mine = call(
        &mut far,
        "net",
        json!({ "action": "status", "session": fs, "player": seat }),
    );
    assert!(
        mine["player"]["prediction"]["confirmations"]
            .as_u64()
            .unwrap()
            > 100,
        "{mine}"
    );
    call(&mut far, "net", json!({ "action": "stop", "session": fs }));
    call(&mut host, "net", json!({ "action": "stop", "session": hs }));
}
