// Sprites and particles, over the finished 3D picture: world-space
// rectangles tested against the scene's depth (solid things in front hide
// them), sampling the atlas times their colour. Particles are soft discs:
// their texture coordinates are the rectangle's corners, turned into a round
// mask. Colours are linear; the picture's sRGB target encodes them.

struct Overlay {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> overlay: Overlay;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) disc: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) disc: f32,
) -> VertexOut {
    var out: VertexOut;
    out.clip = overlay.view_proj * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.colour = colour;
    out.disc = disc;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled in uniform control flow, used or not.
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    if in.disc > 0.5 {
        let from_centre = length(in.uv * 2.0 - vec2<f32>(1.0, 1.0));
        let mask = 1.0 - smoothstep(0.6, 1.0, from_centre);
        return vec4<f32>(in.colour.rgb, in.colour.a * mask);
    }
    return texel * in.colour;
}
