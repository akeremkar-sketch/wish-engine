//! Loading a whole project folder: every prefab, scene, meta and material
//! file, checked together.
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
    /// `*.mat.json`.
    Material,
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

/// Reads a document. An unreadable one (locked, not UTF-8, deleted during
/// the scan) becomes a diagnostic on that file, and the rest of the project
/// is still checked.
fn read(path: &Path, name: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    "file_unreadable",
                    format!("Could not read '{name}': {error}."),
                    "Check that the file can be read and is UTF-8 text.",
                )
                .at_file(name),
            );
            None
        }
    }
}

/// Warnings about `Material` components of a scene or prefab file that say
/// something the engine does not use:
///
/// - `material_fields_unused`: `file` is set and the numbers are written
///   too. The file is the whole look; the numbers do nothing.
/// - `material_unused_on_model`: the entity has a `Model` and its
///   `Material` names no file. A model is drawn with its own materials;
///   only a material file replaces them.
///
/// Entities are looked at as they are written. What a prefab instance
/// inherits or overrides is not followed.
fn material_notes(source: &str, file: &str) -> Vec<Diagnostic> {
    let Ok(root) = wish_scene::json::parse(source, file) else {
        return Vec::new();
    };
    let document = root.to_value();
    let Some(entities) = document.get("entities").and_then(|e| e.as_object()) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    for (id, entity) in entities {
        let Some(material) = entity
            .pointer("/components/Material")
            .and_then(|m| m.as_object())
        else {
            continue;
        };
        let named = material
            .get("file")
            .and_then(|f| f.as_str())
            .is_some_and(|f| !f.is_empty());
        let others: Vec<&str> = material
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "file")
            .collect();
        let pointer = format!("/entities/{id}/components/Material");
        let note = if named && !others.is_empty() {
            Some(Diagnostic::warning(
                "material_fields_unused",
                format!(
                    "Entity '{id}' takes its look from a material file, so the numbers of its Material are not used: {}.",
                    others.join(", ")
                ),
                "Remove them, or change them in the material file (every entity that uses the file changes with it). For a plain look of its own, clear \"file\".",
            ))
        } else if !named && entity.pointer("/components/Model").is_some() {
            Some(Diagnostic::warning(
                "material_unused_on_model",
                format!(
                    "Entity '{id}' is a Model, which is drawn with the materials of its own file: this Material does nothing."
                ),
                "Remove it, or replace every material of the model by naming a material file in Material.file (asset_create makes one).",
            ))
        } else {
            None
        };
        if let Some(mut note) = note {
            if let Some(located) = root.locate(&pointer) {
                let span = located.key.unwrap_or(located.value);
                note = note.at_line(span.line, Some(span.column));
            }
            notes.push(note.at_path(pointer).at_file(file));
        }
    }
    notes
}

/// Loads and checks every prefab, scene, meta and material file below
/// `root`: syntax, schema, references between entities, prefab rules and
/// that referenced asset files exist. Prefabs load first, so that scenes can
/// use them. Nothing stops at the first problem.
pub fn load_project(root: &Path, registry: &Registry) -> Result<Project> {
    let listing = walk_files(root)?;
    let index = AssetIndex::scan(root)?;
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    let mut library = PrefabLibrary::new();

    for (name, path) in listing.iter().filter(|(n, _)| n.ends_with(".prefab.json")) {
        let Some(source) = read(path, name, &mut diagnostics) else {
            continue;
        };
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
        let Some(source) = read(path, name, &mut diagnostics) else {
            continue;
        };
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
        let Some(source) = read(path, name, &mut diagnostics) else {
            continue;
        };
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

    for (name, path) in listing
        .iter()
        .filter(|(n, _)| crate::material::is_material(n))
    {
        // A file far larger than a material can be is not read.
        let bytes = fs::metadata(path).map_or(0, |meta| meta.len());
        if bytes > crate::material::MAX_MATERIAL_BYTES {
            diagnostics.push(
                Diagnostic::error(
                    "material_file_too_large",
                    format!(
                        "'{name}' is {bytes} bytes; a material file is at most {} bytes.",
                        crate::material::MAX_MATERIAL_BYTES
                    ),
                    "A material file holds a few numbers and paths. Pictures go in their own PNG files, which the material names.",
                )
                .at_file(name),
            );
            continue;
        }
        let Some(source) = read(path, name, &mut diagnostics) else {
            continue;
        };
        let (material, problems) = crate::material::check_material(&source, name, &index);
        diagnostics.extend(problems);
        files.push(ProjectFile {
            path: name.clone(),
            kind: FileKind::Material,
            source,
            canonical: material
                .as_ref()
                .map(crate::material::MaterialDocument::to_canonical)
                .transpose()?,
        });
    }

    // What a `Material` on an entity cannot mean, said where it is written.
    for file in files
        .iter()
        .filter(|file| matches!(file.kind, FileKind::Scene | FileKind::Prefab))
    {
        diagnostics.extend(material_notes(&file.source, &file.path));
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
