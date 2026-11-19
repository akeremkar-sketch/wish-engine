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

/// How a surface looks: the glTF metallic-roughness model. On a `Primitive`
/// it is the shape's look; without this component a shape is plain white,
/// rough plastic.
///
/// The four numbers here are a plain look, one colour all over. For
/// textures, cut-outs and the rest, `file` names a material file
/// (`*.mat.json`, see `MaterialFile`): the file is then the whole look, the
/// numbers here are not used, and every entity that names the file changes
/// when the file does.
///
/// On a `Model` entity only `file` counts: a model is drawn with the
/// materials of its own file unless its entity's `Material` names a
/// material file, which then replaces every material of the model. A
/// `Material` without `file` does nothing on a model.
#[component]
pub struct Material {
    /// The colour of the surface (for metals, of its reflection). Its alpha
    /// is how solid the surface is: 1 (the default) is solid; below 1 the
    /// surface is see-through, blended over what is behind it (0.5 half,
    /// 0 invisible): glass, water, a ghost, something fading out. A
    /// see-through surface casts no shadow.
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
    /// A material file (`*.mat.json`, for example
    /// `assets/materials/brick.mat.json`) to take the whole look from:
    /// textures, tiling, cut-outs. Empty (the default): the look is the
    /// numbers above.
    pub file: AssetRef,
}

/// How a material uses the alpha of its base colour (`base_color` times
/// `base_color_texture`).
#[schema]
pub enum AlphaMode {
    /// Alpha is not used: the surface is solid.
    Opaque,
    /// A cut-out: where alpha is below `alpha_cutoff` nothing is drawn (and
    /// no shadow is cast), everywhere else the surface is solid. For
    /// leaves, fences, grates.
    Mask,
    /// See-through: the surface is blended over what is behind it by its
    /// alpha (1 solid, 0.5 half, 0 invisible). For glass, water, smoke,
    /// ghosts. It casts no shadow; it is lit, shadowed and fogged like any
    /// surface. Overlapping see-through surfaces are drawn farthest first.
    Blend,
}

/// What a material file (`*.mat.json`) holds: one look, for every entity
/// that names the file in its `Material.file`. The file is
/// `{ "format": 1, "material": { ... } }` with any of these fields; the
/// others keep their defaults. `asset_create` with a `.mat.json` path makes
/// one.
///
/// It is the glTF metallic-roughness model: four numbers, and textures that
/// multiply them. Textures are PNG files of the project; picture
/// coordinates start at the picture's top left corner, and the picture
/// repeats outside 0 to 1. On a cube every face shows the whole picture
/// upright; on a plane the picture's top is at -Z and its left at -X;
/// around a sphere it wraps once (its top at the north pole); a model uses
/// its own texture coordinates.
#[schema]
pub struct MaterialFile {
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
    /// A picture of the surface's colour (PNG, sRGB), multiplied by
    /// `base_color`. Its alpha is the surface's alpha (see `alpha_mode`).
    /// Empty for none.
    pub base_color_texture: AssetRef,
    /// A picture of where the surface is metal and how rough it is (PNG,
    /// read as numbers, not colours): its blue is multiplied by `metallic`,
    /// its green by `roughness`. Set `metallic` to 1 for the picture's metal
    /// to show. Empty for none.
    pub metallic_roughness_texture: AssetRef,
    /// A normal map: a picture of small bumps (PNG, tangent space, green
    /// up, as glTF and Blender write it). The surface is lit as if it had
    /// the bumps; its outline stays flat. On a model it needs tangents in
    /// the model file. Empty for none.
    pub normal_texture: AssetRef,
    /// How strong the bumps of `normal_texture` are: 1 as the picture says,
    /// 0 flat, 2 twice as steep.
    #[schema(range = 0.0..=8.0, default = 1.0)]
    pub normal_scale: f32,
    /// A picture of how much ambient light reaches each point (PNG, its
    /// red: white is open, black is a crevice). It darkens only ambient
    /// light and the sky's reflection, never the light of a lamp or the
    /// sun. Empty for none.
    pub occlusion_texture: AssetRef,
    /// How much `occlusion_texture` darkens: 1 as the picture says, 0 not
    /// at all.
    #[schema(range = 0.0..=1.0, default = 1.0)]
    pub occlusion_strength: f32,
    /// A picture of the light the surface gives off (PNG, sRGB), multiplied
    /// by `emissive`: with `emissive` at its default of 0 it shows nothing,
    /// so set `emissive` to [1, 1, 1] or more. Empty for none.
    pub emissive_texture: AssetRef,
    /// How many times the textures repeat across the surface, along the
    /// picture's width and height: [4, 4] tiles a picture 4 by 4 on each
    /// face of a cube. Applies to every texture of the material.
    #[schema(default = [1.0, 1.0])]
    pub uv_scale: [f32; 2],
    /// Slides the textures across the surface, in picture widths and
    /// heights (after `uv_scale`): [0.5, 0] moves them by half a picture.
    pub uv_offset: [f32; 2],
    /// What the base colour's alpha does: nothing (`Opaque`), cut holes
    /// (`Mask`) or make the surface see-through (`Blend`).
    pub alpha_mode: AlphaMode,
    /// With `alpha_mode: Mask`: alpha below this is a hole.
    #[schema(range = 0.0..=1.0, default = 0.5)]
    pub alpha_cutoff: f32,
    /// Whether the surface's back is drawn too. Off (the default), a
    /// surface seen from behind is not there: right for closed shapes,
    /// wrong for a plane seen from below or a cut-out seen from inside.
    pub double_sided: bool,
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
    /// How wide the sun's disc is in the sky, from edge to edge (3 by
    /// default). 0 draws no disc. The disc sits where the scene's
    /// `DirectionalLight` shines from and has that light's colour; its edge
    /// is soft; a scene without such a light has no disc. The real sun is
    /// 0.53 degrees wide; games usually draw it larger (2 to 6). An
    /// orthographic camera never shows the disc: such a camera looks at one
    /// point of the sky, so the disc would fill the background or not show
    /// at all.
    #[schema(unit = "deg", range = 0.0..=45.0, default = 3.0)]
    pub sun_size_deg: f32,
}

