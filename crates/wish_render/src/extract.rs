//! Reading the world into a [`RenderScene`]: everything the renderer needs,
//! copied out, so drawing never touches the simulation.
//!
//! Extraction only reads. Draws, lights and cameras come out in entity order,
//! so the same world always gives the same scene (and the same debug
//! images).

use std::collections::BTreeMap;

use wish_core::math::{Mat4, Vec3};
use wish_core::{AssetRef, Color, Diagnostic, Entity, Result};
use wish_ecs::World;
use wish_transform::GlobalTransform;

use crate::components::{
    AlphaMode, Ambient, AmbientSource, Bloom, Camera, ColorGrading, DirectionalLight, Exposure,
    Fog, Material, MaterialFile, Model, PointLight, Primitive, RenderSettings, Shape, Sky,
    SpotLight,
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

/// The texture slots of a material, in the order the shaders bind them: the
/// names of the `MaterialFile` fields.
pub const TEXTURE_SLOTS: [&str; 5] = [
    "base_color_texture",
    "metallic_roughness_texture",
    "normal_texture",
    "occlusion_texture",
    "emissive_texture",
];

/// Whether a texture slot holds colours (sRGB: base colour, emission) or
/// numbers (metal and roughness, normals, occlusion), by its place in
/// [`TEXTURE_SLOTS`]. The same file is decoded differently for the two.
pub const fn slot_is_colour(slot: usize) -> bool {
    slot == 0 || slot == 4
}

/// The rest of a surface's look: its textures and how they are laid on it.
/// The default is what a surface without any of it has: no textures, solid,
/// single-sided.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceData {
    /// The project paths of the textures, in [`TEXTURE_SLOTS`] order; `None`
    /// for a slot without one.
    pub textures: [Option<String>; 5],
    /// How strong the normal map is.
    pub normal_scale: f32,
    /// How much the occlusion texture darkens, 0 to 1.
    pub occlusion_strength: f32,
    /// How many times the textures repeat along u and v.
    pub uv_scale: [f32; 2],
    /// How far the textures are slid along u and v.
    pub uv_offset: [f32; 2],
    /// Alpha below this is a hole; 0 for a solid surface.
    pub alpha_cutoff: f32,
    /// Whether back faces are drawn too.
    pub double_sided: bool,
    /// Whether the surface is see-through: blended over what is behind it
    /// by its alpha (`AlphaMode::Blend`).
    pub blend: bool,
}

impl Default for SurfaceData {
    fn default() -> Self {
        Self {
            textures: [None, None, None, None, None],
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            uv_scale: [1.0, 1.0],
            uv_offset: [0.0, 0.0],
            alpha_cutoff: 0.0,
            double_sided: false,
            blend: false,
        }
    }
}

impl SurfaceData {
    /// Whether any slot names a texture.
    pub fn textured(&self) -> bool {
        self.textures.iter().any(Option::is_some)
    }
}

/// A whole look: what a material file says ([`MaterialFile`]), with its
/// texts resolved to paths and its numbers made safe for a shader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    /// The colour, metal, roughness and emission.
    pub material: MaterialData,
    /// The textures and how they are laid on the surface.
    pub surface: SurfaceData,
}

/// `value` within `low..=high`: the nearer bound when it is outside,
/// `fallback` when it is not a number. A look can be built by code as well as
/// read from a checked file; a shader is given only numbers it can compute
/// with.
fn clean(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_nan() {
        fallback
    } else {
        value.clamp(low, high)
    }
}

