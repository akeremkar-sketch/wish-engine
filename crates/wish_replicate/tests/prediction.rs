//! Prediction, reconciliation, interpolation and tick sync over the network
//! simulator (design note `docs/design/1.1-prediction.md` §9).
//!
//! A small deterministic game: avatars walk by their player's `move` action
//! at 3 m/s, and a crate is pushed out of an avatar's way. The host and
//! every client build their app with the same function (`game_app`), so the
//! client's prediction runs exactly the host's systems.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use wish_app::{App, AppConfig, System, SystemContext};
use wish_core::math::Vec3;
use wish_core::{Entity, Result, Rng};
use wish_ecs::{With, World};
use wish_net::{
    Channel, Config, Endpoint, Entropy, Event, LinkConditions, PeerId, RoomCode, SimNetwork,
    SimTransport,
};
use wish_replicate::wire::{self, Frame, Input};
use wish_replicate::{
    Client, ClientConfig, Host, PredictConfig, Predictor, ReplicationConfig, Source, View,
};
use wish_schema::component;
use wish_types::{
    ActionSlot, ActionState, NetOwner, NetViewer, PlayerActionSlots, PlayerActionState, PlayerId,
    Players, Replicated, Transform, action_id,
};

/// A player's body.
#[component]
pub struct Avatar {
    /// Its colour.
    pub color: u32,
}

/// A crate avatars push out of their way.
#[component]
pub struct Crate {
    /// How many ticks it was pushed.
    pub pushes: u32,
}

const TYPES: &[&str] = &["Transform", "NetOwner", "Avatar", "Crate"];
const TICK_MS: u32 = 16;
const SEED: u64 = 0x09E7_D1C7;
/// Where the crate starts: in player 1's way.
const CRATE_AT: Vec3 = Vec3::new(3.0, 5.0, 0.0);

fn room() -> RoomCode {
    RoomCode::parse("PREDKT").unwrap()
}

fn move_id() -> u32 {
    action_id("move")
}

fn register(world: &mut World) {
    world.register_component::<Transform>().unwrap();
    wish_replicate::register(world).unwrap();
    world.register_resource::<ActionState>().unwrap();
    world.register_resource::<Players>().unwrap();
    world.register_resource::<PlayerActionState>().unwrap();
    world.register_component::<Avatar>().unwrap();
    world.register_component::<Crate>().unwrap();
}

/// The project's actions at rest.
fn idle_actions() -> ActionState {
    let mut state = ActionState::default();
    state.slots[0] = ActionSlot {
        id: move_id(),
        ..ActionSlot::default()
    };
    state
}

/// Avatars walk by their player's `move` action at 3 m/s.
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

/// A crate an avatar overlaps (0.8 m boxes) is pushed out along the axis it
/// is furthest along.
fn push(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    let avatars: Vec<Vec3> = world
        .query_filtered::<&Transform, With<Avatar>>()?
        .map(|t| t.translation)
        .collect();
    for (mut pushed, mut transform) in world.query::<(&mut Crate, &mut Transform)>()? {
        for at in &avatars {
            let d = transform.translation - *at;
            if d.x.abs() < 0.8 && d.y.abs() < 0.8 {
                if d.x.abs() >= d.y.abs() {
                    transform.translation.x = at.x + 0.8f32.copysign(d.x);
                } else {
                    transform.translation.y = at.y + 0.8f32.copysign(d.y);
                }
                pushed.pushes += 1;
            }
        }
    }
    Ok(())
}

/// The game's app: the same for the host and for every client's
/// prediction.
fn game_app() -> App {
    let mut app = App::new(AppConfig {
        seed: 11,
        tick_rate_hz: 60,
        ..AppConfig::default()
    });
    register(app.world_mut());
    app.world_mut().insert_resource(idle_actions()).unwrap();
    app.add_system(System::new("walk", walk)).unwrap();
    app.add_system(System::new("push", push).after("walk"))
        .unwrap();
    app
}

/// Where player `p`'s avatar starts.
fn start_of(player: PlayerId) -> Vec3 {
    match player {
        1 => Vec3::new(0.0, 5.0, 0.0),
        p => Vec3::new(0.0, 20.0 * p as f32, 0.0),
    }
}

type Bot = fn(PlayerId, u64) -> Vec<(u32, [f32; 2])>;

