//! The lobby as plain data: its settings, its state, and the rules that
//! resolve names, colours and roles deterministically (design note §3.2,
//! §3.4).

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Result, Rng};
use wish_ecs::World;
use wish_types::{
    Lobby, LobbyClosed, LobbyPhase, LocalPlayer, MAX_PLAYERS, MatchRoles, PlayerId, Seat,
};

/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 24;
/// The longest name, in bytes.
pub const MAX_NAME_BYTES: usize = 64;
/// The longest role name, in bytes.
pub const MAX_ROLE_NAME: usize = 64;
/// Roles a match may have at most.
pub const MAX_ROLES: usize = 16;
/// Colours a palette may have at most.
pub const MAX_COLORS: u32 = 64;

/// One role of the game: role 0 is everyone else's; each later role is
/// dealt to `count` players.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleConfig {
    /// Its name (shown to its holder only).
    pub name: String,
    /// How many players get it (ignored for role 0, which is the rest).
    pub count: u32,
}

impl RoleConfig {
    /// A role called `name` for `count` players.
    pub fn new(name: &str, count: u32) -> Self {
        Self {
            name: name.to_owned(),
            count,
        }
    }
}

/// The lobby's and the session's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyConfig {
    /// The host's secret seed: roles are dealt from it. Never sent.
    pub seed: u64,
    /// Whether the host's machine is a player (seat 0); `false` for a
    /// headless host.
    pub host_plays: bool,
    /// The host player's name.
    pub host_name: String,
    /// The host player's colour wish.
    pub host_color: u32,
    /// Players seated before the host may start (1 to 16).
    pub min_players: u32,
    /// Colours of the game's palette (1 to 64); a colour is an index below.
    pub colors: u32,
    /// The roles (at most 16; the first is everyone else's). Empty: one
    /// role, 0, called "player".
    pub roles: Vec<RoleConfig>,
    /// The simulation's rate, to count message budgets per second (1 to
    /// 1000).
    pub tick_rate_hz: u32,
    /// The largest lobby or game message a player may send, in bytes (16 to
    /// 60 KiB).
    pub message_bytes: usize,
    /// Messages a player may send per second, sustained (1 to 1000).
    pub messages_per_second: u32,
    /// Messages a player may send at once (1 to 1000).
    pub message_burst: u32,
    /// Ticks a connected peer has to send its join (1 to 1 000 000).
    pub join_timeout_ticks: u64,
    /// Ticks a player who dropped during a match keeps its seat (0 to
    /// 1 000 000).
    pub rejoin_grace_ticks: u64,
    /// Ticks the host keeps sending after it closed, so that every player
    /// hears it (0 to 10 000).
    pub close_flush_ticks: u64,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            host_plays: true,
            host_name: "Host".to_owned(),
            host_color: 0,
            min_players: 1,
            colors: 16,
            roles: Vec::new(),
            tick_rate_hz: 60,
            message_bytes: 1024,
            messages_per_second: 30,
            message_burst: 60,
            join_timeout_ticks: 600,
            rejoin_grace_ticks: 1800,
            close_flush_ticks: 60,
        }
    }
}

