//! The lobby's and the messages' wire format (kinds 64 to 95, design note
//! §4), as pure encode and decode functions. Decoding never panics and
//! never allocates beyond what the bytes can hold.

use wish_replicate::wire::{Reader, put_varint};
use wish_types::{LobbyClosed, LobbyPhase, MAX_PLAYERS};

use crate::lobby::{LobbyState, MAX_NAME_BYTES, MAX_ROLE_NAME, SeatState};
use crate::message::MAX_MESSAGE_TYPES;

/// Message kinds of the lobby (64 to 95).
pub mod kind {
    /// Client → host: take a seat.
    pub const JOIN: u8 = 64;
    /// Client → host: a new name and colour.
    pub const PROFILE: u8 = 65;
    /// Client → host: ready or not.
    pub const READY: u8 = 66;
    /// Client → host: leaving.
    pub const LEAVE: u8 = 67;
    /// Client → host: a game message.
    pub const MESSAGE_UP: u8 = 68;
    /// Host → client: the seat, the rejoin token and the message types.
    pub const ADMITTED: u8 = 80;
    /// Host → client: the whole lobby.
    pub const LOBBY: u8 = 81;
    /// Host → client: this player's role.
    pub const ROLE: u8 = 82;
    /// Host → client: a game message.
    pub const MESSAGE_DOWN: u8 = 83;
    /// Host → client: the host refused this player.
    pub const REFUSED: u8 = 84;
    /// Host → client: the host is closing the session.
    pub const CLOSING: u8 = 85;
    /// The first kind of the lobby.
    pub const FIRST: u8 = 64;
    /// The last kind of the lobby.
    pub const LAST: u8 = 95;
}

/// The version of the lobby's messages.
pub const VERSION: u8 = 1;

/// Bytes of a rejoin token.
pub const TOKEN_LEN: usize = 16;

/// Whether a message belongs to the lobby (by its first byte).
pub fn is_lobby(data: &[u8]) -> bool {
    data.first()
        .is_some_and(|k| (kind::FIRST..=kind::LAST).contains(k))
}

/// Why the host refused a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Every seat is taken.
    Full,
    /// A match is running.
    Running,
    /// The rejoin came after the grace period, or the token is unknown.
    RejoinExpired,
    /// Another version of the lobby's messages.
    Version,
}

impl Refusal {
    fn code(self) -> u8 {
        match self {
            Self::Full => 1,
            Self::Running => 2,
            Self::RejoinExpired => 3,
            Self::Version => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Full,
            2 => Self::Running,
            3 => Self::RejoinExpired,
            4 => Self::Version,
            _ => return None,
        })
    }
}

/// A client's join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    /// The version the client speaks.
    pub version: u8,
    /// The token of a seat it held, to come back into it.
    pub token: Option<[u8; TOKEN_LEN]>,
    /// The name it asks for.
    pub name: String,
    /// The colour it asks for.
    pub color: u8,
}

/// One message type as `Admitted` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName {
    /// The event's name.
    pub name: String,
    /// Its schema fingerprint.
    pub fingerprint: [u8; 16],
}

/// What a client may send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// Take a seat.
    Join(Join),
    /// A new name and colour.
    Profile {
        /// The name.
        name: String,
        /// The colour.
        color: u8,
    },
    /// Ready or not.
    Ready(bool),
    /// Leaving.
    Leave,
    /// A game message: the type index and the rest of the bytes (its
    /// leaves, read against the type's layout).
    Message {
        /// The type's index in `Admitted`.
        ty: u16,
        /// Where its leaves start in the message.
        at: usize,
    },
}

/// What the host may send.
#[derive(Debug, Clone, PartialEq)]
pub enum Down {
    /// The seat, the token, the message types.
    Admitted {
        /// The version of the host.
        version: u8,
        /// The seat (the player id).
        seat: u32,
        /// The token to come back with.
        token: [u8; TOKEN_LEN],
        /// The message types, in the order messages number them.
        types: Vec<TypeName>,
    },
    /// The whole lobby.
    Lobby(Box<LobbyState>),
    /// This player's role.
    Role {
        /// The round.
        round: u32,
        /// The role.
        role: u32,
        /// Its name.
        name: String,
    },
    /// A game message.
    Message {
        /// The type's index in `Admitted`.
        ty: u16,
        /// The host's tick when it was sent.
        tick: u64,
        /// Where its leaves start in the message.
        at: usize,
    },
    /// The host refused this player.
    Refused(Refusal),
    /// The host is closing.
    Closing,
}

fn put_str(out: &mut Vec<u8>, text: &str, max: usize) {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    put_varint(out, end as u64);
    out.extend_from_slice(text.as_bytes().get(..end).unwrap_or_default());
}

fn read_str(reader: &mut Reader<'_>, max: usize) -> Option<String> {
    let len = usize::try_from(reader.varint()?).ok()?;
    if len > max {
        return None;
    }
    Some(std::str::from_utf8(reader.take(len)?).ok()?.to_owned())
}

