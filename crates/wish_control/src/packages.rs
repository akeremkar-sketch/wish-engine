//! Packages (M10a; design note M10 §4, §12): a folder with a
//! `wish-package.json` manifest that brings themes, workspaces, languages,
//! recipes or templates. Packages live in two scopes: the project's own
//! (`packages/<id>/`, recorded in `wish.json` and `wish.lock`, so a project
//! opens the same way anywhere) and the person's (`<config>/packages/<id>/`).
//!
//! Installing copies into a side folder, checks it there and only then
//! moves it into place; removing moves a package to a trash folder, never
//! deletes it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};

/// A package's manifest file, at its root.
pub const MANIFEST: &str = "wish-package.json";
/// The project's lock file: the exact packages it uses.
pub const LOCK_FILE: &str = "wish.lock";
/// The project's package folder.
pub const PROJECT_FOLDER: &str = "packages";
/// The manifest format this engine reads and writes.
pub const MANIFEST_FORMAT: u64 = 1;
/// What a package can bring, and the file each item is.
pub const KINDS: &[&str] = &["languages", "recipes", "templates", "themes", "workspaces"];
/// The prefix of the engine's own packages.
pub const ENGINE_PREFIX: &str = "wish.";
/// Most files in a package.
pub const MAX_FILES: usize = 4096;
/// Most bytes in a package.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Licenses a package may have without a warning (SPDX ids).
const KNOWN_LICENSES: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC-BY-4.0",
    "CC0-1.0",
    "ISC",
    "MIT",
    "MIT-0",
    "OFL-1.1",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
];

const MANIFEST_KEYS: &[&str] = &[
    "contents",
    "description",
    "engine",
    "format",
    "id",
    "license",
    "name",
    "version",
];

/// Where a package is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Inside the project (`packages/`), part of it.
    Project,
    /// The person's own (`<config>/packages/`), for every project.
    User,
}

impl Scope {
    /// Its name in tool arguments and answers.
    pub fn name(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

/// A version: `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Incompatible changes.
    pub major: u64,
    /// Compatible additions.
    pub minor: u64,
    /// Fixes.
    pub patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn invalid_version(text: &str) -> Diagnostic {
    Diagnostic::error(
        "invalid_version",
        format!("'{text}' is not a version."),
        "A version is three numbers: major.minor.patch, such as \"1.2.0\".",
    )
}

impl Version {
    /// Parses `major.minor.patch`.
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<&str> = text.trim().split('.').collect();
        let number = |part: &str| -> Option<u64> {
            let ok = !part.is_empty()
                && part.len() <= 9
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            ok.then(|| part.parse().ok()).flatten()
        };
        match parts.as_slice() {
            [major, minor, patch] => Ok(Self {
                major: number(major).ok_or_else(|| invalid_version(text))?,
                minor: number(minor).ok_or_else(|| invalid_version(text))?,
                patch: number(patch).ok_or_else(|| invalid_version(text))?,
            }),
            _ => Err(invalid_version(text)),
        }
    }

    /// This engine's version.
    pub fn engine() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self {
            major: 0,
            minor: 0,
            patch: 0,
        })
    }
}

/// How a requirement compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    /// Compatible: the same left-most non-zero part.
    Caret,
    /// The same major and minor.
    Tilde,
}

