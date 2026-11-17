//! Islands: a small 3D platformer, the showcase game of Wish Engine (M7).
//! Run and jump across floating islands, pick up every crystal, reach the
//! goal. Built with the engine's own tools: the scene through `edit_apply`,
//! this code through `script_build`, the proof through scenario tests.

use wish_sdk::prelude::*;

/// The player: a physics character.
#[component]
pub struct Player {
    /// How fast the player runs.
    #[schema(unit = "m/s", range = 0.0..=20.0, default = 5.0)]
    pub speed: f32,
    /// How fast the player leaves the ground when jumping.
    #[schema(unit = "m/s", range = 0.0..=30.0, default = 6.5)]
    pub jump_speed: f32,
    /// How much the player can steer in the air (0: not at all, 1: fully).
    #[schema(range = 0.0..=1.0, default = 0.6)]
    pub air_control: f32,
    /// Where the player comes back after falling: the last checkpoint.
    #[schema(unit = "m", default = Vec3::new(0.0, 0.95, 1.0))]
    pub respawn: Vec3,
}

/// A camera that follows the player from behind and above.
#[component]
pub struct FollowCamera {
    /// Where the camera wants to be, from the player.
    #[schema(unit = "m", default = Vec3::new(0.0, 3.5, 7.0))]
    pub offset: Vec3,
    /// How quickly it catches up (higher is stiffer).
    #[schema(unit = "1/s", range = 0.1..=50.0, default = 5.0)]
    pub stiffness: f32,
    /// How far above the player's feet it looks.
    #[schema(unit = "m", default = 1.0)]
    pub look_height: f32,
}

/// A crystal to pick up (a sensor collider).
#[component]
pub struct Crystal {
    /// Points it is worth.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
}

/// A checkpoint (a sensor collider): touching it moves the respawn point.
#[component]
pub struct Checkpoint {
    /// Where the player comes back after falling.
    #[schema(unit = "m")]
    pub respawn: Vec3,
}

/// A platform that goes back and forth between two places (a kinematic
/// body: game code moves it, physics carries what stands on it).
#[component]
pub struct MovingPlatform {
    /// One end.
    #[schema(unit = "m")]
    pub from: Vec3,
    /// The other end.
    #[schema(unit = "m")]
    pub to: Vec3,
    /// Seconds from one end to the other.
    #[schema(unit = "s", range = 0.1..=600.0, default = 3.0)]
    pub seconds: f32,
    /// Seconds it waits at each end.
    #[schema(unit = "s", range = 0.0..=600.0, default = 1.0)]
    pub wait: f32,
}

/// The goal (a sensor collider): reaching it wins.
#[component]
pub struct Goal {
    /// Whether it has been reached.
    pub reached: bool,
}

/// The screen shown when the goal is reached (its text says how it went).
#[component]
pub struct WinBanner {
    /// Whether it is shown.
    pub shown: bool,
}

/// The text that shows how many crystals were picked up.
#[component]
pub struct CrystalLabel {
    /// The count it shows now.
    pub shown: u32,
}

/// How the run is going.
#[resource]
pub struct Game {
    /// Crystals picked up.
    pub crystals: u32,
    /// Crystals in the level (counted on the first tick).
    pub total: u32,
    /// Times the player fell off the world.
    pub falls: u32,
    /// Whether the goal was reached.
    pub won: bool,
    /// The tick it was reached at.
    pub won_at: u64,
}

/// Gravity, added by the game itself (a character is moved, not pushed).
const GRAVITY: f32 = 18.0;
/// Below this height the player has fallen off the world.
const FALL_LINE: f32 = -8.0;

fn sound(path: &str, at: Vec3) -> PlaySound {
    PlaySound {
        clip: asset(path),
        bus: AudioBus::Sfx,
        volume: 1.0,
        pitch: 1.0,
        spatial: false,
        position: at,
    }
}

