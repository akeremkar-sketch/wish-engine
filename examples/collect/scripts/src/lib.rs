//! Collect: move the player with WASD or the arrow keys, pick up every
//! coin, win. A small but complete game for Wish Engine: components and
//! resources declared with the engine's macros, input as actions, commands,
//! an event, and ordering between systems.

use wish_sdk::prelude::*;

/// The player.
#[component]
pub struct Player {
    /// How fast the player walks.
    #[schema(unit = "m/s", range = 0.0..=20.0, default = 4.0)]
    pub speed: f32,
}

/// A coin waiting to be picked up.
#[component]
pub struct Coin {
    /// Points it is worth.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
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

/// A coin was picked up.
#[event]
pub struct Collected {
    /// The coin's points.
    pub value: u32,
}

/// How close the player must come to pick a coin up.
const REACH: f32 = 0.8;

/// Moves the player on the ground: `move` is [x, y] with y forward (-Z).
#[system]
fn move_player(actions: Actions, mut players: Query<(&mut Transform, &Player)>, time: Res<Time>) {
    let go = actions.axis2("move");
    for (transform, player) in players.iter_mut() {
        transform.translation += Vec3::new(go.x, 0.0, -go.y) * player.speed * time.delta;
    }
}

/// Picks up the coins the player touches.
#[system(after = "move_player")]
fn collect(
    players: Query<&Transform, With<Player>>,
    coins: Query<(Entity, &Transform, &Coin)>,
    mut commands: Commands,
    mut collected: EventWriter<Collected>,
) {
    for at in players.iter() {
        for (coin, coin_at, worth) in coins.iter() {
            let apart = at.translation - coin_at.translation;
            if Vec3::new(apart.x, 0.0, apart.z).length() < REACH {
                commands.despawn(coin);
                collected.send(Collected { value: worth.value });
            }
        }
    }
}

/// Counts what was picked up.
#[system(after = "collect")]
fn score(collected: EventReader<Collected>, mut score: ResMut<Score>) {
    for event in collected.read() {
        score.points += event.value;
        score.coins += 1;
    }
}

/// The game is won when no coin is left.
#[system(after = "score")]
fn check_win(coins: Query<&Coin>, mut state: ResMut<GameState>, time: Res<Time>) {
    if coins.is_empty() && !state.won {
        state.won = true;
        state.won_at = time.tick;
        wish_sdk::info!("Every coin collected at tick {}!", time.tick);
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
    components: [Player, Coin],
    resources: [Score, GameState],
    events: [Collected],
    systems: [move_player, collect, score, check_win, spin_coins],
}
