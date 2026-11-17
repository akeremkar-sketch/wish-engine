//! Checkpoints (M8b WP3): the project's scenes, prefabs and settings as they
//! were at a moment, kept under `.wish/checkpoints/<name>/`, to go back to
//! ("back to before the agent started") and to compare with (before /
//! after pictures).
//!
//! Going back never loses anything: each scene and prefab is brought back
//! by one transaction in its history (the operations that turn it back),
//! so the return can itself be undone, or taken back on its own.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::{Actor, SceneDocument};

use crate::workspace::Workspace;

/// Where checkpoints live, inside the project.
pub const CHECKPOINTS: &str = ".wish/checkpoints";
/// The project files a checkpoint keeps besides the documents.
const PROJECT_FILES: &[&str] = &[crate::project::PROJECT_FILE, wish_input::INPUT_FILE];
/// How many checkpoints a project keeps at most.
pub const MAX_CHECKPOINTS: usize = 200;

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
    /// (a new name, unless `overwrite`).
    pub(crate) fn save_checkpoint(
        &mut self,
        name: Option<&str>,
        note: Option<&str>,
        actor: &Actor,
        overwrite: bool,
    ) -> Result<Value> {
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
        // (an open document's own text), and the project files.
        let mut files: Vec<(String, String)> = Vec::new();
        let documents: Vec<String> = wish_asset::index::documents(self.root())?
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for path in &documents {
            let text = match self.document(path) {
                Ok(open) => open.doc.to_canonical()?,
                Err(_) => match std::fs::read_to_string(self.root().join(path)) {
                    Ok(text) => text,
                    Err(_) => continue,
                },
            };
            files.push((path.clone(), text));
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
        Ok(manifest)
    }

    /// The document `path` as checkpoint `name` kept it.
    pub(crate) fn checkpoint_document(&mut self, name: &str, path: &str) -> Result<SceneDocument> {
        self.checkpoint(name)?;
        let file = self.checkpoint_dir(name).join("files").join(path);
        let text = std::fs::read_to_string(&file).map_err(|_| {
            Diagnostic::error(
                "checkpoint_lacks_document",
                format!(
                    "Checkpoint '{name}' has no '{path}' (it was made before the file existed)."
                ),
                "Pick a later checkpoint, or another scene.",
            )
        })?;
        self.load_detached(&text, path)
    }

    /// Brings every scene and prefab back to checkpoint `name`, each by one
    /// transaction by `actor` (undoable), and the project files.
    pub(crate) fn restore_checkpoint(
        &mut self,
        name: &str,
        actor: &Actor,
        dry_run: bool,
    ) -> Result<Value> {
        let manifest = self.checkpoint(name)?;
        let dir = self.checkpoint_dir(name).join("files");
        let mut kept: Vec<String> = manifest["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect();
        // Prefabs first: the scenes' instances are built from them.
        kept.sort_by_key(|path| !path.ends_with(".prefab.json"));
        let mut restored = Vec::new();
        let mut unchanged = Vec::new();
        let reason = format!("back to checkpoint {name}");
        for path in &kept {
            let text = std::fs::read_to_string(dir.join(path))
                .map_err(|error| unreadable(&dir.join(path), &error))?;
            let target = self.load_detached(&text, path)?;
            let exists = self.root().join(path).is_file();
            if !exists && !dry_run {
                // A document deleted since: written back as it was.
                crate::documents::write_project_file(self.root(), path, &text)?;
                restored.push(json!({ "path": path, "written_back": true }));
                continue;
            }
            if !exists {
                restored.push(json!({ "path": path, "written_back": true }));
                continue;
            }
            self.open_document(path, false)?;
            if dry_run {
                let same = self.document(path)?.doc.to_value() == target.to_value();
                if same {
                    unchanged.push(json!(path));
                } else {
                    restored.push(json!({ "path": path }));
                }
                continue;
            }
            match self.make_document_match(path, &target, actor, &reason)? {
                Some(revision) => restored.push(json!({ "path": path, "revision": revision })),
                None => unchanged.push(json!(path)),
            }
        }
        // Documents made after the checkpoint are left as they are.
        let now: Vec<String> = wish_asset::index::documents(self.root())?
            .into_iter()
            .map(|(name, _)| name)
            .filter(|path| !kept.contains(path))
            .collect();
        let mut settings = Vec::new();
        for path in PROJECT_FILES {
            let Ok(text) = std::fs::read_to_string(dir.join(path)) else {
                continue;
            };
            let current = std::fs::read_to_string(self.root().join(path)).ok();
            if current.as_deref() != Some(text.as_str()) {
                if !dry_run {
                    crate::documents::write_project_file(self.root(), path, &text)?;
                }
                settings.push(json!(path));
            }
        }
        if !settings.is_empty() && !dry_run {
            self.reload_config();
        }
        Ok(json!({
            "checkpoint": name,
            "restored": restored,
            "unchanged": unchanged,
            "kept_newer": now,
            "settings_restored": settings,
        }))
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
