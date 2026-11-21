//! A deterministic network simulator.
//!
//! A [`SimNetwork`] carries datagrams between [`SimTransport`] nodes. Every
//! sent datagram draws, from the network's one seeded [`Rng`], whether it is
//! lost, whether it is duplicated, its jitter and whether it is held back
//! (reordered); it is delivered at the first tick at or after its arrival
//! time. The network moves only when [`SimNetwork::step`] is called and never
//! reads the clock, so the same seed and the same sends give the same
//! deliveries, byte for byte, on every machine.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wish_core::{Diagnostic, Hash128, Result, Rng, StableHasher};

use crate::transport::{Recv, Transport};

/// The largest datagram the simulator carries (the largest UDP payload over
/// IPv4). Larger ones are dropped, as a real network would refuse them.
pub const SIM_MAX_DATAGRAM: usize = 65_507;

/// Datagrams a node holds before it drops new ones (a socket's receive
/// buffer).
pub const SIM_INBOX: usize = 4096;

/// How a directed link treats datagrams.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkConditions {
    /// One-way delay every datagram has, in milliseconds.
    pub latency_ms: u32,
    /// Extra delay drawn uniformly from `0..=jitter_ms` per datagram, in
    /// milliseconds. Jitter alone already reorders datagrams.
    pub jitter_ms: u32,
    /// Chance that a datagram is lost, from 0 to 1.
    pub loss: f64,
    /// Chance that a datagram arrives twice, from 0 to 1 (the copy draws its
    /// own delay).
    pub duplicate: f64,
    /// Chance that a datagram is held back behind later ones, from 0 to 1.
    pub reorder: f64,
}

impl LinkConditions {
    /// No delay, no loss: every datagram arrives at the next tick, in order.
    pub const PERFECT: Self = Self {
        latency_ms: 0,
        jitter_ms: 0,
        loss: 0.0,
        duplicate: 0.0,
        reorder: 0.0,
    };

    /// A bad internet link: 100 ms latency, 30 ms jitter, 5 % loss, 2 %
    /// duplication and 10 % reordering (the yardstick of the 1.1 plan).
    pub const BAD: Self = Self {
        latency_ms: 100,
        jitter_ms: 30,
        loss: 0.05,
        duplicate: 0.02,
        reorder: 0.10,
    };

    /// Probabilities outside 0 to 1, and values that are not numbers, are
    /// clamped (NaN counts as 0).
    fn cleaned(self) -> Self {
        let clean = |p: f64| if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) };
        Self {
            loss: clean(self.loss),
            duplicate: clean(self.duplicate),
            reorder: clean(self.reorder),
            ..self
        }
    }
}

impl Default for LinkConditions {
    fn default() -> Self {
        Self::PERFECT
    }
}

/// What the network did, counted since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Datagrams handed to the network (injected ones included).
    pub sent: u64,
    /// Datagrams lost on the way.
    pub lost: u64,
    /// Extra copies made.
    pub duplicated: u64,
    /// Datagrams held back behind later ones.
    pub reordered: u64,
    /// Datagrams put into a node's inbox (a broadcast counts once per node).
    pub delivered: u64,
    /// Datagrams dropped because the receiving node's inbox was full, or
    /// because no node was bound to the address.
    pub undeliverable: u64,
    /// Datagrams dropped because they were larger than [`SIM_MAX_DATAGRAM`].
    pub oversized: u64,
}

struct InFlight {
    from: SocketAddr,
    to: SocketAddr,
    bytes: Vec<u8>,
}

/// A datagram put on the wire, as [`SimNetwork::take_captured`] gives it:
/// what a listener on the network sees, whether it then arrives or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The tick it was sent at.
    pub tick: u64,
    /// Its source address (as sent, forged or not).
    pub from: SocketAddr,
    /// Its destination address (perhaps the broadcast address).
    pub to: SocketAddr,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

struct Inner {
    rng: Rng,
    tick: u64,
    tick_ms: u32,
    order: u64,
    next_host: u32,
    nodes: BTreeMap<SocketAddr, VecDeque<(SocketAddr, Vec<u8>)>>,
    /// Keyed by (tick of delivery, order of sending).
    pending: BTreeMap<(u64, u64), InFlight>,
    all: LinkConditions,
    links: BTreeMap<(SocketAddr, SocketAddr), LinkConditions>,
    hasher: StableHasher,
    stats: SimStats,
    capture: Option<Vec<Captured>>,
}

/// Captured datagrams kept at most (older ones are not kept beyond this).
pub const SIM_CAPTURE_MAX: usize = 1 << 20;

