//! The relay itself: routes envelopes between a registered host and the
//! clients attached to its room. It never opens an inner datagram, holds no
//! key, writes nothing to disk and reads no clock (design note
//! `docs/design/1.1-relay.md` §3.3 to §3.7).

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;

use wish_core::{Diagnostic, Result};

use crate::crypto::{Entropy, same_bytes};
use crate::endpoint::{COOKIE_BUCKET_MS, MAX_PEERS_LIMIT, MAX_READS_PER_UPDATE};
use crate::relay::envelope::{
    self, Envelope, HOST_TOKEN_LEN, Locator, MAX_RELAY_DATAGRAM, RELAY_COOKIE_LEN, Refusal, Role,
};
use crate::transport::{Recv, Transport};

/// The UDP port `wen relay` listens on by default.
pub const RELAY_PORT: u16 = 47_620;
/// The most rooms a relay can be set to hold.
pub const MAX_ROOMS_LIMIT: u32 = 65_536;
/// The most clients a relay can be set to hold in all.
pub const MAX_CLIENTS_LIMIT: u32 = 1 << 20;
/// Events kept for [`Relay::poll_event`] at most; older ones are dropped.
pub const MAX_EVENTS: usize = 1024;
/// How often idle rooms and clients are looked for, in milliseconds.
const SWEEP_MS: u64 = 250;

/// The settings of a relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayConfig {
    /// Rooms the relay holds (1 to [`MAX_ROOMS_LIMIT`]).
    pub max_rooms: u32,
    /// Clients one room holds (1 to 256, the most peers a host takes).
    pub max_peers: u32,
    /// Clients the relay holds in all rooms together (1 to
    /// [`MAX_CLIENTS_LIMIT`]).
    pub max_clients: u32,
    /// A room whose host sent nothing for this long is closed, in
    /// milliseconds (at least 1000).
    pub room_idle_ms: u64,
    /// A client that sent nothing for this long loses its slot, in
    /// milliseconds (at least 1000).
    pub client_idle_ms: u64,
    /// Handshake answers (challenges, refusals, `NotAttached`) the relay
    /// sends per second, to all addresses together (at least 10).
    pub handshakes_per_second: u32,
    /// Datagrams accepted per second from one client (at least 10).
    pub client_packets_per_second: u32,
    /// Bytes accepted per second from one client (at least 64 KiB).
    pub client_bytes_per_second: u64,
    /// Datagrams carried per second through one room, both ways (at least
    /// 10).
    pub room_packets_per_second: u32,
    /// Bytes carried per second through one room, both ways (at least
    /// 64 KiB).
    pub room_bytes_per_second: u64,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            max_rooms: 256,
            max_peers: 32,
            max_clients: 4096,
            room_idle_ms: 15_000,
            client_idle_ms: 15_000,
            handshakes_per_second: 512,
            client_packets_per_second: 1500,
            client_bytes_per_second: 1 << 20,
            room_packets_per_second: 8000,
            room_bytes_per_second: 8 << 20,
        }
    }
}

impl RelayConfig {
    /// Checks every setting against its range.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String, hint: &str| {
            Err(
                Diagnostic::error("net_config_invalid", message, hint.to_owned())
                    .at_path(format!("/relay/{field}")),
            )
        };
        if !(1..=MAX_ROOMS_LIMIT).contains(&self.max_rooms) {
            return bad(
                "max_rooms",
                format!("max_rooms is {}", self.max_rooms),
                "Let the relay hold from 1 to 65536 rooms (the default is 256).",
            );
        }
        if !(1..=MAX_PEERS_LIMIT).contains(&self.max_peers) {
            return bad(
                "max_peers",
                format!("max_peers is {}", self.max_peers),
                "Let one room hold from 1 to 256 clients (the default is 32).",
            );
        }
        if !(1..=MAX_CLIENTS_LIMIT).contains(&self.max_clients) {
            return bad(
                "max_clients",
                format!("max_clients is {}", self.max_clients),
                "Let the relay hold from 1 to 1048576 clients in all (the default is 4096).",
            );
        }
        if self.room_idle_ms < 1000 || self.client_idle_ms < 1000 {
            return bad(
                "room_idle_ms",
                format!(
                    "room_idle_ms is {} and client_idle_ms is {}",
                    self.room_idle_ms, self.client_idle_ms
                ),
                "Give hosts and clients at least a second of silence (the defaults are 15000 ms).",
            );
        }
        if self.handshakes_per_second < 10
            || self.client_packets_per_second < 10
            || self.room_packets_per_second < 10
        {
            return bad(
                "handshakes_per_second",
                "a datagram rate of the relay is below 10 a second".to_owned(),
                "Allow at least 10 datagrams a second for handshakes, clients and rooms.",
            );
        }
        if self.client_bytes_per_second < 64 * 1024 || self.room_bytes_per_second < 64 * 1024 {
            return bad(
                "client_bytes_per_second",
                "a byte rate of the relay is below 64 KiB a second".to_owned(),
                "Allow at least 65536 bytes a second for clients and rooms (the defaults are 1 MiB and 8 MiB).",
            );
        }
        Ok(())
    }
}

