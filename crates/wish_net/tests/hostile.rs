//! Hostile peers (invariant 7 applied to the network): tampered, replayed,
//! forged, malformed, oversized and flooding packets of every kind, and a
//! peer that holds real keys and sends random frames. No panic, no freeze,
//! and the honest connection goes on.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::net::SocketAddr;

use common::*;
use wish_core::Rng;
use wish_net::crypto::{CONNECT_NUMBER, KeyPair, SessionKeys};
use wish_net::endpoint::MAX_READS_PER_UPDATE;
use wish_net::wire::{self, DATA_HEADER_LEN, FRAGMENT_LEN, Frame, MAX_PLAINTEXT, Packet};
use wish_net::{
    Channel, Endpoint, Entropy, Event, LinkConditions, PeerId, Reason, RoomCode, SimNetwork,
    SimTransport, Transport,
};

fn is_data(bytes: &[u8]) -> bool {
    matches!(wire::decode(bytes), Some(Packet::Data { .. }))
}

fn assert_no_disconnect(events: &[Vec<Event>]) {
    for (side, events) in events.iter().enumerate() {
        for event in events {
            assert!(
                !matches!(event, Event::Disconnected { .. }),
                "side {side}: {event:?}"
            );
        }
    }
}

#[test]
fn a_tampered_or_replayed_packet_is_dropped_and_the_connection_goes_on() {
    let (net, mut host, mut client) = connected_pair(1, LinkConditions::PERFECT);
    let (client_addr, host_addr) = (client.local_addr(), host.local_addr());
    // An attacker on the path takes the client's packet: it never arrives.
    net.start_capture();
    net.set_link(
        client_addr,
        host_addr,
        LinkConditions {
            loss: 1.0,
            ..LinkConditions::PERFECT
        },
    );
    client
        .send(PeerId::HOST, Channel::Reliable, b"first")
        .unwrap();
    assert_no_disconnect(&tick(&net, &mut [&mut host, &mut client]));
    net.set_link(client_addr, host_addr, LinkConditions::PERFECT);
    let packet = net
        .take_captured()
        .into_iter()
        .find(|c| c.from == client_addr && c.to == host_addr && is_data(&c.bytes))
        .unwrap()
        .bytes;
    let before = host.stats();
    // A changed connection id, packet number, body and tag.
    for at in [5, 9, DATA_HEADER_LEN + 1, packet.len() - 1] {
        let mut bad = packet.clone();
        bad[at] ^= 0x10;
        net.inject(client_addr, host_addr, &bad);
    }
    let events = tick(&net, &mut [&mut host, &mut client]);
    assert_no_disconnect(&events);
    assert!(messages(&events[0], Channel::Reliable).is_empty());
    let after = host.stats();
    assert_eq!(after.dropped_unknown - before.dropped_unknown, 1);
    assert_eq!(after.dropped_unauthentic - before.dropped_unauthentic, 3);
    // The packet as it was opens (a tampered copy leaves no mark), once.
    net.inject(client_addr, host_addr, &packet);
    let events = tick(&net, &mut [&mut host, &mut client]);
    assert_eq!(
        messages(&events[0], Channel::Reliable),
        vec![b"first".to_vec()]
    );
    net.inject(client_addr, host_addr, &packet);
    let events = tick(&net, &mut [&mut host, &mut client]);
    assert_no_disconnect(&events);
    assert!(messages(&events[0], Channel::Reliable).is_empty());
    assert_eq!(host.stats().dropped_replayed - after.dropped_replayed, 1);
    // The connection still works.
    client
        .send(PeerId::HOST, Channel::Reliable, b"second")
        .unwrap();
    let mut got = Vec::new();
    for _ in 0..5 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        assert_no_disconnect(&events);
        got.extend(messages(&events[0], Channel::Reliable));
    }
    assert!(got.contains(&b"second".to_vec()), "{got:?}");
}

