//! An endpoint: a host with many peers, or a client with one host.
//!
//! The caller drives it: [`Endpoint::update`] with the time in milliseconds
//! reads what arrived, sends what is due (handshake packets, resends,
//! acknowledgements, keepalives, queued messages) and ends connections that
//! went silent; [`Endpoint::poll_event`] gives what happened;
//! [`Endpoint::send`] queues a message for the next update. Nothing here
//! reads the clock or depends on hash order, so an endpoint over the
//! simulator is deterministic.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;

use wish_core::{Diagnostic, Result};

use crate::crypto::{
    self, CONNECT_NUMBER, COOKIE_LEN, Entropy, KeyPair, PUBLIC_KEY_LEN, SessionKeys, TOKEN_LEN,
    same_bytes,
};
use crate::room::RoomCode;
use crate::transport::{Recv, Transport};
use crate::wire::{self, FRAGMENT_LEN, Frame, MAX_PACKET, MAX_PLAINTEXT, Packet, RejectReason};

/// The most peers a host can be set to take.
pub const MAX_PEERS_LIMIT: u32 = 256;
/// The largest message size an endpoint can be set to allow (1 MiB).
pub const MAX_MESSAGE_LIMIT: usize = 1 << 20;
/// Datagrams one update reads at most; the rest wait for the next update,
/// so a flood cannot keep an endpoint busy.
pub const MAX_READS_PER_UPDATE: usize = 4096;
/// Reliable fragments in flight at most (from the oldest unacknowledged).
pub const SEND_WINDOW: u32 = 256;
/// Reliable fragments a receiver keeps ahead of the one it waits for.
pub const RECV_WINDOW: u32 = 512;
/// Partial unreliable messages a receiver keeps per peer.
pub const MAX_PARTIAL: usize = 8;
/// How long a partial unreliable message is kept, in milliseconds.
pub const PARTIAL_LIFETIME_MS: u64 = 1000;
/// Handshake packets (from all addresses together) a host answers per
/// second.
pub const HANDSHAKES_PER_SECOND: u32 = 256;
/// Key agreements a host does per second.
pub const KEY_AGREEMENTS_PER_SECOND: u32 = 64;
/// How long one cookie bucket lasts, in milliseconds (a cookie is valid in
/// its bucket and the next).
pub const COOKIE_BUCKET_MS: u64 = 5000;
const RTO_MIN_MS: u64 = 50;
const RTO_MAX_MS: u64 = 2000;
const RTO_INITIAL_MS: u64 = 300;
const RTT_MAX_MS: u64 = 60_000;

/// A peer of an endpoint. On a host, peers are numbered from 1 in the order
/// they joined (never reused); on a client, the host is [`PeerId::HOST`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u32);

impl PeerId {
    /// The host, as a client sees it.
    pub const HOST: Self = Self(0);
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {}", self.0)
    }
}

/// How a message travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// Arrives exactly once and in the order sent (resent until
    /// acknowledged). For messages, joins and inputs.
    Reliable,
    /// Arrives at most once, whole, in any order, or not at all. For state
    /// that the next update replaces.
    Unreliable,
}

/// The settings of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The room code a client must give (a host checks it).
    pub room_code: RoomCode,
    /// Peers a host takes (1 to [`MAX_PEERS_LIMIT`]).
    pub max_peers: u32,
    /// The largest message, in bytes (1 to [`MAX_MESSAGE_LIMIT`]), sent or
    /// received.
    pub max_message_size: usize,
    /// Bytes waiting to be sent or acknowledged per peer before `send`
    /// refuses more (at least `max_message_size`).
    pub max_queued_bytes: usize,
    /// A peer silent for this long is disconnected, in milliseconds (at
    /// least twice `keepalive_ms`).
    pub timeout_ms: u64,
    /// A client that is not connected after this long gives up, in
    /// milliseconds.
    pub connect_timeout_ms: u64,
    /// A ping is sent this often, in milliseconds (at least 1).
    pub keepalive_ms: u64,
    /// Handshake packets are sent again this often, in milliseconds (at
    /// least 1).
    pub handshake_resend_ms: u64,
    /// Datagrams accepted per second from one peer (at least 10); more are
    /// dropped.
    pub max_packets_per_second: u32,
    /// A client joins only a host with this public key, when set (from
    /// discovery, or given by hand). Without it, an active attacker on the
    /// path could stand between the two.
    pub host_key: Option<[u8; PUBLIC_KEY_LEN]>,
}

impl Config {
    /// The defaults, with a room code.
    pub fn new(room_code: RoomCode) -> Self {
        Self {
            room_code,
            max_peers: 16,
            max_message_size: 64 * 1024,
            max_queued_bytes: 4 << 20,
            timeout_ms: 10_000,
            connect_timeout_ms: 10_000,
            keepalive_ms: 500,
            handshake_resend_ms: 250,
            max_packets_per_second: 1000,
            host_key: None,
        }
    }

    /// Checks every setting against its range.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String, hint: &str| {
            Err(
                Diagnostic::error("net_config_invalid", message, hint.to_owned())
                    .at_path(format!("/{field}")),
            )
        };
        if !(1..=MAX_PEERS_LIMIT).contains(&self.max_peers) {
            return bad(
                "max_peers",
                format!("max_peers is {}", self.max_peers),
                "Set max_peers from 1 to 256.",
            );
        }
        if !(1..=MAX_MESSAGE_LIMIT).contains(&self.max_message_size) {
            return bad(
                "max_message_size",
                format!("max_message_size is {}", self.max_message_size),
                "Set max_message_size from 1 byte to 1 MiB (1048576).",
            );
        }
        if self.max_queued_bytes < self.max_message_size {
            return bad(
                "max_queued_bytes",
                format!(
                    "max_queued_bytes ({}) is smaller than max_message_size ({})",
                    self.max_queued_bytes, self.max_message_size
                ),
                "Let the queue hold at least one message of the largest size.",
            );
        }
        if self.keepalive_ms == 0 || self.handshake_resend_ms == 0 {
            return bad(
                "keepalive_ms",
                "keepalive_ms and handshake_resend_ms must be at least 1".to_owned(),
                "Give both intervals in milliseconds, at least 1 (the defaults are 500 and 250).",
            );
        }
        if self.timeout_ms < self.keepalive_ms.saturating_mul(2) {
            return bad(
                "timeout_ms",
                format!(
                    "timeout_ms ({}) is less than twice keepalive_ms ({})",
                    self.timeout_ms, self.keepalive_ms
                ),
                "A peer must have time for at least two pings before it times out.",
            );
        }
        if self.connect_timeout_ms < self.handshake_resend_ms {
            return bad(
                "connect_timeout_ms",
                format!("connect_timeout_ms is {}", self.connect_timeout_ms),
                "Give a client time to send its handshake at least once (at least handshake_resend_ms).",
            );
        }
        if self.max_packets_per_second < 10 {
            return bad(
                "max_packets_per_second",
                format!("max_packets_per_second is {}", self.max_packets_per_second),
                "Allow at least 10 datagrams a second (the default is 1000).",
            );
        }
        Ok(())
    }

    fn max_fragments(&self) -> u16 {
        u16::try_from(self.max_message_size.div_ceil(FRAGMENT_LEN).max(1)).unwrap_or(u16::MAX)
    }
}

