//! Networked sessions (release 1.1, package 14; design note
//! `docs/design/1.1-net-tool.md`): a session whose world is a match's host,
//! with the players' machines of the match in the same process, over the
//! network simulator (deterministic) or real UDP.
//!
//! Nothing here reads the clock: the endpoints' time is the tick count times
//! the tick length (§2 of the note), so a match over the simulator is a pure
//! function of the scene, the seeds and the calls.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serde_json::{Map, Value, json};
use wish_app::App;
use wish_core::{Diagnostic, Hash128, Result};
use wish_ecs::World;
use wish_lobby::{ClientSession, ClientSessionConfig, HostSession, LobbyConfig, RoleConfig};
use wish_net::discovery::broadcast_addr;
use wish_net::{
    Announce, Config, DISCOVERY_PORT, DiscoveryQuery, DiscoveryResponder, Endpoint, Entropy,
    LinkConditions, PeerId, Recv, RoomCode, SimNetwork, SimTransport, Transport, UdpTransport,
};
use wish_replicate::{Client, PredictConfig, Predictor, ReplicationConfig, TickHook};
use wish_types::{LobbyPhase, MAX_PLAYERS, PlayerId};

use crate::project::NetSettings;

/// Ticks a discovery query waits for an answer before it is sent again.
const DISCOVERY_RESEND_TICKS: u64 = 30;
/// Ticks a discovery query waits in all before the join gives up.
const DISCOVERY_GIVE_UP_TICKS: u64 = 600;

/// What an endpoint of a networked session sends through.
pub(crate) enum Wire {
    /// A node of the session's simulated network.
    Sim(SimTransport),
    /// A real UDP socket.
    Udp(UdpTransport),
}

impl Transport for Wire {
    fn local_addr(&self) -> SocketAddr {
        match self {
            Self::Sim(transport) => transport.local_addr(),
            Self::Udp(transport) => transport.local_addr(),
        }
    }

    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> Result<()> {
        match self {
            Self::Sim(transport) => transport.send(to, bytes),
            Self::Udp(transport) => transport.send(to, bytes),
        }
    }

    fn recv(&mut self, buf: &mut [u8]) -> Recv {
        match self {
            Self::Sim(transport) => transport.recv(buf),
            Self::Udp(transport) => transport.recv(buf),
        }
    }
}

/// The network a match runs over.
pub(crate) enum Network {
    /// The deterministic simulator: its conditions and seed.
    Sim {
        network: SimNetwork,
        conditions: LinkConditions,
        seed: u64,
    },
    /// Real UDP.
    Udp,
}

impl Network {
    /// A new endpoint transport on this network (a free port).
    fn transport(&self) -> Result<Wire> {
        match self {
            Self::Sim { network, .. } => Ok(Wire::Sim(network.add_node())),
            Self::Udp => Ok(Wire::Udp(UdpTransport::bind(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                0,
            ))?)),
        }
    }
}

/// How a player machine's input is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drive {
    /// From the calls: held input, replaced for one call by `players`.
    Input,
    /// From the machine's own game code: whatever its `ActionState` holds
    /// after its systems ran.
    Game,
}

impl Drive {
    pub(crate) fn parse(text: Option<&str>) -> Self {
        if text == Some("game") {
            Self::Game
        } else {
            Self::Input
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Game => "game",
        }
    }
}

/// Where a player machine is in reaching its host.
pub(crate) enum Link {
    /// Asking the network for a host (`address: "discover"`).
    Discovering {
        query: Box<DiscoveryQuery<Wire>>,
        room: Option<RoomCode>,
        asked_at: u64,
        since: u64,
    },
    /// Connecting or connected.
    Connected(Box<Endpoint<Wire>>),
    /// Gave up (discovery found nothing).
    Gone,
}

/// One player's machine: its session with the host and how it plays.
pub(crate) struct Machine {
    pub name: String,
    pub bot: bool,
    pub drive: Drive,
    /// The input it holds when a call does not name its seat.
    pub held: wish_input::Values,
    /// Asks to be ready once seated.
    pub auto_ready: bool,
    pub ready_sent: bool,
    pub session: ClientSession,
    pub link: Link,
    /// It asked to leave.
    pub left: bool,
    /// Diagnostics of its own (discovery), taken with the session's.
    pub notes: Vec<Diagnostic>,
    /// Ticks it has been connected (or connecting) to its host.
    pub ticks: u64,
    /// The person at this computer plays it (`net host` `play_as:
    /// "player"`): the session's own input drives it.
    pub person: bool,
    /// The pointer of every tick its prediction ran and the host has not
    /// confirmed, so that a tick played again sees the same pointer.
    pub pointers: std::collections::BTreeMap<u64, wish_input::Values>,
}

impl Machine {
    fn endpoint(&self) -> Option<&Endpoint<Wire>> {
        match &self.link {
            Link::Connected(endpoint) => Some(endpoint),
            _ => None,
        }
    }
}

/// A player machine in this process (design note `1.1-net-tool.md` §12):
/// the mirror of what its host sent, and the prediction, where its game
/// runs ahead of the host with its own player's input.
pub(crate) struct Player {
    /// The mirror: exactly what the host sent (the replication client and
    /// the lobby write into it; no system runs on it).
    pub mirror: App,
    /// The prediction: the game's systems in an app built like the host's.
    pub predictor: Predictor,
    /// The game code the prediction runs (kept with the systems that use
    /// it).
    pub _script: Option<wish_script::ScriptRuntime>,
    pub machine: Machine,
    /// The world the machine shows (the prediction with every entity where
    /// it is drawn), made when a picture asks for it, and the tick count it
    /// was made at.
    pub shown: Option<(u64, App)>,
}

impl Player {
    /// The world this machine's game runs in: the prediction world.
    pub(crate) fn world(&self) -> &World {
        self.predictor.world()
    }
}

/// The host of a match.
pub(crate) struct HostSide {
    pub endpoint: Endpoint<Wire>,
    pub session: HostSession,
    pub responder: Option<DiscoveryResponder<Wire>>,
}