/// Runs and jumps: says how fast the character wants to go. `move` is
/// [x, y], x to the right (+X) and y forward (-Z).
#[system]
fn run(
    actions: Actions,
    time: Res<Time>,
    mut players: Query<(&Player, &Transform, &mut CharacterMotion, &CharacterState)>,
    mut sounds: EventWriter<PlaySound>,
) {
    let go = actions.axis2("move");
    for (player, at, motion, state) in players.iter_mut() {
        let wanted = Vec3::new(go.x, 0.0, -go.y) * player.speed;
        let current = Vec3::new(motion.velocity.x, 0.0, motion.velocity.z);
        let horizontal = if state.grounded {
            wanted
        } else {
            // In the air the player only steers.
            let blend = (player.air_control * 10.0 * time.delta).min(1.0);
            current + (wanted - current) * blend
        };
        let mut vy = motion.velocity.y - GRAVITY * time.delta;
        if state.grounded {
            vy = if actions.pressed("jump") {
                sounds.send(sound("assets/sounds/jump.wav", at.translation));
                player.jump_speed
            } else {
                -1.0
            };
        }
        motion.velocity = Vec3::new(horizontal.x, vy, horizontal.z);
    }
}

/// A player who fell off the world comes back at the last checkpoint.
#[system]
fn fall(
    mut players: Query<(&Player, &mut Transform, &mut CharacterMotion)>,
    mut game: ResMut<Game>,
    mut sounds: EventWriter<PlaySound>,
) {
    for (player, at, motion) in players.iter_mut() {
        if at.translation.y < FALL_LINE {
            at.translation = player.respawn;
            motion.velocity = Vec3::ZERO;
            game.falls += 1;
            sounds.send(sound("assets/sounds/fall.wav", player.respawn));
        }
    }
}

/// Crystals and checkpoints the player touched.
#[system]
fn touch(
    touches: EventReader<CollisionStarted>,
    mut players: Query<(Entity, &mut Player)>,
    crystals: Query<(Entity, &Transform, &Crystal)>,
    checkpoints: Query<(Entity, &Checkpoint)>,
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut sounds: EventWriter<PlaySound>,
    mut commands: Commands,
) {
    for touch in touches.read() {
        for (who, player) in players.iter_mut() {
            let other = if touch.a == who {
                touch.b
            } else if touch.b == who {
                touch.a
            } else {
                continue;
            };
            if let Some((crystal, at, worth)) = crystals.iter().find(|(e, _, _)| *e == other) {
                game.crystals += worth.value;
                commands.despawn(crystal);
                sounds.send(sound("assets/sounds/crystal.wav", at.translation));
                commands
                    .spawn()
                    .insert(Transform::from_translation(at.translation))
                    .insert(ParticleEmitter {
                        start_time: time.elapsed as f32,
                        rate: 0.0,
                        burst: 20,
                        lifetime: 0.5,
                        speed: 2.0,
                        spread_deg: 90.0,
                        gravity: Vec3::new(0.0, -4.0, 0.0),
                        size_start: 0.12,
                        size_end: 0.0,
                        color_start: Color::rgba(0.5, 0.95, 1.0, 1.0),
                        color_end: Color::rgba(0.2, 0.6, 1.0, 0.0),
                        seed: game.crystals,
                        ..ParticleEmitter::default()
                    });
            }
            if let Some((_, checkpoint)) = checkpoints.iter().find(|(e, _)| *e == other)
                && player.respawn != checkpoint.respawn
            {
                player.respawn = checkpoint.respawn;
                sounds.send(sound("assets/sounds/checkpoint.wav", checkpoint.respawn));
            }
        }
    }
}

