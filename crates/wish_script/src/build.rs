//! Compiling game code (ADR-025): one `rustc` call against an SDK built in
//! advance, no cargo, no host linker.
//!
//! - [`pack_sdk`] (the engine's own build, where cargo and a linker exist)
//!   builds `wish_sdk` for wasm32 and copies what a game build needs into a
//!   folder: the wasm32 libraries, the procedural macro libraries for the
//!   host, and `sdk.json` (the compiler version they belong to, the paths
//!   of the crates game code names).
//! - [`build_game`] compiles `scripts/src/lib.rs` of a project into
//!   `.wish/build/game.wasm` and turns the compiler's messages into
//!   diagnostics with the project-relative file and line.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result, Severity};

/// Where the game's sources are, relative to the project.
pub const SCRIPTS_ENTRY: &str = "scripts/src/lib.rs";
/// Where the built module goes, relative to the project.
pub const GAME_WASM: &str = ".wish/build/game.wasm";
const INCREMENTAL: &str = ".wish/build/incremental";
/// The description file of an SDK pack.
const SDK_FILE: &str = "sdk.json";
/// The most compiler messages kept in a build report.
const MAX_MESSAGES: usize = 20;

/// Empties an output folder before it is written again, but only if
/// everything in it has one of the names wen writes there (`known`; a
/// folder left half-written counts): `--out .` must never delete a project
/// (M5 closing review).
pub fn clear_output_folder(out: &Path, known: &[&str]) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(out) else {
        return Ok(());
    };
    let mut strangers: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !known.contains(&name.as_str()))
        .collect();
    if !strangers.is_empty() {
        strangers.sort();
        strangers.truncate(5);
        return Err(Diagnostic::error(
            "output_exists",
            format!(
                "'{}' has files wen did not make there ({}).",
                out.display(),
                strangers.join(", ")
            ),
            format!(
                "Choose a new or empty folder for --out; wen replaces only folders that hold nothing but what it writes ({}).",
                known.join(", ")
            ),
        ));
    }
    std::fs::remove_dir_all(out).map_err(|e| io_failed("empty the output folder", &e))
}

/// What an SDK pack holds.
pub const SDK_ENTRIES: &[&str] = &[SDK_FILE, "wasm", "host"];
/// What a packed toolchain holds.
pub const TOOLCHAIN_ENTRIES: &[&str] = &[TOOLCHAIN_FILE, "bin", "lib", "share"];
/// Where a packed toolchain keeps the compiler's licence notices (where
/// rustup's rustc component puts them, relative to the sysroot).
pub const TOOLCHAIN_NOTICES: &str = "share/doc/rust";

fn io_failed(what: &str, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "script_build_failed",
        format!("Could not {what}: {error}."),
        "Check that the project folder and the engine's folders can be read and written.",
    )
}

/// The compiler's version line (`rustc 1.100.0 (…)`) for `rustc`.
fn rustc_version(rustc: &Path) -> Result<String> {
    let out = Command::new(rustc)
        .arg("-vV")
        .output()
        .map_err(|e| toolchain_missing(rustc, &e.to_string()))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    text.lines()
        .next()
        .map(str::to_owned)
        .filter(|line| line.starts_with("rustc "))
        .ok_or_else(|| toolchain_missing(rustc, "it did not report a version"))
}

fn toolchain_missing(rustc: &Path, why: &str) -> Diagnostic {
    Diagnostic::error(
        "script_toolchain_missing",
        format!(
            "The game code compiler '{}' could not be run: {why}.",
            rustc.display()
        ),
        "The engine package includes the compiler in its 'toolchain' folder; reinstall the engine if it is missing. For engine development, WISH_RUSTC can point at a rustc.",
    )
}

