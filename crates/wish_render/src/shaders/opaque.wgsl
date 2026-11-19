#import common
#import sky
#import brdf
#import draw
#import lights
#import fog

// Surfaces, lit by one directional light, the point and spot lights of their
// cluster, the ambient light (one colour, or the sky's own light), and the
// sky's reflection, seen through the fog when there is one. Output is
// already multiplied by the exposure. The opaque pass and the blended pass
// (see-through surfaces) both draw with these functions.

// The material's textures. A slot without a texture holds a 1x1 default, so
// the factors alone decide.
@group(2) @binding(0) var base_color_texture: texture_2d<f32>;
@group(2) @binding(1) var metallic_roughness_texture: texture_2d<f32>;
@group(2) @binding(2) var normal_texture: texture_2d<f32>;
@group(2) @binding(3) var occlusion_texture: texture_2d<f32>;
@group(2) @binding(4) var emissive_texture: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec3<f32>,
    @location(4) tangent_sign: f32,
    // Distance in front of the camera, for finding the pixel's light cluster.
    @location(5) view_depth: f32,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let world = draw.model * vec4<f32>(in.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world_position = world.xyz;
    out.normal = (draw.normal_matrix * vec4<f32>(in.normal, 0.0)).xyz;
    out.uv = in.uv * draw.uv_transform.xy + draw.uv_transform.zw;
    out.tangent = (draw.model * vec4<f32>(in.tangent.xyz, 0.0)).xyz;
    out.tangent_sign = in.tangent.w;
    out.view_depth = -(frame.view * world).z;
    return out;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let albedo = textureSample(base_color_texture, material_sampler, in.uv) * draw.base_color;
    // Pixels below the cutoff are not drawn (alpha mode MASK).
    if (draw.params2.z > 0.0 && albedo.a < draw.params2.z) {
        discard;
    }
    let mr = textureSample(metallic_roughness_texture, material_sampler, in.uv);
    let metallic = clamp(draw.params.x * mr.b, 0.0, 1.0);
    let roughness = clamp(draw.params.y * mr.g, 0.045, 1.0);
    let alpha = roughness * roughness;
    let base = albedo.rgb;
    let diffuse_colour = base * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), base, metallic);

    // The surface normal: the geometric one, bent by the normal map where the
    // mesh has tangents. Back faces of double-sided materials flip it.
    // (Sampled outside the branch: sampling needs uniform control flow.)
    let sampled = textureSample(normal_texture, material_sampler, in.uv).xyz * 2.0 - 1.0;
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    if (dot(in.tangent, in.tangent) > 0.25) {
        let t = normalize(in.tangent - n * dot(n, in.tangent));
        let b = cross(n, t) * in.tangent_sign;
        let bent = vec3<f32>(sampled.xy * draw.params2.x, sampled.z);
        n = normalize(t * bent.x + b * bent.y + n * bent.z);
    }
    let v = normalize(frame.camera_position.xyz - in.world_position);
    let n_dot_v = max(dot(n, v), 1e-4);

    // Occlusion darkens only the indirect light.
    let occlusion = 1.0 + draw.params2.y
        * (textureSample(occlusion_texture, material_sampler, in.uv).r - 1.0);

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
            // Shadows only dim the direct light; sampled at the geometric normal.
            // A frame has either one shadow map or cascades.
            var visibility = 1.0;
            if (frame.cascades.info.x > 0.5) {
                visibility = cascaded_visibility(in.world_position, normalize(in.normal));
            } else {
                visibility = shadow_visibility(in.world_position, normalize(in.normal));
            }
            colour += (diffuse + specular) * frame.light_radiance.rgb * n_dot_l * visibility;
        }
    }
    if (frame.cluster.w > 0.5) {
        colour += local_lighting(in.clip.xy, in.view_depth, in.world_position, n, v,
            diffuse_colour, f0, alpha);
    }
    // Ambient light on the diffuse part, the sky's reflection on the
    // specular part.
    let reflected = reflect(-v, n);
    if (frame.sky_light.x > 0.5) {
        // From the sky: each surface gathers the part of the sky it faces,
        // and a rough surface reflects a wide part of it. The sky's strength
        // scales only the light it gives a surface; its reflection shows
        // the sky as it is drawn, as in the flat mode, so a metal is not
        // dimmed with it.
        colour += diffuse_colour * sky_irradiance(n) / PI * (occlusion * frame.sky_light.y);
        colour += environment_brdf(f0, roughness, n_dot_v)
            * sky_reflection(reflected, roughness) * occlusion;
    } else {
        colour += diffuse_colour * frame.ambient.rgb / PI * occlusion;
        colour += environment_brdf(f0, roughness, n_dot_v) * sky_radiance(reflected) * occlusion;
    }
    let emission = textureSample(emissive_texture, material_sampler, in.uv).rgb * draw.emissive.rgb;
    colour += emission * frame.exposure.y;
    if (frame.fog_colour.w > 0.5) {
        if (frame.fog_limit.y > 0.5) {
            // An orthographic camera looks along one direction from every
            // point of its plane: the ray to a surface is as long as the
            // surface is deep, wherever in the picture it is.
            let forward_y = -frame.view[1].z;
            let deep = max(in.view_depth, 0.0);
            colour = fogged(colour, in.world_position.y - forward_y * deep, forward_y, deep);
        } else {
            let to_surface = in.world_position - frame.camera_position.xyz;
            let away = length(to_surface);
            colour = fogged(colour, frame.camera_position.y,
                to_surface.y / max(away, 1e-6), away);
        }
    }
    // The alpha is the surface's: the blended pass mixes a see-through
    // surface by it (its draw's alpha is cleaned to 0..1 on the CPU). In the
    // opaque pass it lands in the HDR picture's alpha, which nothing reads.
    return vec4<f32>(colour * frame.exposure.x, albedo.a);
}
