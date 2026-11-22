//! A small hidden-role meeting game for the lobby's tests: one host app and
//! several client apps over a `SimNetwork`, every side built from the same
//! functions.
//!
//! The host (a player too, seat 0) seats an avatar per player. Once the
//! match runs, the host calls a meeting; every player (the host's own
//! included) chats three lines (`ChatSay` to the host, relayed as `ChatLine`
//! to everyone) and votes (`Vote` to the host); the host announces the
//! result (`VoteResult` to everyone) and tells each saboteur its partner
//! (`Whisper` to one player). The same `bot` system runs on every machine.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;

use wish_app::{App, AppConfig, System, SystemContext};
use wish_core::math::Vec3;
use wish_core::{Entity, Result, StableHasher, Text};
use wish_ecs::{ChangeTick, With, World};
use wish_lobby::{ClientSession, ClientSessionConfig, HostSession, LobbyConfig, RoleConfig, apply};
use wish_net::crypto::{KeyPair, SessionKeys};
use wish_net::wire::{self as net_wire, Frame, Packet};
use wish_net::{
    Captured, Config, Endpoint, Entropy, LinkConditions, RoomCode, SimNetwork, SimTransport,
    Transport,
};
use wish_replicate::{PredictConfig, Predictor, ReplicationConfig, TickHook};
use wish_schema::{component, event, resource};
use wish_types::{
    ActionSlot, ActionState, Lobby, LobbyPhase, LocalPlayer, MAX_PLAYERS, MatchRoles, Net,
    NetOwner, PlayerActionSlots, PlayerActionState, PlayerId, Players, Replicated, Transform,
    action_id,
};

/// A player's body.
#[component]
pub struct Avatar {
    /// Its seat.
    pub seat: u32,
}

/// A player says something (to the host).
#[event]
pub struct ChatSay {
    /// The envelope.
    pub net: Net,
    /// What was said.
    pub text: Text,
}

/// The host passes a line on (to everyone).
#[event]
pub struct ChatLine {
    /// The envelope.
    pub net: Net,
    /// Who said it.
    pub speaker: PlayerId,
    /// What was said.
    pub text: Text,
}

/// A player votes (to the host).
#[event]
pub struct Vote {
    /// The envelope.
    pub net: Net,
    /// The player voted for.
    pub choice: PlayerId,
}

/// The host calls a meeting (to everyone).
#[event]
pub struct MeetingCalled {
    /// The envelope.
    pub net: Net,
    /// Who called it.
    pub by: PlayerId,
}

/// The host announces the vote (to everyone).
#[event]
pub struct VoteResult {
    /// The envelope.
    pub net: Net,
    /// Who leaves (the most votes; ties: the lowest id).
    pub ejected: PlayerId,
    /// Each player's vote plus one (0: none).
    pub votes: [u32; MAX_PLAYERS],
}

/// The host tells a saboteur its partner (to one player).
#[event]
pub struct Whisper {
    /// The envelope.
    pub net: Net,
    /// The other saboteur.
    pub partner: PlayerId,
    /// A number no crew member may receive.
    pub secret: u64,
}

/// A player points at an entity (to the host): references cross the wire as
/// network ids.
#[event]
pub struct Point {
    /// The envelope.
    pub net: Net,
    /// What it points at.
    pub at: Entity,
}

/// The host's meeting.
#[resource]
pub struct Meeting {
    /// The round it belongs to.
    pub round: u32,
    /// The tick the round started at.
    pub started: u64,
    /// Whether it was called.
    pub called: bool,
    /// The tick it was called at.
    pub called_at: u64,
    /// Whether the result was announced.
    pub done: bool,
    /// Votes plus one.
    pub votes: [u32; MAX_PLAYERS],
    /// Chat lines relayed.
    pub lines: u32,
    /// A digest of every line relayed, in order.
    pub digest: u64,
    /// Points received: from whom (plus one) and whether it named the
    /// pointer's own avatar.
    pub points: u32,
    /// Points that named the sender's own avatar.
    pub points_own: u32,
    /// Messages received whose sender was not a seated player.
    pub strangers: u32,
}

