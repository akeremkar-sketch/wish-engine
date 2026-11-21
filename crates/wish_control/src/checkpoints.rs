//! Checkpoints (M8b WP3): the project's scenes, prefabs and settings as they
//! were at a moment, kept under `.wish/checkpoints/<name>/`, to go back to
//! ("back to before the agent started") and to compare with (before /
//! after pictures).
//!
//! Going back never loses anything: each scene and prefab is brought back
//! by one transaction in its history (the operations that turn it back),
//! so the return can itself be undone, or taken back on its own. Material
//! and meta files and the project files, which have no history of their
//! own, are kept and brought back as their text; the texts they replace
//! are kept in the project's trash (`.wish/trash/checkpoints/`), which the
//! answer names.
//!
//! A checkpoint's description is read like any file: a path in it that is
//! not a document of the project refuses the restore (the 1.0 closing
//! review found one could write anywhere).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::{Actor, PrefabLibrary, SceneDocument};

use crate::workspace::Workspace;

/// Where checkpoints live, inside the project.
pub const CHECKPOINTS: &str = ".wish/checkpoints";
/// The project files a checkpoint keeps besides the documents.
const PROJECT_FILES: &[&str] = &[crate::project::PROJECT_FILE, wish_input::INPUT_FILE];
/// How many checkpoints a project keeps at most.
pub const MAX_CHECKPOINTS: usize = 200;
/// Where going back to a checkpoint keeps the texts it writes over (the
/// material, meta and project files, which have no history of their own),
/// inside the project.
pub const TRASH: &str = ".wish/trash/checkpoints";

/// What a restore has done so far.
#[derive(Default)]
struct Progress {
    /// The documents brought back (or, in a dry run, that would be).
    restored: Vec<Value>,
    /// The documents that are as the checkpoint kept them already.
    unchanged: Vec<Value>,
    /// The project files written back (or that would be).
    settings: Vec<Value>,
    /// Every file changed, in order.
    written: Vec<String>,
    /// The folder of the trash the replaced texts went to, once one did.
    trash: Option<String>,
}

/// Whether `name` can name a checkpoint: 1 to 40 of a-z 0-9 _ -.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn checked_name(name: &str) -> Result<()> {
    if valid_name(name) {
        return Ok(());
    }
    Err(Diagnostic::error(
        "invalid_checkpoint_name",
        format!("'{name}' cannot name a checkpoint."),
        "Use 1 to 40 of a-z, 0-9, _ and -, such as \"before_agent\".",
    )
    .at_path("/name"))
}

/// Whether a kept document is a scene or a prefab: one brought back by
/// operations in its history. Every other document (material and meta
/// files, sprite sheets) is brought back as text.
fn is_scene_document(path: &str) -> bool {
    path.ends_with(".scene.json") || path.ends_with(".prefab.json")
}

/// Whether `path` can be a document a checkpoint keeps: written exactly
/// as `wish_asset::index::documents` gives it (project-relative, `/`
/// between folders, no `.` or `..` parts, no hidden folders, drives or
/// device names), a scene, prefab, material, sheet or meta file, outside
/// `exports/`. Anything else in a checkpoint's description was not
/// written by the engine, and is never read or written.
fn keepable(path: &str) -> bool {
    const KINDS: &[&str] = &[
        "scene.json",
        "prefab.json",
        "mat.json",
        "sheet.json",
        "meta.json",
    ];
    crate::documents::normalize_asset_output(path, KINDS).is_ok_and(|normal| normal == path)
        && wish_asset::is_document(path)
        && !path.starts_with(wish_asset::index::EXPORTS_FOLDER)
}

/// A checkpoint that cannot be used, and why.
fn damaged(name: &str, why: &str) -> Diagnostic {
    Diagnostic::error(
        "checkpoint_damaged",
        format!("Checkpoint '{name}' cannot be used: {why}. Nothing was brought back."),
        "Delete it (checkpoint action \"delete\") and save a new one. The engine writes a checkpoint's files; one that names other paths was changed outside it.",
    )
    .at_path("/name")
}

