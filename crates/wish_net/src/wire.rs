//! The packet layout, as pure encode and decode functions. Decoding never
//! panics and never allocates more than the packet holds: every input is
//! untrusted.
//!
//! Every datagram starts with `W N`, the protocol version and the kind. All
//! numbers are little-endian. The layout of `Hello` up to its token and of
//! `Reject` is kept by every later version, so a host can tell a client of
//! another version why it cannot join.

use crate::crypto::{COOKIE_LEN, PUBLIC_KEY_LEN, TAG_LEN, TOKEN_LEN};

/// The first two bytes of every datagram.
pub const MAGIC: [u8; 2] = *b"WN";
/// The protocol version this crate speaks.
pub const VERSION: u8 = 1;
/// The largest datagram an endpoint sends or reads, in bytes. It fits under
/// every common path MTU, so datagrams are never split by IP.
pub const MAX_PACKET: usize = 1200;
/// The plain header of every datagram: magic, version, kind.
pub const HEADER_LEN: usize = 4;
/// A `Hello` is padded to this size, so the `Challenge` that answers it is
/// smaller (the host cannot be used to amplify traffic).
pub const HELLO_LEN: usize = 256;
/// The header of a data packet: magic, version, kind, connection id, packet
/// number. It is the associated data of the seal.
pub const DATA_HEADER_LEN: usize = HEADER_LEN + 4 + 8;
/// The most frame bytes one data packet carries.
pub const MAX_PLAINTEXT: usize = MAX_PACKET - DATA_HEADER_LEN - TAG_LEN;
/// The most message bytes one fragment carries.
pub const FRAGMENT_LEN: usize = 1024;
/// The bytes a fragment frame adds before its message bytes.
pub const FRAGMENT_HEADER_LEN: usize = 1 + 4 + 2 + 2 + 2;
/// The longest game id in discovery, in bytes.
pub const MAX_GAME_ID: usize = 64;
/// The longest match name in discovery, in bytes.
pub const MAX_NAME: usize = 64;

/// The kinds of datagram.
pub mod kind {
    /// Client → host: the first handshake packet.
    pub const HELLO: u8 = 1;
    /// Host → client: the cookie and the host's key.
    pub const CHALLENGE: u8 = 2;
    /// Client → host: the cookie and the sealed room code.
    pub const CONNECT: u8 = 3;
    /// Host → client: the join was refused.
    pub const REJECT: u8 = 4;
    /// Both ways: sealed frames.
    pub const DATA: u8 = 5;
    /// Discovery: who hosts a match of this game?
    pub const QUERY: u8 = 16;
    /// Discovery: a host's answer.
    pub const ANNOUNCE: u8 = 17;
}

/// Why a host refused a join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The room code was not the host's.
    RoomCodeWrong,
    /// The match has as many players as it takes.
    RoomFull,
    /// The client speaks another protocol version.
    VersionMismatch,
}

impl RejectReason {
    fn to_byte(self) -> u8 {
        match self {
            Self::RoomCodeWrong => 1,
            Self::RoomFull => 2,
            Self::VersionMismatch => 3,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::RoomCodeWrong),
            2 => Some(Self::RoomFull),
            3 => Some(Self::VersionMismatch),
            _ => None,
        }
    }
}

/// The reason a `Disconnect` frame gives.
pub mod disconnect {
    /// The other side closed the connection.
    pub const CLOSED: u8 = 0;
    /// The other side broke the protocol.
    pub const PROTOCOL_VIOLATION: u8 = 1;
}

