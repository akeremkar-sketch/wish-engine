//! Moving an asset, a prefab or a folder without breaking references.
//!
//! [`plan_move`] works out everything a move changes without touching the
//! disk; [`apply_move`] then does it: the file or folder is renamed, and
//! every scene, prefab, material and sheet file that referred to something in it
//! is rewritten, all or nothing (a failure half-way puts everything back, including folders
//! the move created).
//!
//! References are asset references (fields of type `AssetRef`, found by
//! schema) and prefab references (an entity's `"prefab"` key). Documents that
//! change are rewritten as canonical JSON (sorted keys, the canonical layout);
//! document-level tidying such as dropping `"order": 0` is left to `wen fmt`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use wish_core::asset::validate_asset_ref;
use wish_core::suggest::closest_match;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;
use wish_scene::json::to_canonical;

use crate::index::{is_document, walk_files};
use crate::material::is_material;
use crate::meta::meta_path;
use crate::refs::{path_and_suffix, read_document, visit_document, visit_material, visit_sheet};
use crate::sheet::is_sheet;

/// A scene, prefab, material or sheet file that a move rewrites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentEdit {
    /// The project-relative path of the file before the move.
    pub file: String,
    /// Where the file is after the move: the same path, unless it is inside
    /// the folder being moved.
    pub target: String,
    /// How many references in it change.
    pub changes: usize,
    original: String,
    updated: String,
}

/// Everything a move does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovePlan {
    /// Where the file or folder is now.
    pub from: String,
    /// Where it goes.
    pub to: String,
    /// The meta file that moves with a file, if it has one.
    pub meta: Option<(String, String)>,
    /// The scene and prefab files that are rewritten, in path order.
    pub documents: Vec<DocumentEdit>,
}

fn failure(code: &'static str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error(code, message, hint)
}

fn io_failure(action: &str, path: &str, error: &std::io::Error) -> Diagnostic {
    failure(
        "file_unwritable",
        format!("Could not {action} '{path}': {error}."),
        "Check that nothing else has the file open and that the folder can be written to.",
    )
}

/// What a path or reference becomes when `from` moves to `to`, or `None`
/// when it is not affected. `from` may be a file or a folder; comparison is
/// exact, byte for byte.
fn remap(reference: &str, from: &str, to: &str) -> Option<String> {
    let (path, suffix) = path_and_suffix(reference);
    if path == from {
        return Some(format!("{to}{suffix}"));
    }
    let rest = path.strip_prefix(from)?.strip_prefix('/')?;
    Some(format!("{to}/{rest}{suffix}"))
}

/// Calls `visit` on every entity's `"prefab"` string in a document's JSON.
fn visit_prefab_refs(document: &mut Value, visit: &mut dyn FnMut(&mut String)) {
    if let Some(Value::Object(entities)) = document.get_mut("entities") {
        for entity in entities.values_mut() {
            if let Some(Value::String(path)) = entity.get_mut("prefab") {
                visit(path);
            }
        }
    }
}

fn check_path(name: &str, path: &str) -> Result<()> {
    if path.is_empty() || path.contains('#') || validate_asset_ref(path).is_err() {
        return Err(failure(
            "invalid_asset_ref",
            format!("'{path}' is not a valid path to move ({name})."),
            "Use a path relative to the project folder with '/' between folders, without '#'.",
        ));
    }
    Ok(())
}

