//! Ridgewatch: a hidden-role party game for 4 to 10 players, online.
//!
//! A storm has buried the Ridgewatch radio observatory. Its keepers must
//! repair five machines before the storm cuts them off for good. One of
//! them is "static": a keeper in name only, who freezes the others where
//! they stand and drowns the station's lights. Anyone can ring the bell in
//! the hall to call everyone together, talk it over and vote one keeper out
//! into the storm.
//!
//! - Keepers win when the five repairs are done, or when the static is voted
//!   out.
//! - The static wins when as many statics are left standing as keepers.
//!
//! The host's machine runs the match (`local.host`, or alone: practice); every
//! player's machine shows what the host sends it: its own keeper, and the
//! others only while it can see them (`Replicated.players`, decided below by
//! line of sight). A player learns their own role only (`LocalPlayer.role`).
//! Chat and votes are network messages to the host, who passes chat on.

use wish_sdk::prelude::*;

// ----------------------------------------------------------------- the rules

/// The numbers of the game, in the scene so they can be tuned by hand.
#[resource]
pub struct Rules {
    /// Repairs that win the game for the keepers.
    #[schema(range = 1..=5, default = 5)]
    pub repairs_needed: u32,
    /// How long a keeper holds `use` at a machine to repair it.
    #[schema(unit = "ticks", default = 120)]
    pub repair_ticks: u32,
    /// How long a keeper holds `use` at the fuse box to bring the lights back.
    #[schema(unit = "ticks", default = 90)]
    pub fuse_ticks: u32,
    /// How near to a machine, the bell or a keeper one has to stand.
    #[schema(unit = "m", default = 1.4)]
    pub reach: f32,
    /// Walking speed.
    #[schema(unit = "m/s", default = 3.5)]
    pub speed: f32,
    /// How far a keeper sees with the lights on.
    #[schema(unit = "m", default = 6.5)]
    pub vision: f32,
    /// How far a keeper sees in the dark (the static always sees `vision`).
    #[schema(unit = "m", default = 2.5)]
    pub dark_vision: f32,
    /// The static's wait before the first freeze of a match.
    #[schema(unit = "ticks", default = 600)]
    pub first_freeze: u32,
    /// The static's wait between two freezes.
    #[schema(unit = "ticks", default = 900)]
    pub freeze_cooldown: u32,
    /// The static's wait between two blackouts.
    #[schema(unit = "ticks", default = 1800)]
    pub blackout_cooldown: u32,
    /// How long a blackout lasts unless the fuse box is fixed.
    #[schema(unit = "ticks", default = 1500)]
    pub blackout_ticks: u32,
    /// The wait after a meeting (and at the start) before the bell rings again.
    #[schema(unit = "ticks", default = 600)]
    pub bell_cooldown: u32,
    /// How long a meeting lasts at most.
    #[schema(unit = "ticks", default = 2400)]
    pub meeting_ticks: u32,
    /// How long the vote's result is shown.
    #[schema(unit = "ticks", default = 240)]
    pub result_ticks: u32,
    /// How long the end of a match is shown before the lobby opens again.
    #[schema(unit = "ticks", default = 600)]
    pub over_ticks: u32,
}

/// The role number of the static (`wish.json`: role 1).
const STATIC: u32 = 1;
/// The group bit of the frozen and the voted out: they see each other.
const GHOSTS: u32 = 1;
/// A vote for nobody.
const SKIP: u32 = 99;
/// The chat lines a player can say (with the chosen player's name).
const PHRASES: [&str; 4] = [
    "I was repairing a machine.",
    "I suspect {}.",
    "{} was with me.",
    "Let's skip this one.",
];

/// Where a seat stands around the bell: 10 places on a circle of 2.6 m.
const RING: [[f32; 2]; 10] = [
    [0.0, 2.6],
    [1.528, 2.103],
    [2.473, 0.803],
    [2.473, -0.803],
    [1.528, -2.103],
    [0.0, -2.6],
    [-1.528, -2.103],
    [-2.473, -0.803],
    [-2.473, 0.803],
    [-1.528, 2.103],
];

/// The colours of the seats (`wish.json` `colors`: 10), linear RGB.
const PALETTE: [[f32; 3]; 10] = [
    [0.90, 0.18, 0.16],
    [0.16, 0.38, 0.95],
    [0.20, 0.75, 0.25],
    [1.00, 0.55, 0.10],
    [0.60, 0.25, 0.85],
    [0.15, 0.80, 0.85],
    [0.95, 0.85, 0.20],
    [1.00, 0.45, 0.70],
    [0.55, 0.35, 0.20],
    [0.85, 0.85, 0.90],
];

/// Where unused keepers and ghosts wait, out of everyone's sight.
const PARKED: Vec3 = Vec3::new(-50.0, -50.0, 0.0);

// ------------------------------------------------------------- the station

/// Where a match is.
#[schema]
pub enum Phase {
    /// Players gather in the hall; the host starts when everyone is ready.
    Lobby,
    /// The match is on: repairs, freezes, blackouts.
    Playing,
    /// The bell rang: everyone talks and votes.
    Meeting,
    /// The vote's result is shown.
    Result,
    /// The match is over (`winner`).
    Over,
}

/// Who won.
#[schema]
pub enum Winner {
    /// Nobody yet.
    Nobody,
    /// The keepers.
    Keepers,
    /// The static.
    Static,
}

