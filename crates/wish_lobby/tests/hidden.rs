//! Secret roles and whispers reach only their owners, proven on the wire:
//! every datagram the host sends is captured, decrypted with keys rebuilt
//! from the seeds, reassembled and decoded (design note §9).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{CREW, Party, SABOTEUR, SEED, WHISPER_SECRET, contains, lobby_config, sniff};
use wish_lobby::wire::{self, Down};
use wish_net::LinkConditions;
use wish_types::MatchRoles;

#[test]
fn roles_and_whispers_reach_only_their_owners() {
    let mut party = Party::new(SEED ^ 7, LinkConditions::BAD, lobby_config(6));
    party.net.start_capture();
    party.fill_and_start(&[("Ann", 1), ("Ben", 2), ("Cem", 3), ("Dua", 4), ("Eli", 5)]);
    party.run_until(900, |p| p.meeting().done);
    party.run(60);
    let captured = party.net.take_captured();
    let streams = sniff(&party, &captured);
    let roles = *party.host.roles();
    let saboteurs: Vec<u32> = (0..6).filter(|p| roles[*p as usize] == 1).collect();
    assert_eq!(saboteurs.len(), 2);

    let mut crew_seen = 0;
    let mut saboteurs_seen = 0;
    for player in &party.players {
        let seat = player.session.seat().unwrap();
        let stream = &streams[&player.addr()];
        let mut own_roles = 0;
        let mut whispers = 0;
        for message in stream.messages() {
            match wire::decode_down(&message) {
                Some(Down::Role { role, name, .. }) => {
                    assert_eq!(
                        role, roles[seat as usize],
                        "player {seat} got a role not its own"
                    );
                    let expected = if role == 1 { SABOTEUR } else { CREW };
                    assert_eq!(name, expected);
                    own_roles += 1;
                }
                Some(Down::Message { .. }) => {
                    if contains(&message, &WHISPER_SECRET.to_le_bytes()) {
                        whispers += 1;
                    }
                }
                Some(Down::Lobby(lobby)) => {
                    // The public lobby says nothing about roles.
                    assert!(!contains(&message, SABOTEUR.as_bytes()));
                    assert_eq!(lobby.seats.len(), 16);
                }
                _ => {}
            }
        }
        assert_eq!(own_roles, 1, "player {seat} got {own_roles} role messages");
        let text = &stream.plaintext;
        if roles[seat as usize] == 1 {
            saboteurs_seen += 1;
            assert_eq!(whispers, 1);
            assert!(contains(text, SABOTEUR.as_bytes()), "the search works");
            assert_eq!(
                player.heard().partner,
                *saboteurs.iter().find(|s| **s != seat).unwrap()
            );
        } else {
            crew_seen += 1;
            assert_eq!(whispers, 0, "crew player {seat} received a whisper");
            assert!(
                !contains(text, SABOTEUR.as_bytes()),
                "crew player {seat} received the saboteur role's name"
            );
            assert!(
                !contains(text, &WHISPER_SECRET.to_le_bytes()),
                "crew player {seat} received the whisper's secret"
            );
            assert_eq!(player.heard().whispers, 0);
        }
        assert!(
            player.world().resource::<MatchRoles>().is_none(),
            "a player's world never holds every role"
        );
        assert_eq!(player.local().role, roles[seat as usize]);
    }
    assert!(crew_seen >= 3);
    // The host may be a saboteur itself (seat 0): then one saboteur is
    // remote.
    assert_eq!(
        saboteurs_seen,
        saboteurs.iter().filter(|s| **s != 0).count()
    );
}
