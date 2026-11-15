//! The fly camera: where the viewer's eye is and how input moves it.
//!
//! Pure arithmetic with no window in it, so it is tested without one.

use wish_core::math::{Mat4, Vec3};
use wish_render::CameraView;

/// Which movement keys are held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Movement {
    /// Along the view direction.
    pub forward: bool,
    /// Against the view direction.
    pub back: bool,
    /// To the left.
    pub left: bool,
    /// To the right.
    pub right: bool,
    /// Straight up.
    pub up: bool,
    /// Straight down.
    pub down: bool,
    /// Move faster.
    pub fast: bool,
}

impl Movement {
    /// Whether any direction key is held.
    pub fn any(&self) -> bool {
        self.forward || self.back || self.left || self.right || self.up || self.down
    }
}

/// A free-flying eye: a position and two angles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyCamera {
    /// World position, meters.
    pub position: Vec3,
    /// Rotation around +Y in radians; 0 looks along -Z, positive turns left.
    pub yaw: f32,
    /// Rotation up from the horizon in radians, kept below 89 degrees.
    pub pitch: f32,
}

/// Meters per second at normal speed.
pub const SPEED: f32 = 4.0;
/// Speed factor while the fast key is held.
pub const FAST: f32 = 4.0;
/// Radians of turning per pixel of mouse movement.
pub const LOOK_SENSITIVITY: f32 = 0.0025;
const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

/// World space to the space of an eye at `position` looking along `forward`
/// (unit length, not straight up or down), +Y up.
fn look_matrix(position: Vec3, forward: Vec3) -> Mat4 {
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        (-forward).extend(0.0),
        position.extend(1.0),
    )
    .inverse()
}

impl FlyCamera {
    /// An eye placed where `camera` is, looking the way it looks.
    pub fn from_view(camera: &CameraView) -> Self {
        // A view matrix's third row is the camera's +Z axis in world space;
        // the camera looks along the opposite direction.
        let back = camera.view.inverse().z_axis.truncate().normalize_or_zero();
        let forward = if back == Vec3::ZERO { -Vec3::Z } else { -back };
        Self {
            position: camera.position,
            yaw: (-forward.x).atan2(-forward.z),
            pitch: forward
                .y
                .clamp(-1.0, 1.0)
                .asin()
                .clamp(-MAX_PITCH, MAX_PITCH),
        }
    }

    /// The direction the eye looks along (unit length).
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// The direction to the eye's right (horizontal, unit length).
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Turns by a mouse movement in pixels: right turns right, up looks up.
    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * LOOK_SENSITIVITY;
        self.pitch = (self.pitch - dy * LOOK_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves for `seconds` with the keys in `keys`.
    pub fn step(&mut self, keys: Movement, seconds: f32) {
        let axis = |plus: bool, minus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
        let mut direction = Vec3::ZERO;
        direction += self.forward() * axis(keys.forward, keys.back);
        direction += self.right() * axis(keys.right, keys.left);
        direction += Vec3::Y * axis(keys.up, keys.down);
        let speed = SPEED * if keys.fast { FAST } else { 1.0 };
        self.position += direction.normalize_or_zero() * speed * seconds;
    }

    /// `camera` seen from this eye (same lens, new placement).
    pub fn apply(&self, camera: &CameraView) -> CameraView {
        CameraView {
            view: look_matrix(self.position, self.forward()),
            position: self.position,
            ..*camera
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wish_core::Entity;

    fn camera_at(position: Vec3, forward: Vec3) -> CameraView {
        CameraView {
            entity: Entity::default(),
            view: look_matrix(position, forward),
            position,
            fov_y: 1.0,
            near: 0.1,
            far: 100.0,
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    fn eye(yaw: f32, pitch: f32) -> FlyCamera {
        FlyCamera {
            position: Vec3::ZERO,
            yaw,
            pitch,
        }
    }

    fn keys(set: impl FnOnce(&mut Movement)) -> Movement {
        let mut keys = Movement::default();
        set(&mut keys);
        keys
    }

    #[test]
    fn the_eye_starts_where_the_camera_is_and_looks_the_same_way() {
        for forward in [
            -Vec3::Z,
            Vec3::new(1.0, 0.3, -1.0).normalize(),
            Vec3::new(-0.5, -0.4, 0.8).normalize(),
        ] {
            let camera = camera_at(Vec3::new(1.0, 2.0, 3.0), forward);
            let start = FlyCamera::from_view(&camera);
            assert!(
                close(start.forward(), forward),
                "{:?} vs {forward:?}",
                start.forward()
            );
            let again = start.apply(&camera);
            assert!(close(again.position, camera.position));
            let (a, b) = (again.view.to_cols_array(), camera.view.to_cols_array());
            assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4));
        }
    }

    #[test]
    fn movement_follows_the_view_and_diagonals_are_not_faster() {
        let mut flying = eye(0.0, 0.0);
        flying.step(keys(|k| k.forward = true), 1.0);
        assert!(close(flying.position, Vec3::new(0.0, 0.0, -SPEED)));

        let mut flying = eye(0.0, 0.0);
        flying.step(
            keys(|k| {
                k.forward = true;
                k.right = true;
            }),
            1.0,
        );
        assert!((flying.position.length() - SPEED).abs() < 1e-4);
        assert!(flying.position.x > 0.0 && flying.position.z < 0.0);

        // Turned a quarter to the left, forward is -X.
        let mut flying = eye(std::f32::consts::FRAC_PI_2, 0.0);
        flying.step(
            keys(|k| {
                k.forward = true;
                k.fast = true;
            }),
            0.5,
        );
        assert!(close(
            flying.position,
            Vec3::new(-SPEED * FAST * 0.5, 0.0, 0.0)
        ));

        // Up is world up whatever the pitch.
        let mut flying = eye(0.0, 1.0);
        flying.step(keys(|k| k.up = true), 1.0);
        assert!(close(flying.position, Vec3::new(0.0, SPEED, 0.0)));

        // Opposite keys cancel.
        let mut flying = eye(0.3, 0.2);
        flying.step(
            keys(|k| {
                k.forward = true;
                k.back = true;
            }),
            1.0,
        );
        assert_eq!(flying.position, Vec3::ZERO);
    }

    #[test]
    fn looking_turns_the_right_way_and_stops_short_of_straight_up() {
        let mut flying = eye(0.0, 0.0);
        flying.look(100.0, 0.0);
        assert!(
            flying.forward().x > 0.0,
            "the mouse moved right: look right"
        );
        flying.look(0.0, -1.0e6);
        assert!(flying.pitch < std::f32::consts::FRAC_PI_2);
        assert!(flying.forward().is_finite());
        flying.look(0.0, 1.0e6);
        assert!(flying.pitch > -std::f32::consts::FRAC_PI_2);
        assert!(flying.forward().is_finite());
    }
}
