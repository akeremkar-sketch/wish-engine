//! Playing sounds for a person (`wen play`): [`Speaker`] follows the sounds
//! the simulation says are playing ([`crate::voices`]) and plays them
//! through the sound card with Kira. It reads the world and never writes
//! it, so listening cannot change what happens. If there is no sound
//! device it says so once and goes quiet.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, Frame, Panning, PlaybackRate, Tween,
};
use wish_core::{Diagnostic, Entity};

use crate::{Voice, clip};

/// A short fade, so that sounds do not click when they start or stop.
const FADE: Duration = Duration::from_millis(15);

fn tween() -> Tween {
    Tween {
        duration: FADE,
        ..Tween::default()
    }
}

/// A decoded clip: its sample rate and frames.
type Decoded = (u32, Arc<[Frame]>);

/// The sound card, and what is playing on it.
pub struct Speaker {
    manager: AudioManager<DefaultBackend>,
    clips: BTreeMap<String, Option<Decoded>>,
    playing: BTreeMap<Entity, (String, StaticSoundHandle)>,
}

impl Speaker {
    /// Opens the default sound device.
    pub fn open() -> Result<Self, Diagnostic> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|error| {
                Diagnostic::warning(
                    "audio_device",
                    format!("No sound device could be opened: {error}."),
                    "The game runs without sound. Check that a speaker or headphones are connected and not in use by an exclusive-mode program.",
                )
            })?;
        Ok(Self {
            manager,
            clips: BTreeMap::new(),
            playing: BTreeMap::new(),
        })
    }

    /// The decoded clip at `path`, read through `read` once and kept.
    fn clip(&mut self, path: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Decoded> {
        self.clips
            .entry(path.to_owned())
            .or_insert_with(|| {
                let bytes = read(path)?;
                let decoded = clip::decode(path, &bytes).ok()?;
                let frames: Vec<Frame> = if decoded.channels == 1 {
                    decoded
                        .samples
                        .iter()
                        .map(|s| Frame::from_mono(*s))
                        .collect()
                } else {
                    decoded
                        .samples
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| Frame {
                            left: pair[0],
                            right: pair[1],
                        })
                        .collect()
                };
                Some((decoded.rate, Arc::from(frames)))
            })
            .clone()
    }

    /// Makes what is heard match `voices`: starts the new ones, adjusts the
    /// ones going on, stops the ones that ended. `read` gives a clip file's
    /// bytes by project path.
    pub fn sync(&mut self, voices: &[Voice], read: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let ended: Vec<Entity> = self
            .playing
            .keys()
            .filter(|entity| {
                !voices
                    .iter()
                    .any(|v| v.entity == **entity && self.playing[*entity].0 == v.clip)
            })
            .copied()
            .collect();
        for entity in ended {
            if let Some((_, mut handle)) = self.playing.remove(&entity) {
                handle.stop(tween());
            }
        }
        for voice in voices {
            if let Some((_, handle)) = self.playing.get_mut(&voice.entity) {
                handle.set_volume(kira::Decibels(amplitude_to_db(voice.gain)), tween());
                handle.set_panning(Panning(voice.pan), tween());
                continue;
            }
            let Some((rate, frames)) = self.clip(&voice.clip, read) else {
                continue;
            };
            let mut data = StaticSoundData {
                sample_rate: rate,
                frames,
                settings: kira::sound::static_sound::StaticSoundSettings::default(),
                slice: None,
            }
            .volume(kira::Decibels(amplitude_to_db(voice.gain)))
            .panning(Panning(voice.pan))
            .playback_rate(PlaybackRate(f64::from(voice.pitch)))
            .start_position(f64::from(voice.position));
            if voice.looping {
                data = data.loop_region(..);
            }
            if let Ok(handle) = self.manager.play(data) {
                self.playing
                    .insert(voice.entity, (voice.clip.clone(), handle));
            }
        }
    }
}

fn amplitude_to_db(gain: f32) -> f32 {
    if gain <= 1e-5 {
        -80.0
    } else {
        20.0 * gain.log10()
    }
}