/// Why a connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The other side closed it.
    Closed,
    /// This side closed it ([`Endpoint::disconnect`] or [`Endpoint::close`]).
    Local,
    /// Nothing arrived from the peer for `timeout_ms`.
    TimedOut,
    /// A client could not reach the host within `connect_timeout_ms`.
    ConnectTimedOut,
    /// The host refused the room code.
    RoomCodeWrong,
    /// The host's match is full.
    RoomFull,
    /// The other side speaks another protocol version.
    VersionMismatch,
    /// The host's key is not the one the client pinned.
    HostKeyMismatch,
    /// The other side sent a key that gives no shared secret.
    KeyRejected,
    /// The peer sent frames that break the protocol.
    ProtocolViolation,
    /// The same address joined again with a new key (a restarted client).
    Replaced,
}

impl Reason {
    /// The diagnostic code of the reason.
    pub fn code(self) -> &'static str {
        match self {
            Self::Closed | Self::Local => "net_closed",
            Self::TimedOut => "net_timed_out",
            Self::ConnectTimedOut => "net_connect_timed_out",
            Self::RoomCodeWrong => "net_room_code_wrong",
            Self::RoomFull => "net_room_full",
            Self::VersionMismatch => "net_version_mismatch",
            Self::HostKeyMismatch => "net_host_key_mismatch",
            Self::KeyRejected => "net_key_rejected",
            Self::ProtocolViolation => "net_protocol_violation",
            Self::Replaced => "net_replaced",
        }
    }

    /// The reason as a diagnostic about `peer`.
    pub fn diagnostic(self, peer: PeerId) -> Diagnostic {
        let diagnostic = match self {
            Self::Closed => Diagnostic::info(
                "net_closed",
                "the other side closed the connection",
                "Nothing went wrong on this side; join or host again to play on.",
            ),
            Self::Local => Diagnostic::info(
                "net_closed",
                "this side closed the connection",
                "Nothing went wrong; join or host again to play on.",
            ),
            Self::TimedOut => Diagnostic::error(
                "net_timed_out",
                "nothing arrived from the other side for too long",
                "The other machine left, crashed or lost its network. Check its connection and join again.",
            ),
            Self::ConnectTimedOut => Diagnostic::error(
                "net_connect_timed_out",
                "the host did not answer",
                "Check the address and port, that the host is running, and that a firewall lets UDP through; on the internet, use a relay.",
            ),
            Self::RoomCodeWrong => Diagnostic::error(
                "net_room_code_wrong",
                "the host refused the room code",
                "Type the room code the host shows again (letters and digits, no I, O, 0 or 1).",
            ),
            Self::RoomFull => Diagnostic::error(
                "net_room_full",
                "the match has as many players as it takes",
                "Wait for a player to leave, or ask the host to allow more players.",
            ),
            Self::VersionMismatch => Diagnostic::error(
                "net_version_mismatch",
                "the other side speaks another version of the network protocol",
                "Both machines must run the same release of the game or the engine; update the older one.",
            ),
            Self::HostKeyMismatch => Diagnostic::error(
                "net_host_key_mismatch",
                "the host's key is not the one this client expected",
                "Someone may stand between you and the host, or the host started a new match: get the host's key again (discovery shows it) and join again.",
            ),
            Self::KeyRejected => Diagnostic::error(
                "net_key_rejected",
                "the other side sent a key that gives no shared secret",
                "The other program is broken or hostile; nothing was sent to it. Join again, or ask the host to check its program.",
            ),
            Self::ProtocolViolation => Diagnostic::error(
                "net_protocol_violation",
                "the other side sent packets that break the protocol, so the connection was closed",
                "The other program is broken or hostile. If both run the same release, report it with the network log.",
            ),
            Self::Replaced => Diagnostic::info(
                "net_replaced",
                "the same address joined again with a new key, so the old connection was closed",
                "A player's game restarted and joined again; nothing needs doing.",
            ),
        };
        diagnostic.at_path(format!("/peers/{}", peer.0))
    }
}

/// What happened at an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A peer joined (on a host), or the host accepted this client.
    Connected {
        /// Who.
        peer: PeerId,
    },
    /// A connection ended.
    Disconnected {
        /// Who.
        peer: PeerId,
        /// Why ([`Reason::diagnostic`] explains it).
        reason: Reason,
    },
    /// A whole message arrived.
    Message {
        /// From whom.
        peer: PeerId,
        /// On which channel.
        channel: Channel,
        /// Its bytes.
        data: Vec<u8>,
    },
}

/// What an endpoint did, counted since it was made. The `dropped_*` counts
/// are datagrams that were read and thrown away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndpointStats {
    /// Datagrams sent.
    pub packets_sent: u64,
    /// Bytes sent.
    pub bytes_sent: u64,
    /// Datagrams read.
    pub packets_received: u64,
    /// Bytes read.
    pub bytes_received: u64,
    /// Reliable fragments sent again.
    pub resent: u64,
    /// Larger than [`MAX_PACKET`], or unreadable.
    pub dropped_oversized: u64,
    /// Not a packet of this protocol, or not well formed.
    pub dropped_malformed: u64,
    /// From an address with no connection, or a kind this side does not
    /// take.
    pub dropped_unknown: u64,
    /// Sealed, but not by the peer's key (tampered or forged).
    pub dropped_unauthentic: u64,
    /// A packet number seen before, or too old.
    pub dropped_replayed: u64,
    /// Over a rate limit.
    pub dropped_rate: u64,
    /// Reliable fragments too far ahead of the receive window.
    pub dropped_window: u64,
    /// Handshakes refused: a stale or forged cookie, a wrong room code, a
    /// full room, a bad key.
    pub handshakes_refused: u64,
    /// Sends the transport refused.
    pub send_errors: u64,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    milli: u64,
    last_ms: u64,
    rate: u64,
}

impl Bucket {
    fn new(per_second: u32) -> Self {
        let rate = u64::from(per_second);
        Self {
            milli: rate * 1000,
            last_ms: 0,
            rate,
        }
    }

