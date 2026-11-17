//! `wish_player`: an exported game's program. It plays the game in `game/`
//! next to it (or `--game <folder>`). `--headless --frames N` runs N ticks
//! without a window and prints where the game ended (for tests).

// An exported game opens its window without a console behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use wish_player::{PlayOptions, play, play_headless};

fn value_of(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

/// Where the game is: `--game`, else `game/` next to this program.
fn game_folder(args: &[String]) -> PathBuf {
    if let Some(folder) = value_of(args, "--game") {
        return PathBuf::from(folder);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("game")))
        .unwrap_or_else(|| PathBuf::from("game"))
}

/// A problem, said where a person can see it: on standard error and in
/// `player-error.txt` next to the program (a double-clicked game has no
/// console).
fn report(problem: &str) {
    eprintln!("{problem}");
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        let _ = std::fs::write(dir.join("player-error.txt"), format!("{problem}\n"));
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = game_folder(&args);
    let scene = value_of(&args, "--scene");
    if args.iter().any(|arg| arg == "--headless") {
        let frames = value_of(&args, "--frames")
            .and_then(|n| n.parse().ok())
            .unwrap_or(600);
        return match play_headless(&root, scene.as_deref(), frames) {
            Ok(answer) => {
                println!("{answer}");
                ExitCode::SUCCESS
            }
            Err(problem) => {
                report(&format!("{problem}"));
                ExitCode::FAILURE
            }
        };
    }
    let size = value_of(&args, "--size")
        .and_then(|text| {
            let (w, h) = text.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280, 720));
    let options = PlayOptions {
        root,
        scene,
        size,
        software: args.iter().any(|arg| arg == "--software"),
        exit_after: value_of(&args, "--exit-after").and_then(|n| n.parse().ok()),
        develop: false,
        title: None,
    };
    match play(&options, Box::new(|message| eprintln!("{message}"))) {
        Ok(_) => ExitCode::SUCCESS,
        Err(problem) => {
            report(&format!("{problem}"));
            ExitCode::FAILURE
        }
    }
}