/// What bot `player` does at tick `tick`: player 1 walks into the crate and
/// pushes it, then walks back; player 2 paces up and down; player 3 walks
/// away diagonally; all stop at tick 340.
fn bot(player: PlayerId, tick: u64) -> Vec<(u32, [f32; 2])> {
    let id = move_id();
    match player {
        1 if tick < 220 => vec![(id, [1.0, 0.0])],
        1 if tick < 340 => vec![(id, [-1.0, 0.0])],
        2 if tick < 340 => {
            let y = if (tick / 40).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            vec![(id, [0.0, y])]
        }
        3 if tick < 340 => vec![(id, [0.5, 0.5])],
        _ => Vec::new(),
    }
}

fn host_app(players: u32) -> (App, Entity) {
    let mut app = game_app();
    let world = app.world_mut();
    for player in 1..=players {
        let avatar = world.spawn().unwrap();
        world
            .insert(avatar, Transform::from_translation(start_of(player)))
            .unwrap();
        world.insert(avatar, Replicated::default()).unwrap();
        world.insert(avatar, NetOwner { player }).unwrap();
        world.insert(avatar, NetViewer { player }).unwrap();
        world.insert(avatar, Avatar { color: player }).unwrap();
    }
    let crate_ = world.spawn().unwrap();
    world
        .insert(crate_, Transform::from_translation(CRATE_AT))
        .unwrap();
    world.insert(crate_, Replicated::default()).unwrap();
    world.insert(crate_, Crate { pushes: 0 }).unwrap();
    // Host ticks start at 1.
    app.set_tick(1);
    (app, crate_)
}

struct Peer {
    endpoint: Endpoint<SimTransport>,
    client: Client,
    mirror: World,
    predictor: Predictor,
}

struct Game {
    net: SimNetwork,
    endpoint: Endpoint<SimTransport>,
    host: Host,
    app: App,
    crate_: Entity,
    peers: Vec<Peer>,
    bot: Bot,
    frames: u64,
    /// The host's target of every player at every tick.
    records: BTreeMap<(PlayerId, u64), View>,
    /// Each client's prediction of each tick, as first predicted.
    predictions: BTreeMap<(PlayerId, u64), View>,
    /// Per frame, every client's (mirror world hash, prediction app hash).
    hashes: Vec<Vec<(u128, u128)>>,
    /// Per frame, every connected player's input slack at the host.
    slack: Vec<BTreeMap<PlayerId, i64>>,
}

fn config() -> PredictConfig {
    PredictConfig::new(TICK_MS * 1000)
}

impl Game {
    fn new(seed: u64, conditions: LinkConditions, clients: u32) -> Self {
        Self::with(seed, conditions, clients, clients)
    }

    /// A host whose world has `avatars` players' avatars, and `clients`
    /// clients.
    fn with(seed: u64, conditions: LinkConditions, avatars: u32, clients: u32) -> Self {
        let net = SimNetwork::new(seed, TICK_MS);
        net.set_conditions(conditions);
        let (app, crate_) = host_app(avatars);
        let endpoint = Endpoint::host(
            net.add_node(),
            Config::new(room()),
            Entropy::seeded(seed ^ 0xA5),
        )
        .unwrap();
        let host = Host::new(app.world(), ReplicationConfig::new(TYPES)).unwrap();
        let mut game = Self {
            net,
            endpoint,
            host,
            app,
            crate_,
            peers: Vec::new(),
            bot,
            frames: 0,
            records: BTreeMap::new(),
            predictions: BTreeMap::new(),
            hashes: Vec::new(),
            slack: Vec::new(),
        };
        for index in 0..clients {
            game.join(seed ^ (0x100 + u64::from(index)));
        }
        game
    }

    fn join(&mut self, seed: u64) {
        let mut mirror = World::new();
        register(&mut mirror);
        let endpoint = Endpoint::connect(
            self.net.add_node(),
            self.endpoint.local_addr(),
            Config::new(room()),
            Entropy::seeded(seed),
        )
        .unwrap();
        self.peers.push(Peer {
            endpoint,
            client: Client::new(ClientConfig::default()),
            mirror,
            predictor: Predictor::new(game_app(), config()).unwrap(),
        });
    }

