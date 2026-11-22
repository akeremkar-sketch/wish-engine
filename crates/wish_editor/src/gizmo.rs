//! Move, rotate and scale handles on the selected entity in the view (M8a
//! WP5): where they are on the screen, which one the pointer is over, and
//! what a drag of one does to the entity's `Transform`.
//!
//! The mathematics is plain and has no window or scene in it, so it is tested
//! on its own. A handle's drag is read from the pointer's *ray* through the
//! view, so it follows the pointer exactly at any distance and angle:
//!
//! - **Move** along an axis: the point of the axis line nearest the ray.
//! - **Rotate** about an axis: the angle of the ray's hit on the ring's plane.
//! - **Scale** along an axis: the ratio of the nearest-point distances (the
//!   centre handle: of the pointer's distances from the centre).
//!
//! The handles keep a fixed size on the screen, whatever the distance.

use egui::{Pos2, Rect, pos2};
use glam::{Mat4, Quat, Vec3};
use wish_render::CameraView;

use crate::raster::Canvas;

/// Length of an axis handle on the screen, in pixels.
pub const HANDLE_PIXELS: f32 = 110.0;
/// How close (pixels) the pointer must be to a handle to take it.
const PICK_RADIUS: f32 = 9.0;
/// The ring of a rotation handle, as a share of the axis length.
const RING: f32 = 0.85;
/// Segments of a ring.
const RING_SEGMENTS: usize = 64;
/// Snapping steps: moving (metres), turning (degrees), scaling (share).
pub const SNAP_MOVE: f32 = 0.5;
/// See [`SNAP_MOVE`].
pub const SNAP_ROTATE_DEG: f32 = 15.0;
/// See [`SNAP_MOVE`].
pub const SNAP_SCALE: f32 = 0.1;

const AXIS_COLORS: [[u8; 4]; 3] = [[230, 70, 70, 255], [90, 200, 90, 255], [80, 130, 240, 255]];
const ACTIVE: [u8; 4] = [255, 220, 60, 255];
const CENTRE: [u8; 4] = [235, 235, 235, 255];

/// What the handles do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Move along an axis.
    Translate,
    /// Turn about an axis.
    Rotate,
    /// Scale along an axis, or all of them at the centre.
    Scale,
}

/// Which axes the handles point along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// The world's.
    World,
    /// The entity's own.
    Local,
}

/// One handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// An axis: 0 is X, 1 is Y, 2 is Z.
    Axis(usize),
    /// The centre of the scale handles.
    Uniform,
}

/// The camera's view of the window's picture: what is where on the screen.
#[derive(Clone, Copy, Debug)]
pub struct Eye {
    view: Mat4,
    position: Vec3,
    fov_y: f32,
    ortho_height: Option<f32>,
    rect: Rect,
}

impl Eye {
    /// The camera as drawn, over the picture's place on the window.
    pub fn new(camera: &CameraView, rect: Rect) -> Self {
        Self {
            view: camera.view,
            position: camera.position,
            fov_y: camera.fov_y,
            ortho_height: camera.ortho_height,
            rect,
        }
    }

    fn aspect(&self) -> f32 {
        self.rect.width() / self.rect.height().max(1.0)
    }

    /// Where a world point is on the window (`None`: behind the camera).
    pub fn project(&self, point: Vec3) -> Option<Pos2> {
        let c = self.view.transform_point3(point);
        let (nx, ny) = match self.ortho_height {
            Some(h) => (c.x / (h * 0.5 * self.aspect()), c.y / (h * 0.5)),
            None => {
                if c.z >= -0.01 {
                    return None;
                }
                let t = (self.fov_y * 0.5).tan();
                (c.x / (-c.z * t * self.aspect()), c.y / (-c.z * t))
            }
        };
        Some(pos2(
            self.rect.min.x + (nx * 0.5 + 0.5) * self.rect.width(),
            self.rect.min.y + (0.5 - ny * 0.5) * self.rect.height(),
        ))
    }

