//! Online play's sessions (ADR-029; design note `docs/design/1.1-lobby.md`).
//!
//! - **Messages.** An event type with a field `net: Net` is a network
//!   message. Game code sends it with a target (players to the host; the
//!   host to one player, a group or everyone); the engine carries it on the
//!   reliable channel and delivers it at the next tick boundary as the same
//!   event, with `net.delivered` set and `net.from` stamped from the
//!   connection it came on (never from the sender's bytes).
//! - **The lobby.** [`HostSession`] seats players who join with the room
//!   code, resolves names and colours, starts the match when everyone is
//!   ready, deals secret roles from its seed and sends each player only its
//!   own, ends the match, reopens the lobby, keeps a dropped player's seat
//!   for a grace period and closes the session for everyone when the host
//!   leaves. [`ClientSession`] is a player's side.
//! - **The match log.** Everything the host's world receives from the
//!   network enters at a tick boundary as one [`TickRecord`], written by one
//!   function, [`apply`]; the [`MatchLog`] of those records replays the
//!   match bit for bit.
//!
//! The crate knows the world, the transport and replication, and nothing
//! else of the engine. Nothing here reads the clock.

pub mod client;
pub mod host;
pub mod lobby;
pub mod message;
pub mod record;
pub mod wire;

pub use client::{ClientSession, ClientSessionConfig, ClientStatus};
pub use host::{HostSession, HostStatus, SeatStatus, SessionStats};
pub use lobby::{LobbyConfig, LobbyState, LocalState, RoleConfig, SeatState};
pub use message::{MessageType, MessageValue, message_types};
pub use record::{Carry, Delivered, MatchLog, TickRecord, apply};

use wish_core::Result;
use wish_ecs::World;
use wish_types::{Lobby, LobbyRequest, LocalPlayer, MatchRoles};

/// Registers the lobby's types: the resources `Lobby`, `LocalPlayer` and
/// `MatchRoles`, and the event `LobbyRequest`. (The envelope `Net` is a
/// schema type inside the game's own message events.)
pub fn register(world: &mut World) -> Result<()> {
    world.register_resource::<Lobby>()?;
    world.register_resource::<LocalPlayer>()?;
    world.register_resource::<MatchRoles>()?;
    world.register_event::<LobbyRequest>()?;
    Ok(())
}
