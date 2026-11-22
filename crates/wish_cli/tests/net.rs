//! `wen net` (release 1.1, package 14): a host and a player as two
//! processes over localhost UDP, the way people play on a local network.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

fn wen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wen"))
}

#[test]
fn wen_net_hosts_and_a_second_process_joins_over_localhost_udp() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wen-net");
    let _ = std::fs::remove_dir_all(&dir);
    let made = wen().args(["new", dir.to_str().unwrap()]).output().unwrap();
    assert!(made.status.success(), "{made:?}");
    let root = dir.to_str().unwrap();
    let mut host = wen()
        .args([
            "net",
            "host",
            "--port",
            "0",
            "--players",
            "2",
            "--frames",
            "1200",
            "--json",
            "--root",
            root,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(host.stdout.take().unwrap()).lines();
    let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(first["event"], "hosting", "{first}");
    let port = first["port"].as_u64().unwrap();
    let room = first["room_code"].as_str().unwrap().to_owned();
    let joined = wen()
        .args([
            "net",
            "join",
            &format!("127.0.0.1:{port}"),
            "--room",
            &room,
            "--name",
            "Ada",
            "--frames",
            "600",
            "--json",
            "--root",
            root,
        ])
        .output()
        .unwrap();
    assert!(joined.status.success(), "{joined:?}");
    let seen: Vec<Value> = String::from_utf8(joined.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let rest: Vec<Value> = lines
        .map_while(std::result::Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    assert!(host.wait().unwrap().success());
    // The host saw Ada take seat 1 and the match start; Ada saw it too.
    let running_with_ada = |events: &[Value]| {
        events.iter().any(|event| {
            event["event"] == "lobby"
                && event["lobby"]["phase"] == "Running"
                && event["seats"].as_array().is_some_and(|seats| {
                    seats.iter().any(|s| s["name"] == "Ada" && s["player"] == 1)
                })
        })
    };
    assert!(running_with_ada(&rest), "host: {rest:?}");
    assert!(running_with_ada(&seen), "player: {seen:?}");
}