/// What a player's machine heard.
#[resource]
pub struct Heard {
    /// The tick the meeting was called, plus one (0: not yet).
    pub called_at: u64,
    /// Chat lines received.
    pub lines: u32,
    /// Their digest, in order.
    pub digest: u64,
    /// Results received.
    pub results: u32,
    /// The ejected player of the last result.
    pub ejected: u32,
    /// Whispers received.
    pub whispers: u32,
    /// The partner the last whisper named.
    pub partner: u32,
    /// Delivered messages whose sender was not the host.
    pub wrong_sender: u32,
}

/// The role names; the saboteur's is a marker the hidden test searches for.
pub const CREW: &str = "crew";
pub const SABOTEUR: &str = "SABOTEUR-ZQX";
/// The whisper's secret number.
pub const WHISPER_SECRET: u64 = 0x5EC2_E7AB_CDEF_0123;

pub const TICK_MS: u32 = 16;
pub const SEED: u64 = 0x010B_B1E5;

pub fn room() -> RoomCode {
    RoomCode::parse("MEET42").unwrap()
}

pub fn net_config() -> Config {
    let mut config = Config::new(room());
    config.timeout_ms = 2000;
    config.connect_timeout_ms = 4000;
    config
}

pub const TYPES: &[&str] = &["Transform", "NetOwner", "Avatar"];

pub fn move_id() -> u32 {
    action_id("move")
}

/// Registers what every world of the game has, in the same order.
pub fn register(world: &mut World) {
    world.register_component::<Transform>().unwrap();
    wish_replicate::register(world).unwrap();
    wish_lobby::register(world).unwrap();
    world.register_resource::<ActionState>().unwrap();
    world.register_resource::<Players>().unwrap();
    world.register_resource::<PlayerActionState>().unwrap();
    world.register_component::<Avatar>().unwrap();
    world.register_resource::<Meeting>().unwrap();
    world.register_resource::<Heard>().unwrap();
    world.register_event::<ChatSay>().unwrap();
    world.register_event::<ChatLine>().unwrap();
    world.register_event::<Vote>().unwrap();
    world.register_event::<MeetingCalled>().unwrap();
    world.register_event::<VoteResult>().unwrap();
    world.register_event::<Whisper>().unwrap();
    world.register_event::<Point>().unwrap();
}

/// The project's actions at rest.
pub fn idle_actions() -> ActionState {
    let mut state = ActionState::default();
    state.slots[0] = ActionSlot {
        id: move_id(),
        ..ActionSlot::default()
    };
    state
}

fn fold(digest: u64, speaker: PlayerId, text: &str) -> u64 {
    let mut hasher = StableHasher::new();
    hasher.write_u64(digest);
    hasher.write_u64(u64::from(speaker));
    hasher.write_str(text);
    hasher.finish().as_u128() as u64
}

/// Seats an avatar per seated player (host only).
fn seat_avatars(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    let Some(lobby) = world.resource::<Lobby>().copied() else {
        return Ok(());
    };
    if !world.resource::<LocalPlayer>().is_some_and(|l| l.host) {
        return Ok(());
    }
    let have: Vec<u32> = world.query::<&Avatar>()?.map(|a| a.seat).collect();
    for (seat, state) in lobby.seats.iter().enumerate() {
        let seat = seat as u32;
        if state.taken && !have.contains(&seat) {
            let entity = world.spawn()?;
            world.insert(
                entity,
                Transform::from_translation(Vec3::new(seat as f32 * 2.0, 0.0, 0.0)),
            )?;
            world.insert(entity, Replicated::default())?;
            world.insert(entity, NetOwner { player: seat })?;
            world.insert(entity, Avatar { seat })?;
        }
    }
    Ok(())
}

/// Avatars walk by their player's `move` action.
fn walk(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    let actions = world.resource::<PlayerActionState>().copied();
    let id = move_id();
    for (owner, mut transform) in
        world.query_filtered::<(&NetOwner, &mut Transform), With<Avatar>>()?
    {
        let slot = actions
            .as_ref()
            .and_then(|a| a.players.get(owner.player as usize))
            .and_then(|p: &PlayerActionSlots| p.slots.iter().find(|s| s.id == id).copied());
        if let Some(slot) = slot
            && slot.held
        {
            transform.translation += Vec3::new(slot.x, slot.y, 0.0) * (3.0 / 60.0);
        }
    }
    Ok(())
}

