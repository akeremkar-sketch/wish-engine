//! Client prediction and reconciliation (design note `1.1-prediction.md`).
//!
//! A [`Predictor`] owns the *prediction app*: an [`App`] the caller builds
//! with the game's types and systems, exactly as the host builds its own.
//! Its world holds a copy of every entity the client mirrors. The entities
//! the local player owns, and unowned ones near them, are simulated ahead
//! of the host with the player's own inputs; when the host confirms a tick,
//! the prediction of that tick is compared with it and, on a difference,
//! replayed from the confirmed state. Everything else is drawn a fixed
//! delay behind the host, interpolated between received states. Corrections
//! are smoothed in the presentation only; the simulated state is never
//! bent.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use wish_app::{App, AppSnapshot};
use wish_core::math::Vec3;
use wish_core::{Diagnostic, Entity, Hash128, Result};
use wish_ecs::{ComponentId, World};
use wish_net::{Endpoint, PeerId, Transport};
use wish_schema::Plain;
use wish_types::{NetOwner, PlayerId, Transform};

use crate::client::{Client, Confirmed};
use crate::host::MAX_DIAGNOSTICS;
use crate::input::{self, Actions};
use crate::interpolate::{self, Source};
use crate::layout::Layout;
use crate::sync::{Plan, SyncSettings, SyncState, TickSync};
use crate::value::{EntityView, View, read_value, write_value};

/// A predicting client's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictConfig {
    /// The length of one tick, in microseconds (16 667 at 60 Hz).
    pub tick_us: u32,
    /// Lead kept on top of the round trip and the jitter, in milliseconds
    /// (default two ticks: it also covers one lost input datagram).
    pub margin_ms: u32,
    /// The round trip assumed until the transport has measured one, in
    /// milliseconds.
    pub assumed_rtt_ms: u32,
    /// How many ticks behind its target the prediction may fall before it
    /// jumps forward.
    pub jump_ticks: u32,
    /// Ticks predicted in one frame at most, outside a jump.
    pub max_steps: u32,
    /// Ticks one jump predicts at most.
    pub max_catch_up: u32,
    /// Ticks predicted past the newest confirmation at most; beyond, the
    /// prediction waits for the host.
    pub max_ahead: u32,
    /// An unowned entity this close (in metres) to an entity the local
    /// player owns is predicted with it; it stops being predicted beyond
    /// 1.5 times this.
    pub touch_radius: f32,
    /// How far behind the host's clock others are drawn, in milliseconds.
    pub interpolation_delay_ms: u32,
    /// How long past its newest known state an entity is extrapolated, in
    /// milliseconds, before it is held.
    pub max_extrapolation_ms: u32,
    /// The share of a correction's smoothing offset removed per tick of
    /// frame time (0 to 1; 1 is no smoothing).
    pub smoothing: f32,
    /// A correction larger than this, in metres, is shown as a jump.
    pub snap_distance: f32,
}

impl PredictConfig {
    /// The defaults for ticks of `tick_us` microseconds.
    pub fn new(tick_us: u32) -> Self {
        let tick_ms = tick_us.div_ceil(1000);
        Self {
            tick_us,
            margin_ms: 2 * tick_us.div_ceil(1000),
            assumed_rtt_ms: 100,
            jump_ticks: 4,
            max_steps: 4,
            max_catch_up: 120,
            max_ahead: 120,
            touch_radius: 2.0,
            interpolation_delay_ms: 100.max(6 * tick_ms),
            max_extrapolation_ms: 100,
            smoothing: 0.2,
            snap_distance: 4.0,
        }
    }

    /// The defaults at `hz` ticks per second.
    pub fn at_rate(hz: u32) -> Self {
        Self::new(1_000_000 / hz.max(1))
    }