/// Encodes a client's message (for `Message`, the header only: the caller
/// appends the leaves).
pub fn encode_up(up: &Up) -> Vec<u8> {
    match up {
        Up::Join(join) => {
            let mut out = vec![kind::JOIN, join.version];
            match &join.token {
                Some(token) => {
                    out.push(1);
                    out.extend_from_slice(token);
                }
                None => out.push(0),
            }
            put_str(&mut out, &join.name, MAX_NAME_BYTES);
            out.push(join.color);
            out
        }
        Up::Profile { name, color } => {
            let mut out = vec![kind::PROFILE];
            put_str(&mut out, name, MAX_NAME_BYTES);
            out.push(*color);
            out
        }
        Up::Ready(ready) => vec![kind::READY, u8::from(*ready)],
        Up::Leave => vec![kind::LEAVE],
        Up::Message { ty, .. } => {
            let mut out = vec![kind::MESSAGE_UP];
            put_varint(&mut out, u64::from(*ty));
            out
        }
    }
}

/// Decodes a client's message; `None` when it is malformed or not a
/// client's kind.
pub fn decode_up(bytes: &[u8]) -> Option<Up> {
    let mut reader = Reader::new(bytes);
    let up = match reader.u8()? {
        kind::JOIN => {
            let version = reader.u8()?;
            let token = match reader.u8()? {
                0 => None,
                1 => Some(reader.take(TOKEN_LEN)?.try_into().ok()?),
                _ => return None,
            };
            let name = read_str(&mut reader, MAX_NAME_BYTES)?;
            let color = reader.u8()?;
            Up::Join(Join {
                version,
                token,
                name,
                color,
            })
        }
        kind::PROFILE => {
            let name = read_str(&mut reader, MAX_NAME_BYTES)?;
            let color = reader.u8()?;
            Up::Profile { name, color }
        }
        kind::READY => match reader.u8()? {
            0 => Up::Ready(false),
            1 => Up::Ready(true),
            _ => return None,
        },
        kind::LEAVE => Up::Leave,
        kind::MESSAGE_UP => {
            let ty = u16::try_from(reader.varint()?).ok()?;
            let at = bytes.len() - reader.remaining();
            return Some(Up::Message { ty, at });
        }
        _ => return None,
    };
    reader.done().then_some(up)
}

fn phase_code(phase: LobbyPhase) -> u8 {
    match phase {
        LobbyPhase::Offline => 0,
        LobbyPhase::Open => 1,
        LobbyPhase::Running => 2,
        LobbyPhase::Ended => 3,
        LobbyPhase::Closed => 4,
    }
}

fn phase_from(code: u8) -> Option<LobbyPhase> {
    Some(match code {
        0 => LobbyPhase::Offline,
        1 => LobbyPhase::Open,
        2 => LobbyPhase::Running,
        3 => LobbyPhase::Ended,
        4 => LobbyPhase::Closed,
        _ => return None,
    })
}

/// The room code's longest form on the wire.
const MAX_ROOM: usize = 32;

/// Encodes a host's message (for `Message`, the header only: the caller
/// appends the leaves).
pub fn encode_down(down: &Down) -> Vec<u8> {
    match down {
        Down::Admitted {
            version,
            seat,
            token,
            types,
        } => {
            let mut out = vec![kind::ADMITTED, *version];
            put_varint(&mut out, u64::from(*seat));
            out.extend_from_slice(token);
            put_varint(&mut out, types.len() as u64);
            for ty in types {
                put_str(&mut out, &ty.name, 128);
                out.extend_from_slice(&ty.fingerprint);
            }
            out
        }
        Down::Lobby(state) => {
            let mut out = vec![kind::LOBBY, phase_code(state.phase)];
            put_varint(&mut out, u64::from(state.round));
            put_varint(&mut out, u64::from(state.outcome));
            put_str(&mut out, &state.room, MAX_ROOM);
            for seat in &state.seats {
                let flags = u8::from(seat.taken)
                    | u8::from(seat.connected) << 1
                    | u8::from(seat.ready) << 2
                    | u8::from(seat.left) << 3;
                out.push(flags);
                out.push(u8::try_from(seat.color).unwrap_or(u8::MAX));
                put_str(&mut out, &seat.name, MAX_NAME_BYTES);
            }
            out
        }
        Down::Role { round, role, name } => {
            let mut out = vec![kind::ROLE];
            put_varint(&mut out, u64::from(*round));
            put_varint(&mut out, u64::from(*role));
            put_str(&mut out, name, MAX_ROLE_NAME);
            out
        }
        Down::Message { ty, tick, .. } => {
            let mut out = vec![kind::MESSAGE_DOWN];
            put_varint(&mut out, u64::from(*ty));
            put_varint(&mut out, *tick);
            out
        }
        Down::Refused(reason) => vec![kind::REFUSED, reason.code()],
        Down::Closing => vec![kind::CLOSING],
    }
}