    fn take(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        // A second's worth of tokens at most: bursts up to the rate.
        self.milli = self
            .milli
            .saturating_add(elapsed.saturating_mul(self.rate))
            .min(self.rate * 1000);
        if self.milli >= 1000 {
            self.milli -= 1000;
            true
        } else {
            false
        }
    }
}

/// Which numbers were seen among the last 128.
#[derive(Debug, Clone, Copy, Default)]
struct Window {
    highest: Option<u64>,
    bits: u128,
}

impl Window {
    fn fresh(&self, n: u64) -> bool {
        match self.highest {
            None => true,
            Some(h) if n > h => true,
            Some(h) => {
                let back = h - n;
                back < 128 && self.bits & (1u128 << back) == 0
            }
        }
    }

    fn mark(&mut self, n: u64) {
        match self.highest {
            None => {
                self.highest = Some(n);
                self.bits = 1;
            }
            Some(h) if n > h => {
                let shift = n - h;
                self.bits = if shift >= 128 { 0 } else { self.bits << shift };
                self.bits |= 1;
                self.highest = Some(n);
            }
            Some(h) => {
                let back = h - n;
                if back < 128 {
                    self.bits |= 1u128 << back;
                }
            }
        }
    }
}

struct OutFragment {
    seq: u32,
    index: u16,
    count: u16,
    bytes: Vec<u8>,
}

struct InFlight {
    index: u16,
    count: u16,
    bytes: Vec<u8>,
    last_sent_ms: u64,
    rto_ms: u64,
}

struct Partial {
    count: u16,
    parts: Vec<Option<Vec<u8>>>,
    have: u16,
    bytes: usize,
    first_ms: u64,
}

enum Outcome {
    Go,
    Violation,
    Closed,
}

struct Connection {
    id: PeerId,
    wire_id: u32,
    addr: SocketAddr,
    client_key: [u8; PUBLIC_KEY_LEN],
    keys: SessionKeys,
    connected: bool,
    welcome_due: bool,
    next_number: u64,
    replay: Window,
    rate: Bucket,
    last_received_ms: u64,
    last_ping_ms: Option<u64>,
    pong_due: Option<u64>,
    srtt_ms: Option<u64>,
    next_seq: u32,
    queue: VecDeque<OutFragment>,
    queued_bytes: usize,
    in_flight: BTreeMap<u32, InFlight>,
    recv_next: u32,
    recv_buffer: BTreeMap<u32, (u16, u16, Vec<u8>)>,
    assembling: Vec<u8>,
    assembling_count: u16,
    assembling_next: u16,
    ack_due: bool,
    next_unreliable: u32,
    unreliable_out: VecDeque<Vec<u8>>,
    partial: BTreeMap<u32, Partial>,
    delivered: Window,
}

struct NewConnection {
    id: PeerId,
    wire_id: u32,
    addr: SocketAddr,
    client_key: [u8; PUBLIC_KEY_LEN],
    keys: SessionKeys,
    connected: bool,
    now_ms: u64,
    packets_per_second: u32,
}

impl Connection {
    fn new(new: NewConnection) -> Self {
        Self {
            id: new.id,
            wire_id: new.wire_id,
            addr: new.addr,
            client_key: new.client_key,
            keys: new.keys,
            connected: new.connected,
            welcome_due: new.connected,
            next_number: 0,
            replay: Window::default(),
            rate: Bucket::new(new.packets_per_second),
            last_received_ms: new.now_ms,
            last_ping_ms: None,
            pong_due: None,
            srtt_ms: None,
            next_seq: 0,
            queue: VecDeque::new(),
            queued_bytes: 0,
            in_flight: BTreeMap::new(),
            recv_next: 0,
            recv_buffer: BTreeMap::new(),
            assembling: Vec::new(),
            assembling_count: 0,
            assembling_next: 0,
            ack_due: false,
            next_unreliable: 0,
            unreliable_out: VecDeque::new(),
            partial: BTreeMap::new(),
            delivered: Window::default(),
        }
    }

    fn rto_ms(&self) -> u64 {
        match self.srtt_ms {
            Some(srtt) => (srtt + srtt / 2 + 25).clamp(RTO_MIN_MS, RTO_MAX_MS),
            None => RTO_INITIAL_MS,
        }
    }

    fn ack_mask(&self) -> u64 {
        let mut mask = 0u64;
        for i in 0..64u32 {
            let Some(seq) = self.recv_next.checked_add(1 + i) else {
                break;
            };
            if self.recv_buffer.contains_key(&seq) {
                mask |= 1 << i;
            }
        }
        mask
    }

    fn on_ack(&mut self, next: u32, mask: u64) {
        let rest = self.in_flight.split_off(&next);
        let acked = std::mem::replace(&mut self.in_flight, rest);
        for fragment in acked.values() {
            self.queued_bytes = self.queued_bytes.saturating_sub(fragment.bytes.len());
        }
        for i in 0..64u32 {
            if mask >> i & 1 == 0 {
                continue;
            }
            let Some(seq) = next.checked_add(1 + i) else {
                break;
            };
            if let Some(fragment) = self.in_flight.remove(&seq) {
                self.queued_bytes = self.queued_bytes.saturating_sub(fragment.bytes.len());
            }
        }
    }

    fn on_pong(&mut self, time: u64, now_ms: u64) {
        if time > now_ms || now_ms - time > RTT_MAX_MS {
            return;
        }
        let rtt = now_ms - time;
        self.srtt_ms = Some(match self.srtt_ms {
            None => rtt,
            Some(srtt) => (srtt * 7 + rtt) / 8,
        });
    }

    fn on_reliable(
        &mut self,
        fragment: (u32, u16, u16, &[u8]),
        config: &Config,
        stats: &mut EndpointStats,
        delivered: &mut Vec<(Channel, Vec<u8>)>,
    ) -> Outcome {
        let (seq, index, count, bytes) = fragment;
        self.ack_due = true;
        if count > config.max_fragments() {
            return Outcome::Violation;
        }
        if seq < self.recv_next {
            return Outcome::Go;
        }
        if seq - self.recv_next >= RECV_WINDOW {
            stats.dropped_window += 1;
            return Outcome::Go;
        }
        self.recv_buffer
            .entry(seq)
            .or_insert_with(|| (index, count, bytes.to_vec()));
        while let Some((index, count, bytes)) = self.recv_buffer.remove(&self.recv_next) {
            if self.assembling_next == 0 {
                if index != 0 {
                    return Outcome::Violation;
                }
                self.assembling_count = count;
            } else if index != self.assembling_next || count != self.assembling_count {
                return Outcome::Violation;
            }
            if self.assembling.len() + bytes.len() > config.max_message_size {
                return Outcome::Violation;
            }
            self.assembling.extend_from_slice(&bytes);
            self.assembling_next += 1;
            let Some(next) = self.recv_next.checked_add(1) else {
                return Outcome::Violation;
            };
            self.recv_next = next;
            if self.assembling_next == self.assembling_count {
                delivered.push((Channel::Reliable, std::mem::take(&mut self.assembling)));
                self.assembling_next = 0;
                self.assembling_count = 0;
            }
        }
        Outcome::Go
    }

