// What every pass of a frame shares. The layout must match
// `FRAME_UNIFORM_SIZE` and `Frame` in renderer.rs (every field is 16 bytes
// aligned).

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz: camera position in world space.
    camera_position: vec4<f32>,
    // xyz: the direction the light travels; w: 1 when there is a light.
    light_direction: vec4<f32>,
    // rgb: light colour times illuminance (lux).
    light_radiance: vec4<f32>,
    // rgb: ambient colour times illuminance (lux).
    ambient: vec4<f32>,
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    // rgb: ground colour; w: sky luminance (cd/m2).
    sky_ground: vec4<f32>,
    // x: exposure multiplier; y: cd/m2 of an emissive value of 1.
    exposure: vec4<f32>,
    // Takes a world position into the light's clip space.
    light_view_proj: mat4x4<f32>,
    // x: 1 when the light casts shadows; y: depth bias; z: normal offset in
    // meters; w: 1 / shadow map size.
    shadow: vec4<f32>,
    // World space to the camera's space (for a pixel's depth).
    view: mat4x4<f32>,
    // x, y: tiles across and down; z: depth slices; w: point and spot lights.
    cluster: vec4<f32>,
    // x: near plane; y: far plane; z: slices per unit of ln(depth / near);
    // w: tile size in pixels.
    cluster_depth: vec4<f32>,
    // The look settings (`look.rs`). Each effect has a flag; when it is 0
    // the frame is computed as if the effect did not exist.
    // rgb: the sun disc's luminance (cd/m2); w: 1 when a disc is drawn.
    sun_radiance: vec4<f32>,
    // x: the angle from the sun's centre (radians) up to which the disc is at
    // full strength; y: 1 / the angle over which it then fades by e.
    sun_disc: vec4<f32>,
    // x: 1 when the ambient light comes from the sky; y: its strength.
    sky_light: vec4<f32>,
    // rgb: the fog's luminance (cd/m2); w: 1 when there is fog.
    fog_colour: vec4<f32>,
    // x: share of light lost per meter at the base height; y: how fast the
    // fog thins with height (1/m); z: the base height (world Y);
    // w: how far from the camera the fog starts (m).
    fog: vec4<f32>,
    // x: the most the fog hides (0 to 1); y: 1 when the camera is
    // orthographic (fog then goes by depth along the view axis).
    fog_limit: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

const PI: f32 = 3.141592653589793;

// How much of the light reaches `world_position` (0 in shadow, 1 lit):
// percentage-closer filtering over 3x3 texels. Outside the shadow map there
// is no shadow. `normal` pushes the lookup off the surface so that a surface
// does not shadow itself.
fn shadow_visibility(world_position: vec3<f32>, normal: vec3<f32>) -> f32 {
    let offset_position = world_position + normal * frame.shadow.z;
    let clip = frame.light_view_proj * vec4<f32>(offset_position, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let depth = ndc.z - frame.shadow.y;
    var lit = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let shifted = uv + vec2<f32>(f32(dx), f32(dy)) * frame.shadow.w;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, shifted, depth);
        }
    }
    lit = lit / 9.0;
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0
        && ndc.z >= 0.0 && ndc.z <= 1.0;
    // Shadows off, or outside the map: lit.
    return select(1.0, lit, inside && frame.shadow.x > 0.5);
}
