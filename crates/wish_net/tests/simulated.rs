//! Endpoints over the deterministic network simulator: delivery under a bad
//! network, the same seed giving the same bytes, and every way a connection
//! can fail or end.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use wish_core::{Hash128, Rng};
use wish_net::discovery::{Announce, DiscoveryQuery, DiscoveryResponder, broadcast_addr};
use wish_net::{
    Channel, Endpoint, Entropy, Event, LinkConditions, PeerId, Reason, RoomCode, SimNetwork,
    Transport,
};

const MESSAGES: u32 = 1000;

struct Run {
    hash: Hash128,
    to_host: Vec<Vec<u8>>,
    to_client: Vec<Vec<u8>>,
    unreliable: Vec<Vec<u8>>,
    resent: u64,
}

/// 1000 reliable messages of 1 byte to 10 KiB each way, and one unreliable
/// message a tick from the host, over `conditions`.
fn exchange(seed: u64, conditions: LinkConditions) -> Run {
    let (net, mut host, mut client) = connected_pair(seed, conditions);
    let peer = host.peers()[0];
    let mut rng = Rng::from_seed(seed);
    let sizes: Vec<usize> = (0..MESSAGES)
        .map(|i| match i % 10 {
            0 => 1 + rng.below(10 * 1024) as usize,
            1 => 1,
            _ => 4 + rng.below(1500) as usize,
        })
        .collect();
    let (mut sent_up, mut sent_down) = (0u32, 0u32);
    let mut to_host = Vec::new();
    let mut to_client = Vec::new();
    let mut unreliable = Vec::new();
    let mut unreliable_sent = 0u32;
    for _ in 0..40_000 {
        // A few messages a tick from each side, as long as the queue takes
        // them.
        for _ in 0..4 {
            if sent_up < MESSAGES {
                let data = message(sent_up, sizes[sent_up as usize]);
                if client
                    .send(
                        PeerId::HOST,
                        Channel::Reliable,
                        &data[..sizes[sent_up as usize]],
                    )
                    .is_ok()
                {
                    sent_up += 1;
                }
            }
            if sent_down < MESSAGES {
                let data = message(sent_down + 100_000, sizes[sent_down as usize]);
                if host
                    .send(peer, Channel::Reliable, &data[..sizes[sent_down as usize]])
                    .is_ok()
                {
                    sent_down += 1;
                }
            }
        }
        if unreliable_sent < 500 {
            let len = 4 + (unreliable_sent as usize * 37) % 3000;
            host.send(
                peer,
                Channel::Unreliable,
                &message(unreliable_sent + 500_000, len),
            )
            .unwrap();
            unreliable_sent += 1;
        }
        let events = tick(&net, &mut [&mut host, &mut client]);
        for event in &events[0] {
            assert!(
                !matches!(event, Event::Disconnected { .. }),
                "host: {event:?}"
            );
        }
        for event in &events[1] {
            assert!(
                !matches!(event, Event::Disconnected { .. }),
                "client: {event:?}"
            );
        }
        to_host.extend(messages(&events[0], Channel::Reliable));
        to_client.extend(messages(&events[1], Channel::Reliable));
        unreliable.extend(messages(&events[1], Channel::Unreliable));
        if to_host.len() == MESSAGES as usize && to_client.len() == MESSAGES as usize {
            break;
        }
    }
    let resent = host.stats().resent + client.stats().resent;
    // Every reliable message was sent exactly as made: check them.
    for (i, data) in to_host.iter().enumerate() {
        let len = sizes[i];
        assert_eq!(data, &message(i as u32, len)[..len], "to host #{i}");
    }
    for (i, data) in to_client.iter().enumerate() {
        let len = sizes[i];
        assert_eq!(
            data,
            &message(i as u32 + 100_000, len)[..len],
            "to client #{i}"
        );
    }
    Run {
        hash: net.delivery_hash(),
        to_host,
        to_client,
        unreliable,
        resent,
    }
}