/// A decoded datagram. Sealed parts are borrowed from the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet<'a> {
    /// Client → host, padded to [`HELLO_LEN`].
    Hello {
        /// The client's public key for this attempt.
        client_key: [u8; PUBLIC_KEY_LEN],
        /// Random bytes the host echoes, so third parties cannot answer.
        token: [u8; TOKEN_LEN],
    },
    /// Host → client.
    Challenge {
        /// The client's token, echoed.
        token: [u8; TOKEN_LEN],
        /// Proof, for the `Connect`, that the client receives at its address.
        cookie: [u8; COOKIE_LEN],
        /// The host's public key for this match.
        host_key: [u8; PUBLIC_KEY_LEN],
    },
    /// Client → host.
    Connect {
        /// The client's public key, as in its `Hello`.
        client_key: [u8; PUBLIC_KEY_LEN],
        /// The client's token, as in its `Hello`.
        token: [u8; TOKEN_LEN],
        /// The host's cookie, echoed.
        cookie: [u8; COOKIE_LEN],
        /// Every byte before the sealed room code (its associated data).
        header: &'a [u8],
        /// The room code, sealed with the client's key.
        sealed: &'a [u8],
    },
    /// Host → client.
    Reject {
        /// The client's token, echoed.
        token: [u8; TOKEN_LEN],
        /// Why.
        reason: RejectReason,
    },
    /// Both ways.
    Data {
        /// The connection's id, given by the host.
        connection: u32,
        /// The packet number (the nonce).
        number: u64,
        /// The [`DATA_HEADER_LEN`] header bytes (the associated data).
        header: &'a [u8],
        /// The sealed frames and the tag.
        sealed: &'a [u8],
    },
    /// Discovery query.
    Query {
        /// Random bytes the answer echoes.
        nonce: [u8; 16],
        /// The game's id (UTF-8, at most [`MAX_GAME_ID`] bytes).
        game: &'a str,
    },
    /// Discovery answer.
    Announce {
        /// The query's nonce, echoed.
        nonce: [u8; 16],
        /// The game's id.
        game: &'a str,
        /// The match's name (UTF-8, at most [`MAX_NAME`] bytes).
        name: &'a str,
        /// The UDP port the match is hosted on.
        port: u16,
        /// Players in the match.
        players: u16,
        /// Players the match takes.
        max_players: u16,
        /// The host's public key (a client can pin it).
        host_key: [u8; PUBLIC_KEY_LEN],
        /// The room code, if the host shares it on the local network.
        room_code: &'a str,
    },
    /// A datagram of this protocol, but of another version.
    OtherVersion {
        /// Its version.
        version: u8,
        /// The client token, when it is a `Hello` (whose layout every version
        /// keeps).
        token: Option<[u8; TOKEN_LEN]>,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn slice(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let out = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(out)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.slice(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.array()?))
    }

    fn text(&mut self, max: usize) -> Option<&'a str> {
        let len = usize::from(self.u8()?);
        if len > max {
            return None;
        }
        std::str::from_utf8(self.slice(len)?).ok()
    }

    fn rest(&mut self) -> &'a [u8] {
        let out = self.bytes.get(self.at..).unwrap_or_default();
        self.at = self.bytes.len();
        out
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Decodes a datagram. `None` for anything that is not a well-formed packet
/// of this protocol (dropped by the caller).
pub fn decode(bytes: &[u8]) -> Option<Packet<'_>> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let mut r = Reader::new(bytes);
    if r.array::<2>()? != MAGIC {
        return None;
    }
    let version = r.u8()?;
    let kind = r.u8()?;
    if version != VERSION {
        let token = if kind == kind::HELLO && bytes.len() == HELLO_LEN {
            r.slice(PUBLIC_KEY_LEN)?;
            Some(r.array()?)
        } else {
            None
        };
        return Some(Packet::OtherVersion { version, token });
    }
    let packet = match kind {
        kind::HELLO => {
            if bytes.len() != HELLO_LEN {
                return None;
            }
            let client_key = r.array()?;
            let token = r.array()?;
            r.rest();
            Packet::Hello { client_key, token }
        }
        kind::CHALLENGE => Packet::Challenge {
            token: r.array()?,
            cookie: r.array()?,
            host_key: r.array()?,
        },
        kind::CONNECT => {
            let client_key = r.array()?;
            let token = r.array()?;
            let cookie = r.array()?;
            let header = bytes.get(..r.at)?;
            let sealed = r.rest();
            if sealed.len() < TAG_LEN {
                return None;
            }
            Packet::Connect {
                client_key,
                token,
                cookie,
                header,
                sealed,
            }
        }
        kind::REJECT => Packet::Reject {
            token: r.array()?,
            reason: RejectReason::from_byte(r.u8()?)?,
        },
        kind::DATA => {
            let connection = r.u32()?;
            let number = r.u64()?;
            let header = bytes.get(..DATA_HEADER_LEN)?;
            let sealed = r.rest();
            if sealed.len() < TAG_LEN {
                return None;
            }
            Packet::Data {
                connection,
                number,
                header,
                sealed,
            }
        }
        kind::QUERY => Packet::Query {
            nonce: r.array()?,
            game: r.text(MAX_GAME_ID)?,
        },
        kind::ANNOUNCE => Packet::Announce {
            nonce: r.array()?,
            game: r.text(MAX_GAME_ID)?,
            name: r.text(MAX_NAME)?,
            port: r.u16()?,
            players: r.u16()?,
            max_players: r.u16()?,
            host_key: r.array()?,
            room_code: r.text(crate::room::ROOM_CODE_MAX)?,
        },
        _ => return None,
    };
    r.done().then_some(packet)
}