/// Where ambient light comes from.
#[schema]
pub enum AmbientSource {
    /// One colour, the same on every surface whichever way it faces:
    /// `Ambient.color` and `Ambient.intensity`.
    Color,
    /// The sky (the default of `Ambient.source`): a surface is lit by the
    /// part of the `Sky` it faces, so tops take the zenith's colour, sides
    /// the horizon's and undersides the ground's. `Ambient.color` and
    /// `Ambient.intensity` are not used.
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
    /// Where the light comes from: the sky's own colours by the direction a
    /// surface faces (`Sky`, the default), or one colour (`Color`).
    #[schema(default = AmbientSource::Sky)]
    pub source: AmbientSource,
    /// With `source: Sky`, how strongly the sky lights the scene: 1 is the
    /// light the sky's colours and luminance give by themselves, 0.5 half of
    /// it. The default of 0.3 suits the default sky, which is about three
    /// times as bright as a real clear one. Not used with `source: Color`.
    #[schema(range = 0.0..=10.0, default = 0.3)]
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
/// resource is drawn with the defaults: shadows from four cascades to 100 m,
/// and edges smoothed with FXAA.
#[resource]
pub struct RenderSettings {
    /// Whether directional lights cast shadows at all. Off, no shadow map is
    /// drawn, whatever a light's own `shadows` says: the cheapest setting
    /// for a slow machine.
    #[schema(default = true)]
    pub shadows: bool,
    /// The side of the directional light's shadow map. A larger map gives
    /// sharper shadow edges and costs more to draw. With one map
    /// (`shadow_cascades` 1) it covers everything drawn, so a large scene
    /// needs a larger map for the same sharpness; with cascades each of
    /// them is a map of this size.
    #[schema(unit = "px", range = 64..=4096, default = 2048)]
    pub shadow_map_size: u32,
    /// How many shadow maps the directional light has. 1 is one map fitted
    /// around everything that is drawn: fine for a room, blurry in a large
    /// level, where one far object makes every shadow coarse. 2 to 4 are
    /// cascades: maps fitted to slices of what the camera sees, a small
    /// sharp one for what is near and larger ones for what is farther, up
    /// to `shadow_distance`. Shadows near the camera are then sharp
    /// whatever the scene's size. Each cascade is drawn as a map of its
    /// own, so more of them cost more; 4 (the default) suits an open level
    /// seen from the ground, 2 a small level or a camera that looks down
    /// from above.
    #[schema(range = 1..=4, default = 4)]
    pub shadow_cascades: u32,
    /// With `shadow_cascades` 2 or more: how far from the camera shadows
    /// are drawn, measured along the direction the camera looks. They fade
    /// out over the last tenth of it; beyond it everything is lit. A
    /// shorter distance gives sharper shadows (the same maps cover less),
    /// a longer one shadows on far things. Never more than the camera's
    /// `far`. Not used with one map, which covers everything.
    #[schema(unit = "m", range = 0.1..=10000.0, default = 100.0)]
    pub shadow_distance: f32,
    /// How jagged edges are smoothed in the lit picture: with FXAA by
    /// default. The in-game interface, sprites and particles are drawn
    /// afterwards and stay as sharp as they were made. `Off` keeps every
    /// pixel of a lit surface as it was drawn (pixel art on a textured
    /// shape).
    #[schema(default = Antialiasing::Fxaa)]
    pub antialiasing: Antialiasing,
}

/// How jagged edges are smoothed.
#[schema]
pub enum Antialiasing {
    /// Not at all: every pixel is one sample.
    Off,
    /// FXAA (the default of `RenderSettings.antialiasing`): one pass over
    /// the finished picture that finds edges by their contrast and blends
    /// along them. Cheap; softens fine detail a little.
    Fxaa,
}

/// Fog: far things fade into the colour of the horizon, and the sky near the
/// horizon hazes over with them. The defaults are a light haze that lies low
/// (half of a thing at ground level shows through at 460 m, and it thins out
/// above 20 m); with `density` 0 there is no fog.
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
    /// meter. 0 is no fog; 0.0015 (the default) is a faint haze, 0.005 a
    /// light one (half of a thing shows through at 140 m), 0.05 a thick fog
    /// (half at 14 m).
    #[schema(unit = "1/m", range = 0.0..=1.0, default = 0.0015)]
    pub density: f32,
    /// How fast the fog thins out with height: at `1 / height_falloff`
    /// meters above `base_height` it is down to about a third (by default at
    /// 20 m). 0 keeps it the same at every height.
    #[schema(unit = "1/m", range = 0.0..=10.0, default = 0.05)]
    pub height_falloff: f32,
    /// The height (world Y) at which the fog is as thick as `density` says.
    /// Below it the fog gets thicker when `height_falloff` is above 0.
    #[schema(unit = "m", range = -100000.0..=100000.0, default = 0.0)]
    pub base_height: f32,
    /// How far from the camera the fog starts; nearer things are clear.
    #[schema(unit = "m", range = 0.0..=100000.0, default = 0.0)]
    pub start: f32,
    /// The most the fog can hide: 1 lets far things vanish completely, 0.9
    /// (the default) always leaves a tenth of them (and of the sky) showing.
    #[schema(range = 0.0..=1.0, default = 0.9)]
    pub max_opacity: f32,
}