impl Look {
    /// The look a material file describes. `text_of` resolves the texture
    /// fields' handles to project paths; an empty path is no texture.
    pub fn of(material: &MaterialFile, text_of: &dyn Fn(AssetRef) -> Option<String>) -> Self {
        let slot = |reference: AssetRef| {
            if reference.is_none() {
                return None;
            }
            text_of(reference).filter(|path| !path.is_empty())
        };
        // A scale of 0 (or one that is not a number) would collapse the
        // picture to one texel: it is drawn once instead.
        let scale = |value: f32| {
            if value.is_finite() && value != 0.0 {
                value.clamp(-4096.0, 4096.0)
            } else {
                1.0
            }
        };
        let blend = material.alpha_mode == AlphaMode::Blend;
        let mut base_color = material
            .base_color
            .to_array()
            .map(|c| clean(c, 0.0, 1.0e4, 1.0));
        // A blended surface's alpha is how much of it covers what is
        // behind: at most all of it.
        if blend {
            base_color[3] = base_color[3].min(1.0);
        }
        Self {
            material: MaterialData {
                base_color,
                metallic: clean(material.metallic, 0.0, 1.0, 0.0),
                roughness: clean(material.roughness, 0.0, 1.0, 0.5),
                emissive: material
                    .emissive
                    .to_array()
                    .map(|c| clean(c, 0.0, 1.0e6, 0.0)),
            },
            surface: SurfaceData {
                textures: [
                    slot(material.base_color_texture),
                    slot(material.metallic_roughness_texture),
                    slot(material.normal_texture),
                    slot(material.occlusion_texture),
                    slot(material.emissive_texture),
                ],
                normal_scale: clean(material.normal_scale, 0.0, 8.0, 1.0),
                occlusion_strength: clean(material.occlusion_strength, 0.0, 1.0, 1.0),
                uv_scale: material.uv_scale.map(scale),
                uv_offset: material.uv_offset.map(|v| clean(v, -4096.0, 4096.0, 0.0)),
                alpha_cutoff: match material.alpha_mode {
                    AlphaMode::Opaque | AlphaMode::Blend => 0.0,
                    // Never 0, which means "no cutoff" to the shaders.
                    AlphaMode::Mask => clean(material.alpha_cutoff, 1e-4, 1.0, 0.5),
                },
                double_sided: material.double_sided,
                blend,
            },
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
    /// Its material's numbers: the entity's `Material`, or the defaults.
    pub material: MaterialData,
    /// The material file the entity's `Material` names (`Material.file`), if
    /// any. When the renderer has that file
    /// ([`crate::Renderer::set_material`]) its look replaces `material`; on
    /// a model it replaces every material of the model.
    pub material_file: Option<String>,
    /// For a model with an `AnimationPlayer`: the clip's name (empty: the
    /// first clip) and the time in it.
    pub animation: Option<(String, f32)>,
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
    /// For an orthographic camera, the height it shows in meters; `None` for
    /// a perspective one.
    pub ortho_height: Option<f32>,
}

impl CameraView {
    /// The projection for an image of `aspect` (width over height), for a
    /// right-handed view space: clip depth runs 0 to 1 and +Y is up, as wgpu
    /// expects.
    pub fn projection(&self, aspect: f32) -> Mat4 {
        match self.ortho_height {
            Some(height) => {
                let (half_h, half_w) = (height * 0.5, height * 0.5 * aspect);
                wish_core::math::camera::rh::proj::directx::orthographic(
                    -half_w, half_w, -half_h, half_h, self.near, self.far,
                )
            }
            None => wish_core::math::camera::rh::proj::directx::perspective(
                self.fov_y, aspect, self.near, self.far,
            ),
        }
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

/// A point or spot light ready to use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalLightView {
    /// The entity that owns it.
    pub entity: Entity,
    /// Where it is, in world space.
    pub position: Vec3,
    /// Linear RGB multiplied by the luminous intensity (candela).
    pub intensity: [f32; 3],
    /// Beyond this distance it lights nothing.
    pub range: f32,
    /// For a spot light: the direction it shines (unit vector) and the
    /// cosines of its inner and outer angles. `None` for a point light.
    pub spot: Option<(Vec3, f32, f32)>,
}

/// A physics collider, for the `colliders` debug view: its shape where
/// physics puts it (the entity's scale applied to the size, as physics does).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColliderView {
    /// The entity that owns it.
    pub entity: Entity,
    /// Its centre in world space.
    pub center: Vec3,
    /// Its orientation.
    pub rotation: wish_core::math::Quat,
    /// The shape.
    pub shape: wish_types::physics::ColliderShape,
    /// Its size in meters, the entity's scale applied.
    pub size: Vec3,
    /// Whether it is a sensor (a trigger, not a solid).
    pub sensor: bool,
    /// Whether it moves (a dynamic or kinematic body).
    pub moving: bool,
}

/// A sprite where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteView {
    /// The entity that owns it.
    pub entity: Entity,
    /// Its placement (local to world).
    pub world: Mat4,
    /// The sprite.
    pub sprite: wish_types::sprite::Sprite,
    /// Its picture's project path, if it has one.
    pub image: Option<String>,
}

/// A particle emitter where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterView {
    /// The entity that owns it.
    pub entity: Entity,
    /// Its placement (local to world).
    pub world: Mat4,
    /// The emitter.
    pub emitter: wish_types::particles::ParticleEmitter,
}

