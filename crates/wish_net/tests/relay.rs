//! The relay over the network simulator: a relay node between a host and
//! its clients, on good and bad links; rooms that expire and clients that
//! come early; the relay never holding plaintext; and hostile traffic at
//! the relay (design note `docs/design/1.1-relay.md` §6).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::net::SocketAddr;

use common::{code, config, index_of, message};
use wish_core::{Hash128, Rng};
use wish_net::relay::envelope::{self, Envelope};
use wish_net::relay::{
    MAX_RELAY_DATAGRAM, Refusal, Relay, RelayClient, RelayConfig, RelayEvent, RelayHost,
    RelayState, RoomClosed, locator,
};
use wish_net::wire::{self, Packet};
use wish_net::{
    Channel, Endpoint, Entropy, Event, LinkConditions, PeerId, Recv, RoomCode, SimNetwork,
    SimTransport, Transport,
};

type HostEndpoint = Endpoint<RelayHost<SimTransport>>;
type ClientEndpoint = Endpoint<RelayClient<SimTransport>>;

struct World {
    net: SimNetwork,
    relay: Relay<SimTransport>,
    relay_addr: SocketAddr,
    host: HostEndpoint,
    clients: Vec<ClientEndpoint>,
    relay_events: Vec<RelayEvent>,
}

#[derive(Default)]
struct Events {
    host: Vec<Event>,
    clients: Vec<Vec<Event>>,
}

fn host_through(net: &SimNetwork, relay: SocketAddr, code: &RoomCode, seed: u64) -> HostEndpoint {
    let mut entropy = Entropy::seeded(seed ^ 0x40);
    let transport = RelayHost::new(net.add_node(), relay, code, &mut entropy);
    Endpoint::host(
        transport,
        wish_net::Config::new(code.clone()),
        Entropy::seeded(seed ^ 0x41),
    )
    .unwrap()
}

fn client_through(
    net: &SimNetwork,
    relay: SocketAddr,
    code: &RoomCode,
    seed: u64,
) -> ClientEndpoint {
    Endpoint::connect(
        RelayClient::new(net.add_node(), relay, code),
        relay,
        wish_net::Config::new(code.clone()),
        Entropy::seeded(seed),
    )
    .unwrap()
}

fn world_with(seed: u64, conditions: LinkConditions, clients: usize, relay: RelayConfig) -> World {
    let net = SimNetwork::new(seed, 16);
    net.set_conditions(conditions);
    let relay_node = net.add_node();
    let relay_addr = relay_node.local_addr();
    let relay = Relay::new(relay_node, relay, Entropy::seeded(seed ^ 0x30)).unwrap();
    let host = host_through(&net, relay_addr, &code(), seed);
    let clients = (0..clients)
        .map(|i| client_through(&net, relay_addr, &code(), seed ^ (0x100 + i as u64)))
        .collect();
    World {
        net,
        relay,
        relay_addr,
        host,
        clients,
        relay_events: Vec::new(),
    }
}

fn world(seed: u64, conditions: LinkConditions, clients: usize) -> World {
    world_with(seed, conditions, clients, RelayConfig::default())
}

impl World {
    /// One tick: the network moves, then the relay, the host and every
    /// client update.
    fn step(&mut self) -> Events {
        self.net.step();
        let now = self.net.now_ms();
        self.relay.update(now);
        while let Some(event) = self.relay.poll_event() {
            self.relay_events.push(event);
        }
        self.host.update(now);
        let mut events = Events {
            host: std::iter::from_fn(|| self.host.poll_event()).collect(),
            clients: Vec::new(),
        };
        for client in &mut self.clients {
            client.update(now);
            events
                .clients
                .push(std::iter::from_fn(|| client.poll_event()).collect());
        }
        events
    }

