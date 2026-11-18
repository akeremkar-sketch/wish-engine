//! Engine versions side by side (M10b; design note M10 §15): the engines a
//! person has on this computer, and which one opens a project.
//!
//! An engine is a folder with `wen` and a `wish-engine.json`
//! (`{ "format": 1, "version": "0.1.0" }`, written by `wen package`). The
//! person's engines are the running one, the folders in
//! `<config>/engines/` (where downloaded engines will go) and folders they
//! added where they are (`<config>/engines.json`, like "Locate" in other
//! hubs). Adding never copies an engine; removing an added one only forgets
//! it, and one under `<config>/engines/` goes to the trash.
//!
//! A project's `wish.json` names the engine version that wrote it. It opens
//! with that exact version if one is installed (a game plays bit for bit the
//! same only on the engine it was made with), else with the newest
//! installed version compatible with it (`^version`, never older); with no
//! such engine it opens with the running one, which says so.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};

use crate::packages::{Version, VersionReq};

/// The file that makes a folder an engine.
pub const ENGINE_FILE: &str = "wish-engine.json";
/// The person's list of added engine folders, in the settings folder.
pub const REGISTRY_FILE: &str = "engines.json";
/// The folder of installed engines, in the settings folder.
pub const ENGINES_FOLDER: &str = "engines";
/// The most engines a person can have.
pub const MAX_ENGINES: usize = 64;
/// The largest `wish-engine.json` or `engines.json` read.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The command-line program of an engine.
pub const WEN: &str = if cfg!(windows) { "wen.exe" } else { "wen" };

/// Where an engine came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The engine this program belongs to.
    Running,
    /// In `<config>/engines/`.
    Installed,
    /// A folder the person added where it is.
    Added,
}

impl Source {
    /// Its name in JSON.
    pub fn name(self) -> &'static str {
        match self {
            Source::Running => "running",
            Source::Installed => "installed",
            Source::Added => "added",
        }
    }
}

/// An engine on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    /// Its version.
    pub version: Version,
    /// Its folder.
    pub folder: PathBuf,
    /// Where it came from.
    pub source: Source,
}

impl Engine {
    /// Its command-line program.
    pub fn wen(&self) -> PathBuf {
        self.folder.join(WEN)
    }

    /// As JSON.
    pub fn to_value(&self) -> Value {
        json!({
            "version": self.version.to_string(),
            "folder": self.folder.display().to_string(),
            "source": self.source.name(),
        })
    }
}

/// The text of `wish-engine.json` for `version` (canonical JSON).
pub fn engine_file_text(version: Version) -> Result<String> {
    wish_scene::json::to_canonical(&json!({ "format": 1, "version": version.to_string() }))
}

/// The engine this program belongs to.
pub fn running() -> Engine {
    let folder = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    Engine {
        version: Version::engine(),
        folder,
        source: Source::Running,
    }
}

fn invalid_folder(folder: &Path, why: &str) -> Diagnostic {
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    Diagnostic::error(
        "engine_folder_invalid",
        format!("'{name}' is not an engine folder: {why}."),
        format!(
            "Choose the folder that has {WEN} and {ENGINE_FILE} (a packaged engine: `wen package` writes both)."
        ),
    )
}