/// Text in an interface box, ready to lay out and draw.
#[derive(Clone, Debug, PartialEq)]
pub struct UiTextView {
    /// The text.
    pub text: String,
    /// The font's project path; empty for the engine's font.
    pub font: String,
    /// The font's height in interface pixels.
    pub size: f32,
    /// Linear RGBA.
    pub color: [f32; 4],
    /// How lines line up.
    pub align: wish_text::Align,
    /// Whether long lines wrap.
    pub wrap: bool,
}

/// One shown box of the in-game interface, where the layout put it.
#[derive(Clone, Debug, PartialEq)]
pub struct UiBoxView {
    /// The entity that owns it.
    pub entity: Entity,
    /// Left, top, width and height, in interface pixels.
    pub rect: [f32; 4],
    /// The same inside the padding: where text and pictures go.
    pub content: [f32; 4],
    /// Linear RGBA; alpha 0 draws nothing.
    pub background: [f32; 4],
    /// A picture: its project path and tint (linear RGBA).
    pub image: Option<(String, [f32; 4])>,
    /// Its text.
    pub text: Option<UiTextView>,
    /// For a button: whether the pointer is over it, and whether it is held
    /// down.
    pub button: Option<(bool, bool)>,
    /// Drawing order: higher is drawn later.
    pub order: u32,
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
    /// The width of the sun's disc in degrees; 0 for none.
    pub sun_size_deg: f32,
}

/// Everything drawn in one frame, copied out of the world.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    /// The cameras, in entity order.
    pub cameras: Vec<CameraView>,
    /// The directional lights, in entity order.
    pub lights: Vec<LightView>,
    /// The point and spot lights, in entity order.
    pub local_lights: Vec<LocalLightView>,
    /// The physics colliders, in entity order (for the `colliders` view).
    pub colliders: Vec<ColliderView>,
    /// What to draw, in entity order.
    pub draws: Vec<Draw>,
    /// The sky.
    pub sky: SkyData,
    /// Ambient light of one colour: linear RGB multiplied by its illuminance
    /// (lux). Not used when [`RenderScene::sky_light`] is set.
    pub ambient: [f32; 3],
    /// When the ambient light comes from the sky (`Ambient.source: Sky`):
    /// its strength, 1 for what the sky gives by itself.
    pub sky_light: Option<f32>,
    /// When the ambient light comes from the sky and `Ambient` sets a
    /// colour or an intensity of its own (other than the default), which
    /// are then not used: the first such field's name (`"color"`, then
    /// `"intensity"`), for the warning `ambient_color_unused`.
    pub ambient_unused: Option<&'static str>,
    /// The fog (the world's `Fog`, or the defaults: a faint low haze).
    pub fog: Fog,
    /// The colour grading (the world's `ColorGrading`, or the defaults: a
    /// gentle grade).
    pub grading: ColorGrading,
    /// The bloom (the world's `Bloom`, or the defaults: a faint glow).
    pub bloom: Bloom,
    /// Exposure value at ISO 100.
    pub ev100: f32,
    /// How the picture is drawn (the world's `RenderSettings`, or the
    /// defaults).
    pub settings: RenderSettings,
    /// The interface's boxes, in drawing order.
    pub ui: Vec<UiBoxView>,
    /// The interface's virtual screen.
    pub ui_settings: wish_types::ui::UiSettings,
    /// The fonts the interface's text uses (the world's).
    pub fonts: wish_text::Fonts,
    /// The sprites, in entity order.
    pub sprites: Vec<SpriteView>,
    /// The particle emitters, in entity order.
    pub emitters: Vec<EmitterView>,
    /// The simulation time shown (`Time.elapsed`), for particles.
    pub time: f64,
    /// What entities are called where the scene came from (their ids in the
    /// scene file), for warnings: "entity 'sun'" can be acted on, "entity
    /// 3v0" cannot. [`extract`] leaves it empty; whoever knows the names
    /// fills it in (the control plane does). Nothing is drawn differently.
    pub names: BTreeMap<Entity, String>,
}

impl RenderScene {
    /// How a warning names `entity`: its name in quotes when
    /// [`RenderScene::names`] has one, `#<id>` otherwise (the way the tools
    /// write an entity that has no name).
    pub fn name_of(&self, entity: Entity) -> String {
        match self.names.get(&entity) {
            Some(name) => format!("'{name}'"),
            None => format!("#{entity}"),
        }
    }
}

fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_array();
    [r, g, b]
}

