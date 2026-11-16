//! Playing sounds for a person (`wen play`): [`Speaker`] follows the sounds
//! the simulation says are playing ([`crate::voices`]) and mixes them onto
//! the sound card with `cpal`. It reads the world and never writes it, so
//! listening cannot change what happens. If there is no sound device it
//! says so once and goes quiet.
//!
//! The mixer is small on purpose: per voice a read position, a speed (the
//! clip's rate against the device's, times the pitch), a gain and an
//! equal-power pan, both eased towards their targets so that nothing clicks.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use wish_core::{Diagnostic, Entity};

use crate::{Clip, Voice, clip};

/// How much of the way to its target a gain or pan moves per output frame
/// (about 5 ms at 48 kHz).
const EASE: f32 = 1.0 / 256.0;

/// A voice being mixed.
struct Playing {
    clip: Arc<Clip>,
    /// Read position in clip frames.
    cursor: f64,
    /// Clip frames per output frame.
    step: f64,
    looping: bool,
    gain: f32,
    target_gain: f32,
    pan: f32,
    target_pan: f32,
    /// Fading out before it is removed.
    stopping: bool,
    finished: bool,
}

impl Playing {
    /// The clip's left and right sample at the cursor (linear interpolation).
    fn sample(&self) -> (f32, f32) {
        let channels = usize::from(self.clip.channels.max(1));
        let frames = self.clip.samples.len() / channels;
        if frames == 0 {
            return (0.0, 0.0);
        }
        let at = self.cursor.max(0.0);
        let first = (at.floor() as usize).min(frames - 1);
        let second = if first + 1 < frames {
            first + 1
        } else if self.looping {
            0
        } else {
            first
        };
        let t = (at - at.floor()) as f32;
        let read = |frame: usize, channel: usize| {
            self.clip.samples[frame * channels + channel.min(channels - 1)]
        };
        let left = read(first, 0) * (1.0 - t) + read(second, 0) * t;
        let right = read(first, 1) * (1.0 - t) + read(second, 1) * t;
        (left, right)
    }

    fn advance(&mut self) {
        let frames = (self.clip.samples.len() / usize::from(self.clip.channels.max(1))) as f64;
        self.cursor += self.step;
        if self.cursor >= frames {
            if self.looping && frames > 0.0 {
                self.cursor %= frames;
            } else {
                self.finished = true;
            }
        }
        self.gain += (self.target_gain - self.gain) * EASE;
        self.pan += (self.target_pan - self.pan) * EASE;
        if self.stopping && self.gain < 1e-4 {
            self.finished = true;
        }
    }
}

/// What the sound card's thread and the game share.
struct Mixer {
    rate: f64,
    voices: BTreeMap<Entity, Playing>,
}

impl Mixer {
    /// Mixes one output frame into `out` (one sample per channel).
    fn frame(&mut self, out: &mut [f32]) {
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for voice in self.voices.values_mut() {
            if voice.finished {
                continue;
            }
            let (l, r) = voice.sample();
            // Equal-power pan: -1 all left, 1 all right, 0 both at full.
            let angle = (voice.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
            left += l * voice.gain * angle.cos() * std::f32::consts::SQRT_2;
            right += r * voice.gain * angle.sin() * std::f32::consts::SQRT_2;
            voice.advance();
        }
        let (left, right) = (left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0));
        match out {
            [mono] => *mono = 0.5 * (left + right),
            [l, r, rest @ ..] => {
                *l = left;
                *r = right;
                rest.fill(0.0);
            }
            [] => {}
        }
    }
}

/// The sound card, and what is playing on it.
pub struct Speaker {
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    clips: BTreeMap<String, Option<Arc<Clip>>>,
}

fn no_device(why: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::warning(
        "audio_device",
        format!("No sound device could be opened: {why}."),
        "The game runs without sound. Check that a speaker or headphones are connected and not in use by an exclusive-mode program.",
    )
}

fn stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
) -> Result<cpal::Stream, Diagnostic> {
    let channels = usize::from(config.channels.max(1));
    let mut scratch = vec![0.0f32; channels];
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                // Never wait on the game: if the mixer is busy, this buffer
                // is silence.
                let Ok(mut mixer) = mixer.try_lock() else {
                    data.fill(T::from_sample(0.0));
                    return;
                };
                for frame in data.chunks_mut(channels) {
                    mixer.frame(&mut scratch);
                    for (out, value) in frame.iter_mut().zip(&scratch) {
                        *out = T::from_sample(*value);
                    }
                }
                mixer.voices.retain(|_, voice| !voice.finished);
            },
            |_| {},
            None,
        )
        .map_err(no_device)
}