    /// The host's half of one frame.
    fn host_step(&mut self) {
        let now = self.net.now_ms();
        self.endpoint.update(now);
        drop(self.host.pump(self.app.world_mut(), &mut self.endpoint));
        let tick = self.app.tick();
        self.host.apply_inputs(self.app.world_mut(), tick).unwrap();
        let report = self.app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        self.host
            .send(self.app.world(), tick, &mut self.endpoint)
            .unwrap();
        self.endpoint.update(now);
        let mut slack = BTreeMap::new();
        for (_, player) in self.host.players() {
            if let Some(target) = self.host.target(player) {
                self.records.insert((player, tick), target.clone());
            }
            if let Some(s) = self.host.input_slack(player) {
                slack.insert(player, s);
            }
        }
        self.slack.push(slack);
    }

    /// One client's half of one frame.
    fn client_step(&mut self, index: usize) {
        let now = self.net.now_ms();
        let bot = self.bot;
        let peer = &mut self.peers[index];
        peer.endpoint.update(now);
        drop(peer.client.pump(&mut peer.mirror, &mut peer.endpoint));
        let player = peer.client.player();
        let next = peer.predictor.tick().map_or(0, |t| t + 1);
        let actions = player.map(|p| bot(p, next)).unwrap_or_default();
        let before = peer.predictor.tick();
        let report = peer
            .predictor
            .frame(&mut peer.client, &mut peer.endpoint, now, &actions)
            .unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        if report.corrected && std::env::var("PREDICT_DEBUG").is_ok() {
            println!(
                "frame {} player {:?}: corrected at confirmed {:?}, predicted {:?}, input ack {:?}, late {}",
                self.frames,
                player,
                peer.predictor.confirmed_tick(),
                peer.predictor.tick(),
                peer.client.input_ack(),
                self.host.stats().late_frames
            );
        }
        assert!(
            peer.predictor.tick() >= before,
            "the predicted tick went back from {before:?} to {:?}",
            peer.predictor.tick()
        );
        if let (Some(player), Some(tick)) = (player, peer.predictor.tick())
            && report.steps > 0
        {
            self.predictions
                .entry((player, tick))
                .or_insert_with(|| peer.predictor.predicted_view().unwrap());
        }
        peer.endpoint.update(now);
    }

    fn step(&mut self) {
        self.net.step();
        self.host_step();
        for index in 0..self.peers.len() {
            self.client_step(index);
        }
        self.frames += 1;
        self.hashes.push(
            self.peers
                .iter()
                .map(|p| (p.mirror.hash().as_u128(), p.predictor.hash().as_u128()))
                .collect(),
        );
    }

    fn run(&mut self, frames: u64) {
        for _ in 0..frames {
            self.step();
        }
    }

    fn peer(&self, player: PlayerId) -> &Peer {
        self.peers
            .iter()
            .find(|p| p.client.player() == Some(player))
            .unwrap_or_else(|| panic!("no client is player {player}"))
    }

    /// Every recorded prediction of `player` with a host record of the
    /// same tick: `(ticks compared, ticks that differed)`.
    fn compare(&self, player: PlayerId) -> (u64, u64) {
        let (mut compared, mut differed) = (0, 0);
        for ((p, tick), predicted) in &self.predictions {
            if *p != player {
                continue;
            }
            let Some(record) = self.records.get(&(player, *tick)) else {
                continue;
            };
            compared += 1;
            if predicted
                .iter()
                .any(|(net, state)| record.get(net) != Some(state))
            {
                differed += 1;
            }
        }
        (compared, differed)
    }

    /// The crate's network id for `player`.
    fn crate_net(&self, player: PlayerId) -> u32 {
        self.host
            .network_ids(player)
            .into_iter()
            .find(|(_, e)| *e == self.crate_)
            .map(|(n, _)| n)
            .unwrap()
    }

    /// Ticks at which `player`'s prediction carried the crate while it had
    /// been pushed.
    fn crate_predicted_pushed(&self, player: PlayerId) -> usize {
        let net = self.crate_net(player);
        let crate_ty = TYPES.iter().position(|t| *t == "Crate").unwrap() as u16;
        self.predictions
            .iter()
            .filter(|((p, _), view)| {
                *p == player
                    && view
                        .get(&net)
                        .and_then(|s| s.get(&crate_ty))
                        .is_some_and(|v| v.bytes.iter().any(|b| *b != 0))
            })
            .count()
    }

    fn late_frames(&self) -> u64 {
        self.host.stats().late_frames
    }
}

