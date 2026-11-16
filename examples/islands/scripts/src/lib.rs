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

/// Gravity, added by the game itself (a character is moved, not pushed).
const GRAVITY: f32 = 18.0;

/// Runs and jumps: says how fast the character wants to go. `move` is
/// [x, y], x to the right (+X) and y forward (-Z).
#[system]
fn run(
    actions: Actions,
    time: Res<Time>,
    mut players: Query<(&Player, &mut CharacterMotion, &CharacterState)>,
) {
    let go = actions.axis2("move");
    for (player, motion, state) in players.iter_mut() {
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
                player.jump_speed
            } else {
                -1.0
            };
        }
        motion.velocity = Vec3::new(horizontal.x, vy, horizontal.z);
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
    components: [Player, FollowCamera],
    systems: [run, follow],
}
