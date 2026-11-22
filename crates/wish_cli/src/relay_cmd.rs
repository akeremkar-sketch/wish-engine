//! `wen relay`: the relay for online play over the internet (design note
//! `docs/design/1.1-relay.md`). A program for a server, not a capability of
//! a project: it touches no project and no file, so it is not a tool of the
//! control plane (as `wen mcp` is not).

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};
use wish_net::relay::{ClientLeft, Relay, RelayConfig, RelayEvent, RelayStats, RoomClosed};
use wish_net::{Entropy, Transport, UdpTransport};

use crate::args::RelayArgs;
use crate::project_cmds::Outcome;

/// How often a summary of the counts is printed.
const SUMMARY_EVERY: Duration = Duration::from_secs(60);

fn line(as_json: bool, value: Value, text: &str) {
    if as_json {
        println!("{value}");
    } else {
        println!("{text}");
    }
}

fn describe(event: &RelayEvent) -> (Value, String) {
    match event {
        RelayEvent::RoomOpened { room, host } => (
            json!({ "event": "room_opened", "room": room.to_string(), "host": host.to_string() }),
            format!("room {room} opened by {host}"),
        ),
        RelayEvent::RoomMoved { room, host } => (
            json!({ "event": "room_moved", "room": room.to_string(), "host": host.to_string() }),
            format!("room {room} moved to {host}"),
        ),
        RelayEvent::RoomClosed {
            room,
            reason,
            clients,
        } => {
            let why = match reason {
                RoomClosed::Idle => "idle",
                RoomClosed::ByHost => "by_host",
                RoomClosed::Replaced => "replaced",
            };
            (
                json!({ "event": "room_closed", "room": room.to_string(), "reason": why, "clients": clients }),
                format!("room {room} closed ({why}), {clients} players left with it"),
            )
        }
        RelayEvent::ClientAttached { room, slot, addr } => (
            json!({ "event": "player_joined", "room": room.to_string(), "slot": slot, "addr": addr.to_string() }),
            format!("room {room}: player {slot} joined from {addr}"),
        ),
        RelayEvent::ClientLeft { room, slot, reason } => {
            let why = match reason {
                ClientLeft::Idle => "idle",
                ClientLeft::Moved => "moved",
            };
            (
                json!({ "event": "player_left", "room": room.to_string(), "slot": slot, "reason": why }),
                format!("room {room}: player {slot} left ({why})"),
            )
        }
    }
}

fn summary(relay: &Relay<UdpTransport>, stats: &RelayStats) -> (Value, String) {
    (
        json!({
            "event": "summary",
            "rooms": relay.rooms(),
            "players": relay.clients(),
            "forwarded": stats.forwarded,
            "forwarded_bytes": stats.forwarded_bytes,
            "refused": stats.refused,
            "dropped_rate": stats.dropped_rate,
            "dropped_handshake_rate": stats.dropped_handshake_rate,
            "dropped_malformed": stats.dropped_malformed,
            "dropped_oversized": stats.dropped_oversized,
            "dropped_unknown": stats.dropped_unknown,
        }),
        format!(
            "{} rooms, {} players; {} datagrams carried ({} bytes); refused {}, over the rate {}, unanswered handshakes {}, malformed {}, oversized {}, from unknown addresses {}",
            relay.rooms(),
            relay.clients(),
            stats.forwarded,
            stats.forwarded_bytes,
            stats.refused,
            stats.dropped_rate,
            stats.dropped_handshake_rate,
            stats.dropped_malformed,
            stats.dropped_oversized,
            stats.dropped_unknown,
        ),
    )
}

/// Runs the relay until `--seconds` pass (or forever with 0).
pub fn relay(args: &RelayArgs, as_json: bool) -> Result<Outcome> {
    let ip: IpAddr = args.bind.parse().map_err(|_| {
        Diagnostic::error(
            "invalid_arguments",
            format!("--bind expects an IP address, but got '{}'.", args.bind),
            "Write it like --bind 0.0.0.0 (every IPv4 address of the server, the default) or --bind :: (every IPv6 address).",
        )
    })?;
    let config = RelayConfig {
        max_rooms: args.max_rooms,
        max_peers: args.max_peers,
        max_clients: args.max_clients,
        ..RelayConfig::default()
    };
    let transport = UdpTransport::bind(SocketAddr::new(ip, args.port))?;
    let mut relay = Relay::new(transport, config, Entropy::os()?)?;
    let addr = relay.transport().local_addr();
    line(
        as_json,
        json!({
            "event": "listening",
            "addr": addr.to_string(),
            "max_rooms": config.max_rooms,
            "max_peers": config.max_peers,
            "max_clients": config.max_clients,
        }),
        &format!(
            "Relay listening on {addr} (UDP): up to {} rooms, {} players a room, {} in all.\n\
             Open UDP port {} in the server's firewall; players need this server's address and the room code.\n\
             The relay carries sealed packets it cannot read, stores nothing and logs no payloads. Stop it with Ctrl+C.",
            config.max_rooms,
            config.max_peers,
            config.max_clients,
            addr.port()
        ),
    );
    let started = Instant::now();
    let mut last_summary = Instant::now();
    while args.seconds == 0 || started.elapsed() < Duration::from_secs(args.seconds) {
        let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let read = relay.update(now);
        while let Some(event) = relay.poll_event() {
            let (value, text) = describe(&event);
            line(as_json, value, &text);
        }
        if last_summary.elapsed() >= SUMMARY_EVERY {
            last_summary = Instant::now();
            let (value, text) = summary(&relay, &relay.stats());
            line(as_json, value, &text);
        }
        if read == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let stats = relay.stats();
    let (value, text) = summary(&relay, &stats);
    Ok(Outcome {
        text: if as_json {
            json!({ "ok": true, "stopped": value }).to_string()
        } else {
            format!("Relay stopped: {text}.")
        },
        success: true,
    })
}