fn header(kind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.push(kind);
    out
}

/// A `Hello`, padded to [`HELLO_LEN`].
pub fn hello(client_key: &[u8; PUBLIC_KEY_LEN], token: &[u8; TOKEN_LEN]) -> Vec<u8> {
    let mut out = header(kind::HELLO);
    out.extend_from_slice(client_key);
    out.extend_from_slice(token);
    out.resize(HELLO_LEN, 0);
    out
}

/// A `Challenge`.
pub fn challenge(
    token: &[u8; TOKEN_LEN],
    cookie: &[u8; COOKIE_LEN],
    host_key: &[u8; PUBLIC_KEY_LEN],
) -> Vec<u8> {
    let mut out = header(kind::CHALLENGE);
    out.extend_from_slice(token);
    out.extend_from_slice(cookie);
    out.extend_from_slice(host_key);
    out
}

/// The part of a `Connect` before its sealed room code (the seal's
/// associated data).
pub fn connect_header(
    client_key: &[u8; PUBLIC_KEY_LEN],
    token: &[u8; TOKEN_LEN],
    cookie: &[u8; COOKIE_LEN],
) -> Vec<u8> {
    let mut out = header(kind::CONNECT);
    out.extend_from_slice(client_key);
    out.extend_from_slice(token);
    out.extend_from_slice(cookie);
    out
}

/// A `Reject`.
pub fn reject(token: &[u8; TOKEN_LEN], reason: RejectReason) -> Vec<u8> {
    let mut out = header(kind::REJECT);
    out.extend_from_slice(token);
    out.push(reason.to_byte());
    out
}

/// The header of a data packet.
pub fn data_header(connection: u32, number: u64) -> [u8; DATA_HEADER_LEN] {
    let mut out = [0u8; DATA_HEADER_LEN];
    out[..2].copy_from_slice(&MAGIC);
    out[2] = VERSION;
    out[3] = kind::DATA;
    out[4..8].copy_from_slice(&connection.to_le_bytes());
    out[8..16].copy_from_slice(&number.to_le_bytes());
    out
}

fn push_text(out: &mut Vec<u8>, text: &str, max: usize) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push(end as u8);
    out.extend_from_slice(&text.as_bytes()[..end]);
}

/// A discovery `Query`. A game id longer than [`MAX_GAME_ID`] is cut.
pub fn query(nonce: &[u8; 16], game: &str) -> Vec<u8> {
    let mut out = header(kind::QUERY);
    out.extend_from_slice(nonce);
    push_text(&mut out, game, MAX_GAME_ID);
    out
}

