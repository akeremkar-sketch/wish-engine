// Sprites and particles, over the finished 3D picture: world-space
// rectangles tested against the scene's depth (solid things in front hide
// them), sampling their picture times their colour. Each rectangle is one
// instance of a quad of six vertices: its four corners come worked out
// from the CPU, so the shader only picks the corner of the vertex. Plain
// colour and particles sample a white texel. Particles are soft discs:
// their texture coordinates are the rectangle's corners, turned into a
// round mask. Colours are linear; the picture's sRGB target encodes them.

struct Overlay {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> overlay: Overlay;
@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) disc: f32,
};

@vertex
fn vs_instance(
    @builtin(vertex_index) vertex: u32,
    @location(0) corner0: vec3<f32>,
    @location(1) corner1: vec3<f32>,
    @location(2) corner2: vec3<f32>,
    @location(3) corner3: vec3<f32>,
    // The picture's part: left, top, right, bottom (0 to 1), flips applied.
    @location(4) uv_rect: vec4<f32>,
    @location(5) colour: vec4<f32>,
    @location(6) disc: f32,
) -> VertexOut {
    // Two triangles: corners 0 1 2 and 0 2 3 (bottom left, bottom right,
    // top right, top left).
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = order[min(vertex, 5u)];
    var positions = array<vec3<f32>, 4>(corner0, corner1, corner2, corner3);
    // The picture's top is the rectangle's top.
    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(uv_rect.x, uv_rect.w),
        vec2<f32>(uv_rect.z, uv_rect.w),
        vec2<f32>(uv_rect.z, uv_rect.y),
        vec2<f32>(uv_rect.x, uv_rect.y),
    );
    var out: VertexOut;
    out.clip = overlay.view_proj * vec4<f32>(positions[corner], 1.0);
    out.uv = uvs[corner];
    out.colour = colour;
    out.disc = disc;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled in uniform control flow, used or not.
    let texel = textureSample(picture, picture_sampler, in.uv);
    if in.disc > 0.5 {
        let from_centre = length(in.uv * 2.0 - vec2<f32>(1.0, 1.0));
        let mask = 1.0 - smoothstep(0.6, 1.0, from_centre);
        return vec4<f32>(in.colour.rgb, in.colour.a * mask);
    }
    return texel * in.colour;
}
