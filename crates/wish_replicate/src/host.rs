//! The host side: per player, the visible set, a schema-encoded delta
//! against the player's acknowledged state within a byte budget, inputs back
//! into `PlayerActionState` (design note §3).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ComponentId, World};
use wish_net::endpoint::MAX_MESSAGE_LIMIT;
use wish_net::{Channel, Endpoint, Event, PeerId, Transport};
use wish_types::{MAX_PLAYERS, PlayerId};

use crate::input::{self, Actions};
use crate::layout::Layout;
use crate::value::{EntityView, Value, View, read_value};
use crate::visibility::{Scene, priority_milli};
use crate::wire::{self, FINGERPRINT_LEN, MAX_TYPE_NAME, MAX_TYPES, TypeEntry, Welcome};

/// Diagnostics a host or client keeps before dropping the oldest.
pub const MAX_DIAGNOSTICS: usize = 256;

/// Ticks between two snapshots a client may ask for.
pub const RESYNC_INTERVAL: u64 = 64;

/// The host's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationConfig {
    /// The component types sent for replicated entities, by registered name.
    /// Types not named are never sent (design note §2: a forgotten type is
    /// loud, a leaked one silent).
    pub components: Vec<String>,
    /// The largest state message per player per tick, in bytes (64 to
    /// 1 MiB). The default, 1024, is one fragment: one datagram.
    pub budget_bytes: usize,
    /// The largest snapshot for a player who joins or asks for a resync, in
    /// bytes (at least `budget_bytes`, at most 1 MiB, and never more than
    /// the endpoint's largest message).
    pub join_bytes: usize,
    /// Sent states kept per player while not acknowledged (2 to 1024).
    pub history: usize,
    /// How far ahead of the host an input frame may be stamped, in ticks (1
    /// to 3600).
    pub input_window: u64,
}

impl ReplicationConfig {
    /// The defaults, replicating `components`.
    pub fn new<S: AsRef<str>>(components: &[S]) -> Self {
        Self {
            components: components.iter().map(|c| c.as_ref().to_owned()).collect(),
            budget_bytes: 1024,
            join_bytes: 60 * 1024,
            history: 64,
            input_window: 120,
        }
    }

    /// Checks every setting against its range.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String, hint: &str| {
            Err(
                Diagnostic::error("net_config_invalid", message, hint.to_owned())
                    .at_path(format!("/{field}")),
            )
        };
        if !(64..=MAX_MESSAGE_LIMIT).contains(&self.budget_bytes) {
            return bad(
                "budget_bytes",
                format!("budget_bytes is {}", self.budget_bytes),
                "Give each player 64 bytes to 1 MiB per tick (the default, 1024, fits one datagram).",
            );
        }
        if self.join_bytes < self.budget_bytes || self.join_bytes > MAX_MESSAGE_LIMIT {
            return bad(
                "join_bytes",
                format!("join_bytes is {}", self.join_bytes),
                "Let a joining player's snapshot be at least budget_bytes and at most 1 MiB.",
            );
        }
        if !(2..=1024).contains(&self.history) {
            return bad(
                "history",
                format!("history is {}", self.history),
                "Keep 2 to 1024 sent states per player (the default is 64).",
            );
        }
        if !(1..=3600).contains(&self.input_window) {
            return bad(
                "input_window",
                format!("input_window is {}", self.input_window),
                "Accept input stamped 1 to 3600 ticks ahead (the default is 120).",
            );
        }
        if self.components.len() > MAX_TYPES {
            return bad(
                "components",
                format!("{} component types are listed", self.components.len()),
                "Replicate at most 256 component types.",
            );
        }
        Ok(())
    }
}

/// What a host did, counted since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostStats {
    /// State messages sent (snapshots included).
    pub states_sent: u64,
    /// Snapshots sent on the reliable channel (joins and resyncs).
    pub snapshots_sent: u64,
    /// Bytes of state messages sent.
    pub state_bytes: u64,
    /// The largest state message sent, in bytes.
    pub largest_state: u64,
    /// Entity changes left for a later tick because the budget was spent.
    pub deferred: u64,
    /// Input messages accepted.
    pub inputs: u64,
    /// Messages from clients dropped as malformed or over a limit.
    pub invalid: u64,
    /// Input messages that named another player.
    pub wrong_player: u64,
    /// Input frames stamped too far ahead.
    pub out_of_window: u64,
    /// Acknowledgements of ticks never sent.
    pub invalid_acks: u64,
    /// Sends the endpoint refused.
    pub send_errors: u64,
    /// Diagnostics dropped because the list was full.
    pub diagnostics_dropped: u64,
}

