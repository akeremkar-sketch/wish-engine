//! What scenes say about how things look: cameras, lights, shapes,
//! materials, models, and the sky.
//!
//! Colours are linear RGB, not sRGB. Units: meters, degrees where a name
//! says so (`fov_y_deg`), lux for illuminance.

use wish_core::math::Vec3;
use wish_core::{AssetRef, Color};
use wish_schema::{component, resource, schema};

/// The luminance of an emissive value of 1, in cd/m².
pub const EMISSIVE_NITS: f32 = 30000.0;

/// How a camera maps the world onto the picture.
#[schema]
pub enum Projection {
    /// Things farther away look smaller (3D).
    Perspective,
    /// Things look the same size at any distance (2D games, maps, plans):
    /// the picture shows `ortho_height` meters from top to bottom.
    Orthographic,
}

/// A camera: looks along its entity's -Z axis, +Y up. The aspect ratio comes
/// from the image being drawn.
#[component]
pub struct Camera {
    /// Vertical field of view.
    #[schema(unit = "deg", range = 1.0..=179.0, default = 60.0)]
    pub fov_y_deg: f32,
    /// Distance to the near clipping plane; anything closer is not drawn.
    #[schema(unit = "m", range = 0.0001..=1000.0, default = 0.1)]
    pub near: f32,
    /// Distance to the far clipping plane; anything farther is not drawn.
    /// Must be larger than `near`.
    #[schema(unit = "m", range = 0.001..=100000.0, default = 1000.0)]
    pub far: f32,
    /// Perspective (3D) or Orthographic (2D).
    pub projection: Projection,
    /// For an orthographic camera: how many meters the picture shows from
    /// top to bottom (the width follows from the picture's shape).
    #[schema(unit = "m", range = 0.001..=100000.0, default = 10.0)]
    pub ortho_height: f32,
}

/// A light as far away as the sun: parallel rays along its entity's -Z
/// axis.
#[component]
pub struct DirectionalLight {
    /// The light's colour.
    pub color: Color,
    /// How much light reaches a surface facing the light. Direct sunlight is
    /// about 100000 lux, an overcast sky about 10000.
    #[schema(unit = "lux", range = 0.0..=200000.0, default = 100000.0)]
    pub illuminance_lux: f32,
    /// Whether the light casts shadows.
    #[schema(default = true)]
    pub shadows: bool,
}

/// A light that shines in every direction from a point, like a bulb. Its
/// light fades with the square of the distance and is gone at `range`.
#[component]
pub struct PointLight {
    /// The light's colour.
    pub color: Color,
    /// How much light it gives off in all: a 60 W light bulb is about 800
    /// lumens, a candle about 12.
    #[schema(unit = "lm", range = 0.0..=10000000.0, default = 800.0)]
    pub intensity_lm: f32,
    /// Beyond this distance it lights nothing (it fades out smoothly before).
    #[schema(unit = "m", range = 0.01..=10000.0, default = 10.0)]
    pub range: f32,
}

/// A light that shines in a cone along its entity's -Z axis, like a
/// torch or a stage light.
#[component]
pub struct SpotLight {
    /// The light's colour.
    pub color: Color,
    /// How much light it would give off if it shone in every direction (so
    /// that narrowing the cone does not change how bright it is).
    #[schema(unit = "lm", range = 0.0..=10000000.0, default = 800.0)]
    pub intensity_lm: f32,
    /// Beyond this distance it lights nothing.
    #[schema(unit = "m", range = 0.01..=10000.0, default = 10.0)]
    pub range: f32,
    /// Inside this angle from the axis the light is at full strength.
    #[schema(unit = "deg", range = 0.0..=89.0, default = 25.0)]
    pub inner_angle_deg: f32,
    /// Beyond this angle from the axis there is no light; in between it
    /// fades.
    #[schema(unit = "deg", range = 0.1..=89.9, default = 35.0)]
    pub outer_angle_deg: f32,
}

/// The built-in shapes.
#[schema]
pub enum Shape {
    /// A 1 m cube centred on the entity.
    Cube,
    /// A sphere of 1 m diameter centred on the entity.
    Sphere,
    /// A 1 m by 1 m square in the entity's XZ plane, facing +Y.
    Plane,
}

/// A built-in shape, sized by the entity's `Transform` scale. Use it for
/// quick scenes and placeholders; real models come from glTF files
/// ([`Model`]).
#[component]
pub struct Primitive {
    /// Which shape.
    pub shape: Shape,
}

/// How a surface looks: the glTF metallic-roughness model. Without this
/// component an entity is drawn as plain white, rough plastic.
#[component]
pub struct Material {
    /// The colour of the surface (for metals, of its reflection).
    pub base_color: Color,
    /// 0 for plastic, wood and stone; 1 for bare metal.
    #[schema(range = 0.0..=1.0, default = 0.0)]
    pub metallic: f32,
    /// 0 is a mirror, 1 is completely rough.
    #[schema(range = 0.0..=1.0, default = 0.5)]
    pub roughness: f32,
    /// Light the surface gives off by itself, linear RGB. 1 is about as
    /// bright as a white surface in full default sunlight
    /// ([`EMISSIVE_NITS`] cd/m²).
    pub emissive: Vec3,
}

/// A model from a glTF or GLB file. The whole model, including its internal
/// hierarchy, is one entity here; its nodes are not separate entities.
#[component]
pub struct Model {
    /// The file, e.g. `assets/models/crate.glb`.
    pub asset: AssetRef,
}

/// The sky behind everything: a gradient from the ground, up through the
/// horizon, to the zenith. It also lights the scene faintly.
#[resource]
pub struct Sky {
    /// The colour straight up.
    #[schema(default = Color::rgb(0.18, 0.36, 0.75))]
    pub zenith: Color,
    /// The colour at the horizon.
    #[schema(default = Color::rgb(0.65, 0.75, 0.85))]
    pub horizon: Color,
    /// The colour straight down.
    #[schema(default = Color::rgb(0.25, 0.22, 0.20))]
    pub ground: Color,
    /// How bright a sky colour of 1 is. A clear daytime sky near the
    /// horizon is around 20000 cd/m².
    #[schema(unit = "cd/m2", range = 0.0..=100000.0, default = 20000.0)]
    pub luminance: f32,
}

/// Light that comes from everywhere, so that shadows are not pitch black.
#[resource]
pub struct Ambient {
    /// The light's colour.
    #[schema(default = Color::rgb(0.5, 0.6, 0.8))]
    pub color: Color,
    /// How much light it adds to every surface. Open shade on a sunny day
    /// gets around 20000 lux.
    #[schema(unit = "lux", range = 0.0..=200000.0, default = 20000.0)]
    pub intensity: f32,
}

/// Camera exposure: how bright the scene looks.
#[resource]
pub struct Exposure {
    /// Exposure value at ISO 100: 15 suits a sunny day, 10 a lit interior,
    /// 5 a night street.
    #[schema(range = -10.0..=30.0, default = 14.0)]
    pub ev100: f32,
}
