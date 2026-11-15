#import common
#import sky
#import brdf

// Opaque surfaces, lit by one directional light, the ambient light, and the
// sky's reflection. Output is already multiplied by the exposure.

// The layout must match `DrawUniform` in renderer.rs.
struct Draw {
    model: mat4x4<f32>,
    // The inverse transpose of the model matrix, for normals.
    normal_matrix: mat4x4<f32>,
    base_color: vec4<f32>,
    // x: metallic, y: roughness.
    params: vec4<f32>,
    // rgb: emissive.
    emissive: vec4<f32>,
};

@group(1) @binding(0) var<uniform> draw: Draw;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let world = draw.model * vec4<f32>(in.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world_position = world.xyz;
    out.normal = (draw.normal_matrix * vec4<f32>(in.normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(frame.camera_position.xyz - in.world_position);
    let base = draw.base_color.rgb;
    let metallic = clamp(draw.params.x, 0.0, 1.0);
    let roughness = clamp(draw.params.y, 0.045, 1.0);
    let alpha = roughness * roughness;
    let diffuse_colour = base * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), base, metallic);
    let n_dot_v = max(dot(n, v), 1e-4);

    var colour = vec3<f32>(0.0);
    if (frame.light_direction.w > 0.5) {
        let l = -normalize(frame.light_direction.xyz);
        let n_dot_l = max(dot(n, l), 0.0);
        if (n_dot_l > 0.0) {
            let h = normalize(v + l);
            let n_dot_h = max(dot(n, h), 0.0);
            let v_dot_h = max(dot(v, h), 0.0);
            let f = fresnel_schlick(f0, v_dot_h);
            let specular = distribution_ggx(n_dot_h, alpha)
                * visibility_smith(n_dot_v, n_dot_l, alpha) * f;
            let diffuse = (vec3<f32>(1.0) - f) * diffuse_colour / PI;
            colour += (diffuse + specular) * frame.light_radiance.rgb * n_dot_l;
        }
    }
    // Ambient light on the diffuse part, the sky's reflection on the
    // specular part.
    colour += diffuse_colour * frame.ambient.rgb / PI;
    let reflected = reflect(-v, n);
    colour += environment_brdf(f0, roughness, n_dot_v) * sky_radiance(reflected);
    colour += draw.emissive.rgb * frame.exposure.y;
    return vec4<f32>(colour * frame.exposure.x, 1.0);
}
