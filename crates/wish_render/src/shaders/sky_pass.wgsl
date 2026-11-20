#import common
#import sky
#import fog

// Fills the whole image with the sky, already multiplied by the exposure.

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> SkyOut {
    // One triangle that covers the screen.
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    var out: SkyOut;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_main(in: SkyOut) -> @location(0) vec4<f32> {
    let far = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let direction = far.xyz / far.w - frame.camera_position.xyz;
    return vec4<f32>(sky_radiance(direction) * frame.exposure.x, 1.0);
}

// The sky of a scene that asks for more than the gradient: the sun's disc,
// and the fog in front of both. It is a function of its own, and `fs_main`
// above is left as it always was, so that a scene that asks for neither is
// drawn by the very same code as before these existed.
@fragment
fn fs_look(in: SkyOut) -> @location(0) vec4<f32> {
    let far = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    // The gradient from the ray as `fs_main` passes it (not normalized
    // first), so that both functions do the same arithmetic for it.
    let ray = far.xyz / far.w - frame.camera_position.xyz;
    let direction = normalize(ray);
    var radiance = sky_radiance(ray);
    if (frame.sun_radiance.w > 0.5) {
        radiance += sun_disc(direction);
    }
    if (frame.fog_colour.w > 0.5) {
        radiance = fogged(radiance, frame.camera_position.y, direction.y, FOG_SKY_DISTANCE);
    }
    return vec4<f32>(radiance * frame.exposure.x, 1.0);
}