/// The fields of an `Announce` to encode.
#[derive(Debug, Clone, Copy)]
pub struct AnnounceFields<'a> {
    /// The query's nonce.
    pub nonce: &'a [u8; 16],
    /// The game's id.
    pub game: &'a str,
    /// The match's name.
    pub name: &'a str,
    /// The match's port.
    pub port: u16,
    /// Players in the match.
    pub players: u16,
    /// Players the match takes.
    pub max_players: u16,
    /// The host's public key.
    pub host_key: &'a [u8; PUBLIC_KEY_LEN],
    /// The room code, or an empty text.
    pub room_code: &'a str,
}

/// A discovery `Announce`. Texts longer than their limits are cut.
pub fn announce(fields: &AnnounceFields<'_>) -> Vec<u8> {
    let mut out = header(kind::ANNOUNCE);
    out.extend_from_slice(fields.nonce);
    push_text(&mut out, fields.game, MAX_GAME_ID);
    push_text(&mut out, fields.name, MAX_NAME);
    out.extend_from_slice(&fields.port.to_le_bytes());
    out.extend_from_slice(&fields.players.to_le_bytes());
    out.extend_from_slice(&fields.max_players.to_le_bytes());
    out.extend_from_slice(fields.host_key);
    push_text(&mut out, fields.room_code, crate::room::ROOM_CODE_MAX);
    out
}

/// A frame inside a sealed data packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame<'a> {
    /// Host → client: the connection was accepted, with the client's id.
    Welcome {
        /// The id the host gave the client.
        peer: u32,
    },
    /// Acknowledges reliable fragments: every one below `next` arrived, and
    /// bit `i` of `mask` says that `next + 1 + i` arrived.
    Ack {
        /// The next reliable sequence the receiver waits for.
        next: u32,
        /// Fragments after `next` that arrived.
        mask: u64,
    },
    /// A fragment of a reliable message.
    Reliable {
        /// The fragment's reliable sequence number.
        seq: u32,
        /// Its place in its message.
        index: u16,
        /// The fragments of its message.
        count: u16,
        /// Its message bytes.
        bytes: &'a [u8],
    },
    /// A fragment of an unreliable message.
    Unreliable {
        /// The message's id.
        id: u32,
        /// Its place in its message.
        index: u16,
        /// The fragments of its message.
        count: u16,
        /// Its message bytes.
        bytes: &'a [u8],
    },
    /// Asks for a `Pong`, to measure the round trip and keep the connection.
    Ping {
        /// The sender's time in milliseconds.
        time: u64,
    },
    /// Answers a `Ping`.
    Pong {
        /// The `Ping`'s time, echoed.
        time: u64,
    },
    /// The sender closes the connection.
    Disconnect {
        /// One of [`disconnect`].
        reason: u8,
    },
}

mod tag {
    pub const WELCOME: u8 = 1;
    pub const ACK: u8 = 2;
    pub const RELIABLE: u8 = 3;
    pub const UNRELIABLE: u8 = 4;
    pub const PING: u8 = 5;
    pub const PONG: u8 = 6;
    pub const DISCONNECT: u8 = 7;
}

