//! The directional light's shadows, worked out on the CPU: whether any are
//! drawn, into how many maps, how large they are, and the light's view for
//! each.
//!
//! Two ways, chosen by `RenderSettings.shadow_cascades`:
//!
//! - **one map** (1, the default), fitted around everything that casts
//!   ([`fit_light`]): the map it always was;
//! - **cascades** (2 to 4): maps fitted to slices of the camera's depth, as
//!   tiles of one texture ([`CascadePlan`]). Each slice is enclosed in a
//!   sphere and its map is a square around that sphere whose centre is
//!   snapped to whole texels, so the map's texels do not move in the world
//!   when the camera turns or moves.
//!
//! Design: `docs/design/M12-look-3-cascaded-shadows.md`.

use wish_core::Diagnostic;
use wish_core::math::{Mat4, Vec3};

use super::Bytes;
use crate::extract::{CameraView, RenderScene};

/// The default side of the shadow map, in texels (`RenderSettings`'s
/// default).
pub const SHADOW_MAP_SIZE: u32 = 2048;
/// The smallest and the largest shadow map the renderer draws, whatever the
/// settings say (game code can write any number into a resource).
pub(super) const SHADOW_MAP_RANGE: (u32, u32) = (64, 4096);
/// The most cascades a light has.
pub(super) const MAX_CASCADES: usize = 4;
/// The shortest and the longest shadow distance, in meters, whatever the
/// settings say.
pub(super) const DISTANCE_RANGE: (f32, f32) = (0.1, 10_000.0);
/// The shadow distance used when the setting is not a number
/// (`RenderSettings`'s default).
const DEFAULT_DISTANCE: f32 = 100.0;
/// The share of the shadow distance over which shadows fade out at its end.
const FADE_SHARE: f32 = 0.1;
/// How much of the slices' depths is logarithmic (the rest is uniform).
const SPLIT_LOGARITHMIC: f32 = 0.75;
/// The ratio of the shadow distance to the depth the logarithm starts at.
const SPLIT_RATIO: f32 = 1000.0;
/// The band at a cascade's border over which it hands over to the next, as
/// a share of the map's side.
const BAND_SHARE: f32 = 0.04;
/// The least that band is, in texels.
const BAND_LEAST: f32 = 2.0;
/// Texels kept free between a slice's sphere and the band: one for the
/// snapping of the map's centre, one and a half for the normal offset of a
/// lookup, two for the filter's footprint, rounded up.
const GUARD_TEXELS: f32 = 5.0;
/// Bytes of `Cascades` in `cascades.wgsl`: four matrices and eleven vectors.
pub(super) const CASCADES_UNIFORM_SIZE: u64 = 432;
/// Distance between the maps' matrices in the shadow pass's light uniform.
/// 256 is a multiple of every adapter's uniform offset alignment.
pub(super) const LIGHT_UNIFORM_STRIDE: u64 = 256;

/// A light's view of the scene.
#[derive(Clone, Copy, Debug)]
pub(super) struct LightFit {
    /// World space to the light's clip space.
    pub view_proj: Mat4,
    /// Depth bias in clip-space depth.
    pub depth_bias: f32,
    /// How far to push a lookup off the surface, along its normal, meters.
    pub normal_offset: f32,
}

/// One cascade: the shadow map of one slice of the camera's depth.
#[derive(Clone, Debug)]
pub(super) struct Cascade {
    /// The light's view of the slice.
    pub fit: LightFit,
    /// One over the part of its clip-space depth over which it hands over
    /// to the next cascade at its far side.
    pub inverse_depth_band: f32,
    /// The draws that can shadow the slice, as positions in the draw list:
    /// the only ones drawn into this map.
    pub draws: Vec<u32>,
}

/// The cascades of a frame.
#[derive(Clone, Debug)]
pub(super) struct CascadePlan {
    /// The side of one cascade's tile in the atlas, in texels.
    pub tile: u32,
    /// How many tiles the atlas is across and down.
    pub grid: (u32, u32),
    /// The cascades, the nearest first.
    pub cascades: Vec<Cascade>,
    /// The view depth of a world position `p` is `dot(xyz, p) + w`.
    pub depth_axis: [f32; 4],
    /// The depth in front of the camera at which shadows are gone, meters.
    pub distance: f32,
    /// The band at a map's border over which it hands over, in texels.
    pub band: f32,
}

impl CascadePlan {
    /// Where cascade `index` is in the atlas: its left and top edge and its
    /// side, in texels.
    pub(super) fn area(&self, index: usize) -> [u32; 3] {
        let index = u32::try_from(index).unwrap_or(0);
        let columns = self.grid.0.max(1);
        [
            (index % columns) * self.tile,
            (index / columns) * self.tile,
            self.tile,
        ]
    }

    /// The atlas's width and height in texels.
    pub(super) fn atlas(&self) -> (u32, u32) {
        (self.grid.0 * self.tile, self.grid.1 * self.tile)
    }
}

/// The shadow maps of a frame.
#[derive(Clone, Debug)]
pub(super) enum ShadowPlan {
    /// One map fitted around the casters.
    Single {
        /// The light's view.
        fit: LightFit,
        /// The map's side, in texels.
        map_size: u32,
    },
    /// Maps fitted to slices of the camera's depth.
    Cascades(CascadePlan),
}

impl ShadowPlan {
    /// How many shadow maps are drawn.
    pub(super) fn maps(&self) -> u32 {
        match self {
            Self::Single { .. } => 1,
            Self::Cascades(plan) => u32::try_from(plan.cascades.len()).unwrap_or(u32::MAX),
        }
    }

    /// The pooled texture the maps are drawn into: its name, width and
    /// height. The single map and the atlas have names of their own, so
    /// that a renderer whose scenes alternate keeps both.
    pub(super) fn texture(&self) -> (&'static str, u32, u32) {
        match self {
            Self::Single { map_size, .. } => ("shadow map", *map_size, *map_size),
            Self::Cascades(plan) => {
                let (width, height) = plan.atlas();
                ("shadow cascades", width, height)
            }
        }
    }

