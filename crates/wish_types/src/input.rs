//! What game code sees of the player's input: actions, never devices
//! (ADR-016).
//!
//! The project's `input.json` maps keys to named actions (`move`, `jump`).
//! Every tick the engine writes the state of each action into the
//! [`ActionState`] resource, whether it came from a keyboard, from a tool
//! injecting input, or from a replay: game code cannot tell the difference,
//! which is what makes a game scriptable and replayable.

use wish_schema::{resource, schema};

/// How many actions a project can have.
pub const MAX_ACTIONS: usize = 32;

/// The id of an action name: a 32-bit FNV-1a hash (never 0; 0 marks an
/// unused slot). Game code looks actions up by it, so it needs no table of
/// names.
pub const fn action_id(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    if hash == 0 { 1 } else { hash }
}

/// The state of one action this tick.
#[schema]
pub struct ActionSlot {
    /// The action's id (see `action_id`); 0 for an unused slot.
    pub id: u32,
    /// Whether the action is active now (a button is down, an axis is away
    /// from zero).
    pub held: bool,
    /// Whether it became active this tick.
    pub pressed: bool,
    /// Whether it stopped being active this tick.
    pub released: bool,
    /// The value: 1 or 0 for a button; the axis value from -1 to 1 for an
    /// axis; the horizontal part of a two-axis action (right is positive).
    #[schema(range = -1.0..=1.0)]
    pub x: f32,
    /// The vertical part of a two-axis action (up is positive).
    #[schema(range = -1.0..=1.0)]
    pub y: f32,
}

/// The state of every action of the project this tick, in the order of the
/// action names. Written by the engine before the systems run.
#[resource]
pub struct ActionState {
    /// One slot per action; unused slots have id 0.
    pub slots: [ActionSlot; MAX_ACTIONS],
}

impl ActionState {
    /// The slot of the action called `name`, if the project has it.
    pub fn slot(&self, name: &str) -> Option<&ActionSlot> {
        let id = action_id(name);
        self.slots.iter().find(|slot| slot.id == id)
    }
}
