//! Open scene documents: loading, writing, and keeping them in step with the
//! files (the files are the source of truth, invariant 3).
//!
//! - A document is opened the first time a tool touches its scene, and every
//!   change is written to disk at once, in canonical form, atomically.
//! - Before each use the file is read again. If someone changed it outside
//!   the engine (a person in a text editor, git), the difference is turned
//!   into a transaction by [`Actor::File`] and applied, so it lands in the
//!   same history as everyone else's edits and can be undone. A file that is
//!   now invalid leaves the last good document in place, and changes are
//!   refused until the file is fixed: an agent must never overwrite a
//!   person's half-finished edit.
//! - Tools that read values (`entity_query`, `entity_get`) look at the
//!   document instantiated into a world ([`WorldView`]): prefabs expanded,
//!   defaults filled in, global transforms computed. Views are cached per
//!   revision.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::{Value, json};
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::World;
use wish_scene::{Actor, EditOp, PrefabLibrary, SceneDocument, Transaction};

use crate::workspace::Workspace;

/// A scene document as the engine sees it.
pub(crate) struct OpenDocument {
    /// The document.
    pub doc: SceneDocument,
    /// The file's text as last read or written: what "unchanged" means.
    disk_text: Option<String>,
    /// Set while the file on disk is invalid; changes are refused.
    pub disk_problem: Option<Diagnostic>,
    view: Option<(u64, WorldView)>,
}

/// A document instantiated into a world.
pub(crate) struct WorldView {
    pub world: World,
    /// Scene id → entity, sorted by id.
    pub ids: BTreeMap<String, Entity>,
}

/// Every open document, and the prefabs they share.
#[derive(Default)]
pub(crate) struct Documents {
    open: BTreeMap<String, OpenDocument>,
    prefabs: Arc<PrefabLibrary>,
    /// (path, length, modified) of each prefab file when last loaded.
    prefab_stamp: Option<Vec<(String, u64, Option<SystemTime>)>>,
}

fn atomic_write(root: &Path, path: &str, text: &str) -> Result<()> {
    let target = root.join(path);
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder).map_err(|error| unwritable(path, &error))?;
    }
    let temporary = target.with_extension("json.wish-tmp");
    std::fs::write(&temporary, text).map_err(|error| unwritable(path, &error))?;
    std::fs::rename(&temporary, &target).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        unwritable(path, &error)
    })
}

fn unwritable(path: &str, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unwritable",
        format!("Could not write '{path}': {error}."),
        "Check that the project folder can be written to and the file is not locked by another program.",
    )
    .at_file(path)
}

fn read_text(root: &Path, path: &str) -> Result<Option<String>> {
    match std::fs::read_to_string(root.join(path)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{path}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
        .at_file(path)),
    }
}

/// A scene path as the tools write it: project-relative, forward slashes,
/// no leading `./`, a `.scene.json` file inside the project.
pub(crate) fn normalize_scene_path(path: &str) -> Result<String> {
    let mut path = path.trim().replace('\\', "/");
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_owned();
    }
    let outside = path.starts_with('/')
        || path.contains(':')
        || path.split('/').any(|part| part == ".." || part.is_empty());
    if outside || !(path.ends_with(".scene.json") || path.ends_with(".prefab.json")) {
        return Err(Diagnostic::error(
            "invalid_scene_path",
            format!("'{path}' is not a scene file path inside the project."),
            "Give a project-relative path ending in .scene.json (or .prefab.json to look at a prefab), such as \"scenes/main.scene.json\".",
        )
        .at_path("/scene"));
    }
    Ok(path)
}

fn load_document(
    text: &str,
    path: &str,
    workspace_registry: &wish_ecs::Registry,
    prefabs: &Arc<PrefabLibrary>,
) -> Result<(SceneDocument, Vec<Diagnostic>)> {
    // Prefabs open read-only, through the prefab loader (it checks the root).
    let result = if path.ends_with(".prefab.json") {
        wish_scene::load_prefab(text, path, workspace_registry)
    } else {
        wish_scene::load_with_prefabs(text, path, workspace_registry, Arc::clone(prefabs))
    };
    let errors = result.diagnostics.iter().filter(|d| d.is_error()).count();
    match result.document {
        Some(document) if errors == 0 => Ok((document, result.diagnostics)),
        _ => {
            let mut first = result
                .diagnostics
                .into_iter()
                .find(Diagnostic::is_error)
                .unwrap_or_else(|| {
                    Diagnostic::error(
                        "scene_invalid",
                        format!("'{path}' could not be loaded."),
                        "Run `check` to see what is wrong.",
                    )
                });
            if errors > 1 {
                first.message = format!(
                    "{} ({} more error(s) in this file; `check` lists them all.)",
                    first.message,
                    errors - 1
                );
            }
            Err(first)
        }
    }
}