/// A networked session's match.
pub(crate) struct NetMatch {
    pub network: Network,
    /// Milliseconds per tick of the endpoints' clock.
    pub tick_ms: u64,
    /// Steps taken (the clock over UDP).
    pub steps: u64,
    pub room: RoomCode,
    /// The game's id in discovery (the project's name).
    pub game: String,
    /// The host, when this session hosts.
    pub host: Option<HostSide>,
    /// This session's own machine, when it joined an outside host.
    pub own: Option<Machine>,
    /// The player machines in this process.
    pub players: Vec<Player>,
    /// Seats the match is for.
    pub expected: usize,
    /// Start the first match once `expected` seats are ready.
    pub auto_start: bool,
    pub host_plays: bool,
    /// The host world's hash after every tick, from tick 0.
    pub hashes: Vec<Hash128>,
    /// The session's seed (machines' keys over the simulator come from it).
    pub seed: u64,
    /// Machines made so far (each gets its own seed).
    pub made: u64,
    /// Diagnostics not taken yet (a discovery port that could not be opened).
    pub notes: Vec<Diagnostic>,
    /// The last diagnostics of the match, newest last, for `net status`.
    pub recent: std::collections::VecDeque<Diagnostic>,
    /// The scene file's hash and the game code's fingerprint when the match
    /// started, for its replay.
    pub scene_hash: String,
    pub code: Option<String>,
}

/// How many of the match's diagnostics `net status` shows.
const RECENT: usize = 16;

/// The simulated network of Play with N players in the editor and of `wen
/// play --players`: 50 ms each way with 5 ms of jitter and no loss (an
/// ordinary internet connection), so that the person plays a seat as an
/// online player does and sees the prediction answer at once (design note
/// `1.1-net-tool.md` §12).
pub fn play_network() -> Value {
    json!({ "latency_ms": 50, "jitter_ms": 5, "seed": 1 })
}

/// What a host is set up with.
pub(crate) struct HostSetup {
    pub seed: u64,
    pub expected: usize,
    pub host_plays: bool,
    pub host_name: String,
    pub auto_start: bool,
    pub room: Option<String>,
    /// `None`: the simulator with these conditions and seed.
    pub port: Option<u16>,
    pub conditions: LinkConditions,
    pub network_seed: u64,
    pub discoverable: bool,
    pub game: String,
    pub tick_rate: u32,
}

fn endpoint_config(room: &RoomCode) -> Config {
    let mut config = Config::new(room.clone());
    config.max_peers = MAX_PLAYERS as u32;
    config
}

/// The entropy of the `salt`-th secret of a match: from the seed over the
/// simulator (the same run every time), from the system over UDP.
fn entropy(network: &Network, seed: u64, salt: u64) -> Result<Entropy> {
    match network {
        Network::Sim { .. } => Ok(Entropy::seeded(
            seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15),
        )),
        Network::Udp => Entropy::os(),
    }
}

/// The lobby's settings for a host of this project.
pub(crate) fn lobby_config(settings: &NetSettings, setup: &HostSetup) -> LobbyConfig {
    let mut config = LobbyConfig {
        seed: setup.seed ^ 0x6C6F_6262_7921,
        host_plays: setup.host_plays,
        host_name: setup.host_name.clone(),
        tick_rate_hz: setup.tick_rate.max(1),
        ..LobbyConfig::default()
    };
    if let Some(n) = settings.min_players {
        config.min_players = n;
    }
    if let Some(n) = settings.max_players {
        config.max_players = n;
    }
    if let Some(n) = settings.colors {
        config.colors = n;
    }
    if let Some(n) = settings.rejoin_grace_ticks {
        config.rejoin_grace_ticks = n;
    }
    if let Some(n) = settings.message_bytes {
        config.message_bytes = n;
    }
    if let Some(n) = settings.messages_per_second {
        config.messages_per_second = n;
    }
    config.roles = settings
        .roles
        .iter()
        .map(|role| RoleConfig::new(&role.name, role.count).more_from(&role.more_from))
        .collect();
    config
}

/// The replication's settings for a host of this project.
pub(crate) fn replication_config(settings: &NetSettings) -> ReplicationConfig {
    let mut config = ReplicationConfig::new(&settings.replicate);
    if let Some(budget) = settings.budget_bytes {
        config.budget_bytes = budget;
        config.join_bytes = config.join_bytes.max(budget);
    }
    config
}

/// `(action id, [x, y])` of every action not at rest, in name order (the
/// form inputs cross the network in).
pub(crate) fn actions_of(values: &wish_input::Values) -> wish_replicate::input::Actions {
    values
        .iter()
        .filter(|(name, _)| {
            name.as_str() != wish_input::POINTER && name.as_str() != wish_input::CLICK
        })
        .map(|(name, [x, y])| {
            (
                wish_types::action_id(name),
                [
                    wish_replicate::input::clean(*x),
                    wish_replicate::input::clean(*y),
                ],
            )
        })
        .filter(|(_, [x, y])| *x != 0.0 || *y != 0.0)
        .collect()
}

