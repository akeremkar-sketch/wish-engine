// Fast approximate anti-aliasing (FXAA), after the "quality" algorithm of
// FXAA 3.11 by Timothy Lottes: one pass over the tone-mapped picture that
// finds edges by their contrast, follows each along its length, and moves
// the pixel a part of a pixel across it, which is a blend with its neighbour
// on that side.
//
// Every read is a `textureLoad` of a whole pixel and every weight is worked
// out here: no sampler takes part, so every adapter computes the same
// picture. A pixel that is not on an edge is written back exactly as it was
// read.

@group(0) @binding(0) var picture: texture_2d<f32>;

// An edge needs this much contrast between a pixel and its four neighbours,
// as a share of the brightest of them...
const EDGE_THRESHOLD: f32 = 0.125;
// ...and at least this much in dark areas, where the eye sees none.
const EDGE_THRESHOLD_MIN: f32 = 0.0312;
// How much a pixel that stands out from all its neighbours (finer than an
// edge) is blended with them: 0 for not at all, 1 for fully.
const SUBPIXEL: f32 = 0.75;
// How many steps an edge is followed each way. The steps grow (1 pixel five
// times, then 2, 2, 2, 2, 4, 8), so the eleven reach 25 pixels.
const SEARCH_STEPS: i32 = 11;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The pixel at `at`; outside the picture, the nearest one inside.
fn pixel(at: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(textureDimensions(picture)) - vec2<i32>(1);
    return textureLoad(picture, clamp(at, vec2<i32>(0), last), 0).rgb;
}

// How bright a colour looks. The picture is read as linear light (its sRGB
// encoding is undone on the way in); the square root brings that close to
// what the eye makes of it, which is what edges are judged by.
fn luma(colour: vec3<f32>) -> f32 {
    return sqrt(dot(colour, vec3<f32>(0.299, 0.587, 0.114)));
}

fn luma_at(at: vec2<i32>) -> f32 {
    return luma(pixel(at));
}

fn stride(index: i32) -> i32 {
    if (index < 5) {
        return 1;
    }
    if (index < 9) {
        return 2;
    }
    if (index < 10) {
        return 4;
    }
    return 8;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(position.xy);
    let centre = pixel(at);
    let m = luma(centre);
    // North is up in the picture (y grows downwards).
    let n = luma_at(at + vec2<i32>(0, -1));
    let s = luma_at(at + vec2<i32>(0, 1));
    let w = luma_at(at + vec2<i32>(-1, 0));
    let e = luma_at(at + vec2<i32>(1, 0));
    let highest = max(m, max(max(n, s), max(w, e)));
    let lowest = min(m, min(min(n, s), min(w, e)));
    let contrast = highest - lowest;
    // No early return for the pixels that are not on an edge: everything
    // below is computed with denominators that cannot be zero, and the
    // unchanged pixel is selected at the end.
    let on_edge = contrast >= max(EDGE_THRESHOLD_MIN, highest * EDGE_THRESHOLD);

    let nw = luma_at(at + vec2<i32>(-1, -1));
    let ne = luma_at(at + vec2<i32>(1, -1));
    let sw = luma_at(at + vec2<i32>(-1, 1));
    let se = luma_at(at + vec2<i32>(1, 1));

    // Which way the edge runs: along the rows (a horizontal edge) when
    // brightness changes more from row to row than from column to column.
    let down_the_columns = abs(nw + sw - 2.0 * w) + 2.0 * abs(n + s - 2.0 * m) + abs(ne + se - 2.0 * e);
    let along_the_rows = abs(nw + ne - 2.0 * n) + 2.0 * abs(w + e - 2.0 * m) + abs(sw + se - 2.0 * s);
    let horizontal = down_the_columns >= along_the_rows;

    // The two neighbours across the edge, and the one the pixel differs
    // from most: the edge lies between the pixel and that one.
    let before = select(w, n, horizontal);
    let after = select(e, s, horizontal);
    let towards_before = abs(before - m) >= abs(after - m);
    let steepness = 0.25 * max(abs(before - m), abs(after - m));
    let unit = select(vec2<i32>(1, 0), vec2<i32>(0, 1), horizontal);
    let across = select(unit, -unit, towards_before);
    let along = select(vec2<i32>(0, 1), vec2<i32>(1, 0), horizontal);
    // The brightness on the edge itself: between the pixel and that
    // neighbour.
    let on_the_edge = 0.5 * (m + select(after, before, towards_before));

    // Follow the edge both ways until the brightness on it changes by as
    // much as the edge is steep: there it ends or turns.
    var reached_back = false;
    var reached_forth = false;
    var change_back = 0.0;
    var change_forth = 0.0;
    var back = 0;
    var forth = 0;
    for (var i = 0; i < SEARCH_STEPS; i = i + 1) {
        if (!reached_back) {
            back = back + stride(i);
            let here = at - along * back;
            change_back = 0.5 * (luma_at(here) + luma_at(here + across)) - on_the_edge;
            reached_back = abs(change_back) >= steepness;
        }
        if (!reached_forth) {
            forth = forth + stride(i);
            let here = at + along * forth;
            change_forth = 0.5 * (luma_at(here) + luma_at(here + across)) - on_the_edge;
            reached_forth = abs(change_forth) >= steepness;
        }
        if (reached_back && reached_forth) {
            break;
        }
    }

    // The nearer the pixel is to an end of the edge, the more of the
    // neighbour it takes: half at the end itself, nothing in the middle.
    // Both distances are at least 1, so the length is at least 2.
    let back_length = f32(back);
    let forth_length = f32(forth);
    let nearer_back = back_length < forth_length;
    let nearest = min(back_length, forth_length);
    var shift = 0.5 - nearest / (back_length + forth_length);
    // Only when the edge really steps towards this pixel at that end: the
    // brightness there has to lie on the other side of the edge's own than
    // the pixel does.
    let end_change = select(change_forth, change_back, nearer_back);
    let steps_this_way = (end_change < 0.0) != (m < on_the_edge);
    shift = select(0.0, shift, steps_this_way);

    // A pixel that stands out from all eight neighbours: blended by how much
    // it does, so single bright or dark pixels do not flicker.
    let around = (2.0 * (n + s + w + e) + nw + ne + sw + se) / 12.0;
    let standing_out = clamp(abs(around - m) / max(contrast, 1e-6), 0.0, 1.0);
    let smooth_out = (3.0 - 2.0 * standing_out) * standing_out * standing_out;
    shift = max(shift, smooth_out * smooth_out * SUBPIXEL);

    let blended = mix(centre, pixel(at + across), shift);
    return vec4<f32>(select(centre, blended, on_edge), 1.0);
}
