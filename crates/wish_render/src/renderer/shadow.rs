//! The directional light's shadow map, worked out on the CPU: whether one is
//! drawn, how large it is, and the light's view fitted around what casts.

use wish_core::math::{Mat4, Vec3};

use crate::extract::RenderScene;

/// The default side of the shadow map, in texels (`RenderSettings`'s
/// default).
pub const SHADOW_MAP_SIZE: u32 = 2048;
/// The smallest and the largest shadow map the renderer draws, whatever the
/// settings say (game code can write any number into a resource).
pub(super) const SHADOW_MAP_RANGE: (u32, u32) = (64, 4096);

/// A light's view of the scene.
#[derive(Clone, Copy)]
pub(super) struct LightFit {
    /// World space to the light's clip space.
    pub view_proj: Mat4,
    /// Depth bias in clip-space depth.
    pub depth_bias: f32,
    /// How far to push a lookup off the surface, along its normal, meters.
    pub normal_offset: f32,
}

/// The shadow map of a frame.
#[derive(Clone, Copy)]
pub(super) struct ShadowPlan {
    /// The light's view, fitted around the casters.
    pub fit: LightFit,
    /// The map's side, in texels.
    pub map_size: u32,
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

/// The shadow map to draw for `scene`: none when shadows are switched off in
/// the settings, when there is no directional light or it casts none, or
/// when there is nothing to cover. Only the first directional light casts.
pub(super) fn plan(
    scene: &RenderScene,
    casters: &[(Mat4, Vec3, Vec3, u32)],
    device_limit: u32,
) -> Option<ShadowPlan> {
    if !scene.settings.shadows {
        return None;
    }
    let map_size = map_size(scene, device_limit);
    let light = scene.lights.first().filter(|light| light.shadows)?;
    let fit = fit_light(light.direction, casters, map_size)?;
    Some(ShadowPlan { fit, map_size })
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

    #[test]
    fn the_map_size_is_held_inside_what_can_be_drawn() {
        let mut world = wish_ecs::World::new();
        wish_transform::register(&mut world).unwrap();
        crate::components::register(&mut world).unwrap();
        let mut scene = crate::extract::extract(&world).unwrap();
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
        scene.lights.push(crate::extract::LightView {
            entity: wish_core::Entity::NULL,
            direction: Vec3::NEG_Y,
            radiance: [1.0; 3],
            shadows: true,
        });
        scene.settings.shadow_map_size = 512;
        assert_eq!(plan(&scene, &boxes, 16384).map(|p| p.map_size), Some(512));
        scene.settings.shadows = false;
        assert!(plan(&scene, &boxes, 16384).is_none());
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
}
