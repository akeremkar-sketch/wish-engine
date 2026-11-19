#import draw
#import cascades

// The debug views: what the scene looks like to a program rather than to a
// person. Each view is one fragment entry point over the same vertex shader.
// Like the lit pass they cull back faces of single-sided materials (in the
// pipeline) and cut out alpha-masked pixels, so that what they say is hidden
// is hidden in the picture too.

struct Debug {
    view_proj: mat4x4<f32>,
    // xyz: camera position in world space.
    camera: vec4<f32>,
    // x: near plane distance; y: far plane distance.
    params: vec4<f32>,
    // The shadow maps of the lit picture of this view, for the
    // `shadow_cascades` view (a count of 0 in every other view).
    cascades: Cascades,
};

@group(0) @binding(0) var<uniform> debug: Debug;
@group(2) @binding(0) var base_color_texture: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VertexOut {
    let world = draw.model * vec4<f32>(position, 1.0);
    var out: VertexOut;
    out.clip = debug.view_proj * world;
    out.world_position = world.xyz;
    out.normal = (draw.normal_matrix * vec4<f32>(normal, 0.0)).xyz;
    out.uv = uv;
    return out;
}

// Pixels below the material's alpha cutoff are not drawn (alpha mode MASK),
// the same test as in the lit pass.
fn cut_out(uv: vec2<f32>) {
    let alpha = textureSample(base_color_texture, material_sampler, uv).a * draw.base_color.a;
    if (draw.params2.z > 0.0 && alpha < draw.params2.z) {
        discard;
    }
}

// The identity view: the scene draw's index plus one (0 is "nothing").
@fragment
fn fs_ids(in: VertexOut) -> @location(0) u32 {
    cut_out(in.uv);
    return u32(draw.params2.w + 0.5);
}

// The depth view: white at the near plane, black at the far plane, on a
// logarithmic scale so that both nearby and distant surfaces are readable.
@fragment
fn fs_depth(in: VertexOut) -> @location(0) vec4<f32> {
    cut_out(in.uv);
    let near = max(debug.params.x, 1e-4);
    let far = max(debug.params.y, near * 1.001);
    let distance = max(length(in.world_position - debug.camera.xyz), near);
    let t = clamp(log2(distance / near) / log2(far / near), 0.0, 1.0);
    let value = 1.0 - t;
    return vec4<f32>(value, value, value, 1.0);
}

// The normals view: the surface normal in world space, each component mapped
// from [-1, 1] to [0, 1] (so +Y, up, is a light green).
@fragment
fn fs_normals(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    cut_out(in.uv);
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    return vec4<f32>(n * 0.5 + 0.5, 1.0);
}

// The wireframe view: a dark fill that hides what is behind it, and the
// edges on top.
@fragment
fn fs_fill(in: VertexOut) -> @location(0) vec4<f32> {
    cut_out(in.uv);
    return vec4<f32>(0.05, 0.06, 0.08, 1.0);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.92, 0.92, 0.82, 1.0);
}

// The colour of a cascade in the `shadow_cascades` view, the finest first:
// red, green, blue, yellow.
fn cascade_colour(index: i32) -> vec3<f32> {
    if (index == 0) {
        return vec3<f32>(0.90, 0.22, 0.18);
    }
    if (index == 1) {
        return vec3<f32>(0.20, 0.72, 0.26);
    }
    if (index == 2) {
        return vec3<f32>(0.22, 0.42, 0.95);
    }
    return vec3<f32>(0.95, 0.80, 0.15);
}

// The shadow cascades view: every surface in the colour of the shadow map
// its shadow is read from in the lit picture. The colours mix where two
// cascades blend, white is the single map, and grey is no shadow at all:
// beyond the shadow distance (with a fade before it, as in the picture),
// outside every map, or a frame without shadows. The cascades are walked
// exactly as `cascaded_visibility` walks them. Surfaces are shaded a little
// by the way they face, so that the picture still reads as the scene.
@fragment
fn fs_cascades(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    cut_out(in.uv);
    // The lit pass looks shadows up at the geometric normal as it is.
    let normal = normalize(in.normal);
    let kept = cascade_fade(debug.cascades.depth_axis, debug.cascades.fade, in.world_position);
    let count = i32(debug.cascades.info.x);
    var left = 1.0;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        let at = cascade_lookup(debug.cascades.view_proj[i], debug.cascades.rect[i],
            debug.cascades.bias[i], debug.cascades.info, in.world_position, normal);
        let own = select(cascade_colour(i), vec3<f32>(1.0), debug.cascades.info.w > 0.5);
        colour += left * at.w * own;
        left = left * (1.0 - at.w);
    }
    let unshadowed = vec3<f32>(0.35);
    let shown = mix(unshadowed, colour + left * unshadowed, kept);
    var facing = normal;
    if (!front) {
        facing = -facing;
    }
    let shade = 0.6 + 0.4 * max(dot(facing, normalize(vec3<f32>(0.3, 0.9, 0.3))), 0.0);
    return vec4<f32>(shown * shade, 1.0);
}
