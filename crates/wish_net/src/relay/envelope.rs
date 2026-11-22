//! The relay's envelope: a small cleartext header around the transport's own
//! datagrams, as pure encode and decode functions. Decoding never panics:
//! every input is untrusted. Design note `docs/design/1.1-relay.md` §3.2.

use std::net::{IpAddr, SocketAddr};

use sha2::{Digest, Sha256};

use crate::room::RoomCode;
use crate::wire;

/// The first two bytes of every relay datagram.
pub const RELAY_MAGIC: [u8; 2] = *b"WR";
/// The relay protocol version this crate speaks.
pub const RELAY_VERSION: u8 = 1;
/// The plain header of every relay datagram: magic, version, kind.
pub const RELAY_HEADER_LEN: usize = 4;
/// The largest relay datagram, in bytes: the largest UDP payload that fits
/// the IPv6 minimum MTU (1280 − 40 − 8). An inner datagram of
/// [`wire::MAX_PACKET`] bytes and its envelope fit under it.
pub const MAX_RELAY_DATAGRAM: usize = 1232;
/// The size of a room locator in bytes.
pub const LOCATOR_LEN: usize = 16;
/// The size of a host token in bytes.
pub const HOST_TOKEN_LEN: usize = 16;
/// The size of a relay cookie in bytes.
pub const RELAY_COOKIE_LEN: usize = 16;
/// A `Register` (and an `Unregister`) is padded to this size, so every
/// answer to it is smaller.
pub const REGISTER_LEN: usize = 96;
/// An `Attach` is padded to this size, so every answer to it is smaller.
pub const ATTACH_LEN: usize = 64;
/// The bytes `HostSend` and `ToHost` add before the inner datagram.
pub const SLOT_ENVELOPE_LEN: usize = RELAY_HEADER_LEN + 4;

/// A room's name at a relay: a hash of its room code (see [`locator`]).
pub type Locator = [u8; LOCATOR_LEN];

/// The kinds of relay datagram.
pub mod kind {
    /// Host → relay: register or refresh a room.
    pub const REGISTER: u8 = 1;
    /// Relay → host: prove you receive at your address.
    pub const REGISTER_CHALLENGE: u8 = 2;
    /// Relay → host: the room is yours.
    pub const REGISTERED: u8 = 3;
    /// Host → relay: close the room.
    pub const UNREGISTER: u8 = 4;
    /// Client → relay: join a room.
    pub const ATTACH: u8 = 5;
    /// Relay → client: prove you receive at your address.
    pub const ATTACH_CHALLENGE: u8 = 6;
    /// Relay → client: you have a slot in the room.
    pub const ATTACHED: u8 = 7;
    /// Relay → host or client: refused, with a reason.
    pub const REFUSED: u8 = 8;
    /// Relay → anyone: your address has no room and no slot here.
    pub const NOT_ATTACHED: u8 = 9;
    /// Host → relay: an inner datagram for a slot.
    pub const HOST_SEND: u8 = 10;
    /// Relay → host: an inner datagram from a slot.
    pub const TO_HOST: u8 = 11;
    /// Client → relay: an inner datagram for the host.
    pub const CLIENT_SEND: u8 = 12;
    /// Relay → client: an inner datagram from the host.
    pub const TO_CLIENT: u8 = 13;
}

/// Why a relay refused a registration or an attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Refusal {
    /// Another host holds a room with this code (another token).
    RoomTaken,
    /// The relay has as many rooms, or clients, as it takes.
    RelayFull,
    /// The room has as many clients as the relay lets one room have.
    RoomFull,
    /// The relay has no room with this code.
    NoRoom,
}

impl Refusal {
    fn to_byte(self) -> u8 {
        match self {
            Self::RoomTaken => 1,
            Self::RelayFull => 2,
            Self::RoomFull => 3,
            Self::NoRoom => 4,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::RoomTaken),
            2 => Some(Self::RelayFull),
            3 => Some(Self::RoomFull),
            4 => Some(Self::NoRoom),
            _ => None,
        }
    }

    /// The reason in words, for logs and messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoomTaken => "room_taken",
            Self::RelayFull => "relay_full",
            Self::RoomFull => "room_full",
            Self::NoRoom => "no_room",
        }
    }
}