/// The state of the match, as every player sees it (one entity).
#[component]
pub struct Station {
    /// Where the match is.
    pub phase: Phase,
    /// The lobby's round this match was set up for.
    pub round: u32,
    /// Machines repaired.
    pub repairs: u32,
    /// Machines to repair.
    pub repairs_needed: u32,
    /// Ticks left of the blackout (0: the lights are on).
    #[schema(unit = "ticks")]
    pub blackout: u32,
    /// Ticks until the bell can ring again.
    #[schema(unit = "ticks")]
    pub bell_wait: u32,
    /// Ticks left of the meeting, the result or the end screen.
    #[schema(unit = "ticks")]
    pub timer: u32,
    /// Who rang the bell.
    pub caller: PlayerId,
    /// The frozen keeper whose statue was found, plus one (0: the bell).
    pub found: u32,
    /// Who has voted, as bits (who voted for whom stays with the host).
    pub voted: u32,
    /// The keeper the last vote sent into the storm, plus one (0: nobody).
    pub ejected: u32,
    /// Whether that keeper was the static.
    pub ejected_static: bool,
    /// Who won, once `phase` is `Over`.
    pub winner: Winner,
    /// How: 1 the repairs, 2 the static voted out, 3 keepers frozen.
    pub how: u32,
}

/// A keeper's body: one per seat, in the scene (`a0` to `a9`).
#[component]
pub struct Avatar {
    /// The seat.
    #[schema(range = 0..=15)]
    pub player: PlayerId,
    /// Whether the seat is taken in this match.
    pub present: bool,
    /// Whether this keeper is out (frozen or voted out): a ghost.
    pub out: bool,
    /// Whether it was frozen (an ice statue stands where it was).
    pub frozen: bool,
    /// Whether the statue was found and taken away.
    pub found: bool,
}

/// A ghost: where an out keeper drifts, seen only by the other ghosts.
#[component]
pub struct Ghost {
    /// The seat.
    #[schema(range = 0..=15)]
    pub player: PlayerId,
    /// Whether this keeper is a ghost now.
    pub active: bool,
}

/// What only its own player sees: waits and the work in hand.
#[component]
pub struct Private {
    /// The seat.
    #[schema(range = 0..=15)]
    pub player: PlayerId,
    /// The static: ticks until it can freeze again.
    #[schema(unit = "ticks")]
    pub freeze_wait: u32,
    /// The static: ticks until it can black the lights out again.
    #[schema(unit = "ticks")]
    pub blackout_wait: u32,
}

/// What a machine is for.
#[schema]
pub enum Task {
    /// The diesel generator.
    Generator,
    /// The radio.
    Radio,
    /// The antenna on the deck.
    Antenna,
    /// The telescope in the dome.
    Telescope,
    /// The boiler.
    Boiler,
    /// The fuse box: brings the lights back after a blackout.
    Fuse,
}

/// A machine to repair.
#[component]
pub struct Console {
    /// Which.
    pub task: Task,
    /// Whether it is repaired.
    pub done: bool,
    /// Ticks of repair so far.
    #[schema(unit = "ticks")]
    pub progress: u32,
}

/// A sliding door: open while a keeper stands near.
#[component]
pub struct Door {
    /// Whether it is open.
    pub open: bool,
}

/// The bell in the hall (its place is where everyone meets).
#[component]
pub struct Bell {}

/// The camera that follows this machine's own keeper.
#[component]
pub struct Follow {}

/// What this machine's player sees with: the vision and the lamps follow it.
#[component]
pub struct Eye {}

/// A lamp of the station: dimmed by a blackout.
#[component]
pub struct Lamp {
    /// Its intensity with the lights on.
    #[schema(range = 0.0..=100.0, default = 1.0)]
    pub bright: f32,
}

/// A text of the interface, filled by `show` (by its slot).
#[component]
pub struct Label {
    /// Which text.
    pub slot: u32,
}

/// A box of the interface shown in some phases only.
#[component]
pub struct Panel {
    /// 0 the lobby, 1 the meeting, 2 the result and the end, 3 the play hud.
    pub screen: u32,
}

// ------------------------------------------------------------- messages

/// A vote, to the host: a seat, or 99 for nobody.
#[event]
pub struct Vote {
    /// From this player to the host.
    pub net: Net,
    /// The seat voted for (99: skip).
    pub choice: u32,
}

/// Something a player says, to the host (who passes it on).
#[event]
pub struct Say {
    /// From this player to the host.
    pub net: Net,
    /// Which phrase (0 to 3).
    pub phrase: u32,
    /// The seat the phrase names.
    pub about: u32,
}

/// A chat line, from the host to everyone (or to the ghosts, for a ghost's).
#[event]
pub struct ChatLine {
    /// From the host.
    pub net: Net,
    /// Who said it.
    pub speaker: u32,
    /// Which phrase.
    pub phrase: u32,
    /// The seat it names.
    pub about: u32,
    /// Whether a ghost said it (only ghosts hear ghosts).
    pub ghost: bool,
}

/// The vote's result, from the host to everyone.
#[event]
pub struct VoteResult {
    /// From the host.
    pub net: Net,
    /// The keeper sent into the storm, plus one (0: nobody).
    pub ejected: u32,
    /// Votes for each seat.
    pub votes: [u32; 16],
    /// Votes to skip.
    pub skips: u32,
}

// -------------------------------------------------------- this machine only

/// This machine's own state: the meeting's cursor, the chat, what it heard.
#[resource]
pub struct Local {
    /// The seat chosen in the meeting (99: skip).
    pub choice: u32,
    /// Whether this player has voted in this meeting.
    pub voted: bool,
    /// The phase seen last tick (to notice changes).
    pub seen: Phase,
    /// The last six chat lines, the newest last.
    pub chat: [Text; 6],
    /// The last vote's result as a line.
    pub result: Text,
    /// Whether `move` was pushed up or down last tick (for the cursor).
    pub held: bool,
}

/// The host's secrets: who voted for whom, and the bell's count.
#[resource]
pub struct Ballots {
    /// Each seat's vote plus one (0: none yet).
    pub votes: [u32; 16],
    /// Meetings so far.
    pub meetings: u32,
}

// ------------------------------------------------------------- helpers

