//! Going through a relay: two [`Transport`]s that wrap another transport, so
//! an [`Endpoint`](crate::Endpoint) runs over a relay unchanged, with the
//! same handshake, keys and limits (design note `docs/design/1.1-relay.md`
//! §3.4).

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use wish_core::{Diagnostic, Result};

use crate::crypto::Entropy;
use crate::relay::envelope::{
    self, Envelope, HOST_TOKEN_LEN, Locator, MAX_RELAY_DATAGRAM, RELAY_COOKIE_LEN, Refusal,
};
use crate::room::RoomCode;
use crate::transport::{Recv, Transport};

/// A registered host registers again this often, in milliseconds.
pub const REGISTER_EVERY_MS: u64 = 2000;
/// A host not yet registered tries again this often, in milliseconds.
pub const REGISTER_RETRY_MS: u64 = 500;
/// A client not yet attached tries again this often, in milliseconds.
pub const ATTACH_RETRY_MS: u64 = 250;
/// After a refusal, a host or client tries again this often, in
/// milliseconds (the room may expire, or its host may arrive).
pub const REFUSED_RETRY_MS: u64 = 1000;
/// Datagrams a client holds until it is attached; older ones are dropped
/// (the endpoint sends its handshake again anyway).
pub const HOLD_MAX: usize = 8;
/// Relay datagrams of the relay's own (not inner datagrams) one `recv`
/// handles before it returns, so a flood of them cannot hold it.
const CONTROL_PER_RECV: usize = 64;

/// The first 64 bits of the virtual addresses of slots: a unique local IPv6
/// prefix (`fd57:4e52:656c:6179::/64`, "WNrelay") that is never routed.
const VIRTUAL_PREFIX: [u16; 4] = [0xfd57, 0x4e52, 0x656c, 0x6179];
/// The port of every virtual address.
const VIRTUAL_PORT: u16 = 1;

/// The address under which a host's endpoint sees the client in `slot` of
/// its room at the relay.
pub fn virtual_addr(slot: u32) -> SocketAddr {
    let [a, b, c, d] = VIRTUAL_PREFIX;
    let ip = Ipv6Addr::new(a, b, c, d, 0, 0, (slot >> 16) as u16, slot as u16);
    SocketAddr::new(IpAddr::V6(ip), VIRTUAL_PORT)
}

/// The slot of a virtual address, or `None` for any other address.
pub fn virtual_slot(addr: SocketAddr) -> Option<u32> {
    let IpAddr::V6(ip) = addr.ip() else {
        return None;
    };
    let s = ip.segments();
    if s[..4] != VIRTUAL_PREFIX || s[4] != 0 || s[5] != 0 || addr.port() != VIRTUAL_PORT {
        return None;
    }
    Some((u32::from(s[6]) << 16) | u32::from(s[7]))
}

/// Where a host's registration, or a client's attach, stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayState {
    /// Asked, no answer yet (or the relay forgot this side, and it asks
    /// again).
    Waiting,
    /// Registered (a host) or attached (a client): traffic goes through.
    Ready,
    /// The relay refused; the wrapper keeps asking every
    /// [`REFUSED_RETRY_MS`].
    Refused(Refusal),
}

impl RelayState {
    /// The refusal as a diagnostic, if the relay refused.
    pub fn diagnostic(self) -> Option<Diagnostic> {
        match self {
            Self::Refused(reason) => Some(refusal_diagnostic(reason)),
            Self::Waiting | Self::Ready => None,
        }
    }
}

