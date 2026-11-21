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
/// (and their golden images), replays, exported games and the game's design
/// (`design/game.json` and the pictures of play-test notes, M9c).
pub const NON_ASSET_FOLDERS: &[&str] = &[
    SCRIPTS_FOLDER,
    "tests/",
    "replays/",
    EXPORTS_FOLDER,
    "design/",
    "packages/",
];

/// Where exported games go (M9): copies of the project's files, not the
/// project's own documents or assets.
pub const EXPORTS_FOLDER: &str = "exports/";

/// Whether a file name is an engine document: a scene, a prefab, a meta
/// file, a material file, a sprite sheet, a tileset or a tilemap.
/// Documents are strict, canonical
/// JSON with a format number, which `wen check`, `wen fmt` and `wen migrate`
/// work on.
///
/// Scenes, prefabs and meta files are not assets. Material files and sprite
/// sheets, tilesets and tilemaps are all both documents and assets ([`AssetIndex`] lists them),
/// because scenes refer to them the way they refer to a model.
pub fn is_document(name: &str) -> bool {
    [".scene.json", ".prefab.json", ".meta.json"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || is_document_asset(name)
}

/// Whether a file name is a document that is also an asset: a material
/// file, a sprite sheet, a tileset or a tilemap.
pub fn is_document_asset(name: &str) -> bool {
    crate::material::is_material(name)
        || crate::sheet::is_sheet(name)
        || crate::tiles::is_tileset(name)
        || crate::tiles::is_tilemap(name)
}

/// Reads a document that is also an asset (a material file or a sprite
/// sheet, [`is_document_asset`]) with its kind's size cap
/// (`material_file_too_large`, `sheet_file_too_large`,
/// `tileset_file_too_large`, `tilemap_file_too_large`): every place that
/// reads such a file whole goes through this or its kind's own reader.
/// Any other file is read as a material file is, with the same cap.
pub fn read_document_asset(file: &Path, name: &str) -> Result<String> {
    if crate::sheet::is_sheet(name) {
        crate::sheet::read_sheet_file(file, name)
    } else if crate::tiles::is_tileset(name) {
        crate::tiles::read_tileset_file(file, name)
    } else if crate::tiles::is_tilemap(name) {
        crate::tiles::read_tilemap_file(file, name)
    } else {
        crate::material::read_material_file(file, name)
    }
}

/// Whether a diagnostic says a document asset was too large to be read.
pub fn is_too_large(problem: &Diagnostic) -> bool {
    matches!(
        &*problem.code,
        "material_file_too_large"
            | "sheet_file_too_large"
            | "tileset_file_too_large"
            | "tilemap_file_too_large"
    )
}

/// Whether a project file is an asset: anything that is not a scene, a
/// prefab or a meta file (material files and sprite sheets are), outside
/// the folders that hold no assets.
fn is_asset(path: &str) -> bool {
    (!is_document(path) || is_document_asset(path))
        && !NON_ASSET_FOLDERS
            .iter()
            .any(|folder| path.starts_with(folder))
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
            // Exported games are copies, not part of the project: check,
            // fmt, moves and the watcher leave them alone.
            if folder == root && kind.is_dir() && format!("{name}/") == EXPORTS_FOLDER {
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

/// Every engine document (scene, prefab, meta, material or sheet file) below
/// `root`, as project-relative paths with their real paths, in path order.
pub fn documents(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    Ok(walk_files(root)?
        .into_iter()
        .filter(|(name, _)| is_document(name) && !name.starts_with(EXPORTS_FOLDER))
        .collect())
}

/// The asset files of a project: everything except hidden folders, scenes,
/// prefabs and meta files. Material files and sprite sheets are assets.
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
            .filter(|(path, _)| is_asset(path))
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