/// Whether this machine runs the match: the host, or a game alone.
fn authority(local: &LocalPlayer, lobby: &Lobby) -> bool {
    local.host || lobby.phase == LobbyPhase::Offline
}

fn flat(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.y)
}

fn near(a: Vec3, b: Vec3, reach: f32) -> bool {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy <= reach * reach
}

fn role_of(roles: &MatchRoles, player: PlayerId) -> u32 {
    roles.roles.get(player as usize).copied().unwrap_or(0)
}

fn name_of(lobby: &Lobby, player: u32) -> String {
    let named = lobby
        .seats
        .get(player as usize)
        .map(|seat| text_of(seat.name))
        .unwrap_or_default();
    if named.is_empty() {
        format!("Keeper {}", player + 1)
    } else {
        named
    }
}

fn seat_color(lobby: &Lobby, player: PlayerId) -> Color {
    let index = lobby
        .seats
        .get(player as usize)
        .map_or(player, |seat| seat.color) as usize;
    let [r, g, b] = PALETTE[index % PALETTE.len()];
    Color::rgb(r, g, b)
}

/// Whether a seat plays this match: a taken seat online, seat 0 alone.
fn seated(lobby: &Lobby, player: PlayerId) -> bool {
    if lobby.phase == LobbyPhase::Offline {
        return player == 0;
    }
    lobby
        .seats
        .get(player as usize)
        .is_some_and(|seat| seat.taken && !seat.left)
}

fn bell_at(bells: &Query<(&'static Bell, &'static Transform)>) -> Vec3 {
    bells
        .iter()
        .next()
        .map_or(Vec3::new(22.0, 16.0, 0.0), |(_, t)| t.translation)
}

fn ring(bell: Vec3, player: PlayerId) -> Vec3 {
    let [x, y] = RING[player as usize % RING.len()];
    Vec3::new(bell.x + x, bell.y + y, 0.0)
}

// ------------------------------------------------- the host: rounds

/// Sets a match up when the lobby starts one (or, alone, a practice round),
/// keeps the lobby's keepers in the hall, and opens the lobby again after
/// the end screen.
#[system]
#[allow(clippy::too_many_arguments)]
fn rounds(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    mut groups: ResMut<NetGroups>,
    mut ballots: ResMut<Ballots>,
    bells: Query<(&Bell, &Transform)>,
    mut stations: Query<&mut Station>,
    mut avatars: Query<(
        &mut Avatar,
        &mut Transform,
        &mut Sprite,
        &mut SpriteAnimator,
        &mut CharacterMotion,
    )>,
    mut privates: Query<&mut Private>,
    mut consoles: Query<&mut Console>,
    mut requests: EventWriter<LobbyRequest>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let bell = bell_at(&bells);
    let Some(station) = stations.single_mut() else {
        return;
    };
    let offline = lobby.phase == LobbyPhase::Offline;
    let starting = match lobby.phase {
        LobbyPhase::Running => station.round != lobby.round,
        LobbyPhase::Offline => {
            station.phase == Phase::Lobby || (station.phase == Phase::Over && station.timer == 0)
        }
        _ => false,
    };
    if starting {
        *station = Station {
            phase: Phase::Playing,
            round: if offline { station.round + 1 } else { lobby.round },
            repairs_needed: rules.repairs_needed,
            bell_wait: rules.bell_cooldown,
            ..Default::default()
        };
        *ballots = Ballots::default();
        *groups = NetGroups::default();
        for console in consoles.iter_mut() {
            console.done = false;
            console.progress = 0;
        }
        for private in privates.iter_mut() {
            private.freeze_wait = rules.first_freeze;
            private.blackout_wait = rules.first_freeze;
        }
    }
    let lobby_open = matches!(lobby.phase, LobbyPhase::Open | LobbyPhase::Ended);
    if lobby_open && station.phase != Phase::Lobby && station.phase != Phase::Over {
        station.phase = Phase::Lobby;
    }
    if !offline && lobby.phase == LobbyPhase::Open && station.phase == Phase::Over {
        station.phase = Phase::Lobby;
    }
    // Who plays: keepers of empty seats wait out of sight.
    for (avatar, transform, sprite, animator, motion) in avatars.iter_mut() {
        let present = seated(&lobby, avatar.player);
        if starting || present != avatar.present {
            avatar.present = present;
            avatar.out = false;
            avatar.frozen = false;
            avatar.found = false;
            transform.translation = if present {
                ring(bell, avatar.player)
            } else {
                PARKED
            };
            motion.velocity = Vec3::ZERO;
            animator.play(text("idle"));
        }
        if present {
            sprite.color = seat_color(&lobby, avatar.player);
        }
    }
    // The end screen, then back to the lobby (or, alone, a new round).
    if station.phase == Phase::Over && station.timer > 0 {
        station.timer -= 1;
        if station.timer == 0 && lobby.phase == LobbyPhase::Ended {
            requests.send(LobbyRequest {
                action: LobbyAction::Reopen,
                ..Default::default()
            });
        }
    }
}

// ------------------------------------------------- the host: moving