    /// Steps until the host and every client are connected; the host's
    /// peer of each client, in client order.
    fn connect_all(&mut self) -> Vec<PeerId> {
        let mut host_peers = Vec::new();
        let mut up = vec![false; self.clients.len()];
        for _ in 0..4000 {
            let events = self.step();
            for event in &events.host {
                if let Event::Connected { peer } = event {
                    host_peers.push(*peer);
                }
            }
            for (i, events) in events.clients.iter().enumerate() {
                for event in events {
                    match event {
                        Event::Connected { peer } if *peer == PeerId::HOST => up[i] = true,
                        Event::Disconnected { .. } => panic!("client {i}: {event:?}"),
                        _ => {}
                    }
                }
            }
            if up.iter().all(|u| *u) && host_peers.len() == self.clients.len() {
                // Which peer is which client: the id each client was given.
                return self.clients.iter().map(|c| c.local_id().unwrap()).collect();
            }
        }
        panic!(
            "not everyone connected: clients {up:?}, host peers {host_peers:?}, relay {:?}",
            self.relay.stats()
        );
    }
}

fn assert_no_disconnect(events: &Events) {
    for event in &events.host {
        assert!(
            !matches!(event, Event::Disconnected { .. }),
            "host: {event:?}"
        );
    }
    for (i, events) in events.clients.iter().enumerate() {
        for event in events {
            assert!(
                !matches!(event, Event::Disconnected { .. }),
                "client {i}: {event:?}"
            );
        }
    }
}

const MESSAGES: u32 = 60;

fn size(i: u32) -> usize {
    4 + (i as usize * 977) % 6000
}

struct Run {
    hash: Hash128,
    to_host: Vec<Vec<Vec<u8>>>,
    to_clients: Vec<Vec<Vec<u8>>>,
    resent: u64,
}

/// A host and three clients through a relay: every client sends
/// [`MESSAGES`] reliable messages to the host and the host as many to every
/// client.
fn exchange(seed: u64, conditions: LinkConditions) -> Run {
    let mut w = world(seed, conditions, 3);
    let peers = w.connect_all();
    let n = w.clients.len();
    let mut to_host: Vec<Vec<Vec<u8>>> = vec![Vec::new(); n];
    let mut to_clients: Vec<Vec<Vec<u8>>> = vec![Vec::new(); n];
    for i in 0..MESSAGES {
        for (c, client) in w.clients.iter_mut().enumerate() {
            let up = message(i + 1000 * c as u32, size(i));
            client.send(PeerId::HOST, Channel::Reliable, &up).unwrap();
            let down = message(i + 100_000 + 1000 * c as u32, size(i + 7));
            w.host.send(peers[c], Channel::Reliable, &down).unwrap();
        }
    }
    for _ in 0..30_000 {
        let events = w.step();
        assert_no_disconnect(&events);
        for event in &events.host {
            if let Event::Message {
                peer,
                channel: Channel::Reliable,
                data,
            } = event
            {
                let c = peers.iter().position(|p| p == peer).unwrap();
                to_host[c].push(data.clone());
            }
        }
        for (c, events) in events.clients.iter().enumerate() {
            to_clients[c].extend(common::messages(events, Channel::Reliable));
        }
        if to_host
            .iter()
            .chain(&to_clients)
            .all(|got| got.len() == MESSAGES as usize)
        {
            break;
        }
    }
    for c in 0..n {
        assert_eq!(to_host[c].len(), MESSAGES as usize, "to host from {c}");
        assert_eq!(to_clients[c].len(), MESSAGES as usize, "to client {c}");
        for i in 0..MESSAGES {
            assert_eq!(
                to_host[c][i as usize],
                message(i + 1000 * c as u32, size(i)),
                "to host from {c}, #{i}"
            );
            assert_eq!(
                to_clients[c][i as usize],
                message(i + 100_000 + 1000 * c as u32, size(i + 7)),
                "to client {c}, #{i}"
            );
        }
    }
    let resent = w.host.stats().resent + w.clients.iter().map(|c| c.stats().resent).sum::<u64>();
    Run {
        hash: w.net.delivery_hash(),
        to_host,
        to_clients,
        resent,
    }
}

#[test]
fn a_relay_carries_a_host_and_three_clients_over_bad_links() {
    let run = exchange(3, LinkConditions::BAD);
    assert!(
        run.resent > 0,
        "5 % loss on both legs must have made resends"
    );
}

#[test]
fn the_same_seed_gives_the_same_delivery_through_a_relay() {
    let a = exchange(11, LinkConditions::BAD);
    let b = exchange(11, LinkConditions::BAD);
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.to_host, b.to_host);
    assert_eq!(a.to_clients, b.to_clients);
    let c = exchange(12, LinkConditions::BAD);
    assert_ne!(a.hash, c.hash);
}

