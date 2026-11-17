//! The package index (M10b; design note M10 §14): where packages are found
//! by name, offline first. An index is a folder: either it has an
//! `index.json` naming each package's versions and their folders, or every
//! package folder in it (`<folder>/` or `<id>/<version>/`) is an entry.
//!
//! The online index (later) only adds fetching the index and a package into
//! a cache folder; choosing a version, checking and installing stay here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};

use crate::packages::{self, Scope, Version, VersionReq};

/// An index's own file.
pub const INDEX_FILE: &str = "index.json";
/// The environment variable naming the index folder.
pub const INDEX_ENV: &str = "WISH_PACKAGE_INDEX";
/// The largest `index.json` read.
pub const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
/// The most packages an index may list.
pub const MAX_PACKAGES: usize = 10_000;
/// The most versions of one package.
pub const MAX_VERSIONS: usize = 1_000;
/// The most results `search` gives.
pub const MAX_RESULTS: usize = 200;

/// One version of a package in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexVersion {
    /// Its version.
    pub version: Version,
    /// The engine versions it works with.
    pub engine: VersionReq,
    /// Its content hash, when the index gives one (checked on install).
    pub hash: Option<String>,
    /// Its folder.
    pub folder: PathBuf,
}

/// A package in the index, its versions newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// Its id.
    pub id: String,
    /// Its name, for people.
    pub name: String,
    /// What it is, in a sentence.
    pub description: String,
    /// Its license (SPDX).
    pub license: String,
    /// The kinds of content it brings.
    pub kinds: Vec<String>,
    /// Its versions, newest first.
    pub versions: Vec<IndexVersion>,
}

impl IndexEntry {
    /// The newest version that works with this engine and meets `wanted`.
    pub fn choose(&self, wanted: Option<&VersionReq>) -> Option<&IndexVersion> {
        let engine = Version::engine();
        self.versions.iter().find(|candidate| {
            candidate.engine.matches(engine)
                && wanted.is_none_or(|wanted| wanted.matches(candidate.version))
        })
    }
}

/// An index, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    /// Its folder.
    pub folder: PathBuf,
    /// Its packages, by id.
    pub entries: Vec<IndexEntry>,
}

impl Index {
    /// The package `id`.
    pub fn get(&self, id: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The package `id`, or a diagnostic naming the close ones.
    pub fn require(&self, id: &str) -> Result<&IndexEntry> {
        self.get(id).ok_or_else(|| {
            let known: Vec<&str> = self.entries.iter().map(|e| e.id.as_str()).collect();
            Diagnostic::error(
                "package_not_found",
                format!("The package index has no '{id}'."),
                wish_core::suggest::unknown_name_hint(id, &known),
            )
        })
    }
}

/// Where the index is: `given`, else `WISH_PACKAGE_INDEX`, else
/// `<config>/index/`.
pub fn index_folder(given: Option<PathBuf>) -> Result<PathBuf> {
    let found = given
        .or_else(|| {
            std::env::var_os(INDEX_ENV)
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            crate::crash::config_folder()
                .map(|folder| folder.join("index"))
                .filter(|folder| folder.is_dir())
        });
    found.ok_or_else(|| {
        Diagnostic::error(
            "package_index_missing",
            "There is no package index to search.",
            format!(
                "Name a folder of packages: the \"index\" parameter, or the {INDEX_ENV} environment variable; a folder of package folders is an index already."
            ),
        )
    })
}

fn invalid(pointer: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("package_index_invalid", message, hint)
        .at_file(INDEX_FILE)
        .at_path(pointer)
}

/// Reads the index in `folder`.
pub fn read_index(folder: &Path) -> Result<Index> {
    if !folder.is_dir() {
        return Err(Diagnostic::error(
            "package_index_missing",
            "The package index folder does not exist.",
            "Check the folder's name, or name another one with the \"index\" parameter.",
        ));
    }
    let file = folder.join(INDEX_FILE);
    let mut entries = if file.is_file() {
        read_index_file(folder, &file)?
    } else {
        scan(folder)
    };
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Index {
        folder: folder.to_path_buf(),
        entries,
    })
}

