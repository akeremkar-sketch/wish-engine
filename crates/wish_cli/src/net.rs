//! `wen net`: online play over UDP with people (release 1.1, package 14;
//! design note `docs/design/1.1-net-tool.md` §8).
//!
//! A loop of `net` tool calls: `host` or `join`, then one `step` per tick
//! paced at 60 ticks a second of real time, and a line whenever the lobby
//! changes, until the match closes (or `--frames` ticks). The pacing is the
//! command line's: the control plane itself never reads the clock.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_core::{Diagnostic, Result};

use crate::args::NetArgs;

/// The ticks of a second.
const RATE: u64 = 60;

fn call(workspace: &mut Workspace, tool: &str, args: &Value) -> Result<Value> {
    let answer = workspace.call(tool, args, &parse_actor("human"));
    if answer["ok"] == true {
        return Ok(answer);
    }
    Err(
        serde_json::from_value::<Diagnostic>(answer["error"].clone()).unwrap_or_else(|_| {
            Diagnostic::error(
                "internal_error",
                format!("The {tool} call failed: {answer}"),
                "This is a bug in wen. Please report it.",
            )
        }),
    )
}

/// One line about the lobby, for people.
fn lobby_line(status: &Value) -> String {
    let lobby = &status["lobby"];
    let seats: Vec<String> = status["seats"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|seat| {
            let mut flags = Vec::new();
            if seat["ready"] == true {
                flags.push("ready");
            }
            if seat["connected"] != true {
                flags.push("away");
            }
            if seat["left"] == true {
                flags.push("left");
            }
            if seat["via"] == "relay" {
                flags.push("relay");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" ({})", flags.join(", "))
            };
            format!(
                "{} {}{flags}",
                seat["player"],
                seat["name"].as_str().unwrap_or("?")
            )
        })
        .collect();
    let relay = match status["relay"]["state"].as_str() {
        Some("refused") => format!(
            " [relay refused: {}]",
            status["relay"]["reason"].as_str().unwrap_or("?")
        ),
        Some(state) => format!(" [relay {state}]"),
        None => String::new(),
    };
    format!(
        "{} round {}: {}{relay}",
        lobby["phase"].as_str().unwrap_or("?"),
        lobby["round"],
        if seats.is_empty() {
            "no players yet".to_owned()
        } else {
            seats.join(", ")
        }
    )
}

fn say(as_json: bool, text: &str, value: Value) {
    let mut out = std::io::stdout();
    if as_json {
        let _ = writeln!(out, "{value}");
    } else {
        let _ = writeln!(out, "{text}");
    }
    let _ = out.flush();
}

/// Runs `wen net`.
pub fn run(args: &NetArgs, as_json: bool) -> Result<String> {
    let root = PathBuf::from(args.root.as_deref().unwrap_or("."));
    let mut workspace = Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })?;
    let mut start = json!({ "action": args.action });
    if let Some(scene) = &args.scene {
        start["scene"] = json!(scene);
    }
    if let Some(room) = &args.room {
        start["room_code"] = json!(room);
    }
    if let Some(name) = &args.name {
        start["name"] = json!(name);
    }
    if let Some(relay) = &args.relay {
        start["relay"] = json!(relay);
    }
    if args.action == "host" {
        start["players"] = json!(args.players);
        start["port"] = json!(args.port);
        start["discoverable"] = json!(true);
        // People mark ready in the game; the match starts once every seat is.
        start["auto_start"] = json!(true);
    } else if let Some(address) = &args.address {
        start["address"] = json!(address);
    }
    let started = call(&mut workspace, "net", &start)?;
    let session = started["session"].as_str().unwrap_or("s1").to_owned();
    if args.action == "host" {
        let port = started["network"]["port"].clone();
        let room = started["room_code"].clone();
        let through = match &args.relay {
            Some(relay) => format!(" and through the relay at {relay}"),
            None => String::new(),
        };
        say(
            as_json,
            &format!(
                "Hosting on UDP port {port}{through}, room code {}.",
                room.as_str().unwrap_or("?")
            ),
            json!({ "event": "hosting", "session": session, "port": port, "room_code": room, "relay": started["relay"] }),
        );
    } else {
        let to = match (&args.relay, &args.address) {
            (Some(relay), _) => format!(
                "room {} through the relay at {relay}",
                args.room.as_deref().unwrap_or("?")
            ),
            (None, address) => address.as_deref().unwrap_or("?").to_owned(),
        };
        say(
            as_json,
            &format!("Joining {to}..."),
            json!({ "event": "joining", "session": session }),
        );
    }
    let tick = Duration::from_micros(1_000_000 / RATE);
    let mut next = Instant::now();
    let mut last = String::new();
    let mut ticks: u64 = 0;
    let closed = loop {
        call(
            &mut workspace,
            "net",
            &json!({ "action": "step", "session": session, "frames": 1 }),
        )?;
        ticks += 1;
        let status = call(
            &mut workspace,
            "net",
            &json!({ "action": "status", "session": session }),
        )?;
        let line = lobby_line(&status);
        if line != last {
            say(
                as_json,
                &line,
                json!({ "event": "lobby", "tick": status["tick"], "lobby": status["lobby"], "seats": status["seats"], "relay": status["relay"] }),
            );
            last = line;
        }
        if status["lobby"]["phase"] == "Closed" {
            break true;
        }
        if args.frames.is_some_and(|frames| ticks >= frames) {
            break false;
        }
        next += tick;
        let now = Instant::now();
        match next.checked_duration_since(now) {
            Some(wait) => std::thread::sleep(wait),
            // Behind (a slow machine): go on from now instead of racing.
            None => next = now,
        }
    };
    let stopped = call(
        &mut workspace,
        "net",
        &json!({ "action": "stop", "session": session }),
    )?;
    Ok(if as_json {
        json!({ "ok": true, "ticks": ticks, "closed": closed, "stopped_at_tick": stopped["stopped_at_tick"] })
            .to_string()
    } else if closed {
        format!("The match closed after {ticks} tick(s).")
    } else {
        format!("Stopped after {ticks} tick(s).")
    })
}
