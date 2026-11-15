// Khronos PBR Neutral tone mapping: colours below about 0.8 pass through
// unchanged (so a material's base colour shows as authored), brighter ones
// roll off smoothly towards white without shifting hue.
// https://github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral
//
// Written without branches or divisions that can reach zero. The reference
// form returns early for dark colours and divides by `peak`; a compiler that
// flattens the branch then divides by zero for black pixels. On Windows'
// software rasterizer (WARP, without the debug layer) that form lost the
// device (measured in M3 WP3), so this form computes both sides safely and
// selects. The result is the same.

fn tonemap_pbr_neutral(input: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    let x = min(input.r, min(input.g, input.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    let colour = input - offset;
    let peak = max(colour.r, max(colour.g, colour.b));
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / max(peak + d - start_compression, 1e-6);
    let scaled = colour * (new_peak / max(peak, 1e-6));
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    let compressed = mix(scaled, vec3<f32>(new_peak), g);
    return select(compressed, colour, peak < start_compression);
}