/// What a relay's refusal means, and what to do about it.
pub fn refusal_diagnostic(reason: Refusal) -> Diagnostic {
    let diagnostic = match reason {
        Refusal::RoomTaken => Diagnostic::error(
            "net_relay_room_taken",
            "the relay already has a room with this code, held by another host",
            "If this host restarted, its old room expires within 15 seconds and the registration goes on trying; otherwise host again with a new room code.",
        ),
        Refusal::RelayFull => Diagnostic::error(
            "net_relay_full",
            "the relay holds as many rooms or players as it was set to take",
            "Try again later, or ask whoever runs the relay to raise --max-rooms or --max-clients.",
        ),
        Refusal::RoomFull => Diagnostic::error(
            "net_relay_room_full",
            "this room at the relay holds as many players as the relay lets one room have",
            "Ask whoever runs the relay to raise --max-peers, or wait for a player to leave.",
        ),
        Refusal::NoRoom => Diagnostic::error(
            "net_relay_no_room",
            "the relay has no room with this code",
            "Check the room code and that the host plays through this relay; the join goes on trying, so it starts once the host is there.",
        ),
    };
    diagnostic.at_path(format!("/relay/{}", reason.as_str()))
}

/// A transport for a host behind a router: it registers the room at a relay
/// and receives the room's clients through it. Each client appears to the
/// endpoint at its own [`virtual_addr`]. Datagrams that do not come from the
/// relay pass through untouched, so the same endpoint also takes players on
/// the local network directly.
pub struct RelayHost<T: Transport> {
    inner: T,
    relay: SocketAddr,
    locator: Locator,
    token: [u8; HOST_TOKEN_LEN],
    cookie: [u8; RELAY_COOKIE_LEN],
    state: RelayState,
    now_ms: u64,
    last_sent_ms: Option<u64>,
    buf: Vec<u8>,
}

impl<T: Transport> std::fmt::Debug for RelayHost<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayHost")
            .field("relay", &self.relay)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> RelayHost<T> {
    /// Hosts the room of `code` through the relay at `relay`. The room's
    /// token (what keeps others from taking it) comes from `entropy`.
    pub fn new(inner: T, relay: SocketAddr, code: &RoomCode, entropy: &mut Entropy) -> Self {
        Self {
            inner,
            relay,
            locator: envelope::locator(code),
            token: entropy.bytes(),
            cookie: [0; RELAY_COOKIE_LEN],
            state: RelayState::Waiting,
            now_ms: 0,
            last_sent_ms: None,
            buf: vec![0; MAX_RELAY_DATAGRAM + 1],
        }
    }

    /// Where the registration stands.
    pub fn relay_state(&self) -> RelayState {
        self.state
    }

    /// The relay's address.
    pub fn relay_addr(&self) -> SocketAddr {
        self.relay
    }

    /// The transport underneath.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Closes the room at the relay now (otherwise it expires when this
    /// host goes quiet).
    pub fn close(&mut self) {
        let bytes = envelope::unregister(&self.locator, &self.token);
        let _ = self.inner.send(self.relay, &bytes);
        self.state = RelayState::Waiting;
    }

    fn send_register(&mut self) {
        self.last_sent_ms = Some(self.now_ms);
        let bytes = envelope::register(&self.locator, &self.token, &self.cookie);
        let _ = self.inner.send(self.relay, &bytes);
    }

    /// Handles a datagram of the relay's own. `Some` for an inner datagram
    /// (its slot, and where it lies in the buffer).
    fn handle_relay(&mut self, len: usize) -> Option<(u32, usize)> {
        match envelope::decode(self.buf.get(..len)?)? {
            Envelope::ToHost { slot, inner } => Some((slot, inner.len())),
            Envelope::RegisterChallenge { locator, cookie } if locator == self.locator => {
                self.cookie = cookie;
                self.send_register();
                None
            }
            Envelope::Registered { locator } if locator == self.locator => {
                self.state = RelayState::Ready;
                None
            }
            Envelope::Refused { locator, reason } if locator == self.locator => {
                self.state = RelayState::Refused(reason);
                None
            }
            Envelope::NotAttached => {
                // The relay forgot the room (it restarted): ask again now.
                self.state = RelayState::Waiting;
                self.cookie = [0; RELAY_COOKIE_LEN];
                self.send_register();
                None
            }
            _ => None,
        }
    }
}

