//! Clustered lighting (ADR-012, design note M6 §5): the view volume is cut
//! into tiles of the picture and slices of depth, and each cluster lists the
//! point and spot lights that can reach it. A pixel then looks only at its
//! cluster's lights instead of all of them.
//!
//! Lights are assigned here, on the CPU, in light order: the same scene
//! always gives the same lists. Each light is given to the clusters its
//! bounding sphere may touch (its box on the picture; its depth range one
//! slice deeper on each side, so that the GPU working out a pixel's slice
//! with a slightly different logarithm never misses a light).

use wish_core::math::{Mat4, Vec3, Vec4};

use crate::extract::LocalLightView;

/// The width and height of a tile, in pixels.
pub(crate) const TILE_PX: u32 = 64;
/// Depth slices between the near and far planes (spaced exponentially).
pub(crate) const SLICES: u32 = 24;
/// The most point and spot lights drawn; more are left out (and reported).
pub(crate) const MAX_LIGHTS: usize = 1024;
/// The most lights one cluster lists.
pub(crate) const MAX_PER_CLUSTER: u32 = 128;

/// The lists, ready for the GPU.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clusters {
    pub tiles_x: u32,
    pub tiles_y: u32,
    /// Slices per unit of `ln(depth / near)`.
    pub log_scale: f32,
    /// For each cluster (x fastest, then y, then slice): where its lights
    /// start in `indices`, and how many there are.
    pub ranges: Vec<[u32; 2]>,
    /// Light indices, cluster after cluster.
    pub indices: Vec<u32>,
    /// Lights left out because a cluster was full.
    pub dropped: u32,
    /// The depth range the slices cover: from the camera's near plane to
    /// the farthest light's reach (not the far plane: slices spread over a
    /// kilometre would each be meters thick and crowded).
    pub near: f32,
}

impl Clusters {
    #[cfg(test)]
    pub fn cluster_count(&self) -> usize {
        (self.tiles_x * self.tiles_y * SLICES) as usize
    }
}

fn slice_of(depth: f32, near: f32, log_scale: f32) -> i64 {
    if depth <= near {
        return 0;
    }
    ((depth / near).ln() * log_scale).floor() as i64
}