    /// The ray through a window pixel: origin and unit direction.
    pub fn ray(&self, at: Pos2) -> (Vec3, Vec3) {
        let nx = ((at.x - self.rect.min.x) / self.rect.width().max(1.0) - 0.5) * 2.0;
        let ny = (0.5 - (at.y - self.rect.min.y) / self.rect.height().max(1.0)) * 2.0;
        let inverse = self.view.inverse();
        match self.ortho_height {
            Some(h) => (
                inverse.transform_point3(Vec3::new(
                    nx * h * 0.5 * self.aspect(),
                    ny * h * 0.5,
                    0.0,
                )),
                inverse.transform_vector3(-Vec3::Z).normalize(),
            ),
            None => {
                let t = (self.fov_y * 0.5).tan();
                (
                    self.position,
                    inverse
                        .transform_vector3(Vec3::new(nx * t * self.aspect(), ny * t, -1.0))
                        .normalize(),
                )
            }
        }
    }

    /// How many world units one pixel is at `point`.
    fn units_per_pixel(&self, point: Vec3) -> f32 {
        let height = self.rect.height().max(1.0);
        match self.ortho_height {
            Some(h) => h / height,
            None => {
                let depth = -self.view.transform_point3(point).z;
                2.0 * depth.max(0.01) * (self.fov_y * 0.5).tan() / height
            }
        }
    }

    /// The length of the handles in the world, so that they are
    /// [`HANDLE_PIXELS`] long on the screen.
    pub fn handle_length(&self, pivot: Vec3) -> f32 {
        HANDLE_PIXELS * self.units_per_pixel(pivot)
    }
}

/// The three axes, in the world or turned by the entity's rotation.
pub fn axes(rotation: Quat, space: Space) -> [Vec3; 3] {
    match space {
        Space::World => [Vec3::X, Vec3::Y, Vec3::Z],
        Space::Local => [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z],
    }
}

/// The point on the line (`origin`, unit `direction`) nearest the ray, as the
/// distance along the line from `origin` (`None`: they are parallel).
fn nearest_on_line(
    origin: Vec3,
    direction: Vec3,
    ray_origin: Vec3,
    ray_direction: Vec3,
) -> Option<f32> {
    let w = origin - ray_origin;
    let b = direction.dot(ray_direction);
    let d = direction.dot(w);
    let e = ray_direction.dot(w);
    let denominator = 1.0 - b * b;
    if denominator.abs() < 1e-4 {
        return None;
    }
    Some((b * e - d) / denominator)
}

/// Two unit vectors perpendicular to `normal` and to each other (the second
/// is `normal x first`).
fn plane_basis(normal: Vec3) -> (Vec3, Vec3) {
    let helper = if normal.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let first = normal.cross(helper).normalize();
    (first, normal.cross(first))
}

