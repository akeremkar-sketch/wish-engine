// Bloom: bright light spreads softly around itself (design note
// M12-look-2-bloom). The exposed HDR picture is halved again and again
// (`fs_first`, then `fs_down`), the levels are added back up from the
// coarsest (`fs_up`, blended into the next finer level by the pipeline), and
// the last step (`fs_composite`) takes a share of each pixel's light and
// gives the spread light back in its place.
//
// Every read is a `textureLoad` of a whole texel at a place held inside the
// texture, and every weight is worked out here: no sampler takes part, so
// every adapter computes the same picture. No pass has numbers of its own:
// sizes come from `textureDimensions`, and the setting's three numbers are
// one uniform that only the first and the last step read.

// What a pass reads: the HDR picture (`fs_first`), the level before
// (`fs_down`), the level after (`fs_up`), the finest level (`fs_composite`).
@group(0) @binding(0) var source: texture_2d<f32>;
// The HDR picture, for `fs_composite`.
@group(0) @binding(1) var picture: texture_2d<f32>;

// The setting (`look.rs`). The layout must match `BLOOM_UNIFORM_SIZE`.
struct Bloom {
    // x: the share of the light that is spread; y: the threshold, in
    // multiples of white; z: half the width of the threshold's soft start.
    params: vec4<f32>,
};

@group(0) @binding(2) var<uniform> bloom: Bloom;

// Light brighter than this many times white blooms as this much: nothing a
// scene draws on purpose is near it, and a surface that overflowed the
// picture's floats cannot wash the whole picture out.
const BRIGHTEST: f32 = 256.0;
// The largest number the picture's 16-bit floats hold.
const LARGEST: f32 = 65504.0;
// However bright one texel is, in the first step down it moves the average
// of its neighbourhood like one this many times white at most.
const FIREFLY: f32 = 8.0;

const LUMINANCE: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The texel of `source` at `at`; outside the texture, the nearest one
// inside.
fn texel(at: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(source)) - vec2<i32>(1);
    return textureLoad(source, clamp(at, vec2<i32>(0), last), 0).rgb;
}

// A texel of the HDR picture as light that can be averaged: what is not a
// number counts as no light, a negative value as none, infinity as the
// largest number there is.
fn finite(light: vec3<f32>) -> vec3<f32> {
    let numbers = select(vec3<f32>(0.0), light, light == light);
    return clamp(numbers, vec3<f32>(0.0), vec3<f32>(LARGEST));
}

// The share of a pixel's light that is spread, for a pixel this bright
// (luminance in multiples of white): all of it without a threshold; with
// one, the part above it, starting softly half a threshold below so that a
// surface that crosses it does not pop. Nothing is divided by zero: both
// denominators are held above it.
fn scattered(luminance: f32) -> f32 {
    let threshold = bloom.params.y;
    let knee = bloom.params.z;
    let soft = clamp(luminance - threshold + knee, 0.0, 2.0 * knee);
    let over = max(soft * soft / max(4.0 * knee, 1e-6), luminance - threshold);
    let share = clamp(over / max(luminance, 1e-6), 0.0, 1.0);
    return select(share, 1.0, threshold <= 0.0);
}

// The down-sampling filter: thirteen bilinear reads at texel corners (after
// Jimenez, "Next Generation Post Processing in Call of Duty: Advanced
// Warfare"), written out as what it is over whole texels. Each bilinear
// read is the mean of four texels, so the filter is a fixed kernel over a
// 6x6 block: half of it a 4x4 box, half a wider tent. Along one axis the
// box weighs the six texels [0, 1, 1, 1, 1, 0] / 4 and the tent
// [1, 1, 2, 2, 1, 1] / 8.
fn box_weight(index: i32) -> f32 {
    return select(0.0, 0.25, index >= 1 && index <= 4);
}

fn tent_weight(index: i32) -> f32 {
    return select(0.125, 0.25, index == 2 || index == 3);
}

// The weight of texel (x, y) of the 6x6 block; the 36 add up to 1.
fn down_weight(x: i32, y: i32) -> f32 {
    return 0.5 * (box_weight(x) * box_weight(y) + tent_weight(x) * tent_weight(y));
}

