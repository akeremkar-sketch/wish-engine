//! `wen play`: the project's game in a window, in real time.
//!
//! The game is the player's (`wish_player`, the program exported games
//! ship), with building on: saving `scripts/src/lib.rs` builds the new code
//! and puts it into the running session (`script_build`), so the game goes
//! on where it was.

use std::path::Path;

use serde_json::json;
use wish_core::Result;
use wish_player::PlayOptions;

use crate::args::PlayArgs;

/// Runs `wen play`.
pub fn play(args: &PlayArgs, as_json: bool) -> Result<String> {
    let options = PlayOptions {
        root: Path::new(args.root.as_deref().unwrap_or(".")).to_path_buf(),
        scene: args.scene.clone(),
        size: args.size,
        software: args.software,
        exit_after: args.exit_after,
        develop: true,
        title: Some("wen play".to_owned()),
    };
    let played = wish_player::play(
        &options,
        Box::new(move |message| {
            if !as_json {
                eprintln!("{message}");
            }
        }),
    )?;
    for message in &played.messages {
        if !as_json {
            eprintln!("{message}");
        }
    }
    Ok(if as_json {
        json!({
            "ok": true,
            "frames": played.frames,
            "code_reloads": played.code_reloads,
            "rejected": played.rejected,
        })
        .to_string()
    } else {
        format!(
            "Closed after {} frame(s); {} code reload(s), {} rejected.",
            played.frames, played.code_reloads, played.rejected
        )
    })
}
