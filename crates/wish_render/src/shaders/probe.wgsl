// The probe: one triangle with a colour per corner. It exercises
// rasterization coverage at the edges and colour interpolation, the two
// places software and hardware rasterizers differ most.

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.8),
        vec2<f32>(-0.8, -0.7),
        vec2<f32>(0.8, -0.6),
    );
    var colours = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.1, 0.1),
        vec3<f32>(0.1, 1.0, 0.1),
        vec3<f32>(0.1, 0.1, 1.0),
    );
    var out: VertexOut;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.colour = colours[index];
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.colour, 1.0);
}
