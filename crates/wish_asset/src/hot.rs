//! Hot reloading: noticing changed files and swapping a scene's content in a
//! running world.
//!
//! Changes are found by polling the project folder (modification time and
//! size), which behaves the same on every platform and needs no extra
//! dependency; a native notification backend can replace it behind
//! [`Watcher::poll`]. Polling reads the clock of the file system, never the
//! simulation's, and the world only changes inside [`HotScene::poll`],
//! between ticks, so determinism is untouched.
//!
//! A change that makes the project invalid never reaches the world: the last
//! good scene keeps running and the problems are reported.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use wish_core::{Diagnostic, Result};
use wish_ecs::World;
use wish_scene::{SceneDocument, SceneEntities};

use crate::index::walk_files;
use crate::project::load_project;

/// Notices which project files changed between two polls.
#[derive(Debug)]
pub struct Watcher {
    root: PathBuf,
    seen: BTreeMap<String, (Option<SystemTime>, u64)>,
}

impl Watcher {
    /// Starts watching `root`; the files present now count as unchanged.
    pub fn new(root: &Path) -> Result<Self> {
        let mut watcher = Self {
            root: root.to_path_buf(),
            seen: BTreeMap::new(),
        };
        watcher.seen = watcher.snapshot()?;
        Ok(watcher)
    }

    fn snapshot(&self) -> Result<BTreeMap<String, (Option<SystemTime>, u64)>> {
        Ok(walk_files(&self.root)?
            .into_iter()
            .filter_map(|(name, path)| {
                let metadata = fs::metadata(path).ok()?;
                Some((name, (metadata.modified().ok(), metadata.len())))
            })
            .collect())
    }

    /// The project-relative paths that were added, changed or removed since
    /// the last poll, sorted.
    pub fn poll(&mut self) -> Result<Vec<String>> {
        let now = self.snapshot()?;
        let mut changed: Vec<String> = now
            .iter()
            .filter(|(name, state)| self.seen.get(*name) != Some(state))
            .map(|(name, _)| name.clone())
            .collect();
        changed.extend(
            self.seen
                .keys()
                .filter(|name| !now.contains_key(*name))
                .cloned(),
        );
        changed.sort();
        self.seen = now;
        Ok(changed)
    }
}

/// What a reload did.
#[derive(Debug)]
pub struct Reload {
    /// The files that changed.
    pub changed: Vec<String>,
    /// Whether the world now shows the new content. `false` means the
    /// project has errors and the last good scene is still in place.
    pub applied: bool,
    /// Errors and warnings found.
    pub diagnostics: Vec<Diagnostic>,
    /// How many scene entities the world has now.
    pub entities: usize,
}

/// A scene loaded into a world that follows the files on disk.
#[derive(Debug)]
pub struct HotScene {
    root: PathBuf,
    scene: String,
    watcher: Watcher,
    entities: SceneEntities,
    document: SceneDocument,
}

impl HotScene {
    /// Loads `scene` (relative to `root`) into `world`. The world's registry
    /// must already have the component types. Fails with the project's first
    /// error if the project is not valid yet: there is no earlier good state.
    pub fn start(root: &Path, scene: &str, world: &mut World) -> Result<(Self, Vec<Diagnostic>)> {
        let watcher = Watcher::new(root)?;
        let mut project = load_project(root, world.registry())?;
        if let Some(error) = project.diagnostics.iter().find(|d| d.is_error()) {
            return Err(error.clone());
        }
        let document = project.scenes.remove(scene).ok_or_else(|| {
            let names: Vec<&str> = project.scenes.keys().map(String::as_str).collect();
            Diagnostic::error(
                "scene_not_found",
                format!("The project has no scene '{scene}'."),
                wish_core::suggest::unknown_name_hint(scene, &names),
            )
        })?;
        let entities = document.instantiate(world)?;
        Ok((
            Self {
                root: root.to_path_buf(),
                scene: scene.to_owned(),
                watcher,
                entities,
                document,
            },
            project.diagnostics,
        ))
    }

    /// The scene as last loaded.
    pub fn document(&self) -> &SceneDocument {
        &self.document
    }

    /// The scene's entities in the world, by scene id.
    pub fn entities(&self) -> &SceneEntities {
        &self.entities
    }

    /// Checks the files; if any changed, reloads the project and, when it is
    /// valid, replaces the scene's entities in `world` with the new ones.
    /// Returns `None` when nothing changed.
    ///
    /// The scene is replaced as a whole: entities the scene created are
    /// despawned and created again, so state they gained while running is
    /// lost. Entities the scene did not create are left alone.
    pub fn poll(&mut self, world: &mut World) -> Result<Option<Reload>> {
        let changed = self.watcher.poll()?;
        if changed.is_empty() {
            return Ok(None);
        }
        let mut project = load_project(&self.root, world.registry())?;
        let diagnostics = std::mem::take(&mut project.diagnostics);
        let new_document = if diagnostics.iter().any(Diagnostic::is_error) {
            None
        } else {
            project.scenes.remove(&self.scene)
        };
        let Some(document) = new_document else {
            let mut diagnostics = diagnostics;
            if !diagnostics.iter().any(Diagnostic::is_error) {
                diagnostics.push(Diagnostic::error(
                    "scene_not_found",
                    format!("The scene '{}' no longer exists.", self.scene),
                    "Restore the file; the last good version keeps running.",
                ));
            }
            return Ok(Some(Reload {
                changed,
                applied: false,
                diagnostics,
                entities: self.entities.len(),
            }));
        };
        for entity in self.entities.values() {
            // Despawning a parent removes its children; they may be gone.
            let _ = world.despawn(*entity);
        }
        self.entities = document.instantiate(world)?;
        self.document = document;
        Ok(Some(Reload {
            changed,
            applied: true,
            diagnostics,
            entities: self.entities.len(),
        }))
    }
}
