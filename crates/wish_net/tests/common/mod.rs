//! Helpers shared by the endpoint tests.

#![allow(dead_code)]

use wish_net::{
    Channel, Config, Endpoint, Entropy, Event, LinkConditions, PeerId, RoomCode, SimNetwork,
    SimTransport,
};

pub type SimEndpoint = Endpoint<SimTransport>;

pub fn code() -> RoomCode {
    RoomCode::parse("STAR42").unwrap()
}

pub fn config() -> Config {
    Config::new(code())
}

/// A message whose bytes say which message it is: index, then a pattern.
pub fn message(index: u32, len: usize) -> Vec<u8> {
    let mut out = index.to_le_bytes().to_vec();
    out.extend((4..len.max(4)).map(|at| (index as usize * 31 + at * 7) as u8));
    out
}

pub fn index_of(data: &[u8]) -> u32 {
    u32::from_le_bytes(data[..4].try_into().unwrap())
}

/// One tick: the network moves, then every endpoint updates; the events
/// are returned per endpoint.
pub fn tick(net: &SimNetwork, endpoints: &mut [&mut SimEndpoint]) -> Vec<Vec<Event>> {
    net.step();
    let now = net.now_ms();
    endpoints
        .iter_mut()
        .map(|e| {
            e.update(now);
            std::iter::from_fn(|| e.poll_event()).collect()
        })
        .collect()
}

/// A host and a client on a network with `conditions`, connected.
pub fn connected_pair(
    seed: u64,
    conditions: LinkConditions,
) -> (SimNetwork, SimEndpoint, SimEndpoint) {
    connected_pair_with(seed, conditions, config(), config())
}

pub fn connected_pair_with(
    seed: u64,
    conditions: LinkConditions,
    host_config: Config,
    client_config: Config,
) -> (SimNetwork, SimEndpoint, SimEndpoint) {
    let net = SimNetwork::new(seed, 16);
    net.set_conditions(conditions);
    let host_node = net.add_node();
    let host_addr = wish_net::Transport::local_addr(&host_node);
    let mut host = Endpoint::host(host_node, host_config, Entropy::seeded(seed ^ 0xA5)).unwrap();
    let mut client = Endpoint::connect(
        net.add_node(),
        host_addr,
        client_config,
        Entropy::seeded(seed ^ 0x5A),
    )
    .unwrap();
    let mut host_up = false;
    let mut client_up = false;
    for _ in 0..2000 {
        let events = tick(&net, &mut [&mut host, &mut client]);
        host_up |= events[0]
            .iter()
            .any(|e| matches!(e, Event::Connected { .. }));
        client_up |= events[1]
            .iter()
            .any(|e| matches!(e, Event::Connected { peer: PeerId::HOST }));
        if host_up && client_up {
            return (net, host, client);
        }
    }
    panic!("the pair did not connect");
}

/// Received messages of one channel, in arrival order.
pub fn messages(events: &[Event], want: Channel) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message { channel, data, .. } if *channel == want => Some(data.clone()),
            _ => None,
        })
        .collect()
}
