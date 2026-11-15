//! Moving an asset (or a folder of assets) without breaking references.
//!
//! [`plan_move`] works out everything a move changes without touching the
//! disk; [`apply_move`] then does it: the file is renamed, and every scene
//! and prefab that referred to it is rewritten, all or nothing (a failure
//! half-way puts everything back).
//!
//! Documents that change are rewritten in canonical form, so a file that was
//! not canonical before also gets reformatted. Meaning does not change.

use std::fs;
use std::path::Path;

use wish_core::asset::validate_asset_ref;
use wish_core::suggest::closest_match;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;
use wish_scene::json::to_canonical;

use crate::index::{AssetIndex, walk_files};
use crate::meta::meta_path;
use crate::refs::{path_and_suffix, read_document, visit_document};

/// A scene or prefab file that a move rewrites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentEdit {
    /// The project-relative path of the file.
    pub file: String,
    /// How many references in it change.
    pub changes: usize,
    original: String,
    updated: String,
}

/// Everything a move does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovePlan {
    /// Where the asset (file or folder) is now.
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

/// What a reference becomes when `from` moves to `to`, or `None` when it is
/// not affected. `from` may be a file or a folder.
fn remap(reference: &str, from: &str, to: &str) -> Option<String> {
    let (path, suffix) = path_and_suffix(reference);
    if path == from {
        return Some(format!("{to}{suffix}"));
    }
    let rest = path.strip_prefix(from)?.strip_prefix('/')?;
    Some(format!("{to}/{rest}{suffix}"))
}

/// Plans moving the asset or folder `from` to `to` (project-relative paths)
/// below `root`. Nothing is changed on disk.
///
/// The registry supplies the component types, so that references are found
/// by schema.
pub fn plan_move(root: &Path, from: &str, to: &str, registry: &Registry) -> Result<MovePlan> {
    for (name, path) in [("from", from), ("to", to)] {
        if path.is_empty() || path.contains('#') || validate_asset_ref(path).is_err() {
            return Err(failure(
                "invalid_asset_ref",
                format!("'{path}' is not a valid asset path to move ({name})."),
                "Use a path relative to the project folder with '/' between folders, without '#'.",
            ));
        }
    }
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
    if !root.join(from).exists() {
        let index = AssetIndex::scan(root)?;
        let hint = match closest_match(from, index.paths()) {
            Some(closest) => format!("Did you mean '{closest}'?"),
            None => "Check the path; it is relative to the project folder (--root).".to_owned(),
        };
        return Err(Diagnostic::error(
            "asset_not_found",
            format!("There is no file or folder '{from}' in the project."),
            hint,
        ));
    }
    if root.join(to).exists() {
        return Err(failure(
            "asset_exists",
            format!("'{to}' already exists."),
            "Choose a destination that does not exist; nothing is overwritten.",
        ));
    }

    let meta = if root.join(from).is_file() && root.join(meta_path(from)).is_file() {
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
    for (file, path) in walk_files(root)? {
        if !(file.ends_with(".scene.json") || file.ends_with(".prefab.json")) {
            continue;
        }
        let original = fs::read_to_string(&path).map_err(|error| {
            failure(
                "file_unreadable",
                format!("Could not read '{file}': {error}."),
                "Check that the file can be read and is UTF-8 text.",
            )
        })?;
        let mut value = read_document(&original, &file)?;
        let mut changes = 0;
        visit_document(registry, &mut value, &mut |_, reference| {
            if let Some(new) = remap(reference, from, to) {
                *reference = new;
                changes += 1;
            }
        });
        if changes > 0 {
            documents.push(DocumentEdit {
                file,
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

/// Carries out a plan. If any step fails, the steps already done are
/// undone, so the project is left as it was.
pub fn apply_move(root: &Path, plan: &MovePlan) -> Result<()> {
    let destination = root.join(&plan.to);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| io_failure("create the folder for", &plan.to, &error))?;
    }
    fs::rename(root.join(&plan.from), &destination)
        .map_err(|error| io_failure("move", &plan.from, &error))?;

    let undo = |moved_meta: bool, written: &[&DocumentEdit]| {
        for edit in written {
            let _ = fs::write(root.join(&edit.file), &edit.original);
        }
        if moved_meta && let Some((old, new)) = &plan.meta {
            let _ = fs::rename(root.join(new), root.join(old));
        }
        let _ = fs::rename(&destination, root.join(&plan.from));
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
        if let Err(error) = fs::write(root.join(&edit.file), &edit.updated) {
            undo(moved_meta, &written);
            return Err(io_failure("rewrite", &edit.file, &error));
        }
        written.push(edit);
    }
    Ok(())
}
