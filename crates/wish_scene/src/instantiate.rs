//! Turning a scene document into entities of a [`World`].
//!
//! The document stays the source of truth; the world is what runs. Loading
//! is deterministic: entities are created parents first, siblings in sibling
//! order, so one document always gives the same world (and world hash).
//! Prefab instances are expanded on the way (see [`SceneDocument::expand`]).

use std::collections::BTreeMap;

use serde_json::json;
use wish_core::{Diagnostic, Entity, Result, Text};
use wish_ecs::World;
use wish_schema::component;

/// The scene id an entity was created from, such as `"player"` or
/// `"enemy_1/weapon"`. It lets agents and tools address entities of a
/// running scene by the same names the scene file uses.
///
/// Added by the engine when a scene is loaded; scene files cannot write it.
#[component]
pub struct SceneId {
    /// The id.
    pub id: Text,
}

/// The name of the [`SceneId`] component.
pub const SCENE_ID: &str = "SceneId";

/// The entities a scene produced, by scene id (including derived ids such as
/// `enemy_1/weapon`).
pub type SceneEntities = BTreeMap<String, Entity>;

/// Registers the types the scene loader adds itself ([`SceneId`]). Call it
/// once before [`crate::SceneDocument::instantiate`].
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<SceneId>()?;
    Ok(())
}

impl crate::SceneDocument {
    /// Creates the scene's entities and resources in `world`.
    ///
    /// Every entity gets a [`SceneId`] and the components the scene (and its
    /// prefabs) give it; the hierarchy follows the scene's `parent` and
    /// `order`. Resources the scene sets are written (a patch on the
    /// resource's current value or default).
    ///
    /// Not transactional: if a value is rejected the world may hold part of
    /// the scene. A document that was loaded with [`crate::load`] against the
    /// same registered types does not fail here.
    pub fn instantiate(&self, world: &mut World) -> Result<SceneEntities> {
        let expanded = self.expand()?;
        let mut entities = SceneEntities::new();
        for item in &expanded {
            let entity = world.spawn()?;
            world.set_json(entity, SCENE_ID, &json!({ "id": item.id }))?;
            for (name, value) in &item.components {
                world.set_json(entity, name, value).map_err(|problem| {
                    problem.at_path(format!("/entities/{}/components/{name}", item.id))
                })?;
            }
            entities.insert(item.id.clone(), entity);
        }
        // Parents come before their children in `expanded`, and siblings in
        // sibling order, so attaching in this order keeps the order.
        for item in &expanded {
            let Some(parent) = &item.parent else {
                continue;
            };
            let (Some(&child), Some(&parent_entity)) =
                (entities.get(&item.id), entities.get(parent))
            else {
                return Err(Diagnostic::error(
                    "internal_error",
                    format!(
                        "Entity '{}' lost its parent '{parent}' while loading.",
                        item.id
                    ),
                    "This is a bug in wish_scene. Please report it.",
                ));
            };
            world.set_parent(child, parent_entity)?;
        }
        for (name, value) in self.resources() {
            world
                .set_resource_json(name, value)
                .map_err(|problem| problem.at_path(format!("/resources/{name}")))?;
        }
        Ok(entities)
    }
}