impl Inner {
    fn now_ms(&self) -> u64 {
        self.tick.saturating_mul(u64::from(self.tick_ms))
    }

    fn conditions(&self, from: SocketAddr, to: SocketAddr) -> LinkConditions {
        self.links.get(&(from, to)).copied().unwrap_or(self.all)
    }

    fn arrival_tick(&mut self, link: &LinkConditions) -> u64 {
        let mut delay = u64::from(link.latency_ms);
        if link.jitter_ms > 0 {
            delay += u64::from(self.rng.below(link.jitter_ms.saturating_add(1)));
        }
        if link.reorder > 0.0 && self.rng.chance(link.reorder) {
            self.stats.reordered += 1;
            let spread = link
                .latency_ms
                .saturating_add(link.jitter_ms)
                .saturating_add(self.tick_ms.saturating_mul(2))
                .max(1);
            delay += 1 + u64::from(self.rng.below(spread));
        }
        let arrival_ms = self.now_ms().saturating_add(delay);
        let tick_ms = u64::from(self.tick_ms.max(1));
        arrival_ms.div_ceil(tick_ms).max(self.tick + 1)
    }

    fn put(&mut self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) {
        self.stats.sent += 1;
        if let Some(capture) = &mut self.capture
            && capture.len() < SIM_CAPTURE_MAX
        {
            capture.push(Captured {
                tick: self.tick,
                from,
                to,
                bytes: bytes.to_vec(),
            });
        }
        if bytes.len() > SIM_MAX_DATAGRAM {
            self.stats.oversized += 1;
            return;
        }
        let link = self.conditions(from, to);
        if link.loss > 0.0 && self.rng.chance(link.loss) {
            self.stats.lost += 1;
            return;
        }
        let copies = if link.duplicate > 0.0 && self.rng.chance(link.duplicate) {
            self.stats.duplicated += 1;
            2
        } else {
            1
        };
        for _ in 0..copies {
            let due = self.arrival_tick(&link);
            let order = self.order;
            self.order += 1;
            self.pending.insert(
                (due, order),
                InFlight {
                    from,
                    to,
                    bytes: bytes.to_vec(),
                },
            );
        }
    }

    fn deliver(&mut self, from: SocketAddr, to: SocketAddr, bytes: Vec<u8>) {
        let Some(inbox) = self.nodes.get_mut(&to) else {
            self.stats.undeliverable += 1;
            return;
        };
        if inbox.len() >= SIM_INBOX {
            self.stats.undeliverable += 1;
            return;
        }
        self.hasher.write_u64(self.tick);
        write_addr(&mut self.hasher, from);
        write_addr(&mut self.hasher, to);
        self.hasher.write_len_prefixed(&bytes);
        inbox.push_back((from, bytes));
        self.stats.delivered += 1;
    }

    fn step(&mut self) {
        self.tick += 1;
        while let Some(entry) = self.pending.first_entry() {
            if entry.key().0 > self.tick {
                break;
            }
            let datagram = entry.remove();
            if is_broadcast(datagram.to) {
                let port = datagram.to.port();
                let targets: Vec<SocketAddr> = self
                    .nodes
                    .keys()
                    .filter(|addr| addr.port() == port && **addr != datagram.from)
                    .copied()
                    .collect();
                if targets.is_empty() {
                    self.stats.undeliverable += 1;
                }
                for target in targets {
                    self.deliver(datagram.from, target, datagram.bytes.clone());
                }
            } else {
                self.deliver(datagram.from, datagram.to, datagram.bytes);
            }
        }
    }
}

fn is_broadcast(addr: SocketAddr) -> bool {
    matches!(addr.ip(), IpAddr::V4(ip) if ip.is_broadcast())
}

fn write_addr(hasher: &mut StableHasher, addr: SocketAddr) {
    match addr.ip() {
        IpAddr::V4(ip) => {
            hasher.write_u8(4);
            hasher.write_bytes(&ip.octets());
        }
        IpAddr::V6(ip) => {
            hasher.write_u8(6);
            hasher.write_bytes(&ip.octets());
        }
    }
    hasher.write_bytes(&addr.port().to_le_bytes());
}

/// A simulated network. Cloning it gives another handle to the same network.
#[derive(Clone)]
pub struct SimNetwork {
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for SimNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        f.debug_struct("SimNetwork")
            .field("tick", &inner.tick)
            .field("nodes", &inner.nodes.len())
            .field("pending", &inner.pending.len())
            .finish()
    }
}

