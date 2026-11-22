//! A hostile client with real keys: it spoofs a sender, sends before it
//! joins and joins twice, floods, sends oversized and malformed messages,
//! host-only kinds and random bytes. The host never panics, stamps the
//! attacker's own seat on everything it accepts, bounds the flood, and the
//! honest players' meeting goes on (design note §9).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{ChatSay, Party, Point, SEED, lobby_config, net_config};
use wish_core::{Entity, Rng, Text};
use wish_lobby::message::{MessageType, MessageValue, put_value};
use wish_lobby::wire::{self, Down, Join, Up};
use wish_net::{Channel, Endpoint, Entropy, Event, LinkConditions, PeerId, SimTransport};
use wish_schema::Plain;
use wish_types::{Lobby, LobbyPhase, Net, NetTo};

struct Attacker {
    endpoint: Endpoint<SimTransport>,
    seat: Option<u32>,
}

impl Attacker {
    fn send(&mut self, bytes: &[u8]) {
        let _ = self.endpoint.send(PeerId::HOST, Channel::Reliable, bytes);
    }

    fn pump(&mut self, now: u64) {
        self.endpoint.update(now);
        while let Some(event) = self.endpoint.poll_event() {
            if let Event::Message { data, .. } = event
                && let Some(Down::Admitted { seat, .. }) = wire::decode_down(&data)
            {
                self.seat = Some(seat);
            }
        }
    }
}

fn message_bytes(ty: &MessageType, index: u16, value: &MessageValue) -> Vec<u8> {
    let mut bytes = wire::encode_up(&Up::Message { ty: index, at: 0 });
    put_value(&mut bytes, &value.to_wire(&|_| 0), &ty.layout);
    bytes
}