/// The edits that turn `old` into `new`, made by a file change.
fn diff(old: &SceneDocument, new: &SceneDocument) -> Vec<EditOp> {
    let same_kind = |id: &str| matches!((old.entity(id), new.entity(id)), (Some(a), Some(b)) if a.prefab() == b.prefab());
    let kept: BTreeSet<&str> = new
        .entities()
        .map(|(id, _)| id)
        .filter(|id| same_kind(id))
        .collect();
    // Parents first, so a new child's parent exists when it is spawned.
    let depth = |doc: &SceneDocument, id: &str| {
        let mut depth = 0;
        let mut at = doc.entity(id).and_then(|e| e.parent());
        while let Some(parent) = at {
            depth += 1;
            if depth > 10_000 {
                break;
            }
            at = doc.entity(parent).and_then(|e| e.parent());
        }
        depth
    };
    let mut new_order: Vec<&str> = new.entities().map(|(id, _)| id).collect();
    new_order.sort_by_key(|id| (depth(new, id), id.to_string()));

    let mut ops = Vec::new();
    for id in &new_order {
        if kept.contains(id) {
            continue;
        }
        let Some(entity) = new.entity(id) else {
            continue;
        };
        let parent = entity.parent().map(str::to_owned);
        let components = entity.components().clone();
        let notes = entity.notes().map(str::to_owned);
        ops.push(match entity.prefab() {
            Some(prefab) => EditOp::Instantiate {
                id: (*id).to_owned(),
                prefab: prefab.to_owned(),
                parent,
                order: entity.order(),
                components,
                overrides: entity.overrides().clone(),
                notes,
            },
            None => EditOp::Spawn {
                id: (*id).to_owned(),
                parent,
                order: entity.order(),
                components,
                notes,
            },
        });
    }
    for id in &new_order {
        let (Some(before), Some(after)) = (old.entity(id), new.entity(id)) else {
            continue;
        };
        if kept.contains(id)
            && (before.parent() != after.parent() || before.order() != after.order())
        {
            ops.push(EditOp::Reparent {
                id: (*id).to_owned(),
                parent: after.parent().map(str::to_owned),
                order: Some(after.order()),
            });
        }
    }
    for (id, entity) in old.entities() {
        if kept.contains(id) {
            continue;
        }
        let parent_goes_too = entity
            .parent()
            .is_some_and(|parent| !kept.contains(parent) && old.entity(parent).is_some());
        if !parent_goes_too {
            ops.push(EditOp::Despawn { id: id.to_owned() });
        }
    }
    for id in &new_order {
        if !kept.contains(id) {
            continue;
        }
        let (Some(before), Some(after)) = (old.entity(id), new.entity(id)) else {
            continue;
        };
        for (name, value) in after.components() {
            if before.components().get(name) != Some(value) {
                ops.push(EditOp::Replace {
                    id: (*id).to_owned(),
                    component: name.clone(),
                    value: value.clone(),
                });
            }
        }
        for name in before.components().keys() {
            if !after.components().contains_key(name) {
                ops.push(EditOp::Remove {
                    id: (*id).to_owned(),
                    component: name.clone(),
                });
            }
        }
        for (entity, patches) in after.overrides() {
            for (component, value) in patches {
                let earlier = before
                    .overrides()
                    .get(entity)
                    .and_then(|p| p.get(component));
                if earlier != Some(value) {
                    ops.push(EditOp::ReplaceOverride {
                        id: (*id).to_owned(),
                        entity: entity.clone(),
                        component: component.clone(),
                        value: value.clone(),
                    });
                }
            }
        }
        for (entity, patches) in before.overrides() {
            for component in patches.keys() {
                let still = after
                    .overrides()
                    .get(entity)
                    .is_some_and(|p| p.contains_key(component));
                if !still {
                    ops.push(EditOp::RemoveOverride {
                        id: (*id).to_owned(),
                        entity: entity.clone(),
                        component: component.clone(),
                    });
                }
            }
        }
        if before.notes() != after.notes() {
            ops.push(EditOp::SetNotes {
                id: (*id).to_owned(),
                notes: after.notes().map(str::to_owned),
            });
        }
    }
    for (name, value) in new.resources() {
        if old.resources().get(name) != Some(value) {
            ops.push(EditOp::ReplaceResource {
                name: name.clone(),
                value: value.clone(),
            });
        }
    }
    for name in old.resources().keys() {
        if !new.resources().contains_key(name) {
            ops.push(EditOp::RemoveResource { name: name.clone() });
        }
    }
    ops
}

