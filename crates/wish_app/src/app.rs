//! The app: a world, its systems and the fixed-step loop.

use std::collections::BTreeSet;

use wish_core::{Diagnostic, Result, Rng, StableHasher};
use wish_ecs::{ChangeTick, World};

use crate::schedule::{System, SystemContext, order};
use crate::time::Time;

/// How an app runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppConfig {
    /// The seed every random stream is derived from.
    pub seed: u64,
    /// Fixed steps per second of simulated time.
    pub tick_rate_hz: u32,
    /// The most steps [`App::update`] runs for one call, so that a slow frame
    /// cannot make the simulation fall further and further behind.
    pub max_steps_per_update: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            tick_rate_hz: 60,
            max_steps_per_update: 8,
        }
    }
}

/// What the app keeps per system between ticks. All of it affects what the
/// system does next, so it belongs to the simulation state (and to snapshots).
#[derive(Clone, Debug, PartialEq)]
pub struct SystemState {
    /// The system's random stream.
    pub rng: Rng,
    /// The change tick at the end of its previous run.
    pub last_run: ChangeTick,
    /// Why the system was disabled, if it was.
    pub disabled: Option<Diagnostic>,
}

/// What one tick did.
#[derive(Clone, Debug, Default)]
pub struct StepReport {
    /// The tick that was simulated.
    pub tick: u64,
    /// Systems that failed during this tick and were disabled.
    pub failures: Vec<Diagnostic>,
}

/// What one [`App::update`] call did.
#[derive(Clone, Debug, Default)]
pub struct UpdateReport {
    /// How many ticks were simulated.
    pub steps: u32,
    /// Systems that failed during those ticks and were disabled.
    pub failures: Vec<Diagnostic>,
    /// Ticks owed but skipped because of `max_steps_per_update`.
    pub skipped_steps: u64,
    /// How far simulated time is into the next tick, from 0 to 1. Renderers
    /// use it to interpolate between the last two ticks.
    pub alpha: f32,
}

/// The seed of a system's random stream: a stable hash of the app seed and
/// the system name, so streams are independent of each other and of the
/// order systems were added in.
fn stream_seed(seed: u64, name: &str) -> u64 {
    let mut hasher = StableHasher::new();
    hasher.write_u64(seed);
    hasher.write_str(name);
    let hash = hasher.finish().as_u128();
    (hash as u64) ^ ((hash >> 64) as u64)
}

fn system_failed(name: &str, tick: u64, cause: &Diagnostic) -> Diagnostic {
    let mut failure = Diagnostic::error(
        "system_failed",
        format!(
            "System '{name}' failed at tick {tick} and was disabled: [{}] {}",
            cause.code, cause.message
        ),
        cause.hint.clone(),
    )
    .with_candidates(cause.candidates.clone());
    failure.location = cause.location.clone();
    failure
}

/// A world, the systems that run on it, and the fixed-step loop.
pub struct App {
    config: AppConfig,
    world: World,
    systems: Vec<System>,
    /// Parallel to `systems`.
    states: Vec<SystemState>,
    /// The run order, as indices into `systems`; recomputed after changes.
    order: Option<Vec<usize>>,
    /// The next tick to simulate.
    tick: u64,
    /// Real time owed to the simulation by [`App::update`], in ticks.
    accumulator: f64,
}

impl App {
    /// An app with an empty world and no systems.
    pub fn new(config: AppConfig) -> Self {
        let config = AppConfig {
            tick_rate_hz: config.tick_rate_hz.max(1),
            max_steps_per_update: config.max_steps_per_update.max(1),
            ..config
        };
        let mut app = Self {
            config,
            world: World::new(),
            systems: Vec::new(),
            states: Vec::new(),
            order: None,
            tick: 0,
            accumulator: 0.0,
        };
        // Inserting a built-in resource cannot fail; `step` sets it again anyway.
        let inserted = app.world.insert_resource(app.time()).is_ok();
        debug_assert!(inserted, "the Time resource failed to insert");
        app
    }

    /// How the app runs.
    pub fn config(&self) -> AppConfig {
        self.config
    }

    /// The world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The world, mutably.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The next tick to simulate (the number of ticks simulated so far).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The length of one tick, in seconds.
    pub fn delta(&self) -> f32 {
        self.time().delta
    }

    /// The time at the start of the next tick.
    pub fn time(&self) -> Time {
        Time::at(self.tick, self.config.tick_rate_hz)
    }

    /// Adds a system. Names must be unique and non-empty. Ordering
    /// constraints may name systems that are added later; they are checked
    /// when the app next runs (or by [`App::system_order`]).
    pub fn add_system(&mut self, system: System) -> Result<()> {
        if system.name.trim().is_empty() {
            return Err(Diagnostic::error(
                "invalid_system_name",
                "A system needs a name.",
                "Give it a short, unique name such as \"movement\"; it appears in diagnostics and seeds its random stream.",
            ));
        }
        if self
            .systems
            .iter()
            .any(|existing| existing.name == system.name)
        {
            return Err(Diagnostic::error(
                "duplicate_system",
                format!("A system named '{}' is already added.", system.name),
                "System names must be unique. Rename one of them.",
            ));
        }
        self.states.push(SystemState {
            rng: Rng::from_seed(stream_seed(self.config.seed, &system.name)),
            last_run: ChangeTick::ZERO,
            disabled: None,
        });
        self.systems.push(system);
        self.order = None;
        Ok(())
    }

