//! Sessions: a player who drops comes back into its seat within the grace
//! period, one who comes too late is refused, and the host closing ends the
//! match for everyone (design note §9).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Party, SEED, lobby_config};
use wish_net::LinkConditions;
use wish_types::{Lobby, LobbyClosed, LobbyPhase};

/// Cuts (or restores) every link to and from player `index`.
fn cut(party: &Party, index: usize, off: bool) {
    let conditions = if off {
        LinkConditions {
            loss: 1.0,
            ..LinkConditions::PERFECT
        }
    } else {
        LinkConditions::BAD
    };
    let host = party.endpoint.local_addr();
    let addr = party.players[index].addr();
    party.net.set_link(host, addr, conditions);
    party.net.set_link(addr, host, conditions);
}

fn seat_connected(party: &Party, seat: u32) -> bool {
    party.host.lobby().seats[seat as usize].connected
}

#[test]
fn a_dropped_player_rejoins_into_its_seat_and_a_late_one_is_refused() {
    let mut config = lobby_config(4);
    config.rejoin_grace_ticks = 400;
    let mut party = Party::new(SEED ^ 0x5E55, LinkConditions::BAD, config);
    party.fill_and_start(&[("Ann", 1), ("Ben", 2), ("Cem", 3)]);
    party.run(60);
    let roles = *party.host.roles();
    let ann = party.players[0].session.seat().unwrap();
    let ann_role = party.players[0].local().role;
    assert_eq!(ann_role, roles[ann as usize]);
    let ann_name = party.host.lobby().seats[ann as usize].name.clone();

    // Ann's network goes away: the host keeps the seat, shown disconnected.
    cut(&party, 0, true);
    party.run_until(400, |p| !seat_connected(p, ann));
    let seat = party.host.lobby().seats[ann as usize].clone();
    assert!(seat.taken && !seat.left);
    assert_eq!(party.host.lobby().phase, LobbyPhase::Running);
    party.run_until(400, |p| {
        p.players[0].session.closed() == Some(LobbyClosed::Lost)
    });
    // Every other machine sees it.
    party.run_until(200, |p| {
        p.players[1..].iter().all(|pl| {
            pl.world()
                .resource::<Lobby>()
                .is_some_and(|l| !l.seats[ann as usize].connected && l.seats[ann as usize].taken)
        })
    });

    // Back, on a new connection, with the token: the same seat, name, role.
    cut(&party, 0, false);
    party.reconnect(0, 1);
    party.run_until(400, |p| {
        seat_connected(p, ann) && p.players[0].session.seat() == Some(ann)
    });
    party.run_until(200, |p| {
        p.players[0].local().role == ann_role && p.players[0].local().joined
    });
    assert_eq!(party.host.lobby().seats[ann as usize].name, ann_name);
    assert_eq!(party.host.stats().rejoins, 1);
    // Its world gets the match again from a fresh snapshot.
    party.run_until(300, |p| {
        let world = p.players[0].world();
        world
            .entities()
            .filter(|e| world.get::<common::Avatar>(*e).is_some())
            .count()
            == 4
    });

    // Ben drops and comes back after the grace period: refused, seat left.
    let ben = party.players[1].session.seat().unwrap();
    cut(&party, 1, true);
    party.run_until(400, |p| !seat_connected(p, ben));
    party.run_until(600, |p| p.host.lobby().seats[ben as usize].left);
    cut(&party, 1, false);
    party.reconnect(1, 2);
    party.run_until(400, |p| {
        p.players[1].session.closed() == Some(LobbyClosed::Refused)
    });
    assert!(party.host.lobby().seats[ben as usize].left);
    let codes: Vec<String> = party
        .host
        .take_diagnostics()
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(
        codes.iter().any(|c| c == "lobby_rejoin_expired"),
        "{codes:?}"
    );

    // The host's log replays all of it: the drop, the rejoin, the refusal.
    party.check_replay();

    // After the match, the lobby reopens without the player who left.
    party.host.end_match(1);
    party.run(10);
    party.host.reopen();
    party.run_until(300, |p| p.host.lobby().phase == LobbyPhase::Open);
    assert!(!party.host.lobby().seats[ben as usize].taken);
    assert!(party.host.lobby().seats[ann as usize].taken);
}

#[test]
fn the_host_leaving_ends_the_match_for_everyone() {
    let mut party = Party::new(SEED ^ 0xC105E, LinkConditions::BAD, lobby_config(4));
    party.fill_and_start(&[("Ann", 1), ("Ben", 2), ("Cem", 3)]);
    party.run(30);
    party.host.close();
    let taken = party.run_until(400, |p| {
        p.host.is_closed()
            && p.players.iter().all(|pl| {
                pl.session.closed() == Some(LobbyClosed::HostLeft)
                    && pl.world().resource::<Lobby>().is_some_and(|l| {
                        l.phase == LobbyPhase::Closed && l.closed == LobbyClosed::HostLeft
                    })
            })
    });
    assert!(taken < 200, "it took {taken} ticks");
    for player in &party.players {
        assert!(!player.local().joined);
    }
    let host_lobby = party.host_lobby();
    assert_eq!(host_lobby.phase, LobbyPhase::Closed);
    assert_eq!(host_lobby.closed, LobbyClosed::HostLeft);
    // Nothing breaks after the close.
    party.run(30);
    party.check_replay();
}