#[test]
fn reliable_messages_arrive_whole_and_in_order_over_a_bad_network() {
    let run = exchange(7, LinkConditions::BAD);
    assert_eq!(run.to_host.len(), MESSAGES as usize);
    assert_eq!(run.to_client.len(), MESSAGES as usize);
    assert!(run.resent > 0, "5 % loss must have made resends");
    // Unreliable messages arrive at most once, whole; most of them arrive.
    let mut seen = std::collections::BTreeSet::new();
    for data in &run.unreliable {
        let index = index_of(data);
        assert!(seen.insert(index), "unreliable #{index} arrived twice");
        let i = index - 500_000;
        let len = 4 + (i as usize * 37) % 3000;
        assert_eq!(data, &message(index, len), "unreliable #{i} is not whole");
    }
    assert!(
        seen.len() > 400 && seen.len() < 500,
        "{} of 500 unreliable messages arrived",
        seen.len()
    );
}

#[test]
fn the_same_seed_gives_the_same_delivery_byte_for_byte() {
    let first = exchange(21, LinkConditions::BAD);
    let second = exchange(21, LinkConditions::BAD);
    assert_eq!(first.hash, second.hash);
    assert_eq!(first.to_host, second.to_host);
    assert_eq!(first.to_client, second.to_client);
    assert_eq!(first.unreliable, second.unreliable);
    assert_eq!(first.resent, second.resent);
    let other = exchange(22, LinkConditions::BAD);
    assert_ne!(first.hash, other.hash);
}

#[test]
fn heavy_loss_and_reordering_still_deliver_in_order() {
    let conditions = LinkConditions {
        latency_ms: 40,
        jitter_ms: 80,
        loss: 0.2,
        duplicate: 0.1,
        reorder: 0.3,
    };
    let run = exchange(3, conditions);
    assert_eq!(run.to_host.len(), MESSAGES as usize);
    assert_eq!(run.to_client.len(), MESSAGES as usize);
}

#[test]
fn the_largest_message_and_an_empty_one_arrive() {
    let (net, mut host, mut client) = connected_pair(5, LinkConditions::BAD);
    let peer = host.peers()[0];
    let big = message(1, host.config().max_message_size);
    host.send(peer, Channel::Reliable, &big).unwrap();
    host.send(peer, Channel::Reliable, &[]).unwrap();
    host.send(peer, Channel::Unreliable, &[]).unwrap();
    let too_big = vec![0u8; host.config().max_message_size + 1];
    let error = host.send(peer, Channel::Reliable, &too_big).unwrap_err();
    assert_eq!(error.code, "net_message_too_large");
    let mut got = Vec::new();
    for _ in 0..3000 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        got.extend(messages(&events[1], Channel::Reliable));
        if got.len() == 2 {
            break;
        }
    }
    assert_eq!(got, vec![big, Vec::new()]);
    // Everything was acknowledged.
    for _ in 0..200 {
        tick(&net, &mut [&mut host, &mut client]);
    }
    assert_eq!(host.queued_bytes(peer), 0);
}

#[test]
fn a_full_queue_is_refused_with_a_diagnostic() {
    let mut config = config();
    config.max_queued_bytes = config.max_message_size;
    let (_net, mut host, _client) =
        connected_pair_with(1, LinkConditions::PERFECT, config, common::config());
    let peer = host.peers()[0];
    host.send(peer, Channel::Reliable, &vec![1; 60_000])
        .unwrap();
    let error = host
        .send(peer, Channel::Reliable, &vec![1; 10_000])
        .unwrap_err();
    assert_eq!(error.code, "net_send_queue_full");
    assert!(!error.hint.is_empty());
    let error = host.send(PeerId(99), Channel::Reliable, b"x").unwrap_err();
    assert_eq!(error.code, "net_unknown_peer");
}

#[test]
fn the_round_trip_is_measured() {
    let (net, mut host, mut client) = connected_pair(
        2,
        LinkConditions {
            latency_ms: 50,
            ..LinkConditions::PERFECT
        },
    );
    for _ in 0..200 {
        tick(&net, &mut [&mut host, &mut client]);
    }
    let rtt = client.rtt_ms(PeerId::HOST).unwrap();
    // 50 ms each way, plus up to a tick of waiting at each end.
    assert!((100..=150).contains(&rtt), "{rtt}");
    assert_eq!(client.local_id(), Some(host.peers()[0]));
}