/// The entity ids (and `resource:<name>`s) a list of change pointers touch.
pub(crate) fn touched(changes: &[String]) -> BTreeSet<String> {
    changes
        .iter()
        .filter_map(|pointer| {
            let mut parts = pointer.trim_start_matches('/').split('/');
            match (parts.next(), parts.next()) {
                (Some("entities"), Some(id)) => Some(id.replace("~1", "/").replace("~0", "~")),
                (Some("resources"), Some(name)) => Some(format!("resource:{name}")),
                _ => None,
            }
        })
        .collect()
}

impl Workspace {
    /// The scene a tool works on: `scene` if given, else the project's
    /// default.
    pub(crate) fn scene_path(&self, scene: Option<&str>) -> Result<String> {
        match scene {
            Some(path) => normalize_scene_path(path),
            None => match self.config() {
                Some(config) => normalize_scene_path(&config.scene),
                None => Err(Diagnostic::error(
                    "no_default_scene",
                    "No scene was named and the project has no default scene (no wish.json).",
                    "Pass \"scene\": \"scenes/main.scene.json\", or create a project with `wen new`.",
                )
                .at_path("/scene")),
            },
        }
    }

    fn prefab_stamp(&self) -> Result<Vec<(String, u64, Option<SystemTime>)>> {
        Ok(wish_asset::index::documents(self.root())?
            .into_iter()
            .filter(|(name, _)| name.ends_with(".prefab.json"))
            .map(|(name, path)| {
                let meta = std::fs::metadata(&path).ok();
                let len = meta.as_ref().map_or(0, std::fs::Metadata::len);
                let modified = meta.and_then(|m| m.modified().ok());
                (name, len, modified)
            })
            .collect())
    }

    /// Reloads the prefab library if a prefab file changed.
    fn refresh_prefabs(&mut self) -> Result<()> {
        let stamp = self.prefab_stamp()?;
        if self.docs.prefab_stamp.as_ref() == Some(&stamp) {
            return Ok(());
        }
        let mut library = PrefabLibrary::new();
        for (name, path) in wish_asset::index::documents(self.root())?
            .into_iter()
            .filter(|(name, _)| name.ends_with(".prefab.json"))
        {
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let result = wish_scene::load_prefab(&source, &name, self.registry());
            let failed = result.has_errors();
            if let Some(document) = result.document
                && !failed
            {
                library.insert(&name, document);
            }
        }
        let library = Arc::new(library);
        for open in self.docs.open.values_mut() {
            open.doc.set_prefabs(Arc::clone(&library));
            open.view = None;
        }
        self.docs.prefabs = library;
        self.docs.prefab_stamp = Some(stamp);
        Ok(())
    }

