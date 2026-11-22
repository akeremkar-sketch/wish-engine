//! The meeting with predicting players (design note
//! `docs/design/1.1-net-tool.md` §12): every player's game runs in its
//! prediction, corrections play ticks again, and still every message is
//! sent once, delivered once and heard in the host's order.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Party, SEED, lobby_config};
use wish_net::LinkConditions;

const NAMES: &[(&str, u32)] = &[("Ann", 1), ("Ben", 2), ("Cem", 3), ("Dua", 4), ("Eli", 5)];

/// The meeting over a bad network on which, for 20 of every 80 frames,
/// everything the players send is lost: the host holds their old inputs,
/// so their predictions are corrected and ticks are played again.
fn predicted_meeting(seed: u64) -> Party {
    let mut party = Party::new(seed, LinkConditions::BAD, lobby_config(6));
    party.predicting = true;
    party.fill_and_start(NAMES);
    let host = party.endpoint.local_addr();
    let mut frame = 0u64;
    while !(party.meeting().done && party.players.iter().all(|p| p.heard().results == 1)) {
        assert!(
            frame < 3000,
            "the meeting did not end: {:?}",
            party.meeting()
        );
        let conditions = if frame % 80 < 20 {
            LinkConditions {
                loss: 1.0,
                ..LinkConditions::BAD
            }
        } else {
            LinkConditions::BAD
        };
        for player in &party.players {
            party.net.set_link(player.addr(), host, conditions);
        }
        party.step();
        frame += 1;
    }
    for player in &party.players {
        party.net.set_link(player.addr(), host, LinkConditions::BAD);
    }
    party.run(60);
    party
}

#[test]
fn a_predicting_player_sends_each_message_once_however_often_a_tick_is_played() {
    let party = predicted_meeting(SEED);
    let host = party.meeting();
    assert!(host.called && host.done);
    assert_eq!(host.lines, 18, "{host:?}");
    assert_eq!(host.strangers, 0);
    // Every player's prediction was corrected, and played ticks again in
    // which its game sent messages.
    let corrections: u64 = party
        .players
        .iter()
        .map(|p| p.predictor.as_ref().unwrap().stats().corrections)
        .sum();
    let replayed: u64 = party.players.iter().map(|p| p.replayed_sends).sum();
    assert!(corrections > 0, "no prediction was corrected");
    assert!(
        replayed > 0,
        "no correction played a tick in which a message was sent"
    );
    // Still each was sent once: three lines, a vote and a point per player.
    let stats = party.host.stats();
    assert_eq!(stats.messages_received, 5 * (3 + 1 + 1), "{stats:?}");
    assert_eq!(
        stats.dropped_invalid + stats.dropped_flood + stats.dropped_oversized,
        0
    );
    for seat in 0..6u32 {
        assert_eq!(host.votes[seat as usize], (seat % 2) + 2, "seat {seat}");
    }
    // A point names the player's own avatar through the prediction's
    // network ids.
    assert_eq!(host.points, 6);
    assert_eq!(host.points_own, 6);
    // And each prediction heard every line once, in the host's order.
    for player in &party.players {
        let heard = player.heard();
        assert_eq!(heard.lines, 18, "player {:?}", player.session.seat());
        assert_eq!(
            heard.digest,
            host.digest,
            "player {:?}",
            player.session.seat()
        );
        assert_eq!(heard.results, 1);
        assert_eq!(heard.wrong_sender, 0);
        assert_eq!(player.session.stats().messages_sent, 5);
    }
}

#[test]
fn the_same_seed_gives_the_same_predicted_match() {
    let a = predicted_meeting(SEED);
    let b = predicted_meeting(SEED);
    assert_eq!(a.net.delivery_hash(), b.net.delivery_hash());
    assert_eq!(a.hashes, b.hashes);
    assert_eq!(a.player_hashes, b.player_hashes);
    assert_eq!(a.check_replay(), a.hashes.len());
}