/// A version requirement: comparators joined with commas, all of which
/// must hold (`>=0.9, <2.0`). A bare version means `^`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionReq {
    text: String,
    comparators: Vec<(Op, Version)>,
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl VersionReq {
    /// Parses a requirement. Versions in it may leave out the minor and
    /// patch parts (`>=1`, `^0.9`).
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = || {
            Diagnostic::error(
                "invalid_version_requirement",
                format!("'{text}' is not a version requirement."),
                "Write comparators joined with commas, such as \">=0.9, <2.0\" or \"^1.2\"; the operators are >=, >, <=, <, =, ^ and ~.",
            )
        };
        let mut comparators = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            let (op, rest) = [
                (">=", Op::GreaterEq),
                ("<=", Op::LessEq),
                (">", Op::Greater),
                ("<", Op::Less),
                ("=", Op::Exact),
                ("^", Op::Caret),
                ("~", Op::Tilde),
            ]
            .iter()
            .find_map(|(prefix, op)| part.strip_prefix(prefix).map(|rest| (*op, rest)))
            .unwrap_or((Op::Caret, part));
            let rest = rest.trim();
            let filled = match rest.split('.').count() {
                1 => format!("{rest}.0.0"),
                2 => format!("{rest}.0"),
                _ => rest.to_owned(),
            };
            let version = Version::parse(&filled).map_err(|_| invalid())?;
            comparators.push((op, version));
        }
        if comparators.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            text: text.trim().to_owned(),
            comparators,
        })
    }

    /// Whether `version` meets every comparator.
    pub fn matches(&self, version: Version) -> bool {
        self.comparators.iter().all(|(op, want)| match op {
            Op::Exact => version == *want,
            Op::Greater => version > *want,
            Op::GreaterEq => version >= *want,
            Op::Less => version < *want,
            Op::LessEq => version <= *want,
            Op::Tilde => {
                version >= *want && version.major == want.major && version.minor == want.minor
            }
            Op::Caret => {
                version >= *want
                    && if want.major > 0 {
                        version.major == want.major
                    } else if want.minor > 0 {
                        version.major == 0 && version.minor == want.minor
                    } else {
                        version.major == 0 && version.minor == 0 && version.patch == want.patch
                    }
            }
        })
    }
}

/// Whether `id` is a package id: lowercase dot-separated parts of
/// `a-z 0-9 -`, at least two, 3 to 64 characters.
pub fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    (3..=64).contains(&id.len())
        && parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// A package's manifest, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Its id (`com.example.dusk`).
    pub id: String,
    /// Its name, for people.
    pub name: String,
    /// Its version.
    pub version: Version,
    /// The engine versions it works with.
    pub engine: VersionReq,
    /// Its license (SPDX).
    pub license: String,
    /// What it is, in a sentence.
    pub description: String,
    /// What it brings: kind → package-relative paths.
    pub contents: BTreeMap<String, Vec<String>>,
}

impl Manifest {
    /// Its JSON form (as `list` and `check` answer).
    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "version": self.version.to_string(),
            "engine": self.engine.to_string(),
            "license": self.license,
            "description": self.description,
            "contents": self.contents,
        })
    }

    /// Whether its license is one of the known permissive ones (every
    /// SPDX id in it, with `OR` / `AND` / parentheses).
    pub fn license_known(&self) -> bool {
        let ids: Vec<&str> = self
            .license
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .filter(|word| !word.is_empty() && *word != "OR" && *word != "AND")
            .collect();
        !ids.is_empty() && ids.iter().all(|id| KNOWN_LICENSES.contains(id))
    }
}

fn manifest_problem(pointer: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("package_invalid", message, hint)
        .at_file(MANIFEST)
        .at_path(pointer)
}

/// Whether `path` is a package-relative path that stays inside it: forward
/// slashes, no `..`, no hidden parts, no drive or root.
fn inside_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && !part.starts_with('.'))
}

