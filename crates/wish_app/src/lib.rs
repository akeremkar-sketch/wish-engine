//! The Wish Engine main loop.
//!
//! An [`App`] owns a world and a list of systems, and advances the world in
//! fixed steps ("ticks"). Everything that decides what happens next is
//! deterministic:
//!
//! - **Time** is a tick counter and a fixed step length ([`Time`]); systems
//!   never see the wall clock.
//! - **Order** is a total order computed from the systems' `before`/`after`
//!   constraints, ties broken by name, so it does not depend on the order
//!   systems were added in.
//! - **Randomness** comes from one [`Rng`](wish_core::Rng) stream per system,
//!   derived from the app's seed and the system's name.
//!
//! The same build, seed and inputs therefore give the same world, tick for
//! tick.
//!
//! ```
//! use wish_app::{App, AppConfig, System, SystemContext};
//! use wish_core::math::Vec3;
//! use wish_ecs::World;
//! use wish_schema::component;
//!
//! /// Where an entity is.
//! #[component]
//! pub struct Position {
//!     /// World-space position.
//!     pub value: Vec3,
//! }
//!
//! fn fall(world: &mut World, ctx: &mut SystemContext) -> wish_core::Result<()> {
//!     for mut position in world.query::<&mut Position>()? {
//!         position.value.y -= 9.81 * ctx.time.delta;
//!     }
//!     Ok(())
//! }
//!
//! let mut app = App::new(AppConfig { seed: 7, ..AppConfig::default() });
//! let entity = app.world_mut().spawn()?;
//! app.world_mut().insert(entity, Position { value: Vec3::ZERO })?;
//! app.add_system(System::new("fall", fall))?;
//! app.run(60)?;
//! assert!(app.world().get::<Position>(entity).unwrap().value.y < -9.0);
//! # Ok::<(), wish_core::Diagnostic>(())
//! ```
//!
//! Design: `docs/design/M1-cekirdek.md`, section 5.

mod app;
mod schedule;
mod time;

pub use app::{App, AppConfig, StepReport, SystemState, UpdateReport};
pub use schedule::{System, SystemContext, SystemFn};
pub use time::Time;