#[test]
fn unreliable_messages_go_through_a_relay_at_most_once_and_whole() {
    let mut w = world(5, LinkConditions::BAD, 1);
    let peers = w.connect_all();
    let mut got = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..400u32 {
        if i < 200 {
            w.host
                .send(
                    peers[0],
                    Channel::Unreliable,
                    &message(i, 4 + (i as usize * 37) % 3000),
                )
                .unwrap();
        }
        let events = w.step();
        got.extend(common::messages(&events.clients[0], Channel::Unreliable));
    }
    for data in &got {
        let i = index_of(data);
        assert!(seen.insert(i), "#{i} twice");
        assert_eq!(data, &message(i, 4 + (i as usize * 37) % 3000));
    }
    assert!(got.len() > 100, "most arrive: {}", got.len());
}

#[test]
fn a_host_takes_direct_and_relayed_clients_at_once() {
    let mut w = world(8, LinkConditions::PERFECT, 1);
    let direct_addr = w.host.local_addr();
    let mut direct =
        Endpoint::connect(w.net.add_node(), direct_addr, config(), Entropy::seeded(99)).unwrap();
    let mut direct_up = false;
    let mut relayed_up = false;
    let mut host_peers = Vec::new();
    for _ in 0..2000 {
        let events = w.step();
        direct.update(w.net.now_ms());
        while let Some(event) = direct.poll_event() {
            direct_up |= matches!(event, Event::Connected { peer: PeerId::HOST });
        }
        relayed_up |= events.clients[0]
            .iter()
            .any(|e| matches!(e, Event::Connected { peer: PeerId::HOST }));
        for event in &events.host {
            if let Event::Connected { peer } = event {
                host_peers.push(*peer);
            }
        }
        if host_peers.len() == 2 && direct_up && relayed_up {
            break;
        }
    }
    assert!(
        direct_up && relayed_up && host_peers.len() == 2,
        "{host_peers:?}"
    );
    let relayed: Vec<_> = host_peers
        .iter()
        .filter(|p| {
            w.host
                .peer_addr(**p)
                .is_some_and(|a| wish_net::relay::virtual_slot(a).is_some())
        })
        .collect();
    assert_eq!(relayed.len(), 1, "one relayed and one direct peer");
    direct
        .send(PeerId::HOST, Channel::Reliable, b"direct")
        .unwrap();
    w.clients[0]
        .send(PeerId::HOST, Channel::Reliable, b"relayed")
        .unwrap();
    let mut got = Vec::new();
    for _ in 0..50 {
        let events = w.step();
        direct.update(w.net.now_ms());
        got.extend(common::messages(&events.host, Channel::Reliable));
    }
    got.sort();
    assert_eq!(got, vec![b"direct".to_vec(), b"relayed".to_vec()]);
}

#[test]
fn a_room_expires_when_its_host_goes_quiet_and_comes_back_when_it_returns() {
    let mut w = world(9, LinkConditions::PERFECT, 1);
    w.connect_all();
    assert_eq!((w.relay.rooms(), w.relay.clients()), (1, 1));
    // The host stops: only the relay and the client move.
    let idle_ticks = (RelayConfig::default().room_idle_ms / 16) as usize + 100;
    for _ in 0..idle_ticks {
        w.net.step();
        let now = w.net.now_ms();
        w.relay.update(now);
        w.clients[0].update(now);
    }
    while let Some(event) = w.relay.poll_event() {
        w.relay_events.push(event);
    }
    assert_eq!(w.relay.rooms(), 0);
    assert_eq!(w.relay.clients(), 0);
    assert!(w.relay_events.iter().any(|e| matches!(
        e,
        RelayEvent::RoomClosed {
            reason: RoomClosed::Idle,
            ..
        }
    )));
    // The host comes back and registers again (its old cookie is stale).
    for _ in 0..300 {
        w.step();
    }
    assert_eq!(w.relay.rooms(), 1);
    assert_eq!(w.host.transport().relay_state(), RelayState::Ready);
}