#[test]
fn with_nothing_lost_the_prediction_equals_the_host_bit_for_bit() {
    let conditions = LinkConditions {
        latency_ms: 100,
        ..LinkConditions::PERFECT
    };
    let mut game = Game::new(SEED, conditions, 3);
    game.run(480);
    for player in 1..=3 {
        let peer = game.peer(player);
        let stats = peer.predictor.stats();
        let (compared, differed) = game.compare(player);
        println!(
            "player {player}: {stats:?}; sync {:?}; compared {compared}",
            peer.predictor.sync()
        );
        assert_eq!(stats.corrections, 0, "player {player} was corrected");
        assert_eq!(differed, 0, "player {player}'s prediction differed");
        assert!(compared > 300, "only {compared} ticks compared");
        assert!(stats.exact > 300, "{stats:?}");
        assert_eq!(stats.write_failures, 0);
    }
    // The crate was pushed by player 1, on the host and in its prediction.
    let pushed = game.app.world().get::<Crate>(game.crate_).unwrap().pushes;
    assert!(pushed > 50, "the crate was pushed {pushed} times");
    assert!(game.crate_predicted_pushed(1) > 50);
    // Tick sync kept every input in time.
    assert_eq!(game.late_frames(), 0, "{:?}", game.host.stats());
    // Moving players were drawn from their prediction.
    let peer = game.peer(1);
    let net = game
        .host
        .network_ids(1)
        .into_iter()
        .find(|(_, e)| {
            game.app
                .world()
                .get::<NetOwner>(*e)
                .is_some_and(|o| o.player == 1)
        })
        .unwrap()
        .0;
    assert_eq!(peer.predictor.presented()[&net].source, Source::Predicted);
}

/// Per-frame observations of one player's client.
#[derive(Default)]
struct Watch {
    corrections: Vec<u64>,
    exact: Vec<u64>,
    changes: Vec<u64>,
    offsets: Vec<f32>,
}

impl Watch {
    fn take(&mut self, game: &Game, player: PlayerId) {
        let Some(peer) = game
            .peers
            .iter()
            .find(|p| p.client.player() == Some(player))
        else {
            self.corrections.push(0);
            self.exact.push(0);
            self.changes.push(0);
            self.offsets.push(0.0);
            return;
        };
        let stats = peer.predictor.stats();
        self.corrections.push(stats.corrections);
        self.exact.push(stats.exact);
        self.changes.push(stats.membership_changes);
        self.offsets.push(peer.predictor.largest_offset());
        if std::env::var("PREDICT_DEBUG").is_ok() && player == 1 {
            for (net, shown) in peer.predictor.presented() {
                if shown.offset != Vec3::ZERO {
                    println!(
                        "frame {}: net {net} {:?} offset {:?} predicted {:?}",
                        self.offsets.len() - 1,
                        shown.source,
                        shown.offset,
                        peer.predictor.predicted()
                    );
                }
            }
        }
    }

    /// The most frames from a correction to the next exact confirmation.
    fn frames_to_exact(&self) -> u64 {
        let mut worst = 0;
        for frame in 1..self.corrections.len() {
            if self.corrections[frame] == self.corrections[frame - 1] {
                continue;
            }
            let exact = self.exact[frame];
            let next = (frame + 1..self.exact.len()).find(|f| self.exact[*f] > exact);
            if let Some(next) = next {
                worst = worst.max((next - frame) as u64);
            }
        }
        worst
    }

    /// The most frames an offset stayed after the last correction or
    /// change of what is predicted.
    fn frames_to_settle(&self) -> u64 {
        let mut worst = 0;
        let mut since = 0u64;
        for frame in 0..self.offsets.len() {
            let event = frame > 0
                && (self.corrections[frame] != self.corrections[frame - 1]
                    || self.changes[frame] != self.changes[frame - 1]);
            since = if event { 0 } else { since + 1 };
            if self.offsets[frame] > 0.0 {
                if since > 42 && std::env::var("PREDICT_DEBUG").is_ok() {
                    println!(
                        "frame {frame}: offset {} {since} frames after",
                        self.offsets[frame]
                    );
                }
                worst = worst.max(since);
            }
        }
        worst
    }
}