/// The angle (radians, turning right-handedly about `normal`) of the ray's
/// hit on the plane through `pivot`.
fn angle_on_plane(pivot: Vec3, normal: Vec3, ray_origin: Vec3, ray_direction: Vec3) -> Option<f32> {
    let facing = ray_direction.dot(normal);
    if facing.abs() < 1e-4 {
        return None;
    }
    let s = (pivot - ray_origin).dot(normal) / facing;
    let hit = ray_origin + ray_direction * s;
    let (u, v) = plane_basis(normal);
    let from = hit - pivot;
    Some(from.dot(v).atan2(from.dot(u)))
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_sq();
    if length_squared < 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

fn ring_points(pivot: Vec3, normal: Vec3, radius: f32) -> Vec<Vec3> {
    let (u, v) = plane_basis(normal);
    (0..=RING_SEGMENTS)
        .map(|i| {
            let angle = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            pivot + (u * angle.cos() + v * angle.sin()) * radius
        })
        .collect()
}

/// Whether the handle of axis `index` is shown in `mode`: in the 2D mode
/// (`flat`) only X and Y move and scale, and only Z turns.
fn shown(mode: Mode, index: usize, flat: bool) -> bool {
    !flat
        || match mode {
            Mode::Translate | Mode::Scale => index < 2,
            Mode::Rotate => index == 2,
        }
}

/// The handle under the pointer, if any: the nearest within a few pixels.
/// In the 2D mode (`flat`): only the X and Y arrows and scale handles, the
/// Z ring, and (moving) the centre, which moves in the plane.
pub fn hit_test(
    eye: &Eye,
    mode: Mode,
    axes: [Vec3; 3],
    pivot: Vec3,
    pointer: Pos2,
    flat: bool,
) -> Option<Handle> {
    let length = eye.handle_length(pivot);
    let centre = eye.project(pivot)?;
    let mut best: Option<(f32, Handle)> = None;
    let mut consider = |distance: f32, handle: Handle| {
        if distance <= PICK_RADIUS && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, handle));
        }
    };
    if flat && mode == Mode::Translate && (pointer - centre).length() <= 10.0 {
        return Some(Handle::Uniform);
    }
    for (index, axis) in axes.iter().enumerate() {
        if !shown(mode, index, flat) {
            continue;
        }
        match mode {
            Mode::Translate | Mode::Scale => {
                if let Some(end) = eye.project(pivot + *axis * length) {
                    consider(segment_distance(pointer, centre, end), Handle::Axis(index));
                }
            }
            Mode::Rotate => {
                let points: Vec<Option<Pos2>> = ring_points(pivot, *axis, length * RING)
                    .into_iter()
                    .map(|p| eye.project(p))
                    .collect();
                for pair in points.windows(2) {
                    if let [Some(a), Some(b)] = pair {
                        consider(segment_distance(pointer, *a, *b), Handle::Axis(index));
                    }
                }
            }
        }
    }
    if mode == Mode::Scale {
        // The centre wins over the axes near it.
        let distance = (pointer - centre).length();
        if distance <= 10.0 {
            return Some(Handle::Uniform);
        }
    }
    best.map(|(_, handle)| handle)
}

/// An entity's `Transform` and its parent's world matrix: what a drag
/// changes and what it must be converted through.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    /// Local translation.
    pub translation: Vec3,
    /// Local rotation.
    pub rotation: Quat,
    /// Local scale.
    pub scale: Vec3,
    /// The parent's world matrix (identity for a root).
    pub parent: Mat4,
}

impl Target {
    /// The entity's world matrix.
    pub fn world(&self) -> Mat4 {
        self.parent
            * Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// The entity's place in the world.
    pub fn pivot(&self) -> Vec3 {
        self.world().w_axis.truncate()
    }

    /// Its rotation in the world.
    pub fn world_rotation(&self) -> Quat {
        self.parent.to_scale_rotation_translation().1 * self.rotation
    }
}

/// A drag in progress: which handle, and where it began.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    mode: Mode,
    handle: Handle,
    /// The axes as they were when it began (they do not turn mid-drag).
    axes: [Vec3; 3],
    pivot: Vec3,
    /// Distance along the axis, the ring's angle, or the pointer's distance
    /// from the centre, when it began.
    start: f32,
    start_target: Target,
    /// Moving in the plane (the 2D mode's centre handle): where the
    /// pointer's ray met the plane through the pivot when it began.
    plane_start: Vec3,
    /// The 2D mode's grid (meters across and up): snapping puts the moved
    /// place on it, instead of moving by whole steps.
    grid: Option<[f32; 2]>,
}

impl Drag {
    /// The handle being dragged.
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// Takes hold of a handle at `pointer`.
    pub fn begin(
        eye: &Eye,
        mode: Mode,
        handle: Handle,
        space: Space,
        target: Target,
        pointer: Pos2,
    ) -> Option<Self> {
        let pivot = target.pivot();
        let axes = axes(target.world_rotation(), space);
        let mut drag = Self {
            mode,
            handle,
            axes,
            pivot,
            start: 0.0,
            start_target: target,
            plane_start: pivot,
            grid: None,
        };
        if (mode, handle) == (Mode::Translate, Handle::Uniform) {
            drag.plane_start = drag.on_plane(eye, pointer)?;
            return Some(drag);
        }
        drag.start = drag.measure(eye, pointer)?;
        Some(drag)
    }

    /// Snaps moves to a grid of `step` meters (across, up) in the world's
    /// plane: the 2D mode's pixels or tiles.
    #[must_use]
    pub fn with_grid(mut self, step: [f32; 2]) -> Self {
        self.grid = Some(step);
        self
    }

