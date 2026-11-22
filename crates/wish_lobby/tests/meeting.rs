//! A meeting with chat and votes over a bad network: every message arrives,
//! in the host's order, at every player; the same seed gives the same
//! match; the host's log replays it bit for bit (design note §9).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Party, SEED, lobby_config};
use wish_net::LinkConditions;

const NAMES: &[(&str, u32)] = &[("Ann", 1), ("Ben", 2), ("Cem", 3), ("Dua", 4), ("Eli", 5)];

fn meeting(seed: u64) -> Party {
    let mut party = Party::new(seed, LinkConditions::BAD, lobby_config(6));
    party.fill_and_start(NAMES);
    party.run_until(900, |p| {
        p.meeting().done && p.players.iter().all(|pl| pl.heard().results == 1)
    });
    party.run(30);
    party
}

#[test]
fn every_player_hears_the_meeting_in_order_under_loss() {
    let mut party = meeting(SEED);
    let host = party.meeting();
    assert!(host.called && host.done);
    // Six players, three lines each, the host's own included.
    assert_eq!(host.lines, 18, "{host:?}");
    assert_eq!(host.strangers, 0);
    let host_heard = party
        .app
        .world()
        .resource::<common::Heard>()
        .copied()
        .unwrap();
    assert_eq!(host_heard.lines, 18);
    assert_eq!(host_heard.digest, host.digest);
    for player in &party.players {
        let heard = player.heard();
        assert_eq!(heard.lines, 18, "player {:?}", player.session.seat());
        assert_eq!(
            heard.digest,
            host.digest,
            "player {:?} heard the lines in another order",
            player.session.seat()
        );
        assert_eq!(heard.results, 1);
        assert_eq!(heard.ejected, host_heard.ejected);
        assert_eq!(heard.wrong_sender, 0);
    }
    // Every vote arrived: players vote for (seat % 2) + 1.
    for seat in 0..6u32 {
        assert_eq!(host.votes[seat as usize], (seat % 2) + 2, "seat {seat}");
    }
    // Every player pointed at its own avatar, through network ids.
    assert_eq!(host.points, 6);
    assert_eq!(host.points_own, 6);
    // The network lost datagrams, and nothing was dropped above it.
    assert!(party.net.stats().lost > 0);
    let stats = party.host.stats();
    assert_eq!(
        stats.dropped_invalid + stats.dropped_flood + stats.dropped_oversized,
        0
    );
    assert_eq!(stats.messages_received, 5 * (3 + 1 + 1));
    let diagnostics = party.host.take_diagnostics();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn the_same_seed_gives_the_same_match() {
    let a = meeting(SEED);
    let b = meeting(SEED);
    assert_eq!(a.net.delivery_hash(), b.net.delivery_hash());
    assert_eq!(a.hashes, b.hashes);
    assert_eq!(a.player_hashes, b.player_hashes);
    let c = meeting(SEED ^ 0xFF);
    assert_ne!(a.net.delivery_hash(), c.net.delivery_hash());
}

#[test]
fn the_hosts_log_replays_the_match_bit_for_bit() {
    let party = meeting(SEED);
    let log = party.host.log();
    let with_messages = log
        .records
        .values()
        .filter(|r| !r.messages.is_empty())
        .count();
    assert!(
        with_messages > 10,
        "{with_messages} ticks delivered messages"
    );
    assert!(log.records.values().any(|r| r.lobby.is_some()));
    assert!(log.records.values().any(|r| r.roles.is_some()));
    assert!(log.records.values().any(|r| r.remote.is_some()));
    let checked = party.check_replay();
    assert_eq!(checked, party.hashes.len());
    assert!(checked > 300);
}
