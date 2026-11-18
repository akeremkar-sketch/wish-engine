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
/// horizon, to the zenith. Shiny surfaces reflect it, and with
/// `Ambient.source` set to `Sky` it is also the scene's ambient light.
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
    /// How wide the sun's disc is in the sky, from edge to edge. 0 draws no
    /// disc. The disc sits where the scene's `DirectionalLight` shines from
    /// and has that light's colour; its edge is soft. The real sun is 0.53
    /// degrees wide; games usually draw it larger (2 to 6). Leave it at 0
    /// for a scene seen through an orthographic camera: such a camera
    /// looks at one point of the sky, so the disc would fill the
    /// background or not show at all.
    #[schema(unit = "deg", range = 0.0..=45.0, default = 0.0)]
    pub sun_size_deg: f32,
}

/// Where ambient light comes from.
#[schema]
pub enum AmbientSource {
    /// One colour, the same on every surface whichever way it faces:
    /// `Ambient.color` and `Ambient.intensity`.
    Color,
    /// The sky: a surface is lit by the part of the `Sky` it faces, so tops
    /// take the zenith's colour, sides the horizon's and undersides the
    /// ground's. `Ambient.color` and `Ambient.intensity` are not used.
    Sky,
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
    /// Where the light comes from: one colour (`Color`), or the sky's own
    /// colours by the direction a surface faces (`Sky`).
    pub source: AmbientSource,
    /// With `source: Sky`, how strongly the sky lights the scene: 1 is the
    /// light the sky's colours and luminance give by themselves, 0.5 half of
    /// it. Not used with `source: Color`.
    #[schema(range = 0.0..=10.0, default = 1.0)]
    pub sky_strength: f32,
}

/// Camera exposure: how bright the scene looks.
#[resource]
pub struct Exposure {
    /// Exposure value at ISO 100: 15 suits a sunny day, 10 a lit interior,
    /// 5 a night street.
    #[schema(range = -10.0..=30.0, default = 14.0)]
    pub ev100: f32,
}

/// How the picture is drawn: settings that change what a frame costs and how
/// fine it looks, never what happens in the game. A scene without this
/// resource is drawn with the defaults.
#[resource]
pub struct RenderSettings {
    /// Whether directional lights cast shadows at all. Off, no shadow map is
    /// drawn, whatever a light's own `shadows` says: the cheapest setting
    /// for a slow machine.
    #[schema(default = true)]
    pub shadows: bool,
    /// The side of the directional light's shadow map. A larger map gives
    /// sharper shadow edges and costs more to draw; the map covers
    /// everything drawn, so a large scene needs a larger map for the same
    /// sharpness.
    #[schema(unit = "px", range = 64..=4096, default = 2048)]
    pub shadow_map_size: u32,
    /// How jagged edges are smoothed in the lit picture. The in-game
    /// interface, sprites and particles are drawn afterwards and stay as
    /// sharp as they were made.
    pub antialiasing: Antialiasing,
}

/// How jagged edges are smoothed.
#[schema]
pub enum Antialiasing {
    /// Not at all: every pixel is one sample.
    Off,
    /// FXAA: one pass over the finished picture that finds edges by their
    /// contrast and blends along them. Cheap; softens fine detail a little.
    Fxaa,
}

/// Fog: far things fade into the colour of the horizon, and the sky near the
/// horizon hazes over with them. With `density` 0 (the default) there is no
/// fog.
///
/// The fog is thickest at `base_height` and thins out above it by
/// `height_falloff`; with a falloff of 0 it is the same at every height and
/// only distance counts. The sky is fogged like a surface 100 km away, so
/// fog without a height falloff covers the sky as well. Sprites, particles
/// and the in-game interface are not fogged.
#[resource]
pub struct Fog {
    /// The fog's colour as a tint of the sky's horizon colour. White is the
    /// horizon's own colour: far things then fade into the horizon without a
    /// seam.
    #[schema(default = Color::rgb(1.0, 1.0, 1.0))]
    pub tint: Color,
    /// How thick the fog is at `base_height`: the share of light lost per
    /// meter. 0 is no fog; 0.005 is a light haze (half of a thing shows
    /// through at 140 m), 0.05 a thick fog (half at 14 m).
    #[schema(unit = "1/m", range = 0.0..=1.0, default = 0.0)]
    pub density: f32,
    /// How fast the fog thins out with height: at `1 / height_falloff`
    /// meters above `base_height` it is down to about a third. 0 keeps it
    /// the same at every height.
    #[schema(unit = "1/m", range = 0.0..=10.0, default = 0.0)]
    pub height_falloff: f32,
    /// The height (world Y) at which the fog is as thick as `density` says.
    /// Below it the fog gets thicker when `height_falloff` is above 0.
    #[schema(unit = "m", range = -100000.0..=100000.0, default = 0.0)]
    pub base_height: f32,
    /// How far from the camera the fog starts; nearer things are clear.
    #[schema(unit = "m", range = 0.0..=100000.0, default = 0.0)]
    pub start: f32,
    /// The most the fog can hide: 1 lets far things vanish completely, 0.8
    /// always leaves a fifth of them (and of the sky) showing.
    #[schema(range = 0.0..=1.0, default = 1.0)]
    pub max_opacity: f32,
}

/// Colour grading: the finishing touches on the lit picture, applied to the
/// sky and to surfaces before tone mapping. The defaults leave the picture
/// as it is. Sprites, particles and the in-game interface keep the colours
/// they were made with.
#[resource]
pub struct ColorGrading {
    /// The difference between dark and bright around middle grey. 1 leaves
    /// it as it is, above 1 is punchier, below 1 flatter (0 is plain grey).
    #[schema(range = 0.0..=2.0, default = 1.0)]
    pub contrast: f32,
    /// How colourful the picture is. 1 leaves it as it is, 0 is black and
    /// white, above 1 is more vivid.
    #[schema(range = 0.0..=2.0, default = 1.0)]
    pub saturation: f32,
    /// White balance between blue and orange. 0 leaves colours as they
    /// are, positive is warmer (towards orange), negative cooler (towards
    /// blue).
    #[schema(range = -1.0..=1.0, default = 0.0)]
    pub temperature: f32,
    /// White balance between green and magenta. 0 leaves colours as they
    /// are, positive is more magenta, negative more green.
    #[schema(range = -1.0..=1.0, default = 0.0)]
    pub tint: f32,
    /// How much the corners of the picture are darkened, like a lens does.
    /// 0 is not at all, 1 takes the corners to black.
    #[schema(range = 0.0..=1.0, default = 0.0)]
    pub vignette: f32,
}
