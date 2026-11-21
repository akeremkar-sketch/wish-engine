//! Local discovery: the matches on the local network, without typing an
//! address.
//!
//! A host runs a [`DiscoveryResponder`] on the discovery port; a player's
//! [`DiscoveryQuery`] sends a query to the broadcast address (or to any
//! address) and collects the answers. Both packets are plain (they carry
//! nothing secret unless the host chooses to share its room code), bounded
//! and checked like every other packet.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use wish_core::{Diagnostic, Result};

use crate::crypto::{Entropy, PUBLIC_KEY_LEN};
use crate::endpoint::MAX_READS_PER_UPDATE;
use crate::room::RoomCode;
use crate::transport::{Recv, Transport};
use crate::wire::{self, AnnounceFields, MAX_GAME_ID, MAX_NAME, MAX_PACKET, Packet};

/// The UDP port discovery uses by default.
pub const DISCOVERY_PORT: u16 = 47_610;

/// Queries a responder answers per second at most.
pub const ANSWERS_PER_SECOND: u32 = 32;

/// The most matches a query keeps.
pub const MAX_FOUND: usize = 256;

/// The broadcast address on a port (every machine of the local network).
pub fn broadcast_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port)
}

/// What a host tells the local network about its match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announce {
    /// The game's id: only queries for the same game are answered (at most
    /// 64 bytes).
    pub game: String,
    /// The match's name, shown to players (at most 64 bytes).
    pub name: String,
    /// The UDP port the match is hosted on.
    pub port: u16,
    /// Players in the match.
    pub players: u16,
    /// Players the match takes.
    pub max_players: u16,
    /// The host's public key ([`crate::Endpoint::public_key`]); a client can
    /// pin it in [`crate::Config::host_key`].
    pub host_key: [u8; PUBLIC_KEY_LEN],
    /// The room code, only if the host shares it with everyone on the local
    /// network.
    pub room_code: Option<RoomCode>,
}

impl Announce {
    fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String| {
            Err(Diagnostic::error(
                "net_config_invalid",
                message,
                "Keep the game id and the match name to 64 bytes of UTF-8 each, and give the match's real port.",
            )
            .at_path(format!("/announce/{field}")))
        };
        if self.game.is_empty() || self.game.len() > MAX_GAME_ID {
            return bad("game", format!("the game id has {} bytes", self.game.len()));
        }
        if self.name.len() > MAX_NAME {
            return bad(
                "name",
                format!("the match name has {} bytes", self.name.len()),
            );
        }
        if self.port == 0 {
            return bad("port", "the port is 0".to_owned());
        }
        Ok(())
    }
}

/// Answers discovery queries for one match.
#[derive(Debug)]
pub struct DiscoveryResponder<T: Transport> {
    transport: T,
    announce: Announce,
    answered_milli: u64,
    last_ms: u64,
    buf: Vec<u8>,
}

impl<T: Transport> DiscoveryResponder<T> {
    /// A responder on `transport` (bound to the discovery port) telling
    /// `announce`.
    pub fn new(transport: T, announce: Announce) -> Result<Self> {
        announce.validate()?;
        Ok(Self {
            transport,
            announce,
            answered_milli: u64::from(ANSWERS_PER_SECOND) * 1000,
            last_ms: 0,
            buf: vec![0; MAX_PACKET + 1],
        })
    }

    /// Changes the number of players announced.
    pub fn set_players(&mut self, players: u16) {
        self.announce.players = players;
    }

    /// What is announced.
    pub fn announce(&self) -> &Announce {
        &self.announce
    }