/// Reads and checks the manifest in `folder` (and that what it lists is
/// there and of the right kind).
pub fn read_manifest(folder: &Path) -> Result<Manifest> {
    let path = folder.join(MANIFEST);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "package_not_found",
            format!("That folder has no {MANIFEST} ({error})."),
            "A package is a folder with a wish-package.json at its root: { \"format\": 1, \"id\": \"com.example.dusk\", \"name\": \"Dusk\", \"version\": \"1.0.0\", \"engine\": \">=0.1\", \"license\": \"MIT\", \"contents\": { \"themes\": [\"dusk.json\"] } }.",
        )
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        manifest_problem(
            "",
            format!("{MANIFEST} is not valid JSON: {error}."),
            "Fix the JSON (a missing comma or quote, usually).",
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err(manifest_problem(
            "",
            format!("{MANIFEST} must be a JSON object."),
            "Start it with { \"format\": 1, \"id\": \"...\" }.",
        ));
    };
    for key in object.keys() {
        if !MANIFEST_KEYS.contains(&key.as_str()) {
            return Err(manifest_problem(
                &format!("/{key}"),
                format!("{MANIFEST} has no setting '{key}'."),
                &wish_core::suggest::unknown_name_hint(key, MANIFEST_KEYS),
            ));
        }
    }
    if object.get("format").and_then(Value::as_u64) != Some(MANIFEST_FORMAT) {
        return Err(manifest_problem(
            "/format",
            format!("{MANIFEST} must say \"format\": {MANIFEST_FORMAT}."),
            "This engine reads package manifests of format 1; a newer package needs a newer engine.",
        ));
    }
    let text_of = |key: &str, hint: &str, max: usize| -> Result<String> {
        match object.get(key) {
            Some(Value::String(text)) if !text.trim().is_empty() && text.chars().count() <= max => {
                Ok(text.trim().to_owned())
            }
            _ => Err(manifest_problem(
                &format!("/{key}"),
                format!("{MANIFEST} needs \"{key}\": a text of 1 to {max} characters."),
                hint,
            )),
        }
    };
    let id = text_of("id", "Write it like \"id\": \"com.example.dusk\".", 64)?;
    if !valid_id(&id) {
        return Err(manifest_problem(
            "/id",
            format!("'{id}' is not a package id."),
            "An id is lowercase parts of a-z, 0-9 and -, joined with dots, at least two, such as \"com.example.dusk\".",
        ));
    }
    let name = text_of("name", "Write it like \"name\": \"Dusk\".", 60)?;
    let version = Version::parse(&text_of(
        "version",
        "Write it like \"version\": \"1.0.0\".",
        32,
    )?)
    .map_err(|problem| problem.at_file(MANIFEST).at_path("/version"))?;
    let engine = VersionReq::parse(&text_of(
        "engine",
        "Write the engine versions it works with, like \"engine\": \">=0.1, <2.0\".",
        64,
    )?)
    .map_err(|problem| problem.at_file(MANIFEST).at_path("/engine"))?;
    let license = text_of(
        "license",
        "Write its SPDX license, like \"license\": \"MIT\".",
        120,
    )?;
    let description = match object.get("description") {
        None => String::new(),
        Some(Value::String(text)) if text.chars().count() <= 400 => text.trim().to_owned(),
        Some(_) => {
            return Err(manifest_problem(
                "/description",
                "\"description\" must be a text of at most 400 characters.".to_owned(),
                "Say what the package is in a sentence.",
            ));
        }
    };
    let Some(Value::Object(listed)) = object.get("contents") else {
        return Err(manifest_problem(
            "/contents",
            format!("{MANIFEST} needs \"contents\": what the package brings."),
            "Write it like \"contents\": { \"themes\": [\"dusk.json\"] }; the kinds are languages, recipes, templates, themes and workspaces.",
        ));
    };
    let mut contents = BTreeMap::new();
    for (kind, items) in listed {
        let pointer = format!("/contents/{kind}");
        if !KINDS.contains(&kind.as_str()) {
            return Err(manifest_problem(
                &pointer,
                format!("A package cannot bring '{kind}'."),
                &wish_core::suggest::unknown_name_hint(kind, KINDS),
            ));
        }
        let Some(items) = items.as_array() else {
            return Err(manifest_problem(
                &pointer,
                format!("'{kind}' must be a list of paths in the package."),
                "Write it like [\"dusk.json\"].",
            ));
        };
        let mut paths = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let item_pointer = format!("{pointer}/{index}");
            let Some(item) = item.as_str().filter(|item| inside_path(item)) else {
                return Err(manifest_problem(
                    &item_pointer,
                    format!("{item} is not a path inside the package."),
                    "Write a path relative to the package's folder, with forward slashes, no '..' and no hidden folders.",
                ));
            };
            let target = folder.join(item);
            let fits = match kind.as_str() {
                "templates" => target.join(crate::project::PROJECT_FILE).is_file(),
                "recipes" => target.is_file() && item.ends_with(".md"),
                _ => target.is_file() && item.ends_with(".json"),
            };
            if !fits {
                let what = match kind.as_str() {
                    "templates" => "a folder with a wish.json (a project)",
                    "recipes" => "a .md file",
                    _ => "a .json file",
                };
                return Err(manifest_problem(
                    &item_pointer,
                    format!("'{item}' is listed under {kind} but is not {what} in the package."),
                    "Check the path and the file's kind.",
                ));
            }
            paths.push(item.to_owned());
        }
        contents.insert(kind.clone(), paths);
    }
    Ok(Manifest {
        id,
        name,
        version,
        engine,
        license,
        description,
        contents,
    })
}

