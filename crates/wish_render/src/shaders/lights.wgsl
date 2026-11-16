// Point and spot lights, found through the clusters (design note M6 §5).
// The layouts must match `LIGHT_STRIDE` and `render` in renderer.rs.

#import common
#import brdf

struct LocalLight {
    // xyz: position in world space; w: range in meters.
    position_range: vec4<f32>,
    // rgb: colour times intensity (candela); w: 1 for a spot light.
    intensity: vec4<f32>,
    // xyz: the direction a spot light shines; w: cosine of its outer angle.
    direction: vec4<f32>,
    // x: cosine of the spot's inner angle.
    cone: vec4<f32>,
};

@group(0) @binding(3) var<storage, read> local_lights: array<LocalLight>;
// For each cluster: where its lights start in `cluster_lights`, and how many.
@group(0) @binding(4) var<storage, read> cluster_ranges: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read> cluster_lights: array<u32>;

// The cluster of a pixel at `pixel` (framebuffer coordinates) whose surface
// is `depth` meters in front of the camera.
fn cluster_index(pixel: vec2<f32>, depth: f32) -> u32 {
    let tiles = vec2<u32>(u32(frame.cluster.x), u32(frame.cluster.y));
    let slices = u32(frame.cluster.z);
    let tile = min(vec2<u32>(max(pixel, vec2<f32>(0.0)) / frame.cluster_depth.w), tiles - vec2<u32>(1u));
    var slice = 0u;
    if (depth > frame.cluster_depth.x) {
        slice = min(u32(log(depth / frame.cluster_depth.x) * frame.cluster_depth.z), slices - 1u);
    }
    return (slice * tiles.y + tile.y) * tiles.x + tile.x;
}

// How far a light reaches: the inverse square law, smoothly cut off at the
// light's range so that it lights nothing beyond.
fn range_falloff(distance_squared: f32, range: f32) -> f32 {
    let ratio = distance_squared / (range * range);
    let window = clamp(1.0 - ratio * ratio, 0.0, 1.0);
    return window * window / max(distance_squared, 1e-4);
}

// The light the point and spot lights of this pixel's cluster add.
fn local_lighting(
    pixel: vec2<f32>,
    depth: f32,
    world_position: vec3<f32>,
    n: vec3<f32>,
    v: vec3<f32>,
    diffuse_colour: vec3<f32>,
    f0: vec3<f32>,
    alpha: f32,
) -> vec3<f32> {
    let range = cluster_ranges[cluster_index(pixel, depth)];
    let n_dot_v = max(dot(n, v), 1e-4);
    var colour = vec3<f32>(0.0);
    for (var i = 0u; i < range.y; i = i + 1u) {
        let light = local_lights[cluster_lights[range.x + i]];
        let to_light = light.position_range.xyz - world_position;
        let distance_squared = dot(to_light, to_light);
        let reach = light.position_range.w;
        if (distance_squared >= reach * reach) {
            continue;
        }
        let l = to_light * inverseSqrt(max(distance_squared, 1e-8));
        let n_dot_l = max(dot(n, l), 0.0);
        if (n_dot_l <= 0.0) {
            continue;
        }
        var cone = 1.0;
        if (light.intensity.w > 0.5) {
            let cos_angle = dot(-l, light.direction.xyz);
            cone = smoothstep(light.direction.w, light.cone.x, cos_angle);
            if (cone <= 0.0) {
                continue;
            }
        }
        let h = normalize(v + l);
        let n_dot_h = max(dot(n, h), 0.0);
        let v_dot_h = max(dot(v, h), 0.0);
        let f = fresnel_schlick(f0, v_dot_h);
        let specular = distribution_ggx(n_dot_h, alpha) * visibility_smith(n_dot_v, n_dot_l, alpha) * f;
        let diffuse = (vec3<f32>(1.0) - f) * diffuse_colour / PI;
        let illuminance = light.intensity.rgb * range_falloff(distance_squared, reach) * cone;
        colour += (diffuse + specular) * illuminance * n_dot_l;
    }
    return colour;
}