/// One replicated type on the host.
#[derive(Debug, Clone)]
struct HostType {
    id: ComponentId,
    name: String,
    fingerprint: [u8; FINGERPRINT_LEN],
    layout: Layout,
}

/// Everything the host keeps about one connected player.
#[derive(Debug, Default)]
struct Link {
    player: PlayerId,
    welcome_pending: bool,
    snapshot_pending: bool,
    last_snapshot: Option<u64>,
    /// Host entity → network id, and back.
    ids: BTreeMap<Entity, u32>,
    entities: BTreeMap<u32, Entity>,
    next_id: u32,
    /// What the client is known to have of each entity it may hold.
    known: BTreeMap<u32, Known>,
    /// What each message not yet acknowledged carried: a state, or `None`
    /// for a removal.
    sent: BTreeMap<u64, Carried>,
    last_sent: Option<u64>,
    /// What the player should have after the last send.
    target: View,
    /// Accumulated priority of entities with unsent changes.
    credit: BTreeMap<u32, u64>,
    /// Input frames not applied yet, by tick.
    frames: BTreeMap<u64, Actions>,
    /// The frame in force and its tick.
    held: Actions,
    applied_input: Option<u64>,
}

/// What the host knows a client has of one entity.
#[derive(Debug, Clone)]
struct Known {
    /// The newest message carrying this entity that the client acknowledged.
    acked_tick: Option<u64>,
    /// The entity's state in it (`None`: absent).
    acked: Option<Arc<EntityView>>,
    /// The newest message that carried this entity.
    latest_tick: u64,
}

impl Known {
    /// Whether a message carrying it is still unacknowledged.
    fn in_flight(&self) -> bool {
        self.acked_tick != Some(self.latest_tick)
    }
}

/// Marks message `tick` as received by the client: the states it carried
/// become what the client is known to have (unless a newer message's
/// already did).
fn acknowledge(link: &mut Link, tick: u64) {
    let Some(carried) = link.sent.remove(&tick) else {
        return;
    };
    for (id, state) in carried {
        // An entity whose newest send was acknowledged as a removal is
        // forgotten; older acknowledgements of it no longer matter.
        let Some(known) = link.known.get_mut(&id) else {
            continue;
        };
        if known.acked_tick.is_none_or(|a| tick > a) {
            known.acked_tick = Some(tick);
            known.acked = state;
        }
    }
    link.known.retain(|_, k| k.acked.is_some() || k.in_flight());
}

/// What one message carries: every entity's state, or `None` for a
/// removal, by network id.
type Carried = Vec<(u32, Option<Arc<EntityView>>)>;

/// One replicated entity's values, read once per tick for every player.
struct Cached {
    /// `(type index, value with references zeroed, reference targets)`.
    values: Vec<(u16, Arc<Value>, Vec<Entity>)>,
}

/// The host of a match: decides what every player receives.
#[derive(Debug)]
pub struct Host {
    config: ReplicationConfig,
    types: Vec<HostType>,
    links: BTreeMap<PeerId, Link>,
    tick: Option<u64>,
    last_sent: Option<u64>,
    pending_disconnect: Vec<PeerId>,
    diagnostics: VecDeque<Diagnostic>,
    stats: HostStats,
}

