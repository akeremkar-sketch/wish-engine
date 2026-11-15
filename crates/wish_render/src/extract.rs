//! Reading the world into a [`RenderScene`]: everything the renderer needs,
//! copied out, so drawing never touches the simulation.
//!
//! Extraction only reads. Draws, lights and cameras come out in entity order,
//! so the same world always gives the same scene (and the same debug
//! images).

use wish_core::math::{Mat4, Vec3};
use wish_core::{AssetRef, Color, Diagnostic, Entity, Result};
use wish_ecs::World;
use wish_transform::GlobalTransform;

use crate::components::{
    Ambient, Camera, DirectionalLight, Exposure, Material, Model, Primitive, Shape, Sky,
};

/// What a draw shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesh {
    /// A built-in shape.
    Primitive(Shape),
    /// A glTF model; its path is in [`Draw::model_path`].
    Model,
}

/// A surface's look, copied out of a [`Material`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialData {
    /// Linear RGBA colour.
    pub base_color: [f32; 4],
    /// 0 to 1.
    pub metallic: f32,
    /// 0 to 1.
    pub roughness: f32,
    /// Linear RGB light given off.
    pub emissive: [f32; 3],
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
        }
    }
}

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    /// The entity that owns it (for identity buffers and `describe`).
    pub entity: Entity,
    /// Local space to world space.
    pub world: Mat4,
    /// What to draw.
    pub mesh: Mesh,
    /// For [`Mesh::Model`], the asset path (project-relative, possibly with
    /// a `#sub` part).
    pub model_path: Option<String>,
    /// Its material.
    pub material: MaterialData,
}

/// A camera ready to use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// The entity that owns it.
    pub entity: Entity,
    /// World space to the camera's space.
    pub view: Mat4,
    /// The camera's position in world space.
    pub position: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Near plane distance.
    pub near: f32,
    /// Far plane distance.
    pub far: f32,
}

impl CameraView {
    /// The projection for an image of `aspect` (width over height), for a
    /// right-handed view space: clip depth runs 0 to 1 and +Y is up, as wgpu
    /// expects.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        wish_core::math::camera::rh::proj::directx::perspective(
            self.fov_y, aspect, self.near, self.far,
        )
    }
}

/// A directional light ready to use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightView {
    /// The entity that owns it.
    pub entity: Entity,
    /// The direction the light travels (unit vector).
    pub direction: Vec3,
    /// Linear RGB multiplied by the illuminance (lux).
    pub radiance: [f32; 3],
    /// Whether it casts shadows.
    pub shadows: bool,
}

/// The sky colours, linear RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyData {
    /// Straight up.
    pub zenith: [f32; 3],
    /// At the horizon.
    pub horizon: [f32; 3],
    /// Straight down.
    pub ground: [f32; 3],
    /// cd/m² of a colour of 1.
    pub luminance: f32,
}

/// Everything drawn in one frame, copied out of the world.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    /// The cameras, in entity order.
    pub cameras: Vec<CameraView>,
    /// The directional lights, in entity order.
    pub lights: Vec<LightView>,
    /// What to draw, in entity order.
    pub draws: Vec<Draw>,
    /// The sky.
    pub sky: SkyData,
    /// Ambient light: linear RGB multiplied by its illuminance (lux).
    pub ambient: [f32; 3],
    /// Exposure value at ISO 100.
    pub ev100: f32,
}

fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_array();
    [r, g, b]
}

fn finite_matrix(matrix: &Mat4) -> bool {
    matrix.to_cols_array().iter().all(|value| value.is_finite())
}

fn bad_camera(entity: Entity, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error(
        "invalid_camera",
        format!("Camera {entity}: {message}"),
        hint,
    )
}

