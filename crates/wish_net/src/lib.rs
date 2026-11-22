//! Online play's transport (ADR-029; design note
//! `docs/design/1.1-net-transport.md`).
//!
//! - [`Endpoint`]: a host (many peers) or a client (one host) speaking the
//!   engine's own protocol: a handshake with a room code, a reliable ordered
//!   channel and an unreliable one, fragmentation, acknowledgements, resends,
//!   limits, timeouts and disconnects. Every data packet is sealed with
//!   ChaCha20-Poly1305 under keys agreed with X25519.
//! - [`Transport`]: what an endpoint sends and receives datagrams through:
//!   [`UdpTransport`] (a real socket) or [`SimTransport`] (a node of a
//!   [`SimNetwork`], whose latency, jitter, loss, duplication and reordering
//!   come from a seed and which is stepped by ticks).
//! - [`discovery`]: finding the matches on the local network by broadcast.
//! - [`relay`]: playing over the internet through a relay the user runs
//!   (`wen relay`): the server and the transports that go through it.
//! - [`wire`] and [`crypto`]: the packet layout and the cryptography, as pure
//!   functions (the relay reads headers with the same code).
//!
//! Nothing in this crate reads the clock: the caller passes the time in
//! milliseconds to every `update`, so a simulated run is a pure function of
//! its seed. Packets from peers are untrusted input: a hostile peer cannot
//! crash an endpoint or keep it busy beyond its limits.

pub mod crypto;
pub mod discovery;
pub mod endpoint;
pub mod relay;
pub mod room;
pub mod sim;
pub mod transport;
pub mod wire;

pub use crypto::Entropy;
pub use discovery::{Announce, DISCOVERY_PORT, DiscoveryQuery, DiscoveryResponder};
pub use endpoint::{Channel, Config, Endpoint, EndpointStats, Event, PeerId, Reason};
pub use relay::{Relay, RelayClient, RelayConfig, RelayEvent, RelayHost, RelayState};
pub use room::RoomCode;
pub use sim::{Captured, LinkConditions, SimNetwork, SimStats, SimTransport};
pub use transport::{Recv, Transport, UdpTransport};
