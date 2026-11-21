//! Hidden information, proven on the wire: every datagram the host puts on
//! the simulated network is captured, decrypted with keys rebuilt from the
//! test's seeds, reassembled into messages and decoded. No message to a
//! player holds an entity that player could not see at that tick, and the
//! other players' secret markers appear nowhere in its bytes.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::net::SocketAddr;

use common::{GHOST_MARKER, Match, SEED, TREASURE_MARKER, replication, secret_marker, secret_role};
use wish_net::crypto::{KeyPair, SessionKeys};
use wish_net::wire::{self as net_wire, Frame, Packet};
use wish_net::{Entropy, LinkConditions};
use wish_replicate::wire::{self, kind};
use wish_types::PlayerId;

/// The plaintext messages the host sent to one client, as that client's
/// keys open them.
#[derive(Default)]
struct Stream {
    /// Reliable fragments by sequence: (index, count, bytes).
    reliable: BTreeMap<u32, (u16, u16, Vec<u8>)>,
    /// Unreliable fragments by message id, then index: (count, bytes).
    unreliable: BTreeMap<u32, (u16, BTreeMap<u16, Vec<u8>>)>,
    /// Every opened plaintext, concatenated (the marker search reads it).
    plaintext: Vec<u8>,
    /// Every sealed byte, concatenated.
    sealed: Vec<u8>,
}

impl Stream {
    /// Every whole message: `(reliable, bytes)`.
    fn messages(&self) -> Vec<(bool, Vec<u8>)> {
        let mut out = Vec::new();
        for (&seq, (index, count, bytes)) in &self.reliable {
            if *index != 0 {
                continue;
            }
            let mut message = bytes.clone();
            let mut whole = true;
            for k in 1..u32::from(*count) {
                match self.reliable.get(&(seq + k)) {
                    Some((i, _, more)) if u32::from(*i) == k => message.extend_from_slice(more),
                    _ => whole = false,
                }
            }
            if whole {
                out.push((true, message));
            }
        }
        for (count, parts) in self.unreliable.values() {
            if parts.len() == usize::from(*count) {
                out.push((false, parts.values().flatten().copied().collect()));
            }
        }
        out
    }
}

/// Opens every captured host → client datagram.
fn sniff(game: &Match, captured: &[wish_net::Captured]) -> BTreeMap<SocketAddr, Stream> {
    let host_keys = KeyPair::generate(&mut Entropy::seeded(game.host_seed));
    let host_public = host_keys.public();
    assert_eq!(
        host_public,
        game.endpoint.public_key(),
        "the rebuilt host key is the real one"
    );
    let host_addr = game.endpoint.local_addr();
    // The clients' public keys, from their handshakes.
    let mut client_keys: BTreeMap<SocketAddr, [u8; 32]> = BTreeMap::new();
    for datagram in captured {
        if datagram.to != host_addr {
            continue;
        }
        if let Some(Packet::Hello { client_key, .. } | Packet::Connect { client_key, .. }) =
            net_wire::decode(&datagram.bytes)
        {
            client_keys.insert(datagram.from, client_key);
        }
    }
    let mut streams: BTreeMap<SocketAddr, Stream> = BTreeMap::new();
    let mut opened = 0;
    for datagram in captured {
        if datagram.from != host_addr {
            continue;
        }
        let stream = streams.entry(datagram.to).or_default();
        stream.sealed.extend_from_slice(&datagram.bytes);
        let Some(Packet::Data {
            number,
            header,
            sealed,
            ..
        }) = net_wire::decode(&datagram.bytes)
        else {
            continue;
        };
        let client = client_keys[&datagram.to];
        let shared = host_keys.agree(&client).unwrap();
        let keys = SessionKeys::client(&shared, &client, &host_public);
        let mut body = sealed.to_vec();
        let len = keys
            .open(number, header, &mut body)
            .expect("every data packet of the host opens with the client's keys");
        opened += 1;
        let text = &body[..len];
        stream.plaintext.extend_from_slice(text);
        let mut frames = Vec::new();
        assert!(net_wire::decode_frames(text, &mut frames));
        for frame in frames {
            match frame {
                Frame::Reliable {
                    seq,
                    index,
                    count,
                    bytes,
                } => {
                    stream.reliable.insert(seq, (index, count, bytes.to_vec()));
                }
                Frame::Unreliable {
                    id,
                    index,
                    count,
                    bytes,
                } => {
                    stream
                        .unreliable
                        .entry(id)
                        .or_insert_with(|| (count, BTreeMap::new()))
                        .1
                        .insert(index, bytes.to_vec());
                }
                _ => {}
            }
        }
    }
    assert!(opened > 1000, "only {opened} packets were opened");
    streams
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_player_never_receives_what_it_may_not_see() {
    let mut game = Match::new(SEED, LinkConditions::BAD, 3, replication());
    game.net.start_capture();
    game.run(400);
    let captured = game.net.take_captured();
    let streams = sniff(&game, &captured);
    let layouts = game.host.layouts();
    let budget = game.host.config().budget_bytes;

    for peer in &game.peers {
        let player: PlayerId = peer.client.player().unwrap();
        let stream = &streams[&peer.addr()];
        let messages = stream.messages();
        let mut states = 0;
        let mut welcomes = 0;
        for (reliable, message) in &messages {
            match message[0] {
                kind::WELCOME => {
                    let welcome = wire::decode_welcome(message).unwrap();
                    assert_eq!(welcome.player, player);
                    welcomes += 1;
                }
                kind::STATE => {
                    let state = wire::decode_state(message, &layouts).unwrap();
                    states += 1;
                    let visible = &game.records[&(player, state.tick)];
                    for section in &state.entities {
                        assert!(
                            visible.contains_key(&section.id),
                            "player {player} received entity {} at tick {} although it could not see it",
                            section.id,
                            state.tick
                        );
                    }
                    if !reliable {
                        assert!(
                            message.len() <= budget,
                            "a delta of {} bytes",
                            message.len()
                        );
                    }
                }
                other => panic!("unexpected message kind {other}"),
            }
        }
        assert_eq!(welcomes, 1);
        assert!(states > 300, "player {player} got {states} states");

        // The markers: its own are on the wire (the search works), the
        // others' are not, in any byte the host sent to it.
        let text = &stream.plaintext;
        assert!(contains(text, &secret_marker(player).to_le_bytes()));
        assert!(contains(text, secret_role(player).as_bytes()));
        for other in 1..=3u32 {
            if other == player {
                continue;
            }
            assert!(
                !contains(text, &secret_marker(other).to_le_bytes()),
                "player {player} received player {other}'s secret number"
            );
            assert!(
                !contains(text, secret_role(other).as_bytes()),
                "player {player} received player {other}'s secret role"
            );
            // Not even in the sealed bytes (as a listener sees them).
            assert!(!contains(&stream.sealed, secret_role(other).as_bytes()));
        }
        assert_eq!(contains(text, &GHOST_MARKER.to_le_bytes()), player == 3);
        assert_eq!(contains(text, &TREASURE_MARKER.to_le_bytes()), player == 1);
    }
}