#[test]
fn a_packet_sealed_with_another_key_is_dropped() {
    let (net, mut host, mut client) = connected_pair(2, LinkConditions::PERFECT);
    let mut other = Endpoint::connect(
        net.add_node(),
        host.local_addr(),
        config(),
        Entropy::seeded(3),
    )
    .unwrap();
    for _ in 0..10 {
        tick(&net, &mut [&mut host, &mut client, &mut other]);
    }
    assert_eq!(host.peers().len(), 2);
    net.start_capture();
    other.send(PeerId::HOST, Channel::Reliable, b"x").unwrap();
    tick(&net, &mut [&mut host, &mut client, &mut other]);
    tick(&net, &mut [&mut host, &mut client, &mut other]);
    let (other_addr, client_addr, host_addr) =
        (other.local_addr(), client.local_addr(), host.local_addr());
    let mut packet = net
        .take_captured()
        .into_iter()
        .find(|c| c.from == other_addr && c.to == host_addr && is_data(&c.bytes))
        .unwrap()
        .bytes;
    // Sent as if from the first client, with its connection id.
    let client_id = client.local_id().unwrap().0;
    packet[4..8].copy_from_slice(&client_id.to_le_bytes());
    packet[8..16].copy_from_slice(&1_000u64.to_le_bytes());
    let before = host.stats();
    net.inject(client_addr, host_addr, &packet);
    let events = tick(&net, &mut [&mut host, &mut client, &mut other]);
    assert_no_disconnect(&events);
    assert_eq!(
        host.stats().dropped_unauthentic - before.dropped_unauthentic,
        1
    );
}

/// Samples of every packet kind, taken from a real exchange.
fn samples() -> Vec<Vec<u8>> {
    let net = SimNetwork::new(5, 16);
    net.start_capture();
    let mut host = Endpoint::host(net.add_node(), config(), Entropy::seeded(6)).unwrap();
    let mut client = Endpoint::connect(
        net.add_node(),
        host.local_addr(),
        config(),
        Entropy::seeded(7),
    )
    .unwrap();
    let mut wrong_config = config();
    wrong_config.room_code = RoomCode::parse("ZZZZ").unwrap();
    let mut wrong = Endpoint::connect(
        net.add_node(),
        host.local_addr(),
        wrong_config,
        Entropy::seeded(8),
    )
    .unwrap();
    for i in 0..60u32 {
        if !host.peers().is_empty() && !client.peers().is_empty() {
            let peer = host.peers()[0];
            host.send(peer, Channel::Reliable, &message(i, 2500))
                .unwrap();
            host.send(peer, Channel::Unreliable, &message(i, 1500))
                .unwrap();
            client
                .send(PeerId::HOST, Channel::Reliable, &message(i, 40))
                .unwrap();
        }
        tick(&net, &mut [&mut host, &mut client, &mut wrong]);
    }
    let mut out: Vec<Vec<u8>> = net.take_captured().into_iter().map(|c| c.bytes).collect();
    let mut other_version = wire::hello(&[1; 32], &[2; 16]);
    other_version[2] = 2;
    out.push(other_version);
    out.push(wire::query(&[3; 16], "station"));
    out.push(wire::announce(&wire::AnnounceFields {
        nonce: &[3; 16],
        game: "station",
        name: "room",
        port: 4000,
        players: 1,
        max_players: 8,
        host_key: &[4; 32],
        room_code: "STAR42",
    }));
    let kinds: std::collections::BTreeSet<u8> = out.iter().map(|p| p[3]).collect();
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 16, 17],
        "every packet kind is sampled"
    );
    out
}

fn mutate(rng: &mut Rng, sample: &[u8]) -> Vec<u8> {
    let mut bytes = sample.to_vec();
    if bytes.is_empty() {
        bytes.push(rng.next_u32() as u8);
    }
    match rng.below(9) {
        0 => {}
        1 => {
            let at = rng.below(bytes.len() as u32) as usize;
            bytes[at] ^= 1 << rng.below(8);
        }
        2 => {
            for _ in 0..1 + rng.below(16) {
                let at = rng.below(bytes.len() as u32) as usize;
                bytes[at] = rng.next_u32() as u8;
            }
        }
        3 => bytes.truncate(rng.below(bytes.len() as u32) as usize),
        4 => bytes.extend((0..1 + rng.below(200)).map(|_| rng.next_u32() as u8)),
        5 => bytes = (0..rng.below(1400)).map(|_| rng.next_u32() as u8).collect(),
        6 if bytes.len() > 3 => bytes[3] = rng.below(20) as u8,
        7 if bytes.len() > 2 => bytes[2] = rng.next_u32() as u8,
        _ => bytes.resize(1200 + rng.below(3000) as usize, 0xEE),
    }
    bytes
}

