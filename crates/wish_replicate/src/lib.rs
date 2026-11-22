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
//!
//! A [`Predictor`] (design note `docs/design/1.1-prediction.md`) runs the
//! game's own systems on a copy of the client's world ahead of the host for
//! what the local player owns and touches, reconciles with every state the
//! host confirms, interpolates everyone else and keeps the client's ticks
//! ahead of the host's by the round trip plus a margin.
//!
//! The crate knows the world (`wish_ecs`, `wish_schema`, the plain types of
//! `wish_types`), the app loop (`wish_app`, for the prediction's systems)
//! and the transport (`wish_net`) and nothing else of the engine. Nothing here reads the clock: ticks and the transport's time come
//! from the caller, so a run over the network simulator is a pure function
//! of its seed.

pub mod client;
pub mod host;
pub mod input;
pub mod interpolate;
pub mod layout;
pub mod predict;
pub mod sync;
pub mod value;
pub mod visibility;
pub mod wire;

pub use client::{Client, ClientConfig, ClientStats, Confirmed};
pub use host::{Host, HostStats, ReplicationConfig};
pub use interpolate::Source;
pub use layout::{Layout, Leaf, LeafKind};
pub use predict::{FrameReport, NoHook, PredictConfig, PredictStats, Predictor, Shown, TickHook};
pub use sync::SyncState;
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