/// The host's game rules: the meeting.
fn meeting(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let tick = ctx.time.tick;
    let Some(lobby) = world.resource::<Lobby>().copied() else {
        return Ok(());
    };
    let Some(local) = world.resource::<LocalPlayer>().copied() else {
        return Ok(());
    };
    if !local.host {
        return Ok(());
    }
    let mut state = world.resource::<Meeting>().copied().unwrap_or_default();
    if lobby.phase != LobbyPhase::Running {
        world.insert_resource(state)?;
        return Ok(());
    }
    if state.round != lobby.round {
        state = Meeting {
            round: lobby.round,
            started: tick,
            ..Meeting::default()
        };
        // Tell each saboteur its partner.
        let roles = world.resource::<MatchRoles>().copied().unwrap_or_default();
        let saboteurs: Vec<PlayerId> = (0..MAX_PLAYERS as PlayerId)
            .filter(|p| roles.roles[*p as usize] == 1)
            .collect();
        for p in &saboteurs {
            let partner = saboteurs.iter().copied().find(|q| q != p).unwrap_or(*p);
            world.send(Whisper {
                net: Net::to_player(*p),
                partner,
                secret: WHISPER_SECRET,
            })?;
        }
    }
    let seated = |p: PlayerId| {
        lobby
            .seats
            .get(p as usize)
            .is_some_and(|s| s.taken && !s.left)
    };
    for say in world.events_since::<ChatSay>(ctx.last_run)? {
        if !say.net.delivered {
            continue;
        }
        if !seated(say.net.from) {
            state.strangers += 1;
            continue;
        }
        if state.called && !state.done {
            let text = world.text(say.text).unwrap_or_default().to_owned();
            state.lines += 1;
            state.digest = fold(state.digest, say.net.from, &text);
            world.send(ChatLine {
                net: Net::to_everyone(),
                speaker: say.net.from,
                text: say.text,
            })?;
        }
    }
    for vote in world.events_since::<Vote>(ctx.last_run)? {
        if vote.net.delivered
            && state.called
            && !state.done
            && seated(vote.net.from)
            && let Some(slot) = state.votes.get_mut(vote.net.from as usize)
        {
            *slot = vote.choice + 1;
        }
    }
    let avatars: BTreeMap<Entity, u32> = world
        .query::<(Entity, &Avatar)>()?
        .map(|(e, a)| (e, a.seat))
        .collect();
    for point in world.events_since::<Point>(ctx.last_run)? {
        if point.net.delivered {
            state.points += 1;
            if avatars.get(&point.at) == Some(&point.net.from) {
                state.points_own += 1;
            }
        }
    }
    if !state.called && tick >= state.started + 40 {
        state.called = true;
        state.called_at = tick;
        world.send(MeetingCalled {
            net: Net::to_everyone(),
            by: 0,
        })?;
    }
    if state.called && !state.done && tick >= state.called_at + 200 {
        state.done = true;
        let mut counts = [0u32; MAX_PLAYERS];
        for vote in state.votes {
            if vote > 0 {
                counts[(vote - 1) as usize % MAX_PLAYERS] += 1;
            }
        }
        let most = counts.iter().copied().max().unwrap_or(0);
        let ejected = counts.iter().position(|c| *c == most).unwrap_or(0) as PlayerId;
        world.send(VoteResult {
            net: Net::to_everyone(),
            ejected,
            votes: state.votes,
        })?;
    }
    world.insert_resource(state)?;
    Ok(())
}

