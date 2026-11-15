//! Loading a whole project folder: every prefab, scene and meta file,
//! checked together.
//!
//! This is what `wen check` and `wen fmt` use, and what hot reloading
//! re-runs when a file changes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;
use wish_scene::{PrefabLibrary, SceneDocument, load_prefab, load_with_prefabs};

use crate::index::{AssetIndex, walk_files};
use crate::meta::parse_meta;
use crate::refs::check_source;

/// The kind of a project file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// `*.scene.json`.
    Scene,
    /// `*.prefab.json`.
    Prefab,
    /// `*.meta.json`.
    Meta,
}

/// One document of the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    /// The project-relative path.
    pub path: String,
    /// What kind of document it is.
    pub kind: FileKind,
    /// The file's text as it is on disk.
    pub source: String,
    /// The file's text in canonical form; `None` if the file has errors.
    pub canonical: Option<String>,
}

/// A loaded project.
#[derive(Debug)]
pub struct Project {
    /// The prefabs that loaded without errors, shared by the scenes.
    pub prefabs: Arc<PrefabLibrary>,
    /// The scenes that loaded without errors, by path.
    pub scenes: BTreeMap<String, SceneDocument>,
    /// Every document, in path order.
    pub files: Vec<ProjectFile>,
    /// Every problem found: errors and warnings, in path order. Includes
    /// asset references to files that do not exist.
    pub diagnostics: Vec<Diagnostic>,
}

impl Project {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

fn read(path: &Path, name: &str) -> Result<String> {
    fs::read_to_string(path).map_err(|error| {
        Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{name}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
    })
}

/// Loads and checks every prefab, scene and meta file below `root`:
/// syntax, schema, references between entities, prefab rules and that
/// referenced asset files exist. Prefabs load first, so that scenes can use
/// them. Nothing stops at the first problem.
pub fn load_project(root: &Path, registry: &Registry) -> Result<Project> {
    let listing = walk_files(root)?;
    let index = AssetIndex::scan(root)?;
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    let mut library = PrefabLibrary::new();

    for (name, path) in listing.iter().filter(|(n, _)| n.ends_with(".prefab.json")) {
        let source = read(path, name)?;
        let result = load_prefab(&source, name, registry);
        let canonical = result.document.as_ref().map(SceneDocument::to_canonical);
        if let Some(document) = result.document {
            library.insert(name, document);
        }
        diagnostics.extend(result.diagnostics);
        diagnostics.extend(
            check_source(&source, name, registry, &index)
                .into_iter()
                .filter(|d| d.code != "json_syntax"),
        );
        files.push(ProjectFile {
            path: name.clone(),
            kind: FileKind::Prefab,
            source,
            canonical: canonical.transpose()?,
        });
    }

    let library = Arc::new(library);
    let mut scenes = BTreeMap::new();
    for (name, path) in listing.iter().filter(|(n, _)| n.ends_with(".scene.json")) {
        let source = read(path, name)?;
        let result = load_with_prefabs(&source, name, registry, Arc::clone(&library));
        let canonical = result.document.as_ref().map(SceneDocument::to_canonical);
        if let Some(document) = result.document {
            scenes.insert(name.clone(), document);
        }
        diagnostics.extend(result.diagnostics);
        diagnostics.extend(
            check_source(&source, name, registry, &index)
                .into_iter()
                .filter(|d| d.code != "json_syntax"),
        );
        files.push(ProjectFile {
            path: name.clone(),
            kind: FileKind::Scene,
            source,
            canonical: canonical.transpose()?,
        });
    }

    for (name, path) in listing.iter().filter(|(n, _)| n.ends_with(".meta.json")) {
        let source = read(path, name)?;
        let canonical = match parse_meta(&source, name) {
            Ok(meta) => Some(meta.to_canonical()?),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                None
            }
        };
        files.push(ProjectFile {
            path: name.clone(),
            kind: FileKind::Meta,
            source,
            canonical,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    // Diagnostics in path order (stable: file order within a file is kept).
    diagnostics.sort_by(|a, b| a.location.file.cmp(&b.location.file));
    Ok(Project {
        prefabs: library,
        scenes,
        files,
        diagnostics,
    })
}