    /// Checks every setting against its range.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String, hint: &str| {
            Err(
                Diagnostic::error("net_config_invalid", message, hint.to_owned())
                    .at_path(format!("/{field}")),
            )
        };
        if !(1000..=1_000_000).contains(&self.tick_us) {
            return bad(
                "tick_us",
                format!("tick_us is {}", self.tick_us),
                "Give the length of one tick in microseconds, 1000 to 1 000 000 (16 667 at 60 Hz).",
            );
        }
        if self.margin_ms > 1000 || self.assumed_rtt_ms > 10_000 {
            return bad(
                "margin_ms",
                format!(
                    "margin_ms is {} and assumed_rtt_ms is {}",
                    self.margin_ms, self.assumed_rtt_ms
                ),
                "Keep the margin within 1 s and the assumed round trip within 10 s.",
            );
        }
        if !(1..=600).contains(&self.jump_ticks)
            || !(1..=60).contains(&self.max_steps)
            || !(1..=1200).contains(&self.max_catch_up)
            || !(1..=1024).contains(&self.max_ahead)
        {
            return bad(
                "max_ahead",
                format!(
                    "jump_ticks {}, max_steps {}, max_catch_up {}, max_ahead {}",
                    self.jump_ticks, self.max_steps, self.max_catch_up, self.max_ahead
                ),
                "Use jump_ticks 1 to 600, max_steps 1 to 60, max_catch_up 1 to 1200 and max_ahead 1 to 1024 ticks.",
            );
        }
        if !(self.touch_radius.is_finite() && (0.0..=1000.0).contains(&self.touch_radius)) {
            return bad(
                "touch_radius",
                format!("touch_radius is {}", self.touch_radius),
                "Give a distance from 0 to 1000 m (the default is 2).",
            );
        }
        if self.interpolation_delay_ms > 2000 || self.max_extrapolation_ms > 2000 {
            return bad(
                "interpolation_delay_ms",
                format!(
                    "interpolation_delay_ms is {} and max_extrapolation_ms is {}",
                    self.interpolation_delay_ms, self.max_extrapolation_ms
                ),
                "Keep the interpolation delay and the extrapolation within 2 s.",
            );
        }
        if !(self.smoothing.is_finite() && self.smoothing > 0.0 && self.smoothing <= 1.0)
            || !(self.snap_distance.is_finite() && self.snap_distance > 0.0)
        {
            return bad(
                "smoothing",
                format!(
                    "smoothing is {} and snap_distance is {}",
                    self.smoothing, self.snap_distance
                ),
                "Give smoothing above 0 and at most 1 (the default is 0.2) and a snap distance above 0 m.",
            );
        }
        Ok(())
    }

    fn sync(&self) -> SyncSettings {
        SyncSettings {
            tick_us: u64::from(self.tick_us),
            margin_us: u64::from(self.margin_ms) * 1000,
            assumed_rtt_us: u64::from(self.assumed_rtt_ms) * 1000,
            jump_ticks: u64::from(self.jump_ticks),
            max_steps: u64::from(self.max_steps),
            max_catch_up: u64::from(self.max_catch_up),
        }
    }

    /// Milliseconds as thousandths of a tick.
    fn milli_ticks(&self, ms: u32) -> u64 {
        u64::from(ms) * 1_000_000 / u64::from(self.tick_us.max(1))
    }
}

/// What a predicting client did, counted since it was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PredictStats {
    /// Ticks predicted for the first time.
    pub predicted_ticks: u64,
    /// Confirmations compared with the prediction.
    pub confirmations: u64,
    /// Confirmations that equalled the prediction exactly.
    pub exact: u64,
    /// Confirmations that differed: the prediction was replayed from the
    /// confirmed state.
    pub corrections: u64,
    /// Ticks predicted again by corrections.
    pub replayed_ticks: u64,
    /// The most ticks one correction predicted again.
    pub longest_replay: u64,
    /// The largest difference in position a correction found, in
    /// millimetres.
    pub largest_correction_mm: u64,
    /// Times the prediction had fallen behind the host and was set to a
    /// confirmed state it had not predicted yet.
    pub rebases: u64,
    /// Jumps of tick sync.
    pub jumps: u64,
    /// Ticks predicted with the actions held before them, because their
    /// frame could not reach the host in time (the start, jumps).
    pub held_ticks: u64,
    /// Frames the prediction waited because it was `max_ahead` ticks past
    /// the newest confirmation.
    pub stalls: u64,
    /// Entities that started or stopped being predicted.
    pub membership_changes: u64,
    /// Values the prediction world refused (an invalid value).
    pub write_failures: u64,
}

