//! The Wish Engine world.
//!
//! A [`World`] holds entities, the components attached to them, resources,
//! events and a text table. Components are plain data described by a schema
//! (see `wish_schema`); the world stores them as bytes, grouped by archetype
//! (the exact set of components an entity has), and hands out typed
//! references without copying.
//!
//! Everything that changes the world goes through `&mut World`, so there is
//! never more than one writer. Iteration order is deterministic: archetypes in
//! the order they were created, rows in the order entities arrived.
//!
//! ```
//! use wish_core::math::Vec3;
//! use wish_ecs::{Changed, World};
//! use wish_schema::component;
//!
//! /// Where an entity is.
//! #[component]
//! pub struct Position {
//!     /// World-space position.
//!     #[schema(unit = "m")]
//!     pub value: Vec3,
//! }
//!
//! let mut world = World::new();
//! let entity = world.spawn()?;
//! world.insert(entity, Position { value: Vec3::ONE })?;
//!
//! // Everything so far happened at or before `since`.
//! let since = world.change_tick();
//! world.advance_change_tick();
//! assert_eq!(world.query_since::<&Position, Changed<Position>>(since)?.count(), 0);
//!
//! for mut position in world.query::<&mut Position>()? {
//!     position.value.y += 1.0;
//! }
//! assert_eq!(world.get::<Position>(entity).map(|p| p.value.y), Some(2.0));
//! assert_eq!(world.query_since::<&Position, Changed<Position>>(since)?.count(), 1);
//! # Ok::<(), wish_core::Diagnostic>(())
//! ```
//!
//! Design: `docs/design/M1-cekirdek.md`, section 4.

mod archetype;
mod column;
mod commands;
mod entities;
mod events;
mod hierarchy;
mod query;
mod registry;
mod text;
mod world;

pub use archetype::Archetype;
pub use commands::{CommandReport, Commands, Target};
pub use events::EventCursor;
pub use hierarchy::Hierarchy;
pub use query::{
    Access, Added, ChangeTick, Changed, Mut, QueryData, QueryFilter, QueryIter, With, Without,
};
pub use registry::{ComponentId, EventId, Registry, ResourceId, TypeInfo};
pub use text::TextTable;
pub use world::{World, WorldSnapshot};

use wish_core::Diagnostic;

/// The diagnostic for a broken internal invariant: a bug in this crate, never
/// a problem with the caller's data.
pub(crate) fn internal_error(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        "internal_error",
        format!("Internal error in the world storage: {}", what.into()),
        "This is a bug in Wish Engine, not in your project. Please report it with the steps that led here.",
    )
}
