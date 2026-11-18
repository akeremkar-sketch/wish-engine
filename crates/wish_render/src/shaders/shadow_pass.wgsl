#import draw

// The shadow pass: every surface drawn from the light, depth only.
//
// Solid surfaces use `vs_main` and no fragment stage. Alpha-masked surfaces
// (alpha mode MASK) use `vs_masked` and `fs_masked`, which leave out the
// pixels the lit pass cuts out, so that a leaf casts a leaf's shadow.

struct Light {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> light: Light;
// Only the masked pipeline binds the material (the same layout as the lit
// pass; only the base colour and the sampler are read).
@group(2) @binding(0) var base_color_texture: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return light.view_proj * draw.model * vec4<f32>(position, 1.0);
}

struct MaskedOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_masked(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> MaskedOut {
    var out: MaskedOut;
    out.clip = light.view_proj * draw.model * vec4<f32>(position, 1.0);
    out.uv = uv;
    return out;
}

// The same test as in the lit pass: below the cutoff, nothing is there.
//
// The texture is read at its finest level, never at the level the light's
// view would pick. A shadow map is often coarser than the texture, and from
// the first mip level on alpha is the pattern's mean coverage: a sparse
// fence would be under the cutoff everywhere and cast no shadow at all. At
// the finest level each shadow texel sees one point of the pattern, solid or
// not, and the filter of the lit pass turns that into part shade. (It also
// makes the result independent of how an adapter computes derivatives.)
@fragment
fn fs_masked(in: MaskedOut) {
    let alpha = textureSampleLevel(base_color_texture, material_sampler, in.uv, 0.0).a
        * draw.base_color.a;
    if (draw.params2.z > 0.0 && alpha < draw.params2.z) {
        discard;
    }
}
