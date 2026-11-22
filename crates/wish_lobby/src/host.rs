//! The host's session: the lobby, the messages and the match log, around
//! the replication host (design note §3).

use std::collections::{BTreeMap, VecDeque};

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
use wish_net::crypto::same_bytes;
use wish_net::{Channel, Endpoint, Entropy, Event, PeerId, Transport};
use wish_replicate::input::Actions;
use wish_replicate::wire::Reader;
use wish_replicate::{Host, ReplicationConfig};
use wish_types::{
    LobbyAction, LobbyClosed, LobbyPhase, LobbyRequest, MAX_PLAYERS, NetGroups, NetTo, PlayerId,
};

use crate::lobby::{
    LobbyConfig, LobbyState, LocalState, RoleConfig, SeatState, clean_name, deal_roles, free_color,
    unique_name,
};
use crate::message::{self, MessageType, MessageValue, message_types};
use crate::record::{Carry, Delivered, MatchLog, TickRecord, apply, player_count};
use crate::wire::{self, Down, Join, Refusal, TOKEN_LEN, TypeName, Up};

/// Diagnostics a session keeps before dropping the oldest.
pub const MAX_DIAGNOSTICS: usize = 256;

/// Messages delivered at one boundary at most (the rest wait, in order).
pub const MAX_DELIVERED: usize = 1024;

/// Ticks a refused peer is kept so that it hears why.
const REFUSE_FLUSH_TICKS: u64 = 60;

/// Thousandths of a message: the token bucket's unit.
const MILLI: u64 = 1000;

/// What a session did, counted since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    /// Players who took a seat.
    pub joins: u64,
    /// Players who came back into their seat.
    pub rejoins: u64,
    /// Peers refused (full, running, a late rejoin).
    pub refusals: u64,
    /// Game messages received from players and accepted.
    pub messages_received: u64,
    /// Game messages delivered into this world.
    pub messages_delivered: u64,
    /// Game messages sent to players (one per receiver).
    pub messages_sent: u64,
    /// Messages dropped because their sender sent too many.
    pub dropped_flood: u64,
    /// Messages dropped because they were too large.
    pub dropped_oversized: u64,
    /// Messages dropped as malformed, out of order or not allowed.
    pub dropped_invalid: u64,
    /// Messages to a player who is not connected.
    pub dropped_offline: u64,
    /// Diagnostics dropped because the list was full.
    pub diagnostics_dropped: u64,
}

/// One seat as the `net` tool shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatStatus {
    /// The player id.
    pub player: PlayerId,
    /// The peer holding it now, if connected.
    pub peer: Option<PeerId>,
    /// The seat.
    pub seat: SeatState,
}

/// The host session as the `net` tool shows it (package 14).
#[derive(Debug, Clone, PartialEq)]
pub struct HostStatus {
    /// Where the lobby is.
    pub phase: LobbyPhase,
    /// The match number.
    pub round: u32,
    /// The tick of the last boundary.
    pub tick: u64,
    /// The taken seats.
    pub seats: Vec<SeatStatus>,
    /// Peers connected that have not joined yet.
    pub pending: usize,
    /// The counts.
    pub stats: SessionStats,
}

/// A request to the lobby, from a seat.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    Ready(bool),
    Name(String),
    Color(u32),
    Profile(String, u32),
    Leave,
    Start,
    End(u32),
    Reopen,
    Close,
}

/// A token bucket in thousandths of a message.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u64,
}

/// A connected peer.
#[derive(Debug)]
struct PeerLink {
    /// Its seat, once it joined.
    seat: Option<PlayerId>,
    connected_at: u64,
    bucket: Bucket,
    /// Refused: disconnect at this tick.
    refused_until: Option<u64>,
}

/// What the host keeps of a seat beyond the lobby's public data.
#[derive(Debug, Clone)]
struct SeatLink {
    peer: Option<PeerId>,
    token: [u8; TOKEN_LEN],
    dropped_at: Option<u64>,
}

/// A message a player sent, accepted at receipt, waiting for the boundary.
#[derive(Debug, Clone)]
struct Incoming {
    from: PlayerId,
    message: Delivered,
}

/// The host of an online session.
#[derive(Debug)]
pub struct HostSession {
    config: LobbyConfig,
    roles: Vec<RoleConfig>,
    replication: Host,
    types: Vec<MessageType>,
    entropy: Entropy,
    /// The lobby as decided; written into the world at the next boundary.
    lobby: LobbyState,
    applied_lobby: Option<LobbyState>,
    sent_lobby: Option<LobbyState>,
    local: LocalState,
    applied_local: Option<LocalState>,
    match_roles: [u32; MAX_PLAYERS],
    applied_roles: Option<[u32; MAX_PLAYERS]>,
    seats: BTreeMap<PlayerId, SeatLink>,
    peers: BTreeMap<PeerId, PeerLink>,
    joins: Vec<(PeerId, Join)>,
    requests: Vec<(PlayerId, Request)>,
    inbox: VecDeque<Incoming>,
    local_outbox: VecDeque<Delivered>,
    /// Peers that must hear their admission (and the lobby and their role).
    to_admit: Vec<PeerId>,
    /// Players whose role must be sent.
    to_role: Vec<PlayerId>,
    carry: Carry,
    log: MatchLog,
    tick: u64,
    closing: Option<u64>,
    closed: bool,
    diagnostics: VecDeque<Diagnostic>,
    stats: SessionStats,
}

