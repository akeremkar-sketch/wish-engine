//! The speaker (needs the `output` feature and a sound device):
//! `cargo test -p wish_audio --features output --test speaker -- --ignored --nocapture`

#![cfg(feature = "output")]
#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_audio::output::Speaker;
use wish_audio::{Voice, sine_wav};
use wish_core::Entity;

#[test]
#[ignore = "plays a tone through the sound card"]
fn a_tone_plays_pans_and_stops() {
    let mut speaker = match Speaker::open() {
        Ok(speaker) => speaker,
        Err(problem) => {
            println!("no sound device: {problem}");
            assert_eq!(problem.code, "audio_device");
            assert!(!problem.hint.is_empty());
            return;
        }
    };
    let wav = sine_wav(440.0, 1.0, 22050);
    let read = |_: &str| Some(wav.clone());
    let mut voice = Voice {
        entity: Entity::from_bits(1),
        clip: "tone.wav".to_owned(),
        gain: 0.2,
        pan: -1.0,
        pitch: 1.0,
        looping: true,
        position: 0.0,
    };
    speaker.sync(std::slice::from_ref(&voice), &read);
    std::thread::sleep(std::time::Duration::from_millis(400));
    voice.pan = 1.0;
    speaker.sync(std::slice::from_ref(&voice), &read);
    std::thread::sleep(std::time::Duration::from_millis(400));
    speaker.sync(&[], &read);
    std::thread::sleep(std::time::Duration::from_millis(100));
}