impl NetMatch {
    /// A host on the simulator or on a UDP port, for a world already built.
    pub(crate) fn host(world: &World, settings: &NetSettings, setup: &HostSetup) -> Result<Self> {
        let network = match setup.port {
            None => {
                let network = SimNetwork::new(setup.network_seed, tick_ms(setup.tick_rate) as u32);
                network.set_conditions(setup.conditions);
                Network::Sim {
                    network,
                    conditions: setup.conditions,
                    seed: setup.network_seed,
                }
            }
            Some(_) => Network::Udp,
        };
        let mut secrets = entropy(&network, setup.seed, 1)?;
        let room = match &setup.room {
            Some(text) => RoomCode::parse(text)?,
            None => RoomCode::generate(&mut secrets),
        };
        let transport = match (&network, setup.port) {
            (Network::Sim { network, .. }, _) => Wire::Sim(network.add_node()),
            (Network::Udp, port) => Wire::Udp(UdpTransport::bind(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                port.unwrap_or(0),
            ))?),
        };
        let endpoint = Endpoint::host(
            transport,
            endpoint_config(&room),
            entropy(&network, setup.seed, 2)?,
        )?;
        let session = HostSession::new(
            world,
            lobby_config(settings, setup),
            replication_config(settings),
            room.as_str(),
            entropy(&network, setup.seed, 3)?,
        )?;
        let mut notes = Vec::new();
        let responder = if setup.discoverable {
            let bound = match &network {
                Network::Sim { network, .. } => network
                    .bind(SocketAddr::new(endpoint.local_addr().ip(), DISCOVERY_PORT))
                    .map(Wire::Sim),
                Network::Udp => UdpTransport::bind(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    DISCOVERY_PORT,
                ))
                .map(Wire::Udp),
            };
            match bound {
                Ok(transport) => Some(DiscoveryResponder::new(
                    transport,
                    Announce {
                        game: discovery_game(&setup.game),
                        name: discovery_game(&setup.game),
                        port: endpoint.local_addr().port(),
                        players: 1,
                        max_players: setup.expected as u16,
                        host_key: endpoint.public_key(),
                        room_code: Some(room.clone()),
                    },
                )?),
                Err(problem) => {
                    notes.push(problem);
                    None
                }
            }
        } else {
            None
        };
        Ok(Self {
            notes,
            recent: std::collections::VecDeque::new(),
            scene_hash: String::new(),
            code: None,
            network,
            tick_ms: tick_ms(setup.tick_rate),
            steps: 0,
            room,
            game: discovery_game(&setup.game),
            host: Some(HostSide {
                endpoint,
                session,
                responder,
            }),
            own: None,
            players: Vec::new(),
            expected: setup.expected,
            auto_start: setup.auto_start,
            host_plays: setup.host_plays,
            hashes: Vec::new(),
            seed: setup.seed,
            made: 0,
        })
    }

    /// The endpoints' time now, in milliseconds.
    fn now_ms(&self) -> u64 {
        match &self.network {
            Network::Sim { network, .. } => network.now_ms(),
            Network::Udp => self.steps.saturating_mul(self.tick_ms),
        }
    }

    /// The address a machine of this process reaches the host at.
    pub(crate) fn host_addr(&self) -> Option<SocketAddr> {
        let host = self.host.as_ref()?;
        let addr = host.endpoint.local_addr();
        Some(match self.network {
            Network::Sim { .. } => addr,
            Network::Udp => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port()),
        })
    }

    /// A new machine that reaches its host at `target` (or by discovery).
    pub(crate) fn machine(
        &mut self,
        world: &World,
        profile: (&str, u32, bool),
        drive: Drive,
        target: Target,
    ) -> Result<Machine> {
        let (name, color, bot) = profile;
        self.made += 1;
        let salt = 0x100 + self.made;
        let session = ClientSession::new(
            world,
            ClientSessionConfig {
                name: name.to_owned(),
                color,
                ..ClientSessionConfig::default()
            },
        );
        let link = match target {
            Target::At(addr, room) => Link::Connected(Box::new(Endpoint::connect(
                self.network.transport()?,
                addr,
                endpoint_config(&room),
                entropy(&self.network, self.seed, salt)?,
            )?)),
            Target::Discover(room) => {
                let mut transport = self.network.transport()?;
                if let Wire::Udp(udp) = &mut transport {
                    udp.enable_broadcast()?;
                }
                let mut secrets = entropy(&self.network, self.seed, salt ^ 0xD15C)?;
                let mut query = DiscoveryQuery::new(transport, &self.game, &mut secrets)?;
                query.send(broadcast_addr(DISCOVERY_PORT))?;
                Link::Discovering {
                    query: Box::new(query),
                    room,
                    asked_at: self.steps,
                    since: self.steps,
                }
            }
        };
        Ok(Machine {
            name: name.to_owned(),
            bot,
            drive,
            held: wish_input::Values::new(),
            auto_ready: true,
            ready_sent: false,
            session,
            link,
            left: false,
            notes: Vec::new(),
            ticks: 0,
            person: false,
            pointers: std::collections::BTreeMap::new(),
        })
    }

    /// One tick of the whole match: the network, the host (with `app`, or
    /// this session's own machine), then every player machine in the order
    /// they joined. `inputs[seat]` is that seat's input when `named[seat]`.
    pub(crate) fn tick(
        &mut self,
        app: &mut App,
        map: &wish_input::InputMap,
        inputs: &[wish_input::Values],
        named: &[bool],
    ) -> Result<Vec<Diagnostic>> {
        let mut failures = Vec::new();
        if let Network::Sim { network, .. } = &self.network {
            network.step();
        }
        self.steps += 1;
        let now = self.now_ms();
        let rest = wish_input::Values::new();
        if let Some(host) = self.host.as_mut() {
            host.endpoint.update(now);
            let present = host.session.lobby().present().len();
            if let Some(responder) = host.responder.as_mut() {
                responder.set_players(u16::try_from(present).unwrap_or(u16::MAX));
                responder.update(now);
            }
            if self.auto_start
                && host.session.lobby().phase == LobbyPhase::Open
                && host.session.lobby().round == 0
                && present >= self.expected
            {
                // Refused (someone is not ready yet) until it is not.
                let _ = host.session.start();
            }
            let tick = app.tick();
            let actions = if self.host_plays {
                actions_of(inputs.first().unwrap_or(&rest))
            } else {
                Vec::new()
            };
            let mark =
                host.session
                    .begin_tick(app.world_mut(), &mut host.endpoint, tick, &actions)?;
            failures.extend(app.step()?.failures);
            host.session
                .end_tick(app.world_mut(), &mut host.endpoint, mark)?;
            host.endpoint.update(now);
            self.hashes.push(app.hash());
        } else if let Some(own) = self.own.as_mut() {
            let given = if named.first() == Some(&true) {
                inputs.first().unwrap_or(&rest).clone()
            } else {
                own.held.clone()
            };
            tick_machine(
                app,
                own,
                map,
                &given,
                (now, self.steps),
                &self.network,
                &mut failures,
            )?;
        }
        // The session's own input drives the person's machine when the host
        // does not play.
        let own_input = (!self.host_plays && named.first() == Some(&true))
            .then(|| inputs.first().cloned().unwrap_or_default());
        for player in &mut self.players {
            let seat = player.machine.session.seat().map(|s| s as usize);
            let given = match (seat, &own_input) {
                (Some(seat), _) if seat > 0 && named.get(seat) == Some(&true) => {
                    inputs.get(seat).cloned().unwrap_or_default()
                }
                (_, Some(own)) if player.machine.person => own.clone(),
                _ => player.machine.held.clone(),
            };
            tick_player(
                player,
                &given,
                (now, self.steps),
                &self.network,
                &mut failures,
            )?;
        }
        Ok(failures)
    }

    /// The seat of the person's machine (`play_as: "player"`), once seated.
    pub(crate) fn person_seat(&self) -> Option<PlayerId> {
        self.players
            .iter()
            .find(|p| p.machine.person)
            .and_then(|p| p.machine.session.seat())
    }

    /// The machine seated at `seat` in this process.
    pub(crate) fn player_at(&self, seat: PlayerId) -> Option<&Player> {
        self.players
            .iter()
            .find(|p| p.machine.session.seat() == Some(seat))
    }

    /// The world of `seat`: the host's own player sees the host's world.
    pub(crate) fn world_of<'a>(&'a self, app: &'a App, seat: PlayerId) -> Option<&'a World> {
        if seat == 0 && self.host.is_some() && self.host_plays {
            return Some(app.world());
        }
        if self.own.as_ref().and_then(|m| m.session.seat()) == Some(seat) {
            return Some(app.world());
        }
        self.player_at(seat).map(Player::world)
    }

    /// The machine seated at `seat` in this process, to change.
    pub(crate) fn player_at_mut(&mut self, seat: PlayerId) -> Option<&mut Player> {
        self.players
            .iter_mut()
            .find(|p| p.machine.session.seat() == Some(seat))
    }

    /// Every seat a player of the match is known at.
    pub(crate) fn seats(&self) -> Vec<PlayerId> {
        match &self.host {
            Some(host) => host.session.lobby().seated(),
            None => self
                .own
                .as_ref()
                .map(|m| m.session.lobby().seated())
                .unwrap_or_default(),
        }
    }

    /// The diagnostics of the host, of every machine and of discovery since
    /// the last call.
    pub(crate) fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        let mut out = std::mem::take(&mut self.notes);
        if let Some(host) = self.host.as_mut() {
            out.extend(host.session.take_diagnostics());
        }
        if let Some(own) = self.own.as_mut() {
            out.append(&mut own.notes);
            out.extend(own.session.take_diagnostics());
        }
        for player in &mut self.players {
            out.append(&mut player.machine.notes);
            out.extend(player.machine.session.take_diagnostics());
        }
        out
    }

    /// Keeps the newest diagnostics for `net status`.
    pub(crate) fn remember(&mut self, notes: &[Diagnostic]) {
        for note in notes {
            if self.recent.len() >= RECENT {
                self.recent.pop_front();
            }
            self.recent.push_back(note.clone());
        }
    }

    /// The match as `net status` shows it (plain data: ticks, counts,
    /// hashes; no wall-clock value).
    pub(crate) fn status(&self, app: &App, rate: u32) -> Map<String, Value> {
        let mut out = Map::new();
        out.insert(
            "role".to_owned(),
            json!(if self.host.is_some() {
                "host"
            } else {
                "player"
            }),
        );
        out.insert("tick".to_owned(), json!(app.tick()));
        out.insert("room_code".to_owned(), json!(self.room.as_str()));
        out.insert("network".to_owned(), self.network_json());
        let lobby = match (&self.host, &self.own) {
            (Some(host), _) => host.session.lobby().clone(),
            (None, Some(own)) => {
                let mut lobby = own.session.lobby().clone();
                if own.session.closed().is_some() || matches!(own.link, Link::Gone) {
                    lobby.phase = LobbyPhase::Closed;
                }
                lobby
            }
            (None, None) => wish_lobby::LobbyState::default(),
        };
        out.insert(
            "lobby".to_owned(),
            json!({
                "phase": format!("{:?}", lobby.phase),
                "round": lobby.round,
                "outcome": lobby.outcome,
                "players": lobby.present().len(),
                "seats_expected": self.expected,
            }),
        );
        let mut seats = Vec::new();
        for player in lobby.seated() {
            let Some(seat) = lobby.seat(player) else {
                continue;
            };
            let mut item = json!({
                "player": player,
                "name": seat.name,
                "color": seat.color,
                "ready": seat.ready,
                "connected": seat.connected,
                "left": seat.left,
            });
            if let Some(host) = &self.host {
                if player == 0 && self.host_plays {
                    item["machine"] = json!("host");
                } else if let Some(peer) = host.session.peer_of(player) {
                    if let Some(rtt) = host.endpoint.rtt_ms(peer) {
                        item["rtt_ms"] = json!(rtt);
                    }
                    item["machine"] = json!("remote");
                    item["late_frames"] = json!(host.session.replication().late_frames(player));
                }
            }
            if let Some(local) = self.player_at(player) {
                item["machine"] = json!(match (local.machine.bot, local.machine.person) {
                    (true, _) => "bot",
                    (false, true) => "you",
                    (false, false) => "local",
                });
                item["drive"] = json!(local.machine.drive.name());
                if let Some(endpoint) = local.machine.endpoint() {
                    let stats = endpoint.stats();
                    let ticks = local.machine.ticks.max(1);
                    item["bytes_received"] = json!(stats.bytes_received);
                    item["bytes_sent"] = json!(stats.bytes_sent);
                    item["received_per_second"] =
                        json!(stats.bytes_received.saturating_mul(u64::from(rate)) / ticks);
                    item["resent"] = json!(stats.resent);
                }
                let predicted = local.predictor.stats();
                item["predicted"] = json!(true);
                item["lead_ticks"] = json!(local.predictor.sync().lead);
                item["corrections"] = json!(predicted.corrections);
                item["largest_correction_mm"] = json!(predicted.largest_correction_mm);
            }
            seats.push(item);
        }
        out.insert("seats".to_owned(), Value::Array(seats));
        let waiting: Vec<Value> = self
            .players
            .iter()
            .filter(|p| p.machine.session.seat().is_none())
            .map(|p| {
                json!({
                    "name": p.machine.name,
                    "state": match (&p.machine.link, p.machine.session.closed()) {
                        (_, Some(closed)) => format!("closed: {closed:?}"),
                        (Link::Discovering { .. }, None) => "discovering".to_owned(),
                        (Link::Gone, None) => "gave up".to_owned(),
                        (Link::Connected(_), None) => "joining".to_owned(),
                    },
                })
            })
            .collect();
        if !waiting.is_empty() {
            out.insert("not_seated".to_owned(), Value::Array(waiting));
        }
        if let Some(host) = &self.host {
            let stats = host.session.stats();
            let replication = host.session.replication().stats();
            let endpoint = host.endpoint.stats();
            out.insert(
                "host".to_owned(),
                json!({
                    "pending": host.session.status().pending,
                    "joins": stats.joins,
                    "rejoins": stats.rejoins,
                    "refusals": stats.refusals,
                    "messages_received": stats.messages_received,
                    "messages_delivered": stats.messages_delivered,
                    "messages_sent": stats.messages_sent,
                    "messages_dropped": stats.dropped_flood + stats.dropped_oversized + stats.dropped_invalid + stats.dropped_offline,
                    "states_sent": replication.states_sent,
                    "snapshots_sent": replication.snapshots_sent,
                    "state_bytes": replication.state_bytes,
                    "largest_state": replication.largest_state,
                    "deferred": replication.deferred,
                    "inputs": replication.inputs,
                    "bytes_sent": endpoint.bytes_sent,
                    "bytes_received": endpoint.bytes_received,
                    "resent": endpoint.resent,
                    "match_log_ticks": host.session.log().records.len(),
                }),
            );
        }
        if !self.recent.is_empty() {
            out.insert("recent_diagnostics".to_owned(), json!(self.recent));
        }
        out
    }

    fn network_json(&self) -> Value {
        match &self.network {
            Network::Sim {
                network,
                conditions,
                seed,
            } => json!({
                "kind": "simulated",
                "latency_ms": conditions.latency_ms,
                "jitter_ms": conditions.jitter_ms,
                "loss": conditions.loss,
                "duplicate": conditions.duplicate,
                "reorder": conditions.reorder,
                "seed": seed,
                "delivery_hash": network.delivery_hash().to_string(),
            }),
            Network::Udp => {
                let port = self.host.as_ref().map(|h| h.endpoint.local_addr().port());
                json!({ "kind": "udp", "port": port })
            }
        }
    }

    /// What player `seat`'s machine has, for `net status` with `player`.
    pub(crate) fn player_status(&self, app: &App, seat: PlayerId) -> Option<Value> {
        let world = self.world_of(app, seat)?;
        let local = world.resource_json("LocalPlayer").unwrap_or(Value::Null);
        let mut item = json!({
            "player": seat,
            "local_player": local,
            "entities": world.entities().count(),
            "world_hash": world.hash().to_string(),
        });
        if let Some(player) = self.player_at(seat) {
            let replication = player.machine.session.replication();
            let stats = replication.stats();
            item["tick"] = json!(player.predictor.app().tick());
            item["replicated_entities"] = json!(replication.shown().len());
            item["mirror_hash"] = json!(player.mirror.world().hash().to_string());
            item["states_received"] = json!(stats.states_received);
            item["out_of_order"] = json!(stats.out_of_order);
            item["resyncs_requested"] = json!(stats.resyncs_requested);
            item["inputs_sent"] = json!(stats.inputs_sent);
            if let Some(endpoint) = player.machine.endpoint()
                && let Some(rtt) = endpoint.rtt_ms(PeerId::HOST)
            {
                item["rtt_ms"] = json!(rtt);
            }
            item["prediction"] = prediction_json(&player.predictor);
            if let Some(host) = &self.host {
                item["prediction"]["late_frames"] =
                    json!(host.session.replication().late_frames(seat));
                if let Some(slack) = host.session.replication().input_slack(seat) {
                    item["prediction"]["input_slack_ticks"] = json!(slack);
                }
            }
        }
        Some(item)
    }
}

