//! The scene view's 2D mode (release 1.1, package 8, design note
//! `docs/design/1.1-editor-2d.md` §5): a top-down orthographic eye over the
//! world's XY plane that pans and zooms (no orbit), the grid it shows, and
//! what a drag or a rectangle means in the plane.
//!
//! Plain arithmetic with no window or scene in it, so it is tested on its
//! own. The eye looks along -Z from far above the plane; +X is right and +Y
//! up on the screen, as in a 2D game.

use egui::{Pos2, Rect, pos2};
use glam::{Mat4, Vec2, Vec3};
use wish_render::CameraView;

/// The eye's height above the plane, meters: it sees from `EYE_Z` in front
/// of the plane to as far behind it.
const EYE_Z: f32 = 1000.0;
/// The least and most the view shows from top to bottom, meters.
const HEIGHT_RANGE: (f32, f32) = (0.05, 100_000.0);
/// The fewest pixels between two grid lines: a finer grid is thinned to
/// every 2nd, 4th... line.
pub const GRID_PIXELS: f32 = 8.0;

/// The 2D mode's eye: where the middle of the view is in the plane, and how
/// many meters the view shows from top to bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatEye {
    /// The world point in the middle of the view (x, y).
    pub centre: Vec2,
    /// Meters from the top of the view to its bottom.
    pub height: f32,
}

impl FlatEye {
    /// An eye over what `camera` (the scene's camera) shows: its place in
    /// the plane, and its height (a pixel camera's game picture, an
    /// orthographic camera's height, or 10 m).
    pub fn from_camera(camera: &CameraView) -> Self {
        let height = match (&camera.pixel, camera.ortho_height) {
            (Some(pixel), _) => pixel.size()[1] as f32 / pixel.unit(),
            (None, Some(height)) => height,
            (None, None) => 10.0,
        };
        let centre = Vec2::new(camera.position.x, camera.position.y);
        Self {
            centre: if centre.is_finite() {
                centre
            } else {
                Vec2::ZERO
            },
            height: height.clamp(HEIGHT_RANGE.0, HEIGHT_RANGE.1),
        }
    }

    /// `camera` replaced by this eye: orthographic, top down, no pixel fit.
    pub fn camera(&self, camera: &CameraView) -> CameraView {
        let position = Vec3::new(self.centre.x, self.centre.y, EYE_Z);
        CameraView {
            view: Mat4::from_translation(-position),
            position,
            near: 0.01,
            far: EYE_Z * 2.0,
            ortho_height: Some(self.height),
            pixel: None,
            ..*camera
        }
    }

    /// Meters per pixel in a view `rect`.
    pub fn per_pixel(&self, rect: Rect) -> f32 {
        self.height / rect.height().max(1.0)
    }

    /// The plane's point under the window pixel `at` of a view `rect`.
    pub fn world_at(&self, rect: Rect, at: Pos2) -> Vec2 {
        let m = self.per_pixel(rect);
        let from = at - rect.center();
        self.centre + Vec2::new(from.x * m, -from.y * m)
    }

    /// The window pixel of the plane's point `point` in a view `rect`.
    pub fn screen_at(&self, rect: Rect, point: Vec2) -> Pos2 {
        let m = self.per_pixel(rect);
        let from = (point - self.centre) / m;
        rect.center() + egui::vec2(from.x, -from.y)
    }

    /// Slides the view so the world follows the pointer by (`dx`, `dy`)
    /// pixels (a grab).
    pub fn pan(&mut self, rect: Rect, dx: f32, dy: f32) {
        let m = self.per_pixel(rect);
        self.centre += Vec2::new(-dx * m, dy * m);
    }

    /// Zooms by `share` (0.125: an eighth nearer; negative: farther) toward
    /// the window pixel `at`, which stays over the same world point.
    pub fn zoom_at(&mut self, rect: Rect, at: Pos2, share: f32) {
        let before = self.world_at(rect, at);
        let next = self.height * (1.0 - share.clamp(-1.0, 0.9));
        self.height = next.clamp(HEIGHT_RANGE.0, HEIGHT_RANGE.1);
        let after = self.world_at(rect, at);
        self.centre += before - after;
    }

    /// Puts a box of the plane (`low` to `high` corners) in the middle,
    /// filling about two thirds of the view.
    pub fn frame(&mut self, rect: Rect, low: Vec2, high: Vec2) {
        let size = (high - low).abs().max(Vec2::splat(0.25));
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        self.centre = (low + high) * 0.5;
        self.height = (size.y.max(size.x / aspect) * 1.5).clamp(HEIGHT_RANGE.0, HEIGHT_RANGE.1);
    }