    fn ensure_order(&mut self) -> Result<&[usize]> {
        if self.order.is_none() {
            self.order = Some(order(&self.systems)?);
        }
        Ok(self.order.as_deref().unwrap_or_default())
    }

    /// The names of the systems in the order they run.
    pub fn system_order(&mut self) -> Result<Vec<String>> {
        let order = self.ensure_order()?.to_vec();
        Ok(order
            .into_iter()
            .filter_map(|index| self.systems.get(index).map(|system| system.name.clone()))
            .collect())
    }

    /// Each system's name and state, in the order the systems were added.
    pub fn system_states(&self) -> impl Iterator<Item = (&str, &SystemState)> {
        self.systems
            .iter()
            .map(|system| system.name.as_str())
            .zip(&self.states)
    }

    /// Re-enables a system that failed, for example after its code was fixed.
    pub fn enable_system(&mut self, name: &str) -> Result<()> {
        let Some(index) = self.systems.iter().position(|system| system.name == name) else {
            let names: Vec<&str> = self.systems.iter().map(System::name).collect();
            return Err(Diagnostic::error(
                "unknown_system",
                format!("No system is named '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &names),
            )
            .with_candidates(names));
        };
        if let Some(state) = self.states.get_mut(index) {
            state.disabled = None;
        }
        Ok(())
    }

    /// Simulates one tick: every enabled system once, in order, then the
    /// end-of-tick bookkeeping (events, tick counter).
    ///
    /// A system that returns an error is disabled and reported; the tick
    /// carries on with the others. The `Err` case of this function is for
    /// problems with the app itself, such as contradictory ordering
    /// constraints, in which case nothing runs.
    pub fn step(&mut self) -> Result<StepReport> {
        let order = self.ensure_order()?.to_vec();
        let time = self.time();
        self.world.insert_resource(time)?;
        let mut report = StepReport {
            tick: self.tick,
            failures: Vec::new(),
        };
        let Self {
            world,
            systems,
            states,
            ..
        } = self;
        for index in order {
            let (Some(system), Some(state)) = (systems.get(index), states.get_mut(index)) else {
                continue;
            };
            if state.disabled.is_some() {
                continue;
            }
            let mut context = SystemContext {
                name: &system.name,
                time,
                rng: &mut state.rng,
                last_run: state.last_run,
            };
            let result = (system.run)(world, &mut context);
            state.last_run = world.change_tick();
            world.advance_change_tick();
            if let Err(cause) = result {
                let failure = system_failed(&system.name, time.tick, &cause);
                state.disabled = Some(failure.clone());
                report.failures.push(failure);
            }
        }
        self.world.update_events();
        self.tick += 1;
        Ok(report)
    }

    /// Simulates `ticks` ticks as fast as possible, independent of real
    /// time. Returns every system failure along the way.
    pub fn run(&mut self, ticks: u64) -> Result<Vec<Diagnostic>> {
        let mut failures = Vec::new();
        for _ in 0..ticks {
            failures.extend(self.step()?.failures);
        }
        Ok(failures)
    }

    /// Advances the simulation by `elapsed_seconds` of real time: runs as
    /// many whole ticks as that time covers (at most
    /// `max_steps_per_update`), and carries the remainder to the next call.
    ///
    /// Real time only decides *how many* ticks run; each tick is the same
    /// fixed step, so the simulation stays deterministic.
    pub fn update(&mut self, elapsed_seconds: f64) -> Result<UpdateReport> {
        // Real time is counted in ticks, not seconds: multiplying by the
        // integer rate avoids the rounding of the tick length (1/60 is not
        // exact). A tick due within a millionth of a tick is treated as due,
        // so 0.05 s at 60 Hz is exactly three ticks.
        const DUE: f64 = 1.0 - 1e-6;
        if elapsed_seconds.is_finite() && elapsed_seconds > 0.0 {
            self.accumulator += elapsed_seconds * f64::from(self.config.tick_rate_hz);
        }
        let mut report = UpdateReport::default();
        while self.accumulator >= DUE && report.steps < self.config.max_steps_per_update {
            report.failures.extend(self.step()?.failures);
            self.accumulator = (self.accumulator - 1.0).max(0.0);
            report.steps += 1;
        }
        if self.accumulator >= DUE {
            let owed = (self.accumulator + 1e-6).floor();
            report.skipped_steps = owed as u64;
            self.accumulator = (self.accumulator - owed).max(0.0);
        }
        report.alpha = self.accumulator.clamp(0.0, 1.0) as f32;
        Ok(report)
    }

    /// The names of disabled systems.
    pub fn disabled_systems(&self) -> BTreeSet<&str> {
        self.system_states()
            .filter(|(_, state)| state.disabled.is_some())
            .map(|(name, _)| name)
            .collect()
    }
}