impl SimNetwork {
    /// A network whose random draws come from `seed` and whose ticks are
    /// `tick_ms` milliseconds long (at least 1). Every link starts perfect.
    pub fn new(seed: u64, tick_ms: u32) -> Self {
        let mut hasher = StableHasher::new();
        hasher.write_str("wish-net-sim-v1");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                rng: Rng::from_seed(seed),
                tick: 0,
                tick_ms: tick_ms.max(1),
                order: 0,
                next_host: 1,
                nodes: BTreeMap::new(),
                pending: BTreeMap::new(),
                all: LinkConditions::PERFECT,
                links: BTreeMap::new(),
                hasher,
                stats: SimStats::default(),
                capture: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // The state stays consistent between calls, so a panic elsewhere
        // while holding the lock leaves nothing half done.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the conditions of every link that has none of its own.
    pub fn set_conditions(&self, conditions: LinkConditions) {
        self.lock().all = conditions.cleaned();
    }

    /// Sets the conditions of the directed link from `from` to `to`.
    pub fn set_link(&self, from: SocketAddr, to: SocketAddr, conditions: LinkConditions) {
        self.lock().links.insert((from, to), conditions.cleaned());
    }

    /// A node at the next free address (`10.0.x.y:40000`).
    pub fn add_node(&self) -> SimTransport {
        let addr = {
            let mut inner = self.lock();
            loop {
                let n = inner.next_host;
                inner.next_host = inner.next_host.wrapping_add(1);
                let ip = Ipv4Addr::new(10, 0, (n >> 8) as u8, n as u8);
                let addr = SocketAddr::new(IpAddr::V4(ip), 40_000);
                if !inner.nodes.contains_key(&addr) && n & 0xFF != 0 && n & 0xFF != 0xFF {
                    inner.nodes.insert(addr, VecDeque::new());
                    break addr;
                }
            }
        };
        SimTransport {
            network: self.clone(),
            addr,
        }
    }

    /// A node at a chosen address.
    pub fn bind(&self, addr: SocketAddr) -> Result<SimTransport> {
        let mut inner = self.lock();
        if inner.nodes.contains_key(&addr) || is_broadcast(addr) || addr.port() == 0 {
            return Err(Diagnostic::error(
                "net_bind_failed",
                format!("the simulated address {addr} is taken or cannot be bound"),
                "Bind each simulated node to its own address with a port other than 0, or use add_node.",
            ));
        }
        inner.nodes.insert(addr, VecDeque::new());
        drop(inner);
        Ok(SimTransport {
            network: self.clone(),
            addr,
        })
    }

    /// Advances the network by one tick and delivers every datagram due by
    /// then, in the order they were sent.
    pub fn step(&self) {
        self.lock().step();
    }

    /// The current tick (0 before the first step).
    pub fn tick(&self) -> u64 {
        self.lock().tick
    }

    /// The simulated time in milliseconds: tick × tick length.
    pub fn now_ms(&self) -> u64 {
        self.lock().now_ms()
    }

    /// Puts a datagram on the wire with any source address, as an attacker
    /// who forges the source would (it takes the link's conditions).
    pub fn inject(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) {
        self.lock().put(from, to, bytes);
    }

    /// A hash of every delivery so far (tick, source, destination, bytes):
    /// two runs delivered the same if and only if (up to hash collisions)
    /// their hashes are equal.
    pub fn delivery_hash(&self) -> Hash128 {
        self.lock().hasher.finish()
    }

    /// What the network did so far.
    pub fn stats(&self) -> SimStats {
        self.lock().stats
    }

    /// Starts keeping a copy of every datagram put on the wire (as a
    /// listener on the network sees them, lost ones included), up to
    /// [`SIM_CAPTURE_MAX`].
    pub fn start_capture(&self) {
        let mut inner = self.lock();
        if inner.capture.is_none() {
            inner.capture = Some(Vec::new());
        }
    }

    /// The datagrams captured since the last call (capturing goes on).
    pub fn take_captured(&self) -> Vec<Captured> {
        self.lock()
            .capture
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Datagrams on the way (not yet delivered).
    pub fn in_flight(&self) -> usize {
        self.lock().pending.len()
    }
}

/// A node of a [`SimNetwork`]. Dropping it unbinds its address.
pub struct SimTransport {
    network: SimNetwork,
    addr: SocketAddr,
}

impl std::fmt::Debug for SimTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimTransport")
            .field("addr", &self.addr)
            .finish()
    }
}

impl SimTransport {
    /// The network this node belongs to.
    pub fn network(&self) -> &SimNetwork {
        &self.network
    }
}