/// One frame of a predicting client.
#[derive(Debug, Clone, Default)]
pub struct FrameReport {
    /// Ticks predicted this frame (new ones, not replays).
    pub steps: u64,
    /// Whether tick sync jumped forward.
    pub jumped: bool,
    /// Whether a confirmation differed and the prediction was replayed.
    pub corrected: bool,
    /// Systems that failed while predicting (the app disables them until a
    /// correction restores their state).
    pub failures: Vec<Diagnostic>,
}

/// How one entity is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    /// The transform to draw (the smoothing offset included).
    pub transform: Transform,
    /// Where it comes from.
    pub source: Source,
    /// The smoothing offset in it, in metres (zero when nothing was
    /// corrected lately).
    pub offset: Vec3,
}

/// One replicated type in the prediction world.
#[derive(Debug, Clone)]
struct PredictType {
    id: ComponentId,
    layout: Layout,
}

/// What the prediction kept of one predicted tick.
#[derive(Debug, Clone)]
struct Saved {
    /// The app after predicting the tick.
    snapshot: AppSnapshot,
    /// Network id → prediction-world entity at that tick.
    ids: BTreeMap<u32, Entity>,
    /// The predicted entities at that tick.
    members: BTreeSet<u32>,
    /// Their states (an entity the prediction no longer has is missing).
    views: View,
}

/// A client's prediction, reconciliation, interpolation and tick sync.
pub struct Predictor {
    config: PredictConfig,
    app: App,
    sync: TickSync,
    player: Option<PlayerId>,
    types: Option<Vec<PredictType>>,
    transform_ty: Option<u16>,
    owner_ty: Option<u16>,
    /// Network id → prediction-world entity, and back.
    ids: BTreeMap<u32, Entity>,
    nets: BTreeMap<Entity, u32>,
    members: BTreeSet<u32>,
    /// The newest predicted tick.
    tick: Option<u64>,
    /// The newest confirmed tick.
    confirmed: Option<u64>,
    /// The local player's actions per predicted tick not confirmed yet.
    inputs: BTreeMap<u64, Actions>,
    /// The actions of the newest predicted tick.
    last_actions: Actions,
    /// The oldest tick from which every predicted tick's frame was sent (a
    /// jump sends only the newest eight).
    sent_from: Option<u64>,
    /// The newest tick predicted with held actions (its frame not sent).
    skipped: u64,
    history: BTreeMap<u64, Saved>,
    /// What each entity that is not predicted was last set to.
    context: BTreeMap<u32, Arc<EntityView>>,
    /// Entities whose prediction was replaced this frame.
    corrected: BTreeSet<u32>,
    presented: BTreeMap<u32, Shown>,
    last_present_us: Option<u64>,
    display: Option<u64>,
    diagnostics: VecDeque<Diagnostic>,
    stats: PredictStats,
}

impl std::fmt::Debug for Predictor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Predictor")
            .field("tick", &self.tick)
            .field("confirmed", &self.confirmed)
            .field("members", &self.members)
            .field("stats", &self.stats)
            .finish()
    }
}