/// Decodes a host's message; `None` when it is malformed or not a host's
/// kind.
pub fn decode_down(bytes: &[u8]) -> Option<Down> {
    let mut reader = Reader::new(bytes);
    let down = match reader.u8()? {
        kind::ADMITTED => {
            let version = reader.u8()?;
            let seat = reader.varint_u32()?;
            if seat as usize >= MAX_PLAYERS {
                return None;
            }
            let token = reader.take(TOKEN_LEN)?.try_into().ok()?;
            let count = reader.count(MAX_MESSAGE_TYPES, 17)?;
            let mut types = Vec::with_capacity(count);
            for _ in 0..count {
                let name = read_str(&mut reader, 128)?;
                let fingerprint = reader.take(16)?.try_into().ok()?;
                types.push(TypeName { name, fingerprint });
            }
            Down::Admitted {
                version,
                seat,
                token,
                types,
            }
        }
        kind::LOBBY => {
            let phase = phase_from(reader.u8()?)?;
            let round = reader.varint_u32()?;
            let outcome = reader.varint_u32()?;
            let room = read_str(&mut reader, MAX_ROOM)?;
            let mut state = LobbyState {
                phase,
                round,
                outcome,
                closed: LobbyClosed::None,
                room,
                ..LobbyState::default()
            };
            for seat in &mut state.seats {
                let flags = reader.u8()?;
                if flags & !0x0F != 0 {
                    return None;
                }
                *seat = SeatState {
                    taken: flags & 1 != 0,
                    connected: flags & 2 != 0,
                    ready: flags & 4 != 0,
                    left: flags & 8 != 0,
                    color: u32::from(reader.u8()?),
                    name: read_str(&mut reader, MAX_NAME_BYTES)?,
                };
            }
            Down::Lobby(Box::new(state))
        }
        kind::ROLE => {
            let round = reader.varint_u32()?;
            let role = reader.varint_u32()?;
            let name = read_str(&mut reader, MAX_ROLE_NAME)?;
            Down::Role { round, role, name }
        }
        kind::MESSAGE_DOWN => {
            let ty = u16::try_from(reader.varint()?).ok()?;
            let tick = reader.varint()?;
            let at = bytes.len() - reader.remaining();
            return Some(Down::Message { ty, tick, at });
        }
        kind::REFUSED => Down::Refused(Refusal::from_code(reader.u8()?)?),
        kind::CLOSING => Down::Closing,
        _ => return None,
    };
    reader.done().then_some(down)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use wish_core::Rng;

    #[test]
    fn every_message_round_trips() {
        let ups = [
            Up::Join(Join {
                version: VERSION,
                token: Some([7; TOKEN_LEN]),
                name: "Ada".into(),
                color: 3,
            }),
            Up::Join(Join {
                version: VERSION,
                token: None,
                name: String::new(),
                color: 0,
            }),
            Up::Profile {
                name: "Şule".into(),
                color: 9,
            },
            Up::Ready(true),
            Up::Ready(false),
            Up::Leave,
        ];
        for up in &ups {
            assert_eq!(decode_up(&encode_up(up)).as_ref(), Some(up));
        }
        let mut state = LobbyState {
            phase: LobbyPhase::Running,
            round: 3,
            outcome: 2,
            room: "STAR42".into(),
            ..LobbyState::default()
        };
        state.seats[2] = SeatState {
            taken: true,
            connected: true,
            ready: true,
            left: false,
            color: 5,
            name: "Bob".into(),
        };
        let downs = [
            Down::Admitted {
                version: VERSION,
                seat: 4,
                token: [9; TOKEN_LEN],
                types: vec![TypeName {
                    name: "Vote".into(),
                    fingerprint: [1; 16],
                }],
            },
            Down::Lobby(Box::new(state)),
            Down::Role {
                round: 1,
                role: 1,
                name: "saboteur".into(),
            },
            Down::Refused(Refusal::Running),
            Down::Closing,
        ];
        for down in &downs {
            assert_eq!(decode_down(&encode_down(down)).as_ref(), Some(down));
        }
        let message = encode_up(&Up::Message { ty: 5, at: 0 });
        assert_eq!(decode_up(&message), Some(Up::Message { ty: 5, at: 2 }));
        let message = encode_down(&Down::Message {
            ty: 2,
            tick: 300,
            at: 0,
        });
        assert_eq!(
            decode_down(&message),
            Some(Down::Message {
                ty: 2,
                tick: 300,
                at: 4
            })
        );
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut rng = Rng::from_seed(12);
        for round in 0..20_000u32 {
            let len = rng.below(200) as usize;
            let mut bytes: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = kind::FIRST + (round % 32) as u8;
            }
            let _ = decode_up(&bytes);
            let _ = decode_down(&bytes);
        }
    }

    #[test]
    fn kinds_of_the_other_direction_are_refused() {
        assert!(decode_up(&encode_down(&Down::Closing)).is_none());
        assert!(decode_down(&encode_up(&Up::Leave)).is_none());
        assert!(decode_up(&[kind::READY, 2]).is_none());
        assert!(decode_up(&[kind::LEAVE, 0]).is_none(), "trailing bytes");
    }
}
