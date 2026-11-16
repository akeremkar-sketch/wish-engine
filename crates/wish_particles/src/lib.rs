//! Particles without state (design note M6 §8).
//!
//! An emitter gives off particle number `n` at a known time: the `burst`
//! ones (numbers `0..burst`) when it starts, then one every `1 / rate`
//! seconds while it emits. A particle's direction and speed come from a hash
//! of the emitter's `seed` and its number, its age from the time, and so
//! where it is and how it looks follow from a formula ([`particles`]). Nothing
//! is stored: the simulation only writes how many are alive
//! ([`ParticleState`], for tools and tests), and drawing works the particles
//! out for the moment it shows.
//!
//! The count uses only additions, multiplications and rounding of the
//! world's numbers, so it is the same on every machine; positions (drawing
//! only) also use sines and cosines.

use wish_core::math::{Mat4, Vec3};
use wish_core::{Entity, Result};
use wish_ecs::World;
pub use wish_types::particles::{ParticleEmitter, ParticleState};
use wish_types::time::Time;
use wish_types::transform::GlobalTransform;

/// The name of the particle system in an app.
pub const SYSTEM: &str = "particles";

/// Registers the particle components.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<ParticleEmitter>()?;
    world.register_component::<ParticleState>()?;
    Ok(())
}

/// The particle system, for `App::add_system`.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, _| step(world))
}

fn finite(value: f32, otherwise: f32) -> f32 {
    if value.is_finite() { value } else { otherwise }
}

/// The numbers of the particles of `emitter` alive at simulation time `now`
/// (seconds), oldest first, at most `max`.
pub fn alive(emitter: &ParticleEmitter, now: f64) -> std::ops::Range<u64> {
    if !emitter.playing {
        return 0..0;
    }
    let since = now - f64::from(finite(emitter.start_time, 0.0));
    let lifetime = f64::from(finite(emitter.lifetime, 1.0).max(0.001));
    if since < 0.0 {
        return 0..0;
    }
    let burst = u64::from(emitter.burst.min(100_000));
    // Burst particles are born at 0 and alive while younger than lifetime.
    let burst_alive = if since < lifetime {
        0..burst
    } else {
        burst..burst
    };
    let rate = f64::from(finite(emitter.rate, 0.0).clamp(0.0, 10_000.0));
    let duration = f64::from(finite(emitter.duration, 0.0).max(0.0));
    let mut first = burst_alive.start;
    let mut end = burst_alive.end;
    if rate > 0.0 {
        // Continuous particle k is born at k / rate (k from 0) while
        // emitting; it lives while since - k / rate < lifetime.
        let emitting_until = if duration > 0.0 {
            since.min(duration)
        } else {
            since
        };
        let last = (emitting_until * rate).floor();
        let oldest = ((since - lifetime) * rate).floor() + 1.0;
        let oldest = oldest.max(0.0);
        let mut last_exclusive = last + 1.0;
        if duration > 0.0 {
            // No particle is born at or after the end of the emission.
            last_exclusive = last_exclusive.min((duration * rate).ceil());
        }
        if last_exclusive > oldest {
            let from = burst + oldest as u64;
            let to = burst + last_exclusive as u64;
            if burst_alive.is_empty() {
                first = from;
            }
            end = to;
        }
    }
    let max = u64::from(emitter.max);
    if end - first > max {
        first = end - max;
    }
    first..end
}

/// How many particles of `emitter` are alive at time `now`.
pub fn count(emitter: &ParticleEmitter, now: f64) -> u32 {
    let range = alive(emitter, now);
    u32::try_from(range.end - range.start).unwrap_or(u32::MAX)
}

/// One particle, worked out for a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// Its number.
    pub number: u64,
    /// Where it is, in world space.
    pub position: Vec3,
    /// Its size in meters.
    pub size: f32,
    /// Its colour, linear RGBA.
    pub color: [f32; 4],
}