impl HostSession {
    /// A host session of `world` (its message types are those registered
    /// now), replicating `replication`'s components, with the room code of
    /// `room`. `entropy` gives the rejoin tokens.
    pub fn new(
        world: &World,
        config: LobbyConfig,
        replication: ReplicationConfig,
        room: &str,
        entropy: Entropy,
    ) -> Result<Self> {
        config.validate()?;
        let replication = Host::new(world, replication)?;
        let (types, problems) = message_types(world);
        let mut lobby = LobbyState {
            phase: LobbyPhase::Open,
            room: room.to_owned(),
            ..LobbyState::default()
        };
        if config.host_plays
            && let Some(seat) = lobby.seat_mut(0)
        {
            seat.taken = true;
            seat.connected = true;
            seat.name = clean_name(&config.host_name, 0);
            seat.color = config.host_color % config.colors;
        }
        let local = LocalState {
            player: 0,
            joined: config.host_plays,
            host: true,
            role: 0,
            role_name: String::new(),
        };
        let mut session = Self {
            roles: config.role_list(),
            config,
            replication,
            types,
            entropy,
            lobby,
            applied_lobby: None,
            sent_lobby: None,
            local,
            applied_local: None,
            match_roles: [0; MAX_PLAYERS],
            applied_roles: None,
            seats: BTreeMap::new(),
            peers: BTreeMap::new(),
            joins: Vec::new(),
            requests: Vec::new(),
            inbox: VecDeque::new(),
            local_outbox: VecDeque::new(),
            to_admit: Vec::new(),
            to_role: Vec::new(),
            carry: Carry::new(),
            log: MatchLog::new(),
            tick: 0,
            closing: None,
            closed: false,
            diagnostics: VecDeque::new(),
            stats: SessionStats::default(),
        };
        for problem in problems {
            session.note(problem);
        }
        Ok(session)
    }

    /// The replication host (its statistics, targets and network ids).
    pub fn replication(&self) -> &Host {
        &self.replication
    }

    /// The network message types, in the order messages number them.
    pub fn message_types(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }

    /// The lobby as decided (written into the world at the next boundary).
    pub fn lobby(&self) -> &LobbyState {
        &self.lobby
    }

    /// Every player's role in the running match.
    pub fn roles(&self) -> &[u32; MAX_PLAYERS] {
        &self.match_roles
    }

    /// The match log so far.
    pub fn log(&self) -> &MatchLog {
        &self.log
    }

    /// What the session did so far.
    pub fn stats(&self) -> SessionStats {
        self.stats
    }

    /// Whether the session closed and its endpoint was closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The seat of `peer`.
    pub fn seat_of(&self, peer: PeerId) -> Option<PlayerId> {
        self.peers.get(&peer).and_then(|p| p.seat)
    }

    /// The peer holding seat `player` now.
    pub fn peer_of(&self, player: PlayerId) -> Option<PeerId> {
        self.seats.get(&player).and_then(|s| s.peer)
    }