#[test]
fn a_client_that_comes_first_gets_in_when_the_host_arrives() {
    let net = SimNetwork::new(4, 16);
    let relay_node = net.add_node();
    let relay_addr = relay_node.local_addr();
    let mut relay = Relay::new(relay_node, RelayConfig::default(), Entropy::seeded(1)).unwrap();
    let mut client = client_through(&net, relay_addr, &code(), 2);
    for _ in 0..60 {
        net.step();
        relay.update(net.now_ms());
        client.update(net.now_ms());
    }
    let state = client.transport().relay_state();
    assert_eq!(state, RelayState::Refused(Refusal::NoRoom));
    assert_eq!(state.diagnostic().unwrap().code, "net_relay_no_room");
    let mut host = host_through(&net, relay_addr, &code(), 3);
    let mut connected = false;
    for _ in 0..400 {
        net.step();
        let now = net.now_ms();
        relay.update(now);
        host.update(now);
        client.update(now);
        while let Some(event) = client.poll_event() {
            connected |= matches!(event, Event::Connected { peer: PeerId::HOST });
        }
        while host.poll_event().is_some() {}
        if connected {
            break;
        }
    }
    assert!(connected);
    assert_eq!(client.transport().relay_state(), RelayState::Ready);
}

#[test]
fn a_wrong_room_code_reaches_no_room_and_a_right_one_with_a_wrong_host_key_is_refused() {
    let mut w = world(10, LinkConditions::PERFECT, 0);
    for _ in 0..20 {
        w.step();
    }
    let other = RoomCode::parse("PLUM77").unwrap();
    let mut lost = client_through(&w.net, w.relay_addr, &other, 1);
    for _ in 0..60 {
        w.step();
        lost.update(w.net.now_ms());
    }
    assert_eq!(
        lost.transport().relay_state(),
        RelayState::Refused(Refusal::NoRoom)
    );
    // The relay carries, the host decides: a pinned key that is not the
    // host's is refused end to end, as without a relay.
    let mut pinned_config = config();
    pinned_config.host_key = Some([1; 32]);
    let mut pinned = Endpoint::connect(
        RelayClient::new(w.net.add_node(), w.relay_addr, &code()),
        w.relay_addr,
        pinned_config,
        Entropy::seeded(5),
    )
    .unwrap();
    let mut reason = None;
    for _ in 0..200 {
        w.step();
        pinned.update(w.net.now_ms());
        while let Some(event) = pinned.poll_event() {
            if let Event::Disconnected { reason: r, .. } = event {
                reason = Some(r);
            }
        }
    }
    assert_eq!(reason.map(|r| r.code()), Some("net_host_key_mismatch"));
}

#[test]
fn the_relay_never_sees_plaintext() {
    let mut w = world(21, LinkConditions::PERFECT, 2);
    w.net.start_capture();
    let peers = w.connect_all();
    let marker = |i: u32| format!("PLAINTEXT-MARKER-{i:04}-").repeat(40).into_bytes();
    for i in 0..20u32 {
        w.clients[0]
            .send(PeerId::HOST, Channel::Reliable, &marker(i))
            .unwrap();
        w.host
            .send(peers[1], Channel::Reliable, &marker(i + 500))
            .unwrap();
        w.host
            .send(peers[0], Channel::Unreliable, &marker(i + 900))
            .unwrap();
    }
    let mut to_host = 0;
    let mut to_client = 0;
    for _ in 0..200 {
        let events = w.step();
        to_host += common::messages(&events.host, Channel::Reliable).len();
        to_client += common::messages(&events.clients[1], Channel::Reliable).len();
    }
    assert_eq!((to_host, to_client), (20, 20), "the messages arrived");
    let captured = w.net.take_captured();
    let code = code();
    let secrets: Vec<Vec<u8>> = vec![
        code.as_str().as_bytes().to_vec(),
        code.as_str().to_ascii_lowercase().into_bytes(),
        b"PLAINTEXT-MARKER".to_vec(),
    ];
    let mut through_relay = 0;
    let mut inner_kinds = std::collections::BTreeSet::new();
    for datagram in &captured {
        for secret in &secrets {
            assert!(
                !datagram
                    .bytes
                    .windows(secret.len())
                    .any(|w| w == secret.as_slice()),
                "a datagram from {} to {} holds {:?} in the clear",
                datagram.from,
                datagram.to,
                String::from_utf8_lossy(secret)
            );
        }
        if datagram.from != w.relay_addr && datagram.to != w.relay_addr {
            continue;
        }
        through_relay += 1;
        let envelope = envelope::decode(&datagram.bytes)
            .unwrap_or_else(|| panic!("not an envelope: {:?}", datagram.bytes));
        let inner = match envelope {
            Envelope::HostSend { inner, .. }
            | Envelope::ToHost { inner, .. }
            | Envelope::ClientSend { inner }
            | Envelope::ToClient { inner } => inner,
            _ => continue,
        };
        // Every carried datagram is a transport packet: a handshake packet
        // (whose secret parts are sealed) or a sealed data packet.
        let kind = match wire::decode(inner).expect("a transport packet") {
            Packet::Hello { .. } => "hello",
            Packet::Challenge { .. } => "challenge",
            Packet::Connect { .. } => "connect",
            Packet::Data { .. } => "data",
            other => panic!("unexpected inner packet {other:?}"),
        };
        inner_kinds.insert(kind);
    }
    assert!(through_relay > 100, "{through_relay}");
    assert_eq!(
        inner_kinds.into_iter().collect::<Vec<_>>(),
        vec!["challenge", "connect", "data", "hello"]
    );
}

