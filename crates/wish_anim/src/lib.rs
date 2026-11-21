//! Skeletal animation in the simulation (design note M6 §6): every tick the
//! [`AnimationPlayer`] of each entity moves on by the tick's time times its
//! speed, loops or stops at the clip's end and says so
//! ([`AnimationFinished`]). The pose itself is worked out from the time
//! when drawing (`wish_render`), so the world holds only what decides the
//! future: clip, time, speed, looping, playing.
//!
//! Clip lengths come from the model files, which the control plane reads
//! when a session starts ([`install`]); they are working state (ADR-026).
//!
//! The same system plays sprite sheets (release 1.1, design note
//! `docs/design/1.1-sprites.md`): every [`SpriteAnimator`] with a clip moves
//! on the same way and writes which of the clip's frames shows into its
//! `frame`. What a sheet's clips are (how many frames, how many a second,
//! whether they loop) comes from the sheet files, which the control plane
//! reads when a session starts ([`install_sprite_clips`]); the renderer
//! turns the frame into a rectangle of the sheet's picture.

use std::collections::BTreeMap;

use wish_core::{Entity, Result};
use wish_ecs::World;
pub use wish_types::anim::{AnimationFinished, AnimationPlayer};
use wish_types::render::Model;
pub use wish_types::sprite::SpriteAnimator;
use wish_types::time::Time;

/// The name of the animation system in an app.
pub const SYSTEM: &str = "animation";

/// Registers the animation components and event.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<AnimationPlayer>()?;
    world.register_component::<SpriteAnimator>()?;
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

/// What the simulation knows of one clip of a sprite sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteClip {
    /// How many frames the clip lists.
    pub frames: u32,
    /// How many it shows a second.
    pub fps: f32,
    /// Whether it starts over at its end.
    pub looping: bool,
}

impl SpriteClip {
    /// How long one round of the clip lasts, in seconds (0 for a clip that
    /// cannot play: no frames, or no speed).
    pub fn seconds(&self) -> f32 {
        if self.frames == 0 || !(self.fps.is_finite() && self.fps > 0.0) {
            0.0
        } else {
            self.frames as f32 / self.fps
        }
    }

    /// Which of the clip's frames shows `time` seconds into it.
    pub fn frame_at(&self, time: f32) -> u32 {
        if self.frames == 0 || !(self.fps.is_finite() && self.fps > 0.0) || !time.is_finite() {
            return 0;
        }
        // Whole frames gone by; a float far beyond any clip saturates.
        let gone = (time * self.fps).floor() as i64;
        let count = i64::from(self.frames);
        let frame = if self.looping {
            gone.rem_euclid(count)
        } else {
            gone.clamp(0, count - 1)
        };
        u32::try_from(frame).unwrap_or(0)
    }
}

/// The clips of the project's sprite sheets: `(sheet path, clip name)` →
/// what the simulation needs of it.
#[derive(Clone, Debug, Default)]
pub struct SpriteClips {
    clips: BTreeMap<(String, String), SpriteClip>,
}

impl SpriteClips {
    /// The clip `clip` of the sheet `sheet`, if known.
    pub fn get(&self, sheet: &str, clip: &str) -> Option<SpriteClip> {
        self.clips
            .get(&(sheet.to_owned(), clip.to_owned()))
            .copied()
    }
}

/// Tells the world what the clips of its sprite sheets are.
pub fn install_sprite_clips(
    world: &mut World,
    clips: impl IntoIterator<Item = ((String, String), SpriteClip)>,
) {
    let mut known = world.take_extension::<SpriteClips>().unwrap_or_default();
    known.clips.extend(clips);
    world.insert_extension(known);
}

/// One animation tick: models' clip players, then sprite animators.
pub fn step(world: &mut World) -> Result<()> {
    step_players(world)?;
    step_sprites(world)
}

/// One tick of every sprite animator that plays a clip, in entity order.
/// An animator without a clip, or whose sheet or clip is not known, is left
/// as it is (the picture says why it shows what it shows).
pub fn step_sprites(world: &mut World) -> Result<()> {
    let Some(clips) = world.take_extension::<SpriteClips>() else {
        return Ok(());
    };
    let result = step_sprites_with(world, &clips);
    world.insert_extension(clips);
    result
}

fn step_sprites_with(world: &mut World, clips: &SpriteClips) -> Result<()> {
    let delta = world.resource::<Time>().map_or(1.0 / 60.0, |t| t.delta);
    let mut animators: Vec<Entity> = world
        .query::<(Entity, &SpriteAnimator)>()?
        .map(|(entity, _)| entity)
        .collect();
    animators.sort_unstable();
    for entity in animators {
        let Some(animator) = world.get::<SpriteAnimator>(entity).copied() else {
            continue;
        };
        if animator.clip.is_empty() {
            continue;
        }
        let sheet = world.text(animator.sheet.text()).unwrap_or_default();
        let sheet = sheet.split('#').next().unwrap_or(sheet);
        let name = world.text(animator.clip).unwrap_or_default();
        let Some(clip) = clips.get(sheet, name) else {
            continue;
        };
        // Game code can write anything: what is not a number plays as the
        // default, and the speed keeps to its range.
        let speed = if animator.speed.is_finite() {
            animator.speed.clamp(-10.0, 10.0)
        } else {
            1.0
        };
        let mut time = if animator.time.is_finite() {
            animator.time
        } else {
            0.0
        };
        let mut next = animator;
        let length = clip.seconds();
        let mut finished = false;
        if animator.playing {
            time += delta * speed;
            if length > 0.0 {
                if clip.looping {
                    time = time.rem_euclid(length);
                } else if time >= length || time <= 0.0 && speed < 0.0 {
                    time = time.clamp(0.0, length);
                    next.playing = false;
                    finished = true;
                }
            }
        }
        next.time = time;
        next.frame = clip.frame_at(time);
        if next != animator {
            world.insert(entity, next)?;
        }
        if finished {
            world.send(AnimationFinished { entity })?;
        }
    }
    Ok(())
}

/// One tick of every model's clip player.
pub fn step_players(world: &mut World) -> Result<()> {
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
        // Clip lengths are known per model file: a `#sub` part of the
        // reference names something inside it.
        let model = world
            .get::<Model>(entity)
            .and_then(|m| world.text(m.asset.text()))
            .unwrap_or_default();
        let model = model.split('#').next().unwrap_or(model).to_owned();
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