/// A decoded relay datagram. Inner datagrams are borrowed from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Envelope<'a> {
    /// Host → relay, padded to [`REGISTER_LEN`].
    Register {
        /// The room.
        locator: Locator,
        /// The secret the host chose for the room.
        token: [u8; HOST_TOKEN_LEN],
        /// The relay's cookie, echoed (zero the first time).
        cookie: [u8; RELAY_COOKIE_LEN],
    },
    /// Relay → host.
    RegisterChallenge {
        /// The room, echoed.
        locator: Locator,
        /// The cookie to send back.
        cookie: [u8; RELAY_COOKIE_LEN],
    },
    /// Relay → host.
    Registered {
        /// The room, echoed.
        locator: Locator,
    },
    /// Host → relay, padded to [`REGISTER_LEN`].
    Unregister {
        /// The room.
        locator: Locator,
        /// The room's token.
        token: [u8; HOST_TOKEN_LEN],
    },
    /// Client → relay, padded to [`ATTACH_LEN`].
    Attach {
        /// The room.
        locator: Locator,
        /// The relay's cookie, echoed (zero the first time).
        cookie: [u8; RELAY_COOKIE_LEN],
    },
    /// Relay → client.
    AttachChallenge {
        /// The room, echoed.
        locator: Locator,
        /// The cookie to send back.
        cookie: [u8; RELAY_COOKIE_LEN],
    },
    /// Relay → client.
    Attached {
        /// The room, echoed.
        locator: Locator,
    },
    /// Relay → host or client.
    Refused {
        /// The room, echoed.
        locator: Locator,
        /// Why.
        reason: Refusal,
    },
    /// Relay → anyone whose address it does not know.
    NotAttached,
    /// Host → relay.
    HostSend {
        /// The client's slot in the room.
        slot: u32,
        /// The transport's datagram, untouched.
        inner: &'a [u8],
    },
    /// Relay → host.
    ToHost {
        /// The client's slot in the room.
        slot: u32,
        /// The transport's datagram, untouched.
        inner: &'a [u8],
    },
    /// Client → relay.
    ClientSend {
        /// The transport's datagram, untouched.
        inner: &'a [u8],
    },
    /// Relay → client.
    ToClient {
        /// The transport's datagram, untouched.
        inner: &'a [u8],
    },
}

/// The room locator for a room code:
/// `SHA-256("wish-net/1 relay room" | code)[..16]`. A relay receives this,
/// never the code (design note §3.1 says what that does and does not hide).
pub fn locator(code: &RoomCode) -> Locator {
    let mut hasher = Sha256::new();
    hasher.update(b"wish-net/1 relay room");
    hasher.update(code.as_str().as_bytes());
    let full: [u8; 32] = hasher.finalize().into();
    let mut out = [0u8; LOCATOR_LEN];
    out.copy_from_slice(&full[..LOCATOR_LEN]);
    out
}

/// Who a relay cookie is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A host registering a room.
    Host,
    /// A client attaching to a room.
    Client,
}

/// The relay's cookie: proof that the sender receives at `addr`.
/// `bucket` is the time divided by the cookie's lifetime.
pub fn cookie(
    secret: &[u8; 32],
    bucket: u64,
    addr: SocketAddr,
    locator: &Locator,
    role: Role,
) -> [u8; RELAY_COOKIE_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(b"wish-net/1 relay cookie");
    hasher.update(secret);
    hasher.update(bucket.to_le_bytes());
    match addr.ip() {
        IpAddr::V4(ip) => {
            hasher.update([4]);
            hasher.update(ip.octets());
        }
        IpAddr::V6(ip) => {
            hasher.update([6]);
            hasher.update(ip.octets());
        }
    }
    hasher.update(addr.port().to_le_bytes());
    hasher.update(locator);
    hasher.update([match role {
        Role::Host => 1,
        Role::Client => 2,
    }]);
    let full: [u8; 32] = hasher.finalize().into();
    let mut out = [0u8; RELAY_COOKIE_LEN];
    out.copy_from_slice(&full[..RELAY_COOKIE_LEN]);
    out
}

/// Whether `bytes` may be carried as an inner datagram: a datagram of the
/// transport (it starts with `W N`) of at most [`wire::MAX_PACKET`] bytes.
pub fn is_inner(bytes: &[u8]) -> bool {
    bytes.len() >= wire::HEADER_LEN && bytes.len() <= wire::MAX_PACKET && bytes[..2] == wire::MAGIC
}

