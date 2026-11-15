// What every pass of a frame shares. The layout must match
// `FrameUniform` in renderer.rs (every field is 16 bytes aligned).

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
};

@group(0) @binding(0) var<uniform> frame: Frame;

const PI: f32 = 3.141592653589793;
