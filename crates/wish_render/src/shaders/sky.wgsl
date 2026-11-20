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

// The light the sky sends onto a surface that faces along `normal` (unit), in
// lux: the gradient gathered over the half of the sky the surface sees.
//
// The gradient only changes with height, so what a surface gathers only
// depends on how far up it faces (t = normal.y), and the first three zonal
// harmonics of the gradient give it in closed form:
//   E(t) / pi = (z + h + g) / 3 + 2/5 (z - g) t + 5/168 (z + g - 2h) (3 t^2 - 1)
// for the zenith, horizon and ground colours z, h, g. Facing straight up
// that is 0.79 z + 0.21 h (the exact answer is 0.8 z + 0.2 h), so tops take
// the zenith's colour, sides a mix of all three and undersides the ground's.
fn sky_irradiance(normal: vec3<f32>) -> vec3<f32> {
    let t = normal.y;
    let z = frame.sky_zenith.rgb;
    let h = frame.sky_horizon.rgb;
    let g = frame.sky_ground.rgb;
    let gathered = (z + h + g) / 3.0
        + (z - g) * (0.4 * t)
        + (z + g - 2.0 * h) * ((5.0 / 168.0) * (3.0 * t * t - 1.0));
    return max(gathered, vec3<f32>(0.0)) * (PI * frame.sky_ground.w);
}

// What a surface of this roughness reflects of the sky along `direction`
// (unit), in cd/m2. A mirror shows the gradient itself; a rough surface
// gathers a wide part of the sky, which tends to the sky's mean light around
// that direction.
fn sky_reflection(direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let sharp = sky_radiance(direction);
    let wide = sky_irradiance(direction) / PI;
    return mix(sharp, wide, roughness * roughness);
}

// The sun's disc seen along `direction` (unit), in cd/m2: full strength up to
// the disc's core, fading out beyond it, and nothing below the horizon
// (there the gradient is the ground). Only called when the frame has a disc.
fn sun_disc(direction: vec3<f32>) -> vec3<f32> {
    // The light's direction is a unit vector, so the two arguments of atan2
    // are never both zero.
    let to_sun = -frame.light_direction.xyz;
    let angle = atan2(length(cross(direction, to_sun)), dot(direction, to_sun));
    let edge = exp(-max(angle - frame.sun_disc.x, 0.0) * frame.sun_disc.y);
    let above = smoothstep(-0.01, 0.01, direction.y);
    return frame.sun_radiance.rgb * (edge * above);
}