impl<T: Transport> Transport for RelayHost<T> {
    fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr()
    }

    fn tick(&mut self, now_ms: u64) {
        self.now_ms = self.now_ms.max(now_ms);
        self.inner.tick(now_ms);
        let every = match self.state {
            RelayState::Ready => REGISTER_EVERY_MS,
            RelayState::Waiting => REGISTER_RETRY_MS,
            RelayState::Refused(_) => REFUSED_RETRY_MS,
        };
        if self
            .last_sent_ms
            .is_none_or(|last| self.now_ms.saturating_sub(last) >= every)
        {
            self.send_register();
        }
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()> {
        match virtual_slot(to) {
            Some(slot) => {
                if !envelope::is_inner(bytes) {
                    // Only the transport's own datagrams go through a relay.
                    return Ok(());
                }
                self.inner
                    .send(self.relay, &envelope::host_send(slot, bytes))
            }
            None => self.inner.send(to, bytes),
        }
    }

    fn recv(&mut self, buf: &mut [u8]) -> Recv {
        for _ in 0..CONTROL_PER_RECV {
            let (len, from) = match self.inner.recv(&mut self.buf) {
                Recv::Empty => return Recv::Empty,
                Recv::Dropped => return Recv::Dropped,
                Recv::Datagram { len, from } => (len, from),
            };
            if len > self.buf.len() {
                return Recv::Dropped;
            }
            if from == self.relay {
                let Some((slot, inner_len)) = self.handle_relay(len) else {
                    continue;
                };
                let start = envelope::SLOT_ENVELOPE_LEN;
                return copy_out(&self.buf[start..start + inner_len], virtual_addr(slot), buf);
            }
            if virtual_slot(from).is_some() {
                // Only the relay speaks for slots.
                continue;
            }
            return copy_out(&self.buf[..len], from, buf);
        }
        Recv::Dropped
    }
}

/// A transport for a client that joins through a relay: the endpoint
/// connects to the relay's address (`Endpoint::connect(RelayClient::new(..),
/// relay, ..)`), and this wrapper attaches to the room and carries the
/// endpoint's datagrams in envelopes.
pub struct RelayClient<T: Transport> {
    inner: T,
    relay: SocketAddr,
    locator: Locator,
    cookie: [u8; RELAY_COOKIE_LEN],
    state: RelayState,
    now_ms: u64,
    last_sent_ms: Option<u64>,
    held: VecDeque<Vec<u8>>,
    buf: Vec<u8>,
}

impl<T: Transport> std::fmt::Debug for RelayClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayClient")
            .field("relay", &self.relay)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> RelayClient<T> {
    /// Joins the room of `code` through the relay at `relay`.
    pub fn new(inner: T, relay: SocketAddr, code: &RoomCode) -> Self {
        Self {
            inner,
            relay,
            locator: envelope::locator(code),
            cookie: [0; RELAY_COOKIE_LEN],
            state: RelayState::Waiting,
            now_ms: 0,
            last_sent_ms: None,
            held: VecDeque::new(),
            buf: vec![0; MAX_RELAY_DATAGRAM + 1],
        }
    }

    /// Where the attach stands.
    pub fn relay_state(&self) -> RelayState {
        self.state
    }

    /// The relay's address (the endpoint's host address).
    pub fn relay_addr(&self) -> SocketAddr {
        self.relay
    }

    /// The transport underneath.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    fn send_attach(&mut self) {
        self.last_sent_ms = Some(self.now_ms);
        let bytes = envelope::attach(&self.locator, &self.cookie);
        let _ = self.inner.send(self.relay, &bytes);
    }

    fn handle_relay(&mut self, len: usize) -> Option<usize> {
        match envelope::decode(self.buf.get(..len)?)? {
            Envelope::ToClient { inner } => Some(inner.len()),
            Envelope::AttachChallenge { locator, cookie } if locator == self.locator => {
                self.cookie = cookie;
                self.send_attach();
                None
            }
            Envelope::Attached { locator } if locator == self.locator => {
                self.state = RelayState::Ready;
                while let Some(bytes) = self.held.pop_front() {
                    let _ = self.inner.send(self.relay, &envelope::client_send(&bytes));
                }
                None
            }
            Envelope::Refused { locator, reason } if locator == self.locator => {
                if self.state != RelayState::Ready {
                    self.state = RelayState::Refused(reason);
                }
                None
            }
            Envelope::NotAttached => {
                self.state = RelayState::Waiting;
                self.cookie = [0; RELAY_COOKIE_LEN];
                self.send_attach();
                None
            }
            _ => None,
        }
    }
}