impl LobbyConfig {
    /// Checks every setting against its range.
    pub fn validate(&self) -> Result<()> {
        let bad = |field: &str, message: String, hint: &str| {
            Err(
                Diagnostic::error("net_config_invalid", message, hint.to_owned())
                    .at_path(format!("/lobby/{field}")),
            )
        };
        if !(1..=MAX_PLAYERS as u32).contains(&self.min_players) {
            return bad(
                "min_players",
                format!("min_players is {}", self.min_players),
                "A match needs 1 to 16 players.",
            );
        }
        if !(1..=MAX_COLORS).contains(&self.colors) {
            return bad(
                "colors",
                format!("colors is {}", self.colors),
                "Give the palette 1 to 64 colours (the default is 16).",
            );
        }
        if self.roles.len() > MAX_ROLES
            || self
                .roles
                .iter()
                .any(|r| r.name.is_empty() || r.name.len() > MAX_ROLE_NAME)
        {
            return bad(
                "roles",
                format!("{} roles are listed", self.roles.len()),
                "List at most 16 roles, each with a name of 1 to 64 bytes.",
            );
        }
        if !(1..=1000).contains(&self.tick_rate_hz) {
            return bad(
                "tick_rate_hz",
                format!("tick_rate_hz is {}", self.tick_rate_hz),
                "Give the simulation's rate, 1 to 1000 ticks per second (usually 60).",
            );
        }
        if !(16..=60 * 1024).contains(&self.message_bytes) {
            return bad(
                "message_bytes",
                format!("message_bytes is {}", self.message_bytes),
                "Let a player's message be 16 bytes to 60 KiB (the default is 1024).",
            );
        }
        if !(1..=1000).contains(&self.messages_per_second)
            || !(1..=1000).contains(&self.message_burst)
        {
            return bad(
                "messages_per_second",
                format!(
                    "messages_per_second is {} and message_burst is {}",
                    self.messages_per_second, self.message_burst
                ),
                "Allow each player 1 to 1000 messages per second and a burst of 1 to 1000.",
            );
        }
        if !(1..=1_000_000).contains(&self.join_timeout_ticks)
            || self.rejoin_grace_ticks > 1_000_000
            || self.close_flush_ticks > 10_000
        {
            return bad(
                "join_timeout_ticks",
                format!(
                    "join_timeout_ticks is {}, rejoin_grace_ticks {}, close_flush_ticks {}",
                    self.join_timeout_ticks, self.rejoin_grace_ticks, self.close_flush_ticks
                ),
                "Give 1 to 1 000 000 ticks to join, at most 1 000 000 of grace and at most 10 000 to flush.",
            );
        }
        Ok(())
    }

    /// The roles, with the default role when none are listed.
    pub fn role_list(&self) -> Vec<RoleConfig> {
        if self.roles.is_empty() {
            vec![RoleConfig::new("player", 0)]
        } else {
            self.roles.clone()
        }
    }
}

/// One seat, as plain data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeatState {
    /// Whether a player holds it.
    pub taken: bool,
    /// Whether that player is connected.
    pub connected: bool,
    /// Whether it is ready.
    pub ready: bool,
    /// Whether it left the running match for good.
    pub left: bool,
    /// Its colour index.
    pub color: u32,
    /// Its name.
    pub name: String,
}

/// The lobby, as plain data (the `Lobby` resource is written from it).
#[derive(Debug, Clone, PartialEq)]
pub struct LobbyState {
    /// Where the lobby is.
    pub phase: LobbyPhase,
    /// The number of the current or last match.
    pub round: u32,
    /// How the last match ended.
    pub outcome: u32,
    /// Why the session closed.
    pub closed: LobbyClosed,
    /// The room code.
    pub room: String,
    /// The seats, by player id.
    pub seats: [SeatState; MAX_PLAYERS],
}

impl Default for LobbyState {
    fn default() -> Self {
        Self {
            phase: LobbyPhase::Offline,
            round: 0,
            outcome: 0,
            closed: LobbyClosed::None,
            room: String::new(),
            seats: Default::default(),
        }
    }
}

impl LobbyState {
    /// The seated players (taken, not left), ascending.
    pub fn seated(&self) -> Vec<PlayerId> {
        (0..MAX_PLAYERS as PlayerId)
            .filter(|p| self.seat(*p).is_some_and(|s| s.taken && !s.left))
            .collect()
    }

    /// The seated players who are connected now, ascending.
    pub fn present(&self) -> Vec<PlayerId> {
        (0..MAX_PLAYERS as PlayerId)
            .filter(|p| {
                self.seat(*p)
                    .is_some_and(|s| s.taken && !s.left && s.connected)
            })
            .collect()
    }

    /// Seat `player`.
    pub fn seat(&self, player: PlayerId) -> Option<&SeatState> {
        self.seats.get(player as usize)
    }

    /// Seat `player`, to change.
    pub fn seat_mut(&mut self, player: PlayerId) -> Option<&mut SeatState> {
        self.seats.get_mut(player as usize)
    }