impl Host {
    /// A host replicating `config.components` of `world`'s types.
    pub fn new(world: &World, config: ReplicationConfig) -> Result<Self> {
        config.validate()?;
        let registry = world.registry();
        let mut types: Vec<HostType> = Vec::new();
        for (index, name) in config.components.iter().enumerate() {
            if types.iter().any(|t| t.name == *name) {
                continue;
            }
            let Some(id) = registry.component_id(name) else {
                return Err(Diagnostic::error(
                    "net_component_unknown",
                    format!("the replicated component {name:?} is not registered"),
                    "Name a component the game or the engine registers (as written in scene files), or register it before making the host.",
                )
                .at_path(format!("/components/{index}")));
            };
            let info = registry.component(id).ok_or_else(|| {
                Diagnostic::error(
                    "net_component_unknown",
                    format!("the replicated component {name:?} has no type information"),
                    "Register the component before making the host.",
                )
            })?;
            if info.is_managed() || name.is_empty() || name.len() > MAX_TYPE_NAME {
                return Err(Diagnostic::error(
                    "net_component_not_replicable",
                    format!("the component {name:?} cannot be replicated"),
                    "The engine keeps managed components (Hierarchy) itself and type names longer than 128 bytes do not fit the welcome; replicate the components the game writes.",
                )
                .at_path(format!("/components/{index}")));
            }
            types.push(HostType {
                id,
                name: name.clone(),
                fingerprint: info.fingerprint().as_u128().to_le_bytes(),
                layout: Layout::of(info.schema())?,
            });
        }
        Ok(Self {
            config,
            types,
            links: BTreeMap::new(),
            tick: None,
            last_sent: None,
            pending_disconnect: Vec::new(),
            diagnostics: VecDeque::new(),
            stats: HostStats::default(),
        })
    }

    /// The settings.
    pub fn config(&self) -> &ReplicationConfig {
        &self.config
    }

    /// The replicated types, in the order messages number them.
    pub fn type_names(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }

    /// The layouts of the replicated types, in the order messages number
    /// them (to decode a captured state message).
    pub fn layouts(&self) -> Vec<Layout> {
        self.types.iter().map(|t| t.layout.clone()).collect()
    }

    /// What the host did so far.
    pub fn stats(&self) -> HostStats {
        self.stats
    }

    /// The diagnostics since the last call (at most [`MAX_DIAGNOSTICS`]).
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    /// The connected players: `(peer, player id)`, in peer order.
    pub fn players(&self) -> Vec<(PeerId, PlayerId)> {
        self.links.iter().map(|(p, l)| (*p, l.player)).collect()
    }

    /// The player id of `peer`.
    pub fn player_of(&self, peer: PeerId) -> Option<PlayerId> {
        self.links.get(&peer).map(|l| l.player)
    }

    fn link_of(&self, player: PlayerId) -> Option<&Link> {
        self.links.values().find(|l| l.player == player)
    }

    /// What `player` should have after the last send: every entity it may
    /// see, as the client's world must read back ([`crate::Client::mirror`]).
    pub fn target(&self, player: PlayerId) -> Option<&View> {
        self.link_of(player).map(|l| &l.target)
    }

    /// The host entity behind `player`'s network id `net`, **only if the
    /// player may see it now**: a later message that names an entity goes
    /// through this, so a forged or stale id resolves to nothing.
    pub fn resolve(&self, player: PlayerId, net: u32) -> Option<Entity> {
        let link = self.link_of(player)?;
        if !link.target.contains_key(&net) {
            return None;
        }
        link.entities.get(&net).copied()
    }

    /// `player`'s whole table of network ids (for tests and tools; game
    /// messages use [`resolve`](Self::resolve)).
    pub fn network_ids(&self, player: PlayerId) -> Vec<(u32, Entity)> {
        self.link_of(player)
            .map(|l| l.entities.iter().map(|(n, e)| (*n, *e)).collect())
            .unwrap_or_default()
    }

    /// The newest input frame of `player` applied, if any.
    pub fn applied_input(&self, player: PlayerId) -> Option<u64> {
        self.link_of(player).and_then(|l| l.applied_input)
    }

