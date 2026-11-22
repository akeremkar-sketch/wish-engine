//! The relay: how a host behind a home router plays with people over the
//! internet, through a small program its owner runs on a server
//! (`wen relay`). Design note `docs/design/1.1-relay.md`.
//!
//! - [`Relay`]: the server. It routes datagrams between a registered host
//!   and the clients attached to its room, by a room locator (a hash of the
//!   room code) and relay-assigned slots, inside a small cleartext envelope.
//!   The inner datagrams stay sealed end to end: the relay holds no key,
//!   runs no game logic, writes nothing to disk and limits rates.
//! - [`RelayHost`] and [`RelayClient`]: transports that wrap another
//!   transport, so an [`Endpoint`](crate::Endpoint) goes through a relay
//!   with the same handshake and encryption.
//! - [`envelope`]: the relay's datagrams, as pure functions.

pub mod envelope;
mod server;
mod transports;

pub use envelope::{Locator, MAX_RELAY_DATAGRAM, Refusal, locator};
pub use server::{
    ClientLeft, MAX_CLIENTS_LIMIT, MAX_EVENTS, MAX_ROOMS_LIMIT, RELAY_PORT, Relay, RelayConfig,
    RelayEvent, RelayStats, RoomClosed, RoomTag,
};
pub use transports::{
    ATTACH_RETRY_MS, HOLD_MAX, REFUSED_RETRY_MS, REGISTER_EVERY_MS, REGISTER_RETRY_MS, RelayClient,
    RelayHost, RelayState, refusal_diagnostic, virtual_addr, virtual_slot,
};
