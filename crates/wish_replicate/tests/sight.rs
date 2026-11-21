//! `NetVisibility::Sight` (design note 1.1-lights-2d §6): an entity behind a
//! wall is not sent to a player until the player's viewer can see it, and
//! is taken away again when it cannot; without walls the rule is `Near`.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Match, SEED, Treasure, replication};
use wish_core::Entity;
use wish_core::math::Vec3;
use wish_net::LinkConditions;
use wish_sight::Occluder;
use wish_types::physics::{Collider, ColliderShape};
use wish_types::{NetVisibility, Replicated, Transform};

const SPY_MARKER: u64 = 0x5317_0000_0000_00CC;

/// Player 1's avatar walks from (0, 10) along +X to (7.5, 10) and back
/// (common::bot). A wall stands at x = 4 from y = 10.5 to 14; a spy waits
/// behind it at (6, 12), seen by line of sight within 20 m.
fn game() -> (Match, Entity, Entity) {
    let mut game = Match::new(SEED, LinkConditions::PERFECT, 1, replication());
    let world = &mut game.world;
    world.register_component::<Collider>().unwrap();
    wish_sight::register(world).unwrap();
    let wall = world.spawn().unwrap();
    world
        .insert(
            wall,
            Transform::from_translation(Vec3::new(4.0, 12.25, 0.0)),
        )
        .unwrap();
    world
        .insert(
            wall,
            Collider {
                shape: ColliderShape::Box,
                size: Vec3::new(0.4, 3.5, 1.0),
                ..Collider::default()
            },
        )
        .unwrap();
    world.insert(wall, Occluder { enabled: true }).unwrap();
    let spy = world.spawn().unwrap();
    world
        .insert(spy, Transform::from_translation(Vec3::new(6.0, 12.0, 0.0)))
        .unwrap();
    world
        .insert(
            spy,
            Replicated {
                visibility: NetVisibility::Sight,
                radius: 20.0,
                ..Replicated::default()
            },
        )
        .unwrap();
    world.insert(spy, Treasure { marker: SPY_MARKER }).unwrap();
    (game, wall, spy)
}

/// Whether the host's target for player 1 holds `entity` now.
fn targeted(game: &Match, entity: Entity) -> bool {
    let Some(target) = game.host.target(1) else {
        return false;
    };
    game.host
        .network_ids(1)
        .iter()
        .any(|(net, e)| *e == entity && target.contains_key(net))
}

/// Whether player 1's client world holds the spy.
fn client_has_spy(game: &Match) -> bool {
    let Some(peer) = game.peers.iter().find(|p| p.client.player() == Some(1)) else {
        return false;
    };
    peer.world
        .entities()
        .filter_map(|e| peer.world.get::<Treasure>(e).copied())
        .any(|t| t.marker == SPY_MARKER)
}

/// Whether the avatar's centre can see the spy's, by the engine's walls.
fn line_free(game: &Match, spy: Entity) -> bool {
    let avatar = game.cast.avatars[&1];
    let at = |e: Entity| game.world.get::<Transform>(e).unwrap().translation;
    let (a, b) = (at(avatar), at(spy));
    a.distance(b) <= 20.0
        && wish_sight::current(&game.world)
            .can_see([a.x, a.y], [b.x, b.y], [Some(avatar), Some(spy)])
            .visible
}

#[test]
fn a_spy_behind_a_wall_is_sent_only_while_in_sight() {
    let (mut game, _, spy) = game();
    let mut seen_ticks = 0;
    let mut hidden_ticks = 0;
    let mut first_seen = None;
    let mut client_saw = false;
    let mut client_lost = false;
    for _ in 0..320 {
        game.step();
        if game.host.target(1).is_none() {
            continue;
        }
        let free = line_free(&game, spy);
        assert_eq!(
            targeted(&game, spy),
            free,
            "tick {}: the host's target holds the spy exactly while the line is free",
            game.tick
        );
        if free {
            seen_ticks += 1;
            first_seen.get_or_insert(game.tick);
        } else {
            hidden_ticks += 1;
        }
        let has = client_has_spy(&game);
        if first_seen.is_none() {
            assert!(
                !has,
                "tick {}: the client got the spy before it was in sight",
                game.tick
            );
        }
        client_saw |= has;
        if client_saw && !has {
            client_lost = true;
        }
    }
    assert!(
        seen_ticks > 20 && hidden_ticks > 20,
        "{seen_ticks} seen, {hidden_ticks} hidden"
    );
    assert!(client_saw, "the client received the spy once in sight");
    assert!(
        client_lost,
        "and lost it again when the wall came between them"
    );
    // The avatar at x = 0 was out of sight though within the radius: without
    // the wall it would have been sent from the start.
    let first = first_seen.unwrap();
    assert!(first > 40, "seen at tick {first}");
}

#[test]
fn without_walls_sight_is_near() {
    let (mut game, wall, spy) = game();
    game.world
        .insert(wall, Occluder { enabled: false })
        .unwrap();
    game.run(30);
    assert!(
        targeted(&game, spy),
        "nothing in the way: within 20 m is enough"
    );
    assert!(client_has_spy(&game));
    // And beyond the radius, never.
    game.world
        .insert(spy, Transform::from_translation(Vec3::new(60.0, 12.0, 0.0)))
        .unwrap();
    game.run(10);
    assert!(!targeted(&game, spy));
    assert!(!client_has_spy(&game));
}