fn join(
    net: &SimNetwork,
    host: &mut SimEndpoint,
    client_config: wish_net::Config,
    seed: u64,
) -> (SimEndpoint, Vec<Event>) {
    let mut client = Endpoint::connect(
        net.add_node(),
        host.local_addr(),
        client_config,
        Entropy::seeded(seed),
    )
    .unwrap();
    let mut client_events = Vec::new();
    for _ in 0..1000 {
        let events = tick(net, &mut [&mut *host, &mut client]);
        client_events.extend(events[1].iter().cloned());
        if client_events
            .iter()
            .any(|e| matches!(e, Event::Connected { .. } | Event::Disconnected { .. }))
        {
            break;
        }
    }
    (client, client_events)
}

fn new_host(net: &SimNetwork, config: wish_net::Config) -> SimEndpoint {
    Endpoint::host(net.add_node(), config, Entropy::seeded(77)).unwrap()
}

#[test]
fn a_wrong_room_code_is_refused() {
    let net = SimNetwork::new(1, 16);
    let mut host = new_host(&net, config());
    let mut wrong = config();
    wrong.room_code = RoomCode::parse("STAR43").unwrap();
    let (client, events) = join(&net, &mut host, wrong, 2);
    assert_eq!(
        events,
        vec![Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::RoomCodeWrong
        }]
    );
    assert!(client.is_closed());
    assert!(host.peers().is_empty());
    assert_eq!(
        Reason::RoomCodeWrong.diagnostic(PeerId::HOST).code,
        "net_room_code_wrong"
    );
}

#[test]
fn a_full_room_is_refused() {
    let net = SimNetwork::new(1, 16);
    let mut host_config = config();
    host_config.max_peers = 1;
    let mut host = new_host(&net, host_config);
    let (mut first, events) = join(&net, &mut host, config(), 2);
    assert!(matches!(events[0], Event::Connected { .. }));
    let (_second, events) = join(&net, &mut host, config(), 3);
    assert_eq!(
        events,
        vec![Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::RoomFull
        }]
    );
    tick(&net, &mut [&mut host, &mut first]);
    assert_eq!(host.peers().len(), 1);
}

#[test]
fn a_pinned_host_key_must_match() {
    let net = SimNetwork::new(1, 16);
    let mut host = new_host(&net, config());
    let mut pinned = config();
    pinned.host_key = Some([9; 32]);
    let (_client, events) = join(&net, &mut host, pinned, 2);
    assert_eq!(
        events,
        vec![Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::HostKeyMismatch
        }]
    );
    let mut pinned = config();
    pinned.host_key = Some(host.public_key());
    let (_client, events) = join(&net, &mut host, pinned, 3);
    assert!(matches!(events[0], Event::Connected { .. }));
}

#[test]
fn a_client_gives_up_when_nobody_answers() {
    let net = SimNetwork::new(1, 16);
    let nobody = net.add_node();
    let mut cfg = config();
    cfg.connect_timeout_ms = 2000;
    let mut client =
        Endpoint::connect(net.add_node(), nobody.local_addr(), cfg, Entropy::seeded(1)).unwrap();
    let mut events = Vec::new();
    for _ in 0..200 {
        events.extend(tick(&net, &mut [&mut client]).remove(0));
    }
    assert_eq!(
        events,
        vec![Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::ConnectTimedOut
        }]
    );
    let error = client
        .send(PeerId::HOST, Channel::Reliable, b"x")
        .unwrap_err();
    assert_eq!(error.code, "net_unknown_peer");
}

#[test]
fn a_silent_peer_times_out_on_both_sides() {
    let mut cfg = config();
    cfg.timeout_ms = 2000;
    let (net, mut host, mut client) =
        connected_pair_with(4, LinkConditions::PERFECT, cfg.clone(), cfg);
    net.set_conditions(LinkConditions {
        loss: 1.0,
        ..LinkConditions::PERFECT
    });
    let (mut host_events, mut client_events) = (Vec::new(), Vec::new());
    for _ in 0..200 {
        let mut events = tick(&net, &mut [&mut host, &mut client]);
        client_events.append(&mut events[1]);
        host_events.append(&mut events[0]);
    }
    assert!(matches!(
        host_events.as_slice(),
        [Event::Disconnected {
            reason: Reason::TimedOut,
            ..
        }]
    ));
    assert_eq!(
        client_events,
        vec![Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::TimedOut
        }]
    );
}

