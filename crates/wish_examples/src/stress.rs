//! The `stress` example: many entities and simple systems, the benchmark
//! workload for `wen bench`.
//!
//! Every entity has a position and a velocity; half also spin. Two systems
//! integrate them each tick. Nothing is spawned or despawned while it runs,
//! so the cost measured is iteration and writing, not structural change.

use wish_app::{App, AppConfig, System, SystemContext};
use wish_core::math::Vec3;
use wish_core::{AssetRef, Result, Rng};
use wish_ecs::World;
use wish_schema::component;

/// Where an entity is.
#[component]
pub struct Position {
    /// World-space position.
    #[schema(unit = "m")]
    pub value: Vec3,
}

/// How fast an entity moves.
#[component]
pub struct Velocity {
    /// Velocity.
    #[schema(unit = "m/s")]
    pub value: Vec3,
}

/// A mesh and textures taken from asset files, to show asset references in
/// scene files. Not part of the benchmark world: it is registered only for
/// scene files (`register_types`).
#[component]
pub struct MeshRef {
    /// The mesh file, e.g. `assets/models/crate.glb#Mesh0`.
    pub mesh: AssetRef,
    /// Texture files, empty for none.
    pub textures: [AssetRef; 2],
}

/// A rotation that advances every tick.
#[component]
pub struct Spin {
    /// Current angle.
    #[schema(unit = "rad")]
    pub angle: f32,
    /// Angular speed.
    #[schema(unit = "rad/s")]
    pub rate: f32,
}

fn movement(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let dt = ctx.time.delta;
    for (mut position, velocity) in world.query::<(&mut Position, &Velocity)>()? {
        position.value += velocity.value * dt;
    }
    Ok(())
}

fn spin(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let dt = ctx.time.delta;
    for mut spin in world.query::<&mut Spin>()? {
        // `%` on floats is fmod, which is computed exactly (no rounding), so
        // it gives the same result everywhere.
        spin.angle = (spin.angle + spin.rate * dt) % core::f32::consts::TAU;
    }
    Ok(())
}

/// Builds the example with `entities` entities.
pub fn build_with(config: AppConfig, entities: u32) -> Result<App> {
    let mut app = App::new(config);
    let mut rng = Rng::from_seed(config.seed);
    let world = app.world_mut();
    world.register_component::<Position>()?;
    world.register_component::<Velocity>()?;
    world.register_component::<Spin>()?;
    for index in 0..entities {
        let entity = world.spawn()?;
        world.insert(
            entity,
            Position {
                value: Vec3::new(
                    rng.range_f32(-100.0, 100.0),
                    rng.range_f32(0.0, 50.0),
                    rng.range_f32(-100.0, 100.0),
                ),
            },
        )?;
        world.insert(
            entity,
            Velocity {
                value: Vec3::new(
                    rng.range_f32(-1.0, 1.0),
                    rng.range_f32(-1.0, 1.0),
                    rng.range_f32(-1.0, 1.0),
                ),
            },
        )?;
        if index.is_multiple_of(2) {
            world.insert(
                entity,
                Spin {
                    angle: 0.0,
                    rate: rng.range_f32(-3.0, 3.0),
                },
            )?;
        }
    }
    app.add_system(System::new("movement", movement))?;
    app.add_system(System::new("spin", spin))?;
    Ok(app)
}

/// Builds the example with 1,000 entities.
pub fn build(config: AppConfig) -> Result<App> {
    build_with(config, 1_000)
}