/// Moves platforms back and forth. Where a platform is follows from the
/// time alone, so it needs no state of its own.
#[system]
fn platforms(time: Res<Time>, mut moving: Query<(&MovingPlatform, &mut Transform)>) {
    for (platform, at) in moving.iter_mut() {
        let leg = platform.seconds.max(0.1) + platform.wait.max(0.0);
        let cycle = 2.0 * leg;
        let t = (time.elapsed as f32).rem_euclid(cycle);
        // 0 at `from`, 1 at `to`: wait, travel, wait, travel back.
        let along = if t < leg {
            ((t - platform.wait).max(0.0) / platform.seconds.max(0.1)).min(1.0)
        } else {
            1.0 - ((t - leg - platform.wait).max(0.0) / platform.seconds.max(0.1)).min(1.0)
        };
        // Smooth start and stop.
        let eased = along * along * (3.0 - 2.0 * along);
        at.translation = platform.from + (platform.to - platform.from) * eased;
    }
}

/// The text on the win banner.
#[component]
pub struct WinText {
    /// Unused.
    pub unused: u32,
}

/// Reaching the goal wins: the banner says how many crystals and how long.
#[system(after = "touch")]
fn finish(
    touches: EventReader<CollisionStarted>,
    players: Query<Entity, With<Player>>,
    mut goals: Query<(Entity, &mut Goal)>,
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut banners: Query<(&mut WinBanner, &mut UiNode)>,
    mut texts: Query<&mut UiText, (With<WinText>, Without<CrystalLabel>)>,
    mut sounds: EventWriter<PlaySound>,
) {
    let Some(player) = players.iter().next() else {
        return;
    };
    for touch in touches.read() {
        for (goal, reached) in goals.iter_mut() {
            let ours = (touch.a == player && touch.b == goal) || (touch.b == player && touch.a == goal);
            if ours && !reached.reached {
                reached.reached = true;
                game.won = true;
                game.won_at = time.tick;
                sounds.send(sound("assets/sounds/win.wav", Vec3::ZERO));
            }
        }
    }
    if game.won {
        for (banner, node) in banners.iter_mut() {
            if !banner.shown {
                banner.shown = true;
                node.visible = true;
                let seconds = game.won_at as f32 * time.delta;
                for words in texts.iter_mut() {
                    words.text = text(&format!(
                        "Well done!
Crystals {}/{} · {:.1} s",
                        game.crystals, game.total, seconds
                    ));
                }
            }
        }
    }
}

/// Counts the crystals of the level on the first tick, and keeps the label
/// up to date.
#[system(after = "touch")]
fn show(
    crystals: Query<&Crystal>,
    mut game: ResMut<Game>,
    mut labels: Query<(&mut CrystalLabel, &mut UiText)>,
) {
    if game.total == 0 {
        game.total = crystals.iter().map(|c| c.value).sum::<u32>() + game.crystals;
    }
    for (label, words) in labels.iter_mut() {
        if label.shown != game.crystals || text_of(words.text).is_empty() || label.shown == 0 {
            words.text = text(&format!("Crystals: {}/{}", game.crystals, game.total));
            label.shown = game.crystals;
        }
    }
}

/// Moves the camera smoothly behind the player and turns it to look at
/// the player.
#[system(after = "run")]
fn follow(
    time: Res<Time>,
    players: Query<&Transform, With<Player>>,
    mut cameras: Query<(&FollowCamera, &mut Transform)>,
) {
    let Some(target) = players.iter().next().map(|t| t.translation) else {
        return;
    };
    for (follow, transform) in cameras.iter_mut() {
        let wanted = target + follow.offset;
        // Frame-rate independent smoothing: the same at any tick length.
        let blend = 1.0 - (-follow.stiffness * time.delta).exp();
        transform.translation += (wanted - transform.translation) * blend;
        let look = target + Vec3::new(0.0, follow.look_height, 0.0) - transform.translation;
        if look.length() > 1e-3 {
            let dir = look.normalize();
            let yaw = (-dir.x).atan2(-dir.z);
            let pitch = dir.y.clamp(-1.0, 1.0).asin();
            transform.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        }
    }
}

wish_sdk::export! {
    components: [Player, FollowCamera, Crystal, Checkpoint, CrystalLabel, MovingPlatform, Goal, WinBanner, WinText],
    resources: [Game],
    systems: [platforms, run, fall, touch, show, finish, follow],
}