/// The documents checkpoint `name` keeps, as its description lists them,
/// every path checked ([`keepable`]).
fn kept_documents(manifest: &Value, name: &str) -> Result<Vec<String>> {
    let mut kept: Vec<String> = Vec::new();
    let listed = match &manifest["documents"] {
        Value::Null => return Ok(kept),
        Value::Array(listed) => listed,
        _ => return Err(damaged(name, "its list of documents is not a list")),
    };
    for entry in listed {
        let Some(path) = entry.as_str() else {
            return Err(damaged(
                name,
                "its list of documents holds something that is not a path",
            ));
        };
        if !keepable(path) {
            return Err(damaged(
                name,
                &format!(
                    "it names '{path}', which is not a scene, prefab, material or meta file inside the project"
                ),
            ));
        }
        if !kept.iter().any(|known| known == path) {
            kept.push(path.to_owned());
        }
    }
    Ok(kept)
}

fn unreadable(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unreadable",
        format!("Could not read '{}': {error}.", path.display()),
        "Check that the project's .wish folder can be read.",
    )
}

impl Workspace {
    fn checkpoint_dir(&self, name: &str) -> PathBuf {
        self.root().join(CHECKPOINTS).join(name)
    }

    /// The checkpoints, oldest first: their manifests.
    pub(crate) fn checkpoints(&self) -> Vec<Value> {
        let mut all: Vec<Value> = std::fs::read_dir(self.root().join(CHECKPOINTS))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path().join("checkpoint.json")).ok()?;
                serde_json::from_str::<Value>(&text).ok()
            })
            .collect();
        all.sort_by_key(|manifest| manifest["number"].as_u64().unwrap_or(0));
        all
    }

    /// The checkpoint `name`'s manifest, or why there is none.
    fn checkpoint(&self, name: &str) -> Result<Value> {
        checked_name(name)?;
        let path = self.checkpoint_dir(name).join("checkpoint.json");
        let text = std::fs::read_to_string(&path).map_err(|_| {
            let names: Vec<String> = self
                .checkpoints()
                .iter()
                .filter_map(|m| m["name"].as_str().map(str::to_owned))
                .collect();
            Diagnostic::error(
                "checkpoint_not_found",
                format!("There is no checkpoint '{name}'."),
                if names.is_empty() {
                    "Save one first: checkpoint with action \"save\".".to_owned()
                } else {
                    format!("The checkpoints are: {}.", names.join(", "))
                },
            )
            .at_path("/name")
        })?;
        serde_json::from_str(&text).map_err(|error| {
            Diagnostic::error(
                "checkpoint_damaged",
                format!("Checkpoint '{name}' cannot be read: {error}."),
                "Delete it (checkpoint action \"delete\") and save a new one.",
            )
        })
    }

    /// Saves the project's documents and settings as checkpoint `name`
    /// (a new name, unless `overwrite`). Returns its description and
    /// warnings about documents it could not keep (a material file larger
    /// than any material can be).
    pub(crate) fn save_checkpoint(
        &mut self,
        name: Option<&str>,
        note: Option<&str>,
        actor: &Actor,
        overwrite: bool,
    ) -> Result<(Value, Vec<Diagnostic>)> {
        let existing = self.checkpoints();
        let number = existing
            .iter()
            .filter_map(|m| m["number"].as_u64())
            .max()
            .unwrap_or(0)
            + 1;
        let name = name.map_or_else(|| format!("cp_{number}"), str::to_owned);
        checked_name(&name)?;
        let dir = self.checkpoint_dir(&name);
        if dir.exists() && !overwrite {
            return Err(Diagnostic::error(
                "checkpoint_exists",
                format!("A checkpoint '{name}' exists already; it was not replaced."),
                "Choose another name, or pass \"overwrite\": true.",
            )
            .at_path("/name"));
        }
        if !dir.exists() && existing.len() >= MAX_CHECKPOINTS {
            return Err(Diagnostic::error(
                "too_many_checkpoints",
                format!("The project keeps at most {MAX_CHECKPOINTS} checkpoints."),
                "Delete old ones with checkpoint action \"delete\".",
            ));
        }
        // What is kept: every scene and prefab as the engine knows it now
        // (an open document's own text), material and meta files as they
        // are on disk, and the project files.
        let mut files: Vec<(String, String)> = Vec::new();
        let mut warnings = Vec::new();
        let mut documents: Vec<String> = Vec::new();
        for (path, file) in wish_asset::index::documents(self.root())? {
            let text = match self.document(&path) {
                Ok(open) => open.doc.to_canonical()?,
                Err(_) if wish_asset::is_document_asset(&path) => {
                    match wish_asset::read_document_asset(&file, &path) {
                        Ok(text) => text,
                        Err(problem) => {
                            if wish_asset::is_too_large(&problem) {
                                warnings.push(
                                    Diagnostic::warning(
                                        problem.code.clone(),
                                        format!(
                                            "{} It is not kept in the checkpoint.",
                                            problem.message
                                        ),
                                        problem.hint.clone(),
                                    )
                                    .at_file(path.clone()),
                                );
                            }
                            continue;
                        }
                    }
                }
                Err(_) => match std::fs::read_to_string(&file) {
                    Ok(text) => text,
                    Err(_) => continue,
                },
            };
            documents.push(path.clone());
            files.push((path, text));
        }
        for path in PROJECT_FILES {
            if let Ok(text) = std::fs::read_to_string(self.root().join(path)) {
                files.push(((*path).to_owned(), text));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in &files {
            crate::documents::write_project_file(&dir, &format!("files/{path}"), text)?;
        }
        let manifest = json!({
            "name": name,
            "number": number,
            "note": note,
            "by": actor.label(),
            "documents": documents,
            "files": files.len(),
        });
        crate::documents::write_project_file(&dir, "checkpoint.json", &format!("{manifest:#}\n"))?;
        Ok((manifest, warnings))
    }

    /// The folder that holds checkpoint `name`'s copies of the project's
    /// files (at their project paths).
    pub(crate) fn checkpoint_files(&self, name: &str) -> PathBuf {
        self.checkpoint_dir(name).join("files")
    }

    /// The text checkpoint `name` kept of `path` (a checked path), or
    /// `None` when it holds none. The copy must really be inside the
    /// checkpoint's folder (not reached through a link); a material file
    /// is read with its size cap.
    fn kept_text(&self, name: &str, path: &str) -> Result<Option<String>> {
        let folder = self.checkpoint_files(name);
        let file = folder.join(path);
        if !file.is_file() {
            return Ok(None);
        }
        let inside = std::fs::canonicalize(&file)
            .ok()
            .zip(std::fs::canonicalize(&folder).ok())
            .is_some_and(|(file, folder)| file.starts_with(folder));
        if !inside {
            return Err(damaged(
                name,
                &format!("its copy of '{path}' leads outside it"),
            ));
        }
        let text = if wish_asset::is_document_asset(path) {
            wish_asset::read_document_asset(&file, path)?
        } else {
            std::fs::read_to_string(&file).map_err(|error| unreadable(&file, &error))?
        };
        Ok(Some(text))
    }

    /// The prefabs as checkpoint `name` kept them (`kept`: its checked
    /// list of documents), with the prefabs made since as they are now:
    /// what the checkpoint's scenes are built from.
    fn checkpoint_prefabs(&mut self, name: &str, kept: &[String]) -> Result<Arc<PrefabLibrary>> {
        let mut library = (*self.prefab_library()?).clone();
        for path in kept.iter().filter(|path| path.ends_with(".prefab.json")) {
            library.remove(path);
            let Some(text) = self.kept_text(name, path)? else {
                continue;
            };
            let loaded = wish_scene::load_prefab(&text, path, self.registry());
            let failed = loaded.has_errors();
            if let Some(prefab) = loaded.document
                && !failed
            {
                library.insert(path, prefab);
            }
        }
        Ok(Arc::new(library))
    }

    /// The document `path` as checkpoint `name` kept it, its instances
    /// built from the prefabs the checkpoint kept.
    pub(crate) fn checkpoint_document(&mut self, name: &str, path: &str) -> Result<SceneDocument> {
        let manifest = self.checkpoint(name)?;
        let kept = kept_documents(&manifest, name)?;
        let text = self.kept_text(name, path)?.ok_or_else(|| {
            Diagnostic::error(
                "checkpoint_lacks_document",
                format!(
                    "Checkpoint '{name}' has no '{path}' (it was made before the file existed)."
                ),
                "Pick a later checkpoint, or another scene.",
            )
        })?;
        let prefabs = self.checkpoint_prefabs(name, &kept)?;
        self.load_detached(&text, path, &prefabs)
    }

    /// Brings every scene and prefab back to checkpoint `name`, each by one
    /// transaction by `actor` (undoable), and the material, meta and
    /// project files as their text, keeping what they replace in the
    /// project's trash (`.wish/trash/checkpoints/<name>-<n>/files/`).
    ///
    /// All or nothing as far as it can be: every path the checkpoint names
    /// is checked, every kept document read and every scene and prefab
    /// loaded (with the prefabs the checkpoint kept) before anything is
    /// written. A write that still fails half-way says what was already
    /// brought back (`checkpoint_restore_incomplete`).
    pub(crate) fn restore_checkpoint(
        &mut self,
        name: &str,
        actor: &Actor,
        dry_run: bool,
    ) -> Result<Value> {
        let manifest = self.checkpoint(name)?;
        let mut kept = kept_documents(&manifest, name)?;
        // Prefabs first: the scenes' instances are built from them.
        kept.sort_by_key(|path| !path.ends_with(".prefab.json"));
        // Everything is read, loaded and checked before anything is written.
        let prefabs = self.checkpoint_prefabs(name, &kept)?;
        let mut plan: Vec<(String, String, Option<SceneDocument>)> = Vec::new();
        for path in &kept {
            crate::documents::inside_project(self.root(), path)?;
            let text = self.kept_text(name, path)?.ok_or_else(|| {
                damaged(name, &format!("it lists '{path}' but holds no copy of it"))
            })?;
            let target = if is_scene_document(path) {
                let target = self.load_detached(&text, path, &prefabs)?;
                if self.root().join(path).is_file() {
                    // A file someone is half-way through editing is not
                    // replaced, before or after the others.
                    self.open_document(path, false)?;
                    self.refuse_if_file_invalid(path)?;
                }
                Some(target)
            } else {
                None
            };
            plan.push((path.clone(), text, target));
        }
        let mut settings_plan: Vec<(&str, String, Option<String>)> = Vec::new();
        for path in PROJECT_FILES {
            let Ok(text) = std::fs::read_to_string(self.checkpoint_files(name).join(path)) else {
                continue;
            };
            crate::documents::inside_project(self.root(), path)?;
            let current = std::fs::read_to_string(self.root().join(path)).ok();
            if current.as_deref() != Some(text.as_str()) {
                settings_plan.push((path, text, current));
            }
        }

        let mut progress = Progress::default();
        let outcome =
            self.restore_planned(name, actor, dry_run, &plan, &settings_plan, &mut progress);
        if !progress.settings.is_empty() && !dry_run {
            self.reload_config();
        }
        if let Err(problem) = outcome {
            if progress.written.is_empty() {
                return Err(problem);
            }
            let mut incomplete = Diagnostic::error(
                "checkpoint_restore_incomplete",
                format!(
                    "Going back to checkpoint '{name}' stopped: {} Already brought back: {}.",
                    problem.message,
                    progress.written.join(", ")
                ),
                format!(
                    "{} What was brought back stays: scenes and prefabs can be undone in their history{}. Restore again once the problem is fixed.",
                    problem.hint,
                    progress
                        .trash
                        .as_ref()
                        .map(|folder| format!(", and the files written over are kept in {folder}"))
                        .unwrap_or_default()
                ),
            );
            incomplete.location = problem.location.clone();
            return Err(incomplete);
        }

        // Documents made after the checkpoint are left as they are.
        let now: Vec<String> = wish_asset::index::documents(self.root())?
            .into_iter()
            .map(|(name, _)| name)
            .filter(|path| !kept.contains(path))
            .collect();
        // A material file brought back may name a texture that has gone
        // since (moved or deleted): said, not hidden.
        let mut warnings = Vec::new();
        if !dry_run {
            let index = wish_asset::AssetIndex::scan(self.root())?;
            for (path, text, _) in &plan {
                if !wish_asset::is_material(path) || !progress.written.contains(path) {
                    continue;
                }
                let Ok(material) = wish_asset::parse_material(text, path) else {
                    continue;
                };
                for used in material.textures() {
                    if let Err(problem) = index.check(&used.reference) {
                        warnings.push(
                            Diagnostic::warning(
                                problem.code.clone(),
                                format!(
                                    "{} The material file '{path}' brought back names it.",
                                    problem.message
                                ),
                                problem.hint.clone(),
                            )
                            .at_file(path.clone())
                            .at_path(used.pointer.clone()),
                        );
                    }
                }
            }
        }
        let mut answer = json!({
            "checkpoint": name,
            "restored": progress.restored,
            "unchanged": progress.unchanged,
            "kept_newer": now,
            "settings_restored": progress.settings,
        });
        if let Some(folder) = progress.trash {
            answer["replaced_kept_in"] = json!(folder);
        }
        if !warnings.is_empty() {
            answer["diagnostics"] = json!(warnings);
        }
        Ok(answer)
    }

    /// The writing half of [`Self::restore_checkpoint`]: `plan` (each kept
    /// document's path, text and, for a scene or prefab, the document it
    /// is brought back to) and `settings` (the project files that differ:
    /// path, kept text, current text), in order. What is done is in
    /// `progress`, also when a step fails.
    fn restore_planned(
        &mut self,
        name: &str,
        actor: &Actor,
        dry_run: bool,
        plan: &[(String, String, Option<SceneDocument>)],
        settings: &[(&str, String, Option<String>)],
        progress: &mut Progress,
    ) -> Result<()> {
        let reason = format!("back to checkpoint {name}");
        // What a dry run says it would write, and what a restore wrote.
        let written_back = |path: &str| {
            if dry_run {
                json!({ "path": path, "would_write_back": true })
            } else {
                json!({ "path": path, "written_back": true })
            }
        };
        for (path, text, target) in plan {
            let exists = self.root().join(path).is_file();
            let Some(target) = target else {
                // A material, sheet or meta file has no operations and no history
                // of its own: it comes back as the text it was, and the
                // text it replaces is kept.
                let file = self.root().join(path);
                let current = if wish_asset::is_document_asset(path) {
                    wish_asset::read_document_asset(&file, path).ok()
                } else {
                    std::fs::read_to_string(&file).ok()
                };
                if current.as_deref() == Some(text.as_str()) {
                    progress.unchanged.push(json!(path));
                    continue;
                }
                if !dry_run {
                    if exists {
                        self.keep_replaced(name, path, progress)?;
                    }
                    crate::documents::write_project_file(self.root(), path, text)?;
                    progress.written.push(path.clone());
                }
                progress.restored.push(written_back(path));
                continue;
            };
            if !exists {
                // A document deleted since: written back as it was.
                if !dry_run {
                    crate::documents::write_project_file(self.root(), path, text)?;
                    progress.written.push(path.clone());
                }
                progress.restored.push(written_back(path));
                continue;
            }
            // Read again: a prefab brought back before it is in the library.
            self.open_document(path, false)?;
            if dry_run {
                if self.document(path)?.doc.to_value() == target.to_value() {
                    progress.unchanged.push(json!(path));
                } else {
                    progress.restored.push(json!({ "path": path }));
                }
                continue;
            }
            match self.make_document_match(path, target, actor, &reason)? {
                Some(revision) => {
                    progress.written.push(path.clone());
                    progress
                        .restored
                        .push(json!({ "path": path, "revision": revision }));
                }
                None => progress.unchanged.push(json!(path)),
            }
        }
        for (path, text, current) in settings {
            if !dry_run {
                if current.is_some() {
                    self.keep_replaced(name, path, progress)?;
                }
                crate::documents::write_project_file(self.root(), path, text)?;
                progress.written.push((*path).to_owned());
            }
            progress.settings.push(json!(path));
        }
        Ok(())
    }

    /// Keeps a copy of the project file `path`, which going back to
    /// checkpoint `name` is about to write over, in this restore's folder
    /// of the project's trash (made at the first file it keeps).
    fn keep_replaced(&self, name: &str, path: &str, progress: &mut Progress) -> Result<()> {
        let folder = match &progress.trash {
            Some(folder) => folder.clone(),
            None => {
                let number = (1u64..)
                    .find(|n| !self.root().join(format!("{TRASH}/{name}-{n}")).exists())
                    .unwrap_or(0);
                format!("{TRASH}/{name}-{number}")
            }
        };
        let kept = format!("{folder}/files/{path}");
        let target = self.root().join(&kept);
        let copied = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::copy(self.root().join(path), &target));
        if let Err(error) = copied {
            return Err(Diagnostic::error(
                "file_unwritable",
                format!("Could not keep a copy of '{path}' in '{kept}': {error}. It was not written over."),
                "Check that the project's .wish folder can be written to, and that the disk is not full.",
            )
            .at_file(path));
        }
        progress.trash = Some(folder);
        Ok(())
    }

    /// Deletes checkpoint `name` (the documents are not touched).
    pub(crate) fn delete_checkpoint(&mut self, name: &str) -> Result<()> {
        self.checkpoint(name)?;
        std::fs::remove_dir_all(self.checkpoint_dir(name)).map_err(|error| {
            Diagnostic::error(
                "file_unwritable",
                format!("Could not delete checkpoint '{name}': {error}."),
                "Close programs that use the project's .wish folder, and try again.",
            )
        })
    }
}