    /// Where the pointer's ray meets the plane through the pivot that faces
    /// +Z (the 2D world's plane).
    fn on_plane(&self, eye: &Eye, pointer: Pos2) -> Option<Vec3> {
        let (origin, direction) = eye.ray(pointer);
        if direction.z.abs() < 1e-6 {
            return None;
        }
        let along = (self.pivot.z - origin.z) / direction.z;
        let hit = origin + direction * along;
        hit.is_finite().then_some(hit)
    }

    fn measure(&self, eye: &Eye, pointer: Pos2) -> Option<f32> {
        let (origin, direction) = eye.ray(pointer);
        match (self.mode, self.handle) {
            (Mode::Translate | Mode::Scale, Handle::Axis(i)) => {
                nearest_on_line(self.pivot, self.axes[i], origin, direction)
            }
            (Mode::Rotate, Handle::Axis(i)) => {
                angle_on_plane(self.pivot, self.axes[i], origin, direction)
            }
            (Mode::Scale, Handle::Uniform) => {
                let centre = eye.project(self.pivot)?;
                Some((pointer - centre).length())
            }
            _ => None,
        }
    }

    /// The transform after the pointer moved to `pointer` (with snapping
    /// if asked): local translation, rotation, scale.
    pub fn transform(&self, eye: &Eye, pointer: Pos2, snap: bool) -> Option<Target> {
        let mut target = self.start_target;
        let on_grid = |mut world: Vec3, axes: [bool; 2]| {
            if let (true, Some(step)) = (snap, self.grid) {
                for (index, moved) in axes.iter().enumerate() {
                    if *moved && step[index].is_finite() && step[index] > 0.0 {
                        world[index] = (world[index] / step[index]).round() * step[index];
                    }
                }
            }
            world
        };
        if (self.mode, self.handle) == (Mode::Translate, Handle::Uniform) {
            let now = self.on_plane(eye, pointer)?;
            let world = on_grid(self.pivot + (now - self.plane_start), [true, true]);
            target.translation = target.parent.inverse().transform_point3(world);
            return Some(target);
        }
        let now = self.measure(eye, pointer)?;
        match (self.mode, self.handle) {
            (Mode::Translate, Handle::Axis(i)) => {
                let mut distance = now - self.start;
                if snap && self.grid.is_none() {
                    distance = (distance / SNAP_MOVE).round() * SNAP_MOVE;
                }
                let world = self.pivot + self.axes[i] * distance;
                let world = on_grid(world, [self.axes[i].x != 0.0, self.axes[i].y != 0.0]);
                target.translation = target.parent.inverse().transform_point3(world);
            }
            (Mode::Rotate, Handle::Axis(i)) => {
                let mut angle = now - self.start;
                // The short way round.
                while angle > std::f32::consts::PI {
                    angle -= std::f32::consts::TAU;
                }
                while angle < -std::f32::consts::PI {
                    angle += std::f32::consts::TAU;
                }
                if snap {
                    let step = SNAP_ROTATE_DEG.to_radians();
                    angle = (angle / step).round() * step;
                }
                let parent_rotation = target.parent.to_scale_rotation_translation().1;
                let turned =
                    Quat::from_axis_angle(self.axes[i], angle) * self.start_target.world_rotation();
                target.rotation = (parent_rotation.inverse() * turned).normalize();
            }
            (Mode::Scale, handle) => {
                // Guarded: a start at the very pivot would divide by nothing.
                let start = if self.start.abs() < 0.05 {
                    0.05_f32.copysign(self.start + 1e-9)
                } else {
                    self.start
                };
                let mut factor = (now / start).clamp(0.01, 100.0);
                if snap {
                    factor = (factor / SNAP_SCALE).round().max(1.0) * SNAP_SCALE;
                }
                match handle {
                    Handle::Axis(i) => target.scale[i] = self.start_target.scale[i] * factor,
                    Handle::Uniform => target.scale = self.start_target.scale * factor,
                }
            }
            _ => return None,
        }
        Some(target)
    }
}