    /// Opens `path` (or brings it up to date with the file), returning
    /// notes about what happened (a reload, a rejected file change).
    /// `create` makes a missing scene a new, empty document.
    pub(crate) fn open_document(&mut self, path: &str, create: bool) -> Result<Vec<Diagnostic>> {
        self.refresh_prefabs()?;
        let root = self.root().to_path_buf();
        let mut notes = Vec::new();
        let text = read_text(&root, path)?;
        if !self.docs.open.contains_key(path) {
            let (doc, warnings, disk_text) = match &text {
                Some(text) => {
                    let (doc, warnings) =
                        load_document(text, path, self.registry(), &self.docs.prefabs)?;
                    (doc, warnings, Some(text.clone()))
                }
                None if create => {
                    let mut doc = SceneDocument::new();
                    doc.set_prefabs(Arc::clone(&self.docs.prefabs));
                    (doc, Vec::new(), None)
                }
                None => return Err(self.scene_not_found(path)),
            };
            notes.extend(warnings.into_iter().filter(|w| w.code != "not_canonical"));
            self.docs.open.insert(
                path.to_owned(),
                OpenDocument {
                    doc,
                    disk_text,
                    disk_problem: None,
                    view: None,
                },
            );
            return Ok(notes);
        }
        let Some(open) = self.docs.open.get(path) else {
            return Ok(notes);
        };
        if text == open.disk_text {
            return Ok(notes);
        }
        let Some(text) = text else {
            notes.push(
                Diagnostic::warning(
                    "scene_file_missing",
                    format!(
                        "'{path}' was deleted outside the engine; the scene is kept in memory."
                    ),
                    "The next change writes it again. To really remove the scene, leave it alone.",
                )
                .at_file(path),
            );
            return Ok(notes);
        };
        // A file read halfway through being written fails to load; try once
        // more after a moment before calling it invalid.
        let (loaded, text) = match load_document(&text, path, self.registry(), &self.docs.prefabs) {
            Ok(loaded) => (Ok(loaded), text),
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let again = read_text(&root, path)?.unwrap_or_default();
                let loaded = load_document(&again, path, self.registry(), &self.docs.prefabs);
                (loaded, again)
            }
        };
        let registry = self.types.world().registry();
        let Some(open) = self.docs.open.get_mut(path) else {
            return Ok(notes);
        };
        match loaded {
            Err(problem) => {
                let mut note = Diagnostic::warning(
                    "external_change_rejected",
                    format!(
                        "'{path}' was changed outside the engine and is now invalid: {} The last valid scene is kept; changes are refused until the file is fixed.",
                        problem.message
                    ),
                    format!("Fix the file ({}), or ask the person editing it.", problem.hint),
                )
                .at_file(path);
                note.location = problem.location.clone();
                open.disk_problem = Some(note.clone());
                notes.push(note);
            }
            Ok((new_doc, _)) => {
                let ops = diff(&open.doc, &new_doc);
                open.disk_problem = None;
                if !ops.is_empty() {
                    let applied = open
                        .doc
                        .apply(&Transaction::new(Actor::File, ops), registry);
                    let matches = applied.is_ok() && open.doc.to_value() == new_doc.to_value();
                    if !matches {
                        // Could not express the change as edits (a rare
                        // reshuffle): take the file as it is.
                        let revision = open.doc.revision();
                        open.doc = new_doc;
                        notes.push(Diagnostic::info(
                            "history_reset",
                            format!("'{path}' changed outside the engine in a way that could not be recorded as edits; its undo history starts again (was at revision {revision})."),
                            "Nothing to do; the scene matches the file.",
                        ).at_file(path));
                    }
                    open.view = None;
                }
                open.disk_text = Some(text);
            }
        }
        Ok(notes)
    }

    fn scene_not_found(&self, path: &str) -> Diagnostic {
        let scenes: Vec<String> = wish_asset::index::documents(self.root())
            .unwrap_or_default()
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.ends_with(".scene.json"))
            .collect();
        let refs: Vec<&str> = scenes.iter().map(String::as_str).collect();
        let hint = match wish_core::suggest::closest_match(path, refs.iter().copied()) {
            Some(closest) => format!(
                "Did you mean '{closest}'? To create a new scene, call edit_apply with \"create\": true."
            ),
            None => "To create a new scene, call edit_apply with \"create\": true.".to_owned(),
        };
        Diagnostic::error(
            "scene_not_found",
            format!("The project has no scene '{path}'."),
            hint,
        )
        .at_path("/scene")
        .with_candidates(refs)
    }

    /// The open document at `path` (after [`Self::open_document`]).
    pub(crate) fn document(&self, path: &str) -> Result<&OpenDocument> {
        self.docs.open.get(path).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                format!("The scene '{path}' is not open."),
                "This is a bug in the engine. Please report it.",
            )
        })
    }

    /// The open document at `path`, for changing.
    pub(crate) fn document_mut(&mut self, path: &str) -> Result<&mut OpenDocument> {
        self.docs.open.get_mut(path).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                format!("The scene '{path}' is not open."),
                "This is a bug in the engine. Please report it.",
            )
        })
    }

    /// Writes the document at `path` to disk in canonical form.
    pub(crate) fn save_document(&mut self, path: &str) -> Result<()> {
        let root = self.root().to_path_buf();
        let open = self.document_mut(path)?;
        let text = open.doc.to_canonical()?;
        atomic_write(&root, path, &text)?;
        open.disk_text = Some(text);
        open.view = None;
        Ok(())
    }

    /// The document at `path` instantiated into a world (cached per
    /// revision).
    pub(crate) fn view(&mut self, path: &str) -> Result<&WorldView> {
        let example = self.example().map(str::to_owned);
        let open = self.document_mut(path)?;
        let revision = open.doc.revision();
        let fresh = open.view.as_ref().is_some_and(|(at, _)| *at == revision);
        if !fresh {
            let mut world = World::new();
            crate::types::register_project_types(example.as_deref(), &mut world)?;
            let ids = open.doc.instantiate(&mut world)?;
            wish_transform::update_global_transforms(&mut world)?;
            open.view = Some((revision, WorldView { world, ids }));
        }
        open.view.as_ref().map(|(_, view)| view).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                "A scene view could not be built.",
                "This is a bug in the engine. Please report it.",
            )
        })
    }

    /// What other actors changed in `path` since `actor` last looked, and
    /// marks everything up to now as seen by `actor`.
    pub(crate) fn changes_by_others(&mut self, path: &str, actor: &Actor) -> Option<Value> {
        let key = (path.to_owned(), actor.label());
        let since = self.seen.get(&key).copied();
        let open = self.docs.open.get(path)?;
        let now = open.doc.revision();
        self.seen.insert(key, now);
        let since = since?;
        let mut entities = BTreeSet::new();
        let mut by = BTreeSet::new();
        for entry in open
            .doc
            .log()
            .filter(|e| e.revision > since && &e.actor != actor)
        {
            entities.extend(touched(&entry.changes));
            by.insert(entry.actor.label());
        }
        if entities.is_empty() {
            return None;
        }
        let total = entities.len();
        let shown: Vec<String> = entities.into_iter().take(20).collect();
        let mut out = json!({ "by": by, "entities": shown });
        if total > 20 {
            out["more"] = json!(total - 20);
        }
        Some(out)
    }
}

