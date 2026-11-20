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

/// The folders the player may write a problem or a crash report to, in
/// order: next to the program, else this person's own folder for the game
/// (`%LOCALAPPDATA%\<game>`; a game under Program Files cannot write next to
/// itself), else the temporary folder.
fn writable_places() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let mut places: Vec<PathBuf> = exe
        .as_ref()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .into_iter()
        .collect();
    let game = exe
        .as_ref()
        .and_then(|exe| exe.file_stem())
        .map_or_else(|| "game".into(), |stem| stem.to_os_string());
    if let Some(local) = std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    })
    .filter(|value| !value.is_empty())
    {
        places.push(PathBuf::from(local).join(game));
    }
    places.push(std::env::temp_dir());
    places
}

/// A problem, said where a person can see it: on standard error, in
/// `player-error.txt` (in the first of [`writable_places`] that takes it)
/// and, for a double-clicked game (no console), opened in Notepad.
fn report(problem: &str, shown: bool) {
    eprintln!("{problem}");
    let written = writable_places().into_iter().find_map(|place| {
        std::fs::create_dir_all(&place).ok()?;
        let path = place.join("player-error.txt");
        std::fs::write(&path, format!("{problem}\n")).ok()?;
        Some(path)
    });
    if shown && let Some(path) = written {
        show(&path);
    }
}

/// Opens a text file for the person to read (only a windowed release game:
/// it has no console to print to).
fn show(path: &std::path::Path) {
    if cfg!(all(windows, not(debug_assertions))) {
        let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
    }
}

fn main() -> ExitCode {
    // An unexpected error leaves a report (never sent): the person playing
    // may not know the engine, but can send the file to whoever made the
    // game.
    let places = writable_places();
    wish_control::crash::install("wish_player", places);
    let windowed = !std::env::args().any(|arg| arg == "--headless");
    match wish_control::crash::guard(run) {
        Ok(code) => code,
        Err(saved) => {
            let told = wish_control::crash::told("The game", saved.as_deref());
            report(&told, windowed);
            ExitCode::from(101)
        }
    }
}

fn run() -> ExitCode {
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
                report(&format!("{problem}"), false);
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
            report(&format!("{problem}"), true);
            ExitCode::FAILURE
        }
    }
}