    fn note(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
            self.stats.diagnostics_dropped += 1;
        }
        self.diagnostics.push_back(diagnostic);
    }

    /// Takes one event of the host's endpoint. Returns it when another layer
    /// may want it too: connections and disconnections (seen here and
    /// passed on), and messages whose kind is not replication's; consumes
    /// replication messages.
    pub fn handle_event(&mut self, world: &mut World, event: Event) -> Option<Event> {
        match &event {
            Event::Connected { peer } => {
                self.connect(world, *peer);
                Some(event)
            }
            Event::Disconnected { peer, .. } => {
                self.links.remove(peer);
                Some(event)
            }
            Event::Message { peer, data, .. } => {
                if !wire::is_replication(data) {
                    return Some(event);
                }
                let peer = *peer;
                if self.links.contains_key(&peer) {
                    self.receive(peer, data);
                }
                None
            }
        }
    }

    /// Takes every event of `endpoint` (after its `update`); returns those
    /// [`handle_event`](Self::handle_event) passed on.
    pub fn pump<T: Transport>(
        &mut self,
        world: &mut World,
        endpoint: &mut Endpoint<T>,
    ) -> Vec<Event> {
        let mut rest = Vec::new();
        while let Some(event) = endpoint.poll_event() {
            if let Some(event) = self.handle_event(world, event) {
                rest.push(event);
            }
        }
        rest
    }

    fn connect(&mut self, world: &mut World, peer: PeerId) {
        if self.links.contains_key(&peer) {
            return;
        }
        let taken: BTreeSet<PlayerId> = self.links.values().map(|l| l.player).collect();
        let Some(player) = (1..MAX_PLAYERS as PlayerId).find(|p| !taken.contains(p)) else {
            self.pending_disconnect.push(peer);
            self.note(
                Diagnostic::error(
                    "net_players_full",
                    format!("{peer} joined, but every player id is taken"),
                    "A match has at most 16 players (the host's and 15 others); wait for a player to leave.",
                )
                .at_path(format!("/peers/{}", peer.0)),
            );
            return;
        };
        if let Err(problem) = input::ensure_players(world, player + 1) {
            self.note(problem);
        }
        self.links.insert(
            peer,
            Link {
                player,
                welcome_pending: true,
                snapshot_pending: true,
                next_id: 1,
                ..Link::default()
            },
        );
    }

    fn receive(&mut self, peer: PeerId, data: &[u8]) {
        let Some(link) = self.links.get(&peer) else {
            return;
        };
        let player = link.player;
        let invalid = |reason: &str| {
            Diagnostic::warning(
                "net_message_invalid",
                format!("player {player} sent a message that is not valid replication: {reason}; it was dropped"),
                "The other program is broken, of another version, or hostile. Nothing was applied; if both run the same release, report it.",
            )
            .at_path(format!("/players/{player}"))
        };
        if data.len() > wire::MAX_CLIENT_MESSAGE {
            self.stats.invalid += 1;
            self.note(invalid("it is larger than 2 KiB"));
            return;
        }
        let Some(message) = wire::decode_input(data) else {
            self.stats.invalid += 1;
            self.note(invalid("it does not parse"));
            return;
        };
        if message.player != player {
            self.stats.wrong_player += 1;
            self.note(
                Diagnostic::warning(
                    "net_input_wrong_player",
                    format!(
                        "player {player} sent input for player {}; it was dropped",
                        message.player
                    ),
                    "A client sends input only for its own player id (the one in its welcome). A modified client may be trying to move someone else.",
                )
                .at_path(format!("/players/{player}")),
            );
            return;
        }
        self.stats.inputs += 1;
        let now = self.tick.unwrap_or(0);
        let window = self.config.input_window;
        let history = self.config.history;
        let mut problems = Vec::new();
        let Some(link) = self.links.get_mut(&peer) else {
            return;
        };
        if let Some(ack) = message.ack {
            if link.last_sent.is_none_or(|sent| ack > sent) {
                problems.push(
                    Diagnostic::warning(
                        "net_ack_invalid",
                        format!("player {player} acknowledged tick {ack}, which was never sent to it"),
                        "A client acknowledges only ticks it received; the acknowledgement was ignored.",
                    )
                    .at_path(format!("/players/{player}")),
                );
            } else {
                acknowledge(link, ack);
                for bit in 0..64u64 {
                    if message.ack_mask & (1 << bit) != 0
                        && let Some(tick) = ack.checked_sub(bit + 1)
                    {
                        acknowledge(link, tick);
                    }
                }
            }
        }
        if message.resync
            && link
                .last_snapshot
                .is_none_or(|s| now >= s + RESYNC_INTERVAL)
        {
            link.snapshot_pending = true;
        }
        let mut late = false;
        for frame in message.frames {
            if link.applied_input.is_some_and(|a| frame.tick <= a) {
                continue;
            }
            if frame.tick > now.saturating_add(window) {
                late = true;
                continue;
            }
            link.frames.entry(frame.tick).or_insert(frame.actions);
        }
        while link.frames.len() > history.max(window as usize) {
            link.frames.pop_first();
        }
        if late {
            problems.push(
                Diagnostic::warning(
                    "net_input_out_of_window",
                    format!("player {player} sent input stamped more than {window} ticks ahead of tick {now}; it was dropped"),
                    "A client stamps input with the host's tick plus a small lead; a frame far ahead is a broken clock or a modified client.",
                )
                .at_path(format!("/players/{player}")),
            );
        }
        for problem in problems {
            match &*problem.code {
                "net_ack_invalid" => self.stats.invalid_acks += 1,
                _ => self.stats.out_of_window += 1,
            }
            self.note(problem);
        }
    }

    /// Applies every connected player's input for `tick`: the newest frame
    /// stamped at or before it (held when none is newer) goes into
    /// `PlayerActionState`; entry 0 is the host's own `ActionState`. Call it
    /// after the host's own input and before the systems.
    pub fn apply_inputs(&mut self, world: &mut World, tick: u64) -> Result<()> {
        self.tick = Some(self.tick.map_or(tick, |t| t.max(tick)));
        let mut held: BTreeMap<PlayerId, Actions> = BTreeMap::new();
        let mut highest = 0;
        for link in self.links.values_mut() {
            if let Some((&stamp, _)) = link.frames.range(..=tick).next_back()
                && let Some(actions) = link.frames.remove(&stamp)
            {
                link.held = actions;
                link.applied_input = Some(stamp);
                link.frames.retain(|t, _| *t > stamp);
            }
            held.insert(link.player, link.held.clone());
            highest = highest.max(link.player);
        }
        if !self.links.is_empty() {
            input::ensure_players(world, highest + 1)?;
        }
        input::write_players(world, &|player| held.get(&player).cloned())
    }

    /// Sends every connected player its state for `tick` (ticks must
    /// increase): first its welcome and snapshot when it just joined (or
    /// asked for a resync), else a delta against its acknowledged state
    /// within the budget.
    pub fn send<T: Transport>(
        &mut self,
        world: &World,
        tick: u64,
        endpoint: &mut Endpoint<T>,
    ) -> Result<()> {
        if self.last_sent.is_some_and(|last| tick <= last) {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("tick {tick} is not after the last tick sent"),
                "Send each tick once, in increasing order.",
            ));
        }
        self.last_sent = Some(tick);
        self.tick = Some(self.tick.map_or(tick, |t| t.max(tick)));
        for peer in std::mem::take(&mut self.pending_disconnect) {
            let _ = endpoint.disconnect(peer);
        }
        let scene = Scene::read(world);
        let cache = self.read_values(world, &scene)?;
        let max_message = endpoint.config().max_message_size;
        let peers: Vec<PeerId> = self.links.keys().copied().collect();
        for peer in peers {
            self.send_to(world, &scene, &cache, tick, peer, max_message, endpoint);
        }
        Ok(())
    }

    fn read_values(&self, world: &World, scene: &Scene) -> Result<BTreeMap<Entity, Cached>> {
        let mut cache = BTreeMap::new();
        if self.links.is_empty() {
            return Ok(cache);
        }
        for candidate in &scene.candidates {
            let mut values = Vec::new();
            for (index, ty) in self.types.iter().enumerate() {
                let mut targets = Vec::new();
                let value = read_value(world, candidate.entity, ty.id, &ty.layout, &mut |e| {
                    targets.push(e);
                    0
                })?;
                if let Some(value) = value {
                    values.push((index as u16, Arc::new(value), targets));
                }
            }
            cache.insert(candidate.entity, Cached { values });
        }
        Ok(cache)
    }

    #[allow(clippy::too_many_arguments)]
    fn send_to<T: Transport>(
        &mut self,
        world: &World,
        scene: &Scene,
        cache: &BTreeMap<Entity, Cached>,
        tick: u64,
        peer: PeerId,
        max_message: usize,
        endpoint: &mut Endpoint<T>,
    ) {
        let types = &self.types;
        let (join_bytes, budget_bytes, history) = (
            self.config.join_bytes,
            self.config.budget_bytes,
            self.config.history,
        );
        let Some(link) = self.links.get_mut(&peer) else {
            return;
        };
        let player = link.player;
        // Who this player may see now, and their network ids.
        let visible: Vec<_> = scene
            .candidates
            .iter()
            .filter(|c| scene.sees(player, c))
            .collect();
        for candidate in &visible {
            if !link.ids.contains_key(&candidate.entity) {
                let id = link.next_id;
                link.next_id = link.next_id.saturating_add(1);
                link.ids.insert(candidate.entity, id);
                link.entities.insert(id, candidate.entity);
            }
        }
        let shown: BTreeSet<Entity> = visible.iter().map(|c| c.entity).collect();
        let mut target = View::new();
        let mut priorities = BTreeMap::new();
        for candidate in &visible {
            let entity = &candidate.entity;
            let Some(&id) = link.ids.get(entity) else {
                continue;
            };
            let mut view = EntityView::new();
            if let Some(cached) = cache.get(entity) {
                for (ty, value, targets) in &cached.values {
                    let value = if targets.is_empty() {
                        Arc::clone(value)
                    } else {
                        let mut own = Value::clone(value);
                        own.refs = targets
                            .iter()
                            .map(|t| {
                                if shown.contains(t) {
                                    link.ids.get(t).copied().unwrap_or(0)
                                } else {
                                    0
                                }
                            })
                            .collect();
                        Arc::new(own)
                    };
                    view.insert(*ty, value);
                }
            }
            target.insert(id, Arc::new(view));
            priorities.insert(id, priority_milli(candidate.rule.priority));
        }
        let mut outgoing: Vec<(Channel, Vec<u8>)> = Vec::new();
        if link.welcome_pending {
            outgoing.push((
                Channel::Reliable,
                wire::encode_welcome(&Welcome {
                    player,
                    tick,
                    types: types
                        .iter()
                        .map(|t| TypeEntry {
                            name: t.name.clone(),
                            fingerprint: t.fingerprint,
                        })
                        .collect(),
                }),
            ));
        }
        let snapshot = link.snapshot_pending;
        let budget = if snapshot {
            join_bytes.min(max_message)
        } else {
            budget_bytes.min(max_message)
        };
        let built = build(
            types,
            tick,
            link.applied_input,
            &link.known,
            &target,
            snapshot,
            &priorities,
            &mut link.credit,
            budget,
            max_message,
        );
        let channel = if snapshot {
            Channel::Reliable
        } else {
            Channel::Unreliable
        };
        outgoing.push((channel, built.bytes));
        let mut failed = None;
        for (channel, bytes) in &outgoing {
            if let Err(problem) = endpoint.send(peer, *channel, bytes) {
                failed = Some(problem);
                break;
            }
        }
        if let Some(problem) = failed {
            self.stats.send_errors += 1;
            self.note(problem);
            return;
        }
        let Some(link) = self.links.get_mut(&peer) else {
            return;
        };
        let size = outgoing.last().map_or(0, |(_, b)| b.len() as u64);
        self.stats.states_sent += 1;
        self.stats.state_bytes += size;
        self.stats.largest_state = self.stats.largest_state.max(size);
        self.stats.deferred += built.deferred;
        link.welcome_pending = false;
        link.last_sent = Some(tick);
        link.target = target;
        for (id, _) in &built.carried {
            link.known
                .entry(*id)
                .or_insert(Known {
                    acked_tick: None,
                    acked: None,
                    latest_tick: tick,
                })
                .latest_tick = tick;
        }
        link.sent.insert(tick, built.carried);
        while link.sent.len() > history {
            link.sent.pop_first();
        }
        if snapshot {
            self.stats.snapshots_sent += 1;
            // The reliable channel delivers it: what it carries is what the
            // client has, at once.
            link.snapshot_pending = false;
            link.last_snapshot = Some(tick);
            acknowledge(link, tick);
        }
        // Forget the ids of entities that no longer exist and that the
        // client no longer holds (ids are never given again).
        let gone: Vec<(Entity, u32)> = link
            .ids
            .iter()
            .filter(|(e, id)| {
                !world.contains(**e)
                    && !link.known.contains_key(id)
                    && !link.target.contains_key(id)
            })
            .map(|(e, id)| (*e, *id))
            .collect();
        for (entity, id) in gone {
            link.ids.remove(&entity);
            link.entities.remove(&id);
        }
    }
}

