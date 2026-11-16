//! Lines drawn over a debug view to show what has no surface of its own
//! (design note M6 §3.4, §5): physics colliders and where lights reach.
//! The lines are made here on the CPU in world space, in entity order, and
//! drawn over everything, so a collider inside a wall is still seen.

use wish_core::math::{Quat, Vec3};
use wish_types::physics::ColliderShape;

use crate::extract::{ColliderView, LocalLightView, RenderScene};

/// One line vertex: position, then linear RGBA.
pub(crate) type LineVertex = [f32; 7];

/// Segments of a full circle.
const SEGMENTS: usize = 32;

/// Fixed ground and walls.
const FIXED: [f32; 4] = [0.25, 1.0, 0.35, 1.0];
/// Bodies that move.
const MOVING: [f32; 4] = [0.2, 0.85, 1.0, 1.0];
/// Sensors (triggers).
const SENSOR: [f32; 4] = [1.0, 0.85, 0.2, 1.0];

#[derive(Default)]
pub(crate) struct Lines {
    pub vertices: Vec<LineVertex>,
}

impl Lines {
    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        for p in [a, b] {
            self.vertices
                .push([p.x, p.y, p.z, color[0], color[1], color[2], color[3]]);
        }
    }

    /// A circle of `radius` around `center` in the plane spanned by `u`, `v`.
    fn circle(&mut self, center: Vec3, u: Vec3, v: Vec3, radius: f32, color: [f32; 4]) {
        self.arc(center, u, v, radius, 0.0, std::f32::consts::TAU, color);
    }

    #[allow(clippy::too_many_arguments)]
    fn arc(
        &mut self,
        center: Vec3,
        u: Vec3,
        v: Vec3,
        radius: f32,
        from: f32,
        to: f32,
        color: [f32; 4],
    ) {
        let steps = ((to - from).abs() / std::f32::consts::TAU * SEGMENTS as f32)
            .ceil()
            .max(2.0) as usize;
        let point = |t: f32| center + (u * t.cos() + v * t.sin()) * radius;
        for i in 0..steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let b = from + (to - from) * (i + 1) as f32 / steps as f32;
            self.line(point(a), point(b), color);
        }
    }

    fn collider(&mut self, c: &ColliderView) {
        let color = if c.sensor {
            SENSOR
        } else if c.moving {
            MOVING
        } else {
            FIXED
        };
        let r: Quat = c.rotation;
        let (x, y, z) = (r * Vec3::X, r * Vec3::Y, r * Vec3::Z);
        let half = c.size.abs() * 0.5;
        match c.shape {
            ColliderShape::Box => {
                let corner = |sx: f32, sy: f32, sz: f32| {
                    c.center + x * half.x * sx + y * half.y * sy + z * half.z * sz
                };
                for (a, b) in [
                    ((-1., -1., -1.), (1., -1., -1.)),
                    ((-1., 1., -1.), (1., 1., -1.)),
                    ((-1., -1., 1.), (1., -1., 1.)),
                    ((-1., 1., 1.), (1., 1., 1.)),
                    ((-1., -1., -1.), (-1., 1., -1.)),
                    ((1., -1., -1.), (1., 1., -1.)),
                    ((-1., -1., 1.), (-1., 1., 1.)),
                    ((1., -1., 1.), (1., 1., 1.)),
                    ((-1., -1., -1.), (-1., -1., 1.)),
                    ((1., -1., -1.), (1., -1., 1.)),
                    ((-1., 1., -1.), (-1., 1., 1.)),
                    ((1., 1., -1.), (1., 1., 1.)),
                ] {
                    self.line(corner(a.0, a.1, a.2), corner(b.0, b.1, b.2), color);
                }
            }
            ColliderShape::Sphere => {
                let radius = half.max_element();
                self.circle(c.center, x, y, radius, color);
                self.circle(c.center, y, z, radius, color);
                self.circle(c.center, z, x, radius, color);
            }
            ColliderShape::Cylinder => {
                let radius = half.x.max(half.z);
                let (top, bottom) = (c.center + y * half.y, c.center - y * half.y);
                self.circle(top, x, z, radius, color);
                self.circle(bottom, x, z, radius, color);
                for side in [x, -x, z, -z] {
                    self.line(top + side * radius, bottom + side * radius, color);
                }
            }
            ColliderShape::Capsule => {
                let radius = half.x.max(half.z);
                let straight = (half.y - radius).max(0.0);
                let (top, bottom) = (c.center + y * straight, c.center - y * straight);
                self.circle(top, x, z, radius, color);
                self.circle(bottom, x, z, radius, color);
                for side in [x, -x, z, -z] {
                    self.line(top + side * radius, bottom + side * radius, color);
                }
                let half_turn = std::f32::consts::PI;
                for side in [x, z] {
                    self.arc(top, side, y, radius, 0.0, half_turn, color);
                    self.arc(bottom, side, -y, radius, 0.0, half_turn, color);
                }
            }
        }
    }

    fn light(&mut self, light: &LocalLightView) {
        let [r, g, b] = light.intensity;
        let peak = r.max(g).max(b).max(1e-6);
        let color = [r / peak, g / peak, b / peak, 1.0];
        let p = light.position;
        // A small cross at the light itself.
        let mark = (light.range * 0.05).clamp(0.05, 0.3);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(p - axis * mark, p + axis * mark, color);
        }
        match light.spot {
            None => {
                self.circle(p, Vec3::X, Vec3::Y, light.range, color);
                self.circle(p, Vec3::Y, Vec3::Z, light.range, color);
                self.circle(p, Vec3::Z, Vec3::X, light.range, color);
            }
            Some((direction, cos_inner, cos_outer)) => {
                let u = direction.any_orthonormal_vector();
                let v = direction.cross(u);
                for (cosine, dim) in [(cos_outer, 1.0), (cos_inner, 0.5)] {
                    let angle = cosine.clamp(-1.0, 1.0).acos();
                    let length = light.range * angle.cos();
                    let radius = light.range * angle.sin();
                    let end = p + direction * length;
                    let color = [color[0] * dim, color[1] * dim, color[2] * dim, 1.0];
                    self.circle(end, u, v, radius, color);
                    if dim == 1.0 {
                        for side in [u, -u, v, -v] {
                            self.line(p, end + side * radius, color);
                        }
                    }
                }
            }
        }
    }
}

/// The colliders of `scene` as lines.
pub(crate) fn colliders(scene: &RenderScene) -> Vec<LineVertex> {
    let mut lines = Lines::default();
    for collider in &scene.colliders {
        lines.collider(collider);
    }
    lines.vertices
}

/// The point and spot lights of `scene` as lines: where they reach.
pub(crate) fn lights(scene: &RenderScene) -> Vec<LineVertex> {
    let mut lines = Lines::default();
    for light in &scene.local_lights {
        lines.light(light);
    }
    lines.vertices
}