/// A token bucket in thousandths, refilled by the caller's time: bursts up
/// to one second's worth.
#[derive(Debug, Clone, Copy)]
struct Rate {
    milli: u64,
    last_ms: u64,
    rate: u64,
}

impl Rate {
    fn new(per_second: u64) -> Self {
        Self {
            milli: per_second.saturating_mul(1000),
            last_ms: 0,
            rate: per_second,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        self.milli = self
            .milli
            .saturating_add(elapsed.saturating_mul(self.rate))
            .min(self.rate.saturating_mul(1000));
    }

    fn has(&self, cost: u64) -> bool {
        self.milli >= cost.saturating_mul(1000)
    }

    fn spend(&mut self, cost: u64) {
        self.milli = self.milli.saturating_sub(cost.saturating_mul(1000));
    }

    fn take(&mut self, now_ms: u64) -> bool {
        self.refill(now_ms);
        let ok = self.has(1);
        if ok {
            self.spend(1);
        }
        ok
    }
}

/// A datagram rate and a byte rate, taken together or not at all.
#[derive(Debug, Clone, Copy)]
struct Budget {
    packets: Rate,
    bytes: Rate,
}

impl Budget {
    fn new(packets: u32, bytes: u64) -> Self {
        Self {
            packets: Rate::new(u64::from(packets)),
            bytes: Rate::new(bytes),
        }
    }

    fn can(&mut self, now_ms: u64, len: usize) -> bool {
        self.packets.refill(now_ms);
        self.bytes.refill(now_ms);
        self.packets.has(1) && self.bytes.has(len as u64)
    }

    fn spend(&mut self, len: usize) {
        self.packets.spend(1);
        self.bytes.spend(len as u64);
    }
}

/// A room's short name in logs: the first four bytes of its locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoomTag(pub [u8; 4]);

impl RoomTag {
    fn of(locator: &Locator) -> Self {
        Self([locator[0], locator[1], locator[2], locator[3]])
    }
}

impl fmt::Display for RoomTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Why a room closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomClosed {
    /// Its host sent nothing for `room_idle_ms`.
    Idle,
    /// Its host closed it.
    ByHost,
    /// Its host's address registered another room.
    Replaced,
}

/// Why a client lost its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientLeft {
    /// It sent nothing for `client_idle_ms`.
    Idle,
    /// Its address attached to another room, or registered one.
    Moved,
}

/// What happened at a relay. Events carry no payloads, tokens, cookies or
/// whole locators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayEvent {
    /// A host registered a room.
    RoomOpened {
        /// The room.
        room: RoomTag,
        /// The host's address.
        host: SocketAddr,
    },
    /// A room's host moved to another address (its router changed the port).
    RoomMoved {
        /// The room.
        room: RoomTag,
        /// The host's new address.
        host: SocketAddr,
    },
    /// A room closed, and every client in it lost its slot.
    RoomClosed {
        /// The room.
        room: RoomTag,
        /// Why.
        reason: RoomClosed,
        /// Clients it held.
        clients: usize,
    },
    /// A client got a slot in a room.
    ClientAttached {
        /// The room.
        room: RoomTag,
        /// The slot.
        slot: u32,
        /// The client's address.
        addr: SocketAddr,
    },
    /// A client lost its slot (not counting rooms that closed).
    ClientLeft {
        /// The room.
        room: RoomTag,
        /// The slot.
        slot: u32,
        /// Why.
        reason: ClientLeft,
    },
}