fn read_small(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Reads the engine in `folder`.
pub fn read_engine(folder: &Path, source: Source) -> Result<Engine> {
    if !folder.is_dir() {
        return Err(invalid_folder(folder, "it does not exist"));
    }
    let Some(text) = read_small(&folder.join(ENGINE_FILE)) else {
        return Err(invalid_folder(folder, &format!("it has no {ENGINE_FILE}")));
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| invalid_folder(folder, &format!("its {ENGINE_FILE} is not JSON")))?;
    if value["format"] != 1 {
        return Err(invalid_folder(
            folder,
            &format!("its {ENGINE_FILE} has another format (this engine reads 1)"),
        ));
    }
    let version = value["version"]
        .as_str()
        .and_then(|text| Version::parse(text).ok())
        .ok_or_else(|| {
            invalid_folder(
                folder,
                &format!("its {ENGINE_FILE} has no valid \"version\""),
            )
        })?;
    if !folder.join(WEN).is_file() {
        return Err(invalid_folder(folder, &format!("it has no {WEN}")));
    }
    Ok(Engine {
        version,
        folder: folder.to_path_buf(),
        source,
    })
}

/// A folder as one key, however it is written (resolved once: through an
/// offline drive every resolution waits).
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn registry_invalid(why: &str) -> Diagnostic {
    Diagnostic::warning(
        "engine_registry_invalid",
        format!("The list of added engines ({REGISTRY_FILE}) {why}; it is read as empty."),
        format!(
            "Adding or removing an engine writes it anew and keeps the old one as {REGISTRY_FILE}.bak; or fix the file."
        ),
    )
}

/// The list of added engine folders, as read.
struct Registry {
    /// Every absolute folder in it.
    folders: Vec<PathBuf>,
    /// Why it could not be read.
    problem: Option<Diagnostic>,
    /// Its format, when that is newer than the one this engine writes.
    newer: Option<u64>,
}

fn read_registry(config: &Path) -> Registry {
    let empty = |problem: Diagnostic, newer: Option<u64>| Registry {
        folders: Vec::new(),
        problem: Some(problem),
        newer,
    };
    let path = config.join(REGISTRY_FILE);
    if !path.exists() {
        return Registry {
            folders: Vec::new(),
            problem: None,
            newer: None,
        };
    }
    let Some(text) = read_small(&path) else {
        return empty(registry_invalid("cannot be read"), None);
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return empty(registry_invalid("is not JSON"), None);
    };
    match value["format"].as_u64() {
        Some(1) => {}
        Some(format) if format > 1 => {
            return empty(
                Diagnostic::warning(
                    "engine_registry_invalid",
                    format!(
                        "The list of added engines ({REGISTRY_FILE}) was written by a newer engine (format {format}); this one does not read it."
                    ),
                    "Add or remove engines from the newest engine's Hub; this one leaves the list alone.",
                ),
                Some(format),
            );
        }
        _ => return empty(registry_invalid("has another format"), None),
    }
    let Some(list) = value["folders"].as_array() else {
        return empty(registry_invalid("has no \"folders\" list"), None);
    };
    Registry {
        folders: list
            .iter()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .collect(),
        problem: None,
        newer: None,
    }
}

/// The folders the person added (`engines.json`), and a warning when the
/// list cannot be read.
pub fn added_folders(config: &Path) -> (Vec<PathBuf>, Option<Diagnostic>) {
    let registry = read_registry(config);
    (registry.folders, registry.problem)
}

/// The list as it may be written again: one a newer engine wrote is never
/// overwritten; a broken one is kept beside it as `engines.json.bak`
/// (not with `dry_run`).
fn writable_registry(config: &Path, dry_run: bool) -> Result<Vec<PathBuf>> {
    let registry = read_registry(config);
    if let Some(format) = registry.newer {
        return Err(Diagnostic::error(
            "engine_registry_invalid",
            format!(
                "The list of added engines ({REGISTRY_FILE}) was written by a newer engine (format {format}); this engine would lose it."
            ),
            "Add or remove engines from the newest engine's Hub (or its `wen engines`).",
        ));
    }
    if registry.problem.is_some() && !dry_run {
        let path = config.join(REGISTRY_FILE);
        std::fs::rename(&path, config.join(format!("{REGISTRY_FILE}.bak"))).map_err(|error| {
            Diagnostic::error(
                "file_unwritable",
                format!("Could not keep the broken list of added engines aside: {error}."),
                "Check that the settings folder can be written to, or fix engines.json by hand.",
            )
        })?;
    }
    Ok(registry.folders)
}

fn write_added(config: &Path, folders: &[PathBuf]) -> Result<()> {
    let list: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
    let text = wish_scene::json::to_canonical(&json!({ "format": 1, "folders": list }))?;
    std::fs::create_dir_all(config)
        .and_then(|()| {
            let part = config.join(format!("{REGISTRY_FILE}.part"));
            std::fs::write(&part, text)?;
            std::fs::rename(&part, config.join(REGISTRY_FILE))
        })
        .map_err(|error| {
            Diagnostic::error(
                "file_unwritable",
                format!("Could not write the list of added engines: {error}."),
                "Check that the settings folder can be written to (WISH_CONFIG_DIR names another).",
            )
        })
}

/// Forgets every listed folder that is `folder`.
fn forget(config: &Path, folder: &Path, dry_run: bool) -> Result<()> {
    let folders = writable_registry(config, dry_run)?;
    if !dry_run {
        let wanted = key(folder);
        let rest: Vec<PathBuf> = folders.into_iter().filter(|f| key(f) != wanted).collect();
        write_added(config, &rest)?;
    }
    Ok(())
}

fn installed_folders(config: &Path) -> Vec<PathBuf> {
    let mut installed: Vec<PathBuf> = std::fs::read_dir(config.join(ENGINES_FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    installed.sort();
    installed
}

/// The engines on this computer, and what is wrong with their list.
pub struct Listing {
    /// Every engine: newest first, the running one first among equals.
    pub engines: Vec<Engine>,
    /// Warnings: a list that cannot be read, installed folders that are
    /// not engines, engines past the limit.
    pub warnings: Vec<Diagnostic>,
    /// Added folders that are no longer engines, and why (removing one
    /// forgets it).
    pub stale: Vec<(PathBuf, Diagnostic)>,
}

impl Listing {
    /// Every warning, the stale folders' too.
    pub fn all_warnings(&self) -> Vec<Diagnostic> {
        let mut all = self.warnings.clone();
        all.extend(self.stale.iter().map(|(_, problem)| problem.clone()));
        all
    }
}

/// Every engine on this computer, with what is wrong with the list.
pub fn listing(config: Option<&Path>) -> Listing {
    let here = running();
    let mut keys = vec![key(&here.folder)];
    let mut found = vec![here];
    let mut warnings = Vec::new();
    let mut stale = Vec::new();
    if let Some(config) = config {
        let registry = read_registry(config);
        warnings.extend(registry.problem);
        let candidates = installed_folders(config)
            .into_iter()
            .map(|folder| (folder, Source::Installed))
            .chain(
                registry
                    .folders
                    .into_iter()
                    .map(|folder| (folder, Source::Added)),
            );
        let mut hidden = 0usize;
        for (folder, source) in candidates {
            let folder_key = key(&folder);
            if keys.contains(&folder_key) {
                continue;
            }
            if found.len() >= MAX_ENGINES {
                hidden += 1;
                continue;
            }
            match read_engine(&folder, source) {
                Ok(engine) => {
                    found.push(engine);
                    keys.push(folder_key);
                }
                Err(problem) if source == Source::Added => stale.push((
                    folder,
                    Diagnostic::warning(
                        problem.code.clone(),
                        problem.message.clone(),
                        "Remove it from the list (Hub, Engine versions) if it is gone for good.",
                    ),
                )),
                Err(problem) => warnings.push(Diagnostic::warning(
                    problem.code.clone(),
                    problem.message.clone(),
                    problem.hint.clone(),
                )),
            }
        }
        if hidden > 0 {
            warnings.push(Diagnostic::warning(
                "engine_too_many",
                format!(
                    "{hidden} more engine folders are past the limit of {MAX_ENGINES} and are not used."
                ),
                "Remove the engines you no longer use.",
            ));
        }
    }
    found.sort_by(|a, b| {
        b.version
            .cmp(&a.version)
            .then((a.source != Source::Running).cmp(&(b.source != Source::Running)))
            .then(a.folder.cmp(&b.folder))
    });
    Listing {
        engines: found,
        warnings,
        stale,
    }
}

/// Every engine on this computer: the running one, the installed ones and
/// the added ones (newest first; the running one first among equals), and
/// warnings about folders that are no longer engines.
pub fn engines(config: Option<&Path>) -> (Vec<Engine>, Vec<Diagnostic>) {
    let listing = listing(config);
    let warnings = listing.all_warnings();
    (listing.engines, warnings)
}

/// Adds the engine in `folder` (absolute) where it is; answers it and
/// whether it was new. Nothing is written with `dry_run`.
pub fn add(config: &Path, folder: &Path, dry_run: bool) -> Result<(Engine, bool)> {
    let engine = read_engine(folder, Source::Added)?;
    let (all, _) = engines(Some(config));
    let wanted = key(folder);
    if let Some(known) = all.iter().find(|e| key(&e.folder) == wanted) {
        return Ok((known.clone(), false));
    }
    let mut folders = writable_registry(config, dry_run)?;
    // Every listed folder counts, also one that is not there now (a drive
    // that is not plugged in).
    if 1 + installed_folders(config).len() + folders.len() >= MAX_ENGINES {
        return Err(Diagnostic::error(
            "engine_too_many",
            format!("There are already {MAX_ENGINES} engines."),
            "Remove the ones you no longer use first (folders that are no longer engines count too).",
        ));
    }
    if !dry_run {
        folders.push(folder.to_path_buf());
        write_added(config, &folders)?;
    }
    Ok((engine, true))
}

/// What removing an engine did.
#[derive(Clone, Debug)]
pub struct Removed {
    /// The engine (version 0.0.0 for a folder that is no longer one).
    pub engine: Engine,
    /// Where an installed engine went (an added one is only forgotten).
    pub trash: Option<PathBuf>,
    /// Whether the folder was no longer an engine (it was only forgotten).
    pub stale: bool,
}

/// Removes the engine in `folder`: an added one is forgotten (its files
/// stay), an installed one goes to `<config>/trash/engines/`. The running
/// engine cannot be removed (but a list entry naming it is forgotten).
pub fn remove(config: &Path, folder: &Path, dry_run: bool) -> Result<Removed> {
    let wanted = key(folder);
    let (all, _) = engines(Some(config));
    let listed = read_registry(config)
        .folders
        .iter()
        .any(|f| key(f) == wanted);
    let Some(engine) = all.iter().find(|e| key(&e.folder) == wanted).cloned() else {
        if listed {
            // A folder that is no longer an engine is still forgotten.
            forget(config, folder, dry_run)?;
            return Ok(Removed {
                engine: Engine {
                    version: Version {
                        major: 0,
                        minor: 0,
                        patch: 0,
                    },
                    folder: folder.to_path_buf(),
                    source: Source::Added,
                },
                trash: None,
                stale: true,
            });
        }
        let known: Vec<String> = all.iter().map(|e| e.folder.display().to_string()).collect();
        return Err(Diagnostic::error(
            "engine_not_found",
            "No engine of this computer is in that folder.",
            format!("The engines are in: {}.", known.join(", ")),
        ));
    };
    match engine.source {
        Source::Running if listed => {
            // The running engine's folder was added too: the entry goes.
            forget(config, folder, dry_run)?;
            Ok(Removed {
                engine,
                trash: None,
                stale: false,
            })
        }
        Source::Running => Err(Diagnostic::error(
            "engine_running",
            format!(
                "The engine {} is the one running; it cannot remove itself.",
                engine.version
            ),
            "Remove it from another engine, or delete its folder yourself after closing it.",
        )),
        Source::Added => {
            forget(config, folder, dry_run)?;
            Ok(Removed {
                engine,
                trash: None,
                stale: false,
            })
        }
        Source::Installed => {
            if listed {
                // Added by hand too: forget that, or it would stay as a
                // folder that is gone.
                forget(config, folder, dry_run)?;
            }
            let trash = config.join("trash").join(ENGINES_FOLDER);
            let name = engine.folder.file_name().map_or_else(
                || engine.version.to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let target = (1..u32::MAX)
                .map(|n| trash.join(format!("{name}-{n}")))
                .find(|path| !path.exists())
                .unwrap_or_else(|| trash.join(name));
            if !dry_run {
                std::fs::create_dir_all(&trash)
                    .and_then(|()| std::fs::rename(&engine.folder, &target))
                    .map_err(|error| {
                        Diagnostic::error(
                            "engine_remove_failed",
                            format!(
                                "Could not move the engine {} to the trash: {error}.",
                                engine.version
                            ),
                            "Close the engine if it is running, then try again.",
                        )
                    })?;
            }
            Ok(Removed {
                engine,
                trash: Some(target),
                stale: false,
            })
        }
    }
}

/// How well an engine fits a project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The version that wrote the project.
    Exact,
    /// A newer version compatible with it (`^version`).
    Compatible,
}

impl Fit {
    /// Its name in JSON.
    pub fn name(self) -> &'static str {
        match self {
            Fit::Exact => "exact",
            Fit::Compatible => "compatible",
        }
    }
}

/// The engine that opens a project written by `wanted`: the exact version
/// (the running engine if it is one), else the newest compatible one;
/// `None` when no engine fits.
pub fn pick(wanted: Version, engines: &[Engine]) -> Option<(Engine, Fit)> {
    let prefer_running = |a: &&Engine, b: &&Engine| {
        a.version
            .cmp(&b.version)
            .then((a.source == Source::Running).cmp(&(b.source == Source::Running)))
            .then(b.folder.cmp(&a.folder))
    };
    if let Some(exact) = engines
        .iter()
        .filter(|e| e.version == wanted)
        .max_by(prefer_running)
    {
        return Some((exact.clone(), Fit::Exact));
    }
    let compatible = VersionReq::parse(&format!("^{wanted}")).ok()?;
    engines
        .iter()
        .filter(|e| compatible.matches(e.version))
        .max_by(prefer_running)
        .map(|e| (e.clone(), Fit::Compatible))
}

/// The engine version a project was written by (`wish.json` `engine`).
pub fn project_version(root: &Path) -> Option<Version> {
    let text = read_small(&root.join(crate::PROJECT_FILE))?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Version::parse(value["engine"].as_str()?).ok()
}

/// Which engine opens the project in `root`.
#[derive(Clone, Debug)]
pub struct Choice {
    /// The version that wrote the project (`None`: `wish.json` names none
    /// that can be read).
    pub project: Option<Version>,
    /// The engine that fits it, and how.
    pub picked: Option<(Engine, Fit)>,
}

impl Choice {
    /// Another engine than the running one should open the project.
    pub fn switch(&self) -> Option<&Engine> {
        self.picked
            .as_ref()
            .map(|(engine, _)| engine)
            .filter(|engine| engine.source != Source::Running)
    }

    /// As JSON (for `pick` and `list`).
    pub fn to_value(&self) -> Map<String, Value> {
        let mut out = Map::new();
        out.insert(
            "project_engine".to_owned(),
            json!(self.project.map(|v| v.to_string())),
        );
        match &self.picked {
            Some((engine, fit)) => {
                out.insert("opens_with".to_owned(), engine.to_value());
                out.insert("fit".to_owned(), json!(fit.name()));
            }
            None => {
                out.insert("opens_with".to_owned(), running().to_value());
                out.insert("fit".to_owned(), json!("none"));
            }
        }
        out.insert("switch".to_owned(), json!(self.switch().is_some()));
        out
    }

    /// A line for the person when the project opens with an engine that
    /// does not fit it (`None` when it fits).
    pub fn note(&self) -> Option<String> {
        let project = self.project?;
        if self.picked.is_some() {
            return None;
        }
        let here = Version::engine();
        Some(if project > here {
            format!(
                "This project was made with Wish Engine {project}, newer than this one ({here}): install {project} or newer and open it there."
            )
        } else {
            format!(
                "This project was made with Wish Engine {project}; no engine compatible with it is installed, so {here} opens it. Its files may change."
            )
        })
    }
}

/// Which engine opens the project in `root`, among this computer's engines.
pub fn choose(root: &Path, config: Option<&Path>) -> Choice {
    let project = project_version(root);
    let picked = project.and_then(|wanted| pick(wanted, &engines(config).0));
    Choice { project, picked }
}
