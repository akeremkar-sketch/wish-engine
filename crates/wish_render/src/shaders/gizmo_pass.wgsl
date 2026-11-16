// Lines over a debug view (colliders, where lights reach): world-space
// positions with their own colours, drawn over everything.

struct Debug {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> debug: Debug;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) colour: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    out.clip = debug.view_proj * vec4<f32>(position, 1.0);
    out.colour = colour;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.colour;
}
