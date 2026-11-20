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
    /// The file's text in canonical form, in the format the file is in
    /// (a scene of an older format is tidied, not migrated: that is
    /// `wen migrate`); `None` if the file has errors.
    pub canonical: Option<String>,
    /// For a scene or prefab file of an older format than the engine
    /// writes: that format. It was read with its own format's defaults.
    pub older_format: Option<u32>,
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
/// - `material_invisible`: a plain `Material` (no file) whose `base_color`
///   alpha is 0 or below. An alpha below 1 makes a surface see-through, and
///   at 0 nothing of it is drawn: written in a file, that is almost always a
///   mistake (game code that fades something out changes the value in the
///   world, not in the file).
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
            notes.push(note.at_path(pointer.clone()).at_file(file));
        }
        let alpha = material
            .get("base_color")
            .and_then(|colour| colour.get(3))
            .and_then(serde_json::Value::as_f64);
        if let Some(alpha) = alpha.filter(|alpha| !named && *alpha <= 0.0) {
            let pointer = format!("{pointer}/base_color/3");
            let mut note = Diagnostic::warning(
                "material_invisible",
                format!(
                    "Entity '{id}' is invisible: the alpha of its Material.base_color is {alpha}, and a surface is see-through by its alpha (1 solid, 0.5 half, 0 or below nothing)."
                ),
                "Set the fourth number of base_color to 1 for a solid surface, or between 0 and 1 for a see-through one. To hide an entity on purpose, remove its Primitive or Model; to fade it, let game code lower the alpha while the game runs.",
            );
            if let Some(located) = root.locate(&pointer) {
                let span = located.key.unwrap_or(located.value);
                note = note.at_line(span.line, Some(span.column));
            }
            notes.push(note.at_path(pointer).at_file(file));
        }
    }
    notes
}

/// The warning `ambient_color_unused` for a scene whose `Ambient` sets a
/// `color` or an `intensity` other than the default while its light comes
/// from the sky (`source: Sky`, the default since scene format 2), which
/// uses neither. Located at the first such field in the file. The scene is
/// judged as loaded, so a file of format 1 is read with its own defaults
/// (there the source was `Color`, written in by the loader). The renderer
/// gives the same warning for a picture (`wish_render`'s frame).
///
/// Only the engine's `Ambient` is looked at: a registry's type of that name
/// without a `source` field defaulting to `"Sky"` is someone else's.
fn ambient_note(
    document: &SceneDocument,
    source: &str,
    file: &str,
    registry: &Registry,
) -> Option<Diagnostic> {
    let patch = document.resources().get("Ambient")?.as_object()?;
    let info = registry
        .resources()
        .map(|(_, info)| info)
        .find(|info| info.name() == "Ambient")?;
    let defaults = wish_schema::to_json(
        info.schema(),
        info.default_bytes(),
        &wish_ecs::TextTable::default(),
    )
    .ok()?;
    if defaults.get("source").and_then(serde_json::Value::as_str) != Some("Sky") {
        return None;
    }
    let source_now = patch.get("source").or_else(|| defaults.get("source"))?;
    if source_now.as_str() != Some("Sky") {
        return None;
    }
    let field = ["color", "intensity"].into_iter().find(|field| {
        patch
            .get(*field)
            .is_some_and(|value| defaults.get(*field).is_none_or(|d| !same(value, d)))
    })?;
    let pointer = format!("/resources/Ambient/{field}");
    let mut note = Diagnostic::warning(
        "ambient_color_unused",
        format!(
            "Ambient.{field} is set, but the ambient light comes from the sky (Ambient.source is Sky, the default since scene format 2), which does not use Ambient.color or Ambient.intensity: the setting changes nothing."
        ),
        "For a flat ambient light of that colour and intensity, set Ambient.source to \"Color\" (or apply the look plain, which turns every look effect off). To keep the light from the sky, remove Ambient.color and Ambient.intensity, and use Ambient.sky_strength to make it stronger or weaker.",
    );
    if let Ok(root) = wish_scene::json::parse(source, file)
        && let Some(located) = root.locate(&pointer)
    {
        let span = located.key.unwrap_or(located.value);
        note = note.at_line(span.line, Some(span.column));
    }
    Some(note.at_path(pointer).at_file(file))
}

/// Whether two JSON values are the same value: numbers by their value
/// (`20000` is `20000.0`), arrays element by element.
fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        _ => a == b,
    }
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
        let (canonical, older_format) = (result.canonical, result.older_format);
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
            canonical,
            older_format,
        });
    }

    let library = Arc::new(library);
    let mut scenes = BTreeMap::new();
    for (name, path) in listing.iter().filter(|(n, _)| n.ends_with(".scene.json")) {
        let Some(source) = read(path, name, &mut diagnostics) else {
            continue;
        };
        let result = load_with_prefabs(&source, name, registry, Arc::clone(&library));
        let (canonical, older_format) = (result.canonical, result.older_format);
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
            canonical,
            older_format,
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
            older_format: None,
        });
    }

    for (name, path) in listing
        .iter()
        .filter(|(n, _)| crate::material::is_material(n))
    {
        // A file far larger than a material can be is not read.
        let source = match crate::material::read_material_file(path, name) {
            Ok(source) => source,
            Err(problem) => {
                diagnostics.push(problem);
                continue;
            }
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
            older_format: None,
        });
    }

    // What a `Material` on an entity cannot mean, said where it is written.
    for file in files
        .iter()
        .filter(|file| matches!(file.kind, FileKind::Scene | FileKind::Prefab))
    {
        diagnostics.extend(material_notes(&file.source, &file.path));
    }
    // An ambient colour or intensity the sky light does not use.
    for file in files.iter().filter(|file| file.kind == FileKind::Scene) {
        if let Some(document) = scenes.get(&file.path) {
            diagnostics.extend(ambient_note(document, &file.source, &file.path, registry));
        }
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