/// Where a new machine finds its host.
pub(crate) enum Target {
    /// At an address, with a room code.
    At(SocketAddr, RoomCode),
    /// By a discovery query, with a room code or the one announced.
    Discover(Option<RoomCode>),
}

fn tick_ms(rate: u32) -> u64 {
    (1000 / u64::from(rate.max(1))).max(1)
}

/// The game's id in discovery: the project's name, cut to 64 bytes.
fn discovery_game(name: &str) -> String {
    let mut end = name.len().min(64);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let cut = name[..end].trim();
    if cut.is_empty() {
        "wish".to_owned()
    } else {
        cut.to_owned()
    }
}

/// A predicting machine's prediction as `net status` shows it (ticks and
/// counts, no clock).
fn prediction_json(predictor: &Predictor) -> Value {
    let stats = predictor.stats();
    let sync = predictor.sync();
    json!({
        "predicted_tick": predictor.tick(),
        "confirmed_tick": predictor.confirmed_tick(),
        "lead_ticks": sync.lead,
        "round_trip_ms": sync.rtt_us / 1000,
        "predicted_entities": predictor.predicted().len(),
        "predicted_ticks": stats.predicted_ticks,
        "confirmations": stats.confirmations,
        "exact": stats.exact,
        "corrections": stats.corrections,
        "largest_correction_mm": stats.largest_correction_mm,
        "replayed_ticks": stats.replayed_ticks,
        "longest_replay": stats.longest_replay,
        "rebases": stats.rebases,
        "jumps": stats.jumps,
        "held_ticks": stats.held_ticks,
        "stalls": stats.stalls,
        "late_reports": sync.late_reports,
        "smoothing_mm": (predictor.largest_offset() * 1000.0).round() as u64,
    })
}

