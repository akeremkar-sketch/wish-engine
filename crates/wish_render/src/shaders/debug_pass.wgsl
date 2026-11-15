#import draw

// The debug views: what the scene looks like to a program rather than to a
// person. Each view is one fragment entry point over the same vertex shader.

struct Debug {
    view_proj: mat4x4<f32>,
    // xyz: camera position in world space.
    camera: vec4<f32>,
    // x: near plane distance; y: far plane distance.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> debug: Debug;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> VertexOut {
    let world = draw.model * vec4<f32>(position, 1.0);
    var out: VertexOut;
    out.clip = debug.view_proj * world;
    out.world_position = world.xyz;
    out.normal = (draw.normal_matrix * vec4<f32>(normal, 0.0)).xyz;
    return out;
}

// The identity view: the scene draw's index plus one (0 is "nothing").
@fragment
fn fs_ids(in: VertexOut) -> @location(0) u32 {
    return u32(draw.params2.w + 0.5);
}

// The depth view: white at the near plane, black at the far plane, on a
// logarithmic scale so that both nearby and distant surfaces are readable.
@fragment
fn fs_depth(in: VertexOut) -> @location(0) vec4<f32> {
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
    return vec4<f32>(0.05, 0.06, 0.08, 1.0);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.92, 0.92, 0.82, 1.0);
}