fn array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// Decodes a relay datagram. `None` for anything that is not a well-formed
/// datagram of this relay protocol and version (dropped by the caller).
pub fn decode(bytes: &[u8]) -> Option<Envelope<'_>> {
    if bytes.len() > MAX_RELAY_DATAGRAM || bytes.len() < RELAY_HEADER_LEN {
        return None;
    }
    if bytes[..2] != RELAY_MAGIC || bytes[2] != RELAY_VERSION {
        return None;
    }
    let body = &bytes[RELAY_HEADER_LEN..];
    let exact = |len: usize| body.len() == len;
    let envelope = match bytes[3] {
        kind::REGISTER if bytes.len() == REGISTER_LEN => Envelope::Register {
            locator: array(body, 0)?,
            token: array(body, LOCATOR_LEN)?,
            cookie: array(body, LOCATOR_LEN + HOST_TOKEN_LEN)?,
        },
        kind::UNREGISTER if bytes.len() == REGISTER_LEN => Envelope::Unregister {
            locator: array(body, 0)?,
            token: array(body, LOCATOR_LEN)?,
        },
        kind::REGISTER_CHALLENGE if exact(LOCATOR_LEN + RELAY_COOKIE_LEN) => {
            Envelope::RegisterChallenge {
                locator: array(body, 0)?,
                cookie: array(body, LOCATOR_LEN)?,
            }
        }
        kind::REGISTERED if exact(LOCATOR_LEN) => Envelope::Registered {
            locator: array(body, 0)?,
        },
        kind::ATTACH if bytes.len() == ATTACH_LEN => Envelope::Attach {
            locator: array(body, 0)?,
            cookie: array(body, LOCATOR_LEN)?,
        },
        kind::ATTACH_CHALLENGE if exact(LOCATOR_LEN + RELAY_COOKIE_LEN) => {
            Envelope::AttachChallenge {
                locator: array(body, 0)?,
                cookie: array(body, LOCATOR_LEN)?,
            }
        }
        kind::ATTACHED if exact(LOCATOR_LEN) => Envelope::Attached {
            locator: array(body, 0)?,
        },
        kind::REFUSED if exact(LOCATOR_LEN + 1) => Envelope::Refused {
            locator: array(body, 0)?,
            reason: Refusal::from_byte(*body.get(LOCATOR_LEN)?)?,
        },
        kind::NOT_ATTACHED if exact(0) => Envelope::NotAttached,
        kind::HOST_SEND | kind::TO_HOST => {
            let slot = u32::from_le_bytes(array(body, 0)?);
            let inner = body.get(4..)?;
            if !is_inner(inner) {
                return None;
            }
            if bytes[3] == kind::HOST_SEND {
                Envelope::HostSend { slot, inner }
            } else {
                Envelope::ToHost { slot, inner }
            }
        }
        kind::CLIENT_SEND | kind::TO_CLIENT => {
            if !is_inner(body) {
                return None;
            }
            if bytes[3] == kind::CLIENT_SEND {
                Envelope::ClientSend { inner: body }
            } else {
                Envelope::ToClient { inner: body }
            }
        }
        _ => return None,
    };
    Some(envelope)
}

fn header(kind: u8, capacity: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(&RELAY_MAGIC);
    out.push(RELAY_VERSION);
    out.push(kind);
    out
}

/// A `Register`, padded to [`REGISTER_LEN`].
pub fn register(
    locator: &Locator,
    token: &[u8; HOST_TOKEN_LEN],
    cookie: &[u8; RELAY_COOKIE_LEN],
) -> Vec<u8> {
    let mut out = header(kind::REGISTER, REGISTER_LEN);
    out.extend_from_slice(locator);
    out.extend_from_slice(token);
    out.extend_from_slice(cookie);
    out.resize(REGISTER_LEN, 0);
    out
}

/// An `Unregister`, padded to [`REGISTER_LEN`].
pub fn unregister(locator: &Locator, token: &[u8; HOST_TOKEN_LEN]) -> Vec<u8> {
    let mut out = header(kind::UNREGISTER, REGISTER_LEN);
    out.extend_from_slice(locator);
    out.extend_from_slice(token);
    out.resize(REGISTER_LEN, 0);
    out
}

/// A `RegisterChallenge`.
pub fn register_challenge(locator: &Locator, cookie: &[u8; RELAY_COOKIE_LEN]) -> Vec<u8> {
    let mut out = header(kind::REGISTER_CHALLENGE, 36);
    out.extend_from_slice(locator);
    out.extend_from_slice(cookie);
    out
}

/// A `Registered`.
pub fn registered(locator: &Locator) -> Vec<u8> {
    let mut out = header(kind::REGISTERED, 20);
    out.extend_from_slice(locator);
    out
}

/// An `Attach`, padded to [`ATTACH_LEN`].
pub fn attach(locator: &Locator, cookie: &[u8; RELAY_COOKIE_LEN]) -> Vec<u8> {
    let mut out = header(kind::ATTACH, ATTACH_LEN);
    out.extend_from_slice(locator);
    out.extend_from_slice(cookie);
    out.resize(ATTACH_LEN, 0);
    out
}

