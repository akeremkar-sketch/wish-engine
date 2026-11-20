//! Bricks: a 2D brick breaker, a small example for Wish Engine (M7). A
//! ball bounces between walls, a paddle and bricks; every brick is worth
//! points, three lives, win by clearing the bricks. With `auto` on the
//! paddle follows the ball by itself (a demo, and how the tests play); the
//! first press of A/D or the arrow keys hands it to the player.

use wish_sdk::prelude::*;

/// The paddle.
#[component]
pub struct Paddle {
    /// How fast it moves.
    #[schema(unit = "m/s", default = 9.0)]
    pub speed: f32,
    /// Whether it follows the ball by itself (until a key moves it).
    pub auto: bool,
}

/// The ball.
#[component]
pub struct Ball {
    /// Its constant speed.
    #[schema(unit = "m/s", default = 10.0)]
    pub speed: f32,
}

/// A brick.
#[component]
pub struct Brick {
    /// Points it is worth.
    #[schema(default = 10)]
    pub value: u32,
}

/// The text that shows the score and the lives.
#[component]
pub struct Hud {
    /// The score and lives it shows now, packed: score * 10 + lives.
    pub shown: u32,
}

/// The text on the end panel.
#[component]
pub struct EndText {
    /// Unused.
    pub unused: u32,
}

/// The panel at the end, with its message.
#[component]
pub struct EndPanel {
    /// Whether it is shown.
    pub shown: bool,
}

/// How the game stands.
#[resource]
pub struct Game {
    /// Points.
    pub score: u32,
    /// Lives left.
    pub lives: u32,
    /// Bricks left (counted on the first tick).
    pub bricks: u32,
    /// Whether the bricks are all gone.
    pub won: bool,
    /// Whether the lives are all gone.
    pub lost: bool,
}

const START_LIVES: u32 = 3;
/// Below this height the ball is lost.
const LOST_LINE: f32 = -6.6;
/// Where the paddle can go (its centre).
const PADDLE_LIMIT: f32 = 7.0;

fn sound(path: &str) -> PlaySound {
    PlaySound {
        clip: asset(path),
        bus: AudioBus::Sfx,
        volume: 1.0,
        pitch: 1.0,
        spatial: false,
        position: Vec3::ZERO,
    }
}

/// Sets up the lives on the first tick.
#[system]
fn start(mut game: ResMut<Game>, bricks: Query<&Brick>) {
    if game.lives == 0 && !game.lost {
        game.lives = START_LIVES;
        game.bricks = bricks.iter().count() as u32;
    }
}

/// The paddle follows the keys, or the ball.
#[system]
fn move_paddle(
    actions: Actions,
    time: Res<Time>,
    game: Res<Game>,
    balls: Query<&Transform, With<Ball>>,
    mut paddles: Query<(&mut Paddle, &mut Transform), Without<Ball>>,
) {
    if game.won || game.lost {
        return;
    }
    let keys = actions.axis2("move").x;
    for (paddle, at) in paddles.iter_mut() {
        // A key takes the paddle over from the demo.
        if paddle.auto && keys.abs() > 0.01 {
            paddle.auto = false;
        }
        let step = paddle.speed * time.delta;
        let wanted = if paddle.auto {
            // Aims at a spot that wanders along the paddle, so the ball
            // leaves at different angles.
            let wander = (time.tick as f32 * 0.037).sin() * 0.8;
            balls
                .iter()
                .next()
                .map(|b| b.translation.x - wander - at.translation.x)
        } else {
            Some(keys * paddle.speed * time.delta)
        };
        if let Some(wanted) = wanted {
            let x = at.translation.x + wanted.clamp(-step, step);
            at.translation.x = x.clamp(-PADDLE_LIMIT, PADDLE_LIMIT);
        }
    }
}