/// Every package folder in `folder` (or one level deeper) as entries.
fn scan(folder: &Path) -> Vec<IndexEntry> {
    let mut found: BTreeMap<String, Vec<(packages::Manifest, PathBuf)>> = BTreeMap::new();
    let children = |dir: &Path| -> Vec<PathBuf> {
        let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .map(|entry| entry.path())
            .collect();
        list.sort();
        list
    };
    let mut add = |dir: PathBuf| {
        if let Ok(manifest) = packages::read_manifest(&dir) {
            found
                .entry(manifest.id.clone())
                .or_default()
                .push((manifest, dir));
        }
    };
    for child in children(folder) {
        if child.join(packages::MANIFEST).is_file() {
            add(child);
        } else {
            for grandchild in children(&child) {
                add(grandchild);
            }
        }
    }
    found
        .into_iter()
        .take(MAX_PACKAGES)
        .map(|(id, mut versions)| {
            versions.sort_by_key(|v| std::cmp::Reverse(v.0.version));
            versions.dedup_by(|a, b| a.0.version == b.0.version);
            versions.truncate(MAX_VERSIONS);
            let newest = &versions[0].0;
            IndexEntry {
                id,
                name: newest.name.clone(),
                description: newest.description.clone(),
                license: newest.license.clone(),
                kinds: newest.contents.keys().cloned().collect(),
                versions: versions
                    .iter()
                    .map(|(manifest, dir)| IndexVersion {
                        version: manifest.version,
                        engine: manifest.engine.clone(),
                        hash: None,
                        folder: dir.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn read_index_file(folder: &Path, file: &Path) -> Result<Vec<IndexEntry>> {
    let unreadable = |error: String| {
        Diagnostic::error(
            "package_index_invalid",
            format!("{INDEX_FILE} could not be read: {error}."),
            "Check that the index folder can be read.",
        )
        .at_file(INDEX_FILE)
    };
    let size = std::fs::metadata(file)
        .map_err(|e| unreadable(e.to_string()))?
        .len();
    if size > MAX_INDEX_BYTES {
        return Err(invalid(
            "",
            format!(
                "{INDEX_FILE} is larger than {} MiB.",
                MAX_INDEX_BYTES / (1024 * 1024)
            ),
            "An index lists packages; it does not hold them. Split it, or check that it is the right file.",
        ));
    }
    let text = std::fs::read_to_string(file).map_err(|e| unreadable(e.to_string()))?;
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        invalid(
            "",
            format!("{INDEX_FILE} is not valid JSON: {error}."),
            "Fix the JSON (a missing comma or quote, usually).",
        )
    })?;
    if value["format"] != 1 {
        return Err(invalid(
            "/format",
            format!("{INDEX_FILE} needs \"format\": 1."),
            "Write { \"format\": 1, \"packages\": [ ... ] }.",
        ));
    }
    let Some(list) = value["packages"].as_array() else {
        return Err(invalid(
            "/packages",
            format!("{INDEX_FILE} needs a \"packages\" list."),
            "Write { \"format\": 1, \"packages\": [ { \"id\": ..., \"versions\": [ ... ] } ] }.",
        ));
    };
    if list.len() > MAX_PACKAGES {
        return Err(invalid(
            "/packages",
            format!("{INDEX_FILE} lists more than {MAX_PACKAGES} packages."),
            "Split the index.",
        ));
    }
    let text_at = |entry: &Value, key: &str| entry[key].as_str().unwrap_or("").to_owned();
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::new();
    for (n, entry) in list.iter().enumerate() {
        let at = format!("/packages/{n}");
        let id = text_at(entry, "id");
        if !packages::valid_id(&id) {
            return Err(invalid(
                &format!("{at}/id"),
                format!("'{id}' is not a package id."),
                "An id is lowercase dot-separated parts, such as com.example.dusk.",
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(invalid(
                &format!("{at}/id"),
                format!("'{id}' is listed twice."),
                "List each package once, with all its versions.",
            ));
        }
        let Some(raw_versions) = entry["versions"].as_array().filter(|v| !v.is_empty()) else {
            return Err(invalid(
                &format!("{at}/versions"),
                format!("'{id}' has no versions."),
                "Write \"versions\": [ { \"version\": \"1.0.0\", \"engine\": \">=0.1\", \"path\": \"dusk/1.0.0\" } ].",
            ));
        };
        if raw_versions.len() > MAX_VERSIONS {
            return Err(invalid(
                &format!("{at}/versions"),
                format!("'{id}' lists more than {MAX_VERSIONS} versions."),
                "Keep the versions people still need.",
            ));
        }
        let mut versions = Vec::new();
        for (m, raw) in raw_versions.iter().enumerate() {
            let here = format!("{at}/versions/{m}");
            let version =
                Version::parse(raw["version"].as_str().unwrap_or("")).map_err(|problem| {
                    problem
                        .at_file(INDEX_FILE)
                        .at_path(format!("{here}/version"))
                })?;
            let engine =
                VersionReq::parse(raw["engine"].as_str().unwrap_or("")).map_err(|problem| {
                    problem
                        .at_file(INDEX_FILE)
                        .at_path(format!("{here}/engine"))
                })?;
            let path = raw["path"].as_str().unwrap_or("");
            if !packages::inside_path(path) {
                return Err(invalid(
                    &format!("{here}/path"),
                    format!("'{path}' is not a folder inside the index."),
                    "Write a folder relative to the index, with forward slashes and no '..', such as \"dusk/1.0.0\".",
                ));
            }
            let hash = match &raw["hash"] {
                Value::Null => None,
                Value::String(hash)
                    if hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()) =>
                {
                    Some(hash.to_ascii_lowercase())
                }
                _ => {
                    return Err(invalid(
                        &format!("{here}/hash"),
                        format!("The hash of '{id}' {version} is not a content hash."),
                        "A content hash is 32 hex digits, as packages with action check gives it.",
                    ));
                }
            };
            if versions.iter().any(|v: &IndexVersion| v.version == version) {
                return Err(invalid(
                    &format!("{here}/version"),
                    format!("'{id}' lists {version} twice."),
                    "List each version once.",
                ));
            }
            versions.push(IndexVersion {
                version,
                engine,
                hash,
                folder: folder.join(path),
            });
        }
        versions.sort_by_key(|v| std::cmp::Reverse(v.version));
        let kinds = entry["kinds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|kind| packages::KINDS.contains(kind))
            .map(str::to_owned)
            .collect();
        entries.push(IndexEntry {
            name: Some(text_at(entry, "name"))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.clone()),
            id,
            description: text_at(entry, "description"),
            license: text_at(entry, "license"),
            kinds,
            versions,
        });
    }
    Ok(entries)
}

/// The installed version of `id` in `scope`, if any.
pub fn installed_version(id: &str, scope: Scope, project: &Path) -> Option<Version> {
    let folder = packages::scope_folder(scope, project)?.join(id);
    packages::read_manifest(&folder)
        .ok()
        .filter(|manifest| manifest.id == id)
        .map(|manifest| manifest.version)
}

/// Plans installing `chosen` of `entry` from the index (its manifest must
/// be what the index says).
pub fn plan_from_index(
    entry: &IndexEntry,
    chosen: &IndexVersion,
    scope: Scope,
    root: &Path,
    replace: bool,
) -> Result<packages::Plan> {
    let mut plan = packages::plan_install(&chosen.folder, scope, root, replace)?;
    if plan.manifest.id != entry.id || plan.manifest.version != chosen.version {
        return Err(Diagnostic::error(
            "package_index_invalid",
            format!(
                "The index names '{}' {}, but its folder holds '{}' {}.",
                entry.id, chosen.version, plan.manifest.id, plan.manifest.version
            ),
            "The index is out of date: write it again from its package folders.",
        )
        .at_file(INDEX_FILE));
    }
    plan.expect_hash.clone_from(&chosen.hash);
    Ok(plan)
}

/// The version `had` of `entry` updates to: the newest that fits this
/// engine and stays compatible (`^had`), when newer.
pub fn update_for(entry: &IndexEntry, had: Version) -> Option<&IndexVersion> {
    let compatible = VersionReq::parse(&format!("^{had}")).ok()?;
    entry
        .choose(Some(&compatible))
        .filter(|chosen| chosen.version > had)
}

/// Plans adding `id` from `index` into `scope`: the newest version that
/// fits the engine and `wanted`; with `update`, the one an installed
/// version updates to (replacing it). Returns the plan and the folder.
pub fn plan_add(
    index: &Index,
    id: &str,
    wanted: Option<&VersionReq>,
    scope: Scope,
    project: &Path,
    replace: bool,
) -> Result<(packages::Plan, PathBuf)> {
    let entry = index.require(id)?;
    let chosen = pick(entry, wanted)?;
    let plan = plan_from_index(entry, chosen, scope, project, replace)?;
    Ok((plan, chosen.folder.clone()))
}

/// Whether `entry` matches `query` (words, all of them, without regard to
/// case, in its id, name, description or kinds).
fn matches(entry: &IndexEntry, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.id,
        entry.name,
        entry.description,
        entry.kinds.join(" ")
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// The index's packages that match `query`, as `search` answers them.
pub fn search(index: &Index, query: &str, project: Option<&Path>) -> Vec<Value> {
    index
        .entries
        .iter()
        .filter(|entry| matches(entry, query))
        .take(MAX_RESULTS)
        .map(|entry| {
            let latest = entry.choose(None).map(|v| v.version);
            let project_has =
                project.and_then(|project| installed_version(&entry.id, Scope::Project, project));
            let user_has = installed_version(&entry.id, Scope::User, project.unwrap_or(Path::new("")));
            let newer = |had: Option<Version>| {
                had.and_then(|had| update_for(entry, had)).is_some()
            };
            json!({
                "id": entry.id,
                "name": entry.name,
                "description": entry.description,
                "license": entry.license,
                "kinds": entry.kinds,
                "latest": latest.map(|v| v.to_string()),
                "versions": entry.versions.iter().map(|v| v.version.to_string()).collect::<Vec<_>>(),
                "installed": {
                    "project": project_has.map(|v| v.to_string()),
                    "user": user_has.map(|v| v.to_string()),
                },
                "update": newer(project_has) || newer(user_has),
            })
        })
        .collect()
}

/// The version of `entry` to add: the newest fitting this engine and
/// `wanted`, or a diagnostic saying why there is none.
pub fn pick<'a>(entry: &'a IndexEntry, wanted: Option<&VersionReq>) -> Result<&'a IndexVersion> {
    entry.choose(wanted).ok_or_else(|| {
        let listed: Vec<String> = entry
            .versions
            .iter()
            .map(|v| format!("{} (engine {})", v.version, v.engine))
            .collect();
        Diagnostic::error(
            "package_version_not_found",
            match wanted {
                Some(wanted) => format!(
                    "No version of '{}' meets {wanted} and works with engine {}.",
                    entry.id,
                    Version::engine()
                ),
                None => format!(
                    "No version of '{}' works with engine {}.",
                    entry.id,
                    Version::engine()
                ),
            },
            format!("The index has: {}.", listed.join(", ")),
        )
    })
}