/// Walks every keeper with its player's `move`, and every ghost.
#[system(after = "rounds")]
fn walk(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    time: Res<Time>,
    players: PlayerActions,
    stations: Query<&Station>,
    mut avatars: Query<(
        &Avatar,
        &mut CharacterMotion,
        &mut Sprite,
        &mut SpriteAnimator,
    )>,
    mut ghosts: Query<(&Ghost, &mut Transform, &mut Sprite)>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let phase = stations.single().map_or(Phase::Lobby, |s| s.phase);
    let free = matches!(phase, Phase::Lobby | Phase::Playing);
    for (avatar, motion, sprite, animator) in avatars.iter_mut() {
        let go = if free && avatar.present && !avatar.out {
            players.of(avatar.player).axis2("move")
        } else {
            Vec2::ZERO
        };
        motion.velocity = Vec3::new(go.x * rules.speed, go.y * rules.speed, 0.0);
        if go.x != 0.0 {
            sprite.flip_x = go.x < 0.0;
        }
        let clip = if avatar.frozen {
            "frozen"
        } else if go.x != 0.0 || go.y != 0.0 {
            "walk"
        } else {
            "idle"
        };
        animator.play(text(clip));
    }
    for (ghost, transform, sprite) in ghosts.iter_mut() {
        if !ghost.active {
            continue;
        }
        let go = if phase == Phase::Playing {
            players.of(ghost.player).axis2("move")
        } else {
            Vec2::ZERO
        };
        let to = transform.translation + Vec3::new(go.x, go.y, 0.0) * rules.speed * time.delta;
        transform.translation = Vec3::new(to.x.clamp(0.0, 44.0), to.y.clamp(0.0, 30.0), 0.0);
        if go.x != 0.0 {
            sprite.flip_x = go.x < 0.0;
        }
    }
}

/// Opens a door while a keeper stands near it.
#[system(after = "walk")]
fn doors(
    avatars: Query<(&Avatar, &Transform)>,
    mut doors: Query<(&Transform, &mut Door, &mut Occluder, &mut Collider, &mut Sprite)>,
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let walkers: Vec<Vec3> = avatars
        .iter()
        .filter(|(a, _)| a.present && !a.out)
        .map(|(_, t)| t.translation)
        .collect();
    for (at, door, occluder, collider, sprite) in doors.iter_mut() {
        let open = walkers.iter().any(|w| near(*w, at.translation, 1.9));
        door.open = open;
        occluder.enabled = !open;
        collider.sensor = open;
        sprite.color.a = if open { 0.15 } else { 1.0 };
    }
}

// ------------------------------------------------- the host: playing