    /// The address the responder listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.transport.local_addr()
    }

    /// Answers the queries that arrived. `now_ms` limits the rate of
    /// answers.
    pub fn update(&mut self, now_ms: u64) {
        let rate = u64::from(ANSWERS_PER_SECOND);
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = self.last_ms.max(now_ms);
        self.answered_milli = self
            .answered_milli
            .saturating_add(elapsed.saturating_mul(rate))
            .min(rate * 1000);
        for _ in 0..MAX_READS_PER_UPDATE {
            let (len, from) = match self.transport.recv(&mut self.buf) {
                Recv::Empty => break,
                Recv::Dropped => continue,
                Recv::Datagram { len, from } => (len, from),
            };
            if len > MAX_PACKET || len > self.buf.len() {
                continue;
            }
            let Some(Packet::Query { nonce, game }) = wire::decode(&self.buf[..len]) else {
                continue;
            };
            if game != self.announce.game || self.answered_milli < 1000 {
                continue;
            }
            self.answered_milli -= 1000;
            let room = self
                .announce
                .room_code
                .as_ref()
                .map(RoomCode::as_str)
                .unwrap_or_default();
            let packet = wire::announce(&AnnounceFields {
                nonce: &nonce,
                game: &self.announce.game,
                name: &self.announce.name,
                port: self.announce.port,
                players: self.announce.players,
                max_players: self.announce.max_players,
                host_key: &self.announce.host_key,
                room_code: room,
            });
            let _ = self.transport.send(from, &packet);
        }
    }
}

/// Looks for matches of one game.
#[derive(Debug)]
pub struct DiscoveryQuery<T: Transport> {
    transport: T,
    game: String,
    nonce: [u8; 16],
    found: BTreeMap<SocketAddr, Announce>,
    buf: Vec<u8>,
}

impl<T: Transport> DiscoveryQuery<T> {
    /// A query for `game` over `transport`. To send to the broadcast address
    /// over UDP, call [`crate::UdpTransport::enable_broadcast`] first.
    pub fn new(transport: T, game: &str, entropy: &mut Entropy) -> Result<Self> {
        if game.is_empty() || game.len() > MAX_GAME_ID {
            return Err(Diagnostic::error(
                "net_config_invalid",
                format!("the game id has {} bytes", game.len()),
                "Give a game id of 1 to 64 bytes of UTF-8 (the same one the host announces).",
            )
            .at_path("/game"));
        }
        Ok(Self {
            transport,
            game: game.to_owned(),
            nonce: entropy.bytes(),
            found: BTreeMap::new(),
            buf: vec![0; MAX_PACKET + 1],
        })
    }

    /// Sends the query to `to`: [`broadcast_addr`] for the whole local
    /// network, or one machine's discovery address.
    pub fn send(&mut self, to: SocketAddr) -> Result<()> {
        self.transport
            .send(to, &wire::query(&self.nonce, &self.game))
    }

    /// Reads the answers that arrived.
    pub fn update(&mut self) {
        for _ in 0..MAX_READS_PER_UPDATE {
            let (len, from) = match self.transport.recv(&mut self.buf) {
                Recv::Empty => break,
                Recv::Dropped => continue,
                Recv::Datagram { len, from } => (len, from),
            };
            if len > MAX_PACKET || len > self.buf.len() {
                continue;
            }
            let Some(Packet::Announce {
                nonce,
                game,
                name,
                port,
                players,
                max_players,
                host_key,
                room_code,
            }) = wire::decode(&self.buf[..len])
            else {
                continue;
            };
            if nonce != self.nonce || game != self.game || port == 0 {
                continue;
            }
            let room_code = if room_code.is_empty() {
                None
            } else {
                match RoomCode::parse(room_code) {
                    Ok(code) => Some(code),
                    Err(_) => continue,
                }
            };
            let addr = SocketAddr::new(from.ip(), port);
            if self.found.len() >= MAX_FOUND && !self.found.contains_key(&addr) {
                continue;
            }
            self.found.insert(
                addr,
                Announce {
                    game: game.to_owned(),
                    name: name.to_owned(),
                    port,
                    players,
                    max_players,
                    host_key,
                    room_code,
                },
            );
        }
    }

    /// The matches found so far: the address to join, and what the host
    /// announced. In address order.
    pub fn found(&self) -> impl Iterator<Item = (SocketAddr, &Announce)> {
        self.found.iter().map(|(addr, announce)| (*addr, announce))
    }
}
