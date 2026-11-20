//! Crash reports (M9): when a program of the engine stops on an unexpected
//! error (a panic that nothing caught), a short report is written on this
//! computer: what failed, where, and a backtrace. Nothing is ever sent; the
//! person may attach the file to an issue to help fix it.
//!
//! Panics that are caught (a hostile model file, a tool that failed) are
//! not crashes and leave no report: the hook only notes each thread's last
//! panic (cheaply: the backtrace is resolved only if a report is written),
//! and [`guard`] (for `main`) or [`thread`] (for a spawned thread's body)
//! writes the report when one escapes.

use std::backtrace::Backtrace;
use std::panic::{AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::ThreadId;

/// How many reports are kept (older ones are removed).
pub const MAX_REPORTS: usize = 20;
/// How many threads' last panics are remembered.
const MAX_NOTES: usize = 16;

/// What the hook noted about a thread's last panic.
#[derive(Debug)]
struct Noted {
    thread: ThreadId,
    message: String,
    place: String,
    backtrace: Backtrace,
}

/// The program's name and the folders reports may go to, in order.
#[derive(Debug, Default)]
struct Setup {
    program: String,
    folders: Vec<PathBuf>,
}

static NOTES: Mutex<Vec<Noted>> = Mutex::new(Vec::new());
static SETUP: Mutex<Option<Setup>> = Mutex::new(None);

/// Where the engine keeps the person's own settings (editor preferences):
/// `WISH_CONFIG_DIR`, else `%APPDATA%\Wish` on Windows,
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

/// This computer's own folder for the engine, for what must not travel
/// with a roaming profile (crash reports, installed engines and the list
/// of engine folders, which name this computer's paths):
/// `%LOCALAPPDATA%\Wish` on Windows, else [`config_folder`] (also when
/// `WISH_CONFIG_DIR` names one).
pub fn machine_folder() -> Option<PathBuf> {
    if std::env::var_os("WISH_CONFIG_DIR").is_none_or(|value| value.is_empty())
        && cfg!(windows)
        && let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())
    {
        return Some(PathBuf::from(local).join("Wish"));
    }
    config_folder()
}

/// Where crash reports of the engine's own programs go: `crashes` under
/// [`machine_folder`].
pub fn folder() -> Option<PathBuf> {
    machine_folder().map(|dir| dir.join("crashes"))
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

/// Notes every panic (what, where, which thread, the unresolved backtrace)
/// and lets the earlier hook print it as before. `program` names the
/// reports; `folders` are tried in order until one can be written. Call
/// once, first thing in `main`.
pub fn install(program: &str, folders: Vec<PathBuf>) {
    if let Ok(mut setup) = SETUP.lock() {
        *setup = Some(Setup {
            program: program.to_owned(),
            folders,
        });
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        let noted = Noted {
            thread: std::thread::current().id(),
            message: payload_text(info.payload()),
            place: info
                .location()
                .map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column()))
                .unwrap_or_default(),
            // Captured, not resolved: caught panics stay cheap.
            backtrace: Backtrace::force_capture(),
        };
        if let Ok(mut notes) = NOTES.lock() {
            notes.retain(|note| note.thread != noted.thread);
            if notes.len() >= MAX_NOTES {
                notes.remove(0);
            }
            notes.push(noted);
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

/// Runs `main`; if a panic escapes it, writes a report and returns its
/// path as the error (`None` if none could be written); the caller says
/// where it is and exits with a failure.
pub fn guard<T>(main: impl FnOnce() -> T) -> Result<T, Option<PathBuf>> {
    std::panic::catch_unwind(AssertUnwindSafe(main))
        .map_err(|payload| write_for(payload.as_ref(), None))
}

/// Runs a spawned thread's body; if a panic escapes it, writes a report
/// (naming the thread) and lets the panic go on, so that joining the
/// thread sees it as before.
pub fn thread<T>(body: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            let name = std::thread::current()
                .name()
                .map_or_else(|| "(unnamed)".to_owned(), str::to_owned);
            let _ = write_for(payload.as_ref(), Some(&name));
            std::panic::resume_unwind(payload)
        }
    }
}

/// The report of the panic that escaped on this thread.
fn write_for(payload: &(dyn std::any::Any + Send), thread: Option<&str>) -> Option<PathBuf> {
    let message = payload_text(payload);
    let here = std::thread::current().id();
    // This thread's own note, and only if it is about this panic (a
    // `resume_unwind` does not run the hook again).
    let noted = NOTES.lock().ok().and_then(|mut notes| {
        let at = notes
            .iter()
            .position(|note| note.thread == here && note.message == message)?;
        Some(notes.remove(at))
    });
    let setup = SETUP.lock().ok()?;
    let setup = setup.as_ref()?;
    setup
        .folders
        .iter()
        .find_map(|folder| write_report(folder, &setup.program, thread, &message, noted.as_ref()))
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

fn write_report(
    folder: &Path,
    program: &str,
    thread: Option<&str>,
    message: &str,
    noted: Option<&Noted>,
) -> Option<PathBuf> {
    // Wall-clock time names the file; it is not part of any simulation.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let safe: String = program
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    std::fs::create_dir_all(folder).ok()?;
    let path = folder.join(format!(
        "crash-{seconds:012}-{}-{safe}.txt",
        std::process::id()
    ));
    let place = noted.map_or("(unknown)", |note| note.place.as_str());
    let backtrace = noted.map_or_else(|| "(none)".to_owned(), |note| note.backtrace.to_string());
    let text = format!(
        "Wish Engine crash report\n\nprogram: {program} {}\nthread: {}\nsystem: {} {}\ntime: {seconds} (seconds since 1970)\nerror: {message}\nwhere: {place}\n\nbacktrace:\n{backtrace}\n\nThis report stays on this computer; nothing was sent. To help fix the problem, attach it to an issue. It holds no project files, but read it first: paths in it may name folders on your computer.\n",
        env!("CARGO_PKG_VERSION"),
        thread.unwrap_or("main"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    std::fs::write(&path, text).ok()?;
    // Only the newest reports are kept.
    for old in reports(folder).into_iter().skip(MAX_REPORTS) {
        let _ = std::fs::remove_file(old);
    }
    Some(path)
}