    /// The light uniform of the shadow pass: one matrix for each map,
    /// [`LIGHT_UNIFORM_STRIDE`] apart.
    pub(super) fn light_uniform(&self) -> Vec<u8> {
        let mut bytes = Bytes::default();
        match self {
            Self::Single { fit, .. } => bytes.mat4(&fit.view_proj),
            Self::Cascades(plan) => {
                for (index, cascade) in plan.cascades.iter().enumerate() {
                    bytes.pad_to(index as u64 * LIGHT_UNIFORM_STRIDE);
                    bytes.mat4(&cascade.fit.view_proj);
                }
            }
        }
        bytes.0
    }
}

/// Appends `count` zero bytes.
fn zeros(bytes: &mut Bytes, count: u64) {
    bytes.pad_to(bytes.0.len() as u64 + count);
}

/// Appends `Cascades` (`cascades.wgsl`) for the lit picture of a frame with
/// this shadow plan: the cascades' numbers, or "no cascades" (a count of 0),
/// with which the shader asks the single map as it always did.
pub(super) fn write_cascades(plan: Option<&ShadowPlan>, bytes: &mut Bytes) {
    match plan {
        Some(ShadowPlan::Cascades(plan)) => write_plan(plan, bytes),
        _ => zeros(bytes, CASCADES_UNIFORM_SIZE),
    }
}

/// Appends `Cascades` for the `shadow_cascades` view: the cascades as the
/// lit picture uses them, the single map described as one cascade that
/// covers everything (and is marked as the single map), or nothing.
pub(super) fn write_described(plan: Option<&ShadowPlan>, bytes: &mut Bytes) {
    match plan {
        Some(ShadowPlan::Cascades(plan)) => write_plan(plan, bytes),
        Some(ShadowPlan::Single { fit, map_size }) => {
            let texel = 1.0 / (*map_size).max(1) as f32;
            let unused = (MAX_CASCADES - 1) as u64;
            bytes.mat4(&fit.view_proj);
            zeros(bytes, 64 * unused);
            bytes.floats(&[0.0, 0.0, 1.0, 1.0]);
            zeros(bytes, 16 * unused);
            // No band at its border and none in depth: inside or not.
            bytes.floats(&[fit.depth_bias, fit.normal_offset, 1e6, 0.0]);
            zeros(bytes, 16 * unused);
            bytes.floats(&[0.0; 4]);
            bytes.floats(&[1.0, *map_size as f32, 1e6, 1.0]);
            // No distance at which it ends.
            bytes.floats(&[1e30, 1.0, texel, texel]);
        }
        None => zeros(bytes, CASCADES_UNIFORM_SIZE),
    }
}

fn write_plan(plan: &CascadePlan, bytes: &mut Bytes) {
    let used = plan.cascades.len().min(MAX_CASCADES);
    let unused = (MAX_CASCADES - used) as u64;
    let (columns, rows) = (plan.grid.0.max(1) as f32, plan.grid.1.max(1) as f32);
    let tile = plan.tile.max(1) as f32;
    for cascade in plan.cascades.iter().take(used) {
        bytes.mat4(&cascade.fit.view_proj);
    }
    zeros(bytes, 64 * unused);
    for index in 0..used {
        let [left, top, _] = plan.area(index);
        bytes.floats(&[
            left as f32 / (columns * tile),
            top as f32 / (rows * tile),
            1.0 / columns,
            1.0 / rows,
        ]);
    }
    zeros(bytes, 16 * unused);
    for cascade in plan.cascades.iter().take(used) {
        bytes.floats(&[
            cascade.fit.depth_bias,
            cascade.fit.normal_offset,
            cascade.inverse_depth_band,
            0.0,
        ]);
    }
    zeros(bytes, 16 * unused);
    bytes.floats(&plan.depth_axis);
    bytes.floats(&[used as f32, tile, 1.0 / plan.band.max(1e-3), 0.0]);
    bytes.floats(&[
        plan.distance,
        1.0 / (plan.distance * FADE_SHARE).max(1e-6),
        1.0 / (columns * tile),
        1.0 / (rows * tile),
    ]);
}

/// The side of the shadow map the scene's settings ask for, held inside what
/// the renderer and the device (`device_limit`, its largest texture side)
/// can draw.
pub(super) fn map_size(scene: &RenderScene, device_limit: u32) -> u32 {
    scene
        .settings
        .shadow_map_size
        .clamp(SHADOW_MAP_RANGE.0, SHADOW_MAP_RANGE.1)
        .min(device_limit.max(1))
}

/// How many shadow maps the scene's settings ask for, held to what the
/// renderer draws: 1 is the single map, 2 to [`MAX_CASCADES`] are cascades.
pub(super) fn count(scene: &RenderScene) -> usize {
    (scene.settings.shadow_cascades as usize).clamp(1, MAX_CASCADES)
}

/// How far from `view` shadows are drawn with cascades, in meters of view
/// depth: the settings' distance, cleaned, and never beyond the camera's
/// far plane.
pub(super) fn distance(scene: &RenderScene, view: &CameraView) -> f32 {
    let asked = scene.settings.shadow_distance;
    let asked = if asked.is_finite() {
        asked.clamp(DISTANCE_RANGE.0, DISTANCE_RANGE.1)
    } else {
        DEFAULT_DISTANCE
    };
    if view.far.is_finite() && view.far > 0.0 {
        asked.min(view.far).max(1e-3)
    } else {
        asked
    }
}

/// What [`plan`] worked out.
pub(super) struct Planned {
    /// The shadow maps to draw, if any.
    pub plan: Option<ShadowPlan>,
    /// Why the picture has no shadows although it was asked for some, when
    /// that is something the scene's settings can mend.
    pub warning: Option<Diagnostic>,
}

/// One draw as the shadow maps see it: its matrix, the corners of its local
/// bounding box and its identity.
pub(super) type Caster = (Mat4, Vec3, Vec3, u32);