impl Frame<'_> {
    /// The frame's encoded size in bytes.
    pub fn encoded_len(&self) -> usize {
        match self {
            Self::Welcome { .. } => 1 + 4,
            Self::Ack { .. } => 1 + 4 + 8,
            Self::Reliable { bytes, .. } | Self::Unreliable { bytes, .. } => {
                FRAGMENT_HEADER_LEN + bytes.len()
            }
            Self::Ping { .. } | Self::Pong { .. } => 1 + 8,
            Self::Disconnect { .. } => 1 + 1,
        }
    }

    /// Appends the frame. A fragment longer than [`FRAGMENT_LEN`] is not
    /// written (the endpoint never makes one).
    pub fn encode(&self, out: &mut Vec<u8>) {
        match *self {
            Self::Welcome { peer } => {
                out.push(tag::WELCOME);
                out.extend_from_slice(&peer.to_le_bytes());
            }
            Self::Ack { next, mask } => {
                out.push(tag::ACK);
                out.extend_from_slice(&next.to_le_bytes());
                out.extend_from_slice(&mask.to_le_bytes());
            }
            Self::Reliable {
                seq: id,
                index,
                count,
                bytes,
            }
            | Self::Unreliable {
                id,
                index,
                count,
                bytes,
            } => {
                let Ok(len) = u16::try_from(bytes.len()) else {
                    return;
                };
                if bytes.len() > FRAGMENT_LEN {
                    return;
                }
                out.push(if matches!(self, Self::Reliable { .. }) {
                    tag::RELIABLE
                } else {
                    tag::UNRELIABLE
                });
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&index.to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(bytes);
            }
            Self::Ping { time } => {
                out.push(tag::PING);
                out.extend_from_slice(&time.to_le_bytes());
            }
            Self::Pong { time } => {
                out.push(tag::PONG);
                out.extend_from_slice(&time.to_le_bytes());
            }
            Self::Disconnect { reason } => {
                out.push(tag::DISCONNECT);
                out.push(reason);
            }
        }
    }
}

/// Decodes every frame of an opened data packet into `out` (cleared first).
/// Returns `false` when any frame is malformed: an unknown tag, a fragment
/// longer than [`FRAGMENT_LEN`], an index not below its count, or bytes cut
/// short.
pub fn decode_frames<'a>(bytes: &'a [u8], out: &mut Vec<Frame<'a>>) -> bool {
    out.clear();
    let mut r = Reader::new(bytes);
    while !r.done() {
        let Some(frame) = decode_frame(&mut r) else {
            out.clear();
            return false;
        };
        out.push(frame);
    }
    true
}

