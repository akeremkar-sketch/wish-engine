//! A modified client: it holds real keys (it joined like anyone) and sends
//! whatever it likes over both channels. The host never panics, drops what
//! is wrong with a diagnostic, and the honest players play on.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeSet;

use common::{Match, SEED, replication};
use wish_core::Rng;
use wish_net::{Channel, Endpoint, Entropy, Event, LinkConditions, PeerId};
use wish_replicate::wire::{self, Frame, Input, kind};
use wish_types::PlayerActionState;

#[test]
fn hostile_messages_never_crash_the_host_and_are_dropped_with_a_diagnostic() {
    let mut game = Match::new(SEED, LinkConditions::BAD, 2, replication());
    // The attacker connects like a player (it gets the next player id).
    let mut attacker = Endpoint::connect(
        game.net.add_node(),
        game.endpoint.local_addr(),
        common::net_config(),
        Entropy::seeded(0xBAD),
    )
    .unwrap();
    let mut rng = Rng::from_seed(0xF022);
    let mut attacker_player = None;
    let mut codes = BTreeSet::new();
    let mut passed_on = 0;
    let real = wire::encode_input(&Input {
        player: 3,
        ack: Some(5),
        ack_mask: 0,
        resync: false,
        frames: vec![Frame {
            tick: 10,
            actions: vec![(common::move_id(), [1.0, 0.0])],
        }],
    });
    for round in 0..600u64 {
        game.net.step();
        let now = game.net.now_ms();
        game.endpoint.update(now);
        for event in game.host.pump(&mut game.world, &mut game.endpoint) {
            if matches!(event, Event::Message { .. }) {
                passed_on += 1;
            }
        }
        game.tick += 1;
        let tick = game.tick;
        game.host.apply_inputs(&mut game.world, tick).unwrap();
        common::move_avatars(&mut game.world);
        game.host
            .send(&game.world, tick, &mut game.endpoint)
            .unwrap();
        game.endpoint.update(now);
        for (_, player) in game.host.players() {
            game.records
                .insert((player, tick), game.host.target(player).unwrap().clone());
        }
        for peer in &mut game.peers {
            peer.endpoint.update(now);
            peer.client.pump(&mut peer.world, &mut peer.endpoint);
            if let Some(player) = peer.client.player() {
                let stamp = peer.client.stamp();
                peer.client
                    .send_input(&mut peer.endpoint, common::bot(player, stamp))
                    .unwrap();
            }
            peer.endpoint.update(now);
        }
        for diagnostic in game.host.take_diagnostics() {
            codes.insert(diagnostic.code.to_string());
        }

        attacker.update(now);
        while let Some(event) = attacker.poll_event() {
            if let Event::Message { data, .. } = event
                && data.first() == Some(&kind::WELCOME)
            {
                attacker_player = Some(wire::decode_welcome(&data).unwrap().player);
            }
        }
        if attacker.local_id().is_none() {
            continue;
        }
        let me = attacker_player.unwrap_or(3);
        let mut send = |bytes: &[u8], reliable: bool| {
            let channel = if reliable {
                Channel::Reliable
            } else {
                Channel::Unreliable
            };
            let _ = attacker.send(PeerId::HOST, channel, bytes);
        };
        // Several attacks per tick, each kind in turn.
        for k in 0..4 {
            let reliable = rng.chance(0.3);
            match (round * 4 + k) % 11 {
                // Random bytes in replication's kind range.
                0 => {
                    let len = 1 + rng.below(300) as usize;
                    let mut bytes: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
                    bytes[0] = rng.below(64) as u8;
                    send(&bytes, reliable);
                }
                // A real message, truncated or extended.
                1 => {
                    let mut bytes = real.clone();
                    bytes[1] = me as u8;
                    if rng.chance(0.5) {
                        bytes.truncate(rng.below(bytes.len() as u32) as usize);
                    } else {
                        bytes.extend((0..1 + rng.below(8)).map(|_| rng.next_u32() as u8));
                    }
                    send(&bytes, reliable);
                }
                // Oversized input.
                2 => {
                    let mut bytes = vec![kind::INPUT; 5000];
                    bytes[1] = me as u8;
                    send(&bytes, true);
                }
                // Input for another player (player 1).
                3 => send(
                    &wire::encode_input(&Input {
                        player: 1,
                        ack: None,
                        ack_mask: 0,
                        resync: false,
                        frames: vec![Frame {
                            tick: tick + 1,
                            actions: vec![(common::move_id(), [-1.0, -1.0])],
                        }],
                    }),
                    reliable,
                ),
                // Acknowledging a tick never sent.
                4 => send(
                    &wire::encode_input(&Input {
                        player: me,
                        ack: Some(1 << 40),
                        ack_mask: u64::MAX,
                        resync: false,
                        frames: vec![],
                    }),
                    reliable,
                ),
                // Input stamped far ahead.
                5 => send(
                    &wire::encode_input(&Input {
                        player: me,
                        ack: None,
                        ack_mask: 0,
                        resync: false,
                        frames: vec![Frame {
                            tick: 1 << 50,
                            actions: vec![(7, [f32::NAN, f32::INFINITY])],
                        }],
                    }),
                    reliable,
                ),
                // Absurd counts: 255 frames, 255 actions.
                6 => {
                    // Player, no ack, an empty mask, no flags, then the
                    // counts.
                    let mut bytes = vec![kind::INPUT, me as u8, 0];
                    bytes.extend([0; 8]);
                    bytes.extend([0, 255, 1, 255]);
                    send(&bytes, reliable);
                }
                // Resync requests every tick.
                7 => send(
                    &wire::encode_input(&Input {
                        player: me,
                        ack: None,
                        ack_mask: 0,
                        resync: true,
                        frames: vec![],
                    }),
                    reliable,
                ),
                // A host's message kind, from a client.
                8 => send(&[kind::STATE, 0, 0, 0, 0, 0], reliable),
                // Not replication's: passed on to the caller.
                9 => send(&[200, 1, 2, 3], true),
                // Valid input with hostile values: its own player only.
                _ => send(
                    &wire::encode_input(&Input {
                        player: me,
                        ack: None,
                        ack_mask: 0,
                        resync: false,
                        frames: vec![Frame {
                            tick: tick + 1,
                            actions: vec![
                                (common::move_id(), [f32::NAN, 1e30]),
                                (common::move_id(), [5.0, -5.0]),
                                (0xDEAD_BEEF, [1.0, 1.0]),
                            ],
                        }],
                    }),
                    reliable,
                ),
            }
        }
        attacker.update(now);
        // The forged input never reaches player 1: its entry is its own.
        if let Some(state) = game.world.resource::<PlayerActionState>() {
            for slot in &state.players[1].slots {
                // Player 1's bot never moves along y; the forged input did.
                assert_eq!(slot.y, 0.0, "player 1 was moved by someone else");
            }
            // The attacker's own values are cleaned.
            for slot in &state.players[me as usize].slots {
                assert!(slot.x.is_finite() && (-1.0..=1.0).contains(&slot.x));
                assert!(slot.y.is_finite() && (-1.0..=1.0).contains(&slot.y));
            }
        }
    }
    let attacker_player = attacker_player.expect("the attacker was welcomed");
    for code in [
        "net_message_invalid",
        "net_input_wrong_player",
        "net_ack_invalid",
        "net_input_out_of_window",
    ] {
        assert!(codes.contains(code), "no {code} among {codes:?}");
    }
    let stats = game.host.stats();
    assert!(stats.invalid > 100 && stats.wrong_player > 100 && stats.invalid_acks > 100);
    assert!(
        passed_on > 0,
        "messages that are not replication's are passed on"
    );
    // At most one resync per 64 ticks.
    assert!(
        stats.snapshots_sent <= 3 + 600 / 64 + 1,
        "{} snapshots",
        stats.snapshots_sent
    );

    // Forged network ids: the attacker resolves only what it may see.
    let target = game.host.target(attacker_player).unwrap().clone();
    for net in 0..10_000 {
        if let Some(entity) = game.host.resolve(attacker_player, net) {
            assert!(target.contains_key(&net));
            assert!(!game.cast.secrets.values().any(|s| *s == entity));
        }
    }
    // Network ids are per player: player 1's id of its secret names
    // something else (or nothing) for the attacker.
    for (net, entity) in game.host.network_ids(1) {
        if game.cast.secrets.get(&1) == Some(&entity) {
            assert_ne!(game.host.resolve(attacker_player, net), Some(entity));
            assert_eq!(game.host.resolve(1, net), Some(entity));
        }
    }

    // The honest players were never disturbed, and are current.
    for _ in 0..100 {
        game.step();
    }
    assert!(game.all_current());
}
