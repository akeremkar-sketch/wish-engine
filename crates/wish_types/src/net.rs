//! Online play: which entities the host sends to which players (ADR-029;
//! design note `docs/design/1.1-replication.md`).
//!
//! An entity with [`Replicated`] is sent to the players its rule allows,
//! and to nobody else: what a player may not see never leaves the host. The
//! player who owns an entity ([`NetOwner`]) always sees it. Game code never
//! opens a socket; it changes who sees what by changing these values.

use crate::input::{MAX_PLAYERS, PlayerId};
use wish_schema::{component, resource, schema};

/// Who receives a replicated entity (its owner always does).
#[schema]
pub enum NetVisibility {
    /// Every player.
    Everyone,
    /// Only the player in its `NetOwner`.
    Owner,
    /// The players whose `NetGroups` bits share a bit with `groups` (for
    /// example ghosts, who see other ghosts).
    Group,
    /// The players with a `NetViewer` within `radius` metres.
    Near,
    /// The players who can see it: within `radius` metres of a `NetViewer`
    /// and not behind a wall (a solid tile of a tilemap's colliding layer,
    /// or an `Occluder`) on the straight line between them in the X-Y
    /// plane. Without walls, the same as `Near`.
    Sight,
    /// The players whose bit is set in `players`, written by game code: a
    /// system decides any rule it likes and writes the bits.
    Players,
}

/// Marks an entity whose replicated components the host sends to the
/// players its rule allows.
#[component]
pub struct Replicated {
    /// Who receives it (its owner always does).
    pub visibility: NetVisibility,
    /// The groups that see it, as bits (rule `Group`): bit `g` is group `g`.
    pub groups: u32,
    /// The players that see it, as bits (rule `Players`): bit `p` is player
    /// `p`.
    pub players: u32,
    /// How far from a `NetViewer` it is seen (rules `Near` and `Sight`).
    #[schema(unit = "m", range = 0.0..=1000000.0, default = 10.0)]
    pub radius: f32,
    /// How urgently its changes are sent when a player's byte budget is
    /// short: an entity with priority 2 goes twice as often as one with 1.
    #[schema(range = 0.01..=1000.0, default = 1.0)]
    pub priority: f32,
}

/// The player who owns the entity: that player always receives it.
#[component]
pub struct NetOwner {
    /// The owner's player id.
    pub player: PlayerId,
}

/// A point a player sees from (an avatar or a camera), for the rules `Near`
/// and `Sight`. A player may have several.
#[component]
pub struct NetViewer {
    /// The player who sees from here.
    pub player: PlayerId,
}

/// The groups of every player, as bits (bit `g` is group `g`), for the rule
/// `Group`. Game code changes them (a player who dies joins the ghosts).
#[resource]
pub struct NetGroups {
    /// One entry per player id.
    pub players: [u32; MAX_PLAYERS],
}
