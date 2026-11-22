//! The client side: mirrors what the host sends into the client's own world
//! and sends the player's input back (design note §3.4).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ComponentId, World};
use wish_net::{Channel, Endpoint, Event, PeerId, Transport};
use wish_types::{ActionState, PlayerId};

use crate::host::MAX_DIAGNOSTICS;
use crate::input::{Actions, actions_of};
use crate::layout::Layout;
use crate::value::{EntityView, Value, View, read_value, write_value};
use crate::wire::{self, Change, Frame, Input, Section};

/// Messages more than this many ticks older than the newest seen are
/// dropped unread.
pub const STALE_TICKS: u64 = 1024;

/// A client's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Received states kept per entity (as possible bases), at least 2.
    pub history: usize,
    /// How many ticks ahead of the newest host tick seen input is stamped
    /// by [`Client::send_input`]. A client that predicts stamps its frames
    /// with its own ticks instead ([`crate::Predictor`], whose tick sync
    /// sets the lead).
    pub input_lead: u64,
    /// After this many state messages in a row that could not be read whole
    /// (a base is gone), the client asks the host for a new snapshot.
    pub resync_after: u32,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            history: 64,
            input_lead: 2,
            resync_after: 32,
        }
    }
}

/// What a client did, counted since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientStats {
    /// State messages received.
    pub states_received: u64,
    /// State messages read whole (every section had its base).
    pub states_whole: u64,
    /// State messages older than the newest read whole.
    pub out_of_order: u64,
    /// Sections whose base the client does not have.
    pub missing_base: u64,
    /// Messages dropped as malformed.
    pub invalid: u64,
    /// Snapshots asked for.
    pub resyncs_requested: u64,
    /// Component writes the world refused (an invalid value).
    pub write_failures: u64,
    /// Input messages sent.
    pub inputs_sent: u64,
}

/// One entity's change on screen: its network id, the state shown until
/// now and the state to show (`None`: absent).
type Shift = (u32, Option<Arc<EntityView>>, Option<Arc<EntityView>>);

/// One replicated type, as this client's world knows it.
#[derive(Debug, Clone)]
struct ClientType {
    name: String,
    id: ComponentId,
    layout: Layout,
}

/// States received of one entity, by tick (`None`: absent at that tick).
pub type States = BTreeMap<u64, Option<Arc<EntityView>>>;

/// What the host confirmed: the newest complete state message read whole
/// (nothing the player may see was left out), as the player's whole view at
/// that tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    /// The host's tick.
    pub tick: u64,
    /// The newest input frame of this player the host had applied by then.
    pub input_ack: Option<u64>,
    /// Every entity the player could see at that tick, by network id.
    pub view: View,
}

/// What the client holds of one entity.
#[derive(Debug, Default)]
struct Track {
    /// Its states by tick (`None`: absent), possible bases of later
    /// messages.
    states: States,
    /// The tick of the state the world shows.
    shown: Option<u64>,
    /// The state the world shows.
    current: Option<Arc<EntityView>>,
}

/// A client of a match: mirrors what its player may see.
#[derive(Debug)]
pub struct Client {
    config: ClientConfig,
    player: Option<PlayerId>,
    types: Vec<ClientType>,
    incompatible: bool,
    tracks: BTreeMap<u32, Track>,
    /// Ticks of messages read whole (acknowledged to the host).
    received: BTreeSet<u64>,
    newest_seen: Option<u64>,
    local: BTreeMap<u32, Entity>,
    net_of: BTreeMap<Entity, u32>,
    frames: BTreeMap<u64, Actions>,
    input_ack: Option<u64>,
    misses: u32,
    /// Ticks of complete messages read whole (interpolation knows that
    /// every entity they did not carry was unchanged).
    complete: BTreeSet<u64>,
    /// The newest confirmed view not taken yet.
    confirmed: Option<Confirmed>,
    last_confirmed: Option<u64>,
    diagnostics: VecDeque<Diagnostic>,
    stats: ClientStats,
}

