//! A six-player lobby: players join with clashing names and colours, get
//! ready, the host starts, roles are dealt, the match ends and the lobby
//! reopens (design note §9).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{CREW, Party, SABOTEUR, SEED, lobby_config};
use wish_net::LinkConditions;
use wish_types::{Lobby, LobbyPhase, LocalPlayer, MatchRoles};

#[test]
fn six_players_fill_ready_and_start() {
    let mut party = Party::new(SEED, LinkConditions::BAD, lobby_config(6));
    // Two players ask for the same name and colour; one asks for the
    // host's colour.
    for (name, color) in [("Sam", 3), ("sam", 3), ("Ada", 0), ("Bo", 7), ("  ", 9)] {
        party.join(name, color);
    }
    party.run_until(600, Party::all_seated);
    let lobby = party.host.lobby().clone();
    assert_eq!(lobby.phase, LobbyPhase::Open);
    assert_eq!(lobby.seated(), vec![0, 1, 2, 3, 4, 5]);
    let names: Vec<&str> = lobby.seats[..6].iter().map(|s| s.name.as_str()).collect();
    let colors: Vec<u32> = lobby.seats[..6].iter().map(|s| s.color).collect();
    // Seats go in connection order; the clash is resolved by seat.
    let seat_of = |index: usize| party.players[index].session.seat().unwrap();
    assert_eq!(names[seat_of(0) as usize], "Sam");
    assert_eq!(names[seat_of(1) as usize], "sam 2");
    assert_eq!(names[seat_of(4) as usize], format!("Player {}", seat_of(4)));
    assert_eq!(colors[seat_of(0) as usize], 3);
    assert_eq!(colors[seat_of(1) as usize], 4, "the next free colour");
    assert_ne!(colors[seat_of(2) as usize], 0, "the host has colour 0");
    let mut distinct = colors.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 6, "every colour differs: {colors:?}");
    // Every machine shows the same lobby.
    party.run(30);
    for player in &party.players {
        for seat in 0..6 {
            assert_eq!(
                Party::name_in(player.world(), seat),
                names[seat as usize],
                "the lobby differs on a player's machine"
            );
        }
        let local = player.local();
        assert!(local.joined && !local.host);
        assert_eq!(local.player, player.session.seat().unwrap());
    }

    // Not everyone is ready: the start is refused.
    let error = party.host.start().unwrap_err();
    assert_eq!(error.code, "lobby_not_ready");
    for player in party.players.iter_mut().take(4) {
        player
            .session
            .set_ready(&mut player.endpoint, true)
            .unwrap();
    }
    party.run(40);
    let error = party.host.start().unwrap_err();
    assert_eq!(error.code, "lobby_not_ready");
    assert_eq!(error.candidates.len(), 1, "one player is not ready");
    let last = &mut party.players[4];
    last.session.set_ready(&mut last.endpoint, true).unwrap();
    party.run(40);
    party.host.start().unwrap();
    party.run_until(300, |p| {
        p.players.iter().all(|pl| {
            pl.world()
                .resource::<Lobby>()
                .is_some_and(|l| l.phase == LobbyPhase::Running && l.round == 1)
        })
    });

    // Two saboteurs among six; each machine knows its own role only.
    let roles = *party.host.roles();
    assert_eq!(roles[..6].iter().filter(|r| **r == 1).count(), 2);
    assert!(roles[6..].iter().all(|r| *r == 0));
    let host_roles = party.app.world().resource::<MatchRoles>().copied().unwrap();
    assert_eq!(host_roles.roles, roles);
    party.run_until(120, |p| {
        p.players
            .iter()
            .all(|pl| pl.local().role == roles[pl.session.seat().unwrap() as usize])
    });
    for player in &party.players {
        let local = player.local();
        let expected = if local.role == 1 { SABOTEUR } else { CREW };
        assert_eq!(player.world().text(local.role_name), Some(expected));
        assert!(
            player.world().resource::<MatchRoles>().is_none(),
            "a player's machine never has every role"
        );
    }
    let host_local = party
        .app
        .world()
        .resource::<LocalPlayer>()
        .copied()
        .unwrap();
    assert!(host_local.host && host_local.joined);
    assert_eq!(host_local.role, roles[0]);

    // Nobody new joins a running match.
    let late = party.join("Late", 1);
    party.run_until(300, |p| p.players[late].session.closed().is_some());
    assert_eq!(
        party.players[late].session.closed(),
        Some(wish_types::LobbyClosed::Refused)
    );
    party.players.pop();
    party.player_hashes.pop();

    // The match ends and the lobby reopens.
    party.host.end_match(2);
    party.run_until(300, |p| {
        p.players.iter().all(|pl| {
            pl.world()
                .resource::<Lobby>()
                .is_some_and(|l| l.phase == LobbyPhase::Ended && l.outcome == 2)
        })
    });
    assert_eq!(party.host.start().unwrap_err().code, "lobby_wrong_phase");
    party.host.reopen();
    party.run_until(300, |p| {
        p.players.iter().all(|pl| {
            pl.world()
                .resource::<Lobby>()
                .is_some_and(|l| l.phase == LobbyPhase::Open)
                && pl.local().role == 0
        })
    });
    let lobby = party.host.lobby();
    assert!(lobby.seats[..6].iter().all(|s| s.taken && !s.ready));
    assert_eq!(lobby.round, 1);
    assert_eq!(*party.host.roles(), [0; 16]);
    let diagnostics = party.host.take_diagnostics();
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code == "lobby_match_running" || d.code == "lobby_not_ready"),
        "{diagnostics:?}"
    );
    party.check_replay();
}