/// The prediction settings of a match whose endpoints' clock moves
/// `tick_ms` milliseconds a tick (the prediction's tick is that long, so
/// that its clock and the host's agree).
pub(crate) fn predict_config(tick_ms: u64) -> PredictConfig {
    PredictConfig::new(u32::try_from(tick_ms.saturating_mul(1000)).unwrap_or(16_000))
}

/// What a predicting machine adds around every tick its prediction runs:
/// the pointer (its actions go through the prediction itself).
struct PointerHook<'a> {
    given: &'a wish_input::Values,
    pointers: &'a mut std::collections::BTreeMap<u64, wish_input::Values>,
}

impl TickHook for PointerHook<'_> {
    fn before(
        &mut self,
        world: &mut World,
        tick: u64,
        again: bool,
        _: &dyn Fn(u32) -> wish_core::Entity,
    ) -> Result<()> {
        let values = if again {
            self.pointers.get(&tick).cloned().unwrap_or_default()
        } else {
            let values: wish_input::Values = self
                .given
                .iter()
                .filter(|(name, _)| {
                    [wish_input::POINTER, wish_input::CLICK, wish_input::SCREEN]
                        .contains(&name.as_str())
                })
                .map(|(name, value)| (name.clone(), *value))
                .collect();
            if !values.is_empty() {
                self.pointers.insert(tick, values.clone());
            }
            values
        };
        wish_input::apply_pointer(world, &values)
    }
}

/// One tick of a player machine in this process (design note
/// `1.1-net-tool.md` §12): the host's news into the mirror, then a frame of
/// the prediction with this tick's input (`given`, or what the machine's own
/// game code holds when it drives itself).
fn tick_player(
    player: &mut Player,
    given: &wish_input::Values,
    (now, steps): (u64, u64),
    network: &Network,
    failures: &mut Vec<Diagnostic>,
) -> Result<()> {
    if !connect_machine(&mut player.machine, steps, network)? {
        return Ok(());
    }
    let machine = &mut player.machine;
    let Link::Connected(endpoint) = &mut machine.link else {
        return Ok(());
    };
    endpoint.update(now);
    machine
        .session
        .begin_tick(player.mirror.world_mut(), endpoint)?;
    machine.ticks += 1;
    if machine.session.seat().is_some()
        && machine.session.closed().is_none()
        && machine.auto_ready
        && !machine.ready_sent
    {
        machine.session.set_ready(endpoint, true)?;
        machine.ready_sent = true;
    }
    // A machine that left, or whose host is gone, has nothing to predict
    // for and no one to send to.
    if machine.session.closed().is_some() || !endpoint.peers().contains(&PeerId::HOST) {
        endpoint.update(now);
        return Ok(());
    }
    let actions = match machine.drive {
        Drive::Input => actions_of(given),
        Drive::Game => Client::local_actions(player.predictor.world()),
    };
    let mut hook = PointerHook {
        given,
        pointers: &mut machine.pointers,
    };
    let report =
        machine
            .session
            .predict(&mut player.predictor, endpoint, now, &actions, &mut hook)?;
    if let Some(confirmed) = player.predictor.confirmed_tick() {
        machine.pointers.retain(|tick, _| *tick > confirmed);
    }
    failures.extend(report.failures);
    endpoint.update(now);
    Ok(())
}