#[test]
fn fuzzed_packets_of_every_kind_never_hurt_an_endpoint() {
    let samples = samples();
    let (net, mut host, mut client) = connected_pair(11, LinkConditions::PERFECT);
    let attacker = net.add_node();
    let (host_addr, client_addr) = (host.local_addr(), client.local_addr());
    let mut rng = Rng::from_seed(99);
    let mut got = Vec::new();
    // Packets of this very connection, replayed and changed.
    net.start_capture();
    let mut pool: Vec<Vec<u8>> = Vec::new();
    for round in 0..400u32 {
        client
            .send(PeerId::HOST, Channel::Reliable, &message(round, 64))
            .unwrap();
        for captured in net.take_captured() {
            if pool.len() < 4096 {
                pool.push(captured.bytes);
            }
        }
        for _ in 0..100 {
            let sample = if !pool.is_empty() && rng.chance(0.25) {
                &pool[rng.below(pool.len() as u32) as usize]
            } else {
                &samples[rng.below(samples.len() as u32) as usize]
            };
            let bytes = mutate(&mut rng, sample);
            let random: SocketAddr = SocketAddr::new(
                std::net::Ipv4Addr::from(rng.next_u32()).into(),
                rng.below(65_535) as u16 + 1,
            );
            let from = match rng.below(4) {
                0 => attacker.local_addr(),
                1 => client_addr,
                2 => host_addr,
                _ => random,
            };
            let to = if rng.chance(0.5) {
                host_addr
            } else {
                client_addr
            };
            net.inject(from, to, &bytes);
        }
        let events = tick(&net, &mut [&mut host, &mut client]);
        assert_no_disconnect(&events);
        got.extend(messages(&events[0], Channel::Reliable));
    }
    for _ in 0..20 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        got.extend(messages(&events[0], Channel::Reliable));
    }
    let order: Vec<u32> = got.iter().map(|d| index_of(d)).collect();
    assert_eq!(order, (0..400).collect::<Vec<_>>());
    let (h, c) = (host.stats(), client.stats());
    for stats in [h, c] {
        assert!(stats.dropped_malformed > 0, "{stats:?}");
        assert!(stats.dropped_unknown > 0, "{stats:?}");
        assert!(stats.dropped_oversized > 0, "{stats:?}");
    }
    assert!(h.dropped_replayed + c.dropped_replayed > 0);
    assert!(h.dropped_unauthentic + c.dropped_unauthentic > 0);
    assert!(h.handshakes_refused > 0, "{h:?}");
}

/// A client written by hand from the public pieces: it holds real keys, so
/// what it seals opens, and its frames are whatever it likes.
struct Rogue {
    node: SimTransport,
    host: SocketAddr,
    keys: Option<SessionKeys>,
    id: u32,
    number: u64,
}

impl Rogue {
    /// Joins, ticking the host and the honest client; their events go to
    /// `seen`.
    fn join(
        net: &SimNetwork,
        host: &mut SimEndpoint,
        honest: &mut SimEndpoint,
        seed: u64,
        seen: &mut Vec<Vec<Event>>,
    ) -> Self {
        let mut node = net.add_node();
        let host_addr = host.local_addr();
        let mut entropy = Entropy::seeded(seed);
        let pair = KeyPair::generate(&mut entropy);
        let token: [u8; 16] = entropy.bytes();
        node.send(host_addr, &wire::hello(&pair.public(), &token))
            .unwrap();
        let mut rogue = Self {
            node,
            host: host_addr,
            keys: None,
            id: 0,
            number: 0,
        };
        let mut buf = vec![0u8; 2048];
        for _ in 0..10 {
            seen.extend(tick(net, &mut [&mut *host, &mut *honest]));
            while let wish_net::Recv::Datagram { len, .. } = rogue.node.recv(&mut buf) {
                match wire::decode(&buf[..len]) {
                    Some(Packet::Challenge {
                        cookie, host_key, ..
                    }) => {
                        let shared = pair.agree(&host_key).unwrap();
                        let keys = SessionKeys::client(&shared, &pair.public(), &host_key);
                        let mut packet = wire::connect_header(&pair.public(), &token, &cookie);
                        let mut body = b"STAR42".to_vec();
                        keys.seal(CONNECT_NUMBER, &packet, &mut body).unwrap();
                        packet.extend_from_slice(&body);
                        rogue.node.send(host_addr, &packet).unwrap();
                        rogue.keys = Some(keys);
                    }
                    Some(Packet::Data { connection, .. }) => rogue.id = connection,
                    _ => {}
                }
            }
            if rogue.id != 0 {
                return rogue;
            }
        }
        panic!("the rogue did not get in");
    }

    fn send_plain(&mut self, text: &[u8]) {
        let number = self.number;
        self.number += 1;
        let header = wire::data_header(self.id, number);
        let mut body = text.to_vec();
        self.keys
            .as_ref()
            .unwrap()
            .seal(number, &header, &mut body)
            .unwrap();
        let mut packet = header.to_vec();
        packet.extend_from_slice(&body);
        self.node.send(self.host, &packet).unwrap();
    }
}