/// Every machine's player: chats and votes in the meeting, points at its own
/// avatar, and keeps what it heard.
fn bot(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let tick = ctx.time.tick;
    let Some(local) = world.resource::<LocalPlayer>().copied() else {
        return Ok(());
    };
    let mut heard = world.resource::<Heard>().copied().unwrap_or_default();
    for called in world.events_since::<MeetingCalled>(ctx.last_run)? {
        if called.net.delivered {
            heard.called_at = tick + 1;
            if called.net.from != 0 {
                heard.wrong_sender += 1;
            }
        }
    }
    for line in world.events_since::<ChatLine>(ctx.last_run)? {
        if line.net.delivered {
            let text = world.text(line.text).unwrap_or_default().to_owned();
            heard.lines += 1;
            heard.digest = fold(heard.digest, line.speaker, &text);
            if line.net.from != 0 {
                heard.wrong_sender += 1;
            }
        }
    }
    for result in world.events_since::<VoteResult>(ctx.last_run)? {
        if result.net.delivered {
            heard.results += 1;
            heard.ejected = result.ejected;
        }
    }
    for whisper in world.events_since::<Whisper>(ctx.last_run)? {
        if whisper.net.delivered {
            heard.whispers += 1;
            heard.partner = whisper.partner;
        }
    }
    if local.joined && heard.called_at > 0 {
        let since = tick + 1 - heard.called_at;
        let seat = u64::from(local.player);
        for k in 0..3u64 {
            if since == 5 + 2 * seat + 25 * k {
                let text = world.intern(&format!("player {seat} says {k}"))?;
                world.send(ChatSay {
                    net: Net::to_host(),
                    text,
                })?;
            }
        }
        if since == 90 + seat {
            world.send(Vote {
                net: Net::to_host(),
                choice: (local.player % 2) + 1,
            })?;
        }
        if since == 100 + seat {
            let own = world
                .query::<(Entity, &Avatar)>()?
                .find(|(_, a)| a.seat == local.player)
                .map(|(e, _)| e)
                .unwrap_or(Entity::NULL);
            world.send(Point {
                net: Net::to_host(),
                at: own,
            })?;
        }
    }
    world.insert_resource(heard)?;
    Ok(())
}

/// The game's app: the same for the host and every player.
pub fn game_app() -> App {
    let mut app = App::new(AppConfig {
        seed: 11,
        tick_rate_hz: 60,
        ..AppConfig::default()
    });
    register(app.world_mut());
    app.world_mut().insert_resource(idle_actions()).unwrap();
    app.add_system(System::new("seat_avatars", seat_avatars))
        .unwrap();
    app.add_system(System::new("walk", walk).after("seat_avatars"))
        .unwrap();
    app.add_system(System::new("meeting", meeting).after("walk"))
        .unwrap();
    app.add_system(System::new("bot", bot).after("meeting"))
        .unwrap();
    app
}

/// The host's settings: two saboteurs, the rest crew.
pub fn lobby_config(min_players: u32) -> LobbyConfig {
    LobbyConfig {
        seed: SEED,
        host_name: "Host".into(),
        min_players,
        roles: vec![RoleConfig::new(CREW, 0), RoleConfig::new(SABOTEUR, 2)],
        rejoin_grace_ticks: 600,
        ..LobbyConfig::default()
    }
}

/// What bot `seat` does with its avatar at `tick`.
pub fn walk_input(seat: PlayerId, tick: u64) -> Vec<(u32, [f32; 2])> {
    if (tick / 30 + u64::from(seat)).is_multiple_of(3) {
        vec![(move_id(), [1.0, 0.0])]
    } else if tick % 50 < 10 {
        vec![(move_id(), [0.0, -1.0])]
    } else {
        Vec::new()
    }
}

/// One player's machine.
pub struct Player {
    pub endpoint: Endpoint<SimTransport>,
    pub session: ClientSession,
    /// The machine's world, or its mirror when it predicts.
    pub app: App,
    /// The prediction, where the game runs, when the machine predicts.
    pub predictor: Option<Predictor>,
    /// Messages the game sent again in ticks a correction played again
    /// (the session must not send them twice).
    pub replayed_sends: u64,
    pub seed: u64,
    pub name: String,
    pub color: u32,
}

impl Player {
    pub fn addr(&self) -> SocketAddr {
        self.endpoint.transport().local_addr()
    }

    /// The world the machine's game runs in.
    pub fn world(&self) -> &World {
        match &self.predictor {
            Some(predictor) => predictor.world(),
            None => self.app.world(),
        }
    }

