//! What scenes say about how things look: cameras, lights, shapes,
//! materials, models, and the sky.
//!
//! The declarations live in `wish_types` (shared with game code compiled to
//! WebAssembly) and are re-exported here; this module registers them.

use wish_core::Result;
use wish_ecs::World;
pub use wish_types::render::{
    Ambient, Camera, DirectionalLight, EMISSIVE_NITS, Exposure, Material, Model, PointLight,
    Primitive, Projection, Shape, Sky, SpotLight,
};

/// Registers every rendering component and resource.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<Camera>()?;
    world.register_component::<DirectionalLight>()?;
    world.register_component::<PointLight>()?;
    world.register_component::<SpotLight>()?;
    world.register_component::<Primitive>()?;
    world.register_component::<Material>()?;
    world.register_component::<Model>()?;
    world.register_component::<wish_types::sprite::Sprite>()?;
    world.register_resource::<Sky>()?;
    world.register_resource::<Ambient>()?;
    world.register_resource::<Exposure>()?;
    Ok(())
}
