//! Skeletal animation (design note M6 §6): which clip of an entity's glTF
//! `Model` plays, and where in it the entity is. The clip's time lives
//! here, in the world, so snapshots and replays include it; the pose is
//! worked out from it when drawing.

use wish_core::{Entity, Text};
use wish_schema::{component, event};

/// Plays an animation clip of the entity's `Model`.
#[component]
pub struct AnimationPlayer {
    /// The clip's name in the model file; empty plays the first clip.
    pub clip: Text,
    /// Seconds into the clip. The engine advances it every tick while
    /// `playing`; write it to jump.
    #[schema(unit = "s")]
    pub time: f32,
    /// Playback speed: 2 is twice as fast, -1 plays backwards.
    #[schema(range = -10.0..=10.0, default = 1.0)]
    pub speed: f32,
    /// Start over at the end, for ever.
    #[schema(default = true)]
    pub looping: bool,
    /// Whether time moves. A clip that ends (not looping) sets it to false.
    #[schema(default = true)]
    pub playing: bool,
}

/// A clip that does not loop reached its end.
#[event]
pub struct AnimationFinished {
    /// The entity whose clip ended.
    pub entity: Entity,
}