/// The shadow maps to draw for `scene` seen through `view` in a picture of
/// `aspect` (width over height): none when shadows are switched off in the
/// settings, when there is no directional light or it casts none, when
/// there is nothing to cover, or when nothing drawn casts a shadow. Only the
/// first directional light casts.
///
/// `casters` are every draw's matrix, box and identity: the maps are fitted
/// around all of them, so that everything drawn can receive a shadow.
/// `casts` says which of them cast one (a see-through surface does not); a
/// position past its end casts. A cascade draws only casters that cast.
pub(super) fn plan(
    scene: &RenderScene,
    view: &CameraView,
    aspect: f32,
    (casters, casts): (&[Caster], &[bool]),
    device_limit: u32,
) -> Planned {
    let none = Planned {
        plan: None,
        warning: None,
    };
    if !scene.settings.shadows {
        return none;
    }
    let map_size = map_size(scene, device_limit);
    let Some(light) = scene.lights.first().filter(|light| light.shadows) else {
        return none;
    };
    let casting = |index: usize| casts.get(index).copied().unwrap_or(true);
    // Nothing that is drawn casts a shadow (only see-through surfaces):
    // no map, and an empty shadow pass is not drawn.
    if !(0..casters.len()).any(casting) {
        return none;
    }
    let count = count(scene);
    if count < 2 {
        return Planned {
            plan: fit_light(light.direction, casters, map_size)
                .map(|fit| ShadowPlan::Single { fit, map_size }),
            warning: None,
        };
    }
    let distance = distance(scene, view);
    let lens = Lens::of(view, aspect);
    let Some(mut cascades) = fit_cascades(
        light.direction,
        &lens,
        (count, distance),
        casters,
        (map_size, device_limit),
    ) else {
        return none;
    };
    for cascade in &mut cascades.cascades {
        cascade.draws.retain(|index| casting(*index as usize));
    }
    if cascades
        .cascades
        .iter()
        .all(|cascade| cascade.draws.is_empty())
    {
        // Nothing that is drawn can shadow any place within the distance:
        // no map is drawn, and the picture says why it has no shadows.
        // (This is what bounding boxes can prove. A ground that reaches
        // under the camera is in reach of the cascades even when everything
        // the camera looks at is beyond the distance; that picture has no
        // shadows either, and the `shadow_cascades` view shows it grey.)
        //
        // How far the nearest thing in front of the camera is, by the
        // measure the shadow distance uses: depth along the direction the
        // camera looks, to the middle of its box. It is said only when it
        // explains the picture, which is when it is beyond the distance. A
        // thing behind the camera, or within the distance but far to the
        // side, is out of reach too, and a number for it would read as a
        // contradiction ("nothing within 20 m: the nearest is 15 m away").
        let nearest = casters
            .iter()
            .enumerate()
            .filter(|(index, _)| casting(*index))
            .map(|(_, (matrix, low, high, _))| {
                lens.forward
                    .dot(matrix.transform_point3((*low + *high) * 0.5) - lens.position)
            })
            .filter(|depth| depth.is_finite() && *depth > 0.0)
            .fold(f32::INFINITY, f32::min);
        let how_far = if nearest.is_finite() && nearest > distance {
            format!(
                " (the nearest is about {} m in front of it)",
                nearest.round()
            )
        } else {
            String::new()
        };
        let warning = (!casters.is_empty()).then(|| {
            Diagnostic::warning(
                "shadows_out_of_range",
                format!(
                    "The picture has no shadows: RenderSettings.shadow_cascades is {count}, so shadows are drawn only up to shadow_distance ({distance} m) in front of the camera, and nothing that is drawn can cast a shadow into that part of the scene{how_far}."
                ),
                "Raise RenderSettings.shadow_distance until it reaches what the camera looks at, or set shadow_cascades to 1: one shadow map then covers the whole scene, whatever the distance.",
            )
        });
        return Planned {
            plan: None,
            warning,
        };
    }
    Planned {
        plan: Some(ShadowPlan::Cascades(cascades)),
        warning: None,
    }
}

/// The depths in front of the camera at which the slices of `count`
/// cascades end, the last one at `distance`: three quarters logarithmic
/// (each slice a fixed factor deeper than the one before, taken from a
/// thousandth of the distance) and one quarter uniform. They are fixed
/// shares of the distance, whatever the camera's near plane.
pub(super) fn splits(count: usize, distance: f32) -> Vec<f32> {
    let count = count.clamp(1, MAX_CASCADES);
    (1..=count)
        .map(|index| {
            if index == count {
                return distance;
            }
            let share = index as f32 / count as f32;
            let logarithmic = SPLIT_RATIO.powf(share - 1.0);
            distance * (SPLIT_LOGARITHMIC * logarithmic + (1.0 - SPLIT_LOGARITHMIC) * share)
        })
        .collect()
}

/// How a camera's view widens with depth: what the sphere around a slice of
/// it depends on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lens {
    /// The camera's place.
    position: Vec3,
    /// The direction it looks, a unit vector.
    forward: Vec3,
    /// For a perspective camera: how far a corner of the view is from the
    /// view axis per meter of depth, squared. `None` for an orthographic
    /// one.
    spread_squared: Option<f32>,
    /// For an orthographic camera: half the diagonal of what it shows,
    /// squared.
    half_diagonal_squared: f32,
}