fn decode_frame<'a>(r: &mut Reader<'a>) -> Option<Frame<'a>> {
    let frame = match r.u8()? {
        tag::WELCOME => Frame::Welcome { peer: r.u32()? },
        tag::ACK => Frame::Ack {
            next: r.u32()?,
            mask: r.u64()?,
        },
        t @ (tag::RELIABLE | tag::UNRELIABLE) => {
            let id = r.u32()?;
            let index = r.u16()?;
            let count = r.u16()?;
            let len = usize::from(r.u16()?);
            if len > FRAGMENT_LEN || count == 0 || index >= count {
                return None;
            }
            let bytes = r.slice(len)?;
            if t == tag::RELIABLE {
                Frame::Reliable {
                    seq: id,
                    index,
                    count,
                    bytes,
                }
            } else {
                Frame::Unreliable {
                    id,
                    index,
                    count,
                    bytes,
                }
            }
        }
        tag::PING => Frame::Ping { time: r.u64()? },
        tag::PONG => Frame::Pong { time: r.u64()? },
        tag::DISCONNECT => Frame::Disconnect { reason: r.u8()? },
        _ => return None,
    };
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wish_core::Rng;

    fn every_packet() -> Vec<Vec<u8>> {
        let mut connect = connect_header(&[1; 32], &[2; 16], &[3; 16]);
        connect.extend_from_slice(&[9; 24]);
        let mut data = data_header(7, 42).to_vec();
        data.extend_from_slice(&[5; 40]);
        vec![
            hello(&[1; 32], &[2; 16]),
            challenge(&[2; 16], &[3; 16], &[4; 32]),
            connect,
            reject(&[2; 16], RejectReason::RoomFull),
            data,
            query(&[6; 16], "station"),
            announce(&AnnounceFields {
                nonce: &[6; 16],
                game: "station",
                name: "Ada's room",
                port: 7777,
                players: 3,
                max_players: 10,
                host_key: &[4; 32],
                room_code: "ABCD23",
            }),
        ]
    }

    #[test]
    fn every_packet_kind_round_trips() {
        let packets = every_packet();
        assert_eq!(
            decode(&packets[0]),
            Some(Packet::Hello {
                client_key: [1; 32],
                token: [2; 16]
            })
        );
        assert_eq!(
            decode(&packets[1]),
            Some(Packet::Challenge {
                token: [2; 16],
                cookie: [3; 16],
                host_key: [4; 32]
            })
        );
        match decode(&packets[2]) {
            Some(Packet::Connect { header, sealed, .. }) => {
                assert_eq!(header.len(), 68);
                assert_eq!(sealed, &[9; 24]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            decode(&packets[3]),
            Some(Packet::Reject {
                reason: RejectReason::RoomFull,
                ..
            })
        ));
        assert!(matches!(
            decode(&packets[4]),
            Some(Packet::Data {
                connection: 7,
                number: 42,
                ..
            })
        ));
        assert!(matches!(
            decode(&packets[5]),
            Some(Packet::Query {
                game: "station",
                ..
            })
        ));
        match decode(&packets[6]) {
            Some(Packet::Announce {
                name,
                port,
                players,
                max_players,
                room_code,
                ..
            }) => {
                assert_eq!(
                    (name, port, players, max_players, room_code),
                    ("Ada's room", 7777, 3, 10, "ABCD23")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_version_is_recognised_with_its_token() {
        let mut packet = hello(&[1; 32], &[2; 16]);
        packet[2] = 9;
        assert_eq!(
            decode(&packet),
            Some(Packet::OtherVersion {
                version: 9,
                token: Some([2; 16])
            })
        );
    }

    #[test]
    fn every_frame_round_trips() {
        let frames = [
            Frame::Welcome { peer: 3 },
            Frame::Ack {
                next: 9,
                mask: u64::MAX - 3,
            },
            Frame::Reliable {
                seq: 1,
                index: 0,
                count: 2,
                bytes: &[1, 2, 3],
            },
            Frame::Unreliable {
                id: 8,
                index: 1,
                count: 2,
                bytes: &[],
            },
            Frame::Ping { time: 77 },
            Frame::Pong { time: 78 },
            Frame::Disconnect {
                reason: disconnect::CLOSED,
            },
        ];
        let mut bytes = Vec::new();
        for frame in &frames {
            let before = bytes.len();
            frame.encode(&mut bytes);
            assert_eq!(bytes.len() - before, frame.encoded_len());
        }
        let mut out = Vec::new();
        assert!(decode_frames(&bytes, &mut out));
        assert_eq!(out, frames);
    }

    #[test]
    fn bad_frames_are_refused() {
        let mut out = Vec::new();
        // An index not below its count.
        let mut bytes = Vec::new();
        bytes.push(3);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        assert!(!decode_frames(&bytes, &mut out));
        // A fragment longer than allowed.
        let mut bytes = Vec::new();
        bytes.push(4);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2000u16.to_le_bytes());
        bytes.resize(bytes.len() + 2000, 0);
        assert!(!decode_frames(&bytes, &mut out));
        // An unknown tag, and a frame cut short.
        assert!(!decode_frames(&[99], &mut out));
        assert!(!decode_frames(&[5, 1, 2], &mut out));
        assert!(decode_frames(&[], &mut out));
    }

    #[test]
    fn random_and_mutated_bytes_never_panic() {
        let mut rng = Rng::from_seed(11);
        let packets = every_packet();
        for round in 0..200_000u32 {
            let mut frames = Vec::new();
            let mut bytes = if round % 2 == 0 {
                packets[rng.below(packets.len() as u32) as usize].clone()
            } else {
                let len = rng.below(1300) as usize;
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
            let _ = decode_frames(&bytes, &mut frames);
        }
    }
}