impl Predictor {
    /// A predictor running the game's systems in `app`. The app's world
    /// must register the replicated types as the host's does, and hold the
    /// project's `ActionState` (the action ids) when the game has actions;
    /// it should hold no entities (they come from the host).
    pub fn new(app: App, config: PredictConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            sync: TickSync::new(config.sync()),
            config,
            app,
            player: None,
            types: None,
            transform_ty: None,
            owner_ty: None,
            ids: BTreeMap::new(),
            nets: BTreeMap::new(),
            members: BTreeSet::new(),
            tick: None,
            confirmed: None,
            inputs: BTreeMap::new(),
            last_actions: Actions::new(),
            sent_from: None,
            skipped: 0,
            history: BTreeMap::new(),
            context: BTreeMap::new(),
            corrected: BTreeSet::new(),
            presented: BTreeMap::new(),
            last_present_us: None,
            display: None,
            diagnostics: VecDeque::new(),
            stats: PredictStats::default(),
        })
    }

    /// The settings.
    pub fn config(&self) -> &PredictConfig {
        &self.config
    }

    /// The prediction app (its world is the prediction world).
    pub fn app(&self) -> &App {
        &self.app
    }

    /// The newest predicted tick, once the first confirmation arrived.
    pub fn tick(&self) -> Option<u64> {
        self.tick
    }

    /// The newest confirmed tick the prediction reconciled with.
    pub fn confirmed_tick(&self) -> Option<u64> {
        self.confirmed
    }

    /// What it did so far.
    pub fn stats(&self) -> PredictStats {
        self.stats
    }

    /// What tick sync knows and decided last.
    pub fn sync(&self) -> SyncState {
        self.sync.state()
    }

    /// The network ids predicted now.
    pub fn predicted(&self) -> &BTreeSet<u32> {
        &self.members
    }

    /// How every entity the client shows is drawn this frame, by network
    /// id.
    pub fn presented(&self) -> &BTreeMap<u32, Shown> {
        &self.presented
    }

    /// The host tick others were drawn at this frame, in thousandths of a
    /// tick (the host's clock less the interpolation delay).
    pub fn display_time(&self) -> Option<u64> {
        self.display
    }

    /// The largest smoothing offset drawn this frame, in metres.
    pub fn largest_offset(&self) -> f32 {
        self.presented
            .values()
            .map(|s| s.offset.length())
            .fold(0.0, f32::max)
    }

    /// A stable hash of the prediction app (world, tick, systems' state).
    pub fn hash(&self) -> Hash128 {
        self.app.hash()
    }

    /// The diagnostics since the last call.
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    fn note(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    /// The predicted entities' states at the newest predicted tick, read
    /// back as the host's form (network ids, canonical values): equal to the
    /// host's state of that tick for those entities when the prediction was
    /// right.
    pub fn predicted_view(&self) -> Result<View> {
        self.read_members()
    }

    /// One frame, after `endpoint.update(now_ms)` and `client.pump`:
    /// reconciles with what the host confirmed, predicts the ticks tick sync
    /// asks for with this frame's `actions` (the local player's actions not
    /// at rest), sends them stamped with their ticks, and works out how
    /// everything is drawn.
    pub fn frame<T: Transport>(
        &mut self,
        client: &mut Client,
        endpoint: &mut Endpoint<T>,
        now_ms: u64,
        actions: &Actions,
    ) -> Result<FrameReport> {
        let now_us = now_ms.saturating_mul(1000);
        let mut report = FrameReport::default();
        self.sync.observe(now_us, client.newest_seen());
        if self.player.is_none() {
            self.player = client.player();
        }
        if let Some(confirmed) = client.take_confirmed() {
            self.ensure_types(client)?;
            self.reconcile(confirmed, &mut report)?;
        }
        if let Some(predicted) = self.tick {
            let rtt = endpoint.rtt_ms(PeerId::HOST).map(|ms| ms * 1000);
            let Plan {
                steps,
                jumped,
                in_time_from,
            } = self.sync.plan(now_us, rtt, predicted);
            let limit = self
                .confirmed
                .map_or(predicted, |c| c + u64::from(self.config.max_ahead));
            let allowed = steps.min(limit.saturating_sub(predicted));
            if allowed < steps {
                self.stats.stalls += 1;
            }
            if jumped {
                self.stats.jumps += 1;
                report.jumped = true;
            }
            // A tick whose frame cannot reach the host before the host
            // simulates it (the start, a jump, a client fallen behind) is
            // predicted with the actions in force before, and its frame is
            // not sent: the host holds the frame in force for every tick it
            // has no newer frame for, which are those actions. The frame's
            // own actions start with the first tick that arrives in time.
            let held = self.last_actions.clone();
            for _ in 0..allowed {
                let tick = self.tick.map_or(0, |t| t + 1);
                let in_time = tick >= in_time_from;
                let used = if in_time { actions } else { &held };
                self.inputs.insert(tick, used.clone());
                self.simulate(tick, used, &mut report)?;
                self.stats.predicted_ticks += 1;
                if in_time {
                    client.queue_frame(tick, used.clone());
                    // From here on every tick's frame is sent.
                    if self.sent_from.is_none_or(|from| self.skipped > from) {
                        self.sent_from = Some(tick);
                    }
                    self.last_actions = used.clone();
                } else {
                    self.skipped = tick;
                    self.stats.held_ticks += 1;
                }
                report.steps += 1;
            }
        }
        client.send_queued(endpoint)?;
        self.present(client, now_us);
        Ok(report)
    }

    /// Resolves the client's replicated types in the prediction world.
    fn ensure_types(&mut self, client: &Client) -> Result<()> {
        if self.types.is_some() {
            return Ok(());
        }
        let names = client.type_names();
        let layouts = client.layouts();
        let registry = self.app.world().registry();
        let mut types = Vec::with_capacity(names.len());
        let mut wrong = Vec::new();
        for (name, layout) in names.iter().zip(&layouts) {
            let found = registry.component_id(name).and_then(|id| {
                let info = registry.component(id)?;
                let own = Layout::of(info.schema()).ok()?;
                (own == *layout).then_some(PredictType { id, layout: own })
            });
            match found {
                Some(ty) => types.push(ty),
                None => wrong.push((*name).to_owned()),
            }
        }
        if !wrong.is_empty() {
            return Err(Diagnostic::error(
                "net_types_mismatch",
                format!(
                    "the prediction app's world lacks replicated types, or has them with another layout: {}",
                    wrong.join(", ")
                ),
                "Build the prediction app with the same function as the host's app, so it registers every replicated type.",
            )
            .with_candidates(wrong));
        }
        self.transform_ty = names
            .iter()
            .position(|n| *n == "Transform")
            .and_then(|i| u16::try_from(i).ok());
        self.owner_ty = names
            .iter()
            .position(|n| *n == "NetOwner")
            .and_then(|i| u16::try_from(i).ok());
        self.types = Some(types);
        Ok(())
    }

    fn reconcile(&mut self, confirmed: Confirmed, report: &mut FrameReport) -> Result<()> {
        let tick = confirmed.tick;
        if self.confirmed.is_some_and(|c| tick <= c) {
            return Ok(());
        }
        // The host applied an older frame than this tick's, which was sent:
        // it came late, so tick sync must lead more.
        if self.tick.is_some_and(|p| tick <= p) && self.sent_from.is_some_and(|s| tick >= s) {
            let applied = confirmed
                .input_ack
                .unwrap_or(0)
                .max(self.sent_from.map_or(0, |s| s.saturating_sub(1)));
            if applied < tick {
                let predicted = self.tick.unwrap_or(tick);
                self.sync.late(tick - applied, tick, predicted);
            }
        }
        let predicted = match self.tick {
            Some(predicted) if tick <= predicted => predicted,
            _ => {
                // The start, or the host got ahead of the prediction: take
                // the confirmed state as it is and predict on from there.
                // While it has not predicted ahead yet (waiting for the
                // round trip) this is just the newest state, no correction.
                let ahead = matches!((self.tick, self.confirmed), (Some(p), Some(c)) if p > c);
                if ahead {
                    self.stats.rebases += 1;
                    self.corrected.extend(self.members.iter().copied());
                }
                self.app.set_tick(tick + 1);
                self.history.clear();
                self.inputs.retain(|t, _| *t > tick);
                self.apply(&confirmed.view, true);
                self.tick = Some(tick);
                self.confirmed = Some(tick);
                self.update_members();
                if ahead {
                    self.corrected.extend(self.members.iter().copied());
                }
                return Ok(());
            }
        };
        self.stats.confirmations += 1;
        let saved = self.history.remove(&tick);
        let differs = saved
            .as_ref()
            .is_none_or(|saved| self.differs(saved, &confirmed.view));
        self.history.retain(|t, _| *t > tick);
        self.inputs.retain(|t, _| *t > tick);
        self.confirmed = Some(tick);
        if !differs {
            self.stats.exact += 1;
            self.apply(&confirmed.view, false);
            return Ok(());
        }
        // A correction: back to the confirmed state, then the unconfirmed
        // inputs again.
        self.stats.corrections += 1;
        report.corrected = true;
        if let Some(saved) = &saved {
            let largest = self.largest_difference(saved, &confirmed.view);
            self.stats.largest_correction_mm = self.stats.largest_correction_mm.max(largest);
        }
        self.corrected.extend(self.members.iter().copied());
        match saved {
            Some(saved) => {
                self.app.restore(&saved.snapshot)?;
                self.ids = saved.ids;
                self.nets = self.ids.iter().map(|(n, e)| (*e, *n)).collect();
            }
            None => self.app.set_tick(tick + 1),
        }
        self.history.clear();
        self.apply(&confirmed.view, true);
        self.tick = Some(tick);
        self.update_members();
        let replay = predicted - tick;
        for next in tick + 1..=predicted {
            let actions = self.inputs.get(&next).cloned().unwrap_or_default();
            self.simulate(next, &actions, report)?;
        }
        self.stats.replayed_ticks += replay;
        self.stats.longest_replay = self.stats.longest_replay.max(replay);
        self.corrected.extend(self.members.iter().copied());
        Ok(())
    }

    /// Whether the host's view at a tick differs from what was predicted
    /// for it: a predicted entity's state, or an entity of this player the
    /// prediction did not have.
    fn differs(&self, saved: &Saved, view: &View) -> bool {
        for net in &saved.members {
            let same = match (saved.views.get(net), view.get(net)) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || **a == **b,
                (None, None) => true,
                _ => false,
            };
            if !same {
                return true;
            }
        }
        let (Some(owner), Some(me)) = (self.owner_ty, self.player) else {
            return false;
        };
        view.iter().any(|(net, state)| {
            !saved.members.contains(net)
                && state
                    .get(&owner)
                    .and_then(|v| NetOwner::decode(&v.bytes))
                    .is_some_and(|o| o.player == me)
        })
    }

    /// The largest distance between a predicted and a confirmed position,
    /// in millimetres.
    fn largest_difference(&self, saved: &Saved, view: &View) -> u64 {
        let Some(ty) = self.transform_ty else {
            return 0;
        };
        let mut largest = 0.0f32;
        for net in &saved.members {
            let a = saved
                .views
                .get(net)
                .and_then(|v| interpolate::transform_of(v, ty));
            let b = view.get(net).and_then(|v| interpolate::transform_of(v, ty));
            if let (Some(a), Some(b)) = (a, b) {
                let d = a.translation.distance(b.translation);
                if d.is_finite() {
                    largest = largest.max(d);
                }
            }
        }
        (largest * 1000.0).round() as u64
    }

    /// Writes the host's `view` into the prediction world: every entity
    /// when `all` (a reset to that tick), else only those not predicted
    /// (they follow the host; the predicted ones are ahead of it).
    fn apply(&mut self, view: &View, all: bool) {
        let Some(types) = self.types.clone() else {
            return;
        };
        if all {
            self.context.clear();
        }
        let world = self.app.world_mut();
        // A restore can bring an entity index back: forget handles of
        // entities that are gone before spawning anything.
        self.ids.retain(|_, e| world.contains(*e));
        self.nets.retain(|e, _| world.contains(*e));
        let leaving: Vec<(u32, Entity)> = self
            .ids
            .iter()
            .filter(|(net, _)| !view.contains_key(net) && (all || !self.members.contains(net)))
            .map(|(n, e)| (*n, *e))
            .collect();
        for (net, entity) in leaving {
            let _ = world.despawn(entity);
            self.ids.remove(&net);
            self.nets.remove(&entity);
            self.context.remove(&net);
        }
        let mut problems = Vec::new();
        for net in view.keys() {
            if self.ids.contains_key(net) {
                continue;
            }
            match world.spawn() {
                Ok(entity) => {
                    self.ids.insert(*net, entity);
                    self.nets.insert(entity, *net);
                }
                Err(problem) => problems.push(problem),
            }
        }
        let ids = &self.ids;
        let map = |net: u32| ids.get(&net).copied().unwrap_or(Entity::NULL);
        let mut written = Vec::new();
        for (net, state) in view {
            if !all && self.members.contains(net) {
                continue;
            }
            if !all && self.context.get(net).is_some_and(|c| Arc::ptr_eq(c, state)) {
                continue;
            }
            let Some(&entity) = ids.get(net) else {
                continue;
            };
            for (index, ty) in types.iter().enumerate() {
                let outcome = match u16::try_from(index).ok().and_then(|i| state.get(&i)) {
                    Some(value) => write_value(world, entity, ty.id, &ty.layout, value, &map),
                    None => world.remove_by_id(entity, ty.id).map(|_| ()),
                };
                if let Err(problem) = outcome {
                    problems.push(problem);
                }
            }
            written.push((*net, Arc::clone(state)));
        }
        self.context.extend(written);
        for problem in problems {
            self.stats.write_failures += 1;
            self.note(problem);
        }
    }

    /// Decides what is predicted, from the prediction world's positions:
    /// what the local player owns, and unowned entities near it.
    fn update_members(&mut self) {
        let Some(me) = self.player else {
            return;
        };
        let world = self.app.world();
        let mut next = BTreeSet::new();
        let mut owned = Vec::new();
        for (net, entity) in &self.ids {
            if world
                .get::<NetOwner>(*entity)
                .is_some_and(|o| o.player == me)
            {
                next.insert(*net);
                if let Some(t) = world.get::<Transform>(*entity) {
                    owned.push(t.translation);
                }
            }
        }
        let touch = self.config.touch_radius;
        for (net, entity) in &self.ids {
            if world.get::<NetOwner>(*entity).is_some() {
                continue;
            }
            let Some(t) = world.get::<Transform>(*entity) else {
                continue;
            };
            let radius = if self.members.contains(net) {
                touch * 1.5
            } else {
                touch
            };
            if owned
                .iter()
                .any(|p| p.distance_squared(t.translation) <= radius * radius)
            {
                next.insert(*net);
            }
        }
        if next != self.members {
            let changed = next.symmetric_difference(&self.members).count();
            self.stats.membership_changes += changed as u64;
            self.members = next;
        }
    }

    /// Predicts tick `tick` with `actions` and keeps what it needs to go
    /// back to it.
    fn simulate(&mut self, tick: u64, actions: &Actions, report: &mut FrameReport) -> Result<()> {
        let Some(me) = self.player else {
            return Ok(());
        };
        let world = self.app.world_mut();
        input::ensure_players(world, me + 1)?;
        input::write_players(world, &|player| (player == me).then(|| actions.clone()))?;
        if self.app.tick() != tick {
            self.app.set_tick(tick);
        }
        let step = self.app.step()?;
        report.failures.extend(step.failures);
        // The prediction world's own spawns and despawns do not touch the
        // mapping, except entities that are gone.
        let world = self.app.world();
        self.ids.retain(|_, e| world.contains(*e));
        self.nets.retain(|e, _| world.contains(*e));
        self.tick = Some(tick);
        self.update_members();
        let views = self.read_members()?;
        self.history.insert(
            tick,
            Saved {
                snapshot: self.app.snapshot(),
                ids: self.ids.clone(),
                members: self.members.clone(),
                views,
            },
        );
        Ok(())
    }

    /// The predicted entities' states, in the host's form.
    fn read_members(&self) -> Result<View> {
        let mut view = View::new();
        let Some(types) = &self.types else {
            return Ok(view);
        };
        let world = self.app.world();
        for net in &self.members {
            let Some(&entity) = self.ids.get(net) else {
                continue;
            };
            if !world.contains(entity) {
                continue;
            }
            let mut state = EntityView::new();
            for (index, ty) in types.iter().enumerate() {
                let value = read_value(world, entity, ty.id, &ty.layout, &mut |e| {
                    self.nets.get(&e).copied().unwrap_or(0)
                })?;
                if let (Some(value), Ok(index)) = (value, u16::try_from(index)) {
                    state.insert(index, Arc::new(value));
                }
            }
            view.insert(*net, Arc::new(state));
        }
        Ok(view)
    }

    /// How every entity the client shows is drawn: predicted ones from the
    /// prediction world, the others interpolated; a correction or a switch
    /// between the two is smoothed by an offset that decays.
    fn present(&mut self, client: &Client, now_us: u64) {
        let elapsed = self
            .last_present_us
            .map_or(0, |last| now_us.saturating_sub(last));
        self.last_present_us = Some(now_us);
        let Some(ty) = self.transform_ty else {
            self.presented.clear();
            self.corrected.clear();
            return;
        };
        let decay = (self.config.smoothing * elapsed as f32 / self.config.tick_us.max(1) as f32)
            .clamp(0.0, 1.0);
        let display = self.sync.host_clock_milli(now_us).map(|clock| {
            clock.saturating_sub(self.config.milli_ticks(self.config.interpolation_delay_ms))
        });
        self.display = display;
        let extrapolation = self.config.milli_ticks(self.config.max_extrapolation_ms);
        let world = self.app.world();
        let mut next = BTreeMap::new();
        for net in client.shown() {
            let raw = if self.members.contains(&net) {
                self.ids
                    .get(&net)
                    .and_then(|e| world.get::<Transform>(*e))
                    .map(|t| (*t, Source::Predicted))
            } else {
                display.and_then(|at| {
                    let states = client.states(net)?;
                    interpolate::sample(states, client.complete_ticks(), ty, at, extrapolation)
                })
            };
            let Some((mut transform, source)) = raw else {
                continue;
            };
            let previous = self.presented.get(&net);
            let jumped = previous.is_some_and(|p| {
                p.source.is_predicted() != source.is_predicted()
                    || (source.is_predicted() && self.corrected.contains(&net))
            });
            let mut offset = match previous {
                Some(previous) if jumped => {
                    let jump = previous.transform.translation - transform.translation;
                    if jump.is_finite() && jump.length() <= self.config.snap_distance {
                        jump
                    } else {
                        Vec3::ZERO
                    }
                }
                Some(previous) => previous.offset * (1.0 - decay),
                None => Vec3::ZERO,
            };
            if !offset.is_finite() || offset.length() < 1e-4 {
                offset = Vec3::ZERO;
            }
            transform.translation += offset;
            next.insert(
                net,
                Shown {
                    transform,
                    source,
                    offset,
                },
            );
        }
        self.presented = next;
        self.corrected.clear();
    }
}

impl Predictor {
    /// The prediction world's entity for network id `net`.
    pub fn entity(&self, net: u32) -> Option<Entity> {
        self.ids.get(&net).copied()
    }

    /// The prediction world.
    pub fn world(&self) -> &World {
        self.app.world()
    }
}
