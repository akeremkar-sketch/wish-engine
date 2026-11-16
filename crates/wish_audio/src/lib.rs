//! Sound for Wish Engine (ADR-015, design note M6 §4).
//!
//! The simulation side is deterministic and lives in the world: sound
//! sources, their playback state (position in the clip, gain and pan at the
//! listener) and `SoundStarted` / `SoundEnded` events. [`step`] runs once a
//! tick. What an agent "hears" is the **audio log**: a line for every sound
//! that starts or ends, drained by the control plane after a run
//! ([`drain_log`]). Playing the sound for a person (`wen play`) is outside
//! the simulation and cannot change it.
//!
//! Clip lengths come from the files, which the control plane reads when a
//! session starts ([`Library`]).

use std::collections::BTreeMap;

use wish_core::math::{Mat4, Vec3};
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
pub use wish_types::audio::{
    AudioBus, AudioListener, AudioOneShot, AudioPlayback, AudioSettings, AudioSource, PlaySound,
    SoundEnded, SoundStarted,
};
use wish_types::time::Time;
use wish_types::transform::{GlobalTransform, Transform};

pub mod clip;
#[cfg(feature = "output")]
pub mod output;

pub use clip::{Clip, decode, sine_wav};

/// The name of the audio system in an app.
pub const SYSTEM: &str = "audio";

/// Registers the audio components, resources and events.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<AudioSource>()?;
    world.register_component::<AudioListener>()?;
    world.register_component::<AudioPlayback>()?;
    world.register_component::<AudioOneShot>()?;
    world.register_resource::<AudioSettings>()?;
    world.register_event::<PlaySound>()?;
    world.register_event::<SoundStarted>()?;
    world.register_event::<SoundEnded>()?;
    Ok(())
}

/// The audio system, for `App::add_system`: after the systems that write
/// sources, before transform propagation.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, context| step(world, context.last_run))
}

/// What happened to a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    /// It started playing.
    Started,
    /// It ended: the clip finished, it was switched off, or its entity went.
    Ended,
    /// Its clip is not known (no such file, or it cannot be read).
    Unknown,
}

/// One line of the audio log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    /// The tick it happened in.
    pub tick: u64,
    /// What happened.
    pub kind: LogKind,
    /// The entity that makes the sound.
    pub source: Entity,
    /// The clip's path.
    pub clip: String,
    /// How loud it is at the listener (linear), for `Started`.
    pub gain: f32,
    /// Left (-1) to right (1), for `Started`.
    pub pan: f32,
}

/// The lengths of the clips a session can play, by project path, and the
/// log waiting to be read. Working state (ADR-026): snapshots carry it, the
/// world hash leaves it out.
#[derive(Clone, Debug, Default)]
pub struct AudioState {
    seconds: BTreeMap<String, f32>,
    /// Sounds playing now: source entity → clip path.
    voices: BTreeMap<Entity, String>,
    log: Vec<LogLine>,
}

impl AudioState {
    /// Tells the state how long a clip is.
    pub fn set_length(&mut self, clip: &str, seconds: f32) {
        self.seconds.insert(clip.to_owned(), seconds);
    }
}

/// Gives the world its audio state (clip lengths) before a session runs.
pub fn install(world: &mut World, clips: impl IntoIterator<Item = (String, f32)>) {
    let mut state = world.extension::<AudioState>().cloned().unwrap_or_default();
    for (clip, seconds) in clips {
        state.set_length(&clip, seconds);
    }
    world.insert_extension(state);
}

/// Takes the audio log lines written since the last call.
pub fn drain_log(world: &mut World) -> Vec<LogLine> {
    world
        .extension_mut::<AudioState>()
        .map(|state| std::mem::take(&mut state.log))
        .unwrap_or_default()
}

/// A sound that is playing now, as a speaker needs to know it.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    /// The entity that makes it.
    pub entity: Entity,
    /// The clip's path.
    pub clip: String,
    /// How loud (linear).
    pub gain: f32,
    /// Left (-1) to right (1).
    pub pan: f32,
    /// Playback speed.
    pub pitch: f32,
    /// Whether it repeats.
    pub looping: bool,
    /// Where in the clip it is, in seconds.
    pub position: f32,
}

