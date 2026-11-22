//! A host and two players over real UDP on this machine (127.0.0.1): they
//! join, get ready, the host starts, and a message goes up and comes back
//! down. The time comes from the test's own clock: the crates read none.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{Heard, game_app, lobby_config, net_config, room};
use wish_app::App;
use wish_lobby::{ClientSession, ClientSessionConfig, HostSession};
use wish_net::{Endpoint, Entropy, UdpTransport};
use wish_replicate::ReplicationConfig;
use wish_types::{Lobby, LobbyPhase};

fn localhost() -> std::net::SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[test]
fn a_lobby_and_a_meeting_over_localhost_udp() {
    let mut app = game_app();
    let mut endpoint = Endpoint::host(
        UdpTransport::bind(localhost()).unwrap(),
        net_config(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut host = HostSession::new(
        app.world(),
        lobby_config(3),
        ReplicationConfig::new(common::TYPES),
        room().as_str(),
        Entropy::os().unwrap(),
    )
    .unwrap();
    let mut players: Vec<(Endpoint<UdpTransport>, ClientSession, App)> = ["Ann", "Ben"]
        .iter()
        .map(|name| {
            let app = game_app();
            let session = ClientSession::new(
                app.world(),
                ClientSessionConfig {
                    name: (*name).to_owned(),
                    ..ClientSessionConfig::default()
                },
            );
            let endpoint = Endpoint::connect(
                UdpTransport::bind(localhost()).unwrap(),
                endpoint.local_addr(),
                net_config(),
                Entropy::os().unwrap(),
            )
            .unwrap();
            (endpoint, session, app)
        })
        .collect();
    let start = Instant::now();
    let mut readied = false;
    let mut started = false;
    let mut done = false;
    while start.elapsed() < Duration::from_secs(30) {
        let now = start.elapsed().as_millis() as u64;
        endpoint.update(now);
        let tick = app.tick();
        let mark = host
            .begin_tick(app.world_mut(), &mut endpoint, tick, &Vec::new())
            .unwrap();
        app.step().unwrap();
        host.end_tick(app.world_mut(), &mut endpoint, mark).unwrap();
        endpoint.update(now);
        for (endpoint, session, app) in &mut players {
            endpoint.update(now);
            let mark = session.begin_tick(app.world_mut(), endpoint).unwrap();
            let own = app.tick();
            app.step().unwrap();
            session.end_tick(app.world(), endpoint, mark, own).unwrap();
            if session.seat().is_some() && !readied {
                session.set_ready(endpoint, true).unwrap();
            }
            endpoint.update(now);
        }
        if !readied && players.iter().all(|(_, s, _)| s.seat().is_some()) {
            readied = true;
        }
        if readied && !started && host.lobby().seats[1..3].iter().all(|s| s.ready) {
            host.start().unwrap();
            started = true;
        }
        let meeting = app.world().resource::<common::Meeting>().copied();
        if meeting.is_some_and(|m| m.done)
            && players.iter().all(|(_, _, a)| {
                a.world()
                    .resource::<Heard>()
                    .is_some_and(|h| h.results == 1)
            })
        {
            done = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(
        done,
        "the meeting did not finish over UDP (tick {})",
        app.tick()
    );
    let meeting = app.world().resource::<common::Meeting>().copied().unwrap();
    assert_eq!(meeting.lines, 9, "three players, three lines each");
    for (_, session, app) in &players {
        let heard = app.world().resource::<Heard>().copied().unwrap();
        assert_eq!(heard.digest, meeting.digest);
        assert!(
            app.world()
                .resource::<Lobby>()
                .is_some_and(|l| l.phase == LobbyPhase::Running)
        );
        assert_eq!(session.stats().dropped_invalid, 0);
    }
    assert!(host.take_diagnostics().is_empty());
}