fn put(canvas: &mut Canvas, clip: Rect, x: i32, y: i32, color: [u8; 4]) {
    if x >= clip.min.x as i32
        && x < clip.max.x as i32
        && y >= clip.min.y as i32
        && y < clip.max.y as i32
        && x >= 0
        && y >= 0
        && (x as u32) < canvas.width
        && (y as u32) < canvas.height
    {
        let at = (y as usize * canvas.width as usize + x as usize) * 4;
        canvas.pixels[at..at + 4].copy_from_slice(&color);
    }
}

/// A line two pixels thick.
fn line(canvas: &mut Canvas, clip: Rect, a: Pos2, b: Pos2, color: [u8; 4]) {
    let steps = (b - a).length().ceil().max(1.0) as i32;
    for i in 0..=steps {
        let p = a + (b - a) * (i as f32 / steps as f32);
        let (x, y) = (p.x.round() as i32, p.y.round() as i32);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            put(canvas, clip, x + dx, y + dy, color);
        }
    }
}

fn square(canvas: &mut Canvas, clip: Rect, at: Pos2, half: i32, color: [u8; 4]) {
    for dy in -half..=half {
        for dx in -half..=half {
            put(
                canvas,
                clip,
                at.x.round() as i32 + dx,
                at.y.round() as i32 + dy,
                color,
            );
        }
    }
}