/// Keeps the ball at its speed and never too flat, and brings it back
/// after it fell.
#[system]
fn move_ball(
    time: Res<Time>,
    mut game: ResMut<Game>,
    mut balls: Query<(&Ball, &mut Transform, &mut BodyVelocity)>,
    mut sounds: EventWriter<PlaySound>,
) {
    for (ball, at, velocity) in balls.iter_mut() {
        if game.won || game.lost {
            velocity.linear = Vec3::ZERO;
            continue;
        }
        if at.translation.y < LOST_LINE {
            game.lives = game.lives.saturating_sub(1);
            sounds.send(sound("assets/sounds/lose.wav"));
            if game.lives == 0 {
                game.lost = true;
            }
            at.translation = Vec3::new(0.0, -4.6, 0.0);
            // A different way out each time, from the tick it happened.
            let side = if time.tick % 2 == 0 { 1.0 } else { -1.0 };
            velocity.linear = Vec3::new(3.0 * side, 5.0, 0.0);
        }
        let flat = Vec3::new(velocity.linear.x, velocity.linear.y, 0.0);
        let mut direction = if flat.length() > 1e-3 { flat.normalize() } else { Vec3::Y };
        // Never nearly flat: such a ball takes ages to come down.
        if direction.y.abs() < 0.3 {
            direction.y = 0.3 * if direction.y < 0.0 { -1.0 } else { 1.0 };
            direction = direction.normalize();
        }
        velocity.linear = direction * ball.speed;
    }
}

/// Bricks the ball touched are gone.
#[system(after = "move_ball")]
fn hit(
    touches: EventReader<CollisionStarted>,
    mut balls: Query<(Entity, &Transform, &mut BodyVelocity), With<Ball>>,
    paddles: Query<(Entity, &Transform), (With<Paddle>, Without<Ball>)>,
    bricks: Query<(Entity, &Brick)>,
    mut game: ResMut<Game>,
    mut sounds: EventWriter<PlaySound>,
    mut commands: Commands,
) {
    let Some((ball, ball_at, velocity)) = balls.iter_mut().next() else {
        return;
    };
    for touch in touches.read() {
        let other = if touch.a == ball {
            touch.b
        } else if touch.b == ball {
            touch.a
        } else {
            continue;
        };
        if let Some((brick, worth)) = bricks.iter().find(|(e, _)| *e == other) {
            game.score += worth.value;
            game.bricks = game.bricks.saturating_sub(1);
            commands.despawn(brick);
            sounds.send(sound("assets/sounds/brick.wav"));
            if game.bricks == 0 {
                game.won = true;
            }
        } else if let Some((_, paddle_at)) = paddles.iter().find(|(e, _)| *e == other) {
            sounds.send(sound("assets/sounds/paddle.wav"));
            // Where the ball hit the paddle decides the way it leaves:
            // the edges send it sideways, the middle straight up.
            let offset = ((ball_at.translation.x - paddle_at.translation.x) / 1.1).clamp(-1.0, 1.0);
            velocity.linear = Vec3::new(offset * 7.0, 6.0, 0.0);
        }
    }
}

/// The score and lives on screen, and the message at the end.
#[system(after = "hit")]
fn show(
    game: Res<Game>,
    mut huds: Query<(&mut Hud, &mut UiText)>,
    mut panels: Query<(&mut EndPanel, &mut UiNode)>,
    mut messages: Query<&mut UiText, (With<EndText>, Without<Hud>)>,
) {
    let packed = game.score * 10 + game.lives;
    for (hud, words) in huds.iter_mut() {
        if hud.shown != packed || text_of(words.text).is_empty() {
            words.text = text(&format!("Score: {}   Lives: {}", game.score, game.lives));
            hud.shown = packed;
        }
    }
    for (panel, node) in panels.iter_mut() {
        if (game.won || game.lost) && !panel.shown {
            panel.shown = true;
            node.visible = true;
            for words in messages.iter_mut() {
                words.text = text(if game.won { "You win!" } else { "Game over" });
            }
        }
    }
}

wish_sdk::export! {
    components: [Paddle, Ball, Brick, Hud, EndText, EndPanel],
    resources: [Game],
    systems: [start, move_paddle, move_ball, hit, show],
}
