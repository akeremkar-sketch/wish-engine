//! Where entities are: local placement, and the world placement the
//! hierarchy turns it into.
//!
//! - [`Transform`] is what scenes and game code write: translation,
//!   rotation and scale relative to the parent.
//! - [`GlobalTransform`] is what the engine derives: the entity's matrix in
//!   world space. Nothing but [`update_global_transforms`] writes it, and
//!   scene files cannot.
//!
//! Conventions (ADR-021): right-handed, +Y up, a camera or light looks along
//! its -Z axis; meters and radians.
//!
//! Propagation is deterministic: entities are visited parents first, roots
//! in entity order, children in sibling order.
//!
//! The components themselves are declared in `wish_types` (shared with game
//! code compiled to WebAssembly) and re-exported here.

use wish_core::math::Mat4;
use wish_core::{Entity, Result};
use wish_ecs::World;
pub use wish_types::transform::{GLOBAL_TRANSFORM, GlobalTransform, Transform, world_position};

/// Registers [`Transform`] and [`GlobalTransform`].
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<Transform>()?;
    world.register_component::<GlobalTransform>()?;
    Ok(())
}

/// Brings every entity's [`GlobalTransform`] up to date with its
/// [`Transform`] and its ancestors'.
///
/// Entities with a `Transform` get a `GlobalTransform` (added if missing,
/// rewritten only if its value changed, so change detection stays
/// meaningful). An entity without a `Transform` counts as no movement, so
/// its children are placed relative to its own parent. A `GlobalTransform`
/// on an entity that has lost its `Transform` is removed.
pub fn update_global_transforms(world: &mut World) -> Result<()> {
    // Collect first: the walk reads the hierarchy while writing components.
    let entities: Vec<Entity> = world.entities().collect();
    let mut stack: Vec<(Entity, Mat4)> = entities
        .iter()
        .rev()
        .filter(|entity| world.parent(**entity).is_none())
        .map(|entity| (*entity, Mat4::IDENTITY))
        .collect();
    while let Some((entity, parent_matrix)) = stack.pop() {
        let matrix = match world.get::<Transform>(entity) {
            Some(local) => {
                let global = parent_matrix * local.to_matrix();
                if world.get::<GlobalTransform>(entity).map(|g| g.matrix) != Some(global) {
                    world.insert(entity, GlobalTransform { matrix: global })?;
                }
                global
            }
            None => {
                if world.has::<GlobalTransform>(entity) {
                    world.remove::<GlobalTransform>(entity)?;
                }
                parent_matrix
            }
        };
        for child in world.children(entity).into_iter().rev() {
            stack.push((child, matrix));
        }
    }
    Ok(())
}