/// A built state message and what it carries.
struct Built {
    bytes: Vec<u8>,
    /// Every entity it carries: its state, or `None` for a removal.
    carried: Carried,
    deferred: u64,
}

/// The section of one entity: the gap to its base, then its operations from
/// `base` (none: from nothing) to `target`.
fn section(
    types: &[HostType],
    tick: u64,
    base: Option<(u64, &EntityView)>,
    target: &EntityView,
) -> Vec<u8> {
    let mut keys: BTreeSet<u16> = target.keys().copied().collect();
    if let Some((_, base)) = base {
        keys.extend(base.keys().copied());
    }
    let mut ops = Vec::new();
    let mut count = 0u64;
    for ty in keys {
        let Some(layout) = types.get(usize::from(ty)).map(|t| &t.layout) else {
            continue;
        };
        let before = base.and_then(|(_, b)| b.get(&ty)).map(|v| &**v);
        let after = target.get(&ty).map(|v| &**v);
        if wire::put_op(&mut ops, ty, layout, before, after) {
            count += 1;
        }
    }
    let mut out = Vec::with_capacity(ops.len() + 4);
    wire::put_varint(&mut out, wire::base_gap(tick, base.map(|(t, _)| t)));
    wire::put_varint(&mut out, count);
    out.extend_from_slice(&ops);
    out
}