/// An `AttachChallenge`.
pub fn attach_challenge(locator: &Locator, cookie: &[u8; RELAY_COOKIE_LEN]) -> Vec<u8> {
    let mut out = header(kind::ATTACH_CHALLENGE, 36);
    out.extend_from_slice(locator);
    out.extend_from_slice(cookie);
    out
}

/// An `Attached`.
pub fn attached(locator: &Locator) -> Vec<u8> {
    let mut out = header(kind::ATTACHED, 20);
    out.extend_from_slice(locator);
    out
}

/// A `Refused`.
pub fn refused(locator: &Locator, reason: Refusal) -> Vec<u8> {
    let mut out = header(kind::REFUSED, 21);
    out.extend_from_slice(locator);
    out.push(reason.to_byte());
    out
}

/// A `NotAttached`.
pub fn not_attached() -> Vec<u8> {
    header(kind::NOT_ATTACHED, RELAY_HEADER_LEN)
}

fn with_slot(kind: u8, slot: u32, inner: &[u8]) -> Vec<u8> {
    let mut out = header(kind, SLOT_ENVELOPE_LEN + inner.len());
    out.extend_from_slice(&slot.to_le_bytes());
    out.extend_from_slice(inner);
    out
}

/// A `HostSend` of an inner datagram to a slot.
pub fn host_send(slot: u32, inner: &[u8]) -> Vec<u8> {
    with_slot(kind::HOST_SEND, slot, inner)
}

/// A `ToHost` of an inner datagram from a slot.
pub fn to_host(slot: u32, inner: &[u8]) -> Vec<u8> {
    with_slot(kind::TO_HOST, slot, inner)
}

fn plain(kind: u8, inner: &[u8]) -> Vec<u8> {
    let mut out = header(kind, RELAY_HEADER_LEN + inner.len());
    out.extend_from_slice(inner);
    out
}

/// A `ClientSend` of an inner datagram.
pub fn client_send(inner: &[u8]) -> Vec<u8> {
    plain(kind::CLIENT_SEND, inner)
}