    /// The diagnostics since the last call (at most [`MAX_DIAGNOSTICS`]).
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        let mut out: Vec<Diagnostic> = self.diagnostics.drain(..).collect();
        out.extend(self.replication.take_diagnostics());
        out
    }

    /// The session as the `net` tool shows it.
    pub fn status(&self) -> HostStatus {
        HostStatus {
            phase: self.lobby.phase,
            round: self.lobby.round,
            tick: self.tick,
            seats: (0..MAX_PLAYERS as PlayerId)
                .filter_map(|p| {
                    let seat = self.lobby.seat(p)?;
                    seat.taken.then(|| SeatStatus {
                        player: p,
                        peer: self.peer_of(p),
                        seat: seat.clone(),
                    })
                })
                .collect(),
            pending: self.peers.values().filter(|p| p.seat.is_none()).count(),
            stats: self.stats,
        }
    }

    fn note(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
            self.stats.diagnostics_dropped += 1;
        }
        self.diagnostics.push_back(diagnostic);
    }

    // -----------------------------------------------------------------
    // The host's own requests (the lobby screen of the host's machine)
    // -----------------------------------------------------------------

    /// Asks to start the match at the next boundary. Refused now (and then)
    /// when the lobby is not open or not everyone is ready.
    pub fn start(&mut self) -> Result<()> {
        self.can_start()?;
        self.requests.push((0, Request::Start));
        Ok(())
    }

    /// Ends the running match at the next boundary with `outcome` (a number
    /// the game chooses, such as the winning side).
    pub fn end_match(&mut self, outcome: u32) {
        self.requests.push((0, Request::End(outcome)));
    }

    /// Returns to the lobby after a match, at the next boundary.
    pub fn reopen(&mut self) {
        self.requests.push((0, Request::Reopen));
    }

    /// The host's own player is ready or not.
    pub fn set_ready(&mut self, ready: bool) {
        self.requests.push((0, Request::Ready(ready)));
    }

    /// The host's own player's name and colour.
    pub fn set_profile(&mut self, name: &str, color: u32) {
        self.requests
            .push((0, Request::Profile(name.to_owned(), color)));
    }

    /// Closes the session at the next boundary: every player hears it, the
    /// match ends for everyone, and the endpoint closes once the news is
    /// sent (or `close_flush_ticks` passed).
    pub fn close(&mut self) {
        self.requests.push((0, Request::Close));
    }

    fn can_start(&self) -> Result<()> {
        if self.lobby.phase != LobbyPhase::Open {
            return Err(Diagnostic::warning(
                "lobby_wrong_phase",
                format!(
                    "the match cannot start: the lobby is {:?}",
                    self.lobby.phase
                ),
                "Start from the open lobby; after a match, reopen the lobby first.",
            ));
        }
        let present = self.lobby.present();
        if (present.len() as u32) < self.config.min_players {
            return Err(Diagnostic::warning(
                "lobby_not_ready",
                format!(
                    "the match needs {} players and {} are here",
                    self.config.min_players,
                    present.len()
                ),
                "Wait for more players to join (share the room code), or lower min_players.",
            ));
        }
        let waiting: Vec<String> = present
            .iter()
            .filter(|p| **p != 0)
            .filter_map(|p| self.lobby.seat(*p))
            .filter(|s| !s.ready)
            .map(|s| s.name.clone())
            .collect();
        if !waiting.is_empty() {
            return Err(Diagnostic::warning(
                "lobby_not_ready",
                format!("not everyone is ready: {}", waiting.join(", ")),
                "Wait until every player marks ready; a player who is away can be asked to leave.",
            )
            .with_candidates(waiting));
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // The tick boundary
    // -----------------------------------------------------------------

    /// The boundary before tick `tick`: takes the endpoint's events (call
    /// `endpoint.update` first), decides joins and lobby requests, takes the
    /// players' inputs for the tick and the messages that arrived, writes
    /// all of it into `world` (with the host's own player's `actions`) as one
    /// [`TickRecord`], records it, and tells the players what changed.
    /// Returns the mark to pass to [`end_tick`](Self::end_tick).
    pub fn begin_tick<T: Transport>(
        &mut self,
        world: &mut World,
        endpoint: &mut Endpoint<T>,
        tick: u64,
        actions: &Actions,
    ) -> Result<ChangeTick> {
        self.tick = tick;
        self.refill();
        while let Some(event) = endpoint.poll_event() {
            self.event(world, endpoint, event);
        }
        self.expire(endpoint);
        let mut joins = std::mem::take(&mut self.joins);
        joins.sort_by_key(|(peer, _)| *peer);
        for (peer, join) in joins {
            self.join(endpoint, peer, join);
        }
        let mut requests = std::mem::take(&mut self.requests);
        requests.sort_by_key(|(player, _)| *player);
        for (player, request) in requests {
            self.decide(endpoint, player, request);
        }

        let remote = self.replication.take_inputs(tick);
        let mut record = TickRecord::empty(tick);
        let highest = self
            .lobby
            .seats
            .iter()
            .rposition(|s| s.taken)
            .map_or(1, |p| p as u32 + 1);
        if highest > player_count(world) {
            record.players = Some(highest);
        }
        if *actions != self.carry.local {
            record.local = Some(actions.clone());
        }
        if remote != self.carry.remote {
            record.remote = Some(remote);
        }
        if self.applied_lobby.as_ref() != Some(&self.lobby) {
            record.lobby = Some(self.lobby.clone());
        }
        if self.applied_local.as_ref() != Some(&self.local) {
            record.local_player = Some(self.local.clone());
        }
        if self.applied_roles.as_ref() != Some(&self.match_roles)
            && (self.applied_roles.is_some() || self.match_roles != [0; MAX_PLAYERS])
        {
            record.roles = Some(self.match_roles);
        }
        let mut messages: Vec<Delivered> = Vec::new();
        while messages.len() < MAX_DELIVERED {
            let Some(message) = self.local_outbox.pop_front() else {
                break;
            };
            messages.push(message);
        }
        // Players' messages: by sender, then in the order each sent them.
        let mut incoming: Vec<Incoming> = self.inbox.drain(..).collect();
        incoming.sort_by_key(|m| m.from);
        let room = MAX_DELIVERED.saturating_sub(messages.len());
        let wait: Vec<Incoming> = if incoming.len() > room {
            incoming.split_off(room)
        } else {
            Vec::new()
        };
        messages.extend(incoming.into_iter().map(|m| m.message));
        self.inbox.extend(wait);
        self.stats.messages_delivered += messages.len() as u64;
        record.messages = messages;

        let (mark, problems) = apply(world, &record, &mut self.carry)?;
        for problem in problems {
            self.stats.dropped_invalid += 1;
            self.note(problem);
        }
        if record.lobby.is_some() {
            self.applied_lobby = Some(self.lobby.clone());
        }
        if record.local_player.is_some() {
            self.applied_local = Some(self.local.clone());
        }
        if record.roles.is_some() {
            self.applied_roles = Some(self.match_roles);
        }
        self.log.push(record);
        self.tell(endpoint);
        Ok(mark)
    }

    /// The end of tick `tick` (after the game's systems): sends the game's
    /// messages and takes its lobby requests (both sent since `mark`), sends
    /// every player its replication state, and closes the endpoint when the
    /// session is closing and the news went out.
    pub fn end_tick<T: Transport>(
        &mut self,
        world: &mut World,
        endpoint: &mut Endpoint<T>,
        mark: ChangeTick,
    ) -> Result<()> {
        let tick = self.tick;
        self.outgoing(world, endpoint, mark)?;
        for request in world.events_since::<LobbyRequest>(mark)? {
            let text = world.text(request.text).unwrap_or_default().to_owned();
            let request = match request.action {
                LobbyAction::Ready => Request::Ready(true),
                LobbyAction::NotReady => Request::Ready(false),
                LobbyAction::Name => Request::Name(text),
                LobbyAction::Color => Request::Color(request.value),
                LobbyAction::Start => Request::Start,
                LobbyAction::End => Request::End(request.value),
                LobbyAction::Reopen => Request::Reopen,
                LobbyAction::Leave => Request::Close,
            };
            self.requests.push((0, request));
        }
        if !self.closed {
            self.replication.send(world, tick, endpoint)?;
        }
        for peer in self.refused_due() {
            let _ = endpoint.disconnect(peer);
            self.peers.remove(&peer);
        }
        if let Some(since) = self.closing
            && !self.closed
        {
            let flushed = endpoint
                .peers()
                .iter()
                .all(|p| endpoint.queued_bytes(*p) == 0);
            if flushed || tick >= since + self.config.close_flush_ticks {
                endpoint.close();
                self.closed = true;
            }
        }
        Ok(())
    }

    fn refused_due(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|(_, p)| p.refused_until.is_some_and(|t| self.tick >= t))
            .map(|(peer, _)| *peer)
            .collect()
    }

    fn refill(&mut self) {
        let gain = u64::from(self.config.messages_per_second) * MILLI
            / u64::from(self.config.tick_rate_hz);
        let cap = u64::from(self.config.message_burst) * MILLI;
        for peer in self.peers.values_mut() {
            peer.bucket.tokens = (peer.bucket.tokens + gain.max(1)).min(cap);
        }
    }

    fn event<T: Transport>(&mut self, world: &mut World, endpoint: &mut Endpoint<T>, event: Event) {
        match event {
            Event::Connected { peer } => {
                if self.closing.is_some() {
                    let _ = endpoint.disconnect(peer);
                    return;
                }
                self.peers.insert(
                    peer,
                    PeerLink {
                        seat: None,
                        connected_at: self.tick,
                        bucket: Bucket {
                            tokens: u64::from(self.config.message_burst) * MILLI,
                        },
                        refused_until: None,
                    },
                );
            }
            Event::Disconnected { peer, .. } => {
                let seat = self.peers.remove(&peer).and_then(|p| p.seat);
                self.replication.release(peer);
                if let Some(seat) = seat {
                    self.dropped(seat, peer, false);
                }
            }
            Event::Message {
                peer,
                data,
                channel,
            } => {
                if wish_replicate::wire::is_replication(&data) {
                    if self.seat_of(peer).is_some() {
                        let _ = self.replication.handle_event(
                            world,
                            Event::Message {
                                peer,
                                channel,
                                data,
                            },
                        );
                    }
                    return;
                }
                self.receive(world, endpoint, peer, &data);
            }
        }
    }

    /// Seat `seat`'s peer `peer` is gone (`left`: on purpose).
    fn dropped(&mut self, seat: PlayerId, peer: PeerId, left: bool) {
        let Some(link) = self.seats.get_mut(&seat) else {
            return;
        };
        if link.peer != Some(peer) {
            return;
        }
        link.peer = None;
        let phase = self.lobby.phase;
        let Some(state) = self.lobby.seat_mut(seat) else {
            return;
        };
        state.connected = false;
        match phase {
            LobbyPhase::Running | LobbyPhase::Ended if !left => {
                link.dropped_at = Some(self.tick);
            }
            LobbyPhase::Running | LobbyPhase::Ended => {
                state.left = true;
                state.ready = false;
                self.seats.remove(&seat);
            }
            _ => {
                *state = SeatState::default();
                self.seats.remove(&seat);
            }
        }
    }

    /// Ends joins that took too long and seats whose grace ran out.
    fn expire<T: Transport>(&mut self, endpoint: &mut Endpoint<T>) {
        let late: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                p.seat.is_none()
                    && p.refused_until.is_none()
                    && self.tick >= p.connected_at + self.config.join_timeout_ticks
            })
            .map(|(peer, _)| *peer)
            .collect();
        for peer in late {
            let _ = endpoint.disconnect(peer);
            self.peers.remove(&peer);
            self.note(
                Diagnostic::warning(
                    "lobby_join_timeout",
                    format!("{peer} connected but did not join within {} ticks; it was disconnected", self.config.join_timeout_ticks),
                    "A client sends its join as soon as it connects; one that does not is broken, of another version, or hostile.",
                )
                .at_path(format!("/peers/{}", peer.0)),
            );
        }
        let grace = self.config.rejoin_grace_ticks;
        let expired: Vec<PlayerId> = self
            .seats
            .iter()
            .filter(|(_, s)| {
                s.peer.is_none() && s.dropped_at.is_some_and(|t| self.tick >= t + grace)
            })
            .map(|(p, _)| *p)
            .collect();
        for player in expired {
            self.seats.remove(&player);
            if let Some(seat) = self.lobby.seat_mut(player) {
                seat.left = true;
                seat.ready = false;
            }
            self.note(
                Diagnostic::info(
                    "lobby_rejoin_expired",
                    format!("player {player} did not come back within {grace} ticks; the seat is left"),
                    "The player can join the next match from the lobby; the game sees the seat as left.",
                )
                .at_path(format!("/players/{player}")),
            );
        }
    }

    fn take_token(peer: &mut PeerLink) -> bool {
        if peer.bucket.tokens < MILLI {
            return false;
        }
        peer.bucket.tokens -= MILLI;
        true
    }

    fn invalid(&mut self, peer: PeerId, reason: &str) {
        self.stats.dropped_invalid += 1;
        self.note(
            Diagnostic::warning(
                "net_message_invalid",
                format!("{peer} sent a lobby message that is not valid: {reason}; it was dropped"),
                "The other program is broken, of another version, or hostile. Nothing was applied; if both run the same release, report it.",
            )
            .at_path(format!("/peers/{}", peer.0)),
        );
    }

    fn receive<T: Transport>(
        &mut self,
        world: &World,
        endpoint: &mut Endpoint<T>,
        peer: PeerId,
        data: &[u8],
    ) {
        let _ = endpoint;
        let limit = self.config.message_bytes + 16;
        let Some(link) = self.peers.get_mut(&peer) else {
            return;
        };
        if link.refused_until.is_some() {
            return;
        }
        if !Self::take_token(link) {
            self.stats.dropped_flood += 1;
            let seat = link.seat;
            self.note(
                Diagnostic::warning(
                    "net_message_flood",
                    format!(
                        "{peer} (player {seat:?}) sent more than {} messages a second; the message was dropped",
                        self.config.messages_per_second
                    ),
                    "A game sends a few messages a second (chat, votes, actions); send state that changes every tick through replication instead.",
                )
                .at_path(format!("/peers/{}", peer.0)),
            );
            return;
        }
        let seat = link.seat;
        if data.len() > limit {
            self.stats.dropped_oversized += 1;
            self.note(
                Diagnostic::warning(
                    "net_message_oversized",
                    format!("{peer} sent a message of {} bytes, more than the {} allowed; it was dropped", data.len(), self.config.message_bytes),
                    "Keep a message small (an id, a choice, a short text); raise message_bytes only if a game really needs it.",
                )
                .at_path(format!("/peers/{}", peer.0)),
            );
            return;
        }
        if !wire::is_lobby(data) {
            self.invalid(peer, "its kind is not a lobby kind");
            return;
        }
        let Some(up) = wire::decode_up(data) else {
            self.invalid(peer, "it does not parse, or is a host's kind");
            return;
        };
        let Some(seat) = seat else {
            match up {
                Up::Join(join) => {
                    if self.joins.iter().any(|(p, _)| *p == peer) {
                        self.invalid(peer, "it joined twice");
                    } else {
                        self.joins.push((peer, join));
                    }
                }
                _ => self.invalid(peer, "it was sent before joining"),
            }
            return;
        };
        match up {
            Up::Join(_) => self.invalid(peer, "the player has a seat already"),
            Up::Profile { name, color } => {
                self.requests
                    .push((seat, Request::Profile(name, u32::from(color))));
            }
            Up::Ready(ready) => self.requests.push((seat, Request::Ready(ready))),
            Up::Leave => self.requests.push((seat, Request::Leave)),
            Up::Message { ty, at } => {
                let Some(kind) = self.types.get(usize::from(ty)) else {
                    self.invalid(peer, "it names an unknown message type");
                    return;
                };
                let mut reader = Reader::new(data.get(at..).unwrap_or_default());
                let Some(value) = message::read_value(&mut reader, &kind.layout) else {
                    self.invalid(peer, "its value does not fit its type");
                    return;
                };
                if !reader.done() {
                    self.invalid(peer, "it has bytes after its value");
                    return;
                }
                let replication = &self.replication;
                let mut value = MessageValue::from_wire(value, &|net| {
                    replication.resolve(seat, net).unwrap_or(Entity::NULL)
                });
                kind.set_envelope(&mut value.bytes, &message::from_player(seat));
                if !message::valid(world, kind.id, &value.bytes) {
                    self.invalid(peer, "its value holds invalid bytes");
                    return;
                }
                self.stats.messages_received += 1;
                let name = kind.name.clone();
                self.inbox.push_back(Incoming {
                    from: seat,
                    message: Delivered {
                        ty: name,
                        from: seat,
                        value,
                    },
                });
            }
        }
    }

    fn refuse<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, peer: PeerId, why: Refusal) {
        self.stats.refusals += 1;
        let _ = endpoint.send(
            peer,
            Channel::Reliable,
            &wire::encode_down(&Down::Refused(why)),
        );
        if let Some(link) = self.peers.get_mut(&peer) {
            link.refused_until = Some(self.tick + REFUSE_FLUSH_TICKS);
        }
        let (code, message, hint) = match why {
            Refusal::Full => (
                "lobby_full",
                "every seat is taken",
                "Wait for a player to leave; a match has at most 16 players.",
            ),
            Refusal::Running => (
                "lobby_match_running",
                "a match is running",
                "Join when the host returns to the lobby; players who dropped come back with their rejoin token.",
            ),
            Refusal::RejoinExpired => (
                "lobby_rejoin_expired",
                "the rejoin came after the grace period, or its token is unknown",
                "Join the next match from the lobby.",
            ),
            Refusal::Version => (
                "lobby_refused",
                "the player speaks another version of the lobby",
                "Host and players must run the same release of the game.",
            ),
        };
        self.note(
            Diagnostic::info(
                code,
                format!("{peer} was refused: {message}"),
                hint.to_owned(),
            )
            .at_path(format!("/peers/{}", peer.0)),
        );
    }

    fn join<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, peer: PeerId, join: Join) {
        if !self.peers.contains_key(&peer) || self.closing.is_some() {
            return;
        }
        if join.version != wire::VERSION {
            self.refuse(endpoint, peer, Refusal::Version);
            return;
        }
        if let Some(token) = join.token {
            let back = self
                .seats
                .iter()
                .find(|(_, s)| same_bytes(&s.token, &token))
                .map(|(p, s)| (*p, s.peer));
            if let Some((seat, old)) = back {
                if let Some(old) = old {
                    // The old connection is still open (a restarted client):
                    // the new one takes over.
                    let _ = endpoint.disconnect(old);
                    self.peers.remove(&old);
                    self.replication.release(old);
                }
                if let Some(link) = self.seats.get_mut(&seat) {
                    link.peer = Some(peer);
                    link.dropped_at = None;
                }
                if let Some(state) = self.lobby.seat_mut(seat) {
                    state.connected = true;
                }
                self.seat(peer, seat);
                self.stats.rejoins += 1;
                return;
            }
            if self.lobby.phase != LobbyPhase::Open {
                self.refuse(endpoint, peer, Refusal::RejoinExpired);
                return;
            }
        }
        if self.lobby.phase != LobbyPhase::Open {
            self.refuse(endpoint, peer, Refusal::Running);
            return;
        }
        let Some(seat) =
            (1..MAX_PLAYERS as PlayerId).find(|p| self.lobby.seat(*p).is_some_and(|s| !s.taken))
        else {
            self.refuse(endpoint, peer, Refusal::Full);
            return;
        };
        let name = unique_name(&clean_name(&join.name, seat), seat, &self.lobby.seats);
        let color = free_color(
            u32::from(join.color),
            seat,
            &self.lobby.seats,
            self.config.colors,
        );
        if let Some(state) = self.lobby.seat_mut(seat) {
            *state = SeatState {
                taken: true,
                connected: true,
                ready: false,
                left: false,
                color,
                name,
            };
        }
        let token: [u8; TOKEN_LEN] = self.entropy.bytes();
        self.seats.insert(
            seat,
            SeatLink {
                peer: Some(peer),
                token,
                dropped_at: None,
            },
        );
        self.seat(peer, seat);
        self.stats.joins += 1;
    }

    /// Gives `peer` seat `seat`: replication and the news at the boundary.
    fn seat(&mut self, peer: PeerId, seat: PlayerId) {
        if let Some(link) = self.peers.get_mut(&peer) {
            link.seat = Some(seat);
        }
        if let Err(problem) = self.replication.admit(peer, seat) {
            self.note(problem);
        }
        self.to_admit.push(peer);
        if matches!(self.lobby.phase, LobbyPhase::Running | LobbyPhase::Ended) {
            self.to_role.push(seat);
        }
    }

    fn wrong_phase(&mut self, player: PlayerId, what: &str) {
        let phase = self.lobby.phase;
        self.note(
            Diagnostic::warning(
                "lobby_wrong_phase",
                format!("player {player} asked to {what}, but the lobby is {phase:?}; nothing changed"),
                "Names, colours and the ready flag change in the open lobby; a match starts from it, ends while running, and the lobby reopens after it ends.",
            )
            .at_path(format!("/players/{player}")),
        );
    }

    fn decide<T: Transport>(
        &mut self,
        endpoint: &mut Endpoint<T>,
        player: PlayerId,
        request: Request,
    ) {
        if self.closing.is_some() {
            return;
        }
        let open = self.lobby.phase == LobbyPhase::Open;
        let seated = self.lobby.seat(player).is_some_and(|s| s.taken);
        match request {
            Request::Ready(ready) => {
                if !open || !seated {
                    return self.wrong_phase(player, "change the ready flag");
                }
                if let Some(seat) = self.lobby.seat_mut(player) {
                    seat.ready = ready;
                }
            }
            Request::Name(name) => {
                let color = self.lobby.seat(player).map_or(0, |s| s.color);
                self.decide(endpoint, player, Request::Profile(name, color));
            }
            Request::Color(color) => {
                let name = self
                    .lobby
                    .seat(player)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                self.decide(endpoint, player, Request::Profile(name, color));
            }
            Request::Profile(name, color) => {
                if !open || !seated {
                    return self.wrong_phase(player, "change name or colour");
                }
                let name = unique_name(&clean_name(&name, player), player, &self.lobby.seats);
                let color = free_color(color, player, &self.lobby.seats, self.config.colors);
                if let Some(seat) = self.lobby.seat_mut(player) {
                    seat.name = name;
                    seat.color = color;
                }
            }
            Request::Leave => {
                if player == 0 {
                    return self.decide(endpoint, 0, Request::Close);
                }
                if let Some(peer) = self.peer_of(player) {
                    let _ = endpoint.disconnect(peer);
                    self.peers.remove(&peer);
                    self.replication.release(peer);
                    self.dropped(player, peer, true);
                }
            }
            Request::Start => {
                if let Err(problem) = self.can_start() {
                    return self.note(problem);
                }
                let present = self.lobby.present();
                self.lobby.phase = LobbyPhase::Running;
                self.lobby.round += 1;
                self.lobby.outcome = 0;
                let dealt = deal_roles(self.config.seed, self.lobby.round, &present, &self.roles);
                self.match_roles = [0; MAX_PLAYERS];
                for (p, role) in &dealt {
                    if let Some(slot) = self.match_roles.get_mut(*p as usize) {
                        *slot = *role;
                    }
                }
                self.set_local_role();
                self.to_role.extend(present.iter().filter(|p| **p != 0));
            }
            Request::End(outcome) => {
                if self.lobby.phase != LobbyPhase::Running {
                    return self.wrong_phase(player, "end the match");
                }
                self.lobby.phase = LobbyPhase::Ended;
                self.lobby.outcome = outcome;
            }
            Request::Reopen => {
                if self.lobby.phase != LobbyPhase::Ended {
                    return self.wrong_phase(player, "reopen the lobby");
                }
                self.lobby.phase = LobbyPhase::Open;
                for (index, seat) in self.lobby.seats.iter_mut().enumerate() {
                    let gone = seat.left || (seat.taken && !seat.connected);
                    if gone {
                        *seat = SeatState::default();
                        self.seats.remove(&(index as PlayerId));
                    } else {
                        seat.ready = false;
                    }
                }
                self.match_roles = [0; MAX_PLAYERS];
                self.set_local_role();
            }
            Request::Close => {
                self.lobby.phase = LobbyPhase::Closed;
                self.lobby.closed = LobbyClosed::HostLeft;
                self.closing = Some(self.tick);
                let closing = wire::encode_down(&Down::Closing);
                for peer in endpoint.peers() {
                    let _ = endpoint.send(peer, Channel::Reliable, &closing);
                }
            }
        }
    }

    fn set_local_role(&mut self) {
        let role = self.match_roles[0];
        self.local.role = if self.config.host_plays { role } else { 0 };
        self.local.role_name = if self.local.role == 0 && self.lobby.phase != LobbyPhase::Running {
            String::new()
        } else {
            self.role_name(self.local.role)
        };
        if !self.config.host_plays {
            self.local.role_name = String::new();
        }
    }

    fn role_name(&self, role: u32) -> String {
        self.roles
            .get(role as usize)
            .map(|r| r.name.clone())
            .unwrap_or_default()
    }

    /// After the boundary: admissions, the lobby when it changed, roles.
    fn tell<T: Transport>(&mut self, endpoint: &mut Endpoint<T>) {
        let types: Vec<TypeName> = self
            .types
            .iter()
            .map(|t| TypeName {
                name: t.name.clone(),
                fingerprint: t.fingerprint,
            })
            .collect();
        let lobby_changed = self.sent_lobby.as_ref() != Some(&self.lobby);
        let lobby_bytes = wire::encode_down(&Down::Lobby(Box::new(self.lobby.clone())));
        let admitted = std::mem::take(&mut self.to_admit);
        for peer in &admitted {
            let Some(seat) = self.seat_of(*peer) else {
                continue;
            };
            let Some(token) = self.seats.get(&seat).map(|s| s.token) else {
                continue;
            };
            let bytes = wire::encode_down(&Down::Admitted {
                version: wire::VERSION,
                seat,
                token,
                types: types.clone(),
            });
            self.send(endpoint, *peer, &bytes);
            if !lobby_changed {
                self.send(endpoint, *peer, &lobby_bytes);
            }
        }
        if lobby_changed && self.closing.is_none() {
            let seated: Vec<PeerId> = self
                .peers
                .iter()
                .filter(|(_, p)| p.seat.is_some() && p.refused_until.is_none())
                .map(|(peer, _)| *peer)
                .collect();
            for peer in seated {
                self.send(endpoint, peer, &lobby_bytes);
            }
            self.sent_lobby = Some(self.lobby.clone());
        }
        let mut roles = std::mem::take(&mut self.to_role);
        roles.sort_unstable();
        roles.dedup();
        for player in roles {
            let Some(peer) = self.peer_of(player) else {
                continue;
            };
            let role = self.match_roles.get(player as usize).copied().unwrap_or(0);
            let bytes = wire::encode_down(&Down::Role {
                round: self.lobby.round,
                role,
                name: self.role_name(role),
            });
            self.send(endpoint, peer, &bytes);
        }
    }

    fn send<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, peer: PeerId, bytes: &[u8]) {
        if let Err(problem) = endpoint.send(peer, Channel::Reliable, bytes) {
            self.note(problem);
        }
    }

    /// The game's messages sent since `mark`: delivered locally at the next
    /// boundary, or sent to their players now.
    fn outgoing<T: Transport>(
        &mut self,
        world: &World,
        endpoint: &mut Endpoint<T>,
        mark: ChangeTick,
    ) -> Result<()> {
        let groups = world.resource::<NetGroups>().copied().unwrap_or_default();
        let present = self.lobby.present();
        let host_plays = self.config.host_plays;
        let types = self.types.clone();
        for (index, ty) in types.iter().enumerate() {
            for raw in world.event_bytes_since(ty.id, mark)? {
                let net = ty.envelope(&raw);
                if net.delivered {
                    continue;
                }
                let value = MessageValue::read(world, ty, &raw);
                let receivers: Vec<PlayerId> = match net.to {
                    NetTo::Host => vec![0],
                    NetTo::Player => vec![net.player],
                    NetTo::Everyone => present
                        .iter()
                        .copied()
                        .filter(|p| *p != 0 || host_plays)
                        .collect(),
                    NetTo::Group => present
                        .iter()
                        .copied()
                        .filter(|p| *p != 0 || host_plays)
                        .filter(|p| {
                            groups
                                .players
                                .get(*p as usize)
                                .is_some_and(|g| g & net.groups != 0)
                        })
                        .collect(),
                };
                for receiver in receivers {
                    if receiver == 0 {
                        let mut local = value.clone();
                        ty.set_envelope(&mut local.bytes, &message::delivered(&net, 0));
                        self.local_outbox.push_back(Delivered {
                            ty: ty.name.clone(),
                            from: 0,
                            value: local,
                        });
                        continue;
                    }
                    let Some(peer) = self.peer_of(receiver) else {
                        self.stats.dropped_offline += 1;
                        continue;
                    };
                    let replication = &self.replication;
                    let wire_value =
                        value.to_wire(&|e| replication.network_id(receiver, e).unwrap_or(0));
                    let mut bytes = wire::encode_down(&Down::Message {
                        ty: index as u16,
                        tick: self.tick,
                        at: 0,
                    });
                    message::put_value(&mut bytes, &wire_value, &ty.layout);
                    match endpoint.send(peer, Channel::Reliable, &bytes) {
                        Ok(()) => self.stats.messages_sent += 1,
                        Err(problem) => {
                            self.stats.dropped_oversized += 1;
                            self.note(problem);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