    pub fn heard(&self) -> Heard {
        self.world()
            .resource::<Heard>()
            .copied()
            .unwrap_or_default()
    }

    pub fn local(&self) -> LocalPlayer {
        self.world()
            .resource::<LocalPlayer>()
            .copied()
            .unwrap_or_default()
    }
}

/// A host and its players over a simulated network.
pub struct Party {
    pub net: SimNetwork,
    pub endpoint: Endpoint<SimTransport>,
    pub host: HostSession,
    pub app: App,
    pub players: Vec<Player>,
    pub host_seed: u64,
    /// The host's world hash after every tick, by tick.
    pub hashes: BTreeMap<u64, u128>,
    /// Every player's world hash after every tick.
    pub player_hashes: Vec<Vec<u128>>,
    /// Whether the players walk.
    pub walking: bool,
    /// Whether players who join from now on predict.
    pub predicting: bool,
}

/// Counts the game's messages in ticks a correction plays again.
struct CountReplays<'a> {
    sends: &'a mut u64,
}

impl TickHook for CountReplays<'_> {
    fn after(
        &mut self,
        world: &World,
        _: u64,
        again: bool,
        mark: ChangeTick,
        _: &dyn Fn(Entity) -> u32,
    ) -> Result<()> {
        if again {
            let sent = world.events_since::<ChatSay>(mark)?.len()
                + world.events_since::<Vote>(mark)?.len()
                + world.events_since::<Point>(mark)?.len();
            *self.sends += sent as u64;
        }
        Ok(())
    }
}

impl Party {
    pub fn new(seed: u64, conditions: LinkConditions, config: LobbyConfig) -> Self {
        let net = SimNetwork::new(seed, TICK_MS);
        net.set_conditions(conditions);
        let host_seed = seed ^ 0xA5;
        let endpoint =
            Endpoint::host(net.add_node(), net_config(), Entropy::seeded(host_seed)).unwrap();
        let app = game_app();
        let host = HostSession::new(
            app.world(),
            config,
            ReplicationConfig::new(TYPES),
            room().as_str(),
            Entropy::seeded(host_seed ^ 0x70C3),
        )
        .unwrap();
        Self {
            net,
            endpoint,
            host,
            app,
            players: Vec::new(),
            host_seed,
            hashes: BTreeMap::new(),
            player_hashes: Vec::new(),
            walking: true,
            predicting: false,
        }
    }

    /// One more player asking for `name` and `color` (it connects over the
    /// next ticks).
    pub fn join(&mut self, name: &str, color: u32) -> usize {
        let index = self.players.len();
        let seed = self.host_seed ^ (0x100 + index as u64);
        let app = game_app();
        let session = ClientSession::new(
            app.world(),
            ClientSessionConfig {
                name: name.to_owned(),
                color,
                ..ClientSessionConfig::default()
            },
        );
        let endpoint = Endpoint::connect(
            self.net.add_node(),
            self.endpoint.local_addr(),
            net_config(),
            Entropy::seeded(seed),
        )
        .unwrap();
        let predictor = self
            .predicting
            .then(|| Predictor::new(game_app(), PredictConfig::new(TICK_MS * 1000)).unwrap());
        self.players.push(Player {
            endpoint,
            session,
            app,
            predictor,
            replayed_sends: 0,
            seed,
            name: name.to_owned(),
            color,
        });
        self.player_hashes.push(Vec::new());
        index
    }

    /// Player `index` connects again on a new endpoint (a rejoin).
    pub fn reconnect(&mut self, index: usize, salt: u64) {
        let seed = self.host_seed ^ (0x900 + index as u64 + salt);
        let player = &mut self.players[index];
        player.session.rejoin(player.app.world_mut());
        player.endpoint = Endpoint::connect(
            self.net.add_node(),
            self.endpoint.local_addr(),
            net_config(),
            Entropy::seeded(seed),
        )
        .unwrap();
        player.seed = seed;
    }