impl Lens {
    /// The lens of `view` for a picture of `aspect` (width over height).
    /// Every number is held to something a sphere can be made of: game code
    /// can write anything into a camera.
    pub(super) fn of(view: &CameraView, aspect: f32) -> Self {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect.clamp(1e-3, 1e3)
        } else {
            1.0
        };
        let forward = view
            .view
            .inverse()
            .transform_vector3(Vec3::NEG_Z)
            .normalize_or(Vec3::NEG_Z);
        let half_angle = if view.fov_y.is_finite() {
            (view.fov_y * 0.5).clamp(0.5f32.to_radians(), 89.5f32.to_radians())
        } else {
            30f32.to_radians()
        };
        let tan = half_angle.tan();
        let spread_squared = view
            .ortho_height
            .is_none()
            .then_some(tan * tan * (1.0 + aspect * aspect));
        let height = match view.ortho_height {
            Some(height) if height.is_finite() => height.clamp(1e-3, 1e5),
            _ => 10.0,
        };
        let half = height * 0.5;
        Self {
            position: view.position,
            forward,
            spread_squared,
            half_diagonal_squared: half * half * (1.0 + aspect * aspect),
        }
    }

    /// The smallest sphere around the part of the view between the depths
    /// `near` and `far` that has its centre on the view axis: the centre's
    /// depth and the radius. They depend on the two depths and the lens
    /// alone, not on where the camera is or looks.
    pub(super) fn sphere(&self, near: f32, far: f32) -> (f32, f32) {
        match self.spread_squared {
            Some(k2) => {
                // The corners of the slice are at near * k and far * k from
                // the axis. The centre that is as far from the near corners
                // as from the far ones is at (near + far)(1 + k^2) / 2; when
                // that is beyond the far plane (a wide view), the far
                // plane's own centre is the best there is.
                let centre = ((near + far) * (1.0 + k2) * 0.5).min(far);
                let radius = ((far - centre).powi(2) + far * far * k2).sqrt();
                (centre, radius)
            }
            None => {
                let centre = (near + far) * 0.5;
                let half_depth = (far - near) * 0.5;
                (
                    centre,
                    (self.half_diagonal_squared + half_depth * half_depth).sqrt(),
                )
            }
        }
    }

    /// The point `depth` meters in front of the camera on its view axis.
    fn on_axis(&self, depth: f32) -> Vec3 {
        self.position + self.forward * depth
    }
}

/// The light's frame: it looks along `direction`, with an "up" that depends
/// on the direction alone, so that the frame is the same for every cascade
/// and every frame.
fn light_view(direction: Vec3) -> Mat4 {
    let up = if direction.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    wish_core::math::camera::rh::view::look_to_mat4(Vec3::ZERO, direction, up)
}

/// The tiles of an atlas of `count` cascades: how many across and down, and
/// the side of each in texels (the asked size, held to what fits a texture
/// of `device_limit`).
fn atlas_grid(count: usize, map_size: u32, device_limit: u32) -> ((u32, u32), u32) {
    let count = u32::try_from(count.clamp(1, MAX_CASCADES)).unwrap_or(1);
    let columns = count.min(2);
    let rows = count.div_ceil(columns);
    let tile = map_size.min(device_limit.max(1) / columns.max(rows)).max(1);
    ((columns, rows), tile)
}

