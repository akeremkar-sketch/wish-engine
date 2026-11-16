// The in-game interface, over the finished picture: rectangles in clip space,
// each sampling the interface atlas (a white texel for plain colour, glyph
// coverage in alpha for text, pictures as they are) times its colour.
// Colours are linear; the picture's sRGB target encodes them.

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.colour = colour;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_sampler, in.uv) * in.colour;
}