/// Colour grading: the finishing touches on the lit picture, applied to the
/// sky and to surfaces before tone mapping. The defaults are a gentle grade:
/// a little more contrast and colour, slightly warm, corners a little
/// darker. Sprites, particles and the in-game interface keep the colours
/// they were made with.
#[resource]
pub struct ColorGrading {
    /// The difference between dark and bright around middle grey. 1 leaves
    /// it as it is, above 1 is punchier (1.05 by default), below 1 flatter
    /// (0 is plain grey).
    #[schema(range = 0.0..=2.0, default = 1.05)]
    pub contrast: f32,
    /// How colourful the picture is. 1 leaves it as it is, 0 is black and
    /// white, above 1 is more vivid (1.1 by default).
    #[schema(range = 0.0..=2.0, default = 1.1)]
    pub saturation: f32,
    /// White balance between blue and orange. 0 leaves colours as they
    /// are, positive is warmer (towards orange; 0.03 by default), negative
    /// cooler (towards blue).
    #[schema(range = -1.0..=1.0, default = 0.03)]
    pub temperature: f32,
    /// White balance between green and magenta. 0 leaves colours as they
    /// are, positive is more magenta, negative more green.
    #[schema(range = -1.0..=1.0, default = 0.0)]
    pub tint: f32,
    /// How much the corners of the picture are darkened, like a lens does.
    /// 0 is not at all, 0.1 (the default) faintly, 1 takes the corners to
    /// black.
    #[schema(range = 0.0..=1.0, default = 0.1)]
    pub vignette: f32,
}

/// Bloom: bright light spreads softly around itself, as it does in a
/// camera's lens and in the eye, so a lamp, the sun's disc or a strong
/// highlight glows instead of ending at a hard edge. The default is a glow
/// only bright lights show; with `strength` 0 there is no bloom.
///
/// The light is moved, not added: a scene with nothing brighter than white
/// keeps its brightness and hardly changes (a small light much brighter
/// than white loses a little more than its glow gives back). What glows is
/// what is much brighter than white before the picture is finished: a
/// `Material` with `emissive` above 1, the sun's disc (`Sky.sun_size_deg`),
/// the highlight of a strong light on a shiny surface. A light that is only
/// a pixel or two wide in the picture hardly glows: single very bright
/// pixels are held back so that they do not flicker. Fog is in the light
/// that blooms, and the glow is graded with the rest of the picture.
/// Sprites, particles and the in-game interface are drawn afterwards: they
/// do not glow and nothing glows over them (particles that glow are planned
/// for a later release).
#[resource]
pub struct Bloom {
    /// How much of the light is spread around: 0 is no bloom, 0.05 (the
    /// default) a glow only bright lights show, 0.15 a clear one, 0.4 a
    /// dream sequence.
    #[schema(range = 0.0..=1.0, default = 0.05)]
    pub strength: f32,
    /// How far the glow reaches. 0 keeps it close around the light, 0.5
    /// falls off evenly (the default of 0.4 is a little closer), 1 lets it
    /// reach a third of the picture's height like a haze. The reach is a
    /// share of the picture, so the look is much the same at every picture
    /// size.
    #[schema(range = 0.0..=1.0, default = 0.4)]
    pub spread: f32,
    /// Only light brighter than this blooms, where 1 is the brightness of a
    /// white surface in the finished picture. 0 (the default) lets all
    /// light bloom, which is how a lens behaves. With a threshold, light
    /// dimmer than half of it is left perfectly sharp and the bloom sets in
    /// softly from there, so 2 or more keeps everything that is not
    /// brighter than white sharp, for a stylised look (raise `strength`
    /// with it: less light is left to spread).
    #[schema(range = 0.0..=64.0, default = 0.0)]
    pub threshold: f32,
}