/// Fits `count` cascades to the view of `lens` up to `distance`, for a light
/// that shines along `direction`: each a square around its slice's sphere,
/// its centre snapped to whole texels, holding every caster that can shadow
/// the slice. None when the numbers are not finite.
pub(super) fn fit_cascades(
    direction: Vec3,
    lens: &Lens,
    (count, distance): (usize, f32),
    casters: &[(Mat4, Vec3, Vec3, u32)],
    (map_size, device_limit): (u32, u32),
) -> Option<CascadePlan> {
    let view = light_view(direction);
    let (grid, tile) = atlas_grid(count, map_size, device_limit);
    let side = tile as f32;
    let band = (side * BAND_SHARE).max(BAND_LEAST);
    // The part of the map's half side the sphere may fill, in texels.
    let usable = (side * 0.5 - GUARD_TEXELS - band).max(1.0);

    // Every caster's box as the light sees it.
    let boxes: Vec<(Vec3, Vec3)> = casters
        .iter()
        .map(|(matrix, min, max, _)| {
            let mut low = Vec3::splat(f32::MAX);
            let mut high = Vec3::splat(f32::MIN);
            for corner in 0..8 {
                let point = Vec3::new(
                    if corner & 1 == 0 { min.x } else { max.x },
                    if corner & 2 == 0 { min.y } else { max.y },
                    if corner & 4 == 0 { min.z } else { max.z },
                );
                let in_light = view.transform_point3(matrix.transform_point3(point));
                low = low.min(in_light);
                high = high.max(in_light);
            }
            (low, high)
        })
        .collect();

    let mut cascades = Vec::with_capacity(count);
    let mut near = 0.0;
    for far in splits(count, distance) {
        let (centre_depth, radius) = lens.sphere(near, far);
        let radius = radius.max(1e-3);
        let texel = radius / usable;
        let half = texel * side * 0.5;
        let centre = view.transform_point3(lens.on_axis(centre_depth));
        // Whole texels of this map, in the light's own frame: as the camera
        // moves, the square moves from texel to texel and the texels stay
        // where they are in the world.
        let x = (centre.x / texel).floor() * texel;
        let y = (centre.y / texel).floor() * texel;
        // In its frame the light looks down -Z: distances grow as z falls.
        let centre_distance = -centre.z;
        let far_plane = centre_distance + radius + (band + 3.0) * texel;
        let mut near_plane = centre_distance - radius;
        let mut draws = Vec::new();
        for (index, (low, high)) in boxes.iter().enumerate() {
            let beside =
                low.x > x + half || high.x < x - half || low.y > y + half || high.y < y - half;
            // Wholly on the far side of the slice: it shadows nothing here.
            let beyond = -high.z > far_plane;
            if beside || beyond || !low.is_finite() || !high.is_finite() {
                continue;
            }
            // Anything nearer the light can shadow the slice, however far
            // away it is: the map starts at the nearest of them.
            near_plane = near_plane.min(-high.z);
            draws.push(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let near_plane = near_plane - 4.0 * texel;
        let projection = wish_core::math::camera::rh::proj::directx::orthographic(
            x - half,
            x + half,
            y - half,
            y + half,
            near_plane,
            far_plane,
        );
        let depth_range = (far_plane - near_plane).max(1e-3);
        let view_proj = projection * view;
        if !view_proj.is_finite() || !texel.is_finite() {
            return None;
        }
        cascades.push(Cascade {
            fit: LightFit {
                view_proj,
                depth_bias: texel * 2.0 / depth_range,
                normal_offset: texel * 1.5,
            },
            inverse_depth_band: depth_range / (band * texel),
            draws,
        });
        near = far;
    }
    let axis = lens.forward;
    let depth_axis = [axis.x, axis.y, axis.z, -axis.dot(lens.position)];
    if !depth_axis.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some(CascadePlan {
        tile,
        grid,
        cascades,
        depth_axis,
        distance,
        band,
    })
}

/// Fits an orthographic view along direction around all casters (a
/// matrix and the corners of a local bounding box each), so that a shadow
/// map of `map_size` texels covers exactly what is drawn. None when there is
/// nothing to cover or the numbers are not finite.
pub(super) fn fit_light(
    direction: Vec3,
    casters: &[(Mat4, Vec3, Vec3, u32)],
    map_size: u32,
) -> Option<LightFit> {
    let up = if direction.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let view = wish_core::math::camera::rh::view::look_to_mat4(Vec3::ZERO, direction, up);
    let mut low = Vec3::splat(f32::MAX);
    let mut high = Vec3::splat(f32::MIN);
    for (matrix, min, max, _) in casters {
        for corner in 0..8 {
            let point = Vec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let in_light = view.transform_point3(matrix.transform_point3(point));
            low = low.min(in_light);
            high = high.max(in_light);
        }
    }
    if casters.is_empty() || !low.is_finite() || !high.is_finite() {
        return None;
    }
    // A margin of a few texels so that edges are not clipped by filtering.
    let width = (high.x - low.x).max(high.y - low.y).max(1e-3);
    let texel = width / map_size.max(1) as f32;
    let margin = texel * 4.0;
    // In this view the light looks down -Z: distances grow as z falls.
    let near = -high.z - margin;
    let far = -low.z + margin;
    let projection = wish_core::math::camera::rh::proj::directx::orthographic(
        low.x - margin,
        high.x + margin,
        low.y - margin,
        high.y + margin,
        near,
        far,
    );
    let depth_range = (far - near).max(1e-3);
    Some(LightFit {
        view_proj: projection * view,
        depth_bias: texel * 2.0 / depth_range,
        normal_offset: texel * 1.5,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(fit: &LightFit, point: Vec3) -> Vec3 {
        let clip = fit.view_proj * point.extend(1.0);
        clip.truncate() / clip.w
    }

    #[test]
    fn the_light_view_covers_every_corner_of_what_is_drawn() {
        let boxes = [
            (
                Mat4::from_translation(Vec3::new(3.0, 0.5, -2.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                1,
            ),
            (
                Mat4::from_scale(Vec3::new(30.0, 1.0, 30.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                1,
            ),
        ];
        for direction in [
            Vec3::NEG_Y,
            Vec3::new(0.3, -0.8, -0.5).normalize(),
            Vec3::Y,
            Vec3::NEG_Z,
        ] {
            for map_size in [SHADOW_MAP_RANGE.0, SHADOW_MAP_SIZE, SHADOW_MAP_RANGE.1] {
                let fit = fit_light(direction, &boxes, map_size).expect("something to cover");
                for (matrix, low, high, _) in &boxes {
                    for corner in 0..8u8 {
                        let point = Vec3::new(
                            if corner & 1 == 0 { low.x } else { high.x },
                            if corner & 2 == 0 { low.y } else { high.y },
                            if corner & 4 == 0 { low.z } else { high.z },
                        );
                        let ndc = clip(&fit, matrix.transform_point3(point));
                        assert!(
                            ndc.x.abs() <= 1.0
                                && ndc.y.abs() <= 1.0
                                && (0.0..=1.0).contains(&ndc.z),
                            "{direction:?} at {map_size}: {ndc:?} is outside the shadow map"
                        );
                    }
                }
                assert!(fit.depth_bias > 0.0 && fit.normal_offset > 0.0);
            }
        }
    }

    #[test]
    fn nothing_drawn_or_bad_numbers_mean_no_shadow_map() {
        assert!(fit_light(Vec3::NEG_Y, &[], SHADOW_MAP_SIZE).is_none());
        let broken = [(
            Mat4::from_scale(Vec3::splat(f32::NAN)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        assert!(fit_light(Vec3::NEG_Y, &broken, SHADOW_MAP_SIZE).is_none());
    }

    fn empty_scene() -> RenderScene {
        let mut world = wish_ecs::World::new();
        wish_transform::register(&mut world).unwrap();
        crate::components::register(&mut world).unwrap();
        crate::extract::extract(&world).unwrap()
    }

    /// A camera at `position` that looks along `forward`.
    fn camera(position: Vec3, forward: Vec3, ortho_height: Option<f32>) -> CameraView {
        let up = if forward.y.abs() > 0.99 {
            Vec3::X
        } else {
            Vec3::Y
        };
        CameraView {
            entity: wish_core::Entity::NULL,
            view: wish_core::math::camera::rh::view::look_to_mat4(position, forward, up),
            position,
            fov_y: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            ortho_height,
        }
    }

    fn sun(direction: Vec3) -> crate::extract::LightView {
        crate::extract::LightView {
            entity: wish_core::Entity::NULL,
            direction,
            radiance: [1.0; 3],
            shadows: true,
        }
    }

    #[test]
    fn the_map_size_is_held_inside_what_can_be_drawn() {
        let mut scene = empty_scene();
        assert_eq!(scene.settings.shadow_map_size, SHADOW_MAP_SIZE);
        assert_eq!(map_size(&scene, 16384), SHADOW_MAP_SIZE);
        // Game code can write any number into the resource.
        for (asked, limit, drawn) in [
            (0, 16384, 64),
            (63, 16384, 64),
            (1000, 16384, 1000),
            (u32::MAX, 16384, 4096),
            // A device with small textures.
            (4096, 2048, 2048),
        ] {
            scene.settings.shadow_map_size = asked;
            assert_eq!(map_size(&scene, limit), drawn, "{asked} on {limit}");
        }
        // Switched off: no map at all, whatever there is to cover.
        let boxes = [(Mat4::IDENTITY, Vec3::splat(-0.5), Vec3::splat(0.5), 1)];
        let view = camera(Vec3::new(0.0, 1.0, 5.0), Vec3::NEG_Z, None);
        scene.lights.push(sun(Vec3::NEG_Y));
        scene.settings.shadow_map_size = 512;
        let planned = plan(&scene, &view, 1.5, (&boxes, &[]), 16384);
        assert!(
            matches!(planned.plan, Some(ShadowPlan::Single { map_size: 512, .. })),
            "{:?}",
            planned.plan
        );
        assert!(planned.warning.is_none());
        scene.settings.shadows = false;
        assert!(
            plan(&scene, &view, 1.5, (&boxes, &[]), 16384)
                .plan
                .is_none()
        );
    }

    #[test]
    fn a_smaller_map_has_larger_texels_and_so_a_larger_bias() {
        let boxes = [(
            Mat4::from_scale(Vec3::new(30.0, 1.0, 30.0)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        let fine = fit_light(Vec3::NEG_Y, &boxes, 2048).expect("something to cover");
        let coarse = fit_light(Vec3::NEG_Y, &boxes, 256).expect("something to cover");
        assert!((coarse.normal_offset / fine.normal_offset - 8.0).abs() < 1e-3);
        assert!(coarse.depth_bias > fine.depth_bias);
    }

    #[test]
    fn the_settings_of_the_cascades_are_cleaned() {
        let mut scene = empty_scene();
        let view = camera(Vec3::ZERO, Vec3::NEG_Z, None);
        // The defaults: one map, a hundred meters.
        assert_eq!(count(&scene), 1);
        assert_eq!(distance(&scene, &view), 100.0);
        for (asked, drawn) in [
            (0, 1),
            (1, 1),
            (2, 2),
            (4, 4),
            (5, 4),
            (99, 4),
            (u32::MAX, 4),
        ] {
            scene.settings.shadow_cascades = asked;
            assert_eq!(count(&scene), drawn, "{asked} cascades");
        }
        for (asked, drawn) in [
            (0.0, 0.1),
            (-5.0, 0.1),
            (f32::NAN, 100.0),
            (f32::INFINITY, 100.0),
            (f32::NEG_INFINITY, 100.0),
            (40.0, 40.0),
            (1e9, 1000.0),
        ] {
            scene.settings.shadow_distance = asked;
            assert_eq!(distance(&scene, &view), drawn, "a distance of {asked}");
        }
        // Never beyond what the camera draws.
        let mut short = view;
        short.far = 25.0;
        scene.settings.shadow_distance = 100.0;
        assert_eq!(distance(&scene, &short), 25.0);
        // A camera whose far plane is not a number does not break it.
        short.far = f32::NAN;
        assert_eq!(distance(&scene, &short), 100.0);
    }

    #[test]
    fn the_slices_are_fixed_shares_of_the_distance() {
        assert_eq!(splits(1, 100.0), vec![100.0]);
        let shares = |count: usize| -> Vec<f32> {
            splits(count, 100.0)
                .into_iter()
                .map(|depth| (depth * 10.0).round() / 10.0)
                .collect()
        };
        assert_eq!(shares(2), vec![14.9, 100.0]);
        assert_eq!(shares(3), vec![9.1, 24.2, 100.0]);
        assert_eq!(shares(4), vec![6.7, 14.9, 32.1, 100.0]);
        // The same shares of any distance, each slice deeper than the last.
        for (near, far) in splits(4, 100.0).iter().zip(splits(4, 20.0)) {
            assert!((near / 5.0 - far).abs() < 1e-4);
        }
        // Out of range counts are held.
        assert_eq!(splits(0, 10.0), vec![10.0]);
        assert_eq!(splits(9, 10.0).len(), MAX_CASCADES);
    }

    /// The eight corners of the part of `view`'s frustum between two depths.
    fn slice_corners(view: &CameraView, aspect: f32, near: f32, far: f32) -> Vec<Vec3> {
        let to_world = view.view.inverse();
        let mut corners = Vec::new();
        for depth in [near, far] {
            let (half_w, half_h) = match view.ortho_height {
                Some(height) => (height * 0.5 * aspect, height * 0.5),
                None => {
                    let tan = (view.fov_y * 0.5).tan();
                    (depth * tan * aspect, depth * tan)
                }
            };
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                corners.push(to_world.transform_point3(Vec3::new(x * half_w, y * half_h, -depth)));
            }
        }
        corners
    }

    #[test]
    fn the_sphere_of_a_slice_holds_the_slice_whatever_the_camera() {
        let position = Vec3::new(3.0, 2.0, -7.0);
        for forward in [
            Vec3::NEG_Z,
            Vec3::new(0.6, -0.3, 0.2).normalize(),
            Vec3::NEG_Y,
        ] {
            for ortho in [None, Some(12.0)] {
                for aspect in [0.5, 16.0 / 9.0, 3.0] {
                    let view = camera(position, forward, ortho);
                    let lens = Lens::of(&view, aspect);
                    let mut near = 0.0;
                    for far in splits(4, 80.0) {
                        let (depth, radius) = lens.sphere(near, far);
                        let centre = lens.on_axis(depth);
                        for corner in slice_corners(&view, aspect, near, far) {
                            let away = (corner - centre).length();
                            assert!(
                                away <= radius * 1.0001,
                                "{forward:?} {ortho:?} {aspect}: a corner of {near}..{far} is {away} from the centre, the radius is {radius}"
                            );
                        }
                        near = far;
                    }
                }
            }
        }
        // The radius depends on the slice and the lens, not on the camera's
        // place or direction: this is what keeps a map's texels one size.
        let a = Lens::of(&camera(Vec3::ZERO, Vec3::NEG_Z, None), 1.5).sphere(5.0, 20.0);
        let b = Lens::of(
            &camera(
                Vec3::new(40.0, 3.0, 9.0),
                Vec3::new(0.3, -0.5, 0.8).normalize(),
                None,
            ),
            1.5,
        )
        .sphere(5.0, 20.0);
        assert_eq!(a, b);
    }

    /// A ground of 400 m and a post that stands 4 m in front of the origin.
    fn level() -> [(Mat4, Vec3, Vec3, u32); 2] {
        [
            (
                Mat4::from_scale(Vec3::new(400.0, 1.0, 400.0)),
                Vec3::new(-0.5, 0.0, -0.5),
                Vec3::new(0.5, 0.0, 0.5),
                1,
            ),
            (
                Mat4::from_translation(Vec3::new(0.0, 1.0, -4.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                2,
            ),
        ]
    }

    #[test]
    fn every_point_of_a_slice_is_well_inside_its_map() {
        let direction = Vec3::new(0.5, -1.0, -0.3).normalize();
        for (forward, ortho) in [
            (Vec3::NEG_Z, None),
            (Vec3::new(0.7, -0.2, -0.4).normalize(), None),
            (Vec3::NEG_Y, Some(30.0)),
        ] {
            let view = camera(Vec3::new(1.3, 1.7, 2.9), forward, ortho);
            let aspect = 16.0 / 9.0;
            for map in [64, 256, 2048] {
                let plan = fit_cascades(
                    direction,
                    &Lens::of(&view, aspect),
                    (4, 60.0),
                    &level(),
                    (map, 16384),
                )
                .expect("finite numbers");
                assert_eq!(plan.cascades.len(), 4);
                assert_eq!((plan.grid, plan.tile), ((2, 2), map));
                let mut near = 0.0;
                for (cascade, far) in plan.cascades.iter().zip(splits(4, 60.0)) {
                    for corner in slice_corners(&view, aspect, near, far) {
                        let ndc = clip(&cascade.fit, corner);
                        // In texels from the map's border.
                        let edge = (1.0 - ndc.x.abs().max(ndc.y.abs())) * 0.5 * map as f32;
                        // The filter's two texels, the band, and room for
                        // a lookup's normal offset.
                        assert!(
                            edge >= 2.0 + plan.band + 1.5,
                            "map {map}, slice to {far}: a corner is {edge} texels from the border (band {})",
                            plan.band
                        );
                        // And clear of the band at its far side, in depth.
                        assert!(
                            ndc.z >= 0.0 && (1.0 - ndc.z) * cascade.inverse_depth_band >= 1.0,
                            "map {map}, slice to {far}: depth {}",
                            ndc.z
                        );
                    }
                    near = far;
                }
                // Finer near the camera: each map's texels are larger than
                // the one's before.
                // (The normal offset is one and a half texels.) Under an
                // orthographic camera a slice is a slab of the same width
                // at every depth, so its map is only a little larger.
                let step = if ortho.is_some() { 1.0 } else { 1.5 };
                for pair in plan.cascades.windows(2) {
                    assert!(pair[1].fit.normal_offset > pair[0].fit.normal_offset * step);
                }
            }
        }
    }

    #[test]
    fn the_texels_of_a_cascade_stay_where_they_are_when_the_camera_moves() {
        let direction = Vec3::new(0.5, -1.0, -0.3).normalize();
        let fit = |position: Vec3, forward: Vec3| {
            fit_cascades(
                direction,
                &Lens::of(&camera(position, forward, None), 1.5),
                (4, 60.0),
                &level(),
                (256, 16384),
            )
            .expect("finite numbers")
        };
        let before = fit(Vec3::new(0.0, 1.7, 3.0), Vec3::NEG_Z);
        let turned = fit(
            Vec3::new(0.0, 1.7, 3.0),
            Vec3::new(0.3, -0.1, -1.0).normalize(),
        );
        let moved = fit(Vec3::new(0.37, 1.9, 2.21), Vec3::NEG_Z);
        // A point of the world, in texels of a cascade's map.
        let texels = |plan: &CascadePlan, index: usize, point: Vec3| {
            let ndc = clip(&plan.cascades[index].fit, point);
            (ndc.x * 0.5 * 256.0, ndc.y * 0.5 * 256.0)
        };
        let point = Vec3::new(0.3, 0.0, -2.2);
        for other in [&turned, &moved] {
            for index in 0..4 {
                // The same texel size (the normal offset is one and a half
                // texels).
                assert_eq!(
                    before.cascades[index].fit.normal_offset,
                    other.cascades[index].fit.normal_offset
                );
                // And the point is a whole number of texels from where it
                // was in the map: the grid did not move, only the window.
                let (x0, y0) = texels(&before, index, point);
                let (x1, y1) = texels(other, index, point);
                for shift in [x1 - x0, y1 - y0] {
                    assert!(
                        (shift - shift.round()).abs() < 0.02,
                        "cascade {index}: the grid moved by {shift} texels"
                    );
                }
            }
        }
    }

    #[test]
    fn what_can_shadow_a_slice_is_drawn_into_its_map_and_nothing_else() {
        // The sun shines along +X and down at 45 degrees. A tower stands
        // 30 m towards the sun from the camera, 40 m high: its shadow
        // reaches the camera's feet. A crate stands 150 m to the side.
        let direction = Vec3::new(1.0, -1.0, 0.0).normalize();
        let casters = [
            (
                Mat4::from_scale(Vec3::new(400.0, 1.0, 400.0)),
                Vec3::new(-0.5, 0.0, -0.5),
                Vec3::new(0.5, 0.0, 0.5),
                1,
            ),
            (
                Mat4::from_translation(Vec3::new(-30.0, 20.0, 0.0))
                    * Mat4::from_scale(Vec3::new(2.0, 40.0, 2.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                2,
            ),
            (
                Mat4::from_translation(Vec3::new(0.0, 0.5, 150.0)),
                Vec3::splat(-0.5),
                Vec3::splat(0.5),
                3,
            ),
        ];
        let view = camera(Vec3::new(0.0, 1.7, 0.0), Vec3::NEG_Z, None);
        let plan = fit_cascades(
            direction,
            &Lens::of(&view, 1.5),
            (4, 60.0),
            &casters,
            (256, 16384),
        )
        .expect("finite numbers");
        let nearest = &plan.cascades[0];
        // The ground and the tower, not the crate.
        assert_eq!(nearest.draws, vec![0, 1]);
        // The tower's top is inside the map's depth, although it is far
        // outside the slice.
        let top = clip(&nearest.fit, Vec3::new(-30.0, 40.0, 0.0));
        assert!((0.0..=1.0).contains(&top.z), "the tower's top: {top:?}");
        // No cascade holds the crate: it is behind the camera, 150 m away.
        assert!(plan.cascades.iter().all(|c| !c.draws.contains(&2)));

        // And the plan of a scene whose every caster is out of reach is no
        // map at all, with the reason.
        let mut scene = empty_scene();
        scene.lights.push(sun(direction));
        scene.settings.shadow_cascades = 3;
        scene.settings.shadow_distance = 20.0;
        let far_away = [casters[2]];
        let planned = plan_of(&scene, &view, &far_away);
        assert!(planned.plan.is_none());
        let warning = planned.warning.expect("the reason");
        assert_eq!(warning.code, "shadows_out_of_range");
        assert!(warning.hint.contains("shadow_distance"));
        // The crate is behind the camera: out of reach, and no number that
        // would read as "within the distance" (it is 150 m away).
        assert!(
            warning.message.contains("(20 m) in front of the camera")
                && !warning.message.contains("nearest"),
            "{}",
            warning.message
        );
        // The case that used to contradict itself: the only thing drawn is
        // 15 m behind the camera, nearer than the 20 m shadows reach.
        let behind = [(
            Mat4::from_translation(Vec3::new(0.0, 0.5, 15.0)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        let message = plan_of(&scene, &view, &behind)
            .warning
            .expect("the reason")
            .message
            .clone();
        assert!(
            message.contains("in front of the camera")
                && !message.contains("nearest")
                && !message.contains("15 m"),
            "{message}"
        );
        // Ahead of the camera and beyond the distance: how far, as depth
        // along the view (the crate is 150 m ahead and 30 m to the side).
        let ahead = [(
            Mat4::from_translation(Vec3::new(30.0, 0.5, -150.0)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        let message = plan_of(&scene, &view, &ahead)
            .warning
            .expect("the reason")
            .message
            .clone();
        assert!(
            message.contains("the nearest is about 150 m in front of it"),
            "{message}"
        );
        // Within the distance in depth but far to the side: out of reach,
        // and again no number.
        let beside = [(
            Mat4::from_translation(Vec3::new(300.0, 0.5, -5.0)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            1,
        )];
        let planned = plan_of(&scene, &view, &beside);
        assert!(planned.plan.is_none());
        let message = planned.warning.expect("the reason").message.clone();
        assert!(!message.contains("nearest"), "{message}");
        // With one map the same scene has its shadow map.
        scene.settings.shadow_cascades = 1;
        let planned = plan_of(&scene, &view, &far_away);
        assert!(matches!(planned.plan, Some(ShadowPlan::Single { .. })));
        assert!(planned.warning.is_none());
    }

    fn plan_of(
        scene: &RenderScene,
        view: &CameraView,
        casters: &[(Mat4, Vec3, Vec3, u32)],
    ) -> Planned {
        plan(scene, view, 1.5, (casters, &[]), 16384)
    }

    #[test]
    fn the_atlas_holds_its_tiles_and_fits_the_device() {
        assert_eq!(atlas_grid(2, 2048, 8192), ((2, 1), 2048));
        assert_eq!(atlas_grid(3, 2048, 8192), ((2, 2), 2048));
        assert_eq!(atlas_grid(4, 2048, 8192), ((2, 2), 2048));
        assert_eq!(atlas_grid(4, 4096, 8192), ((2, 2), 4096));
        // A device with small textures: smaller tiles.
        assert_eq!(atlas_grid(4, 4096, 4096), ((2, 2), 2048));
        assert_eq!(atlas_grid(4, 4096, 0), ((2, 2), 1));
        let view = camera(Vec3::ZERO, Vec3::NEG_Z, None);
        let plan = fit_cascades(
            Vec3::NEG_Y,
            &Lens::of(&view, 1.5),
            (3, 50.0),
            &level(),
            (128, 16384),
        )
        .expect("finite numbers");
        assert_eq!(plan.atlas(), (256, 256));
        assert_eq!(plan.area(0), [0, 0, 128]);
        assert_eq!(plan.area(1), [128, 0, 128]);
        assert_eq!(plan.area(2), [0, 128, 128]);
        // What the shaders read is as long as their struct, for every kind
        // of plan.
        let single = ShadowPlan::Single {
            fit: fit_light(Vec3::NEG_Y, &level(), 128).expect("something to cover"),
            map_size: 128,
        };
        let cascades = ShadowPlan::Cascades(plan);
        for plan in [None, Some(&single), Some(&cascades)] {
            for write in [write_cascades, write_described] {
                let mut bytes = Bytes::default();
                write(plan, &mut bytes);
                assert_eq!(bytes.0.len() as u64, CASCADES_UNIFORM_SIZE);
                let finite = bytes
                    .0
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|value| f32::from_le_bytes(*value).is_finite());
                assert!(finite, "every number the shader reads is finite");
            }
        }
        assert_eq!(single.light_uniform().len(), 64);
        assert_eq!(cascades.light_uniform().len(), 2 * 256 + 64);
        assert_eq!((single.maps(), cascades.maps()), (1, 3));
    }

    #[test]
    fn numbers_that_are_not_numbers_mean_no_cascades() {
        let view = camera(Vec3::ZERO, Vec3::NEG_Z, None);
        let lens = Lens::of(&view, 1.5);
        // A light without a direction.
        assert!(fit_cascades(Vec3::ZERO, &lens, (4, 50.0), &level(), (256, 16384)).is_none());
        assert!(
            fit_cascades(
                Vec3::splat(f32::NAN),
                &lens,
                (4, 50.0),
                &level(),
                (256, 16384)
            )
            .is_none()
        );
        // A camera with numbers game code broke still has a lens.
        let mut broken = view;
        broken.fov_y = f32::NAN;
        broken.ortho_height = Some(f32::INFINITY);
        let lens = Lens::of(&broken, f32::NAN);
        let (depth, radius) = lens.sphere(0.0, 10.0);
        assert!(depth.is_finite() && radius.is_finite() && radius > 0.0);
        // A caster that is nowhere is left out; the others are drawn.
        let mut casters = level().to_vec();
        casters.push((
            Mat4::from_scale(Vec3::splat(f32::NAN)),
            Vec3::splat(-0.5),
            Vec3::splat(0.5),
            3,
        ));
        let plan = fit_cascades(
            Vec3::NEG_Y,
            &Lens::of(&view, 1.5),
            (2, 50.0),
            &casters,
            (256, 16384),
        )
        .expect("finite numbers");
        assert!(plan.cascades.iter().all(|c| !c.draws.contains(&2)));
        assert!(plan.cascades[0].draws.contains(&0));
    }
}