/// Moves a machine that is finding its host by discovery on: `true` once it
/// is connected (or connecting) to a host.
fn connect_machine(machine: &mut Machine, steps: u64, network: &Network) -> Result<bool> {
    if let Link::Discovering {
        query,
        room,
        asked_at,
        since,
    } = &mut machine.link
    {
        query.update();
        // The first host (in address order) whose room code is the one asked
        // for, or that announces one when none was asked for.
        let found = query.found().find_map(|(addr, announce)| {
            let code = match (room.as_ref(), announce.room_code.as_ref()) {
                (Some(wanted), Some(told)) if wanted != told => return None,
                (Some(wanted), _) => wanted.clone(),
                (None, Some(told)) => told.clone(),
                (None, None) => return None,
            };
            Some((addr, announce.host_key, code))
        });
        match found {
            Some((addr, host_key, code)) => {
                let mut config = endpoint_config(&code);
                config.host_key = Some(host_key);
                let secret = Entropy::seeded(steps ^ u64::from(addr.port()));
                let entropy = match network {
                    Network::Sim { .. } => secret,
                    Network::Udp => Entropy::os()?,
                };
                machine.link = Link::Connected(Box::new(Endpoint::connect(
                    network.transport()?,
                    addr,
                    config,
                    entropy,
                )?));
            }
            None if steps.saturating_sub(*since) >= DISCOVERY_GIVE_UP_TICKS => {
                machine.notes.push(Diagnostic::warning(
                    "net_join_failed",
                    format!(
                        "'{}' found no match of this game on the network in {DISCOVERY_GIVE_UP_TICKS} ticks.",
                        machine.name
                    ),
                    "Check that a host is running and discoverable on the same network (a firewall may block the discovery port, 47610), or join by its address.",
                ));
                machine.link = Link::Gone;
                return Ok(false);
            }
            None => {
                if steps.saturating_sub(*asked_at) >= DISCOVERY_RESEND_TICKS {
                    *asked_at = steps;
                    query.send(broadcast_addr(DISCOVERY_PORT))?;
                }
                return Ok(false);
            }
        }
    }
    Ok(matches!(machine.link, Link::Connected(_)))
}

/// One tick of this session's own machine when it plays in a match hosted
/// elsewhere (`(now, steps)`: the endpoints' time and the match's step
/// count): it mirrors the host and runs its systems on the mirror.
fn tick_machine(
    app: &mut App,
    machine: &mut Machine,
    map: &wish_input::InputMap,
    given: &wish_input::Values,
    (now, steps): (u64, u64),
    network: &Network,
    failures: &mut Vec<Diagnostic>,
) -> Result<()> {
    if !connect_machine(machine, steps, network)? {
        return Ok(());
    }
    let Link::Connected(endpoint) = &mut machine.link else {
        return Ok(());
    };
    endpoint.update(now);
    let mark = machine.session.begin_tick(app.world_mut(), endpoint)?;
    if machine.drive == Drive::Input {
        wish_input::apply(app.world_mut(), map, given)?;
    }
    let own = app.tick();
    failures.extend(app.step()?.failures);
    machine.session.end_tick(app.world(), endpoint, mark, own)?;
    if machine.session.seat().is_some() && machine.session.closed().is_none() {
        if machine.auto_ready && !machine.ready_sent {
            machine.session.set_ready(endpoint, true)?;
            machine.ready_sent = true;
        }
        let actions = Client::local_actions(app.world());
        machine
            .session
            .replication_mut()
            .send_input(endpoint, actions)?;
    }
    endpoint.update(now);
    Ok(())
}

impl crate::workspace::Workspace {
    /// Starts a networked session hosting a match of `scene` (`net host`).
    pub(crate) fn start_net_session(&mut self, scene: &str, setup: &HostSetup) -> Result<String> {
        if self.sessions.open.len() >= crate::sessions::MAX_SESSIONS {
            return Err(too_many_sessions());
        }
        let (app, script, assets) = self.net_host_app(scene, setup.seed)?;
        let settings = self.config().map(|c| c.net()).unwrap_or_default();
        let mut net = NetMatch::host(app.world(), &settings, setup)?;
        net.scene_hash =
            wish_core::hash_bytes(&std::fs::read(self.root().join(scene)).unwrap_or_default())
                .to_string();
        net.code = script
            .as_ref()
            .map(|script| script.module().fingerprint().to_string());
        let input = self.input_map()?;
        let id = self.sessions.new_id();
        let room = net.room.as_str().to_owned();
        self.sessions.open.insert(
            id.clone(),
            crate::sessions::Session {
                scene: scene.to_owned(),
                seed: setup.seed,
                app,
                saved: Vec::new(),
                reported: std::collections::BTreeSet::new(),
                script,
                input,
                recording: None,
                assets,
                players: MAX_PLAYERS,
                net: Some(Box::new(net)),
            },
        );
        let started = Diagnostic::info(
            "session_started",
            format!(
                "Session {id} hosts a match of '{scene}' with seed {} for {} seats (room {room}).",
                setup.seed, setup.expected
            ),
            "Add players with net (join, bots) and step it with net step or sim_step.",
        );
        self.sessions
            .record(Some(&id), Some(0), "session", &started);
        Ok(id)
    }

    /// The host's world of a match before its first tick: the session's
    /// world with the project's actions at rest (so that the players' input
    /// frames have their ids). A replay of the match builds the same.
    pub(crate) fn net_host_app(
        &mut self,
        scene: &str,
        seed: u64,
    ) -> Result<(App, Option<wish_script::ScriptRuntime>, String)> {
        let (mut app, script, assets) = crate::sessions::build_app(self, scene, seed, false)?;
        let map = self.input_map()?;
        wish_input::apply(app.world_mut(), &map, &wish_input::Values::new())?;
        Ok((app, script, assets))
    }

    /// A player's machine for session `id`, with its own world.
    fn net_player_app(&mut self, id: &str) -> Result<(App, Option<wish_script::ScriptRuntime>)> {
        let (scene, seed) = {
            let session = self.session(id)?;
            (session.scene.clone(), session.seed)
        };
        let (mut app, script, _) = crate::sessions::build_app(self, &scene, seed, true)?;
        // The project's actions at rest, as on the host: the prediction
        // writes its player's input against these ids.
        let map = self.input_map()?;
        wish_input::apply(app.world_mut(), &map, &wish_input::Values::new())?;
        Ok((app, script))
    }