/// Builds `wish_sdk` for wasm32 with `cargo` (from the engine's workspace,
/// `manifest`) and packs it into `out`. `rustc` is the compiler games will
/// be built with; the pack records its version.
pub fn pack_sdk(cargo: &Path, manifest: &Path, target_dir: &Path, out: &Path) -> Result<Value> {
    let mut command = Command::new(cargo);
    if let Some(dir) = manifest.parent() {
        command.current_dir(dir);
    }
    let output = command
        .args([
            "build",
            "-p",
            "wish_sdk",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--message-format=json",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--target-dir")
        .arg(target_dir)
        .env("RUSTFLAGS", "")
        .output()
        .map_err(|e| io_failed("run cargo to build the SDK", &e))?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            "script_build_failed",
            format!(
                "Building the SDK failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
            "The SDK is built from the engine's own sources (engine development only).",
        ));
    }
    let wasm = out.join("wasm");
    let host = out.join("host");
    clear_output_folder(out, SDK_ENTRIES)?;
    std::fs::create_dir_all(&wasm).map_err(|e| io_failed("create the SDK folder", &e))?;
    std::fs::create_dir_all(&host).map_err(|e| io_failed("create the SDK folder", &e))?;
    let mut externs = serde_json::Map::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let name = message["target"]["name"].as_str().unwrap_or("");
        let kinds: Vec<&str> = message["target"]["kind"]
            .as_array()
            .map(|k| k.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for file in message["filenames"].as_array().into_iter().flatten() {
            let Some(file) = file.as_str().map(PathBuf::from) else {
                continue;
            };
            let Some(file_name) = file.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            let is_wasm = file
                .components()
                .any(|c| c.as_os_str() == "wasm32-unknown-unknown");
            let is_macro = kinds.contains(&"proc-macro")
                && (file_name.ends_with(".dll")
                    || file_name.ends_with(".so")
                    || file_name.ends_with(".dylib"));
            let place = if is_macro {
                &host
            } else if is_wasm && (file_name.ends_with(".rlib") || file_name.ends_with(".rmeta")) {
                &wasm
            } else {
                continue;
            };
            std::fs::copy(&file, place.join(&file_name))
                .map_err(|e| io_failed(&format!("copy {file_name} into the SDK"), &e))?;
            if is_wasm
                && file_name.ends_with(".rlib")
                && ["wish_sdk", "wish_schema"].contains(&name)
            {
                externs.insert(name.to_owned(), Value::from(format!("wasm/{file_name}")));
            }
        }
    }
    // The compiler that built the pack, found the way cargo found it (from
    // the engine's folder, so rustup picks the pinned toolchain). Game builds
    // must use the same one.
    let rustc = std::env::var_os("RUSTC").map_or_else(|| PathBuf::from("rustc"), PathBuf::from);
    let compiler = real_rustc_in(&rustc, manifest.parent())?;
    let description = json!({
        "format": 1,
        "rustc": rustc_version(&compiler)?,
        "rustc_path": compiler.display().to_string(),
        "externs": externs,
    });
    std::fs::write(
        out.join(SDK_FILE),
        serde_json::to_string_pretty(&description).unwrap_or_default(),
    )
    .map_err(|e| io_failed("write sdk.json", &e))?;
    Ok(description)
}

/// The compiler binary behind `rustc`, which may be rustup's proxy (the
/// proxy needs rustup's environment, which the build does not pass on).
fn real_rustc(rustc: &Path) -> Result<PathBuf> {
    real_rustc_in(rustc, None)
}

/// [`real_rustc`], asked from folder `dir` (rustup's toolchain file there
/// decides which compiler the proxy means).
fn real_rustc_in(rustc: &Path, dir: Option<&Path>) -> Result<PathBuf> {
    let mut command = Command::new(rustc);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let out = command
        .args(["--print", "sysroot"])
        .output()
        .map_err(|e| toolchain_missing(rustc, &e.to_string()))?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let exe = if cfg!(windows) { "rustc.exe" } else { "rustc" };
    let real = sysroot.join("bin").join(exe);
    if real.is_file() {
        Ok(real)
    } else {
        Err(toolchain_missing(rustc, "its sysroot has no compiler"))
    }
}

/// Where the engine's SDK and compiler are.
#[derive(Clone, Debug)]
pub struct Toolchain {
    /// The SDK pack folder.
    pub sdk: PathBuf,
    /// The compiler.
    pub rustc: PathBuf,
}

