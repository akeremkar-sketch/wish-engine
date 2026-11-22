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

// 2D lighting and vision (design note 1.1-lights-2d §8). The functions
// above are what every picture without them uses, unchanged; these are
// used through pipelines of their own only when a scene asks for them.

// The light texture: the ambient plus every 2D light, one texel per pixel.
@group(2) @binding(0) var light_map: texture_2d<f32>;
// The vision mask: 1 where a vision sees, 0 where none does.
@group(2) @binding(1) var vision_mask: texture_2d<f32>;

struct Darkness {
    // Linear RGB, and how dark (0 to 1).
    colour: vec4<f32>,
};

@group(2) @binding(2) var<uniform> darkness: Darkness;

// The texel of a picture-sized texture under a fragment.
fn texel_at(size: vec2<u32>, position: vec4<f32>) -> vec2<u32> {
    let at = vec2<u32>(max(position.xy, vec2<f32>(0.0, 0.0)));
    return min(at, max(size, vec2<u32>(1u, 1u)) - vec2<u32>(1u, 1u));
}

@fragment
fn fs_lit(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled in uniform control flow, used or not.
    let texel = textureSample(picture, picture_sampler, in.uv);
    if in.disc > 0.5 {
        // Particles are not lit.
        let from_centre = length(in.uv * 2.0 - vec2<f32>(1.0, 1.0));
        let mask = 1.0 - smoothstep(0.6, 1.0, from_centre);
        return vec4<f32>(in.colour.rgb, in.colour.a * mask);
    }
    let light = textureLoad(light_map, texel_at(textureDimensions(light_map), in.clip), 0).rgb;
    let shown = texel * in.colour;
    return vec4<f32>(shown.rgb * light, shown.a);
}

struct FanOut {
    @builtin(position) clip: vec4<f32>,
    // From the light's (or the vision's) centre, in meters.
    @location(0) offset: vec2<f32>,
    // The radius and the soft edge, in meters (at least 1 mm each).
    @location(1) reach: vec2<f32>,
    @location(2) colour: vec4<f32>,
};

// One corner of a triangle of a light's (or a vision's) area, in the world.
@vertex
fn vs_fan(
    @location(0) position: vec3<f32>,
    @location(1) centre: vec2<f32>,
    @location(2) reach: vec2<f32>,
    @location(3) colour: vec4<f32>,
) -> FanOut {
    var out: FanOut;
    let clip = overlay.view_proj * vec4<f32>(position, 1.0);
    // Held at the middle depth: these passes have no depth buffer, and a
    // light's plane may lie anywhere between the camera's near and far.
    out.clip = vec4<f32>(clip.xy, 0.5 * clip.w, clip.w);
    out.offset = position.xy - centre;
    out.reach = reach;
    out.colour = colour;
    return out;
}

// A light: its colour, falling off smoothly to nothing at its radius.
@fragment
fn fs_light(in: FanOut) -> @location(0) vec4<f32> {
    let share = clamp(1.0 - length(in.offset) / in.reach.x, 0.0, 1.0);
    return vec4<f32>(in.colour.rgb * (share * share), 1.0);
}

// A vision: 1 inside, falling to 0 over the soft edge at the radius.
@fragment
fn fs_vision(in: FanOut) -> @location(0) vec4<f32> {
    let into_edge = (length(in.offset) - (in.reach.x - in.reach.y)) / in.reach.y;
    let t = clamp(into_edge, 0.0, 1.0);
    let seen = 1.0 - t * t * (3.0 - 2.0 * t);
    return vec4<f32>(seen, 0.0, 0.0, 1.0);
}

struct ScreenOut {
    @builtin(position) clip: vec4<f32>,
};

// One triangle over the whole picture.
@vertex
fn vs_screen(@builtin(vertex_index) vertex: u32) -> ScreenOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: ScreenOut;
    out.clip = vec4<f32>(corners[min(vertex, 2u)], 0.0, 1.0);
    return out;
}

// The darkness over what no vision sees.
@fragment
fn fs_darkness(in: ScreenOut) -> @location(0) vec4<f32> {
    let seen = textureLoad(vision_mask, texel_at(textureDimensions(vision_mask), in.clip), 0).r;
    return vec4<f32>(darkness.colour.rgb, darkness.colour.a * (1.0 - seen));
}