    fn on_unreliable(
        &mut self,
        fragment: (u32, u16, u16, &[u8]),
        config: &Config,
        now_ms: u64,
        delivered: &mut Vec<(Channel, Vec<u8>)>,
    ) -> Outcome {
        let (id, index, count, bytes) = fragment;
        if count > config.max_fragments() {
            return Outcome::Violation;
        }
        if !self.delivered.fresh(u64::from(id)) {
            return Outcome::Go;
        }
        if count == 1 {
            self.delivered.mark(u64::from(id));
            delivered.push((Channel::Unreliable, bytes.to_vec()));
            return Outcome::Go;
        }
        if !self.partial.contains_key(&id) {
            if self.partial.len() >= MAX_PARTIAL {
                self.partial.pop_first();
            }
            self.partial.insert(
                id,
                Partial {
                    count,
                    parts: vec![None; usize::from(count)],
                    have: 0,
                    bytes: 0,
                    first_ms: now_ms,
                },
            );
        }
        let Some(partial) = self.partial.get_mut(&id) else {
            return Outcome::Go;
        };
        if partial.count != count {
            return Outcome::Violation;
        }
        let Some(slot) = partial.parts.get_mut(usize::from(index)) else {
            return Outcome::Violation;
        };
        if slot.is_none() {
            if partial.bytes + bytes.len() > config.max_message_size {
                return Outcome::Violation;
            }
            partial.bytes += bytes.len();
            *slot = Some(bytes.to_vec());
            partial.have += 1;
        }
        if partial.have == partial.count
            && let Some(partial) = self.partial.remove(&id)
        {
            let mut message = Vec::with_capacity(partial.bytes);
            for part in partial.parts.into_iter().flatten() {
                message.extend_from_slice(&part);
            }
            self.delivered.mark(u64::from(id));
            delivered.push((Channel::Unreliable, message));
        }
        Outcome::Go
    }

    /// The frames due now, packed into packet plaintexts.
    fn pack(&mut self, now_ms: u64, keepalive_ms: u64, stats: &mut EndpointStats) -> Vec<Vec<u8>> {
        let mut packer = Packer::default();
        if self.welcome_due {
            self.welcome_due = false;
            packer.push(&Frame::Welcome { peer: self.wire_id });
        }
        if self.ack_due {
            self.ack_due = false;
            packer.push(&Frame::Ack {
                next: self.recv_next,
                mask: self.ack_mask(),
            });
        }
        if let Some(time) = self.pong_due.take() {
            packer.push(&Frame::Pong { time });
        }
        if self
            .last_ping_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= keepalive_ms)
        {
            self.last_ping_ms = Some(now_ms);
            packer.push(&Frame::Ping { time: now_ms });
        }
        for (&seq, fragment) in &mut self.in_flight {
            if now_ms >= fragment.last_sent_ms.saturating_add(fragment.rto_ms) {
                packer.push(&Frame::Reliable {
                    seq,
                    index: fragment.index,
                    count: fragment.count,
                    bytes: &fragment.bytes,
                });
                fragment.last_sent_ms = now_ms;
                fragment.rto_ms = (fragment.rto_ms * 2).min(RTO_MAX_MS);
                stats.resent += 1;
            }
        }
        let rto = self.rto_ms();
        while let Some(front) = self.queue.front() {
            let lowest = self.in_flight.keys().next().copied().unwrap_or(front.seq);
            if front.seq - lowest >= SEND_WINDOW {
                break;
            }
            let Some(fragment) = self.queue.pop_front() else {
                break;
            };
            packer.push(&Frame::Reliable {
                seq: fragment.seq,
                index: fragment.index,
                count: fragment.count,
                bytes: &fragment.bytes,
            });
            self.in_flight.insert(
                fragment.seq,
                InFlight {
                    index: fragment.index,
                    count: fragment.count,
                    bytes: fragment.bytes,
                    last_sent_ms: now_ms,
                    rto_ms: rto,
                },
            );
        }
        while let Some(message) = self.unreliable_out.pop_front() {
            self.queued_bytes = self.queued_bytes.saturating_sub(message.len());
            let id = self.next_unreliable;
            self.next_unreliable = self.next_unreliable.wrapping_add(1);
            let count = message.len().div_ceil(FRAGMENT_LEN).max(1);
            let count = u16::try_from(count).unwrap_or(u16::MAX);
            for index in 0..count {
                let start = usize::from(index) * FRAGMENT_LEN;
                let end = (start + FRAGMENT_LEN).min(message.len());
                packer.push(&Frame::Unreliable {
                    id,
                    index,
                    count,
                    bytes: message.get(start..end).unwrap_or_default(),
                });
            }
        }
        packer.finish()
    }
}

#[derive(Default)]
struct Packer {
    current: Vec<u8>,
    done: Vec<Vec<u8>>,
}

impl Packer {
    fn push(&mut self, frame: &Frame<'_>) {
        if !self.current.is_empty() && self.current.len() + frame.encoded_len() > MAX_PLAINTEXT {
            self.done.push(std::mem::take(&mut self.current));
        }
        frame.encode(&mut self.current);
    }

    fn finish(mut self) -> Vec<Vec<u8>> {
        if !self.current.is_empty() {
            self.done.push(self.current);
        }
        self.done
    }
}

struct HostSide {
    keys: KeyPair,
    cookie_secret: [u8; 32],
    next_peer: u32,
    handshakes: Bucket,
    agreements: Bucket,
}

enum Stage {
    Hello,
    Connect { packet: Vec<u8>, challenged_ms: u64 },
    Connected,
    Closed,
}

struct ClientSide {
    host_addr: SocketAddr,
    keys: KeyPair,
    token: [u8; TOKEN_LEN],
    started_ms: Option<u64>,
    last_sent_ms: Option<u64>,
    stage: Stage,
    local_id: Option<PeerId>,
}

enum Role {
    Host(Box<HostSide>),
    Client(Box<ClientSide>),
}

/// A host or a client of the protocol, over any [`Transport`].
pub struct Endpoint<T: Transport> {
    transport: T,
    config: Config,
    role: Role,
    peers: BTreeMap<PeerId, Connection>,
    by_addr: BTreeMap<SocketAddr, PeerId>,
    events: VecDeque<Event>,
    stats: EndpointStats,
    now_ms: u64,
    buf: Vec<u8>,
}

