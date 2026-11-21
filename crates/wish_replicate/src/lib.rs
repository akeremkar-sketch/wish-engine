//! Online play's replication (ADR-029; design note
//! `docs/design/1.1-replication.md`).
//!
//! The host runs the one true simulation. Every tick, for every player,
//! [`Host`] works out which [`Replicated`](wish_types::Replicated) entities
//! that player may see and sends a delta of their replicated components
//! against the last state the player acknowledged, encoded by the schema
//! (field by field, never padding) within a byte budget. What a player may
//! not see never leaves the host. A [`Client`] mirrors what it receives into
//! its own world (spawns, despawns, component changes, with the host's
//! entities mapped to its own) and sends its player's input back, stamped
//! with ticks; the host writes it into `PlayerActionState`.
//!
//! The crate knows the world (`wish_ecs`, `wish_schema`, the plain types of
//! `wish_types`) and the transport (`wish_net`) and nothing else of the
//! engine. Nothing here reads the clock: ticks and the transport's time come
//! from the caller, so a run over the network simulator is a pure function
//! of its seed.

pub mod client;
pub mod host;
pub mod input;
pub mod layout;
pub mod value;
pub mod visibility;
pub mod wire;

pub use client::{Client, ClientConfig, ClientStats};
pub use host::{Host, HostStats, ReplicationConfig};
pub use layout::{Layout, Leaf, LeafKind};
pub use value::{EntityView, Value, View};

use wish_core::Result;
use wish_ecs::World;
use wish_types::{NetGroups, NetOwner, NetViewer, Replicated};

/// Registers the online play types: the components `Replicated`,
/// `NetOwner` and `NetViewer`, and the resource `NetGroups`.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<Replicated>()?;
    world.register_component::<NetOwner>()?;
    world.register_component::<NetViewer>()?;
    world.register_resource::<NetGroups>()?;
    Ok(())
}