impl Client {
    /// A client with `config`; it knows nothing until the host's welcome.
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            player: None,
            types: Vec::new(),
            incompatible: false,
            tracks: BTreeMap::new(),
            received: BTreeSet::new(),
            newest_seen: None,
            local: BTreeMap::new(),
            net_of: BTreeMap::new(),
            frames: BTreeMap::new(),
            input_ack: None,
            misses: 0,
            complete: BTreeSet::new(),
            confirmed: None,
            last_confirmed: None,
            diagnostics: VecDeque::new(),
            stats: ClientStats::default(),
        }
    }

    /// Forgets the match: despawns every entity it mirrored into `world` and
    /// starts again as a new client with the same settings (a player who
    /// rejoins on a new connection gets a new welcome and a full snapshot).
    pub fn forget(&mut self, world: &mut World) {
        for entity in self.local.values() {
            if world.contains(*entity) {
                let _ = world.despawn(*entity);
            }
        }
        *self = Self::new(self.config.clone());
    }

    /// This client's player id, once welcomed.
    pub fn player(&self) -> Option<PlayerId> {
        self.player
    }

    /// The tick of the newest state message read whole: the world shows the
    /// host's state of that tick (for every entity the budget let through).
    pub fn applied_tick(&self) -> Option<u64> {
        self.received.last().copied()
    }

    /// The newest tick any entity the world shows comes from. When it equals
    /// [`applied_tick`](Self::applied_tick), the world is exactly the host's
    /// state of that tick (as far as the budget let it through).
    pub fn newest_shown(&self) -> Option<u64> {
        self.tracks.values().filter_map(|t| t.shown).max()
    }

    /// The newest of this player's input frames the host applied.
    pub fn input_ack(&self) -> Option<u64> {
        self.input_ack
    }

    /// The newest host tick any message (the welcome included) came from.
    pub fn newest_seen(&self) -> Option<u64> {
        self.newest_seen
    }

    /// The newest confirmed view since the last call: the client read a
    /// complete state message whole, newer than every message it read whole
    /// before. A predicting client reconciles against it.
    pub fn take_confirmed(&mut self) -> Option<Confirmed> {
        self.confirmed.take()
    }

    /// The tick of the newest confirmed view, taken or not.
    pub fn confirmed_tick(&self) -> Option<u64> {
        self.last_confirmed
    }

    /// The states received of network id `net`, by tick (at most `history`
    /// of them).
    pub fn states(&self, net: u32) -> Option<&States> {
        self.tracks.get(&net).map(|t| &t.states)
    }

    /// The ticks of the complete messages read whole lately (at most 65):
    /// at each, every entity not carried was unchanged.
    pub fn complete_ticks(&self) -> &BTreeSet<u64> {
        &self.complete
    }

    /// The network ids the client's world shows, ascending.
    pub fn shown(&self) -> Vec<u32> {
        self.local.keys().copied().collect()
    }

    /// The names of the replicated types, in the order messages number
    /// them, once welcomed.
    pub fn type_names(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }

    /// What the client did so far.
    pub fn stats(&self) -> ClientStats {
        self.stats
    }

    /// The diagnostics since the last call.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    /// The client's entity for network id `net`.
    pub fn entity(&self, net: u32) -> Option<Entity> {
        self.local.get(&net).copied()
    }

    /// The network id of the client's entity `entity`.
    pub fn network_id(&self, entity: Entity) -> Option<u32> {
        self.net_of.get(&entity).copied()
    }

    /// The layouts of the replicated types, once welcomed.
    pub fn layouts(&self) -> Vec<Layout> {
        self.types.iter().map(|t| t.layout.clone()).collect()
    }

    fn note(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    fn invalid(&mut self, what: &str) {
        self.stats.invalid += 1;
        self.note(Diagnostic::warning(
            "net_state_invalid",
            format!("the host sent a {what} that is not valid; it was dropped"),
            "The host runs another version of the game or is broken. If both run the same release, report it.",
        ));
    }

    /// Takes one event of the client's endpoint. Returns it when another
    /// layer may want it (connections, and messages that are not
    /// replication's); consumes replication messages.
    pub fn handle_event(&mut self, world: &mut World, event: Event) -> Option<Event> {
        match &event {
            Event::Message { peer, data, .. }
                if *peer == PeerId::HOST && wire::is_replication(data) =>
            {
                match data.first().copied() {
                    Some(wire::kind::WELCOME) => self.welcome(world, data),
                    Some(wire::kind::STATE) => self.state(world, data),
                    _ => self.invalid("message"),
                }
                None
            }
            _ => Some(event),
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

    fn welcome(&mut self, world: &World, data: &[u8]) {
        let Some(welcome) = wire::decode_welcome(data) else {
            self.invalid("welcome");
            return;
        };
        if self.player.is_some() {
            // A welcome sent again (its first send was refused); the types
            // cannot change within a match.
            return;
        }
        let registry = world.registry();
        let mut types = Vec::with_capacity(welcome.types.len());
        let mut wrong = Vec::new();
        for entry in &welcome.types {
            let known = registry.component_id(&entry.name).and_then(|id| {
                let info = registry.component(id)?;
                if info.fingerprint().as_u128().to_le_bytes() != entry.fingerprint {
                    return None;
                }
                let layout = Layout::of(info.schema()).ok()?;
                Some(ClientType {
                    name: entry.name.clone(),
                    id,
                    layout,
                })
            });
            match known {
                Some(ty) => types.push(ty),
                None => wrong.push(entry.name.clone()),
            }
        }
        self.player = Some(welcome.player);
        self.newest_seen = Some(welcome.tick);
        if !wrong.is_empty() {
            self.incompatible = true;
            self.note(
                Diagnostic::error(
                    "net_types_mismatch",
                    format!(
                        "the host replicates types this game does not have, or has with another layout: {}",
                        wrong.join(", ")
                    ),
                    "Host and players must run the same build of the game; build and start both from the same project.",
                )
                .with_candidates(wrong),
            );
            return;
        }
        self.types = types;
    }

    fn state(&mut self, world: &mut World, data: &[u8]) {
        if self.player.is_none() || self.incompatible {
            return;
        }
        self.stats.states_received += 1;
        let layouts: Vec<Layout> = self.types.iter().map(|t| t.layout.clone()).collect();
        let Some(message) = wire::decode_state(data, &layouts) else {
            self.invalid("state");
            return;
        };
        let tick = message.tick;
        if self
            .newest_seen
            .is_some_and(|n| tick.saturating_add(STALE_TICKS) < n)
        {
            self.stats.out_of_order += 1;
            return;
        }
        self.newest_seen = Some(self.newest_seen.map_or(tick, |n| n.max(tick)));
        if self.applied_tick().is_some_and(|a| tick < a) {
            self.stats.out_of_order += 1;
        }
        // Every section becomes a state of its entity at this tick.
        let mut whole = true;
        let mut touched = Vec::new();
        for net in &message.removed {
            self.tracks
                .entry(*net)
                .or_default()
                .states
                .insert(tick, None);
            touched.push(*net);
        }
        for section in &message.entities {
            match self.read_section(section, tick, &layouts) {
                Some(()) => touched.push(section.id),
                None => whole = false,
            }
        }
        if whole {
            let newest = self.received.last().is_none_or(|last| tick > *last);
            if message.complete {
                self.complete.insert(tick);
                while self.complete.len() > 65 {
                    self.complete.pop_first();
                }
                if newest
                    && self.last_confirmed.is_none_or(|last| tick > last)
                    && let Some(view) = self.view_at(tick)
                {
                    self.last_confirmed = Some(tick);
                    self.confirmed = Some(Confirmed {
                        tick,
                        input_ack: message.input_ack,
                        view,
                    });
                }
            }
            self.received.insert(tick);
            while self.received.len() > 65 {
                self.received.pop_first();
            }
            self.stats.states_whole += 1;
            self.misses = 0;
        } else if !self.received.is_empty() {
            self.misses = self.misses.saturating_add(1);
        }
        if let Some(ack) = message.input_ack {
            self.input_ack = Some(self.input_ack.map_or(ack, |a| a.max(ack)));
            self.frames.retain(|t, _| *t > ack);
        }
        // The world shows, per entity, its newest state.
        touched.sort_unstable();
        touched.dedup();
        let mut changes = Vec::new();
        for net in touched {
            let Some(track) = self.tracks.get_mut(&net) else {
                continue;
            };
            if track.shown.is_some_and(|s| s >= tick) {
                continue;
            }
            let Some(state) = track.states.get(&tick).cloned() else {
                continue;
            };
            let old = std::mem::replace(&mut track.current, state.clone());
            track.shown = Some(tick);
            changes.push((net, old, state));
        }
        self.show(world, changes);
        let newest = self.newest_seen.unwrap_or(tick);
        self.tracks.retain(|_, t| {
            t.current.is_some()
                || t.states.values().any(Option::is_some)
                || t.shown
                    .is_some_and(|s| s.saturating_add(STALE_TICKS) >= newest)
        });
    }

    /// The player's whole view at `tick`: per entity, its newest state at
    /// or before it. Exact for a complete message read whole when it is the
    /// newest read whole (no base older than `tick` was dropped); `None`
    /// when some entity shown since before `tick` has no state left at or
    /// before it (it cannot tell).
    fn view_at(&self, tick: u64) -> Option<View> {
        let mut view = View::new();
        for (net, track) in &self.tracks {
            match track.states.range(..=tick).next_back() {
                Some((_, Some(state))) => {
                    view.insert(*net, Arc::clone(state));
                }
                Some((_, None)) => {}
                // Seen first after `tick`: it was not visible then.
                None if track.shown.is_none_or(|s| s > tick) => {}
                None => return None,
            }
        }
        Some(view)
    }

    /// Reads one section into its entity's states. `None` when its base is
    /// gone or it does not fit (nothing is stored then).
    fn read_section(&mut self, section: &Section, tick: u64, layouts: &[Layout]) -> Option<()> {
        let history = self.config.history.max(2);
        let track = self.tracks.entry(section.id).or_default();
        let mut state: EntityView = match section.base {
            None => EntityView::new(),
            Some(base) => match track.states.get(&base) {
                Some(Some(state)) => (**state).clone(),
                Some(None) => EntityView::new(),
                None => {
                    self.stats.missing_base += 1;
                    return None;
                }
            },
        };
        for op in &section.ops {
            match &op.change {
                Change::Remove => {
                    state.remove(&op.ty);
                }
                Change::Whole(value) => {
                    state.insert(op.ty, Arc::new(value.clone()));
                }
                Change::Leaves(leaves) => {
                    let layout = layouts.get(usize::from(op.ty))?;
                    let Some(before) = state.get(&op.ty) else {
                        self.invalid("state");
                        return None;
                    };
                    let mut value = Value::clone(before);
                    for (index, data) in leaves {
                        if !value.set_leaf(layout, *index, data.clone()) {
                            self.invalid("state");
                            return None;
                        }
                    }
                    state.insert(op.ty, Arc::new(value));
                }
            }
        }
        let track = self.tracks.entry(section.id).or_default();
        track.states.insert(tick, Some(Arc::new(state)));
        // The host never names a base older than one it named before.
        if let Some(base) = section.base {
            track.states.retain(|t, _| *t >= base);
        }
        while track.states.len() > history {
            track.states.pop_first();
        }
        Some(())
    }

    /// Brings the world's entities from their old states to their new ones.
    fn show(&mut self, world: &mut World, changes: Vec<Shift>) {
        let mut spawned = BTreeSet::new();
        let mut writes: Vec<(u32, u16, Option<Arc<Value>>)> = Vec::new();
        for (net, old, new) in &changes {
            let Some(new) = new else {
                if let Some(entity) = self.local.remove(net) {
                    self.net_of.remove(&entity);
                    if world.contains(entity) {
                        let _ = world.despawn(entity);
                    }
                }
                continue;
            };
            let exists = self.local.get(net).is_some_and(|e| world.contains(*e));
            if !exists {
                match world.spawn() {
                    Ok(entity) => {
                        if let Some(stale) = self.local.insert(*net, entity) {
                            self.net_of.remove(&stale);
                        }
                        self.net_of.insert(entity, *net);
                        spawned.insert(*net);
                    }
                    Err(problem) => {
                        self.note(problem);
                        continue;
                    }
                }
            }
            let empty = EntityView::new();
            let before = match old {
                Some(old) if exists => &**old,
                _ => &empty,
            };
            for (ty, value) in new.iter() {
                if before
                    .get(ty)
                    .is_some_and(|b| Arc::ptr_eq(b, value) || b == value)
                {
                    continue;
                }
                writes.push((*net, *ty, Some(Arc::clone(value))));
            }
            for ty in before.keys() {
                if !new.contains_key(ty) {
                    writes.push((*net, *ty, None));
                }
            }
        }
        // References to entities that appeared just now resolve only now.
        if !spawned.is_empty() {
            let written: BTreeSet<(u32, u16)> = writes.iter().map(|(n, t, _)| (*n, *t)).collect();
            for (net, track) in &self.tracks {
                let Some(current) = &track.current else {
                    continue;
                };
                for (ty, value) in current.iter() {
                    if value.refs.iter().any(|r| spawned.contains(r))
                        && !written.contains(&(*net, *ty))
                    {
                        writes.push((*net, *ty, Some(Arc::clone(value))));
                    }
                }
            }
        }
        let local = self.local.clone();
        let map = move |net: u32| local.get(&net).copied().unwrap_or(Entity::NULL);
        for (net, ty, value) in writes {
            let (Some(entity), Some(client_type)) = (
                self.local.get(&net).copied(),
                self.types.get(usize::from(ty)),
            ) else {
                continue;
            };
            let (id, layout) = (client_type.id, client_type.layout.clone());
            let outcome = match value {
                Some(value) => write_value(world, entity, id, &layout, &value, &map),
                None => world.remove_by_id(entity, id).map(|_| ()),
            };
            if let Err(problem) = outcome {
                self.stats.write_failures += 1;
                self.note(problem);
            }
        }
    }

    /// Reads the client's world back into the host's form: every mirrored
    /// entity, by network id, with its replicated components. Equal to
    /// `Host::target` for this player when the client is up to date.
    pub fn mirror(&self, world: &World) -> Result<View> {
        let mut view = View::new();
        for (net, entity) in &self.local {
            if !world.contains(*entity) {
                continue;
            }
            let mut entity_view = EntityView::new();
            for (index, ty) in self.types.iter().enumerate() {
                let value = read_value(world, *entity, ty.id, &ty.layout, &mut |e| {
                    self.net_of.get(&e).copied().unwrap_or(0)
                })?;
                if let Some(value) = value {
                    entity_view.insert(index as u16, Arc::new(value));
                }
            }
            view.insert(*net, Arc::new(entity_view));
        }
        Ok(view)
    }

    /// The tick this client stamps its input with now: the newest host tick
    /// seen plus `input_lead`.
    pub fn stamp(&self) -> u64 {
        self.newest_seen
            .unwrap_or(0)
            .saturating_add(self.config.input_lead)
    }

    /// The actions of this machine's player not at rest, from the client
    /// world's `ActionState` (written by `wish_input::apply`).
    pub fn local_actions(world: &World) -> Actions {
        world
            .resource::<ActionState>()
            .map(|state| actions_of(&state.slots))
            .unwrap_or_default()
    }

    /// The input message for `actions` now: stamped (see
    /// [`stamp`](Self::stamp)), with up to seven earlier frames the host has
    /// not confirmed, and the acknowledgements. `None` before the welcome.
    pub fn input_message(&mut self, actions: Actions) -> Option<Vec<u8>> {
        self.player?;
        let stamp = self.stamp();
        self.queue_frame(stamp, actions);
        self.queued_message()
    }

    /// Adds the frame of tick `tick` to those the next message carries
    /// (with the earlier ones the host has not confirmed, at most
    /// [`wire::MAX_FRAMES`]). A frame the host already confirmed is
    /// ignored; a frame of a tick already queued replaces it.
    pub fn queue_frame(&mut self, tick: u64, actions: Actions) {
        if self.input_ack.is_some_and(|ack| tick <= ack) {
            return;
        }
        self.frames.insert(tick, actions);
        while self.frames.len() > wire::MAX_FRAMES {
            self.frames.pop_first();
        }
    }

    /// The input message of the queued frames and the acknowledgements.
    /// `None` before the welcome.
    pub fn queued_message(&mut self) -> Option<Vec<u8>> {
        let player = self.player?;
        let resync = self.misses >= self.config.resync_after;
        if resync {
            self.stats.resyncs_requested += 1;
            self.misses = 0;
        }
        let frames = self
            .frames
            .iter()
            .rev()
            .map(|(tick, actions)| Frame {
                tick: *tick,
                actions: actions.clone(),
            })
            .collect();
        let ack = self.received.last().copied();
        let mut ack_mask = 0u64;
        if let Some(ack) = ack {
            for tick in &self.received {
                if let Some(gap) = ack.checked_sub(*tick)
                    && (1..=64).contains(&gap)
                {
                    ack_mask |= 1 << (gap - 1);
                }
            }
        }
        Some(wire::encode_input(&Input {
            player,
            ack,
            ack_mask,
            resync,
            frames,
        }))
    }

    /// Sends [`queued_message`](Self::queued_message) to the host on the
    /// unreliable channel (also with no new frame: it carries the
    /// acknowledgements). Nothing is sent before the welcome.
    pub fn send_queued<T: Transport>(&mut self, endpoint: &mut Endpoint<T>) -> Result<()> {
        let Some(message) = self.queued_message() else {
            return Ok(());
        };
        endpoint.send(PeerId::HOST, Channel::Unreliable, &message)?;
        self.stats.inputs_sent += 1;
        Ok(())
    }

    /// Sends [`input_message`](Self::input_message) to the host on the
    /// unreliable channel. Nothing is sent before the welcome.
    pub fn send_input<T: Transport>(
        &mut self,
        endpoint: &mut Endpoint<T>,
        actions: Actions,
    ) -> Result<()> {
        let Some(message) = self.input_message(actions) else {
            return Ok(());
        };
        endpoint.send(PeerId::HOST, Channel::Unreliable, &message)?;
        self.stats.inputs_sent += 1;
        Ok(())
    }
}
