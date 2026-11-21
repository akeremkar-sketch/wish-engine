//! Line of sight in the X–Y plane (design note
//! `docs/design/1.1-lights-2d.md`): what blocks sight, "can A see B", and
//! what is visible from a point.
//!
//! Walls are the solid tiles of tilemaps (their shapes, [`TileShapes`],
//! shared with physics) and entities with an enabled `Occluder` beside a
//! `Collider`. The [`system`] (`sight`, after transform propagation)
//! gathers them every tick into the world extension [`Occluders`]; game
//! code (through the control plane), online play's `NetVisibility::Sight`,
//! the shadows of 2D lights and the tools ask it. Every answer is computed
//! with operations IEEE 754 defines to the bit, in a fixed order, so the
//! same world gives the same bits on every machine (§4).

mod gather;
pub mod geometry;
mod occluders;
mod tiles;

pub use gather::{collider_outline, gather, has_walls, placement};
pub use occluders::{
    MAX_AREA_SEGMENTS, MAX_RADIUS, MAX_SEGMENTS, Occluders, Segment, Sight, VisiblePolygon, inside,
    outline_segments,
};
pub use tiles::{MapShapes, TileShapes, install_tile_shapes};
pub use wish_types::lights2d::Occluder;

use wish_core::Result;
use wish_ecs::World;

/// The name of the system that gathers the walls.
pub const SYSTEM: &str = "sight";

/// The system that gathers the world's walls into [`Occluders`] every tick
/// (or removes them when there are none). Runs after transform
/// propagation, so walls are where things were put this tick.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, _| {
        refresh(world);
        Ok(())
    })
}

/// Registers the sight types (`Occluder`).
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<Occluder>()?;
    Ok(())
}

/// Gathers the walls of `world` into its [`Occluders`] extension, or removes
/// it when nothing blocks sight.
pub fn refresh(world: &mut World) {
    if has_walls(world) {
        let walls = gather(world);
        world.insert_extension(walls);
    } else {
        world.remove_extension::<Occluders>();
    }
}

/// Builds the walls of a world about to run (a session), so that questions
/// during its first tick see them.
pub fn prepare(world: &mut World) {
    refresh(world);
}

/// The walls of `world`: what the `sight` system gathered, or, in a world
/// that does not run it (a scene file seen by a tool, a picture), gathered
/// now.
pub fn current(world: &World) -> Occluders {
    match world.extension::<Occluders>() {
        Some(walls) => walls.clone(),
        None if has_walls(world) => gather(world),
        None => Occluders::default(),
    }
}
