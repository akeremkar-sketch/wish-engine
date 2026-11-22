//! `wen relay` as its own process: a host and a client in this process
//! play through it over localhost UDP, and its log names the room and the
//! player without any payload.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use wish_net::relay::{RelayClient, RelayHost};
use wish_net::{Channel, Config, Endpoint, Entropy, Event, PeerId, RoomCode, UdpTransport};

#[test]
fn a_host_and_a_player_meet_through_wen_relay() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wen"))
        .args([
            "relay",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--seconds",
            "8",
            "--json",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("wen relay starts");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(first["event"], "listening");
    let relay_addr: SocketAddr = first["addr"].as_str().unwrap().parse().unwrap();

    let code = RoomCode::parse("STAR42").unwrap();
    let local: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let mut entropy = Entropy::os().unwrap();
    let mut host = Endpoint::host(
        RelayHost::new(
            UdpTransport::bind(local).unwrap(),
            relay_addr,
            &code,
            &mut entropy,
        ),
        Config::new(code.clone()),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut client = Endpoint::connect(
        RelayClient::new(UdpTransport::bind(local).unwrap(), relay_addr, &code),
        relay_addr,
        Config::new(code.clone()),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let start = Instant::now();
    let mut connected = false;
    let mut sent = false;
    let mut got = None;
    while start.elapsed() < Duration::from_secs(6) && got.is_none() {
        let now = start.elapsed().as_millis() as u64;
        host.update(now);
        client.update(now);
        while let Some(event) = host.poll_event() {
            if let Event::Message { data, .. } = event {
                got = Some(data);
            }
        }
        while let Some(event) = client.poll_event() {
            connected |= matches!(event, Event::Connected { peer: PeerId::HOST });
        }
        if connected && !sent {
            client
                .send(
                    PeerId::HOST,
                    Channel::Reliable,
                    b"PAYLOAD-THAT-IS-NEVER-LOGGED",
                )
                .unwrap();
            sent = true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(got.as_deref(), Some(&b"PAYLOAD-THAT-IS-NEVER-LOGGED"[..]));
    let rest: Vec<String> = lines.map(|l| l.unwrap()).collect();
    assert!(child.wait().unwrap().success());
    let events: Vec<Value> = rest
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
    assert!(kinds.contains(&"room_opened"), "{rest:?}");
    assert!(kinds.contains(&"player_joined"), "{rest:?}");
    let last = events.last().unwrap();
    assert_eq!(last["ok"], true);
    assert!(last["stopped"]["forwarded"].as_u64().unwrap() > 0);
    for l in &rest {
        assert!(!l.contains("PAYLOAD"), "{l}");
        assert!(!l.contains("STAR42"), "{l}");
    }
}
