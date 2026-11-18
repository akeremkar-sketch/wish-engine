#import common

// Fog: how much of what is seen along a ray the fog replaces with its own
// colour. Its thickness is `frame.fog.x` at the base height and falls off
// with height as exp(-falloff * height above the base); with a falloff of 0
// it is the same everywhere. The sky pass and the opaque pass both call
// `fog_amount`, so that a far hill and the sky behind it fade the same way.

// The sky is fogged as if it were this far away (meters).
const FOG_SKY_DISTANCE: f32 = 100000.0;

// How much of the light arriving along a ray the fog replaces: 0 for none,
// up to the fog's limit. The ray starts at the height `origin_y` (world Y:
// the camera's, or for an orthographic camera the point of its plane the
// pixel looks from), goes in a direction whose vertical part is
// `direction_y` (of a unit vector) and is `length_m` meters long. Only
// called when the frame has fog.
fn fog_amount(origin_y: f32, direction_y: f32, length_m: f32) -> f32 {
    let falloff = frame.fog.y;
    // The fogged part of the ray: from where the fog starts to its end.
    let travelled = max(length_m - frame.fog.w, 0.0);
    // The heights of its two ends above the fog's base.
    let start_height = origin_y + direction_y * frame.fog.w - frame.fog.z;
    let end_height = start_height + direction_y * travelled;
    // The fog's thickness at both ends, relative to the base. Far below the
    // base it is held (it hides everything there long before).
    let thick_from = exp(min(-falloff * start_height, 40.0));
    let thick_to = exp(min(-falloff * end_height, 40.0));
    // Its mean along the ray: the integral of an exponential, which is
    // (thick_from - thick_to) / rise for the height gained in units of the
    // falloff. A level ray (or no falloff) would divide by zero: there the
    // mean is the thickness at the start, corrected to first order. Both
    // sides are computed safely and one is selected, never a branch around a
    // division.
    let rise = falloff * (end_height - start_height);
    let level = abs(rise) < 0.001;
    let safe_rise = select(rise, 1.0, level);
    let sloped = max((thick_from - thick_to) / safe_rise, min(thick_from, thick_to));
    let mean = select(sloped, thick_from * (1.0 - 0.5 * rise), level);
    let depth = frame.fog.x * mean * travelled;
    return min(1.0 - exp(-depth), frame.fog_limit.x);
}

// `radiance` (cd/m2) seen through the fog along that ray.
fn fogged(radiance: vec3<f32>, origin_y: f32, direction_y: f32, length_m: f32) -> vec3<f32> {
    return mix(radiance, frame.fog_colour.rgb, fog_amount(origin_y, direction_y, length_m));
}