/// Repairs, freezes, blackouts and the bell: what players do with `use`,
/// `sabotage` and `bell` while the match is on.
#[system(after = "doors")]
#[allow(clippy::too_many_arguments)]
fn work(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    roles: Res<MatchRoles>,
    players: PlayerActions,
    bells: Query<(&Bell, &Transform)>,
    mut stations: Query<&mut Station>,
    mut avatars: Query<(&mut Avatar, &Transform)>,
    ghosts: Query<(&Ghost, &Transform)>,
    mut privates: Query<&mut Private>,
    mut consoles: Query<(&mut Console, &Transform, &mut Sprite)>,
    mut sounds: EventWriter<PlaySound>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let bell = bell_at(&bells);
    let Some(station) = stations.single_mut() else {
        return;
    };
    if station.phase != Phase::Playing {
        return;
    }
    station.blackout = station.blackout.saturating_sub(1);
    station.bell_wait = station.bell_wait.saturating_sub(1);
    for private in privates.iter_mut() {
        private.freeze_wait = private.freeze_wait.saturating_sub(1);
        private.blackout_wait = private.blackout_wait.saturating_sub(1);
    }
    // Everyone's place now (keepers, and ghosts for those who are out).
    let bodies: Vec<(PlayerId, Vec3, bool, bool, bool)> = avatars
        .iter()
        .map(|(a, t)| (a.player, t.translation, a.present, a.out, a.frozen && !a.found))
        .collect();
    let ghost_at: Vec<(PlayerId, Vec3)> = ghosts
        .iter()
        .filter(|(g, _)| g.active)
        .map(|(g, t)| (g.player, t.translation))
        .collect();
    let mut frozen_now: Vec<PlayerId> = Vec::new();
    for &(player, at, present, out, _) in &bodies {
        if !present {
            continue;
        }
        let me = players.of(player);
        let is_static = role_of(&roles, player) == STATIC;
        let hands = if out {
            ghost_at.iter().find(|(p, _)| *p == player).map(|(_, at)| *at)
        } else {
            Some(at)
        };
        let Some(hands) = hands else {
            continue;
        };
        if me.held("use") {
            // The static freezes the nearest keeper within reach.
            let mut froze = false;
            if is_static && !out {
                let ready = privates
                    .iter()
                    .find(|p| p.player == player)
                    .is_some_and(|p| p.freeze_wait == 0);
                let victim = bodies
                    .iter()
                    .filter(|(p, other, there, gone, _)| {
                        *p != player
                            && *there
                            && !*gone
                            && role_of(&roles, *p) != STATIC
                            && near(at, *other, rules.reach)
                            && !frozen_now.contains(p)
                    })
                    .min_by(|a, b| {
                        let da = (a.1 - at).length();
                        let db = (b.1 - at).length();
                        da.total_cmp(&db).then(a.0.cmp(&b.0))
                    })
                    .map(|(p, ..)| *p);
                if let (true, Some(victim)) = (ready, victim) {
                    frozen_now.push(victim);
                    if let Some(private) = privates.iter_mut().find(|p| p.player == player) {
                        private.freeze_wait = rules.freeze_cooldown;
                    }
                    froze = true;
                }
            }
            // Keepers (and keepers' ghosts) repair; the static only pretends.
            if !froze && !is_static {
                for (console, place, sprite) in consoles.iter_mut() {
                    if !near(hands, place.translation, rules.reach) {
                        continue;
                    }
                    if console.task == Task::Fuse {
                        if station.blackout > 0 {
                            console.progress += 1;
                            if console.progress >= rules.fuse_ticks {
                                console.progress = 0;
                                station.blackout = 0;
                                sounds.send(PlaySound {
                                    clip: asset("assets/sounds/repair.wav"),
                                    ..Default::default()
                                });
                            }
                        }
                    } else if !console.done {
                        console.progress += 1;
                        if console.progress >= rules.repair_ticks {
                            console.done = true;
                            station.repairs += 1;
                            sprite.color = Color::rgb(0.6, 1.0, 0.7);
                            sounds.send(PlaySound {
                                clip: asset("assets/sounds/repair.wav"),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        // The static drowns the lights.
        if is_static && !out && me.held("sabotage") && station.blackout == 0 {
            let ready = privates
                .iter()
                .find(|p| p.player == player)
                .is_some_and(|p| p.blackout_wait == 0);
            if ready {
                station.blackout = rules.blackout_ticks;
                if let Some(private) = privates.iter_mut().find(|p| p.player == player) {
                    private.blackout_wait = rules.blackout_cooldown;
                }
            }
        }
        // Anyone standing can ring the bell, or call over a statue.
        if !out && me.held("bell") && station.phase == Phase::Playing {
            let statue = bodies
                .iter()
                .find(|(_, other, _, _, statue)| *statue && near(at, *other, rules.reach))
                .map(|(p, ..)| *p);
            let at_bell = near(at, bell, rules.reach + 0.6) && station.bell_wait == 0;
            if statue.is_some() || at_bell {
                station.phase = Phase::Meeting;
                station.caller = player;
                station.found = statue.map_or(0, |p| p + 1);
            }
        }
    }
    for (avatar, _) in avatars.iter_mut() {
        if frozen_now.contains(&avatar.player) {
            avatar.out = true;
            avatar.frozen = true;
        }
    }
    if !frozen_now.is_empty() {
        sounds.send(PlaySound {
            clip: asset("assets/sounds/freeze.wav"),
            ..Default::default()
        });
    }
}

/// An out keeper's ghost rises where its keeper was; a ghost in the dark
/// stays dark: a ghost is seen by the ghosts only (`Replicated` group 1).
#[system(after = "work")]
fn haunt(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    mut groups: ResMut<NetGroups>,
    avatars: Query<(&Avatar, &Transform)>,
    mut ghosts: Query<(&mut Ghost, &mut Transform, &mut Sprite)>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let out: Vec<(PlayerId, Vec3, bool)> = avatars
        .iter()
        .map(|(a, t)| (a.player, t.translation, a.present && a.out))
        .collect();
    for (ghost, transform, sprite) in ghosts.iter_mut() {
        let wanted = out.iter().any(|(p, _, o)| *p == ghost.player && *o);
        if wanted && !ghost.active {
            ghost.active = true;
            let from = out
                .iter()
                .find(|(p, ..)| *p == ghost.player)
                .map_or(PARKED, |(_, at, _)| *at);
            transform.translation = Vec3::new(from.x, from.y + 0.6, 0.0);
            sprite.color = seat_color(&lobby, ghost.player);
            sprite.color.a = 0.8;
        } else if !wanted && ghost.active {
            ghost.active = false;
            transform.translation = PARKED;
        }
        if let Some(bits) = groups.players.get_mut(ghost.player as usize) {
            *bits = if ghost.active { GHOSTS } else { 0 };
        }
    }
}

// ------------------------------------------------- the host: meetings

/// Gathers everyone at the bell, passes chat on, counts the votes and sends
/// one keeper into the storm.
#[system(after = "haunt")]
#[allow(clippy::too_many_arguments)]
fn meeting(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    roles: Res<MatchRoles>,
    mut ballots: ResMut<Ballots>,
    bells: Query<(&Bell, &Transform)>,
    mut stations: Query<&mut Station>,
    mut avatars: Query<(&mut Avatar, &mut Transform, &mut CharacterMotion)>,
    votes: NetReader<Vote>,
    said: NetReader<Say>,
    mut chat: NetWriter<ChatLine>,
    mut results: NetWriter<VoteResult>,
    mut sounds: EventWriter<PlaySound>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let bell = bell_at(&bells);
    let Some(station) = stations.single_mut() else {
        return;
    };
    // Who is in the match, and who still stands.
    let seats: Vec<(u32, bool)> = avatars
        .iter()
        .filter(|(a, ..)| a.present)
        .map(|(a, ..)| (a.player, !a.out))
        .collect();
    let present = |player: u32| seats.iter().any(|(p, _)| *p == player);
    let standing = |player: u32| seats.iter().any(|(p, up)| *p == player && *up);
    // Chat: the standing are heard by all, ghosts by ghosts.
    for line in said.read() {
        let speaker = line.net.from;
        if line.phrase as usize >= PHRASES.len() || !present(speaker) {
            continue;
        }
        let ghost = !standing(speaker);
        if !ghost && station.phase != Phase::Meeting {
            continue;
        }
        let out = ChatLine {
            speaker,
            phrase: line.phrase,
            about: line.about.min(SKIP),
            ghost,
            ..Default::default()
        };
        if ghost {
            chat.to_group(GHOSTS, out);
        } else {
            chat.to_everyone(out);
        }
    }
    // The bell just rang: everyone to the hall.
    if station.phase == Phase::Meeting && station.timer == 0 && station.voted == 0 {
        ballots.meetings += 1;
        ballots.votes = [0; 16];
        station.timer = rules.meeting_ticks;
        station.blackout = 0;
        station.ejected = 0;
        station.ejected_static = false;
        for (avatar, transform, motion) in avatars.iter_mut() {
            motion.velocity = Vec3::ZERO;
            if avatar.present && !avatar.out {
                transform.translation = ring(bell, avatar.player);
            } else if avatar.frozen && !avatar.found {
                // The statues are carried out of the cold.
                avatar.found = true;
                transform.translation = PARKED;
            }
        }
        sounds.send(PlaySound {
            clip: asset("assets/sounds/bell.wav"),
            ..Default::default()
        });
    }
    if station.phase == Phase::Meeting {
        for vote in votes.read() {
            let voter = vote.net.from;
            let valid = vote.choice == SKIP || standing(vote.choice);
            if !valid || !standing(voter) || station.voted & (1 << voter) != 0 {
                continue;
            }
            if let Some(slot) = ballots.votes.get_mut(voter as usize) {
                *slot = vote.choice + 1;
                station.voted |= 1 << voter;
            }
        }
        station.timer = station.timer.saturating_sub(1);
        let everyone = avatars
            .iter()
            .filter(|(a, ..)| a.present && !a.out)
            .all(|(a, ..)| station.voted & (1 << a.player) != 0);
        if everyone || station.timer == 0 {
            let mut counts = [0u32; 16];
            let mut skips = 0;
            for &ballot in ballots.votes.iter() {
                match ballot {
                    0 => {}
                    b if b == SKIP + 1 => skips += 1,
                    b => {
                        if let Some(count) = counts.get_mut(b as usize - 1) {
                            *count += 1;
                        }
                    }
                }
            }
            let most = counts.iter().copied().max().unwrap_or(0);
            let leaders = counts.iter().filter(|c| **c == most).count();
            let ejected = if most > 0 && leaders == 1 && most > skips {
                counts.iter().position(|c| *c == most).map(|p| p as u32)
            } else {
                None
            };
            if let Some(seat) = ejected {
                for (avatar, transform, _) in avatars.iter_mut() {
                    if avatar.player == seat {
                        avatar.out = true;
                        transform.translation = ring(bell, seat);
                    }
                }
                station.ejected = seat + 1;
                station.ejected_static = role_of(&roles, seat) == STATIC;
            }
            results.to_everyone(VoteResult {
                ejected: ejected.map_or(0, |s| s + 1),
                votes: counts,
                skips,
                ..Default::default()
            });
            station.phase = Phase::Result;
            station.timer = rules.result_ticks;
            station.voted = 0;
        }
    } else if station.phase == Phase::Result {
        station.timer = station.timer.saturating_sub(1);
        if station.timer == 0 {
            // The ejected keeper walks out into the storm.
            for (avatar, transform, _) in avatars.iter_mut() {
                if avatar.out && !avatar.frozen && avatar.present {
                    transform.translation = PARKED;
                }
            }
            station.phase = Phase::Playing;
            station.bell_wait = rules.bell_cooldown;
        }
    }
}

/// Decides the match: repairs done, the static voted out, or as many
/// statics standing as keepers.
#[system(after = "meeting")]
fn judge(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    roles: Res<MatchRoles>,
    avatars: Query<&Avatar>,
    mut stations: Query<&mut Station>,
    mut requests: EventWriter<LobbyRequest>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let Some(station) = stations.single_mut() else {
        return;
    };
    if station.phase != Phase::Playing {
        return;
    }
    let (mut statics, mut statics_standing, mut keepers_standing) = (0, 0, 0);
    for avatar in avatars.iter().filter(|a| a.present) {
        let is_static = role_of(&roles, avatar.player) == STATIC;
        statics += u32::from(is_static);
        if !avatar.out {
            if is_static {
                statics_standing += 1;
            } else {
                keepers_standing += 1;
            }
        }
    }
    let (winner, how) = if station.repairs >= station.repairs_needed {
        (Winner::Keepers, 1)
    } else if statics > 0 && statics_standing == 0 {
        (Winner::Keepers, 2)
    } else if statics > 0 && keepers_standing <= statics_standing {
        (Winner::Static, 3)
    } else {
        return;
    };
    station.phase = Phase::Over;
    station.winner = winner;
    station.how = how;
    station.timer = rules.over_ticks;
    if lobby.phase == LobbyPhase::Running {
        requests.send(LobbyRequest {
            action: LobbyAction::End,
            value: how,
            ..Default::default()
        });
    }
}

/// Who receives each keeper: whoever can see it (within their sight and not
/// behind a wall or a closed door), the ghosts, and everyone at the bell.
#[system(after = "judge")]
fn who_sees(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    roles: Res<MatchRoles>,
    stations: Query<&Station>,
    mut avatars: Query<(&Avatar, &Transform, &mut Replicated)>,
) {
    if !authority(&local, &lobby) {
        return;
    }
    let Some(station) = stations.single() else {
        return;
    };
    let open = station.phase != Phase::Playing;
    let dark = station.blackout > 0;
    let viewers: Vec<(PlayerId, Vec3, bool, f32)> = avatars
        .iter()
        .filter(|(a, ..)| a.present)
        .map(|(a, t, _)| {
            let reach = if dark && role_of(&roles, a.player) != STATIC {
                rules.dark_vision
            } else {
                rules.vision
            };
            (a.player, t.translation, a.out, reach)
        })
        .collect();
    for (avatar, transform, replicated) in avatars.iter_mut() {
        let mut bits = 0u32;
        if avatar.present {
            for &(viewer, from, ghost, reach) in &viewers {
                let sees = viewer == avatar.player
                    || open
                    || ghost
                    || (near(from, transform.translation, reach + 0.5)
                        && can_see(flat(from), flat(transform.translation)));
                if sees {
                    bits |= 1 << viewer;
                }
            }
        }
        replicated.players = bits;
    }
}

// --------------------------------------------- every machine: what it shows

/// This machine's camera and vision follow its own keeper (or ghost); a
/// blackout dims the lamps and shortens a keeper's sight.
#[system(after = "who_sees")]
#[allow(clippy::too_many_arguments)]
fn view(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    rules: Res<Rules>,
    mut lighting: ResMut<Lighting2D>,
    stations: Query<&Station>,
    avatars: Query<(&Avatar, &Transform)>,
    ghosts: Query<(&Ghost, &Transform)>,
    mut cameras: Query<(&Follow, &mut Transform)>,
    mut eyes: Query<(&Eye, &mut Transform, &mut Vision2D)>,
    mut lamps: Query<(&Lamp, &mut Light2D)>,
) {
    let me = if lobby.phase == LobbyPhase::Offline {
        0
    } else {
        local.player
    };
    let station = stations.single();
    let dark = station.is_some_and(|s| s.blackout > 0);
    let playing = station.is_some_and(|s| s.phase == Phase::Playing);
    let mine = avatars.iter().find(|(a, _)| a.player == me && a.present);
    let ghost = ghosts
        .iter()
        .find(|(g, _)| g.player == me && g.active)
        .map(|(_, t)| t.translation);
    let out = mine.is_some_and(|(a, _)| a.out);
    let focus = match (out, ghost, mine) {
        (true, Some(at), _) => Some(at),
        (_, _, Some((_, t))) => Some(t.translation),
        _ => None,
    };
    let joined = local.joined || lobby.phase == LobbyPhase::Offline;
    let focus = focus.filter(|_| joined);
    for (_, transform) in cameras.iter_mut() {
        if let Some(at) = focus {
            transform.translation.x = at.x;
            transform.translation.y = at.y + 0.5;
        }
    }
    let is_static = local.role == STATIC && lobby.phase != LobbyPhase::Offline;
    for (_, transform, vision) in eyes.iter_mut() {
        if let Some(at) = focus {
            transform.translation = Vec3::new(at.x, at.y + 0.4, 0.0);
        }
        vision.enabled = playing && !out && focus.is_some();
        vision.radius = if dark && !is_static {
            rules.dark_vision
        } else {
            rules.vision
        };
    }
    lighting.enabled = true;
    lighting.ambient = if dark {
        Color::rgb(0.05, 0.05, 0.09)
    } else {
        Color::rgb(0.45, 0.5, 0.6)
    };
    for (lamp, light) in lamps.iter_mut() {
        light.intensity = if dark { lamp.bright * 0.1 } else { lamp.bright };
    }
}

/// This machine's own hands: getting ready and starting in the lobby;
/// choosing, voting and talking in a meeting.
#[system(after = "view")]
#[allow(clippy::too_many_arguments)]
fn hands(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    actions: Actions,
    mut mine: ResMut<Local>,
    stations: Query<&Station>,
    avatars: Query<&Avatar>,
    mut requests: EventWriter<LobbyRequest>,
    mut votes: NetWriter<Vote>,
    mut says: NetWriter<Say>,
    mut sounds: EventWriter<PlaySound>,
) {
    let me = local.player;
    let Some(station) = stations.single() else {
        return;
    };
    // The lobby: `use` toggles ready; the host starts once all are ready.
    if lobby.phase == LobbyPhase::Open && local.joined && actions.pressed("use") {
        let seat = lobby.seats.get(me as usize).copied().unwrap_or_default();
        let ready = lobby
            .seats
            .iter()
            .filter(|s| s.taken && s.connected)
            .all(|s| s.ready);
        let action = if local.host && ready {
            LobbyAction::Start
        } else if seat.ready {
            LobbyAction::NotReady
        } else {
            LobbyAction::Ready
        };
        requests.send(LobbyRequest {
            action,
            ..Default::default()
        });
    }
    let standing: Vec<u32> = avatars
        .iter()
        .filter(|a| a.present && !a.out)
        .map(|a| a.player)
        .collect();
    let me_standing = standing.contains(&me);
    // A new meeting: the cursor starts on the first keeper.
    if station.phase == Phase::Meeting && mine.seen != Phase::Meeting {
        mine.voted = false;
        mine.choice = standing.first().copied().unwrap_or(SKIP);
        sounds.send(PlaySound {
            clip: asset("assets/sounds/bell.wav"),
            ..Default::default()
        });
    }
    mine.seen = station.phase;
    if station.phase == Phase::Meeting && me_standing {
        // Up and down move the cursor over the keepers standing, then "skip".
        let mut choices = standing.clone();
        choices.push(SKIP);
        let go = actions.axis2("move");
        let pushed = go.y.abs() > 0.5;
        if pushed && !mine.held {
            let at = choices.iter().position(|c| *c == mine.choice).unwrap_or(0);
            let next = if go.y < 0.0 {
                (at + 1) % choices.len()
            } else {
                (at + choices.len() - 1) % choices.len()
            };
            mine.choice = choices[next];
        }
        mine.held = pushed;
        if actions.pressed("use") && !mine.voted {
            mine.voted = true;
            votes.to_host(Vote {
                choice: mine.choice,
                ..Default::default()
            });
            sounds.send(PlaySound {
                clip: asset("assets/sounds/vote.wav"),
                ..Default::default()
            });
        }
    }
    // Talking: the standing in meetings, the ghosts at any time.
    let may_talk = (station.phase == Phase::Meeting && me_standing)
        || (!me_standing && avatars.iter().any(|a| a.player == me && a.present));
    if may_talk {
        for (phrase, action) in ["say_1", "say_2", "say_3", "say_4"].iter().enumerate() {
            if actions.pressed(action) {
                says.to_host(Say {
                    phrase: phrase as u32,
                    about: mine.choice,
                    ..Default::default()
                });
            }
        }
    }
}

/// Hears chat lines and the vote's result.
#[system(after = "hands")]
fn listen(
    lobby: Res<Lobby>,
    lines: NetReader<ChatLine>,
    results: NetReader<VoteResult>,
    mut mine: ResMut<Local>,
) {
    for line in lines.read() {
        let who = name_of(&lobby, line.speaker);
        let about = if line.about == SKIP {
            "nobody".to_owned()
        } else {
            name_of(&lobby, line.about)
        };
        let said = PHRASES
            .get(line.phrase as usize)
            .copied()
            .unwrap_or("...")
            .replace("{}", &about);
        let ghost = if line.ghost { " (ghost)" } else { "" };
        mine.chat.rotate_left(1);
        mine.chat[5] = text(&format!("{who}{ghost}: {said}"));
    }
    for result in results.read() {
        let line = if result.ejected == 0 {
            format!("No one was sent out ({} skipped).", result.skips)
        } else {
            let seat = result.ejected - 1;
            let votes = result.votes.get(seat as usize).copied().unwrap_or(0);
            format!(
                "{} was sent into the storm ({votes} votes).",
                name_of(&lobby, seat)
            )
        };
        mine.result = text(&line);
    }
}

/// Writes the interface: the lobby, the meeting, the result and the hud.
#[system(after = "listen")]
#[allow(clippy::too_many_arguments)]
fn show(
    local: Res<LocalPlayer>,
    lobby: Res<Lobby>,
    mine: Res<Local>,
    stations: Query<&Station>,
    avatars: Query<&Avatar>,
    privates: Query<&Private>,
    mut panels: Query<(&Panel, &mut UiNode)>,
    mut labels: Query<(&Label, &mut UiText)>,
) {
    let Some(station) = stations.single() else {
        return;
    };
    let offline = lobby.phase == LobbyPhase::Offline;
    let me = if offline { 0 } else { local.player };
    let screen = match station.phase {
        Phase::Lobby => 0,
        Phase::Meeting => 1,
        Phase::Result | Phase::Over => 2,
        Phase::Playing => 3,
    };
    for (panel, node) in panels.iter_mut() {
        node.visible = panel.screen == screen;
    }
    let standing: Vec<u32> = avatars
        .iter()
        .filter(|a| a.present && !a.out)
        .map(|a| a.player)
        .collect();
    let me_out = avatars.iter().any(|a| a.player == me && a.present && a.out);
    let is_static = !offline && local.role == STATIC;
    let private = privates.iter().find(|p| p.player == me);
    for (label, ui) in labels.iter_mut() {
        let line = match label.slot {
            // The lobby.
            0 => format!("Room code: {}", text_of(lobby.room)),
            1 => {
                let seated = lobby.seats.iter().filter(|s| s.taken && !s.left).count();
                let ready = lobby
                    .seats
                    .iter()
                    .filter(|s| s.taken && !s.left && s.ready)
                    .count();
                if offline {
                    "Practice: repair the five machines.".to_owned()
                } else if local.host {
                    format!("{seated} keepers here, {ready} ready. Press E to start.")
                } else if lobby.seats.get(me as usize).is_some_and(|s| s.ready) {
                    "Ready. Waiting for the host...".to_owned()
                } else {
                    "Press E when you are ready.".to_owned()
                }
            }
            s @ 2..=11 => {
                let seat = s - 2;
                match lobby.seats.get(seat as usize) {
                    Some(info) if info.taken && !info.left => {
                        let mark = if info.ready { "ready" } else { "..." };
                        format!("{}  {mark}", name_of(&lobby, seat))
                    }
                    _ => String::new(),
                }
            }
            // The meeting.
            20 => {
                let why = if station.found > 0 {
                    format!(
                        "{} found {} frozen!",
                        name_of(&lobby, station.caller),
                        name_of(&lobby, station.found - 1)
                    )
                } else {
                    format!("{} rang the bell.", name_of(&lobby, station.caller))
                };
                format!("{why}  ({} s)", station.timer / 60)
            }
            s @ 21..=31 => {
                let row = (s - 21) as usize;
                let mut choices = standing.clone();
                choices.push(SKIP);
                match choices.get(row) {
                    Some(&choice) => {
                        let cursor = if choice == mine.choice { "> " } else { "  " };
                        let name = if choice == SKIP {
                            "Skip".to_owned()
                        } else {
                            name_of(&lobby, choice)
                        };
                        let voted = if choice != SKIP && station.voted & (1 << choice) != 0 {
                            "  (voted)"
                        } else {
                            ""
                        };
                        format!("{cursor}{name}{voted}")
                    }
                    None => String::new(),
                }
            }
            s @ 40..=45 => text_of(mine.chat[(s - 40) as usize]),
            // The result and the end.
            50 => match station.phase {
                Phase::Over => match (station.winner, station.how) {
                    (Winner::Keepers, 1) => "The keepers repaired Ridgewatch!".to_owned(),
                    (Winner::Keepers, _) => "The keepers cast the static out!".to_owned(),
                    _ => "The static froze Ridgewatch.".to_owned(),
                },
                _ => text_of(mine.result),
            },
            51 => match (station.phase, station.ejected) {
                (Phase::Over, _) => "Back to the lobby soon.".to_owned(),
                (_, 0) => String::new(),
                (_, _) if station.ejected_static => "They were the static.".to_owned(),
                _ => "They were a keeper.".to_owned(),
            },
            // The hud.
            60 => {
                if me_out {
                    "You are a ghost. Repair what you can (E).".to_owned()
                } else if is_static {
                    match private {
                        Some(p) if p.freeze_wait > 0 => {
                            format!("You are the static. Freeze in {} s.", p.freeze_wait / 60 + 1)
                        }
                        _ => "You are the static. E near a keeper freezes them.".to_owned(),
                    }
                } else {
                    "You are a keeper. Hold E at a machine to repair it.".to_owned()
                }
            }
            61 => format!(
                "Repairs {}/{}{}",
                station.repairs,
                station.repairs_needed,
                if station.blackout > 0 {
                    "   BLACKOUT: fix the fuse box!"
                } else {
                    ""
                }
            ),
            _ => continue,
        };
        ui.text = text(&line);
    }
}

wish_sdk::export! {
    components: [Station, Avatar, Ghost, Private, Console, Door, Bell, Follow, Eye, Lamp, Label, Panel],
    resources: [Rules, Local, Ballots],
    events: [Vote, Say, ChatLine, VoteResult],
    systems: [rounds, walk, doors, work, haunt, meeting, judge, who_sees, view, hands, listen, show],
}
