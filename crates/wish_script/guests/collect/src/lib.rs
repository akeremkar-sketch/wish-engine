//! A tiny game: things move, the player picks up coins, the score counts
//! them. Two systems misbehave on purpose when their marker exists.

use wish_sdk::prelude::*;

/// Moves an entity every tick.
#[component]
pub struct Velocity {
    /// How fast, and where to.
    #[schema(unit = "m/s")]
    pub value: Vec3,
}

/// A coin the player can pick up.
#[component]
pub struct Coin {
    /// Points it is worth.
    #[schema(range = 1..=100, default = 1)]
    pub value: u32,
}

/// Marks the player.
#[component]
pub struct Player {}

/// Makes the `explode` system panic.
#[component]
pub struct Bomb {}

/// Makes the `spin` system loop forever.
#[component]
pub struct Spinner {}

/// What the player has collected.
#[resource]
pub struct Score {
    /// Points so far.
    pub points: u32,
    /// Coins picked up.
    pub coins: u32,
}

/// A coin was picked up.
#[event]
pub struct Collected {
    /// Who picked it up.
    pub by: Entity,
    /// What it was worth.
    pub value: u32,
}

#[system]
fn move_things(mut things: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
    for (transform, velocity) in things.iter_mut() {
        transform.translation += velocity.value * time.delta;
    }
}

#[system(after = "move_things")]
fn collect(
    players: Query<(Entity, &Transform), With<Player>>,
    coins: Query<(Entity, &Transform, &Coin)>,
    mut commands: Commands,
    mut collected: EventWriter<Collected>,
) {
    for (player, at) in players.iter() {
        for (coin, coin_at, worth) in coins.iter() {
            if at.translation.distance(coin_at.translation) < 0.75 {
                commands.despawn(coin);
                collected.send(Collected {
                    by: player,
                    value: worth.value,
                });
            }
        }
    }
}

#[system(after = "collect")]
fn score(collected: EventReader<Collected>, mut score: ResMut<Score>) {
    for event in collected.read() {
        score.points += event.value;
        score.coins += 1;
    }
}

#[system]
fn spawn_bonus(time: Res<Time>, mut commands: Commands) {
    if time.tick == 0 {
        commands
            .spawn()
            .insert(Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)))
            .insert(Coin { value: 10 });
    }
}

#[system]
fn explode(bombs: Query<&Bomb>) {
    if !bombs.is_empty() {
        panic!("the bomb went off");
    }
}

#[system]
fn spin(spinners: Query<&Spinner>) {
    if !spinners.is_empty() {
        let mut turns = 0u64;
        loop {
            turns = turns.wrapping_add(1);
            std::hint::black_box(turns);
        }
    }
}

wish_sdk::export! {
    components: [Velocity, Coin, Player, Bomb, Spinner],
    resources: [Score],
    events: [Collected],
    systems: [move_things, collect, score, spawn_bonus, explode, spin],
}