impl Speaker {
    /// Opens the default sound device.
    pub fn open() -> Result<Self, Diagnostic> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| no_device("there is no output device"))?;
        let supported = device.default_output_config().map_err(no_device)?;
        let config: cpal::StreamConfig = supported.config();
        let mixer = Arc::new(Mutex::new(Mixer {
            rate: f64::from(config.sample_rate.max(1)),
            voices: BTreeMap::new(),
        }));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => stream::<f32>(&device, &config, Arc::clone(&mixer))?,
            SampleFormat::I16 => stream::<i16>(&device, &config, Arc::clone(&mixer))?,
            SampleFormat::U16 => stream::<u16>(&device, &config, Arc::clone(&mixer))?,
            other => {
                return Err(no_device(format!(
                    "its sample format {other:?} is not supported"
                )));
            }
        };
        stream.play().map_err(no_device)?;
        Ok(Self {
            _stream: stream,
            mixer,
            clips: BTreeMap::new(),
        })
    }

    /// The decoded clip at `path`, read through `read` once and kept.
    fn clip(&mut self, path: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Arc<Clip>> {
        self.clips
            .entry(path.to_owned())
            .or_insert_with(|| {
                let bytes = read(path)?;
                clip::decode(path, &bytes).ok().map(Arc::new)
            })
            .clone()
    }

    /// Makes what is heard match `voices`: starts the new ones, eases the
    /// ones going on to their new gain and pan, fades out the ones that
    /// ended. `read` gives a clip file's bytes by project path.
    pub fn sync(&mut self, voices: &[Voice], read: &dyn Fn(&str) -> Option<Vec<u8>>) {
        let mut wanted = Vec::with_capacity(voices.len());
        for voice in voices {
            if let Some(clip) = self.clip(&voice.clip, read) {
                wanted.push((voice, clip));
            }
        }
        let Ok(mut mixer) = self.mixer.lock() else {
            return;
        };
        let rate = mixer.rate;
        for (entity, playing) in &mut mixer.voices {
            let still = wanted
                .iter()
                .any(|(voice, clip)| voice.entity == *entity && Arc::ptr_eq(clip, &playing.clip));
            if !still {
                playing.stopping = true;
                playing.target_gain = 0.0;
            }
        }
        for (voice, clip) in wanted {
            let gain = if voice.gain.is_finite() {
                voice.gain.max(0.0)
            } else {
                0.0
            };
            let pan = if voice.pan.is_finite() {
                voice.pan.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            match mixer.voices.get_mut(&voice.entity) {
                Some(playing) if !playing.stopping && Arc::ptr_eq(&playing.clip, &clip) => {
                    playing.target_gain = gain;
                    playing.target_pan = pan;
                }
                _ => {
                    let step = f64::from(clip.rate) / rate * f64::from(voice.pitch.clamp(0.1, 4.0));
                    let cursor = f64::from(voice.position.max(0.0)) * f64::from(clip.rate);
                    mixer.voices.insert(
                        voice.entity,
                        Playing {
                            clip,
                            cursor,
                            step,
                            looping: voice.looping,
                            gain: 0.0,
                            target_gain: gain,
                            pan,
                            target_pan: pan,
                            stopping: false,
                            finished: false,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn tone() -> Arc<Clip> {
        Arc::new(clip::decode("t.wav", &crate::sine_wav(1000.0, 0.01, 8000)).unwrap())
    }

    #[test]
    fn the_mixer_pans_finishes_and_goes_silent() {
        let mut mixer = Mixer {
            rate: 8000.0,
            voices: BTreeMap::new(),
        };
        mixer.voices.insert(
            Entity::from_bits(1),
            Playing {
                clip: tone(),
                cursor: 0.0,
                step: 1.0,
                looping: false,
                gain: 1.0,
                target_gain: 1.0,
                pan: 1.0,
                target_pan: 1.0,
                stopping: false,
                finished: false,
            },
        );
        let mut out = [0.0f32; 2];
        let (mut left, mut right) = (0.0, 0.0);
        for _ in 0..80 {
            mixer.frame(&mut out);
            left += out[0].abs();
            right += out[1].abs();
        }
        assert!(left < 1e-3 && right > 1.0, "{left} {right}");
        // An 80-frame clip played for 80 frames: finished, silence after.
        assert!(mixer.voices.values().all(|v| v.finished));
        mixer.frame(&mut out);
        assert_eq!(out, [0.0, 0.0]);
    }
}