/// The files of the package in `folder`, package-relative with forward
/// slashes, sorted; hidden files and folders are left out. Links and
/// packages over the limits are refused.
pub fn package_files(folder: &Path) -> Result<Vec<String>> {
    fn walk(root: &Path, folder: &Path, out: &mut Vec<String>, bytes: &mut u64) -> Result<()> {
        let entries = std::fs::read_dir(folder).map_err(|error| {
            Diagnostic::error(
                "package_unreadable",
                format!("'{}' could not be read: {error}.", folder.display()),
                "Check that the folder exists and can be read.",
            )
        })?;
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Hidden files stay out, but a template's picture (.template/) is part of it.
            if name.starts_with('.') && name != ".template" {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| {
                Diagnostic::error(
                    "package_unreadable",
                    format!("'{}' could not be read: {error}.", path.display()),
                    "Check that the folder can be read.",
                )
            })?;
            if kind.is_symlink() {
                return Err(Diagnostic::error(
                    "package_invalid",
                    format!("'{}' is a link.", path.display()),
                    "A package holds only real files and folders (a link could lead outside it).",
                ));
            }
            if kind.is_dir() {
                walk(root, &path, out, bytes)?;
            } else {
                *bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(relative);
                if out.len() > MAX_FILES || *bytes > MAX_BYTES {
                    return Err(Diagnostic::error(
                        "package_too_large",
                        format!(
                            "'{}' has more than {MAX_FILES} files or {} MiB.",
                            root.display(),
                            MAX_BYTES / (1024 * 1024)
                        ),
                        "Keep a package to what it brings; big assets belong in the project.",
                    ));
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let mut bytes = 0;
    walk(folder, folder, &mut out, &mut bytes)?;
    out.sort();
    Ok(out)
}

/// The content hash of the package in `folder`: its sorted paths and the
/// bytes of each (32 hex digits).
pub fn content_hash(folder: &Path) -> Result<String> {
    let mut hasher = wish_core::StableHasher::new();
    hasher.write_str("wish-package-v1");
    for file in package_files(folder)? {
        let bytes = std::fs::read(folder.join(&file)).map_err(|error| {
            Diagnostic::error(
                "package_unreadable",
                format!("'{file}' could not be read: {error}."),
                "Check that the package's files can be read.",
            )
        })?;
        hasher.write_str(&file);
        hasher.write_len_prefixed(&bytes);
    }
    Ok(hasher.finish().to_string())
}

/// The folder of `scope`'s packages (the user's needs a settings folder).
pub fn scope_folder(scope: Scope, project: &Path) -> Option<PathBuf> {
    match scope {
        Scope::Project => Some(project.join(PROJECT_FOLDER)),
        Scope::User => crate::crash::config_folder().map(|folder| folder.join("packages")),
    }
}

/// The engine's own packages (`wish.*`): `packages` beside the program
/// (an installed engine), else the repository's (engine development), or
/// `WISH_ENGINE_PACKAGES`.
pub fn engine_packages_folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WISH_ENGINE_PACKAGES").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("packages")));
    if let Some(dir) = beside.filter(|dir| dir.is_dir()) {
        return Some(dir);
    }
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages");
    repository.is_dir().then_some(repository)
}

/// The folder removed packages go to.
fn trash_folder(scope: Scope, project: &Path) -> Option<PathBuf> {
    match scope {
        Scope::Project => Some(project.join(".wish").join("trash").join("packages")),
        Scope::User => {
            crate::crash::config_folder().map(|folder| folder.join("trash").join("packages"))
        }
    }
}

/// An installed package.
#[derive(Clone, Debug)]
pub struct Installed {
    /// Its manifest (`None` when its manifest is broken: `problem` says why).
    pub manifest: Option<Manifest>,
    /// Its folder's name (its id, when well made).
    pub id: String,
    /// Its folder.
    pub folder: PathBuf,
    /// Where it is installed.
    pub scope: Scope,
    /// Its content hash.
    pub hash: Option<String>,
    /// What is wrong with it, if anything.
    pub problem: Option<Diagnostic>,
}

/// The packages installed in `scope`, by id.
pub fn installed(scope: Scope, project: &Path) -> Vec<Installed> {
    let Some(folder) = scope_folder(scope, project) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut found: Vec<Installed> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let folder = entry.path();
            let read = read_manifest(&folder).and_then(|manifest| {
                if manifest.id == id {
                    Ok(manifest)
                } else {
                    Err(Diagnostic::error(
                        "package_invalid",
                        format!("The package in '{id}/' says its id is '{}'.", manifest.id),
                        "A package's folder is named by its id; install it again with the packages tool.",
                    ))
                }
            });
            let hash = content_hash(&folder).ok();
            match read {
                Ok(manifest) => Installed {
                    manifest: Some(manifest),
                    id,
                    folder,
                    scope,
                    hash,
                    problem: None,
                },
                Err(problem) => Installed {
                    manifest: None,
                    id,
                    folder,
                    scope,
                    hash,
                    problem: Some(problem),
                },
            }
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// What `wish.lock` records for a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locked {
    /// Its id.
    pub id: String,
    /// Its version.
    pub version: String,
    /// Its content hash.
    pub hash: String,
}

/// Reads the project's `wish.lock` (empty when there is none).
pub fn read_lock(project: &Path) -> Result<Vec<Locked>> {
    let Ok(text) = std::fs::read_to_string(project.join(LOCK_FILE)) else {
        return Ok(Vec::new());
    };
    let invalid = |why: &str| {
        Diagnostic::error(
            "lock_file_invalid",
            format!("{LOCK_FILE} is not valid: {why}."),
            "Write it again from the installed packages: packages with action lock.",
        )
        .at_file(LOCK_FILE)
    };
    let value: Value = serde_json::from_str(&text).map_err(|error| invalid(&error.to_string()))?;
    if value["format"] != 1 {
        return Err(invalid("it needs \"format\": 1"));
    }
    let mut locked = Vec::new();
    for entry in value["packages"].as_array().into_iter().flatten() {
        match (
            entry["id"].as_str(),
            entry["version"].as_str(),
            entry["hash"].as_str(),
        ) {
            (Some(id), Some(version), Some(hash)) => locked.push(Locked {
                id: id.to_owned(),
                version: version.to_owned(),
                hash: hash.to_owned(),
            }),
            _ => return Err(invalid("every package needs an id, a version and a hash")),
        }
    }
    Ok(locked)
}

/// The text of `wish.lock` for the project's installed packages.
pub fn lock_text(project: &Path) -> Result<String> {
    let entries: Vec<Value> = installed(Scope::Project, project)
        .iter()
        .filter_map(|package| {
            let manifest = package.manifest.as_ref()?;
            Some(json!({
                "id": manifest.id,
                "version": manifest.version.to_string(),
                "hash": package.hash.clone()?,
            }))
        })
        .collect();
    wish_scene::json::to_canonical(&json!({ "format": 1, "packages": entries }))
}

/// Copies the package files of `from` into `to` (which must not exist).
fn copy_package(from: &Path, to: &Path) -> Result<()> {
    let failed = |what: &str, error: std::io::Error| {
        Diagnostic::error(
            "package_install_failed",
            format!("Could not {what}: {error}."),
            "Check that the package folder can be written to.",
        )
    };
    for file in package_files(from)? {
        let target = to.join(&file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| failed("create a folder", e))?;
        }
        std::fs::copy(from.join(&file), &target).map_err(|e| failed("copy a file", e))?;
    }
    Ok(())
}

/// Moves `folder` into the trash of `scope`; returns where it went.
fn to_trash(scope: Scope, project: &Path, folder: &Path, id: &str) -> Result<PathBuf> {
    let failed = |error: std::io::Error| {
        Diagnostic::error(
            "package_remove_failed",
            format!("Could not move the package '{id}' to the trash: {error}."),
            "Close programs that use its files, then try again.",
        )
    };
    let trash = trash_folder(scope, project).ok_or_else(|| {
        Diagnostic::error(
            "package_remove_failed",
            "There is no settings folder for the trash.",
            "Set WISH_CONFIG_DIR to a folder the engine may write to.",
        )
    })?;
    std::fs::create_dir_all(&trash).map_err(failed)?;
    let target = (1..u32::MAX)
        .map(|n| trash.join(format!("{id}-{n}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| trash.join(id));
    std::fs::rename(folder, &target).map_err(failed)?;
    Ok(target)
}

/// What an install would do or did.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The package.
    pub manifest: Manifest,
    /// Where it goes.
    pub folder: PathBuf,
    /// The version it replaces, if any.
    pub replaces: Option<String>,
    /// Warnings (an unknown license).
    pub warnings: Vec<Diagnostic>,
}

/// Checks the package in `from` for installing into `scope`: its manifest,
/// the engine version, what is there already.
pub fn plan_install(from: &Path, scope: Scope, project: &Path, replace: bool) -> Result<Plan> {
    let manifest = read_manifest(from)?;
    package_files(from)?;
    let engine = Version::engine();
    if !manifest.engine.matches(engine) {
        return Err(Diagnostic::error(
            "package_engine_mismatch",
            format!(
                "'{}' {} works with engine {}, and this engine is {engine}.",
                manifest.id, manifest.version, manifest.engine
            ),
            "Use a version of the package made for this engine, or this engine's version the package names.",
        ));
    }
    if manifest.id.starts_with(ENGINE_PREFIX)
        && !engine_packages_folder().is_some_and(|engine| from.starts_with(engine))
    {
        return Err(Diagnostic::error(
            "package_invalid",
            format!(
                "'{}' uses the engine's own prefix '{ENGINE_PREFIX}'.",
                manifest.id
            ),
            "Name your package after yourself, such as com.yourname.dusk.",
        ));
    }
    let folder = scope_folder(scope, project)
        .ok_or_else(|| {
            Diagnostic::error(
                "package_install_failed",
                "There is no settings folder for the person's packages.",
                "Set WISH_CONFIG_DIR to a folder the engine may write to, or install into the project.",
            )
        })?
        .join(&manifest.id);
    let replaces = if folder.exists() {
        let had = read_manifest(&folder)
            .map(|old| old.version.to_string())
            .unwrap_or_else(|_| "?".to_owned());
        if !replace {
            return Err(Diagnostic::error(
                "package_exists",
                format!("'{}' {had} is installed already.", manifest.id),
                "Pass \"replace\": true to replace it (the old one goes to the trash).",
            ));
        }
        Some(had)
    } else {
        None
    };
    let mut warnings = Vec::new();
    if !manifest.license_known() {
        warnings.push(Diagnostic::warning(
            "package_license_unknown",
            format!(
                "'{}' has the license '{}', which is not one of the known permissive ones.",
                manifest.id, manifest.license
            ),
            "Read the package's license before you share a game made with it.",
        ));
    }
    Ok(Plan {
        manifest,
        folder,
        replaces,
        warnings,
    })
}

/// Installs the package in `from` as `plan` says: copied beside its place,
/// checked there, then moved into place (the old one to the trash).
pub fn install(from: &Path, plan: &Plan, scope: Scope, project: &Path) -> Result<()> {
    let parent = plan.folder.parent().unwrap_or(project).to_path_buf();
    let failed = |what: &str, error: std::io::Error| {
        Diagnostic::error(
            "package_install_failed",
            format!("Could not {what}: {error}."),
            "Check that the package folder can be written to.",
        )
    };
    std::fs::create_dir_all(&parent).map_err(|e| failed("create the package folder", e))?;
    let side = parent.join(format!(".{}.installing", plan.manifest.id));
    if side.exists() {
        std::fs::remove_dir_all(&side).map_err(|e| failed("clear an earlier install", e))?;
    }
    let copied = copy_package(from, &side).and_then(|()| {
        let manifest = read_manifest(&side)?;
        if manifest == plan.manifest {
            Ok(())
        } else {
            Err(Diagnostic::error(
                "package_install_failed",
                "The package changed while it was being copied.",
                "Install it again.",
            ))
        }
    });
    if let Err(problem) = copied {
        let _ = std::fs::remove_dir_all(&side);
        return Err(problem);
    }
    if plan.folder.exists() {
        to_trash(scope, project, &plan.folder, &plan.manifest.id)?;
    }
    std::fs::rename(&side, &plan.folder).map_err(|e| failed("move the package into place", e))?;
    Ok(())
}

/// Removes the installed package `id` of `scope` (to the trash); returns
/// where it went.
pub fn remove(id: &str, scope: Scope, project: &Path) -> Result<PathBuf> {
    let folder = scope_folder(scope, project)
        .map(|folder| folder.join(id))
        .filter(|folder| valid_id(id) && folder.is_dir())
        .ok_or_else(|| {
            let known: Vec<String> = installed(scope, project)
                .into_iter()
                .map(|p| p.id)
                .collect();
            let known: Vec<&str> = known.iter().map(String::as_str).collect();
            Diagnostic::error(
                "package_not_found",
                format!(
                    "No package '{id}' is installed in the {} scope.",
                    scope.name()
                ),
                wish_core::suggest::unknown_name_hint(id, &known),
            )
        })?;
    to_trash(scope, project, &folder, id)
}

/// The installed packages of a scope as JSON (for `list`), with the lock's
/// view for the project's.
pub fn listing(scope: Scope, project: &Path, locked: &[Locked]) -> (Vec<Value>, Vec<Diagnostic>) {
    let mut warnings = Vec::new();
    let packages = installed(scope, project)
        .into_iter()
        .map(|package| {
            let mut entry = match &package.manifest {
                Some(manifest) => manifest.to_value(),
                None => json!({ "id": package.id }),
            };
            entry["scope"] = json!(scope.name());
            entry["hash"] = json!(package.hash);
            if let Some(problem) = &package.problem {
                entry["problem"] = json!(problem.message);
                warnings.push(problem.clone());
            }
            if scope == Scope::Project {
                match locked.iter().find(|lock| lock.id == package.id) {
                    Some(lock) if Some(&lock.hash) == package.hash.as_ref() => {
                        entry["locked"] = json!(true);
                    }
                    Some(_) => {
                        entry["locked"] = json!(false);
                        warnings.push(Diagnostic::warning(
                            "package_lock_mismatch",
                            format!("The files of '{}' are not the ones {LOCK_FILE} records.", package.id),
                            "If the change is yours, write the lock again (packages with action lock); else install the package again.",
                        ));
                    }
                    None => {
                        entry["locked"] = json!(false);
                        warnings.push(Diagnostic::warning(
                            "package_lock_mismatch",
                            format!("'{}' is installed but not in {LOCK_FILE}.", package.id),
                            "Write the lock again: packages with action lock.",
                        ));
                    }
                }
            }
            entry
        })
        .collect();
    if scope == Scope::Project {
        for lock in locked {
            if !project.join(PROJECT_FOLDER).join(&lock.id).is_dir() {
                warnings.push(Diagnostic::warning(
                    "package_lock_mismatch",
                    format!(
                        "{LOCK_FILE} records '{}' {}, which is not installed.",
                        lock.id, lock.version
                    ),
                    "Install it, or write the lock again: packages with action lock.",
                ));
            }
        }
    }
    (packages, warnings)
}

/// The project's `packages` map for `wish.json`: every installed project
/// package at a compatible requirement of its version.
pub fn requirements(project: &Path) -> BTreeMap<String, String> {
    installed(Scope::Project, project)
        .into_iter()
        .filter_map(|package| {
            let manifest = package.manifest?;
            Some((manifest.id, format!("^{}", manifest.version)))
        })
        .collect()
}

/// A manifest as `check` answers it, with its files.
pub fn describe(folder: &Path) -> Result<Map<String, Value>> {
    let manifest = read_manifest(folder)?;
    let files = package_files(folder)?;
    let mut out = Map::new();
    out.insert("package".to_owned(), manifest.to_value());
    out.insert("files".to_owned(), json!(files.len()));
    out.insert("hash".to_owned(), json!(content_hash(folder)?));
    out.insert("license_known".to_owned(), json!(manifest.license_known()));
    out.insert(
        "engine_ok".to_owned(),
        json!(manifest.engine.matches(Version::engine())),
    );
    Ok(out)
}
