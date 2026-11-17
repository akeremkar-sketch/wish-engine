//! Crash reports (M9): when a program of the engine stops on an unexpected
//! error (a panic that nothing caught), a short report is written on this
//! computer: what failed, where, and a backtrace. Nothing is ever sent; the
//! person may attach the file to an issue to help fix it.
//!
//! Panics that are caught (a hostile model file, a tool that failed) are
//! not crashes and leave no report: the hook only notes the last panic, and
//! [`guard`] writes the report when one escapes `main`.

use std::backtrace::Backtrace;
use std::panic::{AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What the hook noted about the last panic.
#[derive(Clone, Debug, Default)]
struct Noted {
    message: String,
    place: String,
    backtrace: String,
}

static LAST: Mutex<Option<Noted>> = Mutex::new(None);

/// Where the engine keeps the person's own files (editor preferences,
/// crash reports): `WISH_CONFIG_DIR`, else `%APPDATA%\Wish` on Windows,
/// `$XDG_CONFIG_HOME/wish` or `~/.config/wish` elsewhere.
pub fn config_folder() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    if let Some(dir) = var("WISH_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        return Some(PathBuf::from(var("APPDATA")?).join("Wish"));
    }
    if let Some(dir) = var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("wish"));
    }
    Some(PathBuf::from(var("HOME")?).join(".config").join("wish"))
}

/// Where crash reports of the engine's own programs go.
pub fn folder() -> Option<PathBuf> {
    config_folder().map(|dir| dir.join("crashes"))
}

/// The crash reports in `folder`, newest first.
pub fn reports(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("crash-") && name.ends_with(".txt"))
        })
        .collect();
    // The names start with the time, zero-padded: by name is by time.
    found.sort();
    found.reverse();
    found
}

/// Notes every panic (what and where, with a backtrace) and then lets the
/// earlier hook print it as before. Call once, first thing in `main`.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        let noted = Noted {
            message: payload_text(info.payload()),
            place: info
                .location()
                .map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column()))
                .unwrap_or_default(),
            backtrace: Backtrace::force_capture().to_string(),
        };
        if let Ok(mut last) = LAST.lock() {
            *last = Some(noted);
        }
        previous(info);
    }));
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

/// Runs `main`; if a panic escapes it, writes a report into `folder` and
/// returns the report's path as the error (the caller says where it is and
/// exits with a failure).
pub fn guard<T>(
    program: &str,
    folder: Option<&Path>,
    main: impl FnOnce() -> T,
) -> Result<T, Option<PathBuf>> {
    match std::panic::catch_unwind(AssertUnwindSafe(main)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            let noted = LAST
                .lock()
                .ok()
                .and_then(|mut last| last.take())
                .unwrap_or_else(|| Noted {
                    message: payload_text(payload.as_ref()),
                    ..Noted::default()
                });
            Err(folder.and_then(|folder| write_report(folder, program, &noted)))
        }
    }
}

/// What to tell the person after a crash (standard error).
pub fn told(program: &str, report: Option<&Path>) -> String {
    match report {
        Some(path) => format!(
            "{program} stopped on an unexpected error. A report was saved to {} (nothing was sent); attaching it to an issue helps fix it.",
            path.display()
        ),
        None => format!("{program} stopped on an unexpected error (no report could be saved)."),
    }
}

fn write_report(folder: &Path, program: &str, noted: &Noted) -> Option<PathBuf> {
    // Wall-clock time names the file; it is not part of any simulation.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let safe: String = program
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    std::fs::create_dir_all(folder).ok()?;
    let path = folder.join(format!("crash-{seconds:012}-{safe}.txt"));
    let text = format!(
        "Wish Engine crash report\n\nprogram: {program} {}\nsystem: {} {}\ntime: {seconds} (seconds since 1970)\nerror: {}\nwhere: {}\n\nbacktrace:\n{}\n\nThis report stays on this computer; nothing was sent. To help fix the problem, attach it to an issue. It holds no project files, but read it first: paths in it may name folders on your computer.\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        noted.message,
        if noted.place.is_empty() {
            "(unknown)"
        } else {
            &noted.place
        },
        if noted.backtrace.is_empty() {
            "(none)"
        } else {
            &noted.backtrace
        },
    );
    std::fs::write(&path, text).ok()?;
    Some(path)
}