/// Copies what the renderer needs out of the world.
///
/// Call [`wish_transform::update_global_transforms`] first so that placements
/// are current. Entities with a `Primitive` or `Model` but no
/// `GlobalTransform` are drawn at the origin. A camera with `far <= near`, or
/// whose placement is not finite, is an `invalid_camera` error.
pub fn extract(world: &World) -> Result<RenderScene> {
    let mut scene = RenderScene {
        cameras: Vec::new(),
        lights: Vec::new(),
        draws: Vec::new(),
        sky: {
            let sky = world.resource::<Sky>().copied().unwrap_or_default();
            SkyData {
                zenith: rgb(sky.zenith),
                horizon: rgb(sky.horizon),
                ground: rgb(sky.ground),
                luminance: sky.luminance,
            }
        },
        ambient: {
            let ambient = world.resource::<Ambient>().copied().unwrap_or_default();
            let [r, g, b] = rgb(ambient.color);
            [
                r * ambient.intensity,
                g * ambient.intensity,
                b * ambient.intensity,
            ]
        },
        ev100: world
            .resource::<Exposure>()
            .copied()
            .unwrap_or_default()
            .ev100,
    };
    let global = |world: &World, entity: Entity| {
        world
            .get::<GlobalTransform>(entity)
            .map_or(Mat4::IDENTITY, |global| global.matrix)
    };

    let entities: Vec<Entity> = world.entities().collect();
    for entity in entities {
        let placement = global(world, entity);
        if let Some(camera) = world.get::<Camera>(entity).copied() {
            if camera.far <= camera.near {
                return Err(bad_camera(
                    entity,
                    format!(
                        "far ({}) must be larger than near ({}).",
                        camera.far, camera.near
                    ),
                    "Raise \"far\" or lower \"near\" on the Camera component.",
                ));
            }
            if !finite_matrix(&placement) {
                return Err(bad_camera(
                    entity,
                    "its placement is not a finite matrix.".to_owned(),
                    "Check the camera's Transform (and its parents') for NaN or infinite values.",
                ));
            }
            let view = placement.inverse();
            if !finite_matrix(&view) {
                return Err(bad_camera(
                    entity,
                    "its placement cannot be inverted (a scale of zero?).".to_owned(),
                    "Give the camera's Transform (and its parents') a non-zero scale.",
                ));
            }
            scene.cameras.push(CameraView {
                entity,
                view,
                position: placement.w_axis.truncate(),
                fov_y: camera.fov_y_deg.to_radians(),
                near: camera.near,
                far: camera.far,
            });
        }
        if let Some(light) = world.get::<DirectionalLight>(entity).copied() {
            // The light shines along its -Z axis.
            let direction = placement
                .transform_vector3(Vec3::NEG_Z)
                .try_normalize()
                .unwrap_or(Vec3::NEG_Y);
            let [r, g, b] = rgb(light.color);
            scene.lights.push(LightView {
                entity,
                direction,
                radiance: [
                    r * light.illuminance_lux,
                    g * light.illuminance_lux,
                    b * light.illuminance_lux,
                ],
                shadows: light.shadows,
            });
        }
        let mesh = if let Some(primitive) = world.get::<Primitive>(entity) {
            Some((Mesh::Primitive(primitive.shape), None))
        } else {
            world.get::<Model>(entity).and_then(|model| {
                (model.asset != AssetRef::NONE).then_some((Mesh::Model, Some(model.asset)))
            })
        };
        if let Some((mesh, asset)) = mesh {
            let material = world
                .get::<Material>(entity)
                .map_or_else(MaterialData::default, |m| MaterialData {
                    base_color: m.base_color.to_array(),
                    metallic: m.metallic,
                    roughness: m.roughness,
                    emissive: m.emissive.to_array(),
                });
            let model_path = match asset {
                Some(asset) => world.text(asset.text()).map(str::to_owned),
                None => None,
            };
            scene.draws.push(Draw {
                entity,
                world: placement,
                mesh,
                model_path,
                material,
            });
        }
    }
    Ok(scene)
}