/// A file path a tool may write to: project-relative, forward slashes, no
/// `..`, not a hidden or engine document (a picture must never overwrite a
/// scene).
pub(crate) fn normalize_output_path(path: &str) -> Result<String> {
    let mut path = path.trim().replace('\\', "/");
    while let Some(rest) = path.strip_prefix("./") {
        path = rest.to_owned();
    }
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part.starts_with('.'))
        || wish_asset::is_document(&path)
        || path == crate::project::PROJECT_FILE;
    if bad {
        return Err(Diagnostic::error(
            "invalid_output_path",
            format!("'{path}' is not a file the tool may write."),
            "Give a project-relative path such as \"captures/shot.png\" (not a scene, prefab, meta or project file).",
        )
        .at_path("/out"));
    }
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_paths_stay_inside_the_project() {
        assert_eq!(
            normalize_scene_path(".\\scenes\\a.scene.json").unwrap(),
            "scenes/a.scene.json"
        );
        for bad in [
            "../x.scene.json",
            "/abs.scene.json",
            "C:/x.scene.json",
            "a.json",
            "a//b.scene.json",
        ] {
            assert_eq!(
                normalize_scene_path(bad).unwrap_err().code,
                "invalid_scene_path",
                "{bad}"
            );
        }
    }

    #[test]
    fn change_pointers_name_entities_and_resources() {
        let touched = touched(&[
            "/entities/crate/components/Transform".to_owned(),
            "/entities/a~1b".to_owned(),
            "/resources/Sky".to_owned(),
        ]);
        assert_eq!(
            touched.into_iter().collect::<Vec<_>>(),
            ["a/b", "crate", "resource:Sky"]
        );
    }
}
