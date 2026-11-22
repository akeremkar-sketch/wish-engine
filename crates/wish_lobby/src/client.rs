//! A player's session: joins the host's lobby, mirrors it into the world,
//! delivers the host's messages and sends the game's (design note §3.6).

use std::collections::{BTreeMap, VecDeque};

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
use wish_net::{Channel, Endpoint, Event, PeerId, Transport};
use wish_replicate::wire::Reader;
use wish_replicate::{Client, ClientConfig, Value};
use wish_types::{LobbyAction, LobbyClosed, LobbyPhase, LobbyRequest, NetTo, PlayerId};

use crate::host::{MAX_DELIVERED, MAX_DIAGNOSTICS, SessionStats};
use crate::lobby::{LobbyState, LocalState, MAX_NAME_BYTES};
use crate::message::{self, MessageType, MessageValue, message_types};
use crate::wire::{self, Down, Join, Refusal, TOKEN_LEN, Up};

/// Ticks of sent messages a client remembers (so that a tick its prediction
/// plays again does not send them twice).
const SENT_MEMORY: u64 = 256;

/// A player's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSessionConfig {
    /// The name asked for.
    pub name: String,
    /// The colour asked for.
    pub color: u32,
    /// The largest message the game may send (the host's `message_bytes`).
    pub message_bytes: usize,
    /// The replication client's settings.
    pub replication: ClientConfig,
}

impl Default for ClientSessionConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: 0,
            message_bytes: 1024,
            replication: ClientConfig::default(),
        }
    }
}

/// The client session as the `net` tool shows it (package 14).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientStatus {
    /// This player's seat, once admitted.
    pub seat: Option<PlayerId>,
    /// The lobby as last heard.
    pub lobby: LobbyState,
    /// Why the session closed, once it did.
    pub closed: Option<LobbyClosed>,
    /// The counts.
    pub stats: SessionStats,
}

/// A message from the host, waiting for the boundary.
#[derive(Debug, Clone)]
struct Pending {
    ty: usize,
    value: Value,
}

/// A player's online session.
#[derive(Debug)]
pub struct ClientSession {
    config: ClientSessionConfig,
    replication: Client,
    types: Vec<MessageType>,
    /// The host's type index → this world's.
    remote_types: Vec<Option<usize>>,
    /// This world's type index → the host's.
    host_index: BTreeMap<usize, u16>,
    seat: Option<PlayerId>,
    token: Option<[u8; TOKEN_LEN]>,
    lobby: LobbyState,
    lobby_dirty: bool,
    local: LocalState,
    local_dirty: bool,
    inbox: VecDeque<Pending>,
    sent: BTreeMap<u64, BTreeMap<usize, usize>>,
    closed: Option<LobbyClosed>,
    closing_heard: bool,
    diagnostics: VecDeque<Diagnostic>,
    stats: SessionStats,
}

impl ClientSession {
    /// A player's session for `world` (its message types are those
    /// registered now).
    pub fn new(world: &World, config: ClientSessionConfig) -> Self {
        let (types, problems) = message_types(world);
        let mut session = Self {
            replication: Client::new(config.replication.clone()),
            config,
            types,
            remote_types: Vec::new(),
            host_index: BTreeMap::new(),
            seat: None,
            token: None,
            // A player's machine is in an online session from its first
            // tick: until the host's lobby arrives it says the lobby is
            // open (joining), never `Offline`, which game code reads as a
            // game alone (friction F13).
            lobby: LobbyState {
                phase: LobbyPhase::Open,
                ..LobbyState::default()
            },
            lobby_dirty: true,
            local: LocalState::default(),
            local_dirty: false,
            inbox: VecDeque::new(),
            sent: BTreeMap::new(),
            closed: None,
            closing_heard: false,
            diagnostics: VecDeque::new(),
            stats: SessionStats::default(),
        };
        for problem in problems {
            session.note(problem);
        }
        session
    }

    /// The replication client (mirror, inputs, prediction).
    pub fn replication(&self) -> &Client {
        &self.replication
    }

    /// The replication client, to send inputs or predict.
    pub fn replication_mut(&mut self) -> &mut Client {
        &mut self.replication
    }

    /// This player's seat (its player id), once admitted.
    pub fn seat(&self) -> Option<PlayerId> {
        self.seat
    }

    /// The lobby as last heard from the host.
    pub fn lobby(&self) -> &LobbyState {
        &self.lobby
    }

    /// This player's role, as the host sent it.
    pub fn role(&self) -> u32 {
        self.local.role
    }

    /// Why the session closed, once it did.
    pub fn closed(&self) -> Option<LobbyClosed> {
        self.closed
    }

    /// What the session did so far.
    pub fn stats(&self) -> SessionStats {
        self.stats
    }