/// Which lobby buttons the test presses next on one machine.
#[wish_schema::resource]
pub struct Press {
    /// Rename to "Kimberly".
    pub name: bool,
    /// Mark ready.
    pub ready: bool,
    /// Start the match.
    pub start: bool,
}

/// A lobby screen's buttons: game code sends `LobbyRequest`s.
fn buttons(
    world: &mut wish_ecs::World,
    _: &mut wish_app::SystemContext<'_>,
) -> wish_core::Result<()> {
    use wish_types::{LobbyAction, LobbyRequest};
    let Some(press) = world.resource::<Press>().copied() else {
        return Ok(());
    };
    let request = |action, text| LobbyRequest {
        action,
        value: 0,
        text,
    };
    if press.name {
        let text = world.intern("Kimberly")?;
        world.send(request(LobbyAction::Name, text))?;
    }
    if press.ready {
        world.send(request(LobbyAction::Ready, wish_core::Text::EMPTY))?;
    }
    if press.start {
        world.send(request(LobbyAction::Start, wish_core::Text::EMPTY))?;
    }
    world.insert_resource(Press::default())
}

fn press(app: &mut wish_app::App, value: Press) {
    app.world_mut().insert_resource(value).unwrap();
}

#[test]
fn game_code_drives_the_lobby_with_requests() {
    let mut party = Party::new(SEED ^ 1, LinkConditions::PERFECT, lobby_config(2));
    party.join("Kim", 5);
    party
        .app
        .add_system(wish_app::System::new("buttons", buttons))
        .unwrap();
    party.players[0]
        .app
        .add_system(wish_app::System::new("buttons", buttons))
        .unwrap();
    party.run_until(300, Party::all_seated);
    // A player's game code: a new name, ready, and a start it may not ask
    // for.
    press(
        &mut party.players[0].app,
        Press {
            name: true,
            ready: true,
            start: true,
        },
    );
    party.run_until(300, |p| {
        let seat = &p.host.lobby().seats[1];
        seat.ready && seat.name == "Kimberly"
    });
    let diagnostics = party.players[0].session.take_diagnostics();
    assert!(
        diagnostics.iter().any(|d| d.code == "lobby_not_host"),
        "{diagnostics:?}"
    );
    assert_eq!(party.host.lobby().phase, LobbyPhase::Open);
    // The host's game code starts the match.
    press(
        &mut party.app,
        Press {
            start: true,
            ..Press::default()
        },
    );
    party.run_until(300, |p| p.host.lobby().phase == LobbyPhase::Running);
    party.run_until(300, |p| {
        p.players[0]
            .world()
            .resource::<Lobby>()
            .is_some_and(|l| l.phase == LobbyPhase::Running)
    });
}