#[test]
fn over_a_bad_network_corrections_are_rare_and_converge() {
    let mut game = Game::new(SEED, LinkConditions::BAD, 3);
    let mut watches: BTreeMap<PlayerId, Watch> = (1..=3).map(|p| (p, Watch::default())).collect();
    let mut late_at = BTreeMap::new();
    for frame in 0..900u64 {
        game.step();
        for (player, watch) in &mut watches {
            watch.take(&game, *player);
        }
        if frame == 199 || frame == 899 {
            late_at.insert(frame, game.late_frames());
        }
    }
    let stats = game.net.stats();
    println!("network: {stats:?}; host: {:?}", game.host.stats());
    for (player, watch) in &watches {
        let peer = game.peer(*player);
        let s = peer.predictor.stats();
        let (compared, differed) = game.compare(*player);
        let to_exact = watch.frames_to_exact();
        let to_settle = watch.frames_to_settle();
        println!(
            "player {player}: {s:?}; sync {:?}; compared {compared}, differed {differed}; frames to exact {to_exact}, to settle {to_settle}",
            peer.predictor.sync()
        );
        // Rare: at most 2 % of the confirmations needed a correction.
        assert!(
            s.corrections * 50 <= s.confirmations,
            "player {player}: {s:?}"
        );
        assert!(s.confirmations > 500, "{s:?}");
        // Converges: the next confirmation after a correction is exact
        // within 8 frames, and the smoothing offset is gone within 42 frames
        // of the last correction or switch.
        assert!(to_exact <= 8, "player {player}: {to_exact} frames to exact");
        assert!(
            to_settle <= 42,
            "player {player}: {to_settle} frames to settle"
        );
        // After the bots stopped (tick 340) every prediction is the host's.
        let late_differ = game
            .predictions
            .iter()
            .filter(|((p, tick), predicted)| {
                *p == *player
                    && *tick > 600
                    && game.records.get(&(*p, *tick)).is_some_and(|record| {
                        predicted
                            .iter()
                            .any(|(net, state)| record.get(net) != Some(state))
                    })
            })
            .count();
        assert_eq!(late_differ, 0, "player {player}");
        assert_eq!(s.write_failures, 0);
    }
    // Late inputs are rare: at most 1 % of the frames sent after the start.
    let after_start = late_at[&899] - late_at[&199];
    assert!(after_start * 100 <= 700 * 3, "{after_start} late frames");
}

#[test]
fn the_same_seed_gives_the_same_client_hashes() {
    let run = |seed: u64| {
        let mut game = Game::new(seed, LinkConditions::BAD, 2);
        game.run(400);
        let stats: Vec<_> = game.peers.iter().map(|p| p.predictor.stats()).collect();
        (game.hashes, stats, game.net.delivery_hash())
    };
    let (a, a_stats, a_delivery) = run(SEED);
    let (b, b_stats, b_delivery) = run(SEED);
    assert_eq!(a_delivery, b_delivery);
    assert_eq!(a_stats, b_stats);
    assert_eq!(a.len(), b.len());
    for (frame, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(x, y, "the client hashes differ at frame {frame}");
    }
    let (c, _, _) = run(SEED + 1);
    assert_ne!(a, c, "another seed gives other hashes");
}

#[test]
fn tick_sync_follows_a_rise_in_latency_without_going_back() {
    let quiet = LinkConditions {
        latency_ms: 50,
        jitter_ms: 10,
        ..LinkConditions::PERFECT
    };
    let mut game = Game::new(SEED, quiet, 2);
    let mut leads = Vec::new();
    let mut late = Vec::new();
    for frame in 0..1000u64 {
        if frame == 300 {
            game.net.set_conditions(LinkConditions {
                latency_ms: 150,
                ..quiet
            });
        }
        game.step();
        leads.push(
            game.peers
                .iter()
                .find(|p| p.client.player() == Some(1))
                .map_or(0, |p| p.predictor.sync().lead),
        );
        late.push(game.late_frames());
    }
    let before = leads[290];
    let after = leads[990];
    println!(
        "lead before {before}, after {after}; late at 300 {}, 600 {}, 1000 {}; sync {:?}; {:?}",
        late[299],
        late[599],
        late[999],
        game.peer(1).predictor.sync(),
        game.peer(1).predictor.stats()
    );
    // The round trip rose by 200 ms, 12.5 ticks.
    assert!(after >= before + 10, "lead {before} -> {after}");
    assert_eq!(late[299], 0, "no late input before the rise");
    // Settled: no input late in the last 300 frames.
    assert_eq!(late[999], late[699], "late inputs after settling");
    // The rise itself made a few inputs late, never many.
    assert!(late[999] < 160, "{} late frames", late[999]);
    // The predicted tick never went back (checked every frame by `step`).
}

