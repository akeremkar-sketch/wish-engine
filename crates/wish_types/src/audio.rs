//! Sound (ADR-015, design note M6 §4): what a scene or game code writes to
//! make things heard, and what the engine writes back.
//!
//! An agent cannot hear, so the sound is *described*: every sound that
//! starts or ends is an event with the clip, the entity, how loud it is at
//! the listener (`gain`, linear: 1 is the clip as recorded) and where it
//! is (`pan`: -1 left, 0 centre, 1 right), and the session log lists them
//! (`logs_get` kind `audio`). Actually playing the sound (`wen play`) is
//! not part of the simulation and never changes it.
//!
//! Units: meters, seconds; `volume` and `gain` are linear amplitudes.

use wish_core::math::Vec3;
use wish_core::{AssetRef, Entity};
use wish_schema::{component, event, resource, schema};

/// Which volume control a sound belongs to.
#[schema]
pub enum AudioBus {
    /// Effects: footsteps, hits, doors.
    Sfx,
    /// Music.
    Music,
}

/// Something that makes a sound: plays a clip while `playing` is true. For
/// a sound at a place, put it on an entity with a `Transform`; for a
/// one-off sound send a [`PlaySound`] event instead.
#[component]
pub struct AudioSource {
    /// The sound file (`.wav` or `.ogg`), a path in the project.
    pub clip: AssetRef,
    /// Which volume control it follows.
    pub bus: AudioBus,
    /// How loud, 0 (silent) to 1 (as recorded); more is allowed.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub volume: f32,
    /// Playback speed: 2 is an octave higher and twice as fast.
    #[schema(range = 0.1..=4.0, default = 1.0)]
    pub pitch: f32,
    /// Start over when the clip ends, for ever.
    #[schema(alias = "loop")]
    pub looping: bool,
    /// Whether it gets quieter with distance from the listener and pans
    /// left or right. Off: plays the same everywhere (music, menus).
    pub spatial: bool,
    /// Up to this distance it is at full volume.
    #[schema(unit = "m", range = 0.01..=10000.0, default = 1.0)]
    pub min_distance: f32,
    /// From this distance on it is silent; in between it fades as
    /// `min_distance / distance`.
    #[schema(unit = "m", range = 0.01..=10000.0, default = 50.0)]
    pub max_distance: f32,
    /// Plays while true. A clip that ends (not looping) sets it to false.
    pub playing: bool,
}

/// Where the player hears from: usually on the camera. The first entity
/// with this component (in entity order) is the listener.
#[component]
pub struct AudioListener {
    /// Extra volume for everything this listener hears.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub volume: f32,
}

/// What the engine knows of a sound it is playing, written every tick.
#[component]
pub struct AudioPlayback {
    /// Seconds played so far, in clip time.
    #[schema(unit = "s")]
    pub position: f32,
    /// How loud it is at the listener, as a linear amplitude (0 silent).
    pub gain: f32,
    /// Left (-1) to right (1) as the listener hears it.
    #[schema(range = -1.0..=1.0)]
    pub pan: f32,
    /// Whether the engine has started this sound (so that a new start is
    /// told from one in progress).
    pub started: bool,
}

/// Marks the short-lived entity a [`PlaySound`] made: the engine removes it
/// when its clip ends.
#[component]
pub struct AudioOneShot {
    /// Reserved; always false.
    pub reserved: bool,
}

/// Volume controls.
#[resource]
pub struct AudioSettings {
    /// Everything.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub master: f32,
    /// Effects.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub sfx: f32,
    /// Music.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub music: f32,
}

/// Plays a sound once. The engine makes a short-lived entity for it, so it
/// can sit at a place (`at`) and is removed when the clip ends.
#[event]
pub struct PlaySound {
    /// The sound file.
    pub clip: AssetRef,
    /// Which volume control it follows.
    pub bus: AudioBus,
    /// How loud, 0 to 1.
    #[schema(range = 0.0..=4.0, default = 1.0)]
    pub volume: f32,
    /// Playback speed.
    #[schema(range = 0.1..=4.0, default = 1.0)]
    pub pitch: f32,
    /// Whether it sits at `position` (fading with distance); off plays the
    /// same everywhere.
    pub spatial: bool,
    /// Where it is, in world space.
    #[schema(unit = "m")]
    pub position: Vec3,
}

/// A sound started playing.
#[event]
pub struct SoundStarted {
    /// The entity that makes it.
    pub source: Entity,
    /// The sound file.
    pub clip: AssetRef,
    /// How loud it is at the listener (linear).
    pub gain: f32,
    /// Left (-1) to right (1).
    pub pan: f32,
}

/// A sound stopped: its clip ended, it was switched off, or its entity is
/// gone.
#[event]
pub struct SoundEnded {
    /// The entity that made it.
    pub source: Entity,
    /// The sound file.
    pub clip: AssetRef,
}