impl Toolchain {
    /// Finds them: `WISH_SDK` / `WISH_RUSTC` if set, else `sdk/` and
    /// `toolchain/bin/rustc` next to the running program, else (for the
    /// compiler) the `rustc` on the PATH.
    pub fn locate() -> Result<Self> {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let sdk = std::env::var_os("WISH_SDK")
            .map(PathBuf::from)
            .or_else(|| beside.as_ref().map(|dir| dir.join("sdk")))
            .filter(|dir| dir.join(SDK_FILE).is_file())
            .ok_or_else(|| {
                Diagnostic::error(
                    "script_sdk_missing",
                    "The game SDK the engine compiles game code against was not found.",
                    "The engine package has it in its 'sdk' folder; reinstall the engine if it is missing. For engine development, run `wen sdk` once (it builds the SDK next to wen), or point WISH_SDK at a pack.",
                )
            })?;
        let exe = if cfg!(windows) { "rustc.exe" } else { "rustc" };
        let rustc = match std::env::var_os("WISH_RUSTC") {
            Some(path) => PathBuf::from(path),
            None => match beside.map(|dir| dir.join("toolchain").join("bin").join(exe)) {
                Some(bundled) if bundled.is_file() => bundled,
                // Engine development: the compiler that built the SDK pack
                // (another rustup toolchain of the same version may lack
                // the wasm32 target).
                _ => std::fs::read_to_string(sdk.join(SDK_FILE))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|d| d["rustc_path"].as_str().map(PathBuf::from))
                    .filter(|path| path.is_file())
                    .map_or_else(|| real_rustc(Path::new("rustc")), Ok)?,
            },
        };
        Ok(Self { sdk, rustc })
    }
}

/// What a build did.
#[derive(Clone, Debug)]
pub struct BuildReport {
    /// Whether the module was built.
    pub passed: bool,
    /// The compiler's errors and warnings, as diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// How many errors and warnings there were in all (the list is cut).
    pub errors: usize,
    /// How many warnings.
    pub warnings: usize,
    /// The built module (project-relative), when it passed.
    pub output: Option<String>,
}

impl BuildReport {
    /// The report in tool-answer form. It carries no duration: a tool's
    /// answer is the same bytes in every process (ADR-027, rule 2); the
    /// command line and the editor time the call themselves.
    pub fn to_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "errors": self.errors,
            "warnings": self.warnings,
            "output": self.output,
            "diagnostics": self.diagnostics.iter().map(|d| serde_json::to_value(d).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        })
    }
}

/// One compiler message as a diagnostic.
fn from_rustc(message: &Value) -> Option<Diagnostic> {
    let level = message.get("level")?.as_str()?;
    let severity = match level {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warning,
        _ => return None,
    };
    let text = message.get("message")?.as_str()?;
    if text.starts_with("aborting due to")
        || text.ends_with("warnings emitted")
        || text.ends_with("warning emitted")
    {
        return None;
    }
    let code = message
        .get("code")
        .and_then(|c| c.get("code"))
        .and_then(Value::as_str);
    let mut hints: Vec<String> = Vec::new();
    for child in message
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let level = child.get("level").and_then(Value::as_str).unwrap_or("");
        let Some(child_text) = child.get("message").and_then(Value::as_str) else {
            continue;
        };
        if level == "help" || level == "note" {
            let suggestion = child
                .get("spans")
                .and_then(Value::as_array)
                .and_then(|spans| spans.first())
                .and_then(|span| span.get("suggested_replacement"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() < 200);
            hints.push(match suggestion {
                Some(replacement) => format!("{child_text}: `{replacement}`"),
                None => child_text.to_owned(),
            });
        }
    }
    let hint = if hints.is_empty() {
        match code {
            Some(code) => format!(
                "Fix the line named. `rustc --explain {code}` describes this error in detail."
            ),
            None => "Fix the line named; the message says what the compiler expected.".to_owned(),
        }
    } else {
        // The compiler's notes can list dozens of trait implementations;
        // the first lines carry the advice.
        let joined = hints
            .iter()
            .take(3)
            .map(|h| h.lines().take(2).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join(" ");
        if joined.chars().count() > 500 {
            format!("{}…", joined.chars().take(500).collect::<String>())
        } else {
            joined
        }
    };
    let prefix = code.map(|c| format!("[{c}] ")).unwrap_or_default();
    let (diagnostic_code, kind) = if severity == Severity::Error {
        ("script_compile_error", "error")
    } else {
        ("script_compile_warning", "warning")
    };
    let mut diagnostic = match severity {
        Severity::Error => Diagnostic::error(
            diagnostic_code,
            format!("Compile {kind}: {prefix}{text}"),
            hint,
        ),
        _ => Diagnostic::warning(
            diagnostic_code,
            format!("Compile {kind}: {prefix}{text}"),
            hint,
        ),
    };
    let primary = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| {
            spans
                .iter()
                .find(|s| s.get("is_primary").and_then(Value::as_bool) == Some(true))
                .or_else(|| spans.first())
        });
    if let Some(span) = primary {
        if let Some(file) = span.get("file_name").and_then(Value::as_str) {
            diagnostic = diagnostic.at_file(file.replace('\\', "/"));
        }
        if let Some(line) = span.get("line_start").and_then(Value::as_u64) {
            let column = span
                .get("column_start")
                .and_then(Value::as_u64)
                .and_then(|c| u32::try_from(c).ok());
            diagnostic = diagnostic.at_line(u32::try_from(line).unwrap_or(0), column);
        }
    }
    Some(diagnostic)
}

