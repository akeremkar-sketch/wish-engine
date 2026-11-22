//! Input as actions (ADR-016): `input.json` says which keys make which
//! action; whatever the source of the keys — a window, a tool injecting
//! values, a replay — [`apply`] writes the actions' state into the world
//! before the systems of a tick run. Game code reads
//! [`ActionState`](wish_types::ActionState) and never sees a device.
//! A session with more than one player feeds every player's values through
//! [`apply_players`] (release 1.1): `ActionState` stays player 0's, and
//! `PlayerActionState` holds everyone's.
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

pub use apply::{
    CLICK, POINTER, SCREEN, Values, apply, apply_players, apply_pointer, game_screen,
    interface_to_picture, main_camera, picture_to_interface, register, set_players,
};
pub use map::{Binding, INPUT_FILE, InputMap, key_names};
pub use wish_types::MAX_PLAYERS;