impl<T: Transport> Transport for RelayClient<T> {
    fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr()
    }

    fn tick(&mut self, now_ms: u64) {
        self.now_ms = self.now_ms.max(now_ms);
        self.inner.tick(now_ms);
        let every = match self.state {
            RelayState::Ready => return,
            RelayState::Waiting => ATTACH_RETRY_MS,
            RelayState::Refused(_) => REFUSED_RETRY_MS,
        };
        if self
            .last_sent_ms
            .is_none_or(|last| self.now_ms.saturating_sub(last) >= every)
        {
            self.send_attach();
        }
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()> {
        if to != self.relay {
            return self.inner.send(to, bytes);
        }
        if !envelope::is_inner(bytes) {
            return Ok(());
        }
        if self.state == RelayState::Ready {
            return self.inner.send(self.relay, &envelope::client_send(bytes));
        }
        if self.held.len() >= HOLD_MAX {
            self.held.pop_front();
        }
        self.held.push_back(bytes.to_vec());
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> Recv {
        for _ in 0..CONTROL_PER_RECV {
            let (len, from) = match self.inner.recv(&mut self.buf) {
                Recv::Empty => return Recv::Empty,
                Recv::Dropped => return Recv::Dropped,
                Recv::Datagram { len, from } => (len, from),
            };
            if len > self.buf.len() {
                return Recv::Dropped;
            }
            if from != self.relay {
                return copy_out(&self.buf[..len], from, buf);
            }
            if let Some(inner_len) = self.handle_relay(len) {
                let start = envelope::RELAY_HEADER_LEN;
                return copy_out(&self.buf[start..start + inner_len], from, buf);
            }
        }
        Recv::Dropped
    }
}

/// Copies a datagram to the caller's buffer, as a transport would (a
/// datagram larger than the buffer is cut and reported at its full size).
fn copy_out(bytes: &[u8], from: SocketAddr, buf: &mut [u8]) -> Recv {
    let n = bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    Recv::Datagram {
        len: bytes.len(),
        from,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_addresses_round_trip_and_nothing_else_is_one() {
        for slot in [1, 2, 0xFFFF, 0x1_0000, u32::MAX] {
            assert_eq!(virtual_slot(virtual_addr(slot)), Some(slot));
        }
        assert_eq!(virtual_slot("10.0.0.1:1".parse().unwrap()), None);
        assert_eq!(
            virtual_slot("[fd57:4e52:656c:6179::1]:2".parse().unwrap()),
            None
        );
        assert_eq!(
            virtual_slot("[fd57:4e52:656c:6170::1]:1".parse().unwrap()),
            None
        );
        assert_eq!(
            virtual_slot("[fd57:4e52:656c:6179:1::1]:1".parse().unwrap()),
            None
        );
    }

    #[test]
    fn every_refusal_has_a_diagnostic_with_a_hint() {
        for reason in [
            Refusal::RoomTaken,
            Refusal::RelayFull,
            Refusal::RoomFull,
            Refusal::NoRoom,
        ] {
            let d = RelayState::Refused(reason).diagnostic().unwrap();
            assert!(d.code.starts_with("net_relay_"), "{}", d.code);
            assert!(!d.hint.is_empty());
        }
        assert!(RelayState::Ready.diagnostic().is_none());
    }
}