/// Where a build that is not installed leaves its module.
pub const CHECKED_WASM: &str = ".wish/build/game.next.wasm";

/// How long the compiler may run before the build is stopped: compile-time
/// evaluation can be made to run for ever (M5 closing review).
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(120);

/// Compiles the game code of the project at `root`. With `install`, a
/// module that builds replaces [`GAME_WASM`]; without, it is left at
/// [`CHECKED_WASM`] (a check that changes nothing). A failed build leaves
/// the previous module in place.
pub fn build_game(toolchain: &Toolchain, root: &Path, install: bool) -> Result<BuildReport> {
    build_game_within(toolchain, root, install, BUILD_TIMEOUT)
}

/// A report for a build that never reached the compiler, or was stopped.
fn refused_build(diagnostic: Diagnostic) -> BuildReport {
    BuildReport {
        passed: false,
        diagnostics: vec![diagnostic],
        errors: 1,
        warnings: 0,
        output: None,
    }
}

/// [`build_game`] with a time limit for the compiler.
pub fn build_game_within(
    toolchain: &Toolchain,
    root: &Path,
    install: bool,
    timeout: Duration,
) -> Result<BuildReport> {
    // The compiler runs in the project folder: every path it is given must
    // not depend on where this program runs (`wen build --root game`).
    let absolute = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let root = absolute.as_path();
    if !root.join(SCRIPTS_ENTRY).is_file() {
        return Err(Diagnostic::error(
            "script_sources_missing",
            format!("The project has no game code: '{SCRIPTS_ENTRY}' does not exist."),
            "Game code lives in scripts/src/lib.rs. Projects made with `wen new` have one; create the file to start (use wish_sdk::prelude::*; then #[system] functions and wish_sdk::export!).",
        )
        .at_file(SCRIPTS_ENTRY));
    }
    let description: Value = std::fs::read_to_string(toolchain.sdk.join(SDK_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| {
            Diagnostic::error(
                "script_sdk_missing",
                "The game SDK's sdk.json could not be read.",
                "Reinstall the engine, or rebuild the SDK pack (`wen sdk`) for engine development.",
            )
        })?;
    let version = rustc_version(&toolchain.rustc)?;
    let expected = description["rustc"].as_str().unwrap_or("");
    if version != expected {
        return Err(Diagnostic::error(
            "script_toolchain_mismatch",
            format!(
                "The game SDK was built by '{expected}', but the compiler found is '{version}'."
            ),
            "The SDK and the compiler must come from the same engine package. Reinstall the engine; for engine development, rebuild the SDK pack with `wen sdk`.",
        ));
    }
    // Game code is untrusted: it may not read the machine while it builds.
    if let Err(refusal) = crate::source_check::check_sources(root) {
        return Ok(refused_build(refusal));
    }
    let build = root.join(".wish").join("build");
    std::fs::create_dir_all(&build).map_err(|e| io_failed("create .wish/build", &e))?;
    // The build folder must be the project's own, not a link elsewhere.
    let inside = std::fs::canonicalize(&build)
        .ok()
        .zip(std::fs::canonicalize(root).ok())
        .is_some_and(|(build, root)| build.starts_with(root));
    if !inside {
        return Ok(refused_build(Diagnostic::error(
            "script_build_failed",
            "The project's .wish/build folder leads outside the project.",
            "Delete .wish/build (it is a link) and build again; the engine recreates it.",
        )));
    }
    let output = root.join(CHECKED_WASM);
    let _ = std::fs::remove_file(&output);
    let mut command = Command::new(&toolchain.rustc);
    // Nothing from the user's environment reaches the compiler: the same
    // sources build the same module on every machine.
    command.env_clear();
    if let Some(bin) = toolchain.rustc.parent() {
        command.env("PATH", bin);
    }
    for keep in ["SystemRoot", "SYSTEMROOT", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(keep) {
            command.env(keep, value);
        }
    }
    command.current_dir(root).args([
        "--edition",
        "2024",
        "--crate-type",
        "cdylib",
        "--crate-name",
        "game",
        "--target",
        "wasm32-unknown-unknown",
        "-C",
        "opt-level=2",
        "-C",
        "panic=abort",
        "-C",
        "debuginfo=line-tables-only",
        "--error-format=json",
    ]);
    command
        .arg("-C")
        .arg(format!("incremental={}", root.join(INCREMENTAL).display()));
    for (name, file) in description["externs"].as_object().into_iter().flatten() {
        if let Some(file) = file.as_str() {
            command
                .arg("--extern")
                .arg(format!("{name}={}", toolchain.sdk.join(file).display()));
        }
    }
    command
        .arg("-L")
        .arg(format!(
            "dependency={}",
            toolchain.sdk.join("wasm").display()
        ))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            toolchain.sdk.join("host").display()
        ))
        .arg("-o")
        .arg(&output)
        .arg(SCRIPTS_ENTRY);
    let start = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| toolchain_missing(&toolchain.rustc, &e.to_string()))?;
    // Read the messages on a thread so a full pipe cannot stall the wait.
    let reader = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            text
        })
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stderr = reader
        .and_then(|thread| thread.join().ok())
        .unwrap_or_default();
    let Some(status) = status else {
        let _ = std::fs::remove_file(&output);
        return Ok(refused_build(
            Diagnostic::error(
                "script_build_timeout",
                format!(
                    "The compiler was stopped after {} seconds.",
                    timeout.as_secs()
                ),
                "Something in the game code makes compilation run for very long: look for a const or static computed by a long loop, or a deeply recursive macro. The previous build stays in use.",
            )
            .at_file(SCRIPTS_ENTRY),
        ));
    };
    let mut diagnostics = Vec::new();
    let (mut errors, mut warnings) = (0, 0);
    for line in String::from_utf8_lossy(&stderr).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(diagnostic) = from_rustc(&message) {
            if diagnostic.severity == Severity::Error {
                errors += 1;
            } else {
                warnings += 1;
            }
            if diagnostics.len() < MAX_MESSAGES {
                diagnostics.push(diagnostic);
            }
        }
    }
    let passed = status.success() && output.is_file();
    if passed && install {
        std::fs::rename(&output, root.join(GAME_WASM))
            .map_err(|e| io_failed("put the built module in place", &e))?;
    } else if !passed && errors == 0 {
        errors = 1;
        diagnostics.push(Diagnostic::error(
            "script_compile_error",
            format!(
                "The compiler failed without a message it could explain: {}",
                if stderr.is_empty() {
                    format!("it exited with {status} and wrote no module to {CHECKED_WASM}")
                } else {
                    String::from_utf8_lossy(&stderr)
                        .chars()
                        .take(2000)
                        .collect::<String>()
                }
            ),
            "Report this with the message; the previous build stays in use.",
        ));
    }
    Ok(BuildReport {
        passed,
        diagnostics,
        errors,
        warnings,
        output: passed.then(|| if install { GAME_WASM } else { CHECKED_WASM }.to_owned()),
    })
}

