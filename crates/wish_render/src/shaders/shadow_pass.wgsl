#import draw

// The shadow pass: every surface drawn from the light, depth only.

struct Light {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> light: Light;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return light.view_proj * draw.model * vec4<f32>(position, 1.0);
}