fn random_frames(rng: &mut Rng) -> Vec<u8> {
    let mut out = Vec::new();
    if rng.chance(0.2) {
        return (0..rng.below(MAX_PLAINTEXT as u32 + 1))
            .map(|_| rng.next_u32() as u8)
            .collect();
    }
    let payload: Vec<u8> = (0..FRAGMENT_LEN).map(|i| i as u8).collect();
    for _ in 0..1 + rng.below(12) {
        let small = |rng: &mut Rng| match rng.below(3) {
            0 => rng.below(4),
            1 => rng.below(600),
            _ => rng.next_u32(),
        };
        let count = match rng.below(3) {
            0 => 1,
            1 => 1 + rng.below(80) as u16,
            _ => rng.next_u32() as u16 | 1,
        };
        let index = rng.below(u32::from(count)) as u16;
        let len = rng.below(400) as usize;
        let frame = match rng.below(7) {
            0 => Frame::Welcome { peer: small(rng) },
            1 => Frame::Ack {
                next: small(rng),
                mask: rng.next_u64(),
            },
            2 => Frame::Reliable {
                seq: small(rng),
                index,
                count,
                bytes: &payload[..len],
            },
            3 => Frame::Unreliable {
                id: small(rng),
                index,
                count,
                bytes: &payload[..len],
            },
            4 => Frame::Ping {
                time: rng.next_u64(),
            },
            5 => Frame::Pong {
                time: rng.next_u64(),
            },
            _ => Frame::Ack {
                next: u32::MAX - rng.below(3),
                mask: u64::MAX,
            },
        };
        if out.len() + frame.encoded_len() > MAX_PLAINTEXT {
            break;
        }
        frame.encode(&mut out);
    }
    out
}

#[test]
fn a_peer_with_real_keys_and_random_frames_cannot_hurt_the_host() {
    let mut host_config = config();
    host_config.max_peers = 64;
    let (net, mut host, mut client) =
        connected_pair_with(12, LinkConditions::PERFECT, host_config, config());
    let honest = host.peers()[0];
    let mut rng = Rng::from_seed(5);
    let mut violations = 0;
    let mut got = Vec::new();
    let mut sent = 0u32;
    // Events come per tick as [host, client].
    let mut absorb = |ticks: &[Vec<Event>], got: &mut Vec<u32>| {
        for pair in ticks.chunks(2) {
            for event in &pair[0] {
                match event {
                    Event::Disconnected { peer, reason } => {
                        assert_ne!(*peer, honest, "{event:?}");
                        if *reason == Reason::ProtocolViolation {
                            violations += 1;
                        }
                    }
                    Event::Message { peer, data, .. } if *peer == honest => {
                        got.push(index_of(data));
                    }
                    _ => {}
                }
            }
            assert_no_disconnect(&pair[1..]);
        }
    };
    for session in 0..40u64 {
        let mut seen = Vec::new();
        let mut rogue = Rogue::join(&net, &mut host, &mut client, 1000 + session, &mut seen);
        absorb(&seen, &mut got);
        for _ in 0..60 {
            for _ in 0..5 {
                let frames = random_frames(&mut rng);
                rogue.send_plain(&frames);
            }
            client
                .send(PeerId::HOST, Channel::Reliable, &message(sent, 32))
                .unwrap();
            sent += 1;
            let events = tick(&net, &mut [&mut host, &mut client]);
            absorb(&events, &mut got);
        }
    }
    for _ in 0..20 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        absorb(&events, &mut got);
    }
    assert_eq!(got, (0..sent).collect::<Vec<_>>());
    assert!(violations > 20, "{violations}");
    assert!(host.peers().contains(&honest));
}