/// The description file of a packed toolchain.
const TOOLCHAIN_FILE: &str = "toolchain.json";

fn copy_tree(from: &Path, to: &Path, skip: &dyn Fn(&str) -> bool) -> Result<u64> {
    std::fs::create_dir_all(to).map_err(|e| io_failed("create the toolchain folder", &e))?;
    let mut bytes = 0;
    let entries =
        std::fs::read_dir(from).map_err(|e| io_failed("read the compiler's folder", &e))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if skip(&name) {
            continue;
        }
        let target = to.join(&name);
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => bytes += copy_tree(&entry.path(), &target, skip)?,
            Ok(_) => {
                bytes += std::fs::copy(entry.path(), &target)
                    .map_err(|e| io_failed(&format!("copy {name}"), &e))?;
            }
            Err(_) => {}
        }
    }
    Ok(bytes)
}

/// Copies the parts of the compiler `rustc` that a game build needs into
/// `out` (ADR-025): the compiler and its libraries, the WebAssembly linker,
/// and the standard library for wasm32, with the compiler's licence notices
/// ([`TOOLCHAIN_NOTICES`]: `COPYRIGHT*` and `licenses/`, when the compiler
/// has them; a package redistributes the compiler). Nothing else: no cargo,
/// no rustup, no host standard library, no debug databases, no
/// documentation. The folder works on its own: `out/bin/rustc` finds its
/// standard library relative to itself.
pub fn pack_toolchain(rustc: &Path, out: &Path) -> Result<Value> {
    let real = real_rustc(rustc)?;
    let version = rustc_version(&real)?;
    let bin = real
        .parent()
        .ok_or_else(|| toolchain_missing(&real, "it is not in a bin folder"))?;
    let sysroot = bin
        .parent()
        .ok_or_else(|| toolchain_missing(&real, "it has no sysroot"))?;
    let host = Command::new(&real)
        .arg("-vV")
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
        })
        .ok_or_else(|| toolchain_missing(&real, "it does not say which machine it runs on"))?;
    clear_output_folder(out, TOOLCHAIN_ENTRIES)?;
    let library =
        |name: &str| name.ends_with(".dll") || name.contains(".so") || name.ends_with(".dylib");
    let mut bytes = 0;
    // The compiler and the libraries it is made of.
    for (folder, keep) in [
        (
            "bin",
            &(|name: &str| name == "rustc" || name == "rustc.exe" || library(name))
                as &dyn Fn(&str) -> bool,
        ),
        ("lib", &library),
    ] {
        let target = out.join(folder);
        std::fs::create_dir_all(&target)
            .map_err(|e| io_failed("create the toolchain folder", &e))?;
        for entry in std::fs::read_dir(sysroot.join(folder))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type().is_ok_and(|kind| kind.is_file()) && keep(&name) {
                bytes += std::fs::copy(entry.path(), target.join(&name))
                    .map_err(|e| io_failed(&format!("copy {name}"), &e))?;
            }
        }
    }
    let rustlib = Path::new("lib").join("rustlib");
    // The standard library for wasm32.
    bytes += copy_tree(
        &sysroot.join(&rustlib).join("wasm32-unknown-unknown"),
        &out.join(&rustlib).join("wasm32-unknown-unknown"),
        &|name| name.ends_with(".pdb"),
    )?;
    // The WebAssembly linker (rust-lld and the wasm-ld name it is called by).
    let tools = sysroot.join(&rustlib).join(&host).join("bin");
    bytes += copy_tree(
        &tools,
        &out.join(&rustlib).join(&host).join("bin"),
        &|name| {
            let wanted =
                name.starts_with("rust-lld") || name == "gcc-ld" || name.starts_with("wasm-ld");
            !wanted || name.ends_with(".pdb")
        },
    )?;
    // The compiler's notices (the Rust project's and those of what it is
    // built from: LLVM and others): COPYRIGHT*.html and licenses/.
    let notices = sysroot.join(TOOLCHAIN_NOTICES);
    if notices.is_dir() {
        bytes += copy_tree(&notices, &out.join(TOOLCHAIN_NOTICES), &|name| {
            !(name.starts_with("COPYRIGHT") || name.starts_with("LICENSE") || name == "licenses")
                && !name.ends_with(".txt")
        })?;
    }
    let description = json!({
        "format": 1,
        "rustc": version,
        "host": host,
        "bytes": bytes,
    });
    std::fs::write(
        out.join(TOOLCHAIN_FILE),
        serde_json::to_string_pretty(&description).unwrap_or_default(),
    )
    .map_err(|e| io_failed("write toolchain.json", &e))?;
    Ok(description)
}
