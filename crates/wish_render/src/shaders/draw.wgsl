// One draw's data, shared by the shadow, opaque and debug passes. The layout must
// match `DrawList::push` in renderer.rs.

struct Draw {
    model: mat4x4<f32>,
    // The inverse transpose of the model matrix, for normals.
    normal_matrix: mat4x4<f32>,
    base_color: vec4<f32>,
    // x: metallic factor, y: roughness factor.
    params: vec4<f32>,
    // rgb: emissive factor.
    emissive: vec4<f32>,
    // x: normal scale, y: occlusion strength, z: alpha cutoff (0: no cutoff).
    params2: vec4<f32>,
    // How the material's textures are laid on the surface: xy scales the
    // texture coordinates, zw is added to them. (1, 1, 0, 0) for a model's
    // own materials and for surfaces without textures.
    uv_transform: vec4<f32>,
};

@group(1) @binding(0) var<uniform> draw: Draw;
