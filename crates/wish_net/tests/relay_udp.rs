//! A relay, a host and two clients in one process, over real UDP sockets on
//! this machine (127.0.0.1). The time comes from the test's own clock: the
//! crate reads none.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{code, config, message};
use wish_net::relay::{Relay, RelayClient, RelayConfig, RelayHost, RelayState, virtual_slot};
use wish_net::{Channel, Endpoint, Entropy, Event, PeerId, UdpTransport};

fn localhost() -> std::net::SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn a_relay_carries_a_match_over_localhost_udp() {
    let mut relay = Relay::new(
        UdpTransport::bind(localhost()).unwrap(),
        RelayConfig::default(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let relay_addr = relay.local_addr();
    let mut entropy = Entropy::os().unwrap();
    let mut host = Endpoint::host(
        RelayHost::new(
            UdpTransport::bind(localhost()).unwrap(),
            relay_addr,
            &code(),
            &mut entropy,
        ),
        config(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut clients: Vec<_> = (0..2)
        .map(|_| {
            Endpoint::connect(
                RelayClient::new(
                    UdpTransport::bind(localhost()).unwrap(),
                    relay_addr,
                    &code(),
                ),
                relay_addr,
                config(),
                Entropy::os().unwrap(),
            )
            .unwrap()
        })
        .collect();
    let start = Instant::now();
    let mut host_peers: Vec<PeerId> = Vec::new();
    let mut up = [false; 2];
    let mut sent = false;
    let mut to_host: Vec<(PeerId, Vec<u8>)> = Vec::new();
    let mut to_clients: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    let mut unreliable = [0usize; 2];
    let mut last_unreliable_ms = 0;
    const MESSAGES: u32 = 30;
    let size = |i: u32| 4 + (i as usize * 1931) % 9000;
    while start.elapsed() < Duration::from_secs(30) {
        let now = start.elapsed().as_millis() as u64;
        let read = relay.update(now);
        host.update(now);
        while let Some(event) = host.poll_event() {
            match event {
                Event::Connected { peer } => host_peers.push(peer),
                Event::Message {
                    peer,
                    channel: Channel::Reliable,
                    data,
                } => to_host.push((peer, data)),
                Event::Message { .. } => {}
                Event::Disconnected { .. } => panic!("host: {event:?}"),
            }
        }
        for (c, client) in clients.iter_mut().enumerate() {
            client.update(now);
            while let Some(event) = client.poll_event() {
                match event {
                    Event::Connected { peer } => {
                        assert_eq!(peer, PeerId::HOST);
                        up[c] = true;
                    }
                    Event::Message {
                        channel: Channel::Reliable,
                        data,
                        ..
                    } => to_clients[c].push(data),
                    Event::Message { .. } => unreliable[c] += 1,
                    Event::Disconnected { .. } => panic!("client {c}: {event:?}"),
                }
            }
        }
        if !sent && up.iter().all(|u| *u) && host_peers.len() == 2 {
            sent = true;
            for (c, client) in clients.iter_mut().enumerate() {
                let peer = client.local_id().unwrap();
                for i in 0..MESSAGES {
                    client
                        .send(
                            PeerId::HOST,
                            Channel::Reliable,
                            &message(i + 1000 * c as u32, size(i)),
                        )
                        .unwrap();
                    host.send(peer, Channel::Reliable, &message(i + 5000, size(i + 3)))
                        .unwrap();
                }
            }
        }
        let reliable_done = sent
            && to_host.len() == 2 * MESSAGES as usize
            && to_clients.iter().all(|got| got.len() == MESSAGES as usize);
        if reliable_done {
            if unreliable.iter().all(|n| *n > 0) {
                break;
            }
            // Unreliable state after the burst (a burst may overflow a
            // socket's buffer on this machine, and nothing resends it).
            if now >= last_unreliable_ms + 20 {
                last_unreliable_ms = now;
                for client in &clients {
                    let peer = client.local_id().unwrap();
                    host.send(peer, Channel::Unreliable, &message(7, 2000))
                        .unwrap();
                }
            }
        }
        if read == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert!(sent, "not everyone connected: {up:?} {host_peers:?}");
    assert_eq!(host.transport().relay_state(), RelayState::Ready);
    for client in &clients {
        assert_eq!(client.transport().relay_state(), RelayState::Ready);
    }
    // Every client reached the host through a slot of the relay.
    for peer in &host_peers {
        let addr = host.peer_addr(*peer).unwrap();
        assert!(virtual_slot(addr).is_some(), "{addr}");
    }
    for (c, client) in clients.iter().enumerate() {
        let peer = client.local_id().unwrap();
        let got: Vec<&Vec<u8>> = to_host
            .iter()
            .filter(|(p, _)| *p == peer)
            .map(|(_, d)| d)
            .collect();
        assert_eq!(got.len(), MESSAGES as usize, "to host from {c}");
        for (i, data) in got.iter().enumerate() {
            assert_eq!(**data, message(i as u32 + 1000 * c as u32, size(i as u32)));
        }
        for (i, data) in to_clients[c].iter().enumerate() {
            assert_eq!(*data, message(i as u32 + 5000, size(i as u32 + 3)));
        }
        assert_eq!(to_clients[c].len(), MESSAGES as usize, "to client {c}");
    }
    assert!(unreliable.iter().all(|n| *n > 0), "{unreliable:?}");
    let stats = relay.stats();
    assert_eq!(relay.rooms(), 1);
    assert_eq!(relay.clients(), 2);
    assert!(stats.forwarded > 100, "{stats:?}");
}