/// A well-mixed 64-bit number from a seed and a particle number
/// (SplitMix64).
fn mix(seed: u32, number: u64, salt: u64) -> u64 {
    let mut z = (u64::from(seed) << 32 ^ number)
        .wrapping_add(salt.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A number in [0, 1) from a seed and a particle number.
fn unit(seed: u32, number: u64, salt: u64) -> f32 {
    (mix(seed, number, salt) >> 40) as f32 / (1u64 << 24) as f32
}

/// When particle `number` of `emitter` was born, in seconds after it
/// started.
fn born(emitter: &ParticleEmitter, number: u64) -> f64 {
    let burst = u64::from(emitter.burst);
    if number < burst {
        0.0
    } else {
        let rate = f64::from(finite(emitter.rate, 0.0).max(1e-6));
        (number - burst) as f64 / rate
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Every particle of `emitter` alive at time `now`, for an emitter placed
/// by `world` (its `GlobalTransform`).
pub fn particles(emitter: &ParticleEmitter, world: &Mat4, now: f64) -> Vec<Particle> {
    let since = now - f64::from(finite(emitter.start_time, 0.0));
    let lifetime = finite(emitter.lifetime, 1.0).max(0.001);
    let spread = finite(emitter.spread_deg, 0.0)
        .clamp(0.0, 180.0)
        .to_radians();
    let speed = finite(emitter.speed, 0.0).max(0.0);
    let variation = finite(emitter.speed_variation, 0.0).clamp(0.0, 1.0);
    let gravity = if emitter.gravity.is_finite() {
        emitter.gravity
    } else {
        Vec3::ZERO
    };
    let origin = world.transform_point3(Vec3::ZERO);
    let up = world.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
    let side = up.any_orthonormal_vector();
    let other = up.cross(side);
    let start = emitter.color_start.to_array();
    let end = emitter.color_end.to_array();
    alive(emitter, now)
        .map(|number| {
            let age = (since - born(emitter, number)).max(0.0) as f32;
            let t = (age / lifetime).clamp(0.0, 1.0);
            // A direction in the cone, evenly over its cap.
            let cos_theta = 1.0 - unit(emitter.seed, number, 1) * (1.0 - spread.cos());
            let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
            let phi = unit(emitter.seed, number, 2) * std::f32::consts::TAU;
            let direction = up * cos_theta + (side * phi.cos() + other * phi.sin()) * sin_theta;
            let speed = speed * (1.0 + variation * (unit(emitter.seed, number, 3) * 2.0 - 1.0));
            let position = origin + direction * speed * age + gravity * (0.5 * age * age);
            let mut color = [0.0; 4];
            for (i, channel) in color.iter_mut().enumerate() {
                *channel = lerp(start[i], end[i], t);
            }
            Particle {
                number,
                position,
                size: lerp(
                    finite(emitter.size_start, 0.0),
                    finite(emitter.size_end, 0.0),
                    t,
                )
                .max(0.0),
                color,
            }
        })
        .collect()
}

/// The simulation time now: the start of the tick being drawn.
pub fn now(world: &World) -> f64 {
    world.resource::<Time>().map_or(0.0, |time| time.elapsed)
}

/// One particle tick: writes every emitter's `ParticleState` (where the
/// count changed).
pub fn step(world: &mut World) -> Result<()> {
    let now = now(world);
    let mut emitters: Vec<Entity> = world
        .entities()
        .filter(|entity| world.has::<ParticleEmitter>(*entity))
        .collect();
    emitters.sort_unstable();
    for entity in emitters {
        let Some(emitter) = world.get::<ParticleEmitter>(entity).copied() else {
            continue;
        };
        let state = ParticleState {
            alive: count(&emitter, now),
        };
        if world.get::<ParticleState>(entity) != Some(&state) {
            world.insert(entity, state)?;
        }
    }
    Ok(())
}

/// The world placement of `entity` (identity without a `GlobalTransform`).
pub fn placement(world: &World, entity: Entity) -> Mat4 {
    world
        .get::<GlobalTransform>(entity)
        .map_or(Mat4::IDENTITY, |global| global.matrix)
}