/// What a relay did, counted since it started. The `dropped_*` counts are
/// datagrams that were read and thrown away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayStats {
    /// Datagrams read.
    pub received: u64,
    /// Bytes read.
    pub received_bytes: u64,
    /// Inner datagrams carried to a host or a client.
    pub forwarded: u64,
    /// Bytes of the envelopes carried.
    pub forwarded_bytes: u64,
    /// Handshake answers sent (challenges, registrations, attaches,
    /// refusals, `NotAttached`).
    pub answered: u64,
    /// Datagrams larger than a relay datagram, or cut by the socket.
    pub dropped_oversized: u64,
    /// Datagrams that are not well-formed relay datagrams.
    pub dropped_malformed: u64,
    /// Datagrams from addresses that hold no room and no slot, or for a slot
    /// the room does not have.
    pub dropped_unknown: u64,
    /// Datagrams over a client's or a room's rate.
    pub dropped_rate: u64,
    /// Handshake datagrams not answered because of the handshake rate.
    pub dropped_handshake_rate: u64,
    /// Registrations and attaches refused.
    pub refused: u64,
    /// Events dropped because nobody took them.
    pub dropped_events: u64,
    /// Datagrams the transport could not send.
    pub send_errors: u64,
}

struct Room {
    host: SocketAddr,
    token: [u8; HOST_TOKEN_LEN],
    last_heard_ms: u64,
    next_slot: u32,
    slots: BTreeMap<u32, SocketAddr>,
    budget: Budget,
}

struct Client {
    locator: Locator,
    slot: u32,
    last_heard_ms: u64,
    budget: Budget,
}

/// A relay over any [`Transport`].
pub struct Relay<T: Transport> {
    transport: T,
    config: RelayConfig,
    secret: [u8; 32],
    rooms: BTreeMap<Locator, Room>,
    hosts: BTreeMap<SocketAddr, Locator>,
    clients: BTreeMap<SocketAddr, Client>,
    handshakes: Rate,
    events: VecDeque<RelayEvent>,
    stats: RelayStats,
    now_ms: u64,
    last_sweep_ms: u64,
    buf: Vec<u8>,
}