#[test]
fn a_hostile_client_cannot_crash_flood_or_impersonate() {
    let mut party = Party::new(SEED ^ 0x0BAD, LinkConditions::BAD, lobby_config(4));
    party.join("Ann", 1);
    party.join("Ben", 2);
    let mut attacker = Attacker {
        endpoint: Endpoint::connect(
            party.net.add_node(),
            party.endpoint.local_addr(),
            net_config(),
            Entropy::seeded(0xE71),
        )
        .unwrap(),
        seat: None,
    };
    let types = wish_lobby::message_types(party.app.world()).0;
    let index_of = |name: &str| types.iter().position(|t| t.name == name).unwrap() as u16;
    let chat = index_of("ChatSay");
    let point = index_of("Point");
    let spoof = |text: &str| {
        let mut bytes = vec![0u8; size_of::<ChatSay>()];
        ChatSay {
            net: Net {
                to: NetTo::Everyone,
                player: 2,
                groups: 0xFFFF,
                from: 1,
                delivered: true,
            },
            text: Text::EMPTY,
        }
        .encode(&mut bytes);
        let value = MessageValue {
            bytes,
            texts: vec![text.to_owned()],
            refs: Vec::new(),
        };
        message_bytes(&types[usize::from(chat)], chat, &value)
    };
    let forged_point = {
        let mut bytes = vec![0u8; size_of::<Point>()];
        Point {
            net: Net::to_host(),
            at: Entity::NULL,
        }
        .encode(&mut bytes);
        let mut value = wish_lobby::MessageValue {
            bytes,
            texts: Vec::new(),
            refs: vec![Entity::NULL],
        }
        .to_wire(&|_| 0);
        value.refs = vec![9999];
        let mut out = wire::encode_up(&Up::Message { ty: point, at: 0 });
        put_value(&mut out, &value, &types[usize::from(point)].layout);
        out
    };

    let mut rng = Rng::from_seed(0xF00D);
    let mut joined = false;
    let mut readied = false;
    let mut started = false;
    let mut flooded = 0;
    for frame in 0..1500u64 {
        party.step();
        let now = party.net.now_ms();
        attacker.pump(now);
        let connected = !attacker.endpoint.peers().is_empty();
        if connected && !joined {
            // Before joining: a message, then the join, then a second join.
            attacker.send(&spoof("too early"));
            let join = wire::encode_up(&Up::Join(Join {
                version: wire::VERSION,
                token: None,
                name: "Mallory".into(),
                color: 1,
            }));
            attacker.send(&join);
            attacker.send(&join);
            joined = true;
        }
        if attacker.seat.is_some() && !readied {
            attacker.send(&wire::encode_up(&Up::Ready(true)));
            readied = true;
        }
        if !started && frame % 10 == 0 {
            for player in &mut party.players {
                if player.session.seat().is_some() {
                    player
                        .session
                        .set_ready(&mut player.endpoint, true)
                        .unwrap();
                }
            }
        }
        if readied && !started && party.host.lobby().seats[1..4].iter().all(|s| s.ready) {
            party.host.start().unwrap();
            started = true;
        }
        let meeting = party.meeting();
        if meeting.called && !meeting.done && attacker.seat.is_some() {
            match frame % 40 {
                // A spoofed sender, target and delivered flag.
                0 => attacker.send(&spoof("spoof")),
                // A flood.
                1 if flooded < 2 => {
                    for _ in 0..300 {
                        attacker.send(&spoof("flood"));
                    }
                    flooded += 1;
                }
                // Oversized, unknown type, truncated, host-only kinds.
                2 => {
                    let mut big = spoof("x");
                    big.extend(std::iter::repeat_n(0u8, 3000));
                    attacker.send(&big);
                    attacker.send(&wire::encode_up(&Up::Message { ty: 200, at: 0 }));
                    let real = spoof("cut");
                    attacker.send(&real[..real.len() - 3]);
                    attacker.send(&wire::encode_down(&Down::Closing));
                    attacker.send(&wire::encode_down(&Down::Refused(wire::Refusal::Full)));
                    attacker.send(&[wire::kind::ADMITTED, 1, 0]);
                }
                // A reference to an entity it never saw.
                3 => attacker.send(&forged_point),
                // Random bytes of the lobby's kinds and of no one's.
                4 => {
                    for _ in 0..20 {
                        let len = rng.below(80) as usize + 1;
                        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
                        bytes[0] = 64 + rng.below(64) as u8;
                        attacker.send(&bytes);
                    }
                }
                _ => {}
            }
        }
        attacker.endpoint.update(now);
        if meeting.done && frame > 900 {
            break;
        }
    }
    assert!(started, "the match started");
    let seat = attacker.seat.unwrap();
    let host = party.meeting();
    assert!(host.done);

    // Everything the host accepted from the attacker carries its own seat.
    let log = party.host.log();
    let mut accepted = 0;
    for record in log.records.values() {
        for message in &record.messages {
            if message.ty == "ChatSay"
                && message
                    .value
                    .texts
                    .first()
                    .is_some_and(|t| t == "spoof" || t == "flood")
            {
                assert_eq!(message.from, seat, "a forged sender was believed");
                let net = types[usize::from(chat)].envelope(&message.value.bytes);
                assert_eq!(net.from, seat);
                assert_eq!(
                    net.to,
                    NetTo::Host,
                    "a player's message goes to the host only"
                );
                assert!(net.delivered);
                accepted += 1;
            }
            assert!(
                message
                    .value
                    .texts
                    .first()
                    .is_none_or(|t| t != "too early" && t != "cut"),
                "a message before the join or a truncated one was delivered"
            );
        }
    }
    assert!(accepted > 0);
    // The flood was bounded by the bucket.
    let stats = party.host.stats();
    assert!(stats.dropped_flood >= 400, "{stats:?}");
    assert!(accepted < 200, "{accepted} spoofed lines got through");
    assert!(stats.dropped_oversized >= 1);
    assert!(stats.dropped_invalid >= 5);
    let codes: Vec<String> = party
        .host
        .take_diagnostics()
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    for code in [
        "net_message_flood",
        "net_message_invalid",
        "net_message_oversized",
    ] {
        assert!(codes.iter().any(|c| c == code), "no {code} in {codes:?}");
    }
    // The forged reference resolved to nothing: only honest points name
    // their own avatars.
    assert_eq!(host.points_own, 3, "{host:?}");
    assert_eq!(host.strangers, 0);

    // The honest players heard the whole meeting, in the host's order.
    for player in &party.players {
        let heard = player.heard();
        assert_eq!(heard.digest, host.digest);
        assert_eq!(heard.lines, host.lines);
        assert_eq!(heard.results, 1);
    }
    for honest in [0u32, 1, 2] {
        let seat_of = if honest == 0 {
            0
        } else {
            party.players[honest as usize - 1].session.seat().unwrap()
        };
        assert_eq!(host.votes[seat_of as usize], (seat_of % 2) + 2);
    }
    // The attacker is still a player; nothing it sent ended the match.
    let lobby = party.app.world().resource::<Lobby>().copied().unwrap();
    assert_eq!(lobby.phase, LobbyPhase::Running);
    assert!(lobby.seats[seat as usize].connected);
    party.check_replay();
}