#[test]
fn rule_breaking_frames_end_the_rogue_connection_only() {
    let cases: Vec<(&str, Vec<u8>)> = {
        let mut cases = Vec::new();
        let mut frame = |name: &'static str, frames: &[Frame<'_>]| {
            let mut out = Vec::new();
            for f in frames {
                f.encode(&mut out);
            }
            cases.push((name, out));
        };
        let chunk = vec![1u8; FRAGMENT_LEN];
        frame(
            "a fragment count above the message limit",
            &[Frame::Reliable {
                seq: 0,
                index: 0,
                count: 1000,
                bytes: &chunk,
            }],
        );
        frame(
            "a message that does not start at index 0",
            &[Frame::Reliable {
                seq: 0,
                index: 1,
                count: 2,
                bytes: &chunk,
            }],
        );
        frame(
            "a fragment whose count changes inside its message",
            &[
                Frame::Reliable {
                    seq: 0,
                    index: 0,
                    count: 3,
                    bytes: &chunk[..10],
                },
                Frame::Reliable {
                    seq: 1,
                    index: 1,
                    count: 4,
                    bytes: &chunk[..10],
                },
            ],
        );
        frame(
            "an unreliable message whose count changes",
            &[
                Frame::Unreliable {
                    id: 4,
                    index: 0,
                    count: 3,
                    bytes: &chunk[..10],
                },
                Frame::Unreliable {
                    id: 4,
                    index: 1,
                    count: 5,
                    bytes: &chunk[..10],
                },
            ],
        );
        frame("a welcome sent to the host", &[Frame::Welcome { peer: 1 }]);
        cases.push(("bytes that are not frames", vec![200, 1, 2, 3]));
        cases
    };
    let (net, mut host, mut client) = connected_pair(13, LinkConditions::PERFECT);
    for (i, (name, text)) in cases.iter().enumerate() {
        let mut rogue = Rogue::join(&net, &mut host, &mut client, 50 + i as u64, &mut Vec::new());
        let rogue_peer = PeerId(rogue.id);
        rogue.send_plain(text);
        let events = tick(&net, &mut [&mut host, &mut client]);
        assert!(
            events[0].contains(&Event::Disconnected {
                peer: rogue_peer,
                reason: Reason::ProtocolViolation
            }),
            "{name}: {:?}",
            events[0]
        );
        assert_no_disconnect(&events[1..]);
    }
    // Fragments far ahead of the window are dropped, not kept.
    let mut rogue = Rogue::join(&net, &mut host, &mut client, 99, &mut Vec::new());
    let before = host.stats().dropped_window;
    for k in 0..50u32 {
        let mut out = Vec::new();
        Frame::Reliable {
            seq: 10_000 + k,
            index: 0,
            count: 1,
            bytes: &[1; 100],
        }
        .encode(&mut out);
        rogue.send_plain(&out);
    }
    let events = tick(&net, &mut [&mut host, &mut client]);
    assert_no_disconnect(&events);
    assert_eq!(host.stats().dropped_window - before, 50);
    assert_eq!(host.peers().len(), 2);
}

#[test]
fn a_flood_is_read_a_bounded_amount_per_update() {
    let (net, mut host, mut client) = connected_pair(14, LinkConditions::PERFECT);
    let host_addr = host.local_addr();
    let mut rng = Rng::from_seed(1);
    for _ in 0..3 {
        for _ in 0..20_000 {
            let from = SocketAddr::new(
                std::net::Ipv4Addr::from(rng.next_u32()).into(),
                1 + rng.below(60_000) as u16,
            );
            let packet = wire::hello(&[rng.next_u32() as u8; 32], &[7; 16]);
            net.inject(from, host_addr, &packet);
        }
        let before = host.stats().packets_received;
        let events = tick(&net, &mut [&mut host, &mut client]);
        assert_no_disconnect(&events);
        let read = host.stats().packets_received - before;
        assert!(read as usize <= MAX_READS_PER_UPDATE, "{read}");
        // Most handshakes are over the rate limit and get no answer.
        assert!(host.stats().dropped_rate > 0);
    }
    client
        .send(PeerId::HOST, Channel::Reliable, b"still here")
        .unwrap();
    let mut got = Vec::new();
    for _ in 0..10 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        got.extend(messages(&events[0], Channel::Reliable));
    }
    assert!(got.contains(&b"still here".to_vec()));
}

#[test]
fn a_client_of_another_version_is_told_why() {
    let net = SimNetwork::new(1, 16);
    let mut host = Endpoint::host(net.add_node(), config(), Entropy::seeded(1)).unwrap();
    let mut old = net.add_node();
    let mut hello = wire::hello(&[5; 32], &[6; 16]);
    hello[2] = wire::VERSION + 1;
    old.send(host.local_addr(), &hello).unwrap();
    for _ in 0..3 {
        net.step();
        host.update(net.now_ms());
    }
    let mut buf = [0u8; 2048];
    let wish_net::Recv::Datagram { len, .. } = old.recv(&mut buf) else {
        panic!("no answer");
    };
    assert_eq!(
        wire::decode(&buf[..len]),
        Some(Packet::Reject {
            token: [6; 16],
            reason: wire::RejectReason::VersionMismatch
        })
    );
}
