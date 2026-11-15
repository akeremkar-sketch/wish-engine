#import tonemap

// Reads the exposed HDR image and writes the displayable one. The target is
// sRGB, so the hardware applies the transfer function on write.

@group(0) @binding(0) var hdr: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureLoad(hdr, vec2<i32>(position.xy), 0).rgb;
    // NaN or negative values from a broken input never reach the image.
    let safe = select(vec3<f32>(0.0), texel, texel == texel);
    return vec4<f32>(tonemap_pbr_neutral(max(safe, vec3<f32>(0.0))), 1.0);
}
