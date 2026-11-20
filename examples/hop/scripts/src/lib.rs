//! Hop: a small platformer for Wish Engine's game systems (M6). Walk with
//! WASD or the arrow keys, jump with Space, pick up every coin.
//!
//! The player is a physics character (`CharacterController`): game code
//! only says how fast it wants to go, physics moves it, stops it at walls
//! and says whether it stands on something. Coins are sensors: touching one
//! is a `CollisionStarted` event. A picked-up coin leaves a burst of sparks
//! (a `ParticleEmitter` started at that moment), and the interface shows the
//! score and, at the end, that the game is won.

use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// How fast the player walks.
    #[schema(unit = "m/s", range = 0.0..=20.0, default = 4.0)]
    pub speed: f32,
    /// How fast the player leaves the ground when jumping.
    #[schema(unit = "m/s", range = 0.0..=30.0, default = 6.0)]
    pub jump_speed: f32,
    /// Where the player comes back to after falling off the world.
    #[schema(unit = "m")]
    pub start: Vec3,
}

/// A coin waiting to be picked up.
#[component]
pub struct Coin {
    /// Points it is worth.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
}

/// The text that shows the score.
#[component]
pub struct ScoreLabel {
    /// The points it shows now.
    pub shown: u32,
}

/// The box that appears when the game is won.
#[component]
pub struct WinBanner {
    /// Whether it has been shown.
    pub shown: bool,
}

/// The points collected so far.
#[resource]
pub struct Score {
    /// Points so far.
    pub points: u32,
    /// Coins picked up so far.
    pub coins: u32,
}

/// Where the game stands.
#[resource]
pub struct GameState {
    /// Whether every coin has been picked up.
    pub won: bool,
    /// The tick the game was won at (0 until then).
    pub won_at: u64,
}

/// Gravity, added by the game itself (a character is moved, not pushed).
const GRAVITY: f32 = 9.81;
/// Below this height the player has fallen off the world.
const FLOOR_OF_THE_WORLD: f32 = -10.0;

/// Walks and jumps: says how fast the character wants to go.
#[system]
fn run(
    actions: Actions,
    time: Res<Time>,
    mut players: Query<(&Player, &mut CharacterMotion, &CharacterState)>,
) {
    let go = actions.axis2("move");
    for (player, motion, state) in players.iter_mut() {
        let mut vy = motion.velocity.y - GRAVITY * time.delta;
        if state.grounded {
            vy = if actions.pressed("jump") {
                player.jump_speed
            } else {
                -1.0
            };
        }
        motion.velocity = Vec3::new(go.x * player.speed, vy, -go.y * player.speed);
    }
}

/// A player that fell off the world comes back to the start.
#[system]
fn respawn(mut players: Query<(&Player, &mut Transform, &mut CharacterMotion)>) {
    for (player, transform, motion) in players.iter_mut() {
        if transform.translation.y < FLOOR_OF_THE_WORLD {
            transform.translation = player.start;
            motion.velocity = Vec3::ZERO;
        }
    }
}

/// Picks up the coins the player touches: points, and a burst of sparks
/// where the coin was.
#[system]
fn pick(
    touches: EventReader<CollisionStarted>,
    players: Query<Entity, With<Player>>,
    coins: Query<(Entity, &Transform, &Coin)>,
    time: Res<Time>,
    mut score: ResMut<Score>,
    mut commands: Commands,
) {
    for touch in touches.read() {
        let player_touched = players.iter().any(|p| p == touch.a || p == touch.b);
        if !player_touched {
            continue;
        }
        for (coin, at, worth) in coins.iter() {
            if coin != touch.a && coin != touch.b {
                continue;
            }
            score.points += worth.value;
            score.coins += 1;
            commands.despawn(coin);
            commands
                .spawn()
                .insert(Transform::from_translation(at.translation))
                .insert(ParticleEmitter {
                    start_time: time.elapsed as f32,
                    rate: 0.0,
                    burst: 24,
                    lifetime: 0.6,
                    speed: 2.5,
                    spread_deg: 70.0,
                    gravity: Vec3::new(0.0, -6.0, 0.0),
                    size_start: 0.15,
                    size_end: 0.0,
                    color_start: Color::rgba(1.0, 0.8, 0.2, 1.0),
                    color_end: Color::rgba(1.0, 0.5, 0.1, 0.0),
                    seed: score.coins,
                    ..ParticleEmitter::default()
                });
        }
    }
}

/// The game is won when no coin is left.
#[system(after = "pick")]
fn check_win(coins: Query<&Coin>, mut state: ResMut<GameState>, time: Res<Time>) {
    if coins.is_empty() && !state.won {
        state.won = true;
        state.won_at = time.tick;
        wish_sdk::info!("Every coin collected at tick {}!", time.tick);
    }
}

/// Keeps the interface up to date: the score, and the banner once won.
#[system(after = "check_win")]
fn show(
    score: Res<Score>,
    state: Res<GameState>,
    mut labels: Query<(&mut ScoreLabel, &mut UiText)>,
    mut banners: Query<(&mut WinBanner, &mut UiNode)>,
) {
    for (label, words) in labels.iter_mut() {
        if label.shown != score.points || text_of(words.text).is_empty() {
            words.text = text(&format!("Score: {}", score.points));
            label.shown = score.points;
        }
    }
    for (banner, node) in banners.iter_mut() {
        if state.won && !banner.shown {
            node.visible = true;
            banner.shown = true;
        }
    }
}

/// Coins turn slowly, so they catch the eye.
#[system]
fn spin_coins(mut coins: Query<&mut Transform, With<Coin>>, time: Res<Time>) {
    for transform in coins.iter_mut() {
        transform.rotation = Quat::from_rotation_y(2.0 * time.delta) * transform.rotation;
    }
}

wish_sdk::export! {
    components: [Player, Coin, ScoreLabel, WinBanner],
    resources: [Score, GameState],
    systems: [run, respawn, pick, check_win, show, spin_coins],
}
