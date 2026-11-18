#import tonemap

// Reads the exposed HDR image and writes the displayable one. The target is
// sRGB, so the hardware applies the transfer function on write.

@group(0) @binding(0) var hdr: texture_2d<f32>;

// The colour grading (`look.rs`), read by `fs_graded` only. The layout must
// match `GRADE_UNIFORM_SIZE`.
struct Grade {
    // The white balance: the rows of the matrix a colour is multiplied by.
    balance_r: vec4<f32>,
    balance_g: vec4<f32>,
    balance_b: vec4<f32>,
    // x: contrast; y: saturation; z: how much the corners are darkened.
    params: vec4<f32>,
    // x, y: 1 / the picture's width and height in pixels.
    pixel: vec4<f32>,
};

@group(0) @binding(1) var<uniform> grade: Grade;

// Middle grey after exposure: contrast turns around it.
const MIDDLE_GREY: f32 = 0.18;

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

// The exposed colour at `position` (pixels), graded: white balance, contrast,
// saturation, then the lens's darker corners. All of it happens before tone
// mapping, on light, so that highlights still roll off smoothly afterwards.
fn graded(colour: vec3<f32>, position: vec2<f32>) -> vec3<f32> {
    var c = max(vec3<f32>(
        dot(grade.balance_r.rgb, colour),
        dot(grade.balance_g.rgb, colour),
        dot(grade.balance_b.rgb, colour),
    ), vec3<f32>(0.0));
    // Contrast as a power law around middle grey: the same change in stops
    // above and below it. The base is kept above zero (pow of 0 is not
    // defined everywhere).
    c = MIDDLE_GREY * pow(max(c / MIDDLE_GREY, vec3<f32>(1e-6)), vec3<f32>(grade.params.x));
    let luminance = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = max(mix(vec3<f32>(luminance), c, grade.params.y), vec3<f32>(0.0));
    // 0 in the middle of the picture, 1 in its corners.
    let from_centre = length(position * grade.pixel.xy * 2.0 - vec2<f32>(1.0)) * 0.70710678;
    return c * (1.0 - grade.params.z * smoothstep(0.35, 1.0, from_centre));
}

// Tone mapping of a picture that is graded on the way. It is a function of
// its own, and `fs_main` above is left as it always was, so that a scene
// that asks for no grading is drawn by the very same code as before grading
// existed.
@fragment
fn fs_graded(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = textureLoad(hdr, vec2<i32>(position.xy), 0).rgb;
    // NaN or negative values from a broken input never reach the image.
    let safe = select(vec3<f32>(0.0), texel, texel == texel);
    let colour = graded(max(safe, vec3<f32>(0.0)), position.xy);
    return vec4<f32>(tonemap_pbr_neutral(colour), 1.0);
}