    /// One tick of the whole party.
    pub fn step(&mut self) {
        self.net.step();
        let now = self.net.now_ms();
        self.endpoint.update(now);
        let tick = self.app.tick();
        let actions = if self.walking {
            walk_input(0, tick)
        } else {
            Vec::new()
        };
        let mark = self
            .host
            .begin_tick(self.app.world_mut(), &mut self.endpoint, tick, &actions)
            .unwrap();
        let report = self.app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        self.host
            .end_tick(self.app.world_mut(), &mut self.endpoint, mark)
            .unwrap();
        self.endpoint.update(now);
        self.hashes.insert(tick, self.app.world().hash().as_u128());
        for (index, player) in self.players.iter_mut().enumerate() {
            player.endpoint.update(now);
            if let Some(predictor) = player.predictor.as_mut() {
                // A predicting machine: the host's news into the mirror,
                // then a frame of the prediction (the game runs there).
                player
                    .session
                    .begin_tick(player.app.world_mut(), &mut player.endpoint)
                    .unwrap();
                let input = match player.session.seat() {
                    Some(seat) if self.walking => {
                        walk_input(seat, predictor.tick().map_or(0, |t| t + 1))
                    }
                    _ => Vec::new(),
                };
                let mut count = CountReplays {
                    sends: &mut player.replayed_sends,
                };
                let report = player
                    .session
                    .predict(predictor, &mut player.endpoint, now, &input, &mut count)
                    .unwrap();
                assert!(report.failures.is_empty(), "{:?}", report.failures);
                player.endpoint.update(now);
                self.player_hashes[index].push(predictor.hash().as_u128());
                continue;
            }
            let mark = player
                .session
                .begin_tick(player.app.world_mut(), &mut player.endpoint)
                .unwrap();
            let own = player.app.tick();
            let report = player.app.step().unwrap();
            assert!(report.failures.is_empty(), "{:?}", report.failures);
            player
                .session
                .end_tick(player.app.world(), &mut player.endpoint, mark, own)
                .unwrap();
            if let Some(seat) = player.session.seat() {
                let stamp = player.session.replication().stamp();
                let input = if self.walking {
                    walk_input(seat, stamp)
                } else {
                    Vec::new()
                };
                player
                    .session
                    .replication_mut()
                    .send_input(&mut player.endpoint, input)
                    .unwrap();
            }
            player.endpoint.update(now);
            self.player_hashes[index].push(player.app.world().hash().as_u128());
        }
    }

    pub fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Runs until `done` holds (at most `limit` ticks); returns the ticks
    /// taken.
    pub fn run_until(&mut self, limit: u64, done: impl Fn(&Party) -> bool) -> u64 {
        for taken in 0..limit {
            if done(self) {
                return taken;
            }
            self.step();
        }
        panic!("not done after {limit} ticks");
    }

    /// Every player seated (the host knows them all and each knows its
    /// seat).
    pub fn all_seated(&self) -> bool {
        self.players.iter().all(|p| {
            p.session.seat().is_some()
                && p.local().joined
                && p.world()
                    .resource::<Lobby>()
                    .is_some_and(|l| l.phase == LobbyPhase::Open)
        })
    }

    pub fn host_lobby(&self) -> Lobby {
        self.app.world().resource::<Lobby>().copied().unwrap()
    }

    pub fn meeting(&self) -> Meeting {
        self.app
            .world()
            .resource::<Meeting>()
            .copied()
            .unwrap_or_default()
    }

    /// The name of seat `seat` in `world`'s lobby.
    pub fn name_in(world: &World, seat: PlayerId) -> String {
        let lobby = world.resource::<Lobby>().copied().unwrap();
        world
            .text(lobby.seats[seat as usize].name)
            .unwrap_or_default()
            .to_owned()
    }

    /// Joins `names.len()` players, readies everyone and starts the match.
    pub fn fill_and_start(&mut self, names: &[(&str, u32)]) {
        for (name, color) in names {
            self.join(name, *color);
        }
        self.run_until(600, Party::all_seated);
        for player in &mut self.players {
            player
                .session
                .set_ready(&mut player.endpoint, true)
                .unwrap();
        }
        let count = self.players.len();
        self.run_until(600, |p| {
            let lobby = p.host.lobby();
            (1..=count).all(|s| lobby.seats[s].ready)
        });
        self.host.start().unwrap();
        self.run_until(600, |p| {
            p.players.iter().all(|pl| {
                pl.world()
                    .resource::<Lobby>()
                    .is_some_and(|l| l.phase == LobbyPhase::Running)
            })
        });
    }