impl<T: Transport> fmt::Debug for Relay<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Relay")
            .field("local_addr", &self.transport.local_addr())
            .field("rooms", &self.rooms.len())
            .field("clients", &self.clients.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> Relay<T> {
    /// A relay on `transport`. The secret of its cookies comes from
    /// `entropy` and lives only in memory.
    pub fn new(transport: T, config: RelayConfig, mut entropy: Entropy) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            transport,
            config,
            secret: entropy.bytes(),
            rooms: BTreeMap::new(),
            hosts: BTreeMap::new(),
            clients: BTreeMap::new(),
            handshakes: Rate::new(u64::from(config.handshakes_per_second)),
            events: VecDeque::new(),
            stats: RelayStats::default(),
            now_ms: 0,
            last_sweep_ms: 0,
            buf: vec![0; MAX_RELAY_DATAGRAM + 1],
        })
    }

    /// The address the relay receives on.
    pub fn local_addr(&self) -> SocketAddr {
        self.transport.local_addr()
    }

    /// The settings.
    pub fn config(&self) -> &RelayConfig {
        &self.config
    }

    /// What the relay did so far.
    pub fn stats(&self) -> RelayStats {
        self.stats
    }

    /// Rooms open now.
    pub fn rooms(&self) -> usize {
        self.rooms.len()
    }

    /// Clients holding a slot now, in all rooms.
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// The transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The next event, if any.
    pub fn poll_event(&mut self) -> Option<RelayEvent> {
        self.events.pop_front()
    }

    /// Reads what arrived (at most [`MAX_READS_PER_UPDATE`] datagrams),
    /// carries it on, and closes idle rooms and slots. `now_ms` is a
    /// monotonic time in milliseconds chosen by the caller (a smaller value
    /// than the last one counts as the last one). Returns the datagrams
    /// read, so a caller can sleep when there were none.
    pub fn update(&mut self, now_ms: u64) -> usize {
        self.now_ms = self.now_ms.max(now_ms);
        let mut buf = std::mem::take(&mut self.buf);
        let mut reads = 0;
        for _ in 0..MAX_READS_PER_UPDATE {
            match self.transport.recv(&mut buf) {
                Recv::Empty => break,
                Recv::Dropped => {
                    reads += 1;
                    self.stats.dropped_oversized += 1;
                }
                Recv::Datagram { len, from } => {
                    reads += 1;
                    self.stats.received += 1;
                    self.stats.received_bytes += len as u64;
                    if len > MAX_RELAY_DATAGRAM || len > buf.len() {
                        self.stats.dropped_oversized += 1;
                        continue;
                    }
                    self.handle(from, &buf[..len]);
                }
            }
        }
        self.buf = buf;
        if self.now_ms.saturating_sub(self.last_sweep_ms) >= SWEEP_MS {
            self.last_sweep_ms = self.now_ms;
            self.sweep();
        }
        reads
    }

    fn event(&mut self, event: RelayEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
            self.stats.dropped_events += 1;
        }
        self.events.push_back(event);
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) {
        if self.transport.send(to, bytes).is_err() {
            self.stats.send_errors += 1;
        }
    }

    /// Sends a handshake answer, within the handshake rate. Every answer is
    /// smaller than the datagram it answers (no amplification).
    fn answer(&mut self, to: SocketAddr, bytes: &[u8]) {
        if !self.handshakes.take(self.now_ms) {
            self.stats.dropped_handshake_rate += 1;
            return;
        }
        self.stats.answered += 1;
        self.send(to, bytes);
    }

    fn refuse(&mut self, to: SocketAddr, locator: &Locator, reason: Refusal) {
        self.stats.refused += 1;
        self.answer(to, &envelope::refused(locator, reason));
    }

    fn cookie_for(
        &self,
        addr: SocketAddr,
        locator: &Locator,
        role: Role,
    ) -> [u8; RELAY_COOKIE_LEN] {
        envelope::cookie(
            &self.secret,
            self.now_ms / COOKIE_BUCKET_MS,
            addr,
            locator,
            role,
        )
    }

    fn cookie_ok(
        &self,
        addr: SocketAddr,
        locator: &Locator,
        role: Role,
        cookie: &[u8; RELAY_COOKIE_LEN],
    ) -> bool {
        let bucket = self.now_ms / COOKIE_BUCKET_MS;
        [bucket, bucket.saturating_sub(1)].iter().any(|b| {
            same_bytes(
                cookie,
                &envelope::cookie(&self.secret, *b, addr, locator, role),
            )
        })
    }

    fn handle(&mut self, from: SocketAddr, bytes: &[u8]) {
        let Some(envelope) = envelope::decode(bytes) else {
            self.stats.dropped_malformed += 1;
            return;
        };
        match envelope {
            Envelope::Register {
                locator,
                token,
                cookie,
            } => self.register(from, locator, token, cookie),
            Envelope::Unregister { locator, token } => self.unregister(from, locator, token),
            Envelope::Attach { locator, cookie } => self.attach(from, locator, cookie),
            Envelope::HostSend { slot, inner } => self.host_send(from, slot, inner),
            Envelope::ClientSend { inner } => self.client_send(from, inner),
            // What a relay sends, never what it is sent.
            _ => self.stats.dropped_malformed += 1,
        }
    }

    fn register(
        &mut self,
        from: SocketAddr,
        locator: Locator,
        token: [u8; HOST_TOKEN_LEN],
        cookie: [u8; RELAY_COOKIE_LEN],
    ) {
        let now = self.now_ms;
        let existing = self.rooms.get(&locator).map(|room| room.host);
        if let Some(host) = existing {
            let same_token = self
                .rooms
                .get(&locator)
                .is_some_and(|room| same_bytes(&room.token, &token));
            if !same_token {
                self.refuse(from, &locator, Refusal::RoomTaken);
                return;
            }
            if host == from {
                if let Some(room) = self.rooms.get_mut(&locator) {
                    room.last_heard_ms = now;
                }
                self.answer(from, &envelope::registered(&locator));
                return;
            }
        }
        if !self.cookie_ok(from, &locator, Role::Host, &cookie) {
            let cookie = self.cookie_for(from, &locator, Role::Host);
            self.answer(from, &envelope::register_challenge(&locator, &cookie));
            return;
        }
        if existing.is_none() && self.rooms.len() >= self.config.max_rooms as usize {
            self.refuse(from, &locator, Refusal::RelayFull);
            return;
        }
        // An address holds one room, and is not a client at the same time.
        if let Some(other) = self.hosts.get(&from).copied()
            && other != locator
        {
            self.close_room(&other, RoomClosed::Replaced);
        }
        self.drop_client(from, ClientLeft::Moved);
        let tag = RoomTag::of(&locator);
        match existing {
            Some(old_host) => {
                self.hosts.remove(&old_host);
                if let Some(room) = self.rooms.get_mut(&locator) {
                    room.host = from;
                    room.last_heard_ms = now;
                }
                self.event(RelayEvent::RoomMoved {
                    room: tag,
                    host: from,
                });
            }
            None => {
                self.rooms.insert(
                    locator,
                    Room {
                        host: from,
                        token,
                        last_heard_ms: now,
                        next_slot: 1,
                        slots: BTreeMap::new(),
                        budget: Budget::new(
                            self.config.room_packets_per_second,
                            self.config.room_bytes_per_second,
                        ),
                    },
                );
                self.event(RelayEvent::RoomOpened {
                    room: tag,
                    host: from,
                });
            }
        }
        self.hosts.insert(from, locator);
        self.answer(from, &envelope::registered(&locator));
    }

    fn unregister(&mut self, from: SocketAddr, locator: Locator, token: [u8; HOST_TOKEN_LEN]) {
        let owned = self
            .rooms
            .get(&locator)
            .is_some_and(|room| room.host == from && same_bytes(&room.token, &token));
        if owned {
            self.close_room(&locator, RoomClosed::ByHost);
        } else {
            self.stats.dropped_unknown += 1;
        }
    }

    fn attach(&mut self, from: SocketAddr, locator: Locator, cookie: [u8; RELAY_COOKIE_LEN]) {
        let now = self.now_ms;
        if self.hosts.contains_key(&from) {
            // A host's own address cannot be a client of a room as well.
            self.stats.dropped_unknown += 1;
            return;
        }
        let Some(room) = self.rooms.get(&locator) else {
            self.refuse(from, &locator, Refusal::NoRoom);
            return;
        };
        let room_clients = room.slots.len();
        let exhausted = room.next_slot == u32::MAX;
        if let Some(client) = self.clients.get_mut(&from)
            && client.locator == locator
        {
            // A lost `Attached`: say it again.
            client.last_heard_ms = now;
            self.answer(from, &envelope::attached(&locator));
            return;
        }
        if !self.cookie_ok(from, &locator, Role::Client, &cookie) {
            let cookie = self.cookie_for(from, &locator, Role::Client);
            self.answer(from, &envelope::attach_challenge(&locator, &cookie));
            return;
        }
        if room_clients >= self.config.max_peers as usize || exhausted {
            self.refuse(from, &locator, Refusal::RoomFull);
            return;
        }
        let moving = self.clients.contains_key(&from);
        if !moving && self.clients.len() >= self.config.max_clients as usize {
            self.refuse(from, &locator, Refusal::RelayFull);
            return;
        }
        self.drop_client(from, ClientLeft::Moved);
        let Some(room) = self.rooms.get_mut(&locator) else {
            return;
        };
        let slot = room.next_slot;
        room.next_slot += 1;
        room.slots.insert(slot, from);
        self.clients.insert(
            from,
            Client {
                locator,
                slot,
                last_heard_ms: now,
                budget: Budget::new(
                    self.config.client_packets_per_second,
                    self.config.client_bytes_per_second,
                ),
            },
        );
        self.event(RelayEvent::ClientAttached {
            room: RoomTag::of(&locator),
            slot,
            addr: from,
        });
        self.answer(from, &envelope::attached(&locator));
    }

    fn not_attached(&mut self, from: SocketAddr) {
        self.stats.dropped_unknown += 1;
        self.answer(from, &envelope::not_attached());
    }

    fn host_send(&mut self, from: SocketAddr, slot: u32, inner: &[u8]) {
        let now = self.now_ms;
        let Some(locator) = self.hosts.get(&from).copied() else {
            self.not_attached(from);
            return;
        };
        let Some(room) = self.rooms.get_mut(&locator) else {
            self.not_attached(from);
            return;
        };
        room.last_heard_ms = now;
        let Some(to) = room.slots.get(&slot).copied() else {
            self.stats.dropped_unknown += 1;
            return;
        };
        let len = envelope::RELAY_HEADER_LEN + inner.len();
        if !room.budget.can(now, len) {
            self.stats.dropped_rate += 1;
            return;
        }
        room.budget.spend(len);
        let out = envelope::to_client(inner);
        self.stats.forwarded += 1;
        self.stats.forwarded_bytes += out.len() as u64;
        self.send(to, &out);
    }

    fn client_send(&mut self, from: SocketAddr, inner: &[u8]) {
        let now = self.now_ms;
        let len = envelope::SLOT_ENVELOPE_LEN + inner.len();
        let Some(client) = self.clients.get_mut(&from) else {
            self.not_attached(from);
            return;
        };
        client.last_heard_ms = now;
        // The client's own budget first, so one client cannot spend its
        // room's.
        if !client.budget.can(now, len) {
            self.stats.dropped_rate += 1;
            return;
        }
        client.budget.spend(len);
        let (locator, slot) = (client.locator, client.slot);
        let Some(room) = self.rooms.get_mut(&locator) else {
            self.stats.dropped_unknown += 1;
            return;
        };
        if !room.budget.can(now, len) {
            self.stats.dropped_rate += 1;
            return;
        }
        room.budget.spend(len);
        let host = room.host;
        let out = envelope::to_host(slot, inner);
        self.stats.forwarded += 1;
        self.stats.forwarded_bytes += out.len() as u64;
        self.send(host, &out);
    }

    /// Takes `addr`'s slot away, if it has one.
    fn drop_client(&mut self, addr: SocketAddr, reason: ClientLeft) {
        let Some(client) = self.clients.remove(&addr) else {
            return;
        };
        if let Some(room) = self.rooms.get_mut(&client.locator) {
            room.slots.remove(&client.slot);
        }
        self.event(RelayEvent::ClientLeft {
            room: RoomTag::of(&client.locator),
            slot: client.slot,
            reason,
        });
    }

    fn close_room(&mut self, locator: &Locator, reason: RoomClosed) {
        let Some(room) = self.rooms.remove(locator) else {
            return;
        };
        if self.hosts.get(&room.host) == Some(locator) {
            self.hosts.remove(&room.host);
        }
        for addr in room.slots.values() {
            self.clients.remove(addr);
        }
        self.event(RelayEvent::RoomClosed {
            room: RoomTag::of(locator),
            reason,
            clients: room.slots.len(),
        });
    }

    fn sweep(&mut self) {
        let now = self.now_ms;
        let room_idle = self.config.room_idle_ms;
        let client_idle = self.config.client_idle_ms;
        let idle_rooms: Vec<Locator> = self
            .rooms
            .iter()
            .filter(|(_, room)| now.saturating_sub(room.last_heard_ms) > room_idle)
            .map(|(locator, _)| *locator)
            .collect();
        for locator in idle_rooms {
            self.close_room(&locator, RoomClosed::Idle);
        }
        let idle_clients: Vec<SocketAddr> = self
            .clients
            .iter()
            .filter(|(_, client)| now.saturating_sub(client.last_heard_ms) > client_idle)
            .map(|(addr, _)| *addr)
            .collect();
        for addr in idle_clients {
            self.drop_client(addr, ClientLeft::Idle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_out_of_range_are_explained() {
        assert!(RelayConfig::default().validate().is_ok());
        for config in [
            RelayConfig {
                max_rooms: 0,
                ..RelayConfig::default()
            },
            RelayConfig {
                max_peers: 257,
                ..RelayConfig::default()
            },
            RelayConfig {
                max_clients: 0,
                ..RelayConfig::default()
            },
            RelayConfig {
                room_idle_ms: 10,
                ..RelayConfig::default()
            },
            RelayConfig {
                client_packets_per_second: 1,
                ..RelayConfig::default()
            },
            RelayConfig {
                room_bytes_per_second: 100,
                ..RelayConfig::default()
            },
        ] {
            let error = config.validate().unwrap_err();
            assert_eq!(error.code, "net_config_invalid");
            assert!(!error.hint.is_empty());
        }
    }

    #[test]
    fn rates_refill_with_time_and_burst_one_second() {
        let mut rate = Rate::new(10);
        assert!((0..10).all(|_| rate.take(0)));
        assert!(!rate.take(0));
        assert!(rate.take(100));
        assert!(!rate.take(100));
        // Long silence refills one second's worth, no more.
        assert_eq!((0..20).filter(|_| rate.take(60_000)).count(), 10);
    }

    #[test]
    fn room_tags_print_as_hex() {
        assert_eq!(
            RoomTag::of(&[0xAB, 0x01, 0x00, 0xFF, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]).to_string(),
            "ab0100ff"
        );
    }
}