/// A modified client: it joined with real keys, and sends input for
/// another player, input stamped far ahead, garbage and resync floods.
struct Attacker {
    endpoint: Endpoint<SimTransport>,
    player: Option<PlayerId>,
    rng: Rng,
}

impl Attacker {
    fn step(&mut self, now: u64, host_tick: u64) {
        self.endpoint.update(now);
        while let Some(event) = self.endpoint.poll_event() {
            if let Event::Message { data, .. } = event
                && let Some(welcome) = wire::decode_welcome(&data)
            {
                self.player = Some(welcome.player);
            }
        }
        let Some(me) = self.player else {
            self.endpoint.update(now);
            return;
        };
        let forged = Input {
            player: 1,
            ack: None,
            ack_mask: 0,
            resync: true,
            frames: vec![Frame {
                tick: host_tick + 3,
                actions: vec![(move_id(), [0.0, -1.0])],
            }],
        };
        let ahead = Input {
            player: me,
            ack: Some(u64::MAX / 2),
            ack_mask: u64::MAX,
            resync: true,
            frames: vec![Frame {
                tick: host_tick + 1_000_000,
                actions: vec![(move_id(), [1.0, 1.0])],
            }],
        };
        let len = self.rng.below(48) as usize;
        let mut garbage: Vec<u8> = (0..len).map(|_| self.rng.next_u32() as u8).collect();
        if let Some(first) = garbage.first_mut() {
            *first = wire::kind::INPUT;
        }
        for message in [
            wire::encode_input(&forged),
            wire::encode_input(&ahead),
            garbage,
        ] {
            let _ = self
                .endpoint
                .send(PeerId::HOST, Channel::Unreliable, &message);
        }
        self.endpoint.update(now);
    }
}

#[test]
fn a_lagging_or_hostile_client_cannot_make_the_host_wait() {
    // Three avatars: the honest client, the lagging one and the attacker
    // each get one (ids go to whoever connects first).
    let mut game = Game::with(SEED, LinkConditions::PERFECT, 3, 2);
    let host_addr = game.endpoint.local_addr();
    let lag_addr = game.peers[1].endpoint.local_addr();
    let lag = LinkConditions {
        latency_ms: 750,
        jitter_ms: 40,
        loss: 0.3,
        ..LinkConditions::PERFECT
    };
    game.net.set_link(lag_addr, host_addr, lag);
    game.net.set_link(host_addr, lag_addr, lag);
    let mut attacker = Attacker {
        endpoint: Endpoint::connect(
            game.net.add_node(),
            host_addr,
            Config::new(room()),
            Entropy::seeded(0xBAD),
        )
        .unwrap(),
        player: None,
        rng: Rng::from_seed(0xF00D),
    };
    let mut codes = std::collections::BTreeSet::new();
    for frame in 0..1100u64 {
        game.step();
        // The host simulated exactly one tick this frame, whatever the
        // others did.
        assert_eq!(game.app.tick(), frame + 2);
        let now = game.net.now_ms();
        attacker.step(now, game.app.tick());
        for diagnostic in game.host.take_diagnostics() {
            codes.insert(diagnostic.code.to_string());
        }
    }
    println!("host: {:?}; diagnostics {codes:?}", game.host.stats());
    assert!(codes.contains("net_input_wrong_player"), "{codes:?}");
    assert!(codes.contains("net_input_out_of_window"), "{codes:?}");
    // The honest client is never corrected: the forged input moved nobody.
    let honest = game.peers[0].client.player().unwrap();
    let stats = game.peers[0].predictor.stats();
    let (compared, differed) = game.compare(honest);
    println!("honest player {honest}: {stats:?}; compared {compared}");
    assert_eq!(stats.corrections, 0);
    assert_eq!(differed, 0);
    assert!(compared > 700);
    // The lagging client is corrected while its tick sync learns its round
    // trip, and is exact once the bots stopped.
    let lagging = game.peers[1].client.player().unwrap();
    let lag_stats = game.peers[1].predictor.stats();
    println!(
        "lagging player {lagging}: {lag_stats:?}; sync {:?}",
        game.peers[1].predictor.sync()
    );
    assert!(lag_stats.exact > 100, "{lag_stats:?}");
    let wrong_late = game
        .predictions
        .iter()
        .filter(|((p, tick), predicted)| {
            *p == lagging
                && *tick > 800
                && game.records.get(&(*p, *tick)).is_some_and(|record| {
                    predicted
                        .iter()
                        .any(|(net, state)| record.get(net) != Some(state))
                })
        })
        .count();
    assert_eq!(wrong_late, 0);
}