/// Every sound playing in `world`, in entity order.
pub fn voices(world: &World) -> Vec<Voice> {
    let mut found: Vec<Voice> = world
        .entities()
        .filter_map(|entity| {
            let source = world.get::<AudioSource>(entity)?;
            let playback = world.get::<AudioPlayback>(entity)?;
            (source.playing && playback.started).then(|| Voice {
                entity,
                clip: world
                    .text(source.clip.text())
                    .unwrap_or_default()
                    .to_owned(),
                gain: playback.gain,
                pan: playback.pan,
                pitch: source.pitch,
                looping: source.looping,
                position: playback.position,
            })
        })
        .collect();
    found.sort_by_key(|voice| voice.entity);
    found
}

fn matrix_of(world: &World, entity: Entity) -> Mat4 {
    world
        .get::<GlobalTransform>(entity)
        .map(|g| g.matrix)
        .or_else(|| world.get::<Transform>(entity).map(Transform::to_matrix))
        .unwrap_or(Mat4::IDENTITY)
}

/// How loud a source is at the listener (linear) and where: `(gain, pan)`.
fn mix(
    source: &AudioSource,
    position: Vec3,
    listener: Option<(Vec3, Vec3, f32)>,
    settings: &AudioSettings,
) -> (f32, f32) {
    let bus = match source.bus {
        AudioBus::Sfx => settings.sfx,
        AudioBus::Music => settings.music,
    };
    let (listening, volume) = match listener {
        Some((_, _, volume)) => (true, volume),
        None => (false, 1.0),
    };
    let mut gain =
        source.volume.max(0.0) * bus.max(0.0) * settings.master.max(0.0) * volume.max(0.0);
    let mut pan = 0.0;
    if source.spatial
        && listening
        && let Some((at, right, _)) = listener
    {
        let to = position - at;
        let distance = to.length();
        let min = source.min_distance.max(0.01);
        let max = source.max_distance.max(min);
        if distance >= max {
            gain = 0.0;
        } else if distance > min {
            gain *= min / distance;
        }
        if distance > 1e-4 {
            pan = (to / distance).dot(right).clamp(-1.0, 1.0);
        }
    }
    if !gain.is_finite() || !pan.is_finite() {
        return (0.0, 0.0);
    }
    (gain, pan)
}

/// One audio tick for `world`: the system [`SYSTEM`].
pub fn step(world: &mut World, since: ChangeTick) -> Result<()> {
    let mut state = world.extension::<AudioState>().cloned().unwrap_or_default();
    world.remove_extension::<AudioState>();
    let result = run(world, &mut state, since);
    world.insert_extension(state);
    result
}