// The first texel of the block of `source` that the pixel at `position` of
// a picture half its size is made from. Pixel i lies over texels 2i and
// 2i + 1, the middle two of the six.
fn block(position: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(position.xy) * 2 - vec2<i32>(2);
}

// The first step down, from the HDR picture: each texel is cleaned, the
// part of it that scatters is taken, and the average is weighted by
// 1 / (1 + luminance / FIREFLY), so that one texel far brighter than its
// neighbours (a firefly) moves the average like one of FIREFLY times white
// at most and does not bloom as a flickering blob. A bright area is not
// held: its texels weigh the same. The sum of the weights is at least
// 1 / 33, never zero.
@fragment
fn fs_first(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let first = block(position);
    var total = vec3<f32>(0.0);
    var weights = 0.0;
    for (var y = 0; y < 6; y = y + 1) {
        for (var x = 0; x < 6; x = x + 1) {
            let light = min(finite(texel(first + vec2<i32>(x, y))), vec3<f32>(BRIGHTEST));
            let luminance = dot(light, LUMINANCE);
            let weight = down_weight(x, y) / (1.0 + luminance / FIREFLY);
            total += light * (scattered(luminance) * weight);
            weights += weight;
        }
    }
    return vec4<f32>(total / max(weights, 1e-6), 1.0);
}

// Every further step down: the same filter, plain.
@fragment
fn fs_down(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let first = block(position);
    var total = vec3<f32>(0.0);
    for (var y = 0; y < 6; y = y + 1) {
        for (var x = 0; x < 6; x = x + 1) {
            total += texel(first + vec2<i32>(x, y)) * down_weight(x, y);
        }
    }
    return vec4<f32>(total, 1.0);
}

// `source`, which is half the size of the picture being drawn, at the pixel
// `at` of that picture: the 3x3 tent of bilinear reads of the same talk,
// written out over whole texels. Along each axis it is the four texels
// nearest the pixel's centre, each weighted by 2 minus its distance in
// texels (7, 5, 3 and 1 sixteenths). The weights are worked out from the
// place asked for, before it is held inside the texture.
fn spread_up(at: vec2<i32>) -> vec3<f32> {
    // The pixel's centre, in texels of `source`.
    let centre = (vec2<f32>(at) + vec2<f32>(0.5)) * 0.5;
    // A pixel in the left (upper) half of its texel has two texels before
    // it and one after; one in the right (lower) half the reverse.
    let first = (at >> vec2<u32>(1u)) - vec2<i32>(2) + (at & vec2<i32>(1));
    var total = vec3<f32>(0.0);
    for (var y = 0; y < 4; y = y + 1) {
        for (var x = 0; x < 4; x = x + 1) {
            let place = first + vec2<i32>(x, y);
            let away = abs(vec2<f32>(place) + vec2<f32>(0.5) - centre);
            let weight = max(vec2<f32>(2.0) - away, vec2<f32>(0.0)) * 0.25;
            total += texel(place) * (weight.x * weight.y);
        }
    }
    return total;
}

// A step up: the coarser level, spread over the level being drawn. The
// pipeline blends it into what that level holds, by the pass's blend
// constant (how much of the glow lies in the coarser levels).
@fragment
fn fs_up(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(spread_up(vec2<i32>(position.xy)), 1.0);
}

// The last step: the picture with its bloom. Each pixel gives up a share of
// the part of its light that scatters and takes the same share of the
// light spread from everywhere, so an ordinary picture keeps its
// brightness. (A small light far brighter than white gives up more than is
// spread from it: the first step down holds such texels back.) A pixel that
// is not a number becomes black here (as tone mapping would make it), an
// infinite one the largest number, and the sum is held to that number too:
// past it a 16-bit float is infinity, which tone mapping draws black.
@fragment
fn fs_composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(position.xy);
    let light = finite(textureLoad(picture, at, 0).rgb);
    let held = min(light, vec3<f32>(BRIGHTEST));
    let given = held * (scattered(dot(held, LUMINANCE)) * bloom.params.x);
    let taken = spread_up(at) * bloom.params.x;
    return vec4<f32>(min(light - given + taken, vec3<f32>(LARGEST)), 1.0);
}