    /// Writes the `Lobby` resource (names and the room code are interned in
    /// `world`).
    pub fn write(&self, world: &mut World) -> Result<()> {
        let mut lobby = Lobby {
            phase: self.phase,
            round: self.round,
            outcome: self.outcome,
            closed: self.closed,
            room: world.intern(&self.room)?,
            seats: [Seat::default(); MAX_PLAYERS],
        };
        for (slot, seat) in lobby.seats.iter_mut().zip(&self.seats) {
            *slot = Seat {
                taken: seat.taken,
                connected: seat.connected,
                ready: seat.ready,
                left: seat.left,
                color: seat.color,
                name: world.intern(&seat.name)?,
            };
        }
        world.insert_resource(lobby)
    }
}

/// Who this machine is, as plain data (the `LocalPlayer` resource is
/// written from it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalState {
    /// This machine's player id.
    pub player: PlayerId,
    /// Whether it holds a seat.
    pub joined: bool,
    /// Whether it is the host.
    pub host: bool,
    /// Its role.
    pub role: u32,
    /// Its role's name.
    pub role_name: String,
}

impl LocalState {
    /// Writes the `LocalPlayer` resource.
    pub fn write(&self, world: &mut World) -> Result<()> {
        let role_name = world.intern(&self.role_name)?;
        world.insert_resource(LocalPlayer {
            player: self.player,
            joined: self.joined,
            host: self.host,
            role: self.role,
            role_name,
        })
    }
}

/// Writes the `MatchRoles` resource (on the host only).
pub fn write_roles(world: &mut World, roles: &[u32; MAX_PLAYERS]) -> Result<()> {
    world.insert_resource(MatchRoles { roles: *roles })
}