/// With `source: Sky`, the first of `Ambient`'s own colour and intensity
/// that is not its default: set, and not used. (Before the look lock the
/// default source was `Color`, so a scene that sets only these fields now
/// changes nothing; the warning says so.)
fn ambient_unused(ambient: &Ambient) -> Option<&'static str> {
    let default = Ambient::default();
    if ambient.source != AmbientSource::Sky {
        None
    } else if ambient.color != default.color {
        Some("color")
    } else if ambient.intensity != default.intensity {
        Some("intensity")
    } else {
        None
    }
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
    let ambient = world.resource::<Ambient>().copied().unwrap_or_default();
    let mut scene = RenderScene {
        cameras: Vec::new(),
        lights: Vec::new(),
        local_lights: Vec::new(),
        colliders: Vec::new(),
        draws: Vec::new(),
        sky: {
            let sky = world.resource::<Sky>().copied().unwrap_or_default();
            SkyData {
                zenith: rgb(sky.zenith),
                horizon: rgb(sky.horizon),
                ground: rgb(sky.ground),
                luminance: sky.luminance,
                sun_size_deg: sky.sun_size_deg,
            }
        },
        ambient: {
            let [r, g, b] = rgb(ambient.color);
            [
                r * ambient.intensity,
                g * ambient.intensity,
                b * ambient.intensity,
            ]
        },
        sky_light: (ambient.source == AmbientSource::Sky).then_some(ambient.sky_strength),
        ambient_unused: ambient_unused(&ambient),
        fog: world.resource::<Fog>().copied().unwrap_or_default(),
        grading: world
            .resource::<ColorGrading>()
            .copied()
            .unwrap_or_default(),
        bloom: world.resource::<Bloom>().copied().unwrap_or_default(),
        ev100: world
            .resource::<Exposure>()
            .copied()
            .unwrap_or_default()
            .ev100,
        settings: world
            .resource::<RenderSettings>()
            .copied()
            .unwrap_or_default(),
        ui: Vec::new(),
        ui_settings: world
            .resource::<wish_types::ui::UiSettings>()
            .copied()
            .unwrap_or_default(),
        fonts: world
            .extension::<wish_text::Fonts>()
            .cloned()
            .unwrap_or_default(),
        sprites: Vec::new(),
        emitters: Vec::new(),
        time: wish_particles::now(world),
        names: BTreeMap::new(),
    };
    let global = |world: &World, entity: Entity| {
        world
            .get::<GlobalTransform>(entity)
            .map_or(Mat4::IDENTITY, |global| global.matrix)
    };

    let entities: Vec<Entity> = world.entities().collect();
    for entity in entities {
        if let Some(view) = ui_box(world, entity) {
            scene.ui.push(view);
        }
        let placement = global(world, entity);
        if finite_matrix(&placement) {
            if let Some(sprite) = world.get::<wish_types::sprite::Sprite>(entity).copied() {
                let image = world
                    .text(sprite.image.text())
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned);
                scene.sprites.push(SpriteView {
                    entity,
                    world: placement,
                    sprite,
                    image,
                });
            }
            if let Some(emitter) = world
                .get::<wish_types::particles::ParticleEmitter>(entity)
                .copied()
            {
                scene.emitters.push(EmitterView {
                    entity,
                    world: placement,
                    emitter,
                });
            }
        }
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
                ortho_height: (camera.projection == crate::components::Projection::Orthographic)
                    .then(|| {
                        if camera.ortho_height.is_finite() {
                            camera.ortho_height.max(0.001)
                        } else {
                            10.0
                        }
                    }),
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
        // Lumens over the whole sphere give the intensity in candela; a spot
        // light keeps the same intensity whatever its cone (design note M6 §5).
        let candela = |color: Color, lumens: f32| {
            let [r, g, b] = rgb(color);
            let intensity = lumens.max(0.0) / (4.0 * std::f32::consts::PI);
            [r * intensity, g * intensity, b * intensity]
        };
        let position = placement.w_axis.truncate();
        if let Some(light) = world.get::<PointLight>(entity).copied()
            && position.is_finite()
        {
            scene.local_lights.push(LocalLightView {
                entity,
                position,
                intensity: candela(light.color, light.intensity_lm),
                range: light.range.max(0.01),
                spot: None,
            });
        }
        if let Some(light) = world.get::<SpotLight>(entity).copied()
            && position.is_finite()
        {
            let direction = placement
                .transform_vector3(Vec3::NEG_Z)
                .try_normalize()
                .unwrap_or(Vec3::NEG_Y);
            let outer = light.outer_angle_deg.clamp(0.1, 89.9).to_radians();
            let inner = light
                .inner_angle_deg
                .clamp(0.0, 89.0)
                .to_radians()
                .min(outer);
            scene.local_lights.push(LocalLightView {
                entity,
                position,
                intensity: candela(light.color, light.intensity_lm),
                range: light.range.max(0.01),
                spot: Some((direction, inner.cos(), outer.cos())),
            });
        }
        if let Some(collider) = world.get::<wish_types::physics::Collider>(entity).copied() {
            let (scale, rotation, translation) = placement.to_scale_rotation_translation();
            if translation.is_finite() && rotation.is_finite() {
                let moving = world
                    .get::<wish_types::physics::RigidBody>(entity)
                    .is_some_and(|body| body.kind != wish_types::physics::BodyKind::Fixed);
                scene.colliders.push(ColliderView {
                    entity,
                    center: placement.transform_point3(collider.offset),
                    rotation: rotation.normalize(),
                    shape: collider.shape,
                    size: collider.size * scale.abs(),
                    sensor: collider.sensor,
                    moving,
                });
            }
        }
        let mesh = if let Some(primitive) = world.get::<Primitive>(entity) {
            Some((Mesh::Primitive(primitive.shape), None))
        } else {
            world.get::<Model>(entity).and_then(|model| {
                (model.asset != AssetRef::NONE).then_some((Mesh::Model, Some(model.asset)))
            })
        };
        if let Some((mesh, asset)) = mesh {
            let component = world.get::<Material>(entity);
            let material = component.map_or_else(MaterialData::default, |m| MaterialData {
                base_color: m.base_color.to_array(),
                metallic: m.metallic,
                roughness: m.roughness,
                emissive: m.emissive.to_array(),
            });
            let material_file = component
                .filter(|m| !m.file.is_none())
                .and_then(|m| world.text(m.file.text()))
                .filter(|path| !path.is_empty())
                .map(str::to_owned);
            let model_path = match asset {
                Some(asset) => world.text(asset.text()).map(str::to_owned),
                None => None,
            };
            let animation = world
                .get::<wish_types::anim::AnimationPlayer>(entity)
                .map(|player| {
                    let clip = world.text(player.clip).unwrap_or_default().to_owned();
                    let time = if player.time.is_finite() {
                        player.time
                    } else {
                        0.0
                    };
                    (clip, time)
                });
            scene.draws.push(Draw {
                entity,
                world: placement,
                mesh,
                model_path,
                material,
                material_file,
                animation,
            });
        }
    }
    scene.ui.sort_by_key(|view| view.order);
    Ok(scene)
}

