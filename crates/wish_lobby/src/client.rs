//! A player's session: joins the host's lobby, mirrors it into the world,
//! delivers the host's messages and sends the game's (design note §3.6).

use std::collections::{BTreeMap, VecDeque};

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
use wish_net::{Channel, Endpoint, Event, PeerId, Transport};
use wish_replicate::input::Actions;
use wish_replicate::wire::Reader;
use wish_replicate::{Client, ClientConfig, FrameReport, Predictor, TickHook, Value};
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

/// What a predicting session keeps for the prediction world (design note
/// `1.1-net-tool.md` §12): the host's messages wait for the next tick the
/// prediction runs for the first time, and are delivered into it again
/// when a correction plays it again; the lobby is written into it when it
/// changed and on every tick played again.
#[derive(Debug, Default)]
struct Predicted {
    inbox: VecDeque<Pending>,
    delivered: BTreeMap<u64, Vec<Pending>>,
    lobby_dirty: bool,
    local_dirty: bool,
    /// The newest predicted tick whose lobby requests were sent.
    requests_through: Option<u64>,
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
    /// The game runs in a prediction world (`predict`), not in the mirror.
    predicting: bool,
    predicted: Predicted,
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
            lobby: LobbyState::default(),
            lobby_dirty: false,
            local: LocalState::default(),
            local_dirty: false,
            inbox: VecDeque::new(),
            sent: BTreeMap::new(),
            predicting: false,
            predicted: Predicted::default(),
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
        self.predicted = Predicted::default();
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
            self.lobby_now().write(world)?;
            self.lobby_dirty = false;
            self.predicted.lobby_dirty = true;
        }
        if self.local_dirty {
            self.local.write(world)?;
            self.local_dirty = false;
            self.predicted.local_dirty = true;
        }
        if self.predicting {
            // The game runs in the prediction world: the messages wait for
            // the next tick it predicts (`predict`).
            self.predicted.inbox.extend(self.inbox.drain(..));
        } else {
            let mut delivered = 0;
            while delivered < MAX_DELIVERED {
                let Some(pending) = self.inbox.pop_front() else {
                    break;
                };
                delivered += 1;
                let replication = std::mem::replace(
                    &mut self.replication,
                    Client::new(self.config.replication.clone()),
                );
                let done = self.deliver(world, pending, &|net| {
                    replication.entity(net).unwrap_or(Entity::NULL)
                });
                self.replication = replication;
                if done {
                    self.stats.messages_delivered += 1;
                }
            }
        }
        world.advance_change_tick();
        Ok(mark)
    }

    /// The lobby as this machine shows it (closed once the session closed).
    fn lobby_now(&self) -> LobbyState {
        let mut lobby = self.lobby.clone();
        if let Some(closed) = self.closed {
            lobby.phase = LobbyPhase::Closed;
            lobby.closed = closed;
        }
        lobby
    }

    /// Delivers one message of the host into `world` as an event (`entity`
    /// resolves the network ids it names); `true` when it was delivered.
    fn deliver(
        &mut self,
        world: &mut World,
        pending: Pending,
        entity: &dyn Fn(u32) -> Entity,
    ) -> bool {
        let Some(ty) = self.types.get(pending.ty) else {
            return false;
        };
        let mut value = MessageValue::from_wire(pending.value, entity);
        let net = ty.envelope(&value.bytes);
        ty.set_envelope(&mut value.bytes, &message::delivered(&net, 0));
        let outcome = value
            .write(world, ty)
            .and_then(|bytes| world.send_bytes(ty.id, &bytes));
        match outcome {
            Ok(()) => true,
            Err(problem) => {
                self.stats.dropped_invalid += 1;
                self.note(problem);
                false
            }
        }
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
        let ups = self.requests(world, mark)?;
        self.send_ups(endpoint, ups)?;
        let replication = std::mem::replace(
            &mut self.replication,
            Client::new(self.config.replication.clone()),
        );
        let outgoing = self.outgoing(world, mark, tick, &|entity| {
            replication.network_id(entity).unwrap_or(0)
        });
        self.replication = replication;
        self.send_messages(endpoint, outgoing?)
    }

    /// The lobby requests game code sent since `mark`, as what goes up.
    fn requests(&mut self, world: &World, mark: ChangeTick) -> Result<Vec<Up>> {
        let mut ups = Vec::new();
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
                LobbyAction::Leave => Up::Leave,
                LobbyAction::Start | LobbyAction::End | LobbyAction::Reopen => {
                    self.note(Diagnostic::warning(
                        "lobby_not_host",
                        format!("game code on a player's machine asked to {:?} the match; only the host can", request.action),
                        "Let the host's game code start, end and reopen; a player's lobby screen shows the host's decision.",
                    ));
                    continue;
                }
            };
            ups.push(up);
        }
        Ok(ups)
    }

    /// Sends lobby requests (a leave closes this side too).
    fn send_ups<T: Transport>(&mut self, endpoint: &mut Endpoint<T>, ups: Vec<Up>) -> Result<()> {
        for up in ups {
            if matches!(up, Up::Leave) {
                self.leave(endpoint)?;
            } else {
                self.up(endpoint, &up)?;
            }
        }
        Ok(())
    }

    /// Sends encoded game messages to the host.
    fn send_messages<T: Transport>(
        &mut self,
        endpoint: &mut Endpoint<T>,
        messages: Vec<Vec<u8>>,
    ) -> Result<()> {
        for bytes in messages {
            endpoint.send(PeerId::HOST, Channel::Reliable, &bytes)?;
            self.stats.messages_sent += 1;
        }
        Ok(())
    }

    /// The game's messages of tick `tick` sent since `mark`, encoded (to
    /// the host only; `network_id` names the entities they refer to), less
    /// those an earlier run of the same tick sent already.
    fn outgoing(
        &mut self,
        world: &World,
        mark: ChangeTick,
        tick: u64,
        network_id: &dyn Fn(Entity) -> u32,
    ) -> Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
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
                let wire_value = value.to_wire(network_id);
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
                out.push(bytes);
            }
            if ordinal > already {
                self.sent.entry(tick).or_default().insert(index, ordinal);
            }
        }
        let keep = tick.saturating_sub(SENT_MEMORY);
        self.sent.retain(|t, _| *t >= keep);
        Ok(out)
    }

    /// One frame of a predicting player (design note `1.1-net-tool.md`
    /// §12), after [`begin_tick`](Self::begin_tick) on the mirror world:
    /// `predictor` reconciles, predicts and sends this player's `actions`
    /// stamped with their ticks. Around every tick it runs, the host's
    /// messages and the lobby go into the prediction world and the game's
    /// messages and lobby requests come out of it, at most once per tick
    /// however often a correction plays it. `extra` is called around every
    /// tick too: after this session's part before a tick, and after it
    /// once the tick ran.
    pub fn predict<T: Transport>(
        &mut self,
        predictor: &mut Predictor,
        endpoint: &mut Endpoint<T>,
        now_ms: u64,
        actions: &Actions,
        extra: &mut dyn TickHook,
    ) -> Result<FrameReport> {
        self.predicting = true;
        let mut client = std::mem::replace(
            &mut self.replication,
            Client::new(self.config.replication.clone()),
        );
        let mut hook = SessionHook {
            session: self,
            extra,
            ups: Vec::new(),
            messages: Vec::new(),
        };
        let report = predictor.frame_with(&mut client, endpoint, now_ms, actions, &mut hook);
        let SessionHook { ups, messages, .. } = hook;
        self.replication = client;
        if let Some(confirmed) = predictor.confirmed_tick() {
            self.predicted.delivered.retain(|tick, _| *tick > confirmed);
        }
        let report = report?;
        self.send_ups(endpoint, ups)?;
        self.send_messages(endpoint, messages)?;
        Ok(report)
    }
}