/// Draws the handles over the picture, inside `clip`. `hot` is the handle
/// under the pointer or being dragged. In the 2D mode (`flat`): the handles
/// [`hit_test`] takes, and a centre square that moves in the plane.
pub fn draw(
    canvas: &mut Canvas,
    clip: Rect,
    eye: &Eye,
    (mode, flat): (Mode, bool),
    axes: [Vec3; 3],
    pivot: Vec3,
    hot: Option<Handle>,
) {
    let length = eye.handle_length(pivot);
    let Some(centre) = eye.project(pivot) else {
        return;
    };
    if flat && mode == Mode::Translate {
        let color = if hot == Some(Handle::Uniform) {
            ACTIVE
        } else {
            CENTRE
        };
        square(canvas, clip, centre, 5, color);
    }
    for (index, axis) in axes.iter().enumerate() {
        if !shown(mode, index, flat) {
            continue;
        }
        let color = if hot == Some(Handle::Axis(index)) {
            ACTIVE
        } else {
            AXIS_COLORS[index]
        };
        match mode {
            Mode::Translate | Mode::Scale => {
                let Some(end) = eye.project(pivot + *axis * length) else {
                    continue;
                };
                line(canvas, clip, centre, end, color);
                if mode == Mode::Scale {
                    square(canvas, clip, end, 4, color);
                } else {
                    // An arrow head: two short strokes back from the tip.
                    let back = (centre - end).normalized() * 14.0;
                    let side = egui::vec2(-back.y, back.x) * 0.4;
                    line(canvas, clip, end, end + back + side, color);
                    line(canvas, clip, end, end + back - side, color);
                }
            }
            Mode::Rotate => {
                let points: Vec<Option<Pos2>> = ring_points(pivot, *axis, length * RING)
                    .into_iter()
                    .map(|p| eye.project(p))
                    .collect();
                for pair in points.windows(2) {
                    if let [Some(a), Some(b)] = pair {
                        line(canvas, clip, *a, *b, color);
                    }
                }
            }
        }
    }
    if mode == Mode::Scale {
        let color = if hot == Some(Handle::Uniform) {
            ACTIVE
        } else {
            CENTRE
        };
        square(canvas, clip, centre, 5, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye(ortho: Option<f32>) -> Eye {
        // A camera 10 m in front of the origin, looking down -Z.
        let camera = CameraView {
            entity: wish_core::Entity::from_bits(0),
            view: Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0)),
            position: Vec3::new(0.0, 0.0, 10.0),
            fov_y: 60f32.to_radians(),
            near: 0.1,
            far: 100.0,
            ortho_height: ortho,
            pixel: None,
        };
        Eye::new(
            &camera,
            Rect::from_min_size(pos2(200.0, 30.0), egui::vec2(800.0, 600.0)),
        )
    }

    fn near(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn a_point_projects_and_its_ray_comes_back_through_it() {
        for ortho in [None, Some(8.0)] {
            let eye = eye(ortho);
            let point = Vec3::new(1.5, -0.75, 2.0);
            let at = eye.project(point).unwrap();
            let (origin, direction) = eye.ray(at);
            // The point lies on the ray.
            let to = point - origin;
            let off = to - direction * to.dot(direction);
            assert!(off.length() < 1e-3, "{ortho:?}: {off:?}");
        }
        // The middle of the picture is where the camera looks.
        let middle = eye(None).project(Vec3::ZERO).unwrap();
        assert!(
            near(middle.x, 600.0, 1e-3) && near(middle.y, 330.0, 1e-3),
            "{middle:?}"
        );
        // Behind the camera there is no place.
        assert!(eye(None).project(Vec3::new(0.0, 0.0, 20.0)).is_none());
    }

    #[test]
    fn handles_have_the_same_length_on_the_screen_at_any_distance() {
        let eye = eye(None);
        for z in [-30.0, 0.0, 6.0] {
            let pivot = Vec3::new(0.0, 0.0, z);
            let length = eye.handle_length(pivot);
            let a = eye.project(pivot).unwrap();
            let b = eye.project(pivot + Vec3::X * length).unwrap();
            assert!(near((b - a).length(), HANDLE_PIXELS, 0.5), "z = {z}");
        }
    }

    fn target() -> Target {
        Target {
            translation: Vec3::new(1.0, 2.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            parent: Mat4::IDENTITY,
        }
    }

    #[test]
    fn dragging_a_move_handle_follows_the_pointer_along_its_axis() {
        let eye = eye(None);
        let target = target();
        let pivot = target.pivot();
        let start = eye.project(pivot + Vec3::X * 0.5).unwrap();
        let drag = Drag::begin(
            &eye,
            Mode::Translate,
            Handle::Axis(0),
            Space::World,
            target,
            start,
        )
        .unwrap();
        // The pointer goes where +1.25 m along X is.
        let to = eye.project(pivot + Vec3::X * 1.75).unwrap();
        let moved = drag.transform(&eye, to, false).unwrap();
        assert!(
            near(moved.translation.x, 2.25, 1e-3),
            "{:?}",
            moved.translation
        );
        assert!(near(moved.translation.y, 2.0, 1e-3) && near(moved.translation.z, 0.0, 1e-3));
        // Snapped to half metres.
        let snapped = drag
            .transform(&eye, eye.project(pivot + Vec3::X * 1.9).unwrap(), true)
            .unwrap();
        assert!(
            near(snapped.translation.x, 2.5, 1e-3),
            "{:?}",
            snapped.translation
        );
    }

    #[test]
    fn a_move_goes_through_the_parent() {
        // A parent turned 90 degrees about Y and moved: a child's local X is
        // the world's -Z. Dragging the world X handle sets the local
        // translation that puts the child there.
        let eye = eye(None);
        let parent = Mat4::from_rotation_translation(
            Quat::from_rotation_y(90f32.to_radians()),
            Vec3::new(5.0, 0.0, 0.0),
        );
        let target = Target {
            translation: Vec3::new(0.0, 1.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            parent,
        };
        let pivot = target.pivot();
        assert!(near(pivot.x, 5.0, 1e-4));
        let start = eye.project(pivot + Vec3::X * 0.5).unwrap();
        let drag = Drag::begin(
            &eye,
            Mode::Translate,
            Handle::Axis(0),
            Space::World,
            target,
            start,
        )
        .unwrap();
        let to = eye.project(pivot + Vec3::X * 2.5).unwrap();
        let moved = drag.transform(&eye, to, false).unwrap();
        // World +2 in X is local -Z... of the turned parent: local (0, 1, ...).
        let world = moved.world().w_axis.truncate();
        assert!(
            near(world.x, 7.0, 1e-3) && near(world.y, 1.0, 1e-3),
            "{world:?}"
        );
    }

    #[test]
    fn dragging_a_ring_turns_by_the_angle_the_pointer_went_round() {
        let eye = eye(None);
        let target = target();
        let pivot = target.pivot();
        let radius = eye.handle_length(pivot) * RING;
        // Round the Z axis (facing the camera): from +X to +Y is 90 degrees.
        let start = eye.project(pivot + Vec3::X * radius).unwrap();
        let drag = Drag::begin(
            &eye,
            Mode::Rotate,
            Handle::Axis(2),
            Space::World,
            target,
            start,
        )
        .unwrap();
        let to = eye.project(pivot + Vec3::Y * radius).unwrap();
        let turned = drag.transform(&eye, to, false).unwrap();
        let (axis, angle) = turned.rotation.to_axis_angle();
        assert!(near(angle.to_degrees(), 90.0, 0.1), "{angle}");
        assert!(axis.dot(Vec3::Z) > 0.999, "{axis:?}");
        // Snapped to 15 degrees: 80 degrees becomes 75.
        let at = eye
            .project(
                pivot + Vec3::new(80f32.to_radians().cos(), 80f32.to_radians().sin(), 0.0) * radius,
            )
            .unwrap();
        let snapped = drag.transform(&eye, at, true).unwrap();
        assert!(near(
            snapped.rotation.to_axis_angle().1.to_degrees(),
            75.0,
            0.1
        ));
    }

    #[test]
    fn scaling_by_an_axis_and_from_the_centre() {
        let eye = eye(None);
        let target = target();
        let pivot = target.pivot();
        let start = eye.project(pivot + Vec3::X * 1.0).unwrap();
        let drag = Drag::begin(
            &eye,
            Mode::Scale,
            Handle::Axis(0),
            Space::World,
            target,
            start,
        )
        .unwrap();
        let scaled = drag
            .transform(&eye, eye.project(pivot + Vec3::X * 2.0).unwrap(), false)
            .unwrap();
        assert!(near(scaled.scale.x, 2.0, 1e-3), "{:?}", scaled.scale);
        assert!(near(scaled.scale.y, 1.0, 1e-3) && near(scaled.scale.z, 1.0, 1e-3));
        let centre = eye.project(pivot).unwrap();
        let drag = Drag::begin(
            &eye,
            Mode::Scale,
            Handle::Uniform,
            Space::World,
            target,
            centre + egui::vec2(40.0, 0.0),
        )
        .unwrap();
        let bigger = drag
            .transform(&eye, centre + egui::vec2(100.0, 0.0), false)
            .unwrap();
        assert!(
            near(bigger.scale.x, 2.5, 1e-3) && near(bigger.scale.y, 2.5, 1e-3),
            "{:?}",
            bigger.scale
        );
    }

    #[test]
    fn the_pointer_takes_the_nearest_handle_and_only_near_one() {
        let eye = eye(None);
        let target = target();
        let pivot = target.pivot();
        let world = axes(Quat::IDENTITY, Space::World);
        let on_y = eye
            .project(pivot + Vec3::Y * eye.handle_length(pivot) * 0.6)
            .unwrap();
        assert_eq!(
            hit_test(&eye, Mode::Translate, world, pivot, on_y, false),
            Some(Handle::Axis(1))
        );
        let far = on_y + egui::vec2(60.0, 0.0);
        assert_eq!(
            hit_test(&eye, Mode::Translate, world, pivot, far, false),
            None
        );
        let centre = eye.project(pivot).unwrap();
        assert_eq!(
            hit_test(&eye, Mode::Scale, world, pivot, centre, false),
            Some(Handle::Uniform)
        );
        // A ring is taken on its circle, not in its middle.
        let ring = eye
            .project(pivot + Vec3::Y * eye.handle_length(pivot) * RING)
            .unwrap();
        assert!(hit_test(&eye, Mode::Rotate, world, pivot, ring, false).is_some());
    }

    #[test]
    fn local_axes_follow_the_rotation() {
        let turned = Quat::from_rotation_y(90f32.to_radians());
        let local = axes(turned, Space::Local);
        assert!(
            (local[0] - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5,
            "{:?}",
            local[0]
        );
        assert_eq!(axes(turned, Space::World)[0], Vec3::X);
    }
}