/// The message that brings the client from what it is known to have
/// (`known`) towards `target`, within `budget` bytes (design note §3.5).
/// An entity is carried while its state differs from the acknowledged one
/// or while a message carrying it is unacknowledged; `whole` (a snapshot)
/// sends every state whole.
#[allow(clippy::too_many_arguments)]
fn build(
    types: &[HostType],
    tick: u64,
    input_ack: Option<u64>,
    known: &BTreeMap<u32, Known>,
    target: &View,
    whole: bool,
    priorities: &BTreeMap<u32, u64>,
    credit: &mut BTreeMap<u32, u64>,
    budget: usize,
    max: usize,
) -> Built {
    let header = wire::state_header(tick, input_ack);
    let mut ids: BTreeSet<u32> = target.keys().copied().collect();
    ids.extend(known.keys().copied());
    let mut removed = Vec::new();
    let mut changed: Vec<(u32, Vec<u8>)> = Vec::new();
    for id in ids {
        let state = known.get(&id);
        let acked = state.and_then(|k| k.acked.as_ref());
        let after = target.get(&id);
        let same = match (acked, after) {
            (Some(a), Some(t)) => Arc::ptr_eq(a, t) || **a == **t,
            (None, None) => true,
            _ => false,
        };
        if !whole && same && !state.is_some_and(Known::in_flight) {
            continue;
        }
        match after {
            None => removed.push(id),
            Some(after) => {
                let base = if whole {
                    None
                } else {
                    state.and_then(|k| Some((k.acked_tick?, &**k.acked.as_ref()?)))
                };
                changed.push((id, section(types, tick, base, after)));
            }
        }
    }
    // Accumulated priority: every entity waiting gains its priority.
    let waiting: BTreeSet<u32> = changed.iter().map(|(id, _)| *id).collect();
    credit.retain(|id, _| waiting.contains(id));
    for (id, _) in &changed {
        let gain = priorities.get(id).copied().unwrap_or(1000);
        let entry = credit.entry(*id).or_insert(0);
        *entry = entry.saturating_add(gain);
    }
    changed.sort_by(|(a, _), (b, _)| {
        let ca = credit.get(a).copied().unwrap_or(0);
        let cb = credit.get(b).copied().unwrap_or(0);
        cb.cmp(&ca).then(a.cmp(b))
    });

    let mut used = header.len()
        + wire::varint_len(removed.len() as u64)
        + wire::varint_len(changed.len() as u64);
    let mut removed_in = Vec::new();
    for id in removed {
        let n = wire::varint_len(u64::from(id));
        if used + n > budget {
            break;
        }
        used += n;
        removed_in.push(id);
    }
    let mut chosen: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut deferred = 0;
    for (id, bytes) in changed {
        let n = wire::varint_len(u64::from(id)) + bytes.len();
        if used + n <= budget || (chosen.is_empty() && used + n <= max) {
            used += n;
            chosen.insert(id, bytes);
        } else {
            deferred += 1;
        }
    }
    for id in chosen.keys() {
        credit.remove(id);
    }

    let mut bytes = header;
    let mut carried = Vec::with_capacity(removed_in.len() + chosen.len());
    wire::put_varint(&mut bytes, removed_in.len() as u64);
    for id in &removed_in {
        wire::put_varint(&mut bytes, u64::from(*id));
        carried.push((*id, None));
    }
    wire::put_varint(&mut bytes, chosen.len() as u64);
    for (id, section) in chosen {
        wire::put_varint(&mut bytes, u64::from(id));
        bytes.extend_from_slice(&section);
        carried.push((id, target.get(&id).cloned()));
    }
    carried.sort_by_key(|(id, _)| *id);
    Built {
        bytes,
        carried,
        deferred,
    }
}
