//! Which asset files exist in a project.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use wish_core::asset::{split_asset_ref, validate_asset_ref};
use wish_core::suggest::closest_match;
use wish_core::{Diagnostic, Result};

/// The folder of a project's game code (M5): Rust sources the engine
/// compiles, not assets.
pub const SCRIPTS_FOLDER: &str = "scripts/";

/// Folders of the project that are not assets: game code, scenario tests
/// (and their golden images) and replays.
pub const NON_ASSET_FOLDERS: &[&str] = &[SCRIPTS_FOLDER, "tests/", "replays/"];

/// Whether a file name is an engine document rather than an asset: a scene,
/// a prefab or a meta file.
pub fn is_document(name: &str) -> bool {
    [".scene.json", ".prefab.json", ".meta.json"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// Whether a file name is a temporary file of an editor or of the engine's
/// own atomic writes: never an asset, and not a change worth reacting to.
pub(crate) fn is_temporary(name: &str) -> bool {
    const SUFFIXES: &[&str] = &[".part", ".tmp", ".swp", ".swx", ".wish-tmp", "~", ".bak"];
    SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        || (name.starts_with('#') && name.ends_with('#'))
}

/// Every file below `root` that is not inside a hidden folder (one whose name
/// starts with `.`), as project-relative `/` paths with their real paths, in
/// path order so that everything built from it is deterministic. Names that
/// are not valid UTF-8 are skipped: a JSON file cannot refer to them.
///
/// Symbolic links and junctions are not followed (a link back up the tree
/// would never end), and a sub-folder that cannot be read, for example
/// because it was deleted during the walk, is skipped. Only an unreadable
/// project folder is an error.
pub(crate) fn walk_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if folder != root => continue,
            Err(error) => {
                return Err(Diagnostic::error(
                    "file_unreadable",
                    format!("Could not read the folder '{}': {error}.", folder.display()),
                    "Check that the project folder exists and can be read.",
                ));
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') || kind.is_symlink() || is_temporary(name) {
                continue;
            }
            if kind.is_dir() {
                folders.push(path);
            } else if let Ok(relative) = path.strip_prefix(root) {
                let parts: Option<Vec<&str>> = relative
                    .components()
                    .map(|part| part.as_os_str().to_str())
                    .collect();
                if let Some(parts) = parts {
                    found.push((parts.join("/"), path));
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Every engine document (scene, prefab or meta file) below `root`, as
/// project-relative paths with their real paths, in path order.
pub fn documents(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    Ok(walk_files(root)?
        .into_iter()
        .filter(|(name, _)| is_document(name))
        .collect())
}

/// The asset files of a project: everything except hidden folders and
/// engine documents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetIndex {
    files: BTreeSet<String>,
}

impl AssetIndex {
    /// Scans a project folder. Game code (`scripts/`) is source, not an
    /// asset, and is left out.
    pub fn scan(root: &Path) -> Result<Self> {
        let files = walk_files(root)?
            .into_iter()
            .filter(|(path, _)| {
                !is_document(path)
                    && !NON_ASSET_FOLDERS
                        .iter()
                        .any(|folder| path.starts_with(folder))
            })
            .map(|(path, _)| path)
            .collect();
        Ok(Self { files })
    }

    /// An index of the given project-relative paths.
    pub fn from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            files: paths.into_iter().map(str::to_owned).collect(),
        }
    }

    /// Whether an asset file exists at this project-relative path (no
    /// `#sub` part).
    pub fn contains(&self, path: &str) -> bool {
        self.files.contains(path)
    }

    /// The asset paths, sorted.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(String::as_str)
    }

    /// How many asset files there are.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether there are no asset files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Checks an asset reference: well formed, and its file exists. An empty
    /// reference (no asset) is fine. The `#sub` object is not checked: which
    /// sub-objects a file has is known only once it is imported (M3).
    pub fn check(&self, reference: &str) -> Result<()> {
        if let Err(problem) = validate_asset_ref(reference) {
            return Err(Diagnostic::error(
                "invalid_asset_ref",
                format!("'{reference}' is not a valid asset reference: {problem}."),
                "Write a path relative to the project folder with '/' between folders, such as \"assets/models/crate.glb\".",
            ));
        }
        let (path, _) = split_asset_ref(reference);
        if path.is_empty() || self.contains(path) {
            return Ok(());
        }
        let hint = match closest_match(path, self.paths()) {
            Some(closest) => format!("Did you mean '{closest}'?"),
            None if self.is_empty() => {
                "The project has no asset files. Check that the project folder is right.".to_owned()
            }
            None => "Check the path; `wen mv` moves a file and updates the scenes that use it."
                .to_owned(),
        };
        Err(Diagnostic::error(
            "asset_not_found",
            format!("No asset file '{path}' exists in the project."),
            hint,
        ))
    }
}
