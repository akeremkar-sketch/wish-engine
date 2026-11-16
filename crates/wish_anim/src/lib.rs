//! Skeletal animation in the simulation (design note M6 §6): every tick the
//! [`AnimationPlayer`] of each entity moves on by the tick's time times its
//! speed, loops or stops at the clip's end and says so
//! ([`AnimationFinished`]). The pose itself is worked out from the time
//! when drawing (`wish_render`), so the world holds only what decides the
//! future: clip, time, speed, looping, playing.
//!
//! Clip lengths come from the model files, which the control plane reads
//! when a session starts ([`install`]); they are working state (ADR-026).

use std::collections::BTreeMap;

use wish_core::{Entity, Result};
use wish_ecs::World;
pub use wish_types::anim::{AnimationFinished, AnimationPlayer};
use wish_types::render::Model;
use wish_types::time::Time;

/// The name of the animation system in an app.
pub const SYSTEM: &str = "animation";

/// Registers the animation component and event.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<AnimationPlayer>()?;
    world.register_event::<AnimationFinished>()?;
    Ok(())
}

/// The animation system, for `App::add_system`.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, _| step(world))
}

/// How long each clip of each model is: `(model path, clip name)` →
/// seconds. The empty clip name stands for a model's first clip.
#[derive(Clone, Debug, Default)]
pub struct ClipLengths {
    seconds: BTreeMap<(String, String), f32>,
}

impl ClipLengths {
    /// The length of `clip` in `model`, if known.
    pub fn get(&self, model: &str, clip: &str) -> Option<f32> {
        self.seconds
            .get(&(model.to_owned(), clip.to_owned()))
            .copied()
    }
}

/// Tells the world how long the clips of its models are.
pub fn install(world: &mut World, clips: impl IntoIterator<Item = ((String, String), f32)>) {
    let mut lengths = world
        .extension::<ClipLengths>()
        .cloned()
        .unwrap_or_default();
    lengths.seconds.extend(clips);
    world.insert_extension(lengths);
}

/// One animation tick.
pub fn step(world: &mut World) -> Result<()> {
    let delta = world.resource::<Time>().map_or(1.0 / 60.0, |t| t.delta);
    let lengths = world
        .extension::<ClipLengths>()
        .cloned()
        .unwrap_or_default();
    let mut players: Vec<Entity> = world
        .query::<(Entity, &AnimationPlayer)>()?
        .map(|(entity, _)| entity)
        .collect();
    players.sort_unstable();
    for entity in players {
        let Some(player) = world.get::<AnimationPlayer>(entity).copied() else {
            continue;
        };
        if !player.playing {
            continue;
        }
        let model = world
            .get::<Model>(entity)
            .and_then(|m| world.text(m.asset.text()))
            .unwrap_or_default()
            .to_owned();
        let clip = world.text(player.clip).unwrap_or_default().to_owned();
        let speed = if player.speed.is_finite() {
            player.speed
        } else {
            1.0
        };
        let mut next = player;
        let time = if player.time.is_finite() {
            player.time
        } else {
            0.0
        };
        next.time = time + delta * speed;
        let mut finished = false;
        if let Some(length) = lengths.get(&model, &clip).filter(|l| *l > 0.0) {
            if player.looping {
                next.time = next.time.rem_euclid(length);
            } else if next.time >= length || next.time <= 0.0 && speed < 0.0 {
                next.time = next.time.clamp(0.0, length);
                next.playing = false;
                finished = true;
            }
        }
        if next != player {
            world.insert(entity, next)?;
        }
        if finished {
            world.send(AnimationFinished { entity })?;
        }
    }
    Ok(())
}