/// A predicting session's part of every predicted tick.
struct SessionHook<'a> {
    session: &'a mut ClientSession,
    extra: &'a mut dyn TickHook,
    ups: Vec<Up>,
    messages: Vec<Vec<u8>>,
}

impl TickHook for SessionHook<'_> {
    fn before(
        &mut self,
        world: &mut World,
        tick: u64,
        again: bool,
        entity: &dyn Fn(u32) -> Entity,
    ) -> Result<()> {
        let session = &mut *self.session;
        // A tick played again starts from a snapshot that may be older than
        // the lobby's news: it gets the lobby as known now.
        if session.predicted.lobby_dirty || again {
            session.lobby_now().write(world)?;
            session.predicted.lobby_dirty = false;
        }
        if session.predicted.local_dirty || again {
            session.local.write(world)?;
            session.predicted.local_dirty = false;
        }
        let batch: Vec<Pending> = if again {
            session
                .predicted
                .delivered
                .get(&tick)
                .cloned()
                .unwrap_or_default()
        } else {
            let take = session.predicted.inbox.len().min(MAX_DELIVERED);
            let batch: Vec<Pending> = session.predicted.inbox.drain(..take).collect();
            if !batch.is_empty() {
                session.predicted.delivered.insert(tick, batch.clone());
            }
            batch
        };
        for pending in batch {
            if session.deliver(world, pending, entity) && !again {
                session.stats.messages_delivered += 1;
            }
        }
        self.extra.before(world, tick, again, entity)
    }

    fn after(
        &mut self,
        world: &World,
        tick: u64,
        again: bool,
        mark: ChangeTick,
        network_id: &dyn Fn(Entity) -> u32,
    ) -> Result<()> {
        let session = &mut *self.session;
        if session.predicted.requests_through.is_none_or(|t| tick > t) {
            self.ups.extend(session.requests(world, mark)?);
            session.predicted.requests_through = Some(tick);
        }
        self.messages
            .extend(session.outgoing(world, mark, tick, network_id)?);
        self.extra.after(world, tick, again, mark, network_id)
    }
}
