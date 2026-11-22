//! The host's targets: a message to a group reaches only the players whose
//! `NetGroups` bits match (the host's own player included), one to a single
//! player only that player, one to a seat nobody holds nobody; a player's
//! machine sends only to the host (design note §3.3).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Party, SEED, Whisper, lobby_config};
use wish_app::{System, SystemContext};
use wish_core::Result;
use wish_ecs::World;
use wish_net::LinkConditions;
use wish_schema::resource;
use wish_types::{Net, NetGroups};

/// What the host's test system sends next.
#[resource]
pub struct Outbox {
    /// To the ghosts' group (bit 0).
    pub ghosts: bool,
    /// To player 2.
    pub two: bool,
    /// To player 9, who has no seat.
    pub nobody: bool,
    /// From a player's machine to everyone (refused there).
    pub everyone: bool,
}

fn sender(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    let Some(send) = world.resource::<Outbox>().copied() else {
        return Ok(());
    };
    let whisper = |net, secret| Whisper {
        net,
        partner: 0,
        secret,
    };
    if send.ghosts {
        world.send(whisper(Net::to_group(1), 1))?;
    }
    if send.two {
        world.send(whisper(Net::to_player(2), 2))?;
    }
    if send.nobody {
        world.send(whisper(Net::to_player(9), 3))?;
    }
    if send.everyone {
        world.send(whisper(Net::to_everyone(), 4))?;
    }
    world.insert_resource(Outbox::default())
}

#[test]
fn groups_one_player_and_nobody() {
    let mut party = Party::new(SEED ^ 0x6A, LinkConditions::BAD, lobby_config(4));
    for (name, color) in [("Ann", 1), ("Ben", 2), ("Cem", 3)] {
        party.join(name, color);
    }
    party.run_until(600, Party::all_seated);
    party.app.add_system(System::new("sender", sender)).unwrap();
    // The host's own player and seat 3 are ghosts.
    let mut groups = NetGroups::default();
    groups.players[0] = 1;
    groups.players[3] = 1;
    party.app.world_mut().insert_resource(groups).unwrap();
    party
        .app
        .world_mut()
        .insert_resource(Outbox {
            ghosts: true,
            two: true,
            nobody: true,
            everyone: false,
        })
        .unwrap();
    party.run(80);
    let heard = |seat: u32| {
        party
            .players
            .iter()
            .find(|p| p.session.seat() == Some(seat))
            .unwrap()
            .heard()
            .whispers
    };
    assert_eq!(heard(1), 0);
    assert_eq!(heard(2), 1, "the message to player 2");
    assert_eq!(heard(3), 1, "the ghosts' message");
    let host = party
        .app
        .world()
        .resource::<common::Heard>()
        .copied()
        .unwrap();
    assert_eq!(host.whispers, 1, "the host's own player is a ghost");
    assert_eq!(
        party.host.stats().dropped_offline,
        1,
        "player 9 has no seat"
    );

    // A player's game code may send only to the host.
    let player = &mut party.players[0];
    player
        .app
        .add_system(System::new("sender", sender))
        .unwrap();
    player
        .app
        .world_mut()
        .insert_resource(Outbox {
            everyone: true,
            ..Outbox::default()
        })
        .unwrap();
    party.run(40);
    let codes: Vec<String> = party.players[0]
        .session
        .take_diagnostics()
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(
        codes.iter().any(|c| c == "net_message_target_refused"),
        "{codes:?}"
    );
    assert_eq!(party.host.stats().messages_received, 0);
}
