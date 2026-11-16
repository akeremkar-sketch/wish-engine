//! Sound in sessions (design note M6 §4): reading the project's clips so
//! the simulation knows how long they are, and turning the audio log into
//! session log lines (`logs_get` kind `audio`).

use std::collections::BTreeMap;
use std::time::SystemTime;

use wish_audio::{LogKind, LogLine};
use wish_core::{Diagnostic, Entity};
use wish_ecs::World;

use crate::sessions::name_of;
use crate::workspace::Workspace;

/// The most clip files read for a session.
const MAX_CLIPS: usize = 2000;

/// What is known of a clip file, kept until it changes on disk.
#[derive(Clone, Debug)]
pub(crate) struct Known {
    bytes: u64,
    modified: Option<SystemTime>,
    seconds: Option<f32>,
}

impl Workspace {
    /// The clips of the project (`.wav` and `.ogg` files) and their lengths,
    /// for a session about to start. A file that cannot be read has no
    /// length (sources using it log `audio_clip_unknown`). Lengths are kept
    /// until the file changes.
    pub(crate) fn audio_clips(&mut self) -> Vec<(String, f32)> {
        let root = self.root().to_path_buf();
        let Ok(index) = wish_asset::AssetIndex::scan(&root) else {
            return Vec::new();
        };
        let paths: Vec<String> = index
            .paths()
            .filter(|path| {
                let lower = path.to_ascii_lowercase();
                lower.ends_with(".wav") || lower.ends_with(".ogg")
            })
            .take(MAX_CLIPS)
            .map(str::to_owned)
            .collect();
        let mut found = Vec::new();
        for path in paths {
            let file = root.join(&path);
            let Ok(meta) = std::fs::metadata(&file) else {
                continue;
            };
            let (bytes, modified) = (meta.len(), meta.modified().ok());
            let cached = self.sessions.clips.get(&path);
            let seconds = match cached {
                Some(known) if known.bytes == bytes && known.modified == modified => known.seconds,
                _ => {
                    let seconds = std::fs::read(&file)
                        .ok()
                        .and_then(|data| wish_audio::decode(&path, &data).ok())
                        .map(|clip| clip.seconds());
                    self.sessions.clips.insert(
                        path.clone(),
                        Known {
                            bytes,
                            modified,
                            seconds,
                        },
                    );
                    seconds
                }
            };
            if let Some(seconds) = seconds {
                found.push((path, seconds));
            }
        }
        found
    }
}

/// The cache of clip lengths (part of the sessions).
pub(crate) type Clips = BTreeMap<String, Known>;

/// A decibel figure for a linear gain: "-6.0 dB", or "silent".
fn decibels(gain: f32) -> String {
    if gain <= 1e-6 {
        "silent".to_owned()
    } else {
        format!("{:+.1} dB", 20.0 * gain.log10())
    }
}

fn pan_text(pan: f32) -> String {
    if pan.abs() < 0.05 {
        "centre".to_owned()
    } else if pan < 0.0 {
        format!("left {:.0}%", -pan * 100.0)
    } else {
        format!("right {:.0}%", pan * 100.0)
    }
}

/// An audio log line as a session log entry: its tick and the diagnostic
/// that carries the text.
pub(crate) fn entry(world: &World, line: &LogLine) -> (u64, Diagnostic) {
    let source: Entity = line.source;
    let name = name_of(world, source);
    let diagnostic = match line.kind {
        LogKind::Started => Diagnostic::info(
            "audio_started",
            format!(
                "'{}' started on '{name}': {} at the listener, {}.",
                line.clip,
                decibels(line.gain),
                pan_text(line.pan)
            ),
            "Sounds are described, not played, in a session; `wen play` plays them.",
        ),
        LogKind::Ended => Diagnostic::info(
            "audio_ended",
            format!("'{}' ended on '{name}'.", line.clip),
            "The clip finished, it was switched off, or its entity is gone.",
        ),
        LogKind::Unknown => wish_audio::unknown_clip(line),
    };
    (line.tick, diagnostic)
}
