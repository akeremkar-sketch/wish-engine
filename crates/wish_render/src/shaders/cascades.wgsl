// Cascaded shadow maps: the numbers a frame's cascades come to
// (`shadow.rs`), and what they say about a point of the world. The lit pass
// and the `shadow_cascades` view both ask here, so the view shows what the
// picture does.
//
// The cascades are tiles of one depth texture (an atlas), the finest first.
// Each is an orthographic view from the light, so a position's place in a
// map needs no perspective divide; every reciprocal a function here needs is
// worked out on the CPU, and nothing divides.
//
// The layout must match `CASCADES_UNIFORM_SIZE` and `write_plan` in
// `renderer/shadow.rs` (every field is 16 bytes aligned).

struct Cascades {
    // World space to each cascade's clip space.
    view_proj: array<mat4x4<f32>, 4>,
    // Each cascade's tile in the atlas. xy: its corner; zw: its size, as
    // shares of the atlas.
    rect: array<vec4<f32>, 4>,
    // x: depth bias in clip-space depth; y: how far a lookup is pushed off
    // the surface along its normal (m); z: 1 / the part of the clip-space
    // depth over which the cascade fades out at its far side.
    bias: array<vec4<f32>, 4>,
    // The view depth of a world position p is dot(xyz, p) + w.
    depth_axis: vec4<f32>,
    // x: how many cascades there are (0: none, the frame has one shadow map
    // or no shadows); y: the side of a tile in texels; z: 1 / the band at a
    // tile's border over which a cascade fades out, in texels; w: 1 when the
    // one "cascade" stands for the single map (only the view is told).
    info: vec4<f32>,
    // x: the view depth at which shadows are gone (m); y: 1 / the depth over
    // which they fade out before it; zw: one texel of the atlas, as shares
    // of its width and height.
    fade: vec4<f32>,
};

// Where a cascade's map says to look for a point, and how much the cascade
// counts there. xy: the place in the atlas; z: the depth to compare with;
// w: the cascade's weight, 1 well inside its map, falling to 0 over the band
// at its border and at its far side, 0 outside. A cascade has weight only
// where the 3x3 filter's footprint is inside its own tile, so a read with
// weight never touches the next tile.
fn cascade_lookup(
    view_proj: mat4x4<f32>,
    rect: vec4<f32>,
    bias: vec4<f32>,
    info: vec4<f32>,
    world_position: vec3<f32>,
    normal: vec3<f32>,
) -> vec4<f32> {
    let offset_position = world_position + normal * bias.y;
    let clip = view_proj * vec4<f32>(offset_position, 1.0);
    let uv = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    // Texels from the nearest border of the tile (negative outside it).
    let border = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)) * info.y;
    let across = clamp((border - 2.0) * info.z, 0.0, 1.0);
    let along = clamp((1.0 - clip.z) * bias.z, 0.0, 1.0);
    let in_front = select(0.0, 1.0, clip.z >= 0.0);
    return vec4<f32>(rect.xy + uv * rect.zw, clip.z - bias.x, min(across, along) * in_front);
}

// How much of its shadow a point keeps at its depth in front of the camera:
// 1 up to where the fade starts, 0 from the shadow distance on.
fn cascade_fade(depth_axis: vec4<f32>, fade: vec4<f32>, world_position: vec3<f32>) -> f32 {
    let depth = dot(depth_axis.xyz, world_position) + depth_axis.w;
    return clamp((fade.x - depth) * fade.y, 0.0, 1.0);
}
