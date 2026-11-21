//! Two endpoints over real UDP on this machine (127.0.0.1), and discovery
//! answering a query sent to it. The time comes from the test's own clock:
//! the crate reads none.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{config, index_of, message};
use wish_net::discovery::{Announce, DiscoveryQuery, DiscoveryResponder};
use wish_net::{Channel, Endpoint, Entropy, Event, PeerId, UdpTransport};

fn localhost() -> std::net::SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn a_host_and_a_client_talk_over_localhost_udp() {
    let mut host = Endpoint::host(
        UdpTransport::bind(localhost()).unwrap(),
        config(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut client = Endpoint::connect(
        UdpTransport::bind(localhost()).unwrap(),
        host.local_addr(),
        config(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let start = Instant::now();
    let mut peer = None;
    let mut client_connected = false;
    let mut to_host = Vec::new();
    let mut to_client = Vec::new();
    let mut unreliable = 0;
    let mut sent = false;
    while start.elapsed() < Duration::from_secs(20) {
        let now = start.elapsed().as_millis() as u64;
        host.update(now);
        client.update(now);
        while let Some(event) = host.poll_event() {
            match event {
                Event::Connected { peer: p } => peer = Some(p),
                Event::Message { data, .. } => to_host.push(data),
                Event::Disconnected { .. } => panic!("host: {event:?}"),
            }
        }
        while let Some(event) = client.poll_event() {
            match event {
                Event::Connected { peer } => {
                    assert_eq!(peer, PeerId::HOST);
                    client_connected = true;
                }
                Event::Message {
                    channel: Channel::Reliable,
                    data,
                    ..
                } => to_client.push(data),
                Event::Message { .. } => unreliable += 1,
                Event::Disconnected { .. } => panic!("client: {event:?}"),
            }
        }
        if let (Some(peer), true, false) = (peer, client_connected, sent) {
            for i in 0..50u32 {
                let len = 1 + (i as usize * 997) % 20_000;
                host.send(peer, Channel::Reliable, &message(i, len.max(4)))
                    .unwrap();
                client
                    .send(PeerId::HOST, Channel::Reliable, &message(i, 100))
                    .unwrap();
            }
            for i in 0..10u32 {
                host.send(peer, Channel::Unreliable, &message(i, 3000))
                    .unwrap();
            }
            sent = true;
        }
        if to_host.len() == 50 && to_client.len() == 50 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(client_connected, "the client never connected");
    assert_eq!(
        to_host.iter().map(|d| index_of(d)).collect::<Vec<_>>(),
        (0..50).collect::<Vec<_>>()
    );
    for (i, data) in to_client.iter().enumerate() {
        let len = (1 + (i * 997) % 20_000).max(4);
        assert_eq!(data, &message(i as u32, len));
    }
    // Over loopback nothing is lost, but the test does not depend on it.
    assert!(unreliable <= 10);
    assert!(client.rtt_ms(PeerId::HOST).is_some());
    client.close();
    assert!(client.is_closed());
}

#[test]
fn discovery_answers_a_query_over_udp() {
    let host_key = [3u8; 32];
    let mut responder = DiscoveryResponder::new(
        UdpTransport::bind(localhost()).unwrap(),
        Announce {
            game: "station".to_owned(),
            name: "Test room".to_owned(),
            port: 7777,
            players: 2,
            max_players: 8,
            host_key,
            room_code: None,
        },
    )
    .unwrap();
    let mut transport = UdpTransport::bind(localhost()).unwrap();
    transport.enable_broadcast().unwrap();
    let mut query = DiscoveryQuery::new(transport, "station", &mut Entropy::os().unwrap()).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        query.send(responder.local_addr()).unwrap();
        for _ in 0..20 {
            responder.update(start.elapsed().as_millis() as u64);
            query.update();
            std::thread::sleep(Duration::from_millis(1));
        }
        if query.found().next().is_some() {
            break;
        }
    }
    let found: Vec<_> = query.found().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "127.0.0.1:7777".parse().unwrap());
    assert_eq!(found[0].1.name, "Test room");
    assert_eq!(found[0].1.host_key, host_key);
}

#[test]
fn a_port_in_use_is_reported() {
    let first = UdpTransport::bind(localhost()).unwrap();
    let taken = wish_net::Transport::local_addr(&first);
    let error = UdpTransport::bind(taken).unwrap_err();
    assert_eq!(error.code, "net_bind_failed");
    assert!(!error.hint.is_empty());
}