    /// Adds a player machine to session `id` (`net join`, `net bots`).
    pub(crate) fn net_add_machine(
        &mut self,
        id: &str,
        (name, bot): (&str, bool),
        drive: Drive,
        args: &crate::param::Args,
    ) -> Result<()> {
        let held = crate::tools::parse_input(self, id, args.value("input"))?;
        // The mirror (what the host sent) and the prediction (where the
        // machine's game runs) are two worlds of the same build; the
        // mirror runs no system, so its copy of the game code is not kept.
        let (mirror, _) = self.net_player_app(id)?;
        let (app, script) = self.net_player_app(id)?;
        let session = self.session_mut(id)?;
        let Some(net) = session.net.as_deref_mut() else {
            return Err(crate::tools::net_not_networked(id));
        };
        if net.players.len() >= MAX_PLAYERS {
            return Err(Diagnostic::error(
                "lobby_full",
                format!("Session {id} already has {MAX_PLAYERS} player machines."),
                "A match has at most 16 seats.",
            ));
        }
        let room = match args.str("room_code") {
            Some(text) => RoomCode::parse(text)?,
            None => net.room.clone(),
        };
        let target = match args.str("address") {
            None => match net.host_addr() {
                Some(addr) => Target::At(addr, room),
                None => {
                    return Err(join_failed(
                        "this session does not host",
                        "Join with an address, or host a match with net (action host).",
                    ));
                }
            },
            Some("discover") => Target::Discover(args.str("room_code").map(|_| room)),
            Some(text) => Target::At(parse_address(text)?, room),
        };
        let color = u32::try_from(args.int("color").unwrap_or(0)).unwrap_or(0);
        let mut machine = net.machine(mirror.world(), (name, color, bot), drive, target)?;
        machine.held = held;
        machine.auto_ready = args.value("ready").and_then(Value::as_bool) != Some(false);
        let predictor = Predictor::new(app, predict_config(net.tick_ms))?;
        net.players.push(Player {
            mirror,
            predictor,
            _script: script,
            machine,
            shown: None,
        });
        Ok(())
    }

    /// Adds a bot ("Bot N") to session `id`; returns its name.
    pub(crate) fn net_add_bot(
        &mut self,
        id: &str,
        drive: Drive,
        args: &crate::param::Args,
    ) -> Result<String> {
        let number = self
            .session(id)?
            .net
            .as_deref()
            .map_or(0, |n| n.players.iter().filter(|p| p.machine.bot).count())
            + 1;
        let name = format!("Bot {number}");
        self.net_add_machine(id, (&name, true), drive, args)?;
        Ok(name)
    }

    /// This process joins a match hosted elsewhere over UDP: a new session
    /// whose world is this machine's.
    pub(crate) fn join_outside(
        &mut self,
        address: &str,
        args: &crate::param::Args,
    ) -> Result<String> {
        if self.sessions.open.len() >= crate::sessions::MAX_SESSIONS {
            return Err(too_many_sessions());
        }
        let room = args.str("room_code").map(RoomCode::parse).transpose()?;
        let target = if address == "discover" {
            Target::Discover(room.clone())
        } else {
            let addr = parse_address(address)?;
            let Some(room) = room.clone() else {
                return Err(Diagnostic::error(
                    "net_room_code_invalid",
                    "Joining a host by its address needs its room code.",
                    "Pass room_code: the code the host shows (or join with address \"discover\" to take the code a host on the local network announces).",
                )
                .at_path("/room_code"));
            };
            Target::At(addr, room)
        };
        let scene = self.scene_path(args.str("scene"))?;
        self.open_document(&scene, false)?;
        let seed = u64::try_from(args.int("seed").unwrap_or(0)).unwrap_or(0);
        let (app, script, assets) = crate::sessions::build_app(self, &scene, seed, true)?;
        let input = self.input_map()?;
        let game = self
            .config()
            .map_or_else(|| "wish".to_owned(), |c| c.name.clone());
        let mut net = NetMatch {
            network: Network::Udp,
            tick_ms: tick_ms(60),
            steps: 0,
            room: room.unwrap_or_else(|| RoomCode::generate(&mut Entropy::seeded(seed))),
            game: discovery_game(&game),
            host: None,
            own: None,
            players: Vec::new(),
            expected: 0,
            auto_start: false,
            host_plays: false,
            hashes: Vec::new(),
            seed,
            made: 0,
            notes: Vec::new(),
            recent: std::collections::VecDeque::new(),
            scene_hash: String::new(),
            code: None,
        };
        let name = args.str("name").unwrap_or("Player").to_owned();
        let color = u32::try_from(args.int("color").unwrap_or(0)).unwrap_or(0);
        let mut machine = net.machine(
            app.world(),
            (&name, color, false),
            Drive::parse(args.str("drive")),
            target,
        )?;
        machine.auto_ready = args.value("ready").and_then(Value::as_bool) != Some(false);
        net.own = Some(machine);
        let id = self.sessions.new_id();
        self.sessions.open.insert(
            id.clone(),
            crate::sessions::Session {
                scene,
                seed,
                app,
                saved: Vec::new(),
                reported: std::collections::BTreeSet::new(),
                script,
                input,
                recording: None,
                assets,
                players: 1,
                net: Some(Box::new(net)),
            },
        );
        Ok(id)
    }

    /// Player `seat`'s machine leaves session `id` (the session's own
    /// machine when it joined elsewhere and no seat is given).
    pub(crate) fn net_leave(&mut self, id: &str, seat: Option<PlayerId>) -> Result<String> {
        let session = self.session_mut(id)?;
        let Some(net) = session.net.as_deref_mut() else {
            return Err(crate::tools::net_not_networked(id));
        };
        let seats = net.seats();
        let machine = match seat {
            Some(seat) => net
                .players
                .iter_mut()
                .map(|p| &mut p.machine)
                .find(|m| m.session.seat() == Some(seat)),
            None => net.own.as_mut(),
        };
        let Some(machine) = machine else {
            return Err(crate::tools::net_unknown_player(
                id,
                seat.unwrap_or(0),
                &seats,
            ));
        };
        if let Link::Connected(endpoint) = &mut machine.link {
            machine.session.leave(endpoint)?;
        }
        machine.left = true;
        Ok(machine.name.clone())
    }