impl Drop for SimTransport {
    fn drop(&mut self) {
        self.network.lock().nodes.remove(&self.addr);
    }
}

impl Transport for SimTransport {
    fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()> {
        self.network.lock().put(self.addr, to, bytes);
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> Recv {
        let mut inner = self.network.lock();
        let Some(inbox) = inner.nodes.get_mut(&self.addr) else {
            return Recv::Empty;
        };
        let Some((from, bytes)) = inbox.pop_front() else {
            return Recv::Empty;
        };
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Recv::Datagram {
            len: bytes.len(),
            from,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: u64, conditions: LinkConditions) -> (Hash128, Vec<Vec<u8>>, SimStats) {
        let net = SimNetwork::new(seed, 10);
        net.set_conditions(conditions);
        let mut a = net.add_node();
        let mut b = net.add_node();
        let to = b.local_addr();
        let mut got = Vec::new();
        let mut buf = [0u8; 64];
        for i in 0u32..2000 {
            a.send(to, &i.to_le_bytes()).unwrap();
            net.step();
            while let Recv::Datagram { len, .. } = b.recv(&mut buf) {
                got.push(buf[..len].to_vec());
            }
        }
        for _ in 0..100 {
            net.step();
            while let Recv::Datagram { len, .. } = b.recv(&mut buf) {
                got.push(buf[..len].to_vec());
            }
        }
        (net.delivery_hash(), got, net.stats())
    }

    #[test]
    fn the_same_seed_delivers_the_same() {
        let first = run(7, LinkConditions::BAD);
        let second = run(7, LinkConditions::BAD);
        assert_eq!(first.0, second.0);
        assert_eq!(first.1, second.1);
        let other = run(8, LinkConditions::BAD);
        assert_ne!(first.0, other.0);
    }

    #[test]
    fn a_perfect_link_delivers_everything_in_order() {
        let (_, got, stats) = run(1, LinkConditions::PERFECT);
        assert_eq!(got.len(), 2000);
        for (i, bytes) in got.iter().enumerate() {
            assert_eq!(bytes.as_slice(), (i as u32).to_le_bytes());
        }
        assert_eq!(stats.lost, 0);
    }

    #[test]
    fn a_bad_link_loses_duplicates_and_reorders_at_about_its_rates() {
        let (_, got, stats) = run(3, LinkConditions::BAD);
        // 5 % of 2000 is 100; 2 % is 40; 10 % of what survives is about 190.
        assert!((60..=140).contains(&stats.lost), "{stats:?}");
        assert!((15..=70).contains(&stats.duplicated), "{stats:?}");
        assert!((120..=260).contains(&stats.reordered), "{stats:?}");
        let out_of_order = got
            .windows(2)
            .filter(|w| {
                u32::from_le_bytes(w[0].clone().try_into().unwrap())
                    > u32::from_le_bytes(w[1].clone().try_into().unwrap())
            })
            .count();
        assert!(out_of_order > 50, "{out_of_order}");
        assert_eq!(got.len() as u64, stats.delivered);
    }

    #[test]
    fn broadcast_reaches_every_node_on_the_port_but_the_sender() {
        let net = SimNetwork::new(1, 10);
        let mut a = net.add_node();
        let mut b = net.add_node();
        let mut c = net.add_node();
        let other = net.bind("10.0.9.9:5000".parse().unwrap()).unwrap();
        a.send("255.255.255.255:40000".parse().unwrap(), b"hi")
            .unwrap();
        net.step();
        let mut buf = [0u8; 8];
        assert!(matches!(b.recv(&mut buf), Recv::Datagram { len: 2, .. }));
        assert!(matches!(c.recv(&mut buf), Recv::Datagram { len: 2, .. }));
        assert_eq!(a.recv(&mut buf), Recv::Empty);
        drop(other);
    }

    #[test]
    fn a_dropped_node_unbinds_and_nan_rates_are_clean() {
        let net = SimNetwork::new(1, 10);
        net.set_conditions(LinkConditions {
            loss: f64::NAN,
            duplicate: 7.0,
            ..LinkConditions::PERFECT
        });
        let addr: SocketAddr = "10.1.1.1:7".parse().unwrap();
        let node = net.bind(addr).unwrap();
        assert!(net.bind(addr).is_err());
        drop(node);
        let mut node = net.bind(addr).unwrap();
        node.send(addr, b"x").unwrap();
        net.step();
        let mut buf = [0u8; 4];
        assert!(matches!(node.recv(&mut buf), Recv::Datagram { .. }));
        assert!(matches!(node.recv(&mut buf), Recv::Datagram { .. }));
    }
}