/// Plans moving the file or folder `from` to `to` (project-relative paths)
/// below `root`. Nothing is changed on disk.
///
/// `from` must name a file or folder exactly as it is written in the project
/// (on a case-insensitive file system, `Level1` does not stand for `level1`:
/// references are compared exactly, so a near match would move the file but
/// update nothing). The registry supplies the component types, so that asset
/// references are found by schema.
pub fn plan_move(root: &Path, from: &str, to: &str, registry: &Registry) -> Result<MovePlan> {
    check_path("from", from)?;
    check_path("to", to)?;
    if from == to {
        return Err(failure(
            "nothing_to_move",
            format!("'{from}' and '{to}' are the same path."),
            "Give a different destination.",
        ));
    }
    if to.starts_with(&format!("{from}/")) {
        return Err(failure(
            "move_into_itself",
            format!("'{to}' is inside '{from}'."),
            "A folder cannot move into itself. Choose a destination outside it.",
        ));
    }
    if to.split('/').any(|segment| segment.starts_with('.')) {
        return Err(failure(
            "hidden_destination",
            format!("'{to}' is inside a hidden folder or is a hidden file."),
            "The project ignores names that start with '.', so references to it could not be checked. Choose a visible name.",
        ));
    }

    let listing = walk_files(root)?;
    let is_file = listing.iter().any(|(name, _)| name == from);
    let is_folder = !is_file
        && listing
            .iter()
            .any(|(name, _)| remap(name, from, to).is_some());
    if !is_file && !is_folder {
        let names = listing.iter().map(|(name, _)| name.as_str());
        let hint = match closest_match(from, names) {
            Some(closest) => format!("Did you mean '{closest}'? Paths are matched exactly, including upper and lower case."),
            None => "Check the path; it is relative to the project folder (--root) and matched exactly, including case.".to_owned(),
        };
        return Err(Diagnostic::error(
            "asset_not_found",
            format!("There is no file or folder '{from}' in the project."),
            hint,
        ));
    }
    if is_file
        && (is_document(from) != is_document(to)
            || is_material(from) != is_material(to)
            || is_sheet(from) != is_sheet(to))
    {
        return Err(failure(
            "kind_change",
            format!("Moving '{from}' to '{to}' would turn it into a different kind of file."),
            "Keep the ending: assets stay assets, and a .scene.json, .prefab.json, .meta.json, .mat.json or .sheet.json file keeps its ending.",
        ));
    }
    if root.join(to).exists() {
        return Err(failure(
            "asset_exists",
            format!("'{to}' already exists."),
            "Choose a destination that does not exist; nothing is overwritten.",
        ));
    }

    let meta = if is_file && listing.iter().any(|(name, _)| *name == meta_path(from)) {
        let target = meta_path(to);
        if root.join(&target).exists() {
            return Err(failure(
                "asset_exists",
                format!("'{target}' already exists."),
                "Remove the stray meta file, or choose another destination.",
            ));
        }
        Some((meta_path(from), target))
    } else {
        None
    };

    let mut documents = Vec::new();
    for (file, path) in &listing {
        let material = is_material(file);
        let sheet = is_sheet(file);
        if !(file.ends_with(".scene.json") || file.ends_with(".prefab.json") || material || sheet) {
            continue;
        }
        let original = if material {
            // Read with its size cap, like everywhere a material is read.
            crate::material::read_material_file(path, file)?
        } else if sheet {
            crate::sheet::read_sheet_file(path, file)?
        } else {
            fs::read_to_string(path).map_err(|error| {
                failure(
                    "file_unreadable",
                    format!("Could not read '{file}': {error}."),
                    "Check that the file can be read and is UTF-8 text.",
                )
            })?
        };
        let mut value = read_document(&original, file)?;
        let mut changes = 0;
        let mut rewrite = |_: &str, reference: &mut String| {
            if let Some(new) = remap(reference, from, to) {
                *reference = new;
                changes += 1;
            }
        };
        if material {
            // The textures a material file names.
            visit_material(&mut value, &mut rewrite);
        } else if sheet {
            // The picture a sheet is cut from.
            visit_sheet(&mut value, &mut rewrite);
        } else {
            visit_document(registry, &mut value, &mut rewrite);
        }
        visit_prefab_refs(&mut value, &mut |path| {
            if let Some(new) = remap(path, from, to) {
                *path = new;
                changes += 1;
            }
        });
        if changes > 0 {
            documents.push(DocumentEdit {
                file: file.clone(),
                target: remap(file, from, to).unwrap_or_else(|| file.clone()),
                changes,
                original,
                updated: to_canonical(&value)?,
            });
        }
    }
    Ok(MovePlan {
        from: from.to_owned(),
        to: to.to_owned(),
        meta,
        documents,
    })
}

/// The folders between `root` and `path` that do not exist yet, outermost
/// first.
fn missing_folders(root: &Path, path: &Path) -> Vec<PathBuf> {
    let mut missing: Vec<PathBuf> = path
        .ancestors()
        .skip(1)
        .take_while(|folder| *folder != root && !folder.exists())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    missing
}

/// Carries out a plan. If any step fails, the steps already done are
/// undone, so the project is left as it was.
pub fn apply_move(root: &Path, plan: &MovePlan) -> Result<()> {
    let destination = root.join(&plan.to);
    let created = missing_folders(root, &destination);
    let remove_created = || {
        for folder in created.iter().rev() {
            // Only empty folders go: anything else in them is not ours.
            let _ = fs::remove_dir(folder);
        }
    };
    if let Some(parent) = destination.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        remove_created();
        return Err(io_failure("create the folder for", &plan.to, &error));
    }
    if let Err(error) = fs::rename(root.join(&plan.from), &destination) {
        remove_created();
        return Err(io_failure("move", &plan.from, &error));
    }

    let undo = |moved_meta: bool, written: &[&DocumentEdit]| {
        for edit in written {
            let _ = fs::write(root.join(&edit.target), &edit.original);
        }
        if moved_meta && let Some((old, new)) = &plan.meta {
            let _ = fs::rename(root.join(new), root.join(old));
        }
        let _ = fs::rename(&destination, root.join(&plan.from));
        remove_created();
    };

    let mut moved_meta = false;
    if let Some((old, new)) = &plan.meta {
        if let Err(error) = fs::rename(root.join(old), root.join(new)) {
            undo(false, &[]);
            return Err(io_failure("move", old, &error));
        }
        moved_meta = true;
    }
    let mut written: Vec<&DocumentEdit> = Vec::new();
    for edit in &plan.documents {
        if let Err(error) = fs::write(root.join(&edit.target), &edit.updated) {
            undo(moved_meta, &written);
            return Err(io_failure("rewrite", &edit.target, &error));
        }
        written.push(edit);
    }
    Ok(())
}