    /// The session as the `net` tool shows it.
    pub fn status(&self) -> ClientStatus {
        ClientStatus {
            seat: self.seat,
            lobby: self.lobby.clone(),
            closed: self.closed,
            stats: self.stats,
        }
    }

    /// The diagnostics since the last call.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        let mut out: Vec<Diagnostic> = self.diagnostics.drain(..).collect();
        out.extend(self.replication.take_diagnostics());
        out
    }

    fn note(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
            self.stats.diagnostics_dropped += 1;
        }
        self.diagnostics.push_back(diagnostic);
    }

    /// Prepares a rejoin on a new connection (the caller makes a new
    /// endpoint): the mirror is cleared (the host sends a fresh snapshot),
    /// the seat is forgotten until the host gives it back, the rejoin token
    /// and the profile are kept.
    pub fn rejoin(&mut self, world: &mut World) {
        self.replication.forget(world);
        self.seat = None;
        self.closed = None;
        self.closing_heard = false;
        self.inbox.clear();
        self.sent.clear();
        self.remote_types.clear();
        self.host_index.clear();
    }

    /// Asks the host to mark this player ready or not.
    pub fn set_ready<T: Transport>(
        &mut self,
        endpoint: &mut Endpoint<T>,
        ready: bool,
    ) -> Result<()> {
        self.up(endpoint, &Up::Ready(ready))
    }

    /// Asks the host for a name and a colour.
    pub fn set_profile<T: Transport>(
        &mut self,
        endpoint: &mut Endpoint<T>,
        name: &str,
        color: u32,
    ) -> Result<()> {
        self.config.name = name.to_owned();
        self.config.color = color;
        self.up(
            endpoint,
            &Up::Profile {
                name: name.to_owned(),
                color: u8::try_from(color).unwrap_or(u8::MAX),
            },
        )
    }

    /// Leaves the session: the host frees the seat (or marks it left in a
    /// running match), and this side closes.
    pub fn leave<T: Transport>(&mut self, endpoint: &mut Endpoint<T>) -> Result<()> {
        if self.seat.is_some() {
            self.up(endpoint, &Up::Leave)?;
        }
        self.closed = Some(LobbyClosed::Left);
        self.lobby.phase = LobbyPhase::Closed;
        self.lobby.closed = LobbyClosed::Left;
        self.lobby_dirty = true;
        Ok(())
    }

    fn up<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, up: &Up) -> Result<()> {
        if self.seat.is_none() && !matches!(up, Up::Join(_)) {
            return Ok(());
        }
        endpoint.send(PeerId::HOST, Channel::Reliable, &wire::encode_up(up))
    }

    /// The boundary of this player's tick: takes the endpoint's events (call
    /// `endpoint.update` first): replication into the mirror, the lobby into
    /// `Lobby` and `LocalPlayer`, the host's messages as delivered events.
    /// Returns the mark to pass to [`end_tick`](Self::end_tick).
    pub fn begin_tick<T: Transport>(
        &mut self,
        world: &mut World,
        endpoint: &mut Endpoint<T>,
    ) -> Result<ChangeTick> {
        while let Some(event) = endpoint.poll_event() {
            let Some(event) = self.replication.handle_event(world, event) else {
                continue;
            };
            self.event(endpoint, event);
        }
        let mark = world.change_tick();
        if self.lobby_dirty {
            let mut lobby = self.lobby.clone();
            if let Some(closed) = self.closed {
                lobby.phase = LobbyPhase::Closed;
                lobby.closed = closed;
            }
            lobby.write(world)?;
            self.lobby_dirty = false;
        }
        if self.local_dirty {
            self.local.write(world)?;
            self.local_dirty = false;
        }
        let mut delivered = 0;
        while delivered < MAX_DELIVERED {
            let Some(pending) = self.inbox.pop_front() else {
                break;
            };
            delivered += 1;
            let Some(ty) = self.types.get(pending.ty) else {
                continue;
            };
            let replication = &self.replication;
            let mut value = MessageValue::from_wire(pending.value, &|net| {
                replication.entity(net).unwrap_or(Entity::NULL)
            });
            let net = ty.envelope(&value.bytes);
            ty.set_envelope(&mut value.bytes, &message::delivered(&net, 0));
            let outcome = value
                .write(world, ty)
                .and_then(|bytes| world.send_bytes(ty.id, &bytes));
            match outcome {
                Ok(()) => self.stats.messages_delivered += 1,
                Err(problem) => {
                    self.stats.dropped_invalid += 1;
                    self.note(problem);
                }
            }
        }
        world.advance_change_tick();
        Ok(mark)
    }

    fn event<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, event: Event) {
        match event {
            Event::Connected { .. } => {
                let join = Up::Join(Join {
                    version: wire::VERSION,
                    token: self.token,
                    name: self.config.name.clone(),
                    color: u8::try_from(self.config.color).unwrap_or(u8::MAX),
                });
                if let Err(problem) =
                    endpoint.send(PeerId::HOST, Channel::Reliable, &wire::encode_up(&join))
                {
                    self.note(problem);
                }
            }
            Event::Disconnected { reason, .. } => {
                if self.closed.is_none() {
                    let closed = if self.closing_heard {
                        LobbyClosed::HostLeft
                    } else {
                        LobbyClosed::Lost
                    };
                    self.close_as(closed, reason.diagnostic(PeerId::HOST));
                }
            }
            Event::Message { peer, data, .. } => {
                if peer != PeerId::HOST || !wire::is_lobby(&data) {
                    self.stats.dropped_invalid += 1;
                    return;
                }
                self.down(&data);
            }
        }
    }

    fn close_as(&mut self, closed: LobbyClosed, cause: Diagnostic) {
        self.closed = Some(closed);
        // The seat is the host's to give back (a rejoin keeps the token).
        self.seat = None;
        self.lobby.phase = LobbyPhase::Closed;
        self.lobby.closed = closed;
        self.lobby_dirty = true;
        self.local.joined = false;
        self.local_dirty = true;
        let (code, message, hint) = match closed {
            LobbyClosed::HostLeft => (
                "lobby_host_left",
                "the host closed the session; the match is over".to_owned(),
                "Join another match, or host one yourself.",
            ),
            LobbyClosed::Refused => (
                "lobby_refused",
                format!("the host refused this player: {}", cause.message),
                "Join when the host's lobby is open, or ask the host to make room.",
            ),
            _ => (
                "lobby_host_left",
                format!("the connection to the host was lost: {}", cause.message),
                "Check the network and join again; within the grace period the seat is still yours.",
            ),
        };
        self.note(Diagnostic::warning(code, message, hint.to_owned()));
    }

    fn down(&mut self, data: &[u8]) {
        let Some(down) = wire::decode_down(data) else {
            self.stats.dropped_invalid += 1;
            self.note(Diagnostic::warning(
                "net_message_invalid",
                "the host sent a lobby message that is not valid; it was dropped",
                "The host runs another version of the game or is broken. If both run the same release, report it.",
            ));
            return;
        };
        match down {
            Down::Admitted {
                version,
                seat,
                token,
                types,
            } => {
                if version != wire::VERSION {
                    return self.close_as(
                        LobbyClosed::Refused,
                        Diagnostic::error("lobby_refused", "another version", "Update the game."),
                    );
                }
                self.seat = Some(seat);
                self.token = Some(token);
                self.remote_types.clear();
                self.host_index.clear();
                let mut wrong = Vec::new();
                for (index, entry) in types.iter().enumerate() {
                    let mine = self
                        .types
                        .iter()
                        .position(|t| t.name == entry.name && t.fingerprint == entry.fingerprint);
                    if let Some(mine) = mine {
                        self.host_index.insert(mine, index as u16);
                    } else {
                        wrong.push(entry.name.clone());
                    }
                    self.remote_types.push(mine);
                }
                if !wrong.is_empty() {
                    self.note(
                        Diagnostic::error(
                            "net_types_mismatch",
                            format!("the host's message types differ from this game's: {}", wrong.join(", ")),
                            "Host and players must run the same build of the game; build and start both from the same project.",
                        )
                        .with_candidates(wrong),
                    );
                }
                self.local = LocalState {
                    player: seat,
                    joined: true,
                    host: false,
                    role: self.local.role,
                    role_name: self.local.role_name.clone(),
                };
                self.local_dirty = true;
            }
            Down::Lobby(state) => {
                let reopened =
                    state.phase == LobbyPhase::Open && self.lobby.phase != LobbyPhase::Open;
                self.lobby = *state;
                self.lobby_dirty = true;
                if reopened && (self.local.role != 0 || !self.local.role_name.is_empty()) {
                    self.local.role = 0;
                    self.local.role_name = String::new();
                    self.local_dirty = true;
                }
            }
            Down::Role { role, name, .. } => {
                self.local.role = role;
                self.local.role_name = name;
                self.local_dirty = true;
            }
            Down::Message { ty, at, .. } => {
                let Some(Some(mine)) = self.remote_types.get(usize::from(ty)).copied() else {
                    self.stats.dropped_invalid += 1;
                    return;
                };
                let Some(kind) = self.types.get(mine) else {
                    return;
                };
                let mut reader = Reader::new(data.get(at..).unwrap_or_default());
                match message::read_value(&mut reader, &kind.layout) {
                    Some(value) if reader.done() => {
                        self.stats.messages_received += 1;
                        self.inbox.push_back(Pending { ty: mine, value });
                    }
                    _ => {
                        self.stats.dropped_invalid += 1;
                    }
                }
            }
            Down::Refused(why) => {
                let text = match why {
                    Refusal::Full => "every seat is taken",
                    Refusal::Running => "a match is running",
                    Refusal::RejoinExpired => "the rejoin came too late",
                    Refusal::Version => "another version",
                };
                self.close_as(
                    LobbyClosed::Refused,
                    Diagnostic::info("lobby_refused", text, "Join the next match."),
                );
            }
            Down::Closing => {
                self.closing_heard = true;
                self.close_as(
                    LobbyClosed::HostLeft,
                    Diagnostic::info("lobby_host_left", "closing", "Join another match."),
                );
            }
        }
    }

    /// The end of this player's tick `tick` (after its systems): sends the
    /// game's messages (to the host only) and lobby requests sent since
    /// `mark`. A tick played again by prediction sends only the messages it
    /// did not send the first time.
    pub fn end_tick<T: Transport>(
        &mut self,
        world: &World,
        endpoint: &mut Endpoint<T>,
        mark: ChangeTick,
        tick: u64,
    ) -> Result<()> {
        for request in world.events_since::<LobbyRequest>(mark)? {
            let up = match request.action {
                LobbyAction::Ready => Up::Ready(true),
                LobbyAction::NotReady => Up::Ready(false),
                LobbyAction::Name => {
                    let name = world.text(request.text).unwrap_or_default();
                    let mut end = name.len().min(MAX_NAME_BYTES);
                    while !name.is_char_boundary(end) {
                        end -= 1;
                    }
                    self.config.name = name.get(..end).unwrap_or_default().to_owned();
                    Up::Profile {
                        name: self.config.name.clone(),
                        color: u8::try_from(self.config.color).unwrap_or(u8::MAX),
                    }
                }
                LobbyAction::Color => {
                    self.config.color = request.value;
                    Up::Profile {
                        name: self.config.name.clone(),
                        color: u8::try_from(request.value).unwrap_or(u8::MAX),
                    }
                }
                LobbyAction::Leave => {
                    self.leave(endpoint)?;
                    continue;
                }
                LobbyAction::Start | LobbyAction::End | LobbyAction::Reopen => {
                    self.note(Diagnostic::warning(
                        "lobby_not_host",
                        format!("game code on a player's machine asked to {:?} the match; only the host can", request.action),
                        "Let the host's game code start, end and reopen; a player's lobby screen shows the host's decision.",
                    ));
                    continue;
                }
            };
            self.up(endpoint, &up)?;
        }
        let seated = self.seat.is_some() && self.closed.is_none();
        let types = self.types.clone();
        for (index, ty) in types.iter().enumerate() {
            let mut ordinal = 0usize;
            let already = self
                .sent
                .get(&tick)
                .and_then(|t| t.get(&index))
                .copied()
                .unwrap_or(0);
            for raw in world.event_bytes_since(ty.id, mark)? {
                let net = ty.envelope(&raw);
                if net.delivered {
                    continue;
                }
                ordinal += 1;
                if ordinal <= already {
                    continue;
                }
                if net.to != NetTo::Host {
                    self.stats.dropped_invalid += 1;
                    self.note(Diagnostic::warning(
                        "net_message_target_refused",
                        format!("game code on a player's machine sent a {} to {:?}; a player sends only to the host", ty.name, net.to),
                        "Send it to the host (NetWriter::to_host, Net::to_host()) and let the host's game code pass it on to one player, a group or everyone.",
                    ));
                    continue;
                }
                let Some(host_index) = self.host_index.get(&index).copied() else {
                    self.stats.dropped_offline += 1;
                    continue;
                };
                if !seated {
                    self.stats.dropped_offline += 1;
                    continue;
                }
                let value = MessageValue::read(world, ty, &raw);
                let replication = &self.replication;
                let wire_value = value.to_wire(&|e| replication.network_id(e).unwrap_or(0));
                let mut bytes = wire::encode_up(&Up::Message {
                    ty: host_index,
                    at: 0,
                });
                message::put_value(&mut bytes, &wire_value, &ty.layout);
                if bytes.len() > self.config.message_bytes + 16 {
                    self.stats.dropped_oversized += 1;
                    self.note(Diagnostic::warning(
                        "net_message_oversized",
                        format!("a {} of {} bytes is larger than the {} a message may have; it was not sent", ty.name, bytes.len(), self.config.message_bytes),
                        "Keep a message small (an id, a choice, a short text); long texts belong in assets.",
                    ));
                    continue;
                }
                endpoint.send(PeerId::HOST, Channel::Reliable, &bytes)?;
                self.stats.messages_sent += 1;
            }
            if ordinal > already {
                self.sent.entry(tick).or_default().insert(index, ordinal);
            }
        }
        let keep = tick.saturating_sub(SENT_MEMORY);
        self.sent.retain(|t, _| *t >= keep);
        Ok(())
    }
}
