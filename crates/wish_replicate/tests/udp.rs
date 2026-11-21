//! A host and two clients over real UDP on this machine (127.0.0.1). The
//! time comes from the test's own clock: the crates read none.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use wish_ecs::World;
use wish_net::{Endpoint, Entropy, UdpTransport};
use wish_replicate::{Client, ClientConfig, Host};
use wish_types::Transform;

fn localhost() -> std::net::SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn a_host_and_two_clients_replicate_over_localhost_udp() {
    let (mut world, cast) = common::host_world(2);
    let mut endpoint = Endpoint::host(
        UdpTransport::bind(localhost()).unwrap(),
        common::net_config(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut host = Host::new(&world, common::replication()).unwrap();
    let mut clients: Vec<(Endpoint<UdpTransport>, Client, World)> = (0..2)
        .map(|_| {
            let mut world = World::new();
            common::register(&mut world);
            let endpoint = Endpoint::connect(
                UdpTransport::bind(localhost()).unwrap(),
                endpoint.local_addr(),
                common::net_config(),
                Entropy::os().unwrap(),
            )
            .unwrap();
            (endpoint, Client::new(ClientConfig::default()), world)
        })
        .collect();
    let start = Instant::now();
    let mut tick = 0u64;
    let mut current_since: Option<u64> = None;
    while start.elapsed() < Duration::from_secs(20) {
        let now = start.elapsed().as_millis() as u64;
        endpoint.update(now);
        host.pump(&mut world, &mut endpoint);
        tick += 1;
        host.apply_inputs(&mut world, tick).unwrap();
        common::move_avatars(&mut world);
        host.send(&world, tick, &mut endpoint).unwrap();
        endpoint.update(now);
        let mut all = host.players().len() == 2;
        for (endpoint, client, world) in &mut clients {
            endpoint.update(now);
            client.pump(world, endpoint);
            if let Some(player) = client.player() {
                let stamp = client.stamp();
                // Walk for 100 ticks, then stand still.
                let actions = if stamp < 100 {
                    common::bot(player, stamp)
                } else {
                    Vec::new()
                };
                client.send_input(endpoint, actions).unwrap();
                all &= host
                    .target(player)
                    .is_some_and(|t| *t == client.mirror(world).unwrap());
            } else {
                all = false;
            }
            endpoint.update(now);
        }
        // Current for a while, after the avatars moved.
        if all && tick > 120 {
            let since = *current_since.get_or_insert(tick);
            if tick - since > 10 {
                break;
            }
        } else {
            current_since = None;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(
        current_since.is_some(),
        "the clients never caught up (tick {tick})"
    );
    // Player 1 walked (its bot moves along x for 100 ticks).
    let avatar = world.get::<Transform>(cast.avatars[&1]).unwrap();
    assert!(avatar.translation.x > 1.0, "{:?}", avatar.translation);
    assert!(host.take_diagnostics().is_empty());
}