// --- Hostile traffic -------------------------------------------------------

/// A node that speaks the relay protocol by hand.
struct Raw {
    node: SimTransport,
    relay: SocketAddr,
}

impl Raw {
    fn new(net: &SimNetwork, relay: SocketAddr) -> Self {
        Self {
            node: net.add_node(),
            relay,
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.node.send(self.relay, bytes).unwrap();
    }

    fn inbox(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 4096];
        while let Recv::Datagram { len, .. } = self.node.recv(&mut buf) {
            out.push(buf[..len].to_vec());
        }
        out
    }
}

fn decoded(datagrams: &[Vec<u8>]) -> Vec<Envelope<'_>> {
    datagrams
        .iter()
        .filter_map(|d| envelope::decode(d))
        .collect()
}

/// Attaches `raw` to the room of `code` by hand.
fn attach_by_hand(w: &mut World, raw: &mut Raw, code: &RoomCode) {
    let l = locator(code);
    raw.send(&envelope::attach(&l, &[0; 16]));
    w.step();
    w.step();
    let inbox = raw.inbox();
    let cookie = decoded(&inbox)
        .into_iter()
        .find_map(|e| match e {
            Envelope::AttachChallenge { cookie, .. } => Some(cookie),
            _ => None,
        })
        .expect("a challenge");
    raw.send(&envelope::attach(&l, &cookie));
    w.step();
    w.step();
    let inbox = raw.inbox();
    assert!(
        decoded(&inbox)
            .iter()
            .any(|e| matches!(e, Envelope::Attached { .. })),
        "attached"
    );
}

/// The honest match still talks: a message each way between the host and
/// every client.
fn assert_still_talks(w: &mut World, peers: &[PeerId]) {
    for (c, client) in w.clients.iter_mut().enumerate() {
        client
            .send(
                PeerId::HOST,
                Channel::Reliable,
                format!("up {c}").as_bytes(),
            )
            .unwrap();
        w.host
            .send(peers[c], Channel::Reliable, format!("down {c}").as_bytes())
            .unwrap();
    }
    let mut up = Vec::new();
    let mut down = vec![Vec::new(); w.clients.len()];
    for _ in 0..400 {
        let events = w.step();
        assert_no_disconnect(&events);
        up.extend(common::messages(&events.host, Channel::Reliable));
        for (c, events) in events.clients.iter().enumerate() {
            down[c].extend(common::messages(events, Channel::Reliable));
        }
    }
    for (c, down) in down.iter().enumerate() {
        assert!(
            up.contains(&format!("up {c}").into_bytes()),
            "up {c}: {up:?}"
        );
        assert!(down.contains(&format!("down {c}").into_bytes()), "down {c}");
    }
}

