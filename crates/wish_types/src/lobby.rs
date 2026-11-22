//! Online play's lobby and targeted messages (ADR-029; design note
//! `docs/design/1.1-lobby.md`).
//!
//! - A **network message** is any event type with a field `net` of type
//!   [`Net`]: game code sends it with a target, the engine carries it at a
//!   tick boundary and delivers it as the same event with `net.delivered`
//!   set and `net.from` stamped with the sending player. Players send only
//!   to the host; the host sends to one player, a group or everyone.
//! - The **lobby** is the [`Lobby`] resource (public: everyone sees the
//!   same), [`LocalPlayer`] (who this machine is, and its own role, which is
//!   secret) and, on the host only, [`MatchRoles`]. Game code asks the lobby
//!   for things with [`LobbyRequest`].
//!
//! Game code never opens a socket; the engine does all networking.

use crate::input::{MAX_PLAYERS, PlayerId};
use wish_core::Text;
use wish_schema::{event, resource, schema};

/// Where a network message goes.
#[schema]
pub enum NetTo {
    /// To the host (the only target a player's machine may send to).
    Host,
    /// To the player in `Net.player` (sent by the host).
    Player,
    /// To every player in the lobby, the host's own player included (sent
    /// by the host).
    Everyone,
    /// To the players whose `NetGroups` bits share a bit with `Net.groups`
    /// (sent by the host).
    Group,
}

/// The envelope of a network message: an event type with a field
/// `net: Net` crosses the network. The sender fills `to` (and `player` or
/// `groups`); the engine fills `from` and `delivered` when it delivers it.
#[schema]
pub struct Net {
    /// Where it goes.
    pub to: NetTo,
    /// The receiving player, for `to: Player`.
    pub player: PlayerId,
    /// The receiving groups, as bits, for `to: Group`.
    pub groups: u32,
    /// Who sent it, written by the engine from the connection it came on
    /// (never from the sender's bytes). On a player's machine every
    /// delivered message comes from the host, player 0.
    pub from: PlayerId,
    /// Whether this copy was delivered by the engine (`true`) or is the
    /// sender's own outgoing copy (`false`). Read only delivered ones.
    pub delivered: bool,
}

impl Net {
    /// An envelope to the host.
    pub const fn to_host() -> Self {
        Self {
            to: NetTo::Host,
            player: 0,
            groups: 0,
            from: 0,
            delivered: false,
        }
    }

    /// An envelope to one player (the host sends these).
    pub const fn to_player(player: PlayerId) -> Self {
        Self {
            to: NetTo::Player,
            player,
            groups: 0,
            from: 0,
            delivered: false,
        }
    }

    /// An envelope to every player (the host sends these).
    pub const fn to_everyone() -> Self {
        Self {
            to: NetTo::Everyone,
            player: 0,
            groups: 0,
            from: 0,
            delivered: false,
        }
    }

    /// An envelope to the players in `groups` (the host sends these).
    pub const fn to_group(groups: u32) -> Self {
        Self {
            to: NetTo::Group,
            player: 0,
            groups,
            from: 0,
            delivered: false,
        }
    }
}

/// Where the lobby is.
#[schema]
pub enum LobbyPhase {
    /// No online session: a single-player game.
    Offline,
    /// Players join, choose names and colours and get ready.
    Open,
    /// The match is being played; nobody new joins.
    Running,
    /// The match ended (`Lobby.outcome` says how); the host reopens the
    /// lobby next.
    Ended,
    /// The session is over (`Lobby.closed` says why).
    Closed,
}

/// Why a session closed.
#[schema]
pub enum LobbyClosed {
    /// It did not.
    None,
    /// The host left or closed the match.
    HostLeft,
    /// The host refused this player (the match was full or running, or a
    /// rejoin came too late).
    Refused,
    /// The connection to the host was lost.
    Lost,
    /// This player left.
    Left,
}

/// One seat of the lobby: a player.
#[schema]
pub struct Seat {
    /// Whether a player holds this seat.
    pub taken: bool,
    /// Whether that player is connected now (a player who dropped during a
    /// match keeps the seat for a while).
    pub connected: bool,
    /// Whether the player is ready to start.
    pub ready: bool,
    /// Whether the player left the running match for good (left, or did not
    /// come back in time).
    pub left: bool,
    /// The player's colour, an index into the game's palette (no two seats
    /// share one while colours are free).
    pub color: u32,
    /// The player's name (no two seats share one, ignoring case).
    pub name: Text,
}

/// The lobby, as every player sees it: written by the engine from the host's
/// decisions.
#[resource]
pub struct Lobby {
    /// Where the lobby is.
    pub phase: LobbyPhase,
    /// The number of the current or last match (0 before the first).
    pub round: u32,
    /// How the last match ended, a number the game chose when it ended it.
    pub outcome: u32,
    /// Why the session closed, once `phase` is `Closed`.
    pub closed: LobbyClosed,
    /// The room code players type to join.
    pub room: Text,
    /// The seats, by player id.
    pub seats: [Seat; MAX_PLAYERS],
}

/// Who this machine is in the online session.
#[resource]
pub struct LocalPlayer {
    /// This machine's player id (0 on the host).
    pub player: PlayerId,
    /// Whether this machine holds a seat.
    pub joined: bool,
    /// Whether this machine is the host.
    pub host: bool,
    /// This player's role in the running match (0: the default role, also
    /// before a match). Sent only to this player.
    pub role: u32,
    /// The name of that role.
    pub role_name: Text,
}

/// Every player's role in the running match, by player id. Written on the
/// host only: a player's machine never receives it.
#[resource]
pub struct MatchRoles {
    /// The role of each player (0: the default role).
    pub roles: [u32; MAX_PLAYERS],
}

/// What game code asks the lobby for.
#[schema]
pub enum LobbyAction {
    /// This player is ready.
    Ready,
    /// This player is not ready.
    NotReady,
    /// This player's name becomes `text`.
    Name,
    /// This player's colour becomes `value`.
    Color,
    /// Start the match (the host only).
    Start,
    /// End the match with the outcome `value` (the host only).
    End,
    /// Return to the lobby after a match (the host only).
    Reopen,
    /// This player leaves the session (the host leaving closes it).
    Leave,
}

/// A request to the lobby from game code (a lobby screen's buttons). The
/// engine reads it after the systems run.
#[event]
pub struct LobbyRequest {
    /// What is asked.
    pub action: LobbyAction,
    /// A colour (`Color`) or an outcome (`End`).
    pub value: u32,
    /// A name (`Name`).
    pub text: Text,
}