    /// The grid's lines in a view `rect` for cells of `step` meters (x, y):
    /// the window x of each vertical line and the window y of each
    /// horizontal one, thinned to every 2nd, 4th... line so that lines are
    /// at least [`GRID_PIXELS`] apart; and the step they are drawn at.
    pub fn grid(&self, rect: Rect, step: [f32; 2]) -> (Vec<f32>, Vec<f32>, [f32; 2]) {
        let m = self.per_pixel(rect);
        let thin = |step: f32| {
            let mut step = if step.is_finite() && step > 0.0 {
                step
            } else {
                1.0
            };
            for _ in 0..40 {
                if step / m >= GRID_PIXELS {
                    break;
                }
                step *= 2.0;
            }
            step
        };
        let (sx, sy) = (thin(step[0]), thin(step[1]));
        let low = self.world_at(rect, pos2(rect.min.x, rect.max.y));
        let high = self.world_at(rect, pos2(rect.max.x, rect.min.y));
        let lines = |from: f32, to: f32, step: f32| {
            let first = (from / step).ceil() as i64;
            let last = (to / step).floor() as i64;
            (first..=last).take(4096).map(move |i| i as f32 * step)
        };
        let xs = lines(low.x, high.x, sx)
            .map(|x| self.screen_at(rect, Vec2::new(x, 0.0)).x)
            .collect();
        let ys = lines(low.y, high.y, sy)
            .map(|y| self.screen_at(rect, Vec2::new(0.0, y)).y)
            .collect();
        (xs, ys, [sx, sy])
    }
}

/// `value` on the nearest multiple of `step` (unchanged for a step that is
/// not a positive number).
pub fn snap(value: f32, step: f32) -> f32 {
    if step.is_finite() && step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// The rectangle between two window points.
pub fn marquee(a: Pos2, b: Pos2) -> Rect {
    Rect::from_two_pos(a, b)
}

/// Whether a screen box (left, top, right, bottom, in a view's pixels) meets
/// the rectangle `rect` (in the same pixels).
pub fn touches(found: [i32; 4], rect: Rect) -> bool {
    let [left, top, right, bottom] = found.map(|v| v as f32);
    left <= rect.max.x && right >= rect.min.x && top <= rect.max.y && bottom >= rect.min.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use wish_core::Entity;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), egui::vec2(400.0, 200.0))
    }

    fn eye() -> FlatEye {
        FlatEye {
            centre: Vec2::new(3.0, -1.0),
            height: 10.0,
        }
    }

    #[test]
    fn screen_and_world_are_each_other_s_inverse() {
        let eye = eye();
        let rect = rect();
        assert_eq!(eye.world_at(rect, rect.center()), Vec2::new(3.0, -1.0));
        // 20 pixels a meter: the top right corner is 10 m right, 5 m up.
        assert_eq!(eye.world_at(rect, rect.right_top()), Vec2::new(13.0, 4.0));
        let back = eye.screen_at(rect, Vec2::new(13.0, 4.0));
        assert!((back - rect.right_top()).length() < 1e-3);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_and_panning_follows_it() {
        let rect = rect();
        let mut eye = eye();
        let at = pos2(420.0, 90.0);
        let before = eye.world_at(rect, at);
        eye.zoom_at(rect, at, 0.5);
        assert!((eye.height - 5.0).abs() < 1e-4);
        assert!((eye.world_at(rect, at) - before).length() < 1e-4);
        eye.zoom_at(rect, at, -1.0);
        assert!((eye.height - 10.0).abs() < 1e-4);
        assert!((eye.world_at(rect, at) - before).length() < 1e-4);
        // A grab: the world point under the pointer moves with it.
        let grabbed = eye.world_at(rect, at);
        eye.pan(rect, 30.0, -12.0);
        let now = eye.world_at(rect, at + egui::vec2(30.0, -12.0));
        assert!((now - grabbed).length() < 1e-4);
    }

    #[test]
    fn the_eye_looks_down_on_the_plane_with_x_right_and_y_up() {
        let base = CameraView {
            entity: Entity::default(),
            view: Mat4::IDENTITY,
            position: Vec3::new(5.0, 6.0, 10.0),
            fov_y: 1.0,
            near: 0.1,
            far: 100.0,
            ortho_height: Some(8.0),
            pixel: None,
        };
        let eye = FlatEye::from_camera(&base);
        assert_eq!(eye.centre, Vec2::new(5.0, 6.0));
        assert_eq!(eye.height, 8.0);
        let camera = eye.camera(&base);
        let seen = camera.view.transform_point3(Vec3::new(6.0, 7.0, 0.0));
        assert_eq!((seen.x, seen.y), (1.0, 1.0));
        assert!(seen.z < -camera.near && seen.z > -camera.far);
    }

    #[test]
    fn the_grid_is_thinned_and_framing_fits_a_box() {
        let rect = rect();
        let mut eye = eye();
        // 20 pixels a meter: cells of a sixteenth of a meter are thinned to
        // half a meter (10 pixels).
        let (xs, ys, step) = eye.grid(rect, [1.0 / 16.0, 1.0 / 16.0]);
        assert_eq!(step, [0.5, 0.5]);
        assert_eq!(xs.len(), 41);
        assert_eq!(ys.len(), 21);
        eye.frame(rect, Vec2::new(0.0, 0.0), Vec2::new(4.0, 2.0));
        assert_eq!(eye.centre, Vec2::new(2.0, 1.0));
        assert!((eye.height - 3.0).abs() < 1e-4, "{}", eye.height);
        assert_eq!(snap(1.26, 0.25), 1.25);
        assert!(touches(
            [10, 10, 20, 20],
            Rect::from_min_max(pos2(15.0, 0.0), pos2(30.0, 12.0))
        ));
        assert!(!touches(
            [10, 10, 20, 20],
            Rect::from_min_max(pos2(21.0, 0.0), pos2(30.0, 12.0))
        ));
    }
}