#[test]
fn a_forged_room_token_cannot_take_or_close_a_room() {
    let mut w = world(31, LinkConditions::PERFECT, 2);
    let peers = w.connect_all();
    let host_addr = w.host.local_addr();
    let l = locator(&code());
    let mut thief = Raw::new(&w.net, w.relay_addr);
    // A register with the right locator and a guessed token.
    thief.send(&envelope::register(&l, &[0xEE; 16], &[0; 16]));
    w.step();
    w.step();
    let answers = thief.inbox();
    let answers = decoded(&answers);
    assert!(
        answers.iter().any(|e| matches!(
            e,
            Envelope::Refused {
                reason: Refusal::RoomTaken,
                ..
            }
        )),
        "{answers:?}"
    );
    // A forged unregister, from the thief and with the host's address forged.
    thief.send(&envelope::unregister(&l, &[0xEE; 16]));
    w.net.inject(
        host_addr,
        w.relay_addr,
        &envelope::unregister(&l, &[0xEE; 16]),
    );
    // A forged register from the host's own address (spoofed): refused to
    // the host, who is still the room's.
    w.net.inject(
        host_addr,
        w.relay_addr,
        &envelope::register(&l, &[0xEE; 16], &[0; 16]),
    );
    for _ in 0..5 {
        w.step();
    }
    assert_eq!(w.relay.rooms(), 1);
    assert_eq!(w.relay.clients(), 2);
    assert!(!w.relay_events.iter().any(|e| matches!(
        e,
        RelayEvent::RoomClosed { .. } | RelayEvent::RoomMoved { .. }
    )));
    assert!(w.relay.stats().refused >= 2);
    assert_still_talks(&mut w, &peers);
}

#[test]
fn spoofed_sources_cannot_break_a_match_or_make_state() {
    let mut w = world(32, LinkConditions::PERFECT, 2);
    let peers = w.connect_all();
    let host_addr = w.host.local_addr();
    let client_addr = w.clients[0].local_addr();
    let mut rng = Rng::from_seed(5);
    let junk_inner = |rng: &mut Rng| {
        let mut inner = wire::data_header(rng.next_u32(), u64::from(rng.next_u32())).to_vec();
        inner.extend((0..40 + rng.below(400)).map(|_| rng.next_u32() as u8));
        inner
    };
    for round in 0..40 {
        for _ in 0..30 {
            // Forged host data to every slot, forged client data to the host.
            let slot = 1 + rng.below(3);
            w.net.inject(
                host_addr,
                w.relay_addr,
                &envelope::host_send(slot, &junk_inner(&mut rng)),
            );
            w.net.inject(
                client_addr,
                w.relay_addr,
                &envelope::client_send(&junk_inner(&mut rng)),
            );
            // Registers and attaches from random forged sources.
            let from = SocketAddr::new(
                std::net::Ipv4Addr::from(rng.next_u32()).into(),
                1 + rng.below(60_000) as u16,
            );
            let l: [u8; 16] = std::array::from_fn(|_| rng.next_u32() as u8);
            let cookie: [u8; 16] = std::array::from_fn(|_| rng.next_u32() as u8);
            w.net.inject(
                from,
                w.relay_addr,
                &envelope::register(&l, &[1; 16], &cookie),
            );
            w.net.inject(
                from,
                w.relay_addr,
                &envelope::attach(&locator(&code()), &cookie),
            );
        }
        let events = w.step();
        assert_no_disconnect(&events);
        assert_eq!(w.relay.rooms(), 1, "round {round}");
        assert_eq!(w.relay.clients(), 2, "round {round}");
    }
    // The forged data reached the endpoints and was dropped there.
    assert!(w.host.stats().dropped_unknown + w.host.stats().dropped_unauthentic > 0);
    assert_still_talks(&mut w, &peers);
}

