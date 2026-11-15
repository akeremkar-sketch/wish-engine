//! Prefabs: groups of entities defined once and placed many times.
//!
//! A prefab file (`*.prefab.json`) has the same format as a scene, with an
//! entity called `root` at the top and no resources. A scene places it with
//! an *instance*: an entity whose `prefab` names the file. The instance's
//! `components` patch the prefab root's components; `overrides` patch the
//! other prefab entities':
//!
//! ```json
//! "enemy_1": {
//!   "prefab": "prefabs/enemy.prefab.json",
//!   "components": { "Transform": { "translation": [4, 0, 0] } },
//!   "overrides": { "weapon": { "components": { "Damage": { "amount": 20 } } } }
//! }
//! ```
//!
//! When the scene is expanded ([`SceneDocument::expand`]), the instance
//! becomes the prefab root, and each other prefab entity `weapon` becomes
//! `enemy_1/weapon`. Scene ids cannot contain `/`, so these never collide
//! with ids written in the scene.
//!
//! Prefabs are one level deep for now: a prefab cannot place another prefab.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;

use crate::document::{FileKind, LoadResult, SceneDocument, SceneEntity, load_with};
use crate::schema_check::merge;

/// The id of a prefab's top entity.
pub const PREFAB_ROOT: &str = "root";

/// The prefabs a scene can place, by project-relative path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefabLibrary {
    prefabs: BTreeMap<String, SceneDocument>,
}

impl PrefabLibrary {
    /// An empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a prefab. The document should come from
    /// [`load_prefab`], which checks the prefab rules; anything outside the
    /// root is not placed by [`SceneDocument::expand`].
    pub fn insert(&mut self, path: &str, prefab: SceneDocument) {
        self.prefabs.insert(path.to_owned(), prefab);
    }

    /// Removes a prefab.
    pub fn remove(&mut self, path: &str) -> Option<SceneDocument> {
        self.prefabs.remove(path)
    }

    /// One prefab.
    pub fn get(&self, path: &str) -> Option<&SceneDocument> {
        self.prefabs.get(path)
    }

    /// The paths of all prefabs, sorted.
    pub fn paths(&self) -> Vec<&str> {
        self.prefabs.keys().map(String::as_str).collect()
    }

    /// Whether the library holds no prefabs.
    pub fn is_empty(&self) -> bool {
        self.prefabs.is_empty()
    }
}

/// Reads and validates a prefab file: a scene file with an entity `root`
/// that has no parent, every other entity below it, no resources and no
/// prefab instances. Problems are reported like [`crate::load`]'s.
pub fn load_prefab(source: &str, file: &str, registry: &Registry) -> LoadResult {
    load_with(source, file, registry, Arc::default(), FileKind::Prefab)
}

/// One entity of an expanded scene: a scene entity, or an entity that came
/// from a prefab, with its final components.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedEntity {
    /// Its id: a scene id, or `instance/prefab_entity` for prefab entities.
    pub id: String,
    /// Its parent's id.
    pub parent: Option<String>,
    /// Its sibling order.
    pub order: i64,
    /// Its components, by name, as patches on each type's defaults, with
    /// the prefab's values and the scene's changes merged.
    pub components: BTreeMap<String, Value>,
}

impl SceneDocument {
    /// Every entity of the scene with prefab instances expanded, in a
    /// stable order: parents before their children, siblings in sibling
    /// order, and each instance's prefab entities right after it.
    pub fn expand(&self) -> Result<Vec<ExpandedEntity>> {
        let mut out = Vec::with_capacity(self.entities.len());
        let mut queue: Vec<String> = self.roots().into_iter().map(str::to_owned).collect();
        queue.reverse();
        while let Some(id) = queue.pop() {
            let Some(entity) = self.entities.get(&id) else {
                continue;
            };
            match &entity.prefab {
                None => out.push(ExpandedEntity {
                    id: id.clone(),
                    parent: entity.parent.clone(),
                    order: entity.order,
                    components: entity.components.clone(),
                }),
                Some(path) => self.expand_instance(&id, path, entity, &mut out)?,
            }
            let mut children: Vec<String> =
                self.children(&id).into_iter().map(str::to_owned).collect();
            children.reverse();
            queue.extend(children);
        }
        Ok(out)
    }

    fn expand_instance(
        &self,
        id: &str,
        path: &str,
        entity: &SceneEntity,
        out: &mut Vec<ExpandedEntity>,
    ) -> Result<()> {
        let prefab = self.prefabs.get(path).ok_or_else(|| {
            Diagnostic::error(
                "prefab_not_found",
                format!("Entity '{id}' places the prefab '{path}', which is not loaded."),
                "Check the path, or load the prefab file before expanding the scene.",
            )
        })?;
        let mut root_components = prefab
            .entity(PREFAB_ROOT)
            .map(|root| root.components.clone())
            .unwrap_or_default();
        merge_components(&mut root_components, &entity.components);
        out.push(ExpandedEntity {
            id: id.to_owned(),
            parent: entity.parent.clone(),
            order: entity.order,
            components: root_components,
        });
        // Prefab entities below the root, parents first.
        let mut queue: Vec<String> = prefab
            .children(PREFAB_ROOT)
            .into_iter()
            .map(str::to_owned)
            .collect();
        queue.reverse();
        while let Some(inner) = queue.pop() {
            let Some(inner_entity) = prefab.entity(&inner) else {
                continue;
            };
            let mut components = inner_entity.components.clone();
            if let Some(patches) = entity.overrides.get(&inner) {
                merge_components(&mut components, patches);
            }
            let parent = match inner_entity.parent.as_deref() {
                Some(PREFAB_ROOT) | None => id.to_owned(),
                Some(parent) => format!("{id}/{parent}"),
            };
            out.push(ExpandedEntity {
                id: format!("{id}/{inner}"),
                parent: Some(parent),
                order: inner_entity.order,
                components,
            });
            let mut children: Vec<String> = prefab
                .children(&inner)
                .into_iter()
                .map(str::to_owned)
                .collect();
            children.reverse();
            queue.extend(children);
        }
        Ok(())
    }
}

/// Merges component patches into a set of components.
pub(crate) fn merge_components(
    target: &mut BTreeMap<String, Value>,
    patches: &BTreeMap<String, Value>,
) {
    for (name, patch) in patches {
        match target.get_mut(name) {
            Some(existing) => merge(existing, patch),
            None => {
                target.insert(name.clone(), patch.clone());
            }
        }
    }
}