/// Player 2 walks up forever; nobody else moves.
fn walker(player: PlayerId, _tick: u64) -> Vec<(u32, [f32; 2])> {
    if player == 2 {
        vec![(move_id(), [0.0, 1.0])]
    } else {
        Vec::new()
    }
}

/// Player 2's avatar's network id for player 1, and its `y` on the host at
/// every tick (from the host's targets for player 1).
fn walker_track(game: &Game) -> (u32, BTreeMap<u64, f32>) {
    let avatar = game
        .host
        .network_ids(1)
        .into_iter()
        .find(|(_, e)| {
            game.app
                .world()
                .get::<NetOwner>(*e)
                .is_some_and(|o| o.player == 2)
        })
        .unwrap()
        .0;
    let ty = TYPES.iter().position(|t| *t == "Transform").unwrap() as u16;
    let ys = game
        .records
        .iter()
        .filter(|((p, _), _)| *p == 1)
        .filter_map(|((_, tick), view)| {
            let t = wish_replicate::interpolate::transform_of(view.get(&avatar)?, ty)?;
            Some((*tick, t.translation.y))
        })
        .collect();
    (avatar, ys)
}

#[test]
fn others_are_interpolated_a_delay_behind_and_extrapolated_a_little() {
    let mut game = Game::new(SEED, LinkConditions::BAD, 2);
    game.bot = walker;
    let mut shown = Vec::new();
    for _ in 0..500 {
        game.step();
        let Some(peer) = game.peers.iter().find(|p| p.client.player() == Some(1)) else {
            continue;
        };
        if !game.host.players().iter().any(|(_, p)| *p == 2) {
            continue;
        }
        let (avatar, _) = walker_track(&game);
        if let Some(s) = peer.predictor.presented().get(&avatar) {
            shown.push((
                peer.predictor.display_time(),
                s.source,
                s.transform.translation,
            ));
        }
    }
    let (_, ys) = walker_track(&game);
    // From frame 120 on: drawn between the host's states around the display
    // time, moving every frame by about one tick's walk, never back.
    let mut errors = Vec::new();
    let mut interpolated = 0;
    for window in shown[120..].windows(2) {
        let (_, _, a) = window[0];
        let (at, source, b) = window[1];
        let step = b.y - a.y;
        assert!((-0.001..=0.11).contains(&step), "a step of {step} m");
        assert_eq!(b.x, 0.0);
        if source == Source::Interpolated {
            interpolated += 1;
            let at = at.unwrap();
            let (k, f) = (at / 1000, (at % 1000) as f32 / 1000.0);
            if let (Some(y0), Some(y1)) = (ys.get(&k), ys.get(&(k + 1))) {
                errors.push((b.y - (y0 + (y1 - y0) * f)).abs());
            }
        }
    }
    errors.sort_by(f32::total_cmp);
    let median = errors[errors.len() / 2];
    let worst = errors[errors.len() - 1];
    println!(
        "interpolated {interpolated} of {}; error median {median} m, worst {worst} m",
        shown.len() - 121
    );
    assert!(interpolated * 10 >= (shown.len() - 121) * 9);
    assert!(median < 0.001, "median error {median}");
    assert!(worst < 0.06, "worst error {worst}");
    // About the delay behind the host: 100 ms (6.25 ticks) after the
    // host's clock, which itself is one way (6.25 ticks) behind.
    let host_now = game.app.tick() - 1;
    let behind = host_now * 1000 - game.peer(1).predictor.display_time().unwrap();
    println!("display time {behind} thousandths of a tick behind the host");
    assert!((10_000..=16_000).contains(&behind), "{behind} behind");

    // The host's messages stop reaching player 1: it extrapolates for at
    // most 100 ms and then holds; its own prediction waits at max_ahead.
    let host_addr = game.endpoint.local_addr();
    let addr = game.peer(1).endpoint.local_addr();
    game.net.set_link(
        host_addr,
        addr,
        LinkConditions {
            loss: 1.0,
            ..LinkConditions::PERFECT
        },
    );
    let mut after = Vec::new();
    for _ in 0..200 {
        game.step();
        let (avatar, _) = walker_track(&game);
        let s = game.peer(1).predictor.presented()[&avatar];
        after.push((s.source, s.transform.translation.y));
    }
    let extrapolated = after
        .iter()
        .filter(|(s, _)| *s == Source::Extrapolated)
        .count();
    println!(
        "after the cut: {:?}; {:?}",
        after
            .iter()
            .map(|(s, _)| *s)
            .collect::<Vec<_>>()
            .split_at(20)
            .0,
        game.peer(1).predictor.stats()
    );
    assert!(
        (1..=7).contains(&extrapolated),
        "{extrapolated} frames extrapolated"
    );
    let held = after.iter().position(|(s, _)| *s == Source::Held).unwrap();
    assert!(
        after[held..]
            .iter()
            .all(|(s, y)| *s == Source::Held && *y == after[held].1)
    );
    let newest_known = after
        .iter()
        .filter(|(s, _)| *s == Source::Interpolated)
        .map(|(_, y)| *y)
        .fold(f32::MIN, f32::max);
    assert!(after[held].1 - newest_known <= 0.05 * 7.0 + 0.06);
    assert!(game.peer(1).predictor.stats().stalls > 0);
}