#[test]
fn floods_are_bounded_and_honest_rooms_keep_working() {
    let mut w = world(33, LinkConditions::PERFECT, 2);
    let peers = w.connect_all();
    // An attacker who knows the room code attaches and floods.
    let mut flooder = Raw::new(&w.net, w.relay_addr);
    attach_by_hand(&mut w, &mut flooder, &code());
    let junk = {
        let mut inner = wire::data_header(1, 1).to_vec();
        inner.resize(1000, 0x55);
        inner
    };
    let config = RelayConfig::default();
    let forwarded_before = w.relay.stats().forwarded;
    let answered_before = w.relay.stats().answered;
    let mut rng = Rng::from_seed(9);
    const TICKS: u64 = 125; // two seconds of 16 ms ticks
    for _ in 0..TICKS {
        for _ in 0..200 {
            flooder.send(&envelope::client_send(&junk));
        }
        // And a flood from unknown, forged addresses.
        for _ in 0..300 {
            let from = SocketAddr::new(
                std::net::Ipv4Addr::from(rng.next_u32()).into(),
                1 + rng.below(60_000) as u16,
            );
            w.net
                .inject(from, w.relay_addr, &envelope::client_send(&junk));
        }
        let events = w.step();
        assert_no_disconnect(&events);
        flooder.inbox();
    }
    let stats = w.relay.stats();
    let seconds = TICKS * 16 / 1000 + 1;
    // The flooder got through at most its own rate (and a second's burst);
    // the forged ones got no route.
    let forwarded = stats.forwarded - forwarded_before;
    let honest_most = seconds * 3000; // two clients and their host, generously
    assert!(
        forwarded <= u64::from(config.client_packets_per_second) * (seconds + 1) + honest_most,
        "{forwarded}"
    );
    assert!(stats.dropped_rate > 10_000, "{stats:?}");
    assert!(stats.dropped_unknown > 30_000, "{stats:?}");
    // Answers to the forged flood stay within the handshake rate.
    let answered = stats.answered - answered_before;
    assert!(
        answered <= u64::from(config.handshakes_per_second) * (seconds + 1),
        "{answered}"
    );
    assert!(stats.dropped_handshake_rate > 0);
    assert_still_talks(&mut w, &peers);
}

#[test]
fn oversized_random_and_mutated_datagrams_never_hurt_the_relay() {
    let mut w = world(34, LinkConditions::PERFECT, 2);
    let peers = w.connect_all();
    w.net.start_capture();
    let l = locator(&code());
    let inner = wire::data_header(3, 3).to_vec();
    let samples: Vec<Vec<u8>> = vec![
        envelope::register(&l, &[1; 16], &[2; 16]),
        envelope::unregister(&l, &[1; 16]),
        envelope::register_challenge(&l, &[2; 16]),
        envelope::registered(&l),
        envelope::attach(&l, &[2; 16]),
        envelope::attach_challenge(&l, &[2; 16]),
        envelope::attached(&l),
        envelope::refused(&l, Refusal::NoRoom),
        envelope::not_attached(),
        envelope::host_send(1, &[inner.clone(), vec![9; 64]].concat()),
        envelope::to_host(1, &[inner.clone(), vec![9; 64]].concat()),
        envelope::client_send(&[inner.clone(), vec![9; 64]].concat()),
        envelope::to_client(&[inner.clone(), vec![9; 64]].concat()),
    ];
    let mut rng = Rng::from_seed(77);
    let host_addr = w.host.local_addr();
    let client_addr = w.clients[1].local_addr();
    for _ in 0..60 {
        for _ in 0..200 {
            let mut bytes = match rng.below(4) {
                0 => {
                    let len = rng.below(1400) as usize;
                    (0..len).map(|_| rng.next_u32() as u8).collect()
                }
                1 => vec![0x57; MAX_RELAY_DATAGRAM + 1 + rng.below(3000) as usize],
                _ => samples[rng.below(samples.len() as u32) as usize].clone(),
            };
            match rng.below(4) {
                0 if !bytes.is_empty() => {
                    let at = rng.below(bytes.len() as u32) as usize;
                    bytes[at] ^= 1 << rng.below(8);
                }
                1 => bytes.truncate(rng.below(bytes.len() as u32 + 1) as usize),
                2 => bytes.extend((0..rng.below(64)).map(|_| rng.next_u32() as u8)),
                _ => {}
            }
            let from = match rng.below(4) {
                0 => host_addr,
                1 => client_addr,
                _ => SocketAddr::new(
                    std::net::Ipv4Addr::from(rng.next_u32()).into(),
                    1 + rng.below(60_000) as u16,
                ),
            };
            w.net.inject(from, w.relay_addr, &bytes);
        }
        let events = w.step();
        assert_no_disconnect(&events);
    }
    let stats = w.relay.stats();
    assert!(
        stats.dropped_oversized > 0 && stats.dropped_malformed > 0,
        "{stats:?}"
    );
    assert_eq!(w.relay.rooms(), 1);
    // No answer of the relay's own is larger than the smallest request it
    // answers (an attach): nothing to amplify with.
    for datagram in w.net.take_captured() {
        if datagram.from != w.relay_addr {
            continue;
        }
        if let Some(Envelope::ToHost { .. } | Envelope::ToClient { .. }) =
            envelope::decode(&datagram.bytes)
        {
            continue;
        }
        assert!(datagram.bytes.len() < envelope::ATTACH_LEN, "{datagram:?}");
    }
    assert_still_talks(&mut w, &peers);
}

