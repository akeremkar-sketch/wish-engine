#import common

// The sky's radiance (cd/m2) seen along `direction`: a gradient from the
// ground through the horizon to the zenith.
fn sky_radiance(direction: vec3<f32>) -> vec3<f32> {
    let up = normalize(direction).y;
    var colour: vec3<f32>;
    if (up >= 0.0) {
        colour = mix(frame.sky_horizon.rgb, frame.sky_zenith.rgb, sqrt(up));
    } else {
        colour = mix(frame.sky_horizon.rgb, frame.sky_ground.rgb, sqrt(-up));
    }
    return colour * frame.sky_ground.w;
}
