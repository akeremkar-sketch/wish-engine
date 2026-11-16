//! Input as actions (ADR-016): `input.json` says which keys make which
//! action; whatever the source of the keys — a window, a tool injecting
//! values, a replay — [`apply`] writes the actions' state into the world
//! before the systems of a tick run. Game code reads
//! [`ActionState`](wish_types::ActionState) and never sees a device.
//!
//! ```json
//! { "format": 1, "actions": {
//!   "jump": { "kind": "button", "keys": ["Space"] },
//!   "throttle": { "kind": "axis", "positive": ["W"], "negative": ["S"] },
//!   "move": { "kind": "axis2", "up": ["W", "Up"], "down": ["S", "Down"],
//!             "left": ["A", "Left"], "right": ["D", "Right"] } } }
//! ```

mod apply;
mod map;

pub use apply::{Values, apply, register};
pub use map::{Binding, INPUT_FILE, InputMap, key_names};
