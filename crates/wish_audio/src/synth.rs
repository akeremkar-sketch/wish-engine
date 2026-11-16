//! Placeholder sounds made from a few numbers (M7 friction log, F5): a game
//! being built by an agent needs a jump, a pickup and some music before
//! anyone records real ones, and nothing may be downloaded. Deterministic:
//! the same numbers give the same bytes.

use std::io::Cursor;

use wish_core::Rng;

/// The shape of a sound's wave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    /// Smooth (a whistle, a soft beep).
    Sine,
    /// Hollow, retro (a chip-tune blip).
    Square,
    /// Between the two (a soft synth).
    Triangle,
    /// Bright, buzzy.
    Saw,
    /// No pitch (wind, an explosion, footsteps).
    Noise,
}

impl Wave {
    /// The names the tools use.
    pub const NAMES: &'static [&'static str] = &["sine", "square", "triangle", "saw", "noise"];

    /// The wave named `name`.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "sine" => Self::Sine,
            "square" => Self::Square,
            "triangle" => Self::Triangle,
            "saw" => Self::Saw,
            "noise" => Self::Noise,
            _ => return None,
        })
    }
}

/// A sound to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Synth {
    /// The wave.
    pub wave: Wave,
    /// The pitch at the start, in hertz.
    pub frequency: f32,
    /// The pitch at the end (a slide: up for a jump, down for a fall).
    pub end_frequency: f32,
    /// The length in seconds.
    pub seconds: f32,
    /// 0 to 1.
    pub volume: f32,
    /// Seconds to fade in.
    pub attack: f32,
    /// Seconds to fade out at the end.
    pub release: f32,
    /// Seeds the noise.
    pub seed: u64,
}

/// The sample rate of made sounds.
pub const SYNTH_RATE: u32 = 22_050;
/// The longest sound made, in seconds.
pub const MAX_SYNTH_SECONDS: f32 = 60.0;

/// The sound as a mono 16-bit WAV file.
pub fn synth_wav(synth: &Synth) -> Vec<u8> {
    let clean = |v: f32, low: f32, high: f32, otherwise: f32| {
        if v.is_finite() {
            v.clamp(low, high)
        } else {
            otherwise
        }
    };
    let seconds = clean(synth.seconds, 0.001, MAX_SYNTH_SECONDS, 0.2);
    let start = clean(synth.frequency, 1.0, 20_000.0, 440.0);
    let end = clean(synth.end_frequency, 1.0, 20_000.0, start);
    let volume = clean(synth.volume, 0.0, 1.0, 0.5);
    let attack = clean(synth.attack, 0.0, seconds, 0.0);
    let release = clean(synth.release, 0.0, seconds, 0.0);
    let mut rng = Rng::from_seed(synth.seed);
    let mut out = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SYNTH_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut writer) = hound::WavWriter::new(&mut out, spec) {
        let frames = (seconds * SYNTH_RATE as f32) as usize;
        let mut phase = 0.0f32;
        for n in 0..frames {
            let t = n as f32 / SYNTH_RATE as f32;
            let along = t / seconds;
            // The pitch slides exponentially (as the ear hears a slide).
            let frequency = start * (end / start).powf(along);
            phase = (phase + frequency / SYNTH_RATE as f32).fract();
            let value = match synth.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Noise => rng.range_f32(-1.0, 1.0),
            };
            let fade_in = if attack > 0.0 {
                (t / attack).min(1.0)
            } else {
                1.0
            };
            let fade_out = if release > 0.0 {
                ((seconds - t) / release).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let sample = value * volume * fade_in * fade_out;
            let _ = writer.write_sample((sample.clamp(-1.0, 1.0) * 32_000.0) as i16);
        }
        let _ = writer.finalize();
    }
    out.into_inner()
}
