//! `wish-engine`: the program a person starts (the desktop shortcut, a
//! double-click). It starts `wen edit` beside it without a console window
//! (`wen` is a command-line program: started on its own, Windows would open
//! a black console behind the editor) and ends.
//!
//! - No arguments: the editor's welcome screen.
//! - A project folder or its `wish.json` (dropped on the program, "Open
//!   with"): the editor on that project.
//! - Anything else is handed to `wen edit` as it is.
//!
//! A launcher that cannot start the editor has no console to say why: it
//! writes the reason to `launcher-error.txt` in the engine's settings folder
//! and opens it.

// Windows: a program with windows, not a console program.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The command line `wen` gets from what the launcher was given.
fn wen_arguments(given: &[OsString]) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["edit".into()];
    match given {
        [] => {}
        [one] => {
            let path = Path::new(one);
            let folder = if path.file_name().is_some_and(|name| name == "wish.json") {
                path.parent().map(Path::to_path_buf)
            } else if path.is_dir() {
                Some(path.to_path_buf())
            } else {
                None
            };
            match folder {
                Some(folder) => {
                    args.push("--root".into());
                    args.push(folder.into_os_string());
                }
                None => args.push(one.clone()),
            }
        }
        many => args.extend(many.iter().cloned()),
    }
    args
}

/// `wen` beside this program.
fn wen_beside() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "wen.exe" } else { "wen" };
    let wen = exe.with_file_name(name);
    wen.is_file().then_some(wen)
}

/// Tells the person why the editor did not start: a text file, opened.
fn report(problem: &str) {
    let folder = std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(std::env::temp_dir)
    .join(if cfg!(windows) { "Wish" } else { ".wish" });
    let _ = std::fs::create_dir_all(&folder);
    let file = folder.join("launcher-error.txt");
    let text = format!("Wish Engine could not start.\n\n{problem}\n");
    if std::fs::write(&file, text).is_ok() {
        let opener = if cfg!(windows) { "notepad" } else { "xdg-open" };
        let _ = Command::new(opener).arg(&file).spawn();
    }
}

fn main() -> ExitCode {
    let given: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(wen) = wen_beside() else {
        report(
            "wen was not found beside wish-engine. The engine's files belong together in one folder (wish-engine, wen, the sdk and toolchain folders): reinstall the engine if one is missing.",
        );
        return ExitCode::FAILURE;
    };
    let mut command = Command::new(&wen);
    command.args(wen_arguments(&given));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// The child gets no console window (it has its own window).
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // The editor's own words go nowhere without a console: what it says on
    // failing is kept and shown.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let finished = match command.spawn() {
        Ok(child) => child.wait_with_output(),
        Err(error) => {
            report(&format!("{} could not be started: {error}.", wen.display()));
            return ExitCode::FAILURE;
        }
    };
    match finished {
        Ok(output) if output.status.success() => ExitCode::SUCCESS,
        Ok(output) => {
            let said = String::from_utf8_lossy(&output.stderr);
            let said = said.trim();
            report(&if said.is_empty() {
                format!("The editor ended ({}).", output.status)
            } else {
                format!("The editor ended ({}):\n\n{said}", output.status)
            });
            ExitCode::FAILURE
        }
        Err(error) => {
            report(&format!("The editor could not be watched: {error}."));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: Vec<OsString>) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn nothing_opens_the_welcome_screen() {
        assert_eq!(words(wen_arguments(&[])), ["edit"]);
    }

    #[test]
    fn a_project_opens_in_the_editor() {
        let dir = std::env::temp_dir().join("wish-launcher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let folder = dir.display().to_string();
        assert_eq!(
            words(wen_arguments(&[dir.clone().into_os_string()])),
            ["edit", "--root", folder.as_str()]
        );
        let file = dir.join("wish.json");
        assert_eq!(
            words(wen_arguments(&[file.into_os_string()])),
            ["edit", "--root", folder.as_str()]
        );
    }

    #[test]
    fn other_arguments_go_to_wen_edit() {
        let given: Vec<OsString> = vec!["--lang".into(), "tr".into()];
        assert_eq!(words(wen_arguments(&given)), ["edit", "--lang", "tr"]);
    }
}
