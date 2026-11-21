//! What game code sees of the player's input: actions, never devices
//! (ADR-016).
//!
//! The project's `input.json` maps keys to named actions (`move`, `jump`).
//! Every tick the engine writes the state of each action into the
//! [`ActionState`] resource, whether it came from a keyboard, from a tool
//! injecting input, or from a replay: game code cannot tell the difference,
//! which is what makes a game scriptable and replayable.
//!
//! A session can have more than one player (release 1.1, ADR-029). Player
//! 0 is the first player, and [`ActionState`] stays exactly player 0's
//! actions, so a single-player game never sees anything new. With more than
//! one player the engine also writes [`Players`] (how many there are) and
//! [`PlayerActionState`] (every player's actions, player 0's included); a
//! world without them has one player.

use wish_schema::{resource, schema};

/// How many actions a project can have.
pub const MAX_ACTIONS: usize = 32;

/// How many players a session can have.
pub const MAX_PLAYERS: usize = 16;

/// A player of the session, by number: 0 is the first player (the only one
/// in a single-player game), then 1, 2, ... up to `Players.count - 1`.
pub type PlayerId = u32;

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

/// The players of the session. The engine writes it when a session has more
/// than one player; a world without it has one player (this type's
/// default). Player ids are 0 to `count - 1`.
#[resource]
pub struct Players {
    /// How many players the session has.
    #[schema(range = 1..=16, default = 1)]
    pub count: u32,
}

impl Players {
    /// The ids of the players, in order: 0 to `count - 1`.
    pub fn ids(&self) -> impl Iterator<Item = PlayerId> + use<> {
        0..self.count.clamp(1, MAX_PLAYERS as u32)
    }
}

/// The state of every action of one player this tick, in the order of the
/// action names (as in `ActionState`).
#[schema]
pub struct PlayerActionSlots {
    /// One slot per action; unused slots have id 0.
    pub slots: [ActionSlot; MAX_ACTIONS],
}

impl PlayerActionSlots {
    /// The slot of the action called `name`, if the project has it.
    pub fn slot(&self, name: &str) -> Option<&ActionSlot> {
        let id = action_id(name);
        self.slots.iter().find(|slot| slot.id == id)
    }
}

/// The actions of every player this tick, by player id. Written by the
/// engine before the systems run, only in a session with more than one
/// player; entry 0 is then the same as `ActionState`, and the entries of
/// ids not in the session are at rest.
#[resource]
pub struct PlayerActionState {
    /// One entry per player id.
    pub players: [PlayerActionSlots; MAX_PLAYERS],
}

impl PlayerActionState {
    /// The actions of `player` (at rest for an id beyond the last).
    pub fn of(&self, player: PlayerId) -> Option<&PlayerActionSlots> {
        self.players.get(player as usize)
    }
}
