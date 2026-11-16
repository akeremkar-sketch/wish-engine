//! The types a project's scenes can use: the engine's own, those of a
//! built-in example (projects without game code), and those of the
//! project's game module (M5).

use wish_core::Result;
use wish_ecs::World;

/// Registers the engine's components and resources (`SceneId`, `Transform`,
/// `GlobalTransform` and the rendering types).
pub fn register_engine_types(world: &mut World) -> Result<()> {
    wish_scene::instantiate::register(world)?;
    wish_transform::register(world)?;
    wish_input::register(world)?;
    wish_render::components::register(world)
}

/// Registers the engine's types, the example's (if any) and the game
/// module's (if the project has one built).
pub fn register_all(
    example: Option<&str>,
    module: Option<&wish_script::GameModule>,
    world: &mut World,
) -> Result<()> {
    register_project_types(example, world)?;
    match module {
        Some(module) => module.install(world).map(|_| ()),
        None => Ok(()),
    }
}

/// Registers the engine's types and then those of `example`, if any.
pub fn register_project_types(example: Option<&str>, world: &mut World) -> Result<()> {
    register_engine_types(world)?;
    match example {
        Some(name) => wish_examples::register_types(name, world),
        None => Ok(()),
    }
}