    /// Plays the host's match log into a fresh host app and checks its world
    /// hash against the live host's at every tick.
    pub fn check_replay(&self) -> usize {
        let log = self.host.log();
        let log = wish_lobby::MatchLog::from_bytes(&log.to_bytes()).unwrap();
        assert_eq!(&log, self.host.log(), "the log survives its bytes");
        let mut app = game_app();
        let mut carry = wish_lobby::Carry::new();
        let mut checked = 0;
        for tick in log.ticks() {
            assert_eq!(app.tick(), tick);
            let (_, problems) = apply(app.world_mut(), &log.record(tick), &mut carry).unwrap();
            assert!(problems.is_empty(), "{problems:?}");
            let report = app.step().unwrap();
            assert!(report.failures.is_empty(), "{:?}", report.failures);
            assert_eq!(
                app.world().hash().as_u128(),
                self.hashes[&tick],
                "the replay differs from the live host at tick {tick}"
            );
            checked += 1;
        }
        checked
    }
}

/// The plaintext of everything the host sent to one player, as that player's
/// keys open it, and the whole reliable messages.
#[derive(Default)]
pub struct Stream {
    pub reliable: BTreeMap<u32, (u16, u16, Vec<u8>)>,
    pub plaintext: Vec<u8>,
}

impl Stream {
    /// Every whole reliable message, in order.
    pub fn messages(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for (&seq, (index, count, bytes)) in &self.reliable {
            if *index != 0 {
                continue;
            }
            let mut message = bytes.clone();
            let mut whole = true;
            for k in 1..u32::from(*count) {
                match self.reliable.get(&(seq + k)) {
                    Some((i, _, more)) if u32::from(*i) == k => message.extend_from_slice(more),
                    _ => whole = false,
                }
            }
            if whole {
                out.push(message);
            }
        }
        out
    }
}

/// Opens every captured host → player datagram with keys rebuilt from the
/// seeds.
pub fn sniff(party: &Party, captured: &[Captured]) -> BTreeMap<SocketAddr, Stream> {
    let host_keys = KeyPair::generate(&mut Entropy::seeded(party.host_seed));
    let host_public = host_keys.public();
    assert_eq!(host_public, party.endpoint.public_key());
    let host_addr = party.endpoint.local_addr();
    let mut client_keys: BTreeMap<SocketAddr, [u8; 32]> = BTreeMap::new();
    for datagram in captured {
        if datagram.to != host_addr {
            continue;
        }
        if let Some(Packet::Hello { client_key, .. } | Packet::Connect { client_key, .. }) =
            net_wire::decode(&datagram.bytes)
        {
            client_keys.insert(datagram.from, client_key);
        }
    }
    let mut streams: BTreeMap<SocketAddr, Stream> = BTreeMap::new();
    for datagram in captured {
        if datagram.from != host_addr {
            continue;
        }
        let Some(Packet::Data {
            number,
            header,
            sealed,
            ..
        }) = net_wire::decode(&datagram.bytes)
        else {
            continue;
        };
        let client = client_keys[&datagram.to];
        let shared = host_keys.agree(&client).unwrap();
        let keys = SessionKeys::client(&shared, &client, &host_public);
        let mut body = sealed.to_vec();
        let len = keys
            .open(number, header, &mut body)
            .expect("every data packet of the host opens with the player's keys");
        let stream = streams.entry(datagram.to).or_default();
        let text = &body[..len];
        stream.plaintext.extend_from_slice(text);
        let mut frames = Vec::new();
        assert!(net_wire::decode_frames(text, &mut frames));
        for frame in frames {
            if let Frame::Reliable {
                seq,
                index,
                count,
                bytes,
            } = frame
            {
                stream.reliable.insert(seq, (index, count, bytes.to_vec()));
            }
        }
    }
    streams
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
