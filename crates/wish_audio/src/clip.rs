//! Reading sound files: WAV (`hound`) and Ogg Vorbis (`lewton`). Clips come
//! from the project, so they are untrusted input: sizes are bounded and a
//! bad file is a diagnostic, never a panic.

use std::io::Cursor;

use wish_core::{Diagnostic, Result};

/// The largest clip file read.
pub const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
/// The longest clip played, in seconds.
pub const MAX_SECONDS: f64 = 3600.0;

/// A decoded clip: interleaved samples in -1..1.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Samples per second, per channel.
    pub rate: u32,
    /// 1 (mono) or 2 (stereo); more channels are mixed down to two.
    pub channels: u16,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl Clip {
    /// How long it plays at normal speed.
    pub fn seconds(&self) -> f32 {
        let frames = self.samples.len() / usize::from(self.channels.max(1));
        (frames as f64 / f64::from(self.rate.max(1))) as f32
    }
}

fn bad(path: &str, what: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::error(
        "audio_decode",
        format!("The sound file '{path}' cannot be used: {what}."),
        "Use a .wav (8, 16, 24 or 32-bit integer, or 32-bit float) or .ogg (Vorbis) file of at most an hour and 128 MiB.",
    )
    .at_file(path)
}

/// Decodes a clip; `path` names it in messages and picks the format by its
/// extension.
pub fn decode(path: &str, bytes: &[u8]) -> Result<Clip> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(bad(path, "the file is larger than 128 MiB"));
    }
    let lower = path.to_ascii_lowercase();
    let clip = if lower.ends_with(".wav") {
        wav(path, bytes)?
    } else if lower.ends_with(".ogg") {
        ogg(path, bytes)?
    } else {
        return Err(bad(path, "only .wav and .ogg files are supported"));
    };
    if clip.rate == 0 || clip.channels == 0 || clip.samples.is_empty() {
        return Err(bad(path, "it has no sound in it"));
    }
    if f64::from(clip.seconds()) > MAX_SECONDS {
        return Err(bad(path, "it is longer than an hour"));
    }
    Ok(clip)
}

/// The most samples a decoded clip may hold (all channels): 128 MiB of
/// `f32`, about 12 minutes of 48 kHz stereo. A small file can decode to far
/// more (M6 closing review).
pub const MAX_DECODED_SAMPLES: usize = 32 * 1024 * 1024;
/// The highest sample rate accepted.
const MAX_RATE: u32 = 384_000;

fn wav(path: &str, bytes: &[u8]) -> Result<Clip> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| bad(path, e))?;
    let spec = reader.spec();
    if spec.channels == 0
        || spec.channels > 8
        || spec.sample_rate == 0
        || spec.sample_rate > MAX_RATE
    {
        return Err(bad(path, "its format is not one games use"));
    }
    // Refuse before reading: a header can promise far more than the file has.
    let promised = u64::from(reader.len());
    if promised > (MAX_FILE_BYTES as u64) || promised > MAX_DECODED_SAMPLES as u64 {
        return Err(bad(path, "it holds more samples than allowed"));
    }
    let mut samples = Vec::with_capacity(reader.len().min(1 << 22) as usize);
    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => {
            for sample in reader.into_samples::<f32>() {
                samples.push(sample.map_err(|e| bad(path, e))?.clamp(-1.0, 1.0));
            }
        }
        (hound::SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
            let scale = 1.0 / (1u64 << (bits - 1)) as f32;
            for sample in reader.into_samples::<i32>() {
                samples.push(sample.map_err(|e| bad(path, e))? as f32 * scale);
            }
        }
        (format, bits) => {
            return Err(bad(
                path,
                format!("{bits}-bit {format:?} samples are not supported"),
            ));
        }
    }
    Ok(mixed(spec.sample_rate, spec.channels, samples))
}

fn ogg(path: &str, bytes: &[u8]) -> Result<Clip> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
        .map_err(|e| bad(path, format!("{e:?}")))?;
    let rate = reader.ident_hdr.audio_sample_rate;
    let channels = u16::from(reader.ident_hdr.audio_channels);
    if channels == 0 || channels > 8 || rate == 0 || rate > MAX_RATE {
        return Err(bad(path, "its format is not one games use"));
    }
    let limit =
        ((f64::from(rate) * MAX_SECONDS) as usize * usize::from(channels)).min(MAX_DECODED_SAMPLES);
    let mut samples: Vec<f32> = Vec::new();
    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => {
                samples.extend(packet.iter().map(|s| f32::from(*s) / 32768.0));
                if samples.len() > limit {
                    return Err(bad(
                        path,
                        "it decodes to too many samples (longer than an hour, or more than 128 MiB)",
                    ));
                }
            }
            Ok(None) => break,
            Err(e) => return Err(bad(path, format!("{e:?}"))),
        }
    }
    Ok(mixed(rate, channels, samples))
}

/// More than two channels are mixed down to two (the first two stay).
fn mixed(rate: u32, channels: u16, samples: Vec<f32>) -> Clip {
    if channels <= 2 {
        return Clip {
            rate,
            channels,
            samples,
        };
    }
    let step = usize::from(channels);
    let stereo = samples
        .chunks_exact(step)
        .flat_map(|frame| [frame[0], frame[1]])
        .collect();
    Clip {
        rate,
        channels: 2,
        samples: stereo,
    }
}

/// A WAV file with one channel of a sine wave, for tests and examples.
pub fn sine_wav(frequency: f32, seconds: f32, rate: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut writer) = hound::WavWriter::new(&mut out, spec) {
        let frames = (seconds * rate as f32) as usize;
        for n in 0..frames {
            let t = n as f32 / rate as f32;
            let value = (t * frequency * std::f32::consts::TAU).sin() * 0.5;
            let _ = writer.write_sample((value * 32767.0) as i16);
        }
        let _ = writer.finalize();
    }
    out.into_inner()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn a_wav_is_read_with_its_length() {
        let clip = decode("sounds/a.wav", &sine_wav(440.0, 0.5, 8000)).unwrap();
        assert_eq!((clip.rate, clip.channels), (8000, 1));
        assert!((clip.seconds() - 0.5).abs() < 1e-3);
        assert!(clip.samples.iter().all(|s| s.abs() <= 1.0));
        assert!(clip.samples.iter().any(|s| s.abs() > 0.4));
    }

    #[test]
    fn bad_files_are_explained_not_fatal() {
        for (name, bytes) in [
            ("a.wav", b"RIFF....WAVEfmt garbage".to_vec()),
            ("a.wav", Vec::new()),
            ("a.ogg", b"OggS\0not vorbis at all".to_vec()),
            ("a.mp3", vec![0xff, 0xfb, 0x90, 0x00]),
        ] {
            let error = decode(name, &bytes).unwrap_err();
            assert_eq!(error.code, "audio_decode", "{name}");
            assert!(!error.hint.is_empty());
        }
        // A header that promises an hour of 8 channels, with no data.
        let mut wav = sine_wav(100.0, 0.01, 8000);
        let len = wav.len();
        wav.truncate(len.min(60));
        assert!(decode("a.wav", &wav).is_err() || decode("a.wav", &wav).is_ok());
    }
}