#[test]
fn a_disconnect_reaches_the_other_side() {
    let (net, mut host, mut client) = connected_pair(6, LinkConditions::PERFECT);
    let peer = host.peers()[0];
    client.disconnect(PeerId::HOST).unwrap();
    assert_eq!(
        client.poll_event(),
        Some(Event::Disconnected {
            peer: PeerId::HOST,
            reason: Reason::Local
        })
    );
    let events = tick(&net, &mut [&mut host, &mut client]);
    assert_eq!(
        events[0],
        vec![Event::Disconnected {
            peer,
            reason: Reason::Closed
        }]
    );
    assert!(host.peers().is_empty());
    assert!(client.is_closed());
}

#[test]
fn many_clients_join_one_host_and_hear_each_message() {
    let net = SimNetwork::new(9, 16);
    net.set_conditions(LinkConditions::BAD);
    let mut host = new_host(&net, config());
    let mut clients: Vec<SimEndpoint> = (0..10)
        .map(|i| {
            Endpoint::connect(
                net.add_node(),
                host.local_addr(),
                config(),
                Entropy::seeded(100 + i),
            )
            .unwrap()
        })
        .collect();
    let mut received = vec![Vec::new(); clients.len()];
    let mut sent = 0u32;
    for _ in 0..5000 {
        if host.peers().len() == clients.len() && sent < 50 {
            for peer in host.peers() {
                host.send(peer, Channel::Reliable, &message(sent, 300))
                    .unwrap();
            }
            sent += 1;
        }
        let mut all: Vec<&mut SimEndpoint> = vec![&mut host];
        all.extend(clients.iter_mut());
        let events = tick(&net, &mut all);
        for (i, events) in events[1..].iter().enumerate() {
            received[i].extend(messages(events, Channel::Reliable));
        }
        if received.iter().all(|r| r.len() == 50) {
            break;
        }
    }
    for r in &received {
        let order: Vec<u32> = r.iter().map(|d| index_of(d)).collect();
        assert_eq!(order, (0..50).collect::<Vec<_>>());
    }
    let mut ids: Vec<PeerId> = clients.iter().map(|c| c.local_id().unwrap()).collect();
    ids.sort();
    assert_eq!(ids, host.peers());
}

#[test]
fn discovery_finds_a_match_by_broadcast() {
    let net = SimNetwork::new(1, 16);
    let host = new_host(&net, config());
    let game_port = host.local_addr().port();
    let responder_node = net
        .bind(std::net::SocketAddr::new(
            host.local_addr().ip(),
            wish_net::DISCOVERY_PORT,
        ))
        .unwrap();
    let mut responder = DiscoveryResponder::new(
        responder_node,
        Announce {
            game: "station".to_owned(),
            name: "Ada's room".to_owned(),
            port: game_port,
            players: 1,
            max_players: 10,
            host_key: host.public_key(),
            room_code: Some(code()),
        },
    )
    .unwrap();
    // Another game on the same network does not answer this one.
    let other_node = net.bind("10.0.7.7:47610".parse().unwrap()).unwrap();
    let mut other = DiscoveryResponder::new(
        other_node,
        Announce {
            game: "bricks".to_owned(),
            name: "x".to_owned(),
            port: 1234,
            players: 0,
            max_players: 2,
            host_key: [0; 32],
            room_code: None,
        },
    )
    .unwrap();
    let query_node = net.bind("10.0.8.8:50000".parse().unwrap()).unwrap();
    let mut query = DiscoveryQuery::new(query_node, "station", &mut Entropy::seeded(4)).unwrap();
    query
        .send(broadcast_addr(wish_net::DISCOVERY_PORT))
        .unwrap();
    for _ in 0..5 {
        net.step();
        responder.update(net.now_ms());
        other.update(net.now_ms());
        query.update();
    }
    let found: Vec<_> = query.found().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, host.local_addr());
    assert_eq!(found[0].1.name, "Ada's room");
    assert_eq!(found[0].1.room_code, Some(code()));
    assert_eq!(found[0].1.host_key, host.public_key());
}