/// A `ToClient` of an inner datagram.
pub fn to_client(inner: &[u8]) -> Vec<u8> {
    plain(kind::TO_CLIENT, inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wish_core::Rng;

    fn inner() -> Vec<u8> {
        let mut out = wire::data_header(7, 9).to_vec();
        out.extend_from_slice(&[0xAB; 40]);
        out
    }

    fn every_envelope() -> Vec<Vec<u8>> {
        let l = [3u8; LOCATOR_LEN];
        let t = [4u8; HOST_TOKEN_LEN];
        let c = [5u8; RELAY_COOKIE_LEN];
        let i = inner();
        vec![
            register(&l, &t, &c),
            unregister(&l, &t),
            register_challenge(&l, &c),
            registered(&l),
            attach(&l, &c),
            attach_challenge(&l, &c),
            attached(&l),
            refused(&l, Refusal::RoomTaken),
            refused(&l, Refusal::RelayFull),
            refused(&l, Refusal::RoomFull),
            refused(&l, Refusal::NoRoom),
            not_attached(),
            host_send(42, &i),
            to_host(42, &i),
            client_send(&i),
            to_client(&i),
        ]
    }

    #[test]
    fn every_kind_round_trips() {
        let l = [3u8; LOCATOR_LEN];
        let t = [4u8; HOST_TOKEN_LEN];
        let c = [5u8; RELAY_COOKIE_LEN];
        let i = inner();
        assert_eq!(
            decode(&register(&l, &t, &c)),
            Some(Envelope::Register {
                locator: l,
                token: t,
                cookie: c
            })
        );
        assert_eq!(
            decode(&unregister(&l, &t)),
            Some(Envelope::Unregister {
                locator: l,
                token: t
            })
        );
        assert_eq!(
            decode(&register_challenge(&l, &c)),
            Some(Envelope::RegisterChallenge {
                locator: l,
                cookie: c
            })
        );
        assert_eq!(
            decode(&registered(&l)),
            Some(Envelope::Registered { locator: l })
        );
        assert_eq!(
            decode(&attach(&l, &c)),
            Some(Envelope::Attach {
                locator: l,
                cookie: c
            })
        );
        assert_eq!(
            decode(&attach_challenge(&l, &c)),
            Some(Envelope::AttachChallenge {
                locator: l,
                cookie: c
            })
        );
        assert_eq!(
            decode(&attached(&l)),
            Some(Envelope::Attached { locator: l })
        );
        for reason in [
            Refusal::RoomTaken,
            Refusal::RelayFull,
            Refusal::RoomFull,
            Refusal::NoRoom,
        ] {
            assert_eq!(
                decode(&refused(&l, reason)),
                Some(Envelope::Refused { locator: l, reason })
            );
        }
        assert_eq!(decode(&not_attached()), Some(Envelope::NotAttached));
        assert_eq!(
            decode(&host_send(42, &i)),
            Some(Envelope::HostSend {
                slot: 42,
                inner: &i
            })
        );
        assert_eq!(
            decode(&to_host(42, &i)),
            Some(Envelope::ToHost {
                slot: 42,
                inner: &i
            })
        );
        assert_eq!(
            decode(&client_send(&i)),
            Some(Envelope::ClientSend { inner: &i })
        );
        assert_eq!(
            decode(&to_client(&i)),
            Some(Envelope::ToClient { inner: &i })
        );
    }

    #[test]
    fn answers_are_smaller_than_what_they_answer() {
        let l = [3u8; LOCATOR_LEN];
        let c = [5u8; RELAY_COOKIE_LEN];
        for answer in [
            register_challenge(&l, &c),
            registered(&l),
            refused(&l, Refusal::NoRoom),
            not_attached(),
        ] {
            assert!(answer.len() < ATTACH_LEN, "{}", answer.len());
            assert!(answer.len() < REGISTER_LEN);
        }
        assert!(attach_challenge(&l, &c).len() < ATTACH_LEN);
        assert!(attached(&l).len() < ATTACH_LEN);
        // A `NotAttached` answers data, whose envelope is never shorter.
        assert!(not_attached().len() < RELAY_HEADER_LEN + wire::HEADER_LEN);
        // The largest inner datagram fits a relay datagram with its envelope.
        let big = {
            let mut out = wire::data_header(1, 1).to_vec();
            out.resize(wire::MAX_PACKET, 0);
            out
        };
        assert!(host_send(u32::MAX, &big).len() <= MAX_RELAY_DATAGRAM);
        assert!(decode(&host_send(u32::MAX, &big)).is_some());
        let mut too_big = big.clone();
        too_big.push(0);
        assert_eq!(decode(&client_send(&too_big)), None);
    }

    #[test]
    fn only_transport_datagrams_are_carried() {
        assert_eq!(decode(&client_send(b"hello world")), None);
        assert_eq!(decode(&client_send(b"WN")), None);
        assert_eq!(decode(&host_send(1, b"")), None);
        // Wrong version or magic.
        let mut other = client_send(&inner());
        other[2] = 2;
        assert_eq!(decode(&other), None);
        other[2] = RELAY_VERSION;
        other[1] = b'X';
        assert_eq!(decode(&other), None);
        // An unpadded register is not one.
        let mut short = register(&[1; 16], &[2; 16], &[3; 16]);
        short.truncate(52);
        assert_eq!(decode(&short), None);
    }

    #[test]
    fn locators_differ_by_code_and_do_not_contain_it() {
        let a = RoomCode::parse("STAR42").unwrap();
        let b = RoomCode::parse("STAR43").unwrap();
        assert_ne!(locator(&a), locator(&b));
        assert_eq!(locator(&a), locator(&RoomCode::parse("star-42").unwrap()));
        assert!(
            !locator(&a)
                .windows(a.as_str().len())
                .any(|w| w == a.as_str().as_bytes())
        );
    }

    #[test]
    fn cookies_depend_on_everything() {
        let secret = [9u8; 32];
        let addr: SocketAddr = "10.0.0.1:4000".parse().unwrap();
        let other: SocketAddr = "10.0.0.1:4001".parse().unwrap();
        let l = [1u8; LOCATOR_LEN];
        let base = cookie(&secret, 3, addr, &l, Role::Host);
        assert_ne!(base, cookie(&secret, 4, addr, &l, Role::Host));
        assert_ne!(base, cookie(&secret, 3, other, &l, Role::Host));
        assert_ne!(base, cookie(&secret, 3, addr, &[2; 16], Role::Host));
        assert_ne!(base, cookie(&secret, 3, addr, &l, Role::Client));
        assert_ne!(base, cookie(&[8; 32], 3, addr, &l, Role::Host));
    }

    #[test]
    fn random_and_mutated_bytes_never_panic() {
        let mut rng = Rng::from_seed(29);
        let envelopes = every_envelope();
        for round in 0..200_000u32 {
            let mut bytes = if round % 2 == 0 {
                envelopes[rng.below(envelopes.len() as u32) as usize].clone()
            } else {
                let len = rng.below(1400) as usize;
                (0..len).map(|_| rng.next_u32() as u8).collect()
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
            let _ = decode(&bytes);
        }
    }
}