/// Player 1 turns every 30 ticks until tick 600; player 2 stands still.
fn zigzag(player: PlayerId, tick: u64) -> Vec<(u32, [f32; 2])> {
    match player {
        1 if tick < 600 => {
            let x = if (tick / 30).is_multiple_of(2) {
                1.0
            } else {
                -1.0
            };
            vec![(move_id(), [x, 0.0])]
        }
        _ => Vec::new(),
    }
}

#[test]
fn corrections_after_lost_inputs_converge_and_are_smoothed() {
    let mut game = Game::new(SEED, LinkConditions::BAD, 2);
    game.bot = zigzag;
    let host_addr = game.endpoint.local_addr();
    let mut watch = Watch::default();
    let mut bursts = Vec::new();
    for frame in 0..800u64 {
        // Twice, everything player 1 sends is lost for 20 frames: the host
        // holds its last input while the client turns.
        if frame == 200 || frame == 400 {
            let from = game.peer(1).endpoint.local_addr();
            game.net.set_link(
                from,
                host_addr,
                LinkConditions {
                    loss: 1.0,
                    ..LinkConditions::BAD
                },
            );
            bursts.push(frame);
        }
        if frame == 220 || frame == 420 {
            let from = game.peer(1).endpoint.local_addr();
            game.net.set_link(from, host_addr, LinkConditions::BAD);
        }
        game.step();
        watch.take(&game, 1);
    }
    let stats = game.peer(1).predictor.stats();
    let to_settle = watch.frames_to_settle();
    // The last correction after each burst.
    let last_after = |start: usize| {
        (start..start + 200)
            .filter(|f| watch.corrections[*f] != watch.corrections[*f - 1])
            .max()
            .unwrap_or(start)
    };
    let ends: Vec<usize> = bursts.iter().map(|b| last_after(*b as usize + 1)).collect();
    println!(
        "{stats:?}; sync {:?}; frames to settle {to_settle}; last corrections at {ends:?} (bursts end at 220 and 420)",
        game.peer(1).predictor.sync()
    );
    assert!(stats.corrections > 0, "the bursts caused no correction");
    // Converged: no correction later than 40 frames after a burst ended
    // (one round trip of the bad link is about 17 frames, and the eight
    // frames every input message repeats cover the rest).
    assert!(ends[0] <= 260 && ends[1] <= 460, "{ends:?}");
    assert_eq!(
        watch.corrections[399], watch.corrections[ends[0]],
        "no correction between the bursts"
    );
    // The smoothing offset is gone within 42 frames of the last correction.
    assert!(to_settle <= 42, "{to_settle} frames to settle");
    // Every tick predicted after the second burst settled is the host's.
    let (_, _) = game.compare(1);
    let wrong = game
        .predictions
        .iter()
        .filter(|((p, tick), predicted)| {
            *p == 1
                && *tick > 520
                && game.records.get(&(*p, *tick)).is_some_and(|record| {
                    predicted
                        .iter()
                        .any(|(net, state)| record.get(net) != Some(state))
                })
        })
        .count();
    assert_eq!(wrong, 0);
}
