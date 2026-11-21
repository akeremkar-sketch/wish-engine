//! One host world and several client worlds stepped together over the
//! network simulator: exactness, determinism, loss, joining late, budget.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Match, SEED, Secret, Treasure, replication};
use wish_core::math::Vec3;
use wish_net::LinkConditions;
use wish_replicate::ReplicationConfig;
use wish_types::{Replicated, Transform};

fn lan() -> LinkConditions {
    LinkConditions {
        latency_ms: 40,
        jitter_ms: 0,
        loss: 0.0,
        duplicate: 0.0,
        reorder: 0.0,
    }
}

#[test]
fn with_no_loss_every_applied_tick_equals_the_visible_subset() {
    let mut game = Match::new(SEED, lan(), 3, replication());
    game.run(400);
    // Every client applied (and matched) almost every tick once joined.
    for player in 1..=3 {
        let exact = game.exact.get(&player).copied().unwrap_or(0);
        assert!(exact > 350, "player {player} matched only {exact} ticks");
    }
    assert_eq!(game.host.stats().deferred, 0);
    assert!(game.all_current());
    assert!(game.host.take_diagnostics().is_empty());

    // What each player sees: everyone's avatars and the beacon; only its
    // own secret; the treasure only for player 1 (who walked to it, and is
    // back); the ghost light only for player 3.
    let avatars = game.cast.avatars.clone();
    for player in 1..=3u32 {
        let peer = game.peer(player);
        let has = |entity| {
            game.host
                .network_ids(player)
                .iter()
                .find(|(_, e)| *e == entity)
                .and_then(|(net, _)| peer.client.entity(*net))
                .is_some_and(|local| peer.world.contains(local))
        };
        for avatar in avatars.values() {
            assert!(has(*avatar));
        }
        for (owner, secret) in &game.cast.secrets {
            assert_eq!(
                has(*secret),
                *owner == player,
                "player {player}, secret of {owner}"
            );
        }
        assert_eq!(has(game.cast.ghost.unwrap()), player == 3);
        assert!(has(game.cast.beacon.unwrap()));
        let mine = common::all::<Secret>(&peer.world)
            .iter()
            .map(|s| s.marker)
            .collect::<Vec<_>>();
        assert_eq!(mine, vec![common::secret_marker(player)]);
    }
    // Player 1 saw the treasure on the way, and it left the view when the
    // player walked back: nobody holds it now.
    assert!(
        game.records
            .iter()
            .any(|((p, _), view)| *p == 1 && view.values().any(|e| e.contains_key(&4)))
    );
    for peer in &game.peers {
        assert!(common::all::<Treasure>(&peer.world).is_empty());
    }
}

#[test]
fn the_beacon_refers_to_player_twos_secret_only_for_player_two() {
    let mut game = Match::new(SEED, lan(), 3, replication());
    game.run(60);
    for player in 1..=3u32 {
        let peer = game.peer(player);
        let mut follows = common::all::<common::Follow>(&peer.world)
            .iter()
            .map(|f| f.target)
            .collect::<Vec<_>>();
        assert_eq!(follows.len(), 1);
        let target = follows.pop().unwrap();
        if player == 2 {
            let secret = peer.client.network_id(target).unwrap();
            assert_eq!(
                game.host.resolve(2, secret),
                game.cast.secrets.get(&2).copied()
            );
            assert_eq!(
                peer.world.get::<Secret>(target).unwrap().marker,
                common::secret_marker(2)
            );
        } else {
            assert!(
                target.is_null(),
                "player {player} learned where a secret is"
            );
        }
    }
}

fn run_hashes(seed: u64) -> (Vec<Vec<u128>>, u128) {
    let mut game = Match::new(seed, LinkConditions::BAD, 3, replication());
    game.check_exact = false;
    game.run(300);
    (game.hashes, game.net.delivery_hash().as_u128())
}

#[test]
fn the_same_seed_gives_the_same_client_worlds_byte_for_byte() {
    let (a, net_a) = run_hashes(SEED);
    let (b, net_b) = run_hashes(SEED);
    assert_eq!(net_a, net_b);
    assert_eq!(a, b);
    let (c, net_c) = run_hashes(SEED + 1);
    assert_ne!(net_a, net_c);
    assert_ne!(a.last(), c.last());
}