impl<T: Transport> fmt::Debug for Endpoint<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("host", &self.is_host())
            .field("local_addr", &self.transport.local_addr())
            .field("peers", &self.peers.keys().collect::<Vec<_>>())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> Endpoint<T> {
    fn with_role(transport: T, config: Config, role: Role) -> Self {
        Self {
            transport,
            config,
            role,
            peers: BTreeMap::new(),
            by_addr: BTreeMap::new(),
            events: VecDeque::new(),
            stats: EndpointStats::default(),
            now_ms: 0,
            buf: vec![0; MAX_PACKET + 1],
        }
    }

    /// Hosts a match on `transport`. Clients join with the config's room
    /// code. The host's key pair is new for this match.
    pub fn host(transport: T, config: Config, mut entropy: Entropy) -> Result<Self> {
        config.validate()?;
        let host = HostSide {
            keys: KeyPair::generate(&mut entropy),
            cookie_secret: entropy.bytes(),
            next_peer: 1,
            handshakes: Bucket::new(HANDSHAKES_PER_SECOND),
            agreements: Bucket::new(KEY_AGREEMENTS_PER_SECOND),
        };
        Ok(Self::with_role(
            transport,
            config,
            Role::Host(Box::new(host)),
        ))
    }

    /// Joins the match hosted at `host_addr`. The handshake starts at the
    /// first [`update`](Self::update); an [`Event::Connected`] for
    /// [`PeerId::HOST`] says it succeeded, an [`Event::Disconnected`] that it
    /// failed.
    pub fn connect(
        transport: T,
        host_addr: SocketAddr,
        config: Config,
        mut entropy: Entropy,
    ) -> Result<Self> {
        config.validate()?;
        let client = ClientSide {
            host_addr,
            keys: KeyPair::generate(&mut entropy),
            token: entropy.bytes(),
            started_ms: None,
            last_sent_ms: None,
            stage: Stage::Hello,
            local_id: None,
        };
        Ok(Self::with_role(
            transport,
            config,
            Role::Client(Box::new(client)),
        ))
    }

    /// Whether this endpoint hosts.
    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host(_))
    }

    /// This endpoint's own id: [`PeerId::HOST`] on a host; on a client, the
    /// id the host gave it once connected.
    pub fn local_id(&self) -> Option<PeerId> {
        match &self.role {
            Role::Host(_) => Some(PeerId::HOST),
            Role::Client(client) => client.local_id,
        }
    }

    /// This endpoint's public key (on a host, what discovery announces and
    /// clients may pin).
    pub fn public_key(&self) -> [u8; PUBLIC_KEY_LEN] {
        match &self.role {
            Role::Host(host) => host.keys.public(),
            Role::Client(client) => client.keys.public(),
        }
    }

    /// Whether a client's connection is over (it failed or ended). A host is
    /// never closed.
    pub fn is_closed(&self) -> bool {
        matches!(&self.role, Role::Client(client) if matches!(client.stage, Stage::Closed))
    }

    /// The address this endpoint receives on.
    pub fn local_addr(&self) -> SocketAddr {
        self.transport.local_addr()
    }

    /// The connected peers, in id order.
    pub fn peers(&self) -> Vec<PeerId> {
        self.peers
            .values()
            .filter(|c| c.connected)
            .map(|c| c.id)
            .collect()
    }

    /// A peer's address.
    pub fn peer_addr(&self, peer: PeerId) -> Option<SocketAddr> {
        self.peers.get(&peer).map(|c| c.addr)
    }

    /// The smoothed round trip to a peer, in milliseconds, once measured.
    pub fn rtt_ms(&self, peer: PeerId) -> Option<u64> {
        self.peers.get(&peer).and_then(|c| c.srtt_ms)
    }

    /// Bytes queued for a peer that are not yet sent (unreliable) or not
    /// yet acknowledged (reliable).
    pub fn queued_bytes(&self, peer: PeerId) -> usize {
        self.peers.get(&peer).map_or(0, |c| c.queued_bytes)
    }

    /// What this endpoint did so far.
    pub fn stats(&self) -> EndpointStats {
        self.stats
    }

    /// The settings.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The next event, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Queues a message to `peer`; it goes out at the next update.
    pub fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<()> {
        let max_message = self.config.max_message_size;
        let max_queued = self.config.max_queued_bytes;
        let Some(conn) = self.peers.get_mut(&peer) else {
            return Err(Diagnostic::error(
                "net_unknown_peer",
                format!("there is no connection to {peer}"),
                "The peer left or never joined: send only to peers from Endpoint::peers or a Connected event.",
            )
            .at_path(format!("/peers/{}", peer.0)));
        };
        if !conn.connected {
            return Err(Diagnostic::error(
                "net_not_connected",
                format!("the connection to {peer} is not open yet"),
                "Wait for the Connected event before sending.",
            )
            .at_path(format!("/peers/{}", peer.0)));
        }
        if data.len() > max_message {
            return Err(Diagnostic::error(
                "net_message_too_large",
                format!(
                    "the message has {} bytes, more than max_message_size ({max_message})",
                    data.len()
                ),
                "Split the message, send less (a delta instead of the whole state), or raise max_message_size on both sides (at most 1 MiB).",
            )
            .at_path(format!("/peers/{}", peer.0)));
        }
        if conn.queued_bytes + data.len() > max_queued {
            return Err(Diagnostic::error(
                "net_send_queue_full",
                format!(
                    "{} bytes already wait for {peer}; this message would pass max_queued_bytes ({max_queued})",
                    conn.queued_bytes
                ),
                "The peer is slow or the network is bad: send less often, or send state unreliably so that new state replaces old.",
            )
            .at_path(format!("/peers/{}", peer.0)));
        }
        match channel {
            Channel::Reliable => {
                let count = data.len().div_ceil(FRAGMENT_LEN).max(1);
                let (Ok(count16), Some(_)) = (
                    u16::try_from(count),
                    conn.next_seq
                        .checked_add(u32::try_from(count).unwrap_or(u32::MAX)),
                ) else {
                    return Err(Diagnostic::error(
                        "net_sequence_exhausted",
                        format!("the reliable channel to {peer} has used all its sequence numbers"),
                        "A connection carries about four billion reliable fragments; close it and join again.",
                    )
                    .at_path(format!("/peers/{}", peer.0)));
                };
                for index in 0..count16 {
                    let start = usize::from(index) * FRAGMENT_LEN;
                    let end = (start + FRAGMENT_LEN).min(data.len());
                    conn.queue.push_back(OutFragment {
                        seq: conn.next_seq,
                        index,
                        count: count16,
                        bytes: data.get(start..end).unwrap_or_default().to_vec(),
                    });
                    conn.next_seq += 1;
                }
            }
            Channel::Unreliable => conn.unreliable_out.push_back(data.to_vec()),
        }
        conn.queued_bytes += data.len();
        Ok(())
    }

    /// Closes the connection to `peer` (on a client, to the host): tells
    /// the other side and emits [`Event::Disconnected`] with
    /// [`Reason::Local`].
    pub fn disconnect(&mut self, peer: PeerId) -> Result<()> {
        if !self.peers.contains_key(&peer) {
            return Err(Diagnostic::error(
                "net_unknown_peer",
                format!("there is no connection to {peer}"),
                "The peer left already; nothing needs doing.",
            )
            .at_path(format!("/peers/{}", peer.0)));
        }
        // Twice, in two packets: one may be lost; the other side times out
        // anyway if both are.
        for _ in 0..2 {
            self.send_frames(
                peer,
                &[Frame::Disconnect {
                    reason: wire::disconnect::CLOSED,
                }],
            );
        }
        self.end(peer, Reason::Local);
        Ok(())
    }

    /// Closes every connection (see [`disconnect`](Self::disconnect)).
    pub fn close(&mut self) {
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for peer in peers {
            let _ = self.disconnect(peer);
        }
        if let Role::Client(client) = &mut self.role {
            client.stage = Stage::Closed;
        }
    }

    /// Reads what arrived, sends what is due and ends silent connections.
    /// `now_ms` is a monotonic time in milliseconds chosen by the caller (a
    /// smaller value than the last one counts as the last one).
    pub fn update(&mut self, now_ms: u64) {
        self.now_ms = self.now_ms.max(now_ms);
        self.transport.tick(self.now_ms);
        self.receive();
        self.drive_client();
        self.expire();
        self.flush();
    }

    fn send_raw(&mut self, to: SocketAddr, bytes: &[u8]) {
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += bytes.len() as u64;
        if self.transport.send(to, bytes).is_err() {
            self.stats.send_errors += 1;
        }
    }

    fn seal_and_send(&mut self, peer: PeerId, texts: Vec<Vec<u8>>) {
        for mut text in texts {
            let Some(conn) = self.peers.get_mut(&peer) else {
                return;
            };
            let number = conn.next_number;
            conn.next_number += 1;
            let header = wire::data_header(conn.wire_id, number);
            if conn.keys.seal(number, &header, &mut text).is_err() {
                self.stats.send_errors += 1;
                continue;
            }
            let addr = conn.addr;
            let mut packet = Vec::with_capacity(header.len() + text.len());
            packet.extend_from_slice(&header);
            packet.extend_from_slice(&text);
            self.send_raw(addr, &packet);
        }
    }

    fn send_frames(&mut self, peer: PeerId, frames: &[Frame<'_>]) {
        let mut packer = Packer::default();
        for frame in frames {
            packer.push(frame);
        }
        self.seal_and_send(peer, packer.finish());
    }

    fn end(&mut self, peer: PeerId, reason: Reason) {
        if let Some(conn) = self.peers.remove(&peer) {
            if self.by_addr.get(&conn.addr) == Some(&peer) {
                self.by_addr.remove(&conn.addr);
            }
            self.events.push_back(Event::Disconnected { peer, reason });
        }
        if let Role::Client(client) = &mut self.role {
            client.stage = Stage::Closed;
        }
    }

    fn violation(&mut self, peer: PeerId) {
        self.send_frames(
            peer,
            &[Frame::Disconnect {
                reason: wire::disconnect::PROTOCOL_VIOLATION,
            }],
        );
        self.end(peer, Reason::ProtocolViolation);
    }

    fn receive(&mut self) {
        let mut buf = std::mem::take(&mut self.buf);
        for _ in 0..MAX_READS_PER_UPDATE {
            match self.transport.recv(&mut buf) {
                Recv::Empty => break,
                Recv::Dropped => self.stats.dropped_oversized += 1,
                Recv::Datagram { len, from } => {
                    self.stats.packets_received += 1;
                    self.stats.bytes_received += len as u64;
                    if len > MAX_PACKET || len > buf.len() {
                        self.stats.dropped_oversized += 1;
                        continue;
                    }
                    self.handle(from, &buf[..len]);
                }
            }
        }
        self.buf = buf;
    }

    fn handle(&mut self, from: SocketAddr, bytes: &[u8]) {
        let Some(packet) = wire::decode(bytes) else {
            self.stats.dropped_malformed += 1;
            return;
        };
        let host = self.is_host();
        match packet {
            Packet::Hello { client_key, token } if host => self.host_hello(from, client_key, token),
            Packet::Connect {
                client_key,
                token,
                cookie,
                header,
                sealed,
            } if host => self.host_connect(from, client_key, token, cookie, header, sealed),
            Packet::OtherVersion {
                token: Some(token), ..
            } if host => self.host_other_version(from, token),
            Packet::Challenge {
                token,
                cookie,
                host_key,
            } if !host => self.client_challenge(from, token, cookie, host_key),
            Packet::Reject { token, reason } if !host => self.client_reject(from, token, reason),
            Packet::Data {
                connection,
                number,
                header,
                sealed,
            } => self.data(from, connection, number, header, sealed),
            _ => self.stats.dropped_unknown += 1,
        }
    }

    fn host_side(&mut self) -> Option<&mut HostSide> {
        match &mut self.role {
            Role::Host(host) => Some(host),
            Role::Client(_) => None,
        }
    }

    fn host_hello(&mut self, from: SocketAddr, client_key: [u8; 32], token: [u8; TOKEN_LEN]) {
        let now = self.now_ms;
        let Some(host) = self.host_side() else {
            return;
        };
        if !host.handshakes.take(now) {
            self.stats.dropped_rate += 1;
            return;
        }
        let cookie = crypto::cookie(
            &host.cookie_secret,
            now / COOKIE_BUCKET_MS,
            from,
            &client_key,
            &token,
        );
        let packet = wire::challenge(&token, &cookie, &host.keys.public());
        self.send_raw(from, &packet);
    }

    fn host_other_version(&mut self, from: SocketAddr, token: [u8; TOKEN_LEN]) {
        let now = self.now_ms;
        let Some(host) = self.host_side() else {
            return;
        };
        if !host.handshakes.take(now) {
            self.stats.dropped_rate += 1;
            return;
        }
        self.stats.handshakes_refused += 1;
        self.send_raw(from, &wire::reject(&token, RejectReason::VersionMismatch));
    }

    #[allow(clippy::too_many_arguments)]
    fn host_connect(
        &mut self,
        from: SocketAddr,
        client_key: [u8; PUBLIC_KEY_LEN],
        token: [u8; TOKEN_LEN],
        cookie: [u8; COOKIE_LEN],
        header: &[u8],
        sealed: &[u8],
    ) {
        let now = self.now_ms;
        let max_peers = self.config.max_peers as usize;
        let packets_per_second = self.config.max_packets_per_second;
        let Role::Host(host) = &mut self.role else {
            return;
        };
        if !host.handshakes.take(now) {
            self.stats.dropped_rate += 1;
            return;
        }
        let bucket = now / COOKIE_BUCKET_MS;
        let valid = |b: u64| {
            same_bytes(
                &cookie,
                &crypto::cookie(&host.cookie_secret, b, from, &client_key, &token),
            )
        };
        if !(valid(bucket) || (bucket > 0 && valid(bucket - 1))) {
            self.stats.handshakes_refused += 1;
            return;
        }
        // The same client again: its welcome was lost.
        if let Some(conn) = self
            .by_addr
            .get(&from)
            .and_then(|id| self.peers.get_mut(id))
            && conn.client_key == client_key
        {
            conn.welcome_due = true;
            return;
        }
        if !host.agreements.take(now) {
            self.stats.dropped_rate += 1;
            return;
        }
        let Ok(shared) = host.keys.agree(&client_key) else {
            self.stats.handshakes_refused += 1;
            return;
        };
        let host_key = host.keys.public();
        let keys = SessionKeys::host(&shared, &client_key, &host_key);
        let mut body = sealed.to_vec();
        let Some(len) = keys.open(CONNECT_NUMBER, header, &mut body) else {
            self.stats.dropped_unauthentic += 1;
            return;
        };
        if !same_bytes(&body[..len], self.config.room_code.as_str().as_bytes()) {
            self.stats.handshakes_refused += 1;
            self.send_raw(from, &wire::reject(&token, RejectReason::RoomCodeWrong));
            return;
        }
        let replaced = self.by_addr.get(&from).copied();
        let others = self.peers.len() - usize::from(replaced.is_some());
        let Role::Host(host) = &mut self.role else {
            return;
        };
        if others >= max_peers || host.next_peer == u32::MAX {
            self.stats.handshakes_refused += 1;
            self.send_raw(from, &wire::reject(&token, RejectReason::RoomFull));
            return;
        }
        let id = PeerId(host.next_peer);
        host.next_peer += 1;
        if let Some(old) = replaced {
            self.end(old, Reason::Replaced);
        }
        self.peers.insert(
            id,
            Connection::new(NewConnection {
                id,
                wire_id: id.0,
                addr: from,
                client_key,
                keys,
                connected: true,
                now_ms: now,
                packets_per_second,
            }),
        );
        self.by_addr.insert(from, id);
        self.events.push_back(Event::Connected { peer: id });
    }

    fn client_side(&mut self) -> Option<&mut ClientSide> {
        match &mut self.role {
            Role::Client(client) => Some(client),
            Role::Host(_) => None,
        }
    }

    fn client_challenge(
        &mut self,
        from: SocketAddr,
        token: [u8; TOKEN_LEN],
        cookie: [u8; COOKIE_LEN],
        host_key: [u8; PUBLIC_KEY_LEN],
    ) {
        let now = self.now_ms;
        let pinned = self.config.host_key;
        let room = self.config.room_code.as_str().as_bytes().to_vec();
        let packets_per_second = self.config.max_packets_per_second;
        let Some(client) = self.client_side() else {
            return;
        };
        if from != client.host_addr
            || !same_bytes(&token, &client.token)
            || !matches!(client.stage, Stage::Hello)
        {
            self.stats.dropped_unknown += 1;
            return;
        }
        if let Some(pin) = pinned
            && !same_bytes(&pin, &host_key)
        {
            self.end_client(Reason::HostKeyMismatch);
            return;
        }
        let Ok(shared) = client.keys.agree(&host_key) else {
            self.end_client(Reason::KeyRejected);
            return;
        };
        let my_key = client.keys.public();
        let keys = SessionKeys::client(&shared, &my_key, &host_key);
        let mut packet = wire::connect_header(&my_key, &client.token, &cookie);
        let mut body = room;
        if keys.seal(CONNECT_NUMBER, &packet, &mut body).is_err() {
            self.end_client(Reason::KeyRejected);
            return;
        }
        packet.extend_from_slice(&body);
        let host_addr = client.host_addr;
        client.stage = Stage::Connect {
            packet: packet.clone(),
            challenged_ms: now,
        };
        client.last_sent_ms = Some(now);
        self.peers.insert(
            PeerId::HOST,
            Connection::new(NewConnection {
                id: PeerId::HOST,
                wire_id: 0,
                addr: host_addr,
                client_key: my_key,
                keys,
                connected: false,
                now_ms: now,
                packets_per_second,
            }),
        );
        self.by_addr.insert(host_addr, PeerId::HOST);
        self.send_raw(host_addr, &packet);
    }

    fn client_reject(&mut self, from: SocketAddr, token: [u8; TOKEN_LEN], reason: RejectReason) {
        let Some(client) = self.client_side() else {
            return;
        };
        let waiting = matches!(client.stage, Stage::Hello | Stage::Connect { .. });
        if from != client.host_addr || !same_bytes(&token, &client.token) || !waiting {
            self.stats.dropped_unknown += 1;
            return;
        }
        self.end_client(match reason {
            RejectReason::RoomCodeWrong => Reason::RoomCodeWrong,
            RejectReason::RoomFull => Reason::RoomFull,
            RejectReason::VersionMismatch => Reason::VersionMismatch,
        });
    }

    fn end_client(&mut self, reason: Reason) {
        if self.peers.contains_key(&PeerId::HOST) {
            self.end(PeerId::HOST, reason);
        } else {
            self.events.push_back(Event::Disconnected {
                peer: PeerId::HOST,
                reason,
            });
            if let Some(client) = self.client_side() {
                client.stage = Stage::Closed;
            }
        }
    }

    fn data(
        &mut self,
        from: SocketAddr,
        connection: u32,
        number: u64,
        header: &[u8],
        sealed: &[u8],
    ) {
        let now = self.now_ms;
        let host = self.is_host();
        let Some(&id) = self.by_addr.get(&from) else {
            self.stats.dropped_unknown += 1;
            return;
        };
        let Some(conn) = self.peers.get_mut(&id) else {
            self.stats.dropped_unknown += 1;
            return;
        };
        if (host || conn.connected) && connection != conn.wire_id {
            self.stats.dropped_unknown += 1;
            return;
        }
        if !conn.replay.fresh(number) {
            self.stats.dropped_replayed += 1;
            return;
        }
        let mut body = sealed.to_vec();
        let Some(len) = conn.keys.open(number, header, &mut body) else {
            self.stats.dropped_unauthentic += 1;
            return;
        };
        if !conn.rate.take(now) {
            self.stats.dropped_rate += 1;
            return;
        }
        conn.replay.mark(number);
        conn.last_received_ms = now;
        if !conn.connected {
            // The first packet the host sealed for this client: accepted.
            conn.connected = true;
            conn.wire_id = connection;
            if let Role::Client(client) = &mut self.role {
                client.stage = Stage::Connected;
                client.local_id = Some(PeerId(connection));
            }
            self.events.push_back(Event::Connected { peer: id });
        }
        let mut frames = Vec::new();
        if !wire::decode_frames(&body[..len], &mut frames) {
            self.violation(id);
            return;
        }
        let mut delivered = Vec::new();
        let mut outcome = Outcome::Go;
        if let Some(conn) = self.peers.get_mut(&id) {
            for frame in frames {
                outcome = match frame {
                    Frame::Welcome { peer } => {
                        if host || peer != conn.wire_id {
                            Outcome::Violation
                        } else {
                            Outcome::Go
                        }
                    }
                    Frame::Ack { next, mask } => {
                        conn.on_ack(next, mask);
                        Outcome::Go
                    }
                    Frame::Reliable {
                        seq,
                        index,
                        count,
                        bytes,
                    } => conn.on_reliable(
                        (seq, index, count, bytes),
                        &self.config,
                        &mut self.stats,
                        &mut delivered,
                    ),
                    Frame::Unreliable {
                        id: message,
                        index,
                        count,
                        bytes,
                    } => conn.on_unreliable(
                        (message, index, count, bytes),
                        &self.config,
                        now,
                        &mut delivered,
                    ),
                    Frame::Ping { time } => {
                        conn.pong_due = Some(time);
                        Outcome::Go
                    }
                    Frame::Pong { time } => {
                        conn.on_pong(time, now);
                        Outcome::Go
                    }
                    Frame::Disconnect { .. } => Outcome::Closed,
                };
                if !matches!(outcome, Outcome::Go) {
                    break;
                }
            }
        }
        for (channel, data) in delivered {
            self.events.push_back(Event::Message {
                peer: id,
                channel,
                data,
            });
        }
        match outcome {
            Outcome::Go => {}
            Outcome::Violation => self.violation(id),
            Outcome::Closed => self.end(id, Reason::Closed),
        }
    }

    fn drive_client(&mut self) {
        let now = self.now_ms;
        let connect_timeout = self.config.connect_timeout_ms;
        let resend = self.config.handshake_resend_ms;
        let Some(client) = self.client_side() else {
            return;
        };
        if matches!(client.stage, Stage::Connected | Stage::Closed) {
            return;
        }
        let started = *client.started_ms.get_or_insert(now);
        if now.saturating_sub(started) >= connect_timeout {
            self.end_client(Reason::ConnectTimedOut);
            return;
        }
        // A cookie lives one to two buckets: start again before it expires.
        if let Stage::Connect { challenged_ms, .. } = client.stage
            && now.saturating_sub(challenged_ms) >= COOKIE_BUCKET_MS
        {
            client.stage = Stage::Hello;
            client.last_sent_ms = None;
            if let Some(conn) = self.peers.remove(&PeerId::HOST) {
                self.by_addr.remove(&conn.addr);
            }
        }
        let Some(client) = self.client_side() else {
            return;
        };
        if client
            .last_sent_ms
            .is_some_and(|last| now.saturating_sub(last) < resend)
        {
            return;
        }
        client.last_sent_ms = Some(now);
        let to = client.host_addr;
        let packet = match &client.stage {
            Stage::Hello => wire::hello(&client.keys.public(), &client.token),
            Stage::Connect { packet, .. } => packet.clone(),
            Stage::Connected | Stage::Closed => return,
        };
        self.send_raw(to, &packet);
    }

    fn expire(&mut self) {
        let now = self.now_ms;
        let timeout = self.config.timeout_ms;
        let mut silent = Vec::new();
        for conn in self.peers.values_mut() {
            conn.partial
                .retain(|_, p| now.saturating_sub(p.first_ms) < PARTIAL_LIFETIME_MS);
            if conn.connected && now.saturating_sub(conn.last_received_ms) >= timeout {
                silent.push(conn.id);
            }
        }
        for peer in silent {
            self.end(peer, Reason::TimedOut);
        }
    }

    fn flush(&mut self) {
        let now = self.now_ms;
        let keepalive = self.config.keepalive_ms;
        let ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for id in ids {
            let Some(conn) = self.peers.get_mut(&id) else {
                continue;
            };
            if !conn.connected {
                continue;
            }
            let texts = conn.pack(now, keepalive, &mut self.stats);
            self.seal_and_send(id, texts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_window_takes_each_number_once() {
        let mut w = Window::default();
        assert!(w.fresh(5));
        w.mark(5);
        assert!(!w.fresh(5));
        assert!(w.fresh(4));
        w.mark(200);
        assert!(!w.fresh(5), "too old");
        assert!(w.fresh(199));
        w.mark(199);
        assert!(!w.fresh(199));
        assert!(w.fresh(73));
        assert!(!w.fresh(72));
    }

    #[test]
    fn a_bucket_refills_with_time_and_caps_bursts() {
        let mut b = Bucket::new(10);
        let taken = (0..100).filter(|_| b.take(0)).count();
        assert_eq!(taken, 10);
        assert!(!b.take(50));
        assert!(b.take(100));
        let taken = (0..100).filter(|_| b.take(100_000)).count();
        assert_eq!(taken, 10);
    }

    #[test]
    fn configs_are_checked() {
        let code = RoomCode::parse("ABCD").unwrap();
        assert!(Config::new(code.clone()).validate().is_ok());
        let mut bad = Config::new(code.clone());
        bad.max_peers = 0;
        assert_eq!(bad.validate().unwrap_err().code, "net_config_invalid");
        let mut bad = Config::new(code.clone());
        bad.max_message_size = MAX_MESSAGE_LIMIT + 1;
        assert!(bad.validate().is_err());
        let mut bad = Config::new(code);
        bad.timeout_ms = 100;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn every_reason_has_its_code() {
        for reason in [
            Reason::Closed,
            Reason::Local,
            Reason::TimedOut,
            Reason::ConnectTimedOut,
            Reason::RoomCodeWrong,
            Reason::RoomFull,
            Reason::VersionMismatch,
            Reason::HostKeyMismatch,
            Reason::KeyRejected,
            Reason::ProtocolViolation,
            Reason::Replaced,
        ] {
            let diagnostic = reason.diagnostic(PeerId(3));
            assert_eq!(diagnostic.code, reason.code());
            assert!(!diagnostic.hint.is_empty());
        }
    }
}
