//! The types a `wen` command knows about: the engine's own, and those of
//! the chosen built-in example.
//!
//! Until game modules exist (M5), a project's own component types cannot be
//! loaded, so commands that read scene files take the types of a built-in
//! example (`--example`) next to the engine's.

use wish_core::Result;
use wish_ecs::World;

/// Registers the engine's components and resources (`SceneId`, `Transform`,
/// `GlobalTransform`, and the rendering types) and then the types of
/// `example`.
pub fn register_scene_types(example: &str, world: &mut World) -> Result<()> {
    register_engine_types(world)?;
    wish_examples::register_types(example, world)
}

/// Registers only the engine's own types.
pub fn register_engine_types(world: &mut World) -> Result<()> {
    wish_scene::instantiate::register(world)?;
    wish_transform::register(world)?;
    wish_render::components::register(world)
}