#[test]
fn under_loss_latency_and_reordering_clients_stay_exact_and_converge() {
    let mut game = Match::new(SEED, LinkConditions::BAD, 3, replication());
    game.run(300);
    // Bots stop at tick 300; the network keeps losing packets.
    game.run(150);
    assert_eq!(game.host.stats().deferred, 0);
    for player in 1..=3 {
        // Every time the newest state was read whole and nothing newer was
        // shown, the world equalled the host's target (checked in `step`).
        // About 5 % of states are lost and jitter reorders many more, so a
        // newer state is often already shown when an older one arrives.
        let exact = game.exact.get(&player).copied().unwrap_or(0);
        assert!(
            exact > 200,
            "player {player} matched only {exact} of 450 ticks"
        );
        let peer = game.peer(player);
        let whole = peer.client.stats().states_whole;
        assert!(
            whole > 350,
            "player {player} read only {whole} states whole"
        );
        let lag = game.tick - peer.client.applied_tick().unwrap();
        assert!(lag < 30, "player {player} lags {lag} ticks");
    }
    assert!(game.all_current(), "the clients converged");
    // Inputs went the other way under loss too: the host applied recent ones.
    for player in [1, 2] {
        assert!(game.host.applied_input(player).unwrap() > 250);
    }
    // The avatars moved on the host as the bots asked.
    let first = game.world.get::<Transform>(game.cast.avatars[&1]).unwrap();
    assert!(first.translation.distance(Vec3::new(0.0, 10.0, 0.0)) < 3.0);
}

#[test]
fn a_player_joins_a_running_match_and_gets_the_whole_visible_state() {
    let mut game = Match::new(SEED, LinkConditions::BAD, 3, replication());
    game.run(120);
    // A fourth player's body appears on the host, then the player connects.
    let mut cast = game.cast.clone();
    common::add_player(&mut game.world, &mut cast, 4);
    game.cast = cast;
    game.join();
    game.run(300);
    let joined = game.peer(4);
    assert!(joined.client.stats().states_whole > 100);
    assert!(game.exact.get(&4).copied().unwrap_or(0) > 100);
    assert_eq!(game.host.stats().snapshots_sent, 4);
    assert!(game.all_current());
    // The newcomer walked: its input reached the host.
    let avatar = game.world.get::<Transform>(game.cast.avatars[&4]).unwrap();
    assert!(avatar.translation.x > 1.0);
}

#[test]
fn over_budget_the_order_follows_priority_and_the_world_converges() {
    let mut config = ReplicationConfig::new(&["Transform", "Treasure"]);
    config.budget_bytes = 256;
    let mut game = Match::new(SEED, lan(), 1, config);
    game.check_exact = false;
    // 200 crates everyone sees; the even ones matter four times as much.
    let mut crates = Vec::new();
    for i in 0..200u32 {
        let entity = game.world.spawn().unwrap();
        game.world
            .insert(
                entity,
                Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)),
            )
            .unwrap();
        game.world
            .insert(
                entity,
                Replicated {
                    priority: if i % 2 == 0 { 4.0 } else { 1.0 },
                    ..Replicated::default()
                },
            )
            .unwrap();
        game.world
            .insert(
                entity,
                Treasure {
                    marker: u64::from(i),
                },
            )
            .unwrap();
        crates.push(entity);
    }
    game.run(60);
    let peer = &game.peers[0];
    let player = peer.client.player().unwrap();
    let net_of = |entity| {
        game.host
            .network_ids(player)
            .into_iter()
            .find(|(_, e)| *e == entity)
            .map(|(n, _)| n)
            .unwrap()
    };
    let nets: Vec<u32> = crates.iter().map(|c| net_of(*c)).collect();
    // Every crate moves every tick; count how often the client's copy moved.
    let mut last: Vec<f32> = vec![f32::NAN; crates.len()];
    let mut updates = vec![0u32; crates.len()];
    for tick in 0..300 {
        for (i, entity) in crates.iter().enumerate() {
            let mut t = game.world.get_mut::<Transform>(*entity).unwrap();
            t.translation.y = tick as f32 + i as f32;
        }
        game.step();
        let peer = &game.peers[0];
        for (i, net) in nets.iter().enumerate() {
            let y = peer
                .client
                .entity(*net)
                .and_then(|e| peer.world.get::<Transform>(e))
                .map_or(f32::NAN, |t| t.translation.y);
            if y.to_bits() != last[i].to_bits() {
                updates[i] += 1;
                last[i] = y;
            }
        }
    }
    let stats = game.host.stats();
    assert!(stats.deferred > 0, "the budget was binding");
    let even: u32 = updates.iter().step_by(2).sum();
    let odd: u32 = updates.iter().skip(1).step_by(2).sum();
    assert!(odd > 0, "low priority is not starved");
    let ratio = f64::from(even) / f64::from(odd);
    assert!((2.5..6.0).contains(&ratio), "priority ratio {ratio}");
    // Once still, everything arrives.
    game.run(200);
    assert!(game.all_current());
}
