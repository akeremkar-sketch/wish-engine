//! Online play through a relay, every part its own process (design note
//! `docs/design/1.1-net-tool.md` §13): `wen relay`, a host behind it
//! (`wen net host --relay`) and two players that join through it (`wen net
//! join --relay --room`), over localhost UDP. The host sees both seats come
//! through the relay, the match starts, and each joined process plays a
//! predicted seat of its own.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

fn wen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wen"))
}

/// The JSON lines a process printed, once it is done.
fn lines_of(child: Child) -> Vec<Value> {
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[test]
fn a_host_and_two_players_meet_through_wen_relay_and_the_players_predict() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("net-relay-processes");
    let _ = std::fs::remove_dir_all(&dir);
    wish_control::scaffold::create_project(&dir, "Relay test", "wen").unwrap();
    let root = dir.to_str().unwrap().to_owned();

    let mut relay = wen()
        .args([
            "relay",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--seconds",
            "60",
            "--json",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("wen relay starts");
    let mut relay_lines = BufReader::new(relay.stdout.take().unwrap()).lines();
    let first: Value = serde_json::from_str(&relay_lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(first["event"], "listening");
    let relay_addr = first["addr"].as_str().unwrap().to_owned();

    // The host registers its room at the relay (on this computer, so its
    // socket stays on the loopback address) and waits for three seats.
    let mut host = wen()
        .args([
            "net",
            "host",
            "--root",
            &root,
            "--relay",
            &relay_addr,
            "--players",
            "3",
            "--frames",
            "900",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wen net host starts");
    let mut host_out = BufReader::new(host.stdout.take().unwrap()).lines();
    let hosting: Value = serde_json::from_str(&host_out.next().unwrap().unwrap()).unwrap();
    assert_eq!(hosting["event"], "hosting", "{hosting}");
    let room = hosting["room_code"].as_str().unwrap().to_owned();
    assert_eq!(
        hosting["relay"]["address"],
        relay_addr.as_str(),
        "{hosting}"
    );

    let players: Vec<Child> = ["Far 1", "Far 2"]
        .iter()
        .map(|name| {
            wen()
                .args([
                    "net",
                    "join",
                    "--root",
                    &root,
                    "--relay",
                    &relay_addr,
                    "--room",
                    &room,
                    "--name",
                    name,
                    "--frames",
                    "1500",
                    "--json",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("wen net join starts")
        })
        .collect();

    let host_lines: Vec<Value> = host_out
        .map(|line| line.unwrap())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    assert!(host.wait().unwrap().success());
    let running = host_lines
        .iter()
        .rfind(|e| e["event"] == "lobby" && e["lobby"]["phase"] == "Running")
        .unwrap_or_else(|| panic!("the match never ran: {host_lines:#?}"));
    let seats = running["seats"].as_array().unwrap();
    assert_eq!(seats.len(), 3, "{running}");
    let relayed = seats.iter().filter(|s| s["via"] == "relay").count();
    assert_eq!(relayed, 2, "both players came through the relay: {running}");
    assert_eq!(running["relay"]["state"], "ready", "{running}");

    for (player, name) in players.into_iter().zip(["Far 1", "Far 2"]) {
        let lines = lines_of(player);
        let mine = lines
            .iter()
            .rev()
            .filter(|e| e["event"] == "lobby" && e["lobby"]["phase"] == "Running")
            .find_map(|e| {
                let seat = e["seats"]
                    .as_array()?
                    .iter()
                    .find(|s| s["machine"] == "you")?
                    .clone();
                Some((e.clone(), seat))
            })
            .unwrap_or_else(|| panic!("{name} never played: {lines:#?}"));
        let (event, seat) = mine;
        assert_eq!(seat["name"], name, "{event}");
        assert_eq!(seat["via"], "relay", "{event}");
        assert_eq!(
            seat["predicted"], true,
            "{name}'s own seat is predicted: {event}"
        );
        assert_eq!(event["relay"]["state"], "ready", "{event}");
        assert_eq!(event["relay"]["role"], "player", "{event}");
        let last = lines.last().unwrap();
        assert_eq!(last["ok"], true, "{last}");
    }
    let _ = relay.kill();
    let rest: Vec<String> = relay_lines.map_while(Result::ok).collect();
    let _ = relay.wait();
    assert!(
        rest.iter().any(|l| l.contains("room_opened")),
        "the relay opened the room: {rest:?}"
    );
    for line in &rest {
        assert!(
            !line.contains(&room),
            "the relay never sees the room code: {line}"
        );
    }
}