/// A name as a person typed it, cleaned: control characters removed,
/// spaces at both ends trimmed and runs of spaces made one, at most
/// [`MAX_NAME_CHARS`] characters and [`MAX_NAME_BYTES`] bytes; an empty name
/// becomes "Player N".
pub fn clean_name(raw: &str, player: PlayerId) -> String {
    let mut out = String::new();
    let mut chars = 0;
    let mut space = false;
    for c in raw.chars() {
        if c.is_control() {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        let extra = usize::from(space);
        if chars + extra + 1 > MAX_NAME_CHARS || out.len() + extra + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        if space {
            out.push(' ');
            chars += 1;
            space = false;
        }
        out.push(c);
        chars += 1;
    }
    if out.is_empty() {
        format!("Player {player}")
    } else {
        out
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// `name` for seat `player`, made different from every other taken seat's
/// (ignoring case) by the lowest free suffix " 2", " 3"... The name keeps
/// within the limits: the base is shortened to make room for the suffix.
pub fn unique_name(name: &str, player: PlayerId, seats: &[SeatState; MAX_PLAYERS]) -> String {
    let taken = |candidate: &str| {
        seats
            .iter()
            .enumerate()
            .any(|(p, s)| p != player as usize && s.taken && same_name(&s.name, candidate))
    };
    if !taken(name) {
        return name.to_owned();
    }
    for n in 2..=MAX_PLAYERS + 1 {
        let suffix = format!(" {n}");
        let mut base: String = name.to_owned();
        while base.chars().count() + suffix.len() > MAX_NAME_CHARS
            || base.len() + suffix.len() > MAX_NAME_BYTES
        {
            base.pop();
        }
        let candidate = format!("{}{suffix}", base.trim_end());
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("Player {player}")
}

/// The colour seat `player` gets for its wish: the wish when no other taken
/// seat has it, else the next free index upwards (wrapping), else the wish.
pub fn free_color(
    wish: u32,
    player: PlayerId,
    seats: &[SeatState; MAX_PLAYERS],
    colors: u32,
) -> u32 {
    let colors = colors.clamp(1, MAX_COLORS);
    let wish = wish % colors;
    let used = |c: u32| {
        seats
            .iter()
            .enumerate()
            .any(|(p, s)| p != player as usize && s.taken && s.color == c)
    };
    (0..colors)
        .map(|step| (wish + step) % colors)
        .find(|c| !used(*c))
        .unwrap_or(wish)
}

/// Deals the roles of round `round` to `players` (ascending): they are
/// shuffled with a random stream seeded from `seed` and the round; the first
/// `roles[1].count` get role 1, the next `roles[2].count` role 2, and so on;
/// the rest role 0.
pub fn deal_roles(
    seed: u64,
    round: u32,
    players: &[PlayerId],
    roles: &[RoleConfig],
) -> BTreeMap<PlayerId, u32> {
    let mut order: Vec<PlayerId> = players.to_vec();
    let mut rng = Rng::from_seed(seed ^ u64::from(round).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for i in (1..order.len()).rev() {
        let j = rng.below(i as u32 + 1) as usize;
        order.swap(i, j);
    }
    let mut dealt = BTreeMap::new();
    let mut next = 0usize;
    for (role, spec) in roles.iter().enumerate().skip(1) {
        for _ in 0..spec.count {
            let Some(player) = order.get(next) else {
                break;
            };
            dealt.insert(*player, role as u32);
            next += 1;
        }
    }
    for player in order.iter().skip(next) {
        dealt.insert(*player, 0);
    }
    dealt
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn seats_with(names: &[(PlayerId, &str, u32)]) -> [SeatState; MAX_PLAYERS] {
        let mut seats: [SeatState; MAX_PLAYERS] = Default::default();
        for (p, name, color) in names {
            seats[*p as usize] = SeatState {
                taken: true,
                connected: true,
                name: (*name).to_owned(),
                color: *color,
                ..SeatState::default()
            };
        }
        seats
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Ada \u{7}  Lovelace ", 3), "Ada Lovelace");
        assert_eq!(clean_name("\n\t", 4), "Player 4");
        let long = "x".repeat(100);
        assert_eq!(clean_name(&long, 1).chars().count(), MAX_NAME_CHARS);
        let wide = "ş".repeat(40);
        let cleaned = clean_name(&wide, 1);
        assert!(cleaned.len() <= MAX_NAME_BYTES && cleaned.chars().count() <= MAX_NAME_CHARS);
    }

    #[test]
    fn clashing_names_get_the_lowest_free_suffix() {
        let seats = seats_with(&[(0, "Ada", 0), (1, "ada 2", 1)]);
        assert_eq!(unique_name("ADA", 2, &seats), "ADA 3");
        assert_eq!(
            unique_name("Ada", 0, &seats),
            "Ada",
            "its own name is not a clash"
        );
        let long = "y".repeat(MAX_NAME_CHARS);
        let seats = seats_with(&[(0, &long, 0)]);
        let named = unique_name(&long, 1, &seats);
        assert!(named.ends_with(" 2") && named.chars().count() <= MAX_NAME_CHARS);
    }

    #[test]
    fn clashing_colours_take_the_next_free_one() {
        let seats = seats_with(&[(0, "a", 3), (1, "b", 4)]);
        assert_eq!(free_color(3, 2, &seats, 16), 5);
        assert_eq!(free_color(15, 2, &seats, 16), 15);
        assert_eq!(
            free_color(19, 2, &seats, 16),
            5,
            "a wish wraps into the palette"
        );
        let full = seats_with(&[(0, "a", 0), (1, "b", 1)]);
        assert_eq!(
            free_color(1, 2, &full, 2),
            1,
            "every colour is taken: the wish"
        );
    }

    #[test]
    fn roles_are_dealt_from_the_seed() {
        let roles = vec![RoleConfig::new("crew", 0), RoleConfig::new("saboteur", 2)];
        let players: Vec<PlayerId> = (0..6).collect();
        let a = deal_roles(7, 1, &players, &roles);
        let b = deal_roles(7, 1, &players, &roles);
        assert_eq!(a, b);
        assert_eq!(a.values().filter(|r| **r == 1).count(), 2);
        assert_eq!(a.len(), 6);
        let rounds: Vec<_> = (1..20)
            .map(|r| deal_roles(7, r, &players, &roles))
            .collect();
        assert!(rounds.iter().any(|r| *r != a), "another round deals again");
        let few = deal_roles(7, 1, &[4], &roles);
        assert_eq!(few[&4], 1, "too few players: the later roles go first");
    }
}