#[test]
fn a_full_relay_and_a_full_room_refuse_with_reasons() {
    let relay_config = RelayConfig {
        max_rooms: 1,
        max_peers: 1,
        ..RelayConfig::default()
    };
    let mut w = world_with(35, LinkConditions::PERFECT, 1, relay_config);
    let peers = w.connect_all();
    // A second room: the relay is full.
    let other = RoomCode::parse("PLUM77").unwrap();
    let mut second_host = host_through(&w.net, w.relay_addr, &other, 6);
    // A second client of the full room.
    let mut second_client = client_through(&w.net, w.relay_addr, &code(), 7);
    for _ in 0..60 {
        w.step();
        let now = w.net.now_ms();
        second_host.update(now);
        second_client.update(now);
    }
    let host_state = second_host.transport().relay_state();
    assert_eq!(host_state, RelayState::Refused(Refusal::RelayFull));
    assert_eq!(host_state.diagnostic().unwrap().code, "net_relay_full");
    let client_state = second_client.transport().relay_state();
    assert_eq!(client_state, RelayState::Refused(Refusal::RoomFull));
    assert_eq!(
        client_state.diagnostic().unwrap().code,
        "net_relay_room_full"
    );
    assert_still_talks(&mut w, &peers);
}

#[test]
fn a_second_host_with_the_same_code_is_told_the_room_is_taken() {
    let mut w = world(36, LinkConditions::PERFECT, 1);
    w.connect_all();
    assert_eq!(w.relay.rooms(), 1);
    // Its token is its own, not the room's.
    let mut impostor = host_through(&w.net, w.relay_addr, &code(), 77);
    for _ in 0..40 {
        w.step();
        impostor.update(w.net.now_ms());
    }
    assert_eq!(
        impostor.transport().relay_state(),
        RelayState::Refused(Refusal::RoomTaken)
    );
    assert_eq!(
        impostor
            .transport()
            .relay_state()
            .diagnostic()
            .unwrap()
            .code,
        "net_relay_room_taken"
    );
    assert_eq!(w.relay.rooms(), 1);
}

#[test]
fn a_closed_room_is_gone_and_its_code_free_again() {
    let net = SimNetwork::new(37, 16);
    let relay_node = net.add_node();
    let relay_addr = relay_node.local_addr();
    let mut relay = Relay::new(relay_node, RelayConfig::default(), Entropy::seeded(1)).unwrap();
    let mut entropy = Entropy::seeded(2);
    let mut transport = RelayHost::new(net.add_node(), relay_addr, &code(), &mut entropy);
    for _ in 0..20 {
        net.step();
        relay.update(net.now_ms());
        transport.tick(net.now_ms());
        let mut buf = [0u8; 1300];
        while !matches!(transport.recv(&mut buf), Recv::Empty) {}
    }
    assert_eq!(transport.relay_state(), RelayState::Ready);
    assert_eq!(relay.rooms(), 1);
    transport.close();
    net.step();
    relay.update(net.now_ms());
    assert_eq!(relay.rooms(), 0);
    let mut events = Vec::new();
    while let Some(event) = relay.poll_event() {
        events.push(event);
    }
    assert!(events.iter().any(|e| matches!(
        e,
        RelayEvent::RoomClosed {
            reason: RoomClosed::ByHost,
            ..
        }
    )));
    // Another host takes the code now.
    let mut next = host_through(&net, relay_addr, &code(), 9);
    for _ in 0..20 {
        net.step();
        relay.update(net.now_ms());
        next.update(net.now_ms());
    }
    assert_eq!(next.transport().relay_state(), RelayState::Ready);
}
