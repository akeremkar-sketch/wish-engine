//! The types a `wen` command knows about: the engine's own, and those of
//! the chosen built-in example. The control plane owns the list; this module
//! only adapts it for the older commands that take `--example`.

use wish_core::Result;
use wish_ecs::World;

/// Registers the engine's types and then those of `example`.
pub fn register_scene_types(example: &str, world: &mut World) -> Result<()> {
    wish_control::types::register_project_types(Some(example), world)
}

/// Registers only the engine's own types.
pub fn register_engine_types(world: &mut World) -> Result<()> {
    wish_control::types::register_engine_types(world)
}