/// Assigns `lights` to the clusters of a `width` x `height` picture seen
/// through `view` and `projection`, between `near` and `far`.
pub(crate) fn build(
    lights: &[LocalLightView],
    view: &Mat4,
    projection: &Mat4,
    (width, height): (u32, u32),
    (near, far): (f32, f32),
) -> Clusters {
    let tiles_x = width.div_ceil(TILE_PX).max(1);
    let tiles_y = height.div_ceil(TILE_PX).max(1);
    let near = near.max(1e-4);
    let reach = lights
        .iter()
        .take(MAX_LIGHTS)
        .map(|light| -view.transform_point3(light.position).z + light.range)
        .fold(0.0f32, f32::max);
    let far = far.min(reach).max(near * 2.0);
    let log_scale = SLICES as f32 / (far / near).ln();
    let count = (tiles_x * tiles_y * SLICES) as usize;
    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); count];
    let mut dropped = 0;
    for (index, light) in lights.iter().enumerate().take(MAX_LIGHTS) {
        let center = view.transform_point3(light.position);
        let radius = light.range;
        let depth = -center.z;
        if depth + radius < near || depth - radius > far {
            continue;
        }
        let first_slice = (slice_of(depth - radius, near, log_scale) - 1).max(0);
        let last_slice = (slice_of(depth + radius, near, log_scale) + 1).min(i64::from(SLICES) - 1);
        // The light's box on the picture: the corners of its bounding box
        // projected; a sphere that reaches behind the camera covers all.
        let (mut min_x, mut min_y, mut max_x, mut max_y) =
            (0i64, 0i64, i64::from(tiles_x) - 1, i64::from(tiles_y) - 1);
        if center.z + radius < -near {
            let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for corner in 0..8 {
                let offset = Vec3::new(
                    if corner & 1 == 0 { -radius } else { radius },
                    if corner & 2 == 0 { -radius } else { radius },
                    if corner & 4 == 0 { -radius } else { radius },
                );
                let clip = *projection * Vec4::from(((center + offset), 1.0));
                if clip.w <= 1e-6 {
                    continue;
                }
                let ndc = clip.truncate() / clip.w;
                lo = lo.min(ndc);
                hi = hi.max(ndc);
            }
            if lo.x.is_finite() && hi.x.is_finite() {
                let to_tile_x =
                    |ndc: f32| ((ndc * 0.5 + 0.5) * width as f32 / TILE_PX as f32).floor() as i64;
                let to_tile_y =
                    |ndc: f32| ((0.5 - ndc * 0.5) * height as f32 / TILE_PX as f32).floor() as i64;
                // The box of the projected corners already holds the whole
                // sphere, so no extra tile is needed around it.
                min_x = to_tile_x(lo.x).max(0);
                max_x = to_tile_x(hi.x).min(i64::from(tiles_x) - 1);
                min_y = to_tile_y(hi.y).max(0);
                max_y = to_tile_y(lo.y).min(i64::from(tiles_y) - 1);
            }
        }
        if min_x > max_x || min_y > max_y || first_slice > last_slice {
            continue;
        }
        for slice in first_slice..=last_slice {
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let cluster =
                        ((slice * i64::from(tiles_y) + y) * i64::from(tiles_x) + x) as usize;
                    let list = &mut lists[cluster];
                    if list.len() < MAX_PER_CLUSTER as usize {
                        list.push(index as u32);
                    } else {
                        dropped += 1;
                    }
                }
            }
        }
    }
    let mut ranges = Vec::with_capacity(count);
    let mut indices = Vec::new();
    for list in &lists {
        ranges.push([indices.len() as u32, list.len() as u32]);
        indices.extend_from_slice(list);
    }
    Clusters {
        tiles_x,
        tiles_y,
        log_scale,
        ranges,
        indices,
        dropped,
        near,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use wish_core::Entity;

    fn light(position: Vec3, range: f32) -> LocalLightView {
        LocalLightView {
            entity: Entity::from_bits(1),
            position,
            intensity: [1.0; 3],
            range,
            spot: None,
        }
    }

    fn camera() -> (Mat4, Mat4) {
        // A camera at z = 5 looking along -Z.
        let view = Mat4::from_translation(Vec3::new(0.0, 0.0, -5.0));
        let projection =
            wish_core::math::camera::rh::proj::directx::perspective(1.0, 16.0 / 9.0, 0.1, 100.0);
        (view, projection)
    }

    /// The cluster a point of the world falls in, as the shader finds it.
    fn cluster_of(
        c: &Clusters,
        view: &Mat4,
        projection: &Mat4,
        point: Vec3,
        size: (u32, u32),
    ) -> usize {
        let clip = *projection * Vec4::from((view.transform_point3(point), 1.0));
        let ndc = clip.truncate() / clip.w;
        let px = (ndc.x * 0.5 + 0.5) * size.0 as f32;
        let py = (0.5 - ndc.y * 0.5) * size.1 as f32;
        let depth = -view.transform_point3(point).z;
        let slice = slice_of(depth, c.near, c.log_scale).clamp(0, i64::from(SLICES) - 1) as u32;
        let (tx, ty) = (px as u32 / TILE_PX, py as u32 / TILE_PX);
        ((slice * c.tiles_y + ty) * c.tiles_x + tx) as usize
    }

    #[test]
    fn every_lit_point_finds_its_light_in_its_cluster() {
        let (view, projection) = camera();
        let size = (1280, 720);
        let lights = [
            light(Vec3::new(1.0, 0.5, 0.0), 2.0),
            light(Vec3::new(-3.0, 0.0, -10.0), 5.0),
        ];
        let c = build(&lights, &view, &projection, size, (0.1, 100.0));
        assert_eq!(c.ranges.len(), c.cluster_count());
        for (index, light) in lights.iter().enumerate() {
            // Points inside the light's reach, all around it.
            for n in 0..200 {
                let t = n as f32 * 0.61803;
                let dir = Vec3::new(t.sin() * (t * 3.1).cos(), (t * 1.7).sin(), t.cos())
                    .normalize_or_zero();
                let point = light.position + dir * light.range * ((n % 10) as f32 / 10.0);
                let clip = projection * Vec4::from((view.transform_point3(point), 1.0));
                let ndc = clip.truncate() / clip.w;
                if clip.w <= 0.1 || ndc.x.abs() > 1.0 || ndc.y.abs() > 1.0 {
                    continue;
                }
                let cluster = cluster_of(&c, &view, &projection, point, size);
                let [start, len] = c.ranges[cluster];
                let list = &c.indices[start as usize..(start + len) as usize];
                assert!(
                    list.contains(&(index as u32)),
                    "light {index} missing at {point:?}"
                );
            }
        }
    }

    #[test]
    fn a_grid_of_two_hundred_lights_fits() {
        let view = (Mat4::from_translation(Vec3::new(0.0, 6.0, 8.0))
            * Mat4::from_rotation_x(-0.6435011))
        .inverse();
        let projection = wish_core::math::camera::rh::proj::directx::perspective(
            60f32.to_radians(),
            320.0 / 240.0,
            0.1,
            1000.0,
        );
        let lights: Vec<LocalLightView> = (0..200)
            .map(|i| {
                light(
                    Vec3::new((i % 20) as f32 - 9.5, 0.4, (i / 20) as f32 - 6.0),
                    1.5,
                )
            })
            .collect();
        let c = build(&lights, &view, &projection, (320, 240), (0.1, 1000.0));
        assert_eq!(c.dropped, 0);
    }

    #[test]
    fn lights_out_of_view_or_too_far_are_left_out() {
        let (view, projection) = camera();
        let behind = light(Vec3::new(0.0, 0.0, 20.0), 2.0);
        let beyond = light(Vec3::new(0.0, 0.0, -200.0), 5.0);
        let c = build(
            &[behind, beyond],
            &view,
            &projection,
            (640, 360),
            (0.1, 100.0),
        );
        assert!(c.indices.is_empty(), "{:?}", c.indices.len());
        // Deterministic: the same input, the same lists.
        let again = build(
            &[behind, beyond],
            &view,
            &projection,
            (640, 360),
            (0.1, 100.0),
        );
        assert_eq!(c, again);
    }
}