    /// Closes the match of session `id` for everyone: the host tells every
    /// player and the match runs until the news is out (at most 120 ticks).
    /// Returns the ticks it ran.
    pub(crate) fn net_close(&mut self, id: &str) -> Result<u64> {
        let session = self.session_mut(id)?;
        let Some(net) = session.net.as_deref_mut() else {
            return Err(crate::tools::net_not_networked(id));
        };
        if let Some(host) = net.host.as_mut() {
            host.session.close();
        } else if let Some(own) = net.own.as_mut()
            && let Link::Connected(endpoint) = &mut own.link
        {
            own.session.leave(endpoint)?;
        }
        let mut ran = 0;
        let rest = vec![wish_input::Values::new(); MAX_PLAYERS];
        let named = vec![false; MAX_PLAYERS];
        while ran < 120 {
            let closed = net
                .host
                .as_ref()
                .is_none_or(|host| host.session.is_closed());
            if closed && ran > 0 {
                break;
            }
            net.tick(&mut session.app, &session.input, &rest, &named)?;
            ran += 1;
        }
        Ok(ran)
    }

    /// The world a tool looks at in session `id`: the session's own, or
    /// player `seat`'s machine's in a networked match.
    pub(crate) fn session_world(&self, id: &str, seat: Option<PlayerId>) -> Result<&World> {
        let session = self.session(id)?;
        let Some(seat) = seat else {
            return Ok(session.app.world());
        };
        let Some(net) = session.net.as_deref() else {
            return Err(crate::tools::net_not_networked(id).at_path("/player"));
        };
        net.world_of(&session.app, seat)
            .ok_or_else(|| crate::tools::net_unknown_player(id, seat, &net.seats()))
    }

    /// The tick of the world [`Self::session_world`] gives.
    pub(crate) fn session_world_tick(&self, id: &str, seat: Option<PlayerId>) -> Result<u64> {
        let session = self.session(id)?;
        let tick = match (seat, session.net.as_deref()) {
            (Some(seat), Some(net)) => net
                .player_at(seat)
                .map_or(session.app.tick(), |p| p.predictor.app().tick()),
            _ => session.app.tick(),
        };
        Ok(tick)
    }

    /// The world a picture of session `id` draws: as
    /// [`Self::session_world`], except that a predicting player's machine
    /// shows its presented view (design note `1.1-net-tool.md` §12): its
    /// prediction world with every entity it mirrors moved to where the
    /// machine draws it (its own player predicted and smoothed, the others
    /// interpolated between the host's states).
    pub(crate) fn session_picture_world(
        &mut self,
        id: &str,
        seat: Option<PlayerId>,
    ) -> Result<&World> {
        let Some(seat) = seat else {
            return self.session_world(id, None);
        };
        let needed = self
            .session(id)?
            .net
            .as_deref()
            .and_then(|net| net.player_at(seat))
            .map(|player| player.shown.is_none());
        let Some(needed) = needed else {
            return self.session_world(id, Some(seat));
        };
        // A world of the same build to draw from, made once.
        let blank = if needed {
            Some(self.net_player_app(id)?.0)
        } else {
            None
        };
        let session = self.session_mut(id)?;
        let Some(player) = session
            .net
            .as_deref_mut()
            .and_then(|net| net.player_at_mut(seat))
        else {
            return Err(crate::tools::net_not_networked(id).at_path("/player"));
        };
        if let Some(blank) = blank {
            player.shown = Some((u64::MAX, blank));
        }
        let ticks = player.machine.ticks;
        let Some((at, shown)) = player.shown.as_mut() else {
            return Err(crate::tools::net_not_networked(id).at_path("/player"));
        };
        if *at != ticks {
            let world = shown.world_mut();
            world.restore(&player.predictor.world().snapshot())?;
            for (net, drawn) in player.predictor.presented() {
                if let Some(entity) = player.predictor.entity(*net)
                    && world.contains(entity)
                {
                    world.insert(entity, drawn.transform)?;
                }
            }
            wish_transform::update_global_transforms(world)?;
            *at = ticks;
        }
        Ok(shown.world())
    }

    /// Plays the host's match log of session `id` into a fresh host world
    /// and compares its hash after every tick: `None` when every tick is the
    /// same, else the first tick that differs.
    pub(crate) fn verify_match(&mut self, id: &str) -> Result<Option<u64>> {
        let (scene, seed, log, hashes) = {
            let session = self.session(id)?;
            let Some(net) = session.net.as_deref() else {
                return Err(crate::tools::net_not_networked(id));
            };
            let Some(host) = net.host.as_ref() else {
                return Err(join_failed(
                    "the match is hosted elsewhere, so its log is there",
                    "Verify the match on its host.",
                ));
            };
            (
                session.scene.clone(),
                session.seed,
                host.session.log().clone(),
                net.hashes.iter().map(|h| short(*h)).collect::<Vec<_>>(),
            )
        };
        let (mut app, _script, _) = self.net_host_app(&scene, seed)?;
        play_log(&mut app, &log, &hashes)
    }
}

fn too_many_sessions() -> Diagnostic {
    Diagnostic::error(
        "too_many_sessions",
        format!(
            "{} sessions are already open.",
            crate::sessions::MAX_SESSIONS
        ),
        "Close one with sim_stop (or net stop) first.",
    )
}

/// A hash as replays store it (64 bits).
pub(crate) fn short(hash: Hash128) -> u64 {
    (hash.as_u128() >> 64) as u64
}

/// Plays `log` into `app` (a fresh host world), comparing every tick's hash
/// with `hashes`: `None` when all are equal, else the first tick that
/// differs.
pub(crate) fn play_log(
    app: &mut App,
    log: &wish_lobby::MatchLog,
    hashes: &[u64],
) -> Result<Option<u64>> {
    let mut carry = wish_lobby::Carry::new();
    for (tick, expected) in (0..hashes.len() as u64).zip(hashes) {
        wish_lobby::apply(app.world_mut(), &log.record(tick), &mut carry)?;
        app.step()?;
        if short(app.hash()) != *expected {
            return Ok(Some(tick));
        }
    }
    Ok(None)
}

fn join_failed(what: &str, hint: &str) -> Diagnostic {
    Diagnostic::error("net_join_failed", format!("Cannot join: {what}."), hint)
}

/// `host:port` as a socket address.
fn parse_address(text: &str) -> Result<SocketAddr> {
    use std::net::ToSocketAddrs;
    text.to_socket_addrs()
        .ok()
        .and_then(|mut found| found.next())
        .ok_or_else(|| {
            Diagnostic::error(
                "net_join_failed",
                format!("'{text}' is not an address to join."),
                "Give the host's address and port, like \"192.168.1.20:47600\" (the host shows its port), or \"discover\" to find it on the local network.",
            )
            .at_path("/address")
        })
}