/// The interface box of `entity`, if it is one and the layout shows it.
fn ui_box(world: &World, entity: Entity) -> Option<UiBoxView> {
    use wish_types::ui::{UiButton, UiImage, UiInteraction, UiLayout, UiNode, UiText, UiTextAlign};
    let node = world.get::<UiNode>(entity)?;
    let layout = world.get::<UiLayout>(entity)?;
    let finite = layout
        .position
        .iter()
        .chain(&layout.size)
        .all(|v| v.is_finite());
    if !layout.shown || !finite {
        return None;
    }
    let [left, top] = layout.position;
    let [width, height] = layout.size;
    let pad = node
        .padding
        .map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let content = [
        left + pad[3],
        top + pad[0],
        (width - pad[1] - pad[3]).max(0.0),
        (height - pad[0] - pad[2]).max(0.0),
    ];
    let image = world.get::<UiImage>(entity).and_then(|image| {
        let path = world.text(image.image.text())?;
        (!path.is_empty()).then(|| (path.to_owned(), image.tint.to_array()))
    });
    let text = world.get::<UiText>(entity).map(|text| UiTextView {
        text: world.text(text.text).unwrap_or_default().to_owned(),
        font: world.text(text.font.text()).unwrap_or_default().to_owned(),
        size: text.size,
        color: text.color.to_array(),
        align: match text.align {
            UiTextAlign::Left => wish_text::Align::Left,
            UiTextAlign::Center => wish_text::Align::Center,
            UiTextAlign::Right => wish_text::Align::Right,
        },
        wrap: text.wrap,
    });
    let button = world.get::<UiButton>(entity).map(|button| {
        let state = world
            .get::<UiInteraction>(entity)
            .copied()
            .unwrap_or_default();
        (
            button.enabled && state.hovered,
            button.enabled && state.pressed,
        )
    });
    Some(UiBoxView {
        entity,
        rect: [left, top, width, height],
        content,
        background: node.background.to_array(),
        image,
        text,
        button,
        order: layout.order,
    })
}
