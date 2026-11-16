//! Particles (design note M6 §8): sparks, smoke, dust.
//!
//! Particles keep no state. Where each one is follows from the emitter, the
//! time since it started and the particle's number (a closed formula seeded
//! by `seed`), so the simulation stores nothing per particle: snapshots,
//! forks and replays cover them for free, and drawing works them out for
//! the moment shown. They do not collide and do not affect the game. They
//! move with their emitter (they are placed in its space).

use wish_core::Color;
use wish_core::math::Vec3;
use wish_schema::component;

/// Gives off particles from its entity, in a cone around the entity's +Y
/// axis.
#[component]
pub struct ParticleEmitter {
    /// Whether it gives off particles; while false none are shown.
    #[schema(default = true)]
    pub playing: bool,
    /// The simulation time (`Time.elapsed`) it started at; write the current
    /// time to start it again.
    #[schema(unit = "s")]
    pub start_time: f32,
    /// How long it keeps giving off particles after starting (0: for ever).
    #[schema(unit = "s", range = 0.0..=100000.0)]
    pub duration: f32,
    /// Particles given off per second.
    #[schema(unit = "1/s", range = 0.0..=10000.0, default = 20.0)]
    pub rate: f32,
    /// Particles given off at once when it starts.
    #[schema(range = 0..=10000)]
    pub burst: u32,
    /// How long each particle lives.
    #[schema(unit = "s", range = 0.001..=1000.0, default = 1.0)]
    pub lifetime: f32,
    /// How fast particles leave.
    #[schema(unit = "m/s", range = 0.0..=1000.0, default = 2.0)]
    pub speed: f32,
    /// How much the speed varies from one particle to the next (0: none, 1:
    /// anything from 0 to twice `speed`).
    #[schema(range = 0.0..=1.0, default = 0.2)]
    pub speed_variation: f32,
    /// The cone's half angle around +Y: 0 shoots straight up, 180 in every
    /// direction.
    #[schema(unit = "deg", range = 0.0..=180.0, default = 25.0)]
    pub spread_deg: f32,
    /// Acceleration of every particle (gravity, wind), in world space.
    #[schema(unit = "m/s2")]
    pub gravity: Vec3,
    /// A particle's size when it is born.
    #[schema(unit = "m", range = 0.0..=1000.0, default = 0.2)]
    pub size_start: f32,
    /// A particle's size when it dies.
    #[schema(unit = "m", range = 0.0..=1000.0, default = 0.0)]
    pub size_end: f32,
    /// A particle's colour when it is born (linear RGB, and alpha).
    #[schema(default = Color::WHITE)]
    pub color_start: Color,
    /// A particle's colour when it dies.
    #[schema(default = Color::rgba(1.0, 1.0, 1.0, 0.0))]
    pub color_end: Color,
    /// The most particles alive at once (the oldest are left out).
    #[schema(range = 0..=100000, default = 1000)]
    pub max: u32,
    /// Seeds the particles' directions and speeds: emitters with different
    /// seeds look different.
    pub seed: u32,
}

/// How many particles an emitter has alive, written by the engine every
/// tick.
#[component]
pub struct ParticleState {
    /// Particles alive now.
    pub alive: u32,
}