fn run(world: &mut World, state: &mut AudioState, since: ChangeTick) -> Result<()> {
    let settings = world
        .resource::<AudioSettings>()
        .copied()
        .unwrap_or_default();
    let tick = world.resource::<Time>().map_or(0, |t| t.tick);
    let delta = world.resource::<Time>().map_or(1.0 / 60.0, |t| t.delta);

    // One-off sounds become short-lived entities.
    for play in world.events_since::<PlaySound>(since)? {
        let entity = world.spawn()?;
        world.insert(entity, Transform::from_translation(play.position))?;
        world.insert(
            entity,
            AudioSource {
                clip: play.clip,
                bus: play.bus,
                volume: play.volume,
                pitch: play.pitch,
                looping: false,
                spatial: play.spatial,
                min_distance: 1.0,
                max_distance: 50.0,
                playing: true,
            },
        )?;
        world.insert(entity, AudioOneShot::default())?;
    }

    // The listener: the first entity that has the component.
    let mut listeners: Vec<Entity> = world
        .query::<(Entity, &AudioListener)>()?
        .map(|(entity, _)| entity)
        .collect();
    listeners.sort_unstable();
    let listener = listeners.first().map(|entity| {
        let matrix = matrix_of(world, *entity);
        let right = matrix.x_axis.truncate().normalize_or_zero();
        let volume = world
            .get::<AudioListener>(*entity)
            .map_or(1.0, |l| l.volume);
        (matrix.w_axis.truncate(), right, volume)
    });

    let mut sources: Vec<Entity> = world
        .query::<(Entity, &AudioSource)>()?
        .map(|(entity, _)| entity)
        .collect();
    sources.sort_unstable();

    // Sounds whose entity went away.
    let gone: Vec<Entity> = state
        .voices
        .keys()
        .filter(|entity| sources.binary_search(entity).is_err())
        .copied()
        .collect();
    for entity in gone {
        if let Some(clip) = state.voices.remove(&entity) {
            state.log.push(LogLine {
                tick,
                kind: LogKind::Ended,
                source: entity,
                clip: clip.clone(),
                gain: 0.0,
                pan: 0.0,
            });
            let clip_ref = clip_ref(world, &clip)?;
            world.send(SoundEnded {
                source: entity,
                clip: clip_ref,
            })?;
        }
    }

    for entity in sources {
        let Some(source) = world.get::<AudioSource>(entity).copied() else {
            continue;
        };
        let mut playback = world
            .get::<AudioPlayback>(entity)
            .copied()
            .unwrap_or_default();
        let path = world
            .text(source.clip.text())
            .unwrap_or_default()
            .to_owned();
        let position = matrix_of(world, entity).w_axis.truncate();
        let (gain, pan) = mix(&source, position, listener, &settings);
        let mut next = source;
        let mut end = false;

        if source.playing && !playback.started {
            match state.seconds.get(&path).copied() {
                Some(_) => {
                    playback = AudioPlayback {
                        position: 0.0,
                        gain,
                        pan,
                        started: true,
                    };
                    state.voices.insert(entity, path.clone());
                    state.log.push(LogLine {
                        tick,
                        kind: LogKind::Started,
                        source: entity,
                        clip: path.clone(),
                        gain,
                        pan,
                    });
                    world.send(SoundStarted {
                        source: entity,
                        clip: source.clip,
                        gain,
                        pan,
                    })?;
                }
                None => {
                    state.log.push(LogLine {
                        tick,
                        kind: LogKind::Unknown,
                        source: entity,
                        clip: path.clone(),
                        gain: 0.0,
                        pan: 0.0,
                    });
                    next.playing = false;
                    end = world.has::<AudioOneShot>(entity);
                }
            }
        } else if source.playing && playback.started {
            let length = state.seconds.get(&path).copied().unwrap_or(0.0);
            playback.position += delta * source.pitch.clamp(0.1, 4.0);
            playback.gain = gain;
            playback.pan = pan;
            if length > 0.0 && playback.position >= length {
                if source.looping {
                    playback.position %= length;
                } else {
                    next.playing = false;
                }
            }
        }
        if !next.playing && playback.started {
            playback = AudioPlayback::default();
            if state.voices.remove(&entity).is_some() {
                state.log.push(LogLine {
                    tick,
                    kind: LogKind::Ended,
                    source: entity,
                    clip: path.clone(),
                    gain: 0.0,
                    pan: 0.0,
                });
                world.send(SoundEnded {
                    source: entity,
                    clip: source.clip,
                })?;
            }
            end = world.has::<AudioOneShot>(entity);
        }
        if next != source {
            world.insert(entity, next)?;
        }
        if world.get::<AudioPlayback>(entity).copied() != Some(playback) {
            world.insert(entity, playback)?;
        }
        if end {
            world.despawn(entity)?;
        }
    }
    Ok(())
}

/// The asset reference for `path`, interned in the world.
fn clip_ref(world: &mut World, path: &str) -> Result<wish_core::AssetRef> {
    Ok(wish_core::AssetRef::from_text(world.intern(path)?))
}

/// A diagnostic for an audio problem the control plane reports.
pub fn unknown_clip(line: &LogLine) -> Diagnostic {
    Diagnostic::warning(
        "audio_clip_unknown",
        format!(
            "Sound source #{} wants the clip '{}', which is not a readable .wav or .ogg file of the project.",
            line.source, line.clip
        ),
        "Put the file under the project folder (assets/sounds/...), check the path's spelling, and start a new session so the engine reads it.",
    )
}
