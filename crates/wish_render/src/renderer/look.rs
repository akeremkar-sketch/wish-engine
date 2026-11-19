//! The look of a frame: what the scene's look settings come to as the
//! numbers the shaders read.
//!
//! Six effects are switched here, each off unless the scene asks for it
//! (design notes `M12-look-1-sky-fog-grading-fxaa.md` and
//! `M12-look-2-bloom.md`):
//!
//! | Effect | Asked for by | Where it is drawn |
//! |---|---|---|
//! | sky light | `Ambient.source: Sky` | the opaque pass (`sky.wgsl`) |
//! | sun disc | `Sky.sun_size_deg` above 0, and a directional light | the sky pass (`sky.wgsl`) |
//! | fog | `Fog.density` above 0 | the sky and opaque passes (`fog.wgsl`) |
//! | bloom | `Bloom.strength` above 0 | its own passes, before tone mapping (`bloom.rs`) |
//! | colour grading | a `ColorGrading` field off its default | the tone mapping pass |
//! | FXAA | `RenderSettings.antialiasing: Fxaa` | its own pass, after tone mapping |
//!
//! A scene that asks for none of them must keep its picture: the defaults
//! are part of the file format (ADR-027, rule 1). So an effect that is off
//! costs the frame nothing it did not do before:
//!
//! - the sky pass and the tone mapping pass each have two fragment
//!   functions, the one they always had (`fs_main`, untouched) and one with
//!   the look (`fs_look`, `fs_graded`), and a frame uses the second only
//!   when it has a sun disc or fog, or is graded;
//! - the opaque pass branches on a flag in the frame uniform, and with the
//!   flags at 0 runs the instructions it always ran on the inputs it always
//!   had (one shader for both: its lighting code is not to be kept twice);
//! - the bloom passes and the FXAA pass are not run, and their targets are
//!   not taken.
//!
//! Every number is cleaned on the way: game code can write anything into a
//! resource, and nothing that is not finite may reach a shader.

use super::Bytes;
use super::bloom::{BloomPlan, BloomSettings};
use crate::components::Antialiasing;
use crate::extract::RenderScene;

/// Bytes of the tone mapping pass's uniform (`Grade` in
/// `tonemap_pass.wgsl`): five vectors.
pub(super) const GRADE_UNIFORM_SIZE: u64 = 80;

/// The brightest the sun's disc is drawn, as a multiple of a white surface
/// after exposure. A disc as small as the real sun would be a million times
/// brighter than the sky; the picture's 16-bit float target cannot hold that
/// and no bloom should be fed it. Six stops above white is still the
/// brightest thing in any picture.
const SUN_DISC_PEAK: f32 = 64.0;

/// The part of the disc's radius that is at full strength; the rest is its
/// soft edge.
const SUN_DISC_CORE: f32 = 0.8;

/// The smallest disc drawn, in degrees: below it the disc's brightness per
/// solid angle stops being a finite number.
const SUN_DISC_SMALLEST: f32 = 0.01;

/// Which optional effects of the lit picture a frame used. A debug view
/// uses none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    /// Ambient light came from the sky (`Ambient.source: Sky`).
    pub sky_light: bool,
    /// The sun's disc was drawn into the sky (`Sky.sun_size_deg`).
    pub sun_disc: bool,
    /// There was fog (`Fog.density`).
    pub fog: bool,
    /// Bright light was spread around itself (`Bloom.strength`).
    pub bloom: bool,
    /// The picture was graded (`ColorGrading`).
    pub color_grading: bool,
    /// Edges were smoothed with FXAA (`RenderSettings.antialiasing`).
    pub fxaa: bool,
}

impl Effects {
    /// The names of the effects that were on, in the order of the frame's
    /// passes: `sky_light`, `sun_disc`, `fog`, `bloom`, `color_grading`,
    /// `fxaa`.
    pub fn names(&self) -> Vec<&'static str> {
        [
            (self.sky_light, "sky_light"),
            (self.sun_disc, "sun_disc"),
            (self.fog, "fog"),
            (self.bloom, "bloom"),
            (self.color_grading, "color_grading"),
            (self.fxaa, "fxaa"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
}

/// `value` inside `low..=high`, or `fallback` when it is not a finite
/// number.
fn clean(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}

/// A colour's channels as light: finite and not negative.
fn clean_rgb(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(|channel| clean(channel, 0.0, f32::MAX, 0.0))
}

/// The sun's disc as the sky pass draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SunDisc {
    /// Its luminance (cd/m²) at full strength.
    radiance: [f32; 3],
    /// The angle from its centre (radians) up to which it is at full
    /// strength.
    core: f32,
    /// One over the angle (radians) over which it then fades by a factor of
    /// e.
    inverse_falloff: f32,
}

/// The disc of `scene`'s sun for a picture exposed by `exposure`, if it has
/// one: a disc as wide as `Sky.sun_size_deg` that gives off the light's
/// illuminance, held to [`SUN_DISC_PEAK`]. Its edge fades from full strength
/// at [`SUN_DISC_CORE`] of its radius to the brightness of a white surface
/// at the radius, so the disc that shows is as wide as the scene says
/// however bright it is.
fn sun_disc(scene: &RenderScene, exposure: f32) -> Option<SunDisc> {
    let size = clean(scene.sky.sun_size_deg, 0.0, 45.0, 0.0);
    let light = scene.lights.first()?;
    if size <= 0.0 {
        return None;
    }
    let radius = (size.max(SUN_DISC_SMALLEST) * 0.5).to_radians();
    // The disc's solid angle, 2π(1 - cos r), in the form that keeps its
    // digits for a small radius.
    let half = (radius * 0.5).sin();
    let solid_angle = 4.0 * std::f32::consts::PI * half * half;
    let radiance = clean_rgb(light.radiance).map(|channel| channel / solid_angle);
    let brightest = radiance[0].max(radiance[1]).max(radiance[2]) * exposure;
    if !brightest.is_finite() {
        return None;
    }
    let held = if brightest > SUN_DISC_PEAK {
        SUN_DISC_PEAK / brightest
    } else {
        1.0
    };
    let peak = brightest.min(SUN_DISC_PEAK);
    // From `peak` at the core down to 1 at the radius; a disc dimmer than
    // that fades over the same angle as one of e.
    let falloff = radius * (1.0 - SUN_DISC_CORE) / peak.max(std::f32::consts::E).ln();
    Some(SunDisc {
        radiance: radiance.map(|channel| channel * held),
        core: radius * SUN_DISC_CORE,
        inverse_falloff: 1.0 / falloff,
    })
}

/// The fog as the shaders read it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FogData {
    /// Its luminance (cd/m²): the sky's horizon, tinted.
    colour: [f32; 3],
    /// Share of light lost per meter at the base height.
    density: f32,
    /// How fast it thins with height, per meter.
    falloff: f32,
    /// The height (world Y) the density is given at.
    base: f32,
    /// How far from the camera it starts, in meters.
    start: f32,
    /// The most it hides, 0 to 1.
    limit: f32,
}

fn fog(scene: &RenderScene) -> Option<FogData> {
    let fog = &scene.fog;
    let density = clean(fog.density, 0.0, 1.0, 0.0);
    let limit = clean(fog.max_opacity, 0.0, 1.0, 1.0);
    if density <= 0.0 || limit <= 0.0 {
        return None;
    }
    let [r, g, b, _] = fog.tint.to_array();
    let tint = clean_rgb([r, g, b]);
    let horizon = clean_rgb(scene.sky.horizon);
    let luminance = clean(scene.sky.luminance, 0.0, f32::MAX, 0.0);
    Some(FogData {
        colour: clean_rgb([0, 1, 2].map(|i| horizon[i] * tint[i] * luminance)),
        density,
        falloff: clean(fog.height_falloff, 0.0, 10.0, 0.0),
        base: clean(fog.base_height, -100_000.0, 100_000.0, 0.0),
        start: clean(fog.start, 0.0, 100_000.0, 0.0),
        limit,
    })
}

/// The bloom's settings, cleaned; `None` when nothing is spread.
fn bloom(scene: &RenderScene) -> Option<BloomSettings> {
    let bloom = &scene.bloom;
    let strength = clean(bloom.strength, 0.0, 1.0, 0.0);
    if strength <= 0.0 {
        return None;
    }
    Some(BloomSettings {
        strength,
        spread: clean(bloom.spread, 0.0, 1.0, 0.5),
        threshold: clean(bloom.threshold, 0.0, 64.0, 0.0),
    })
}

/// The colour grading as the tone mapping pass reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grade {
    /// The white balance: the rows of the matrix a colour is multiplied by.
    balance: [[f32; 3]; 3],
    contrast: f32,
    saturation: f32,
    vignette: f32,
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn grade(scene: &RenderScene) -> Option<Grade> {
    let grading = &scene.grading;
    let contrast = clean(grading.contrast, 0.0, 2.0, 1.0);
    let saturation = clean(grading.saturation, 0.0, 2.0, 1.0);
    let temperature = clean(grading.temperature, -1.0, 1.0, 0.0);
    let tint = clean(grading.tint, -1.0, 1.0, 0.0);
    let vignette = clean(grading.vignette, 0.0, 1.0, 0.0);
    let balanced = temperature != 0.0 || tint != 0.0;
    if contrast == 1.0 && saturation == 1.0 && vignette == 0.0 && !balanced {
        return None;
    }
    Some(Grade {
        balance: if balanced {
            white_balance(temperature, tint)
        } else {
            IDENTITY
        },
        contrast,
        saturation,
        vignette,
    })
}

type Matrix = [[f64; 3]; 3];

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, cell) in out_row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

fn apply(matrix: &Matrix, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| (0..3).map(|k| matrix[row][k] * v[k]).sum())
}

/// The inverse of a matrix that has one (the two fixed matrices below do).
fn inverse(m: &Matrix) -> Matrix {
    let cofactor = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let determinant: f64 = (0..3).map(|c| m[0][c] * cofactor(0, c)).sum();
    let mut out = [[0.0; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, cell) in out_row.iter_mut().enumerate() {
            *cell = cofactor(column, row) / determinant;
        }
    }
    out
}

/// Linear sRGB to CIE XYZ (D65 white).
const RGB_TO_XYZ: Matrix = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

/// CIE XYZ to the cone responses the eye adapts in (CAT02).
const XYZ_TO_LMS: Matrix = [
    [0.7328, 0.4296, -0.1624],
    [-0.7036, 1.6975, 0.0061],
    [0.0030, 0.0136, 0.9834],
];

/// The chromaticity of daylight white (D65).
const WHITE_XY: (f64, f64) = (0.31271, 0.32902);

/// The cone responses to a white of chromaticity (`x`, `y`) at full
/// brightness.
fn white_lms(x: f64, y: f64) -> [f64; 3] {
    apply(&XYZ_TO_LMS, [x / y, 1.0, (1.0 - x - y) / y])
}

/// The matrix that re-balances linear sRGB colours as an eye would that had
/// adapted to a light warmer or cooler (`temperature`, -1 to 1) and greener
/// or more magenta (`tint`, -1 to 1) than daylight: each cone response is
/// scaled by how daylight white compares with that light's white (a von
/// Kries adaptation in CAT02 space). Positive `temperature` makes the
/// picture warmer, positive `tint` more magenta.
///
/// Only `+ - * /` on doubles, so every machine gets the same matrix.
fn white_balance(temperature: f32, tint: f32) -> [[f32; 3]; 3] {
    let (temperature, tint) = (f64::from(temperature) * 1.5, f64::from(tint) * 1.5);
    // The white the picture is taken to have been lit by: along the daylight
    // curve for the temperature (further when cooling the picture than when
    // warming it: the curve is steeper there), across it for the tint.
    let x = WHITE_XY.0 - temperature * if temperature < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.275_095_07 + tint * 0.05;
    let from = white_lms(x, y);
    let to = white_lms(WHITE_XY.0, WHITE_XY.1);
    let rgb_to_lms = multiply(&XYZ_TO_LMS, &RGB_TO_XYZ);
    let lms_to_rgb = inverse(&rgb_to_lms);
    let mut scaled = rgb_to_lms;
    for (row, gain) in scaled.iter_mut().zip([0, 1, 2].map(|i| to[i] / from[i])) {
        for cell in row {
            *cell *= gain;
        }
    }
    multiply(&lms_to_rgb, &scaled).map(|row| row.map(|cell| cell as f32))
}

/// What a frame's look settings come to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    sun: Option<SunDisc>,
    /// The strength of the sky's light when it is the ambient light.
    sky_light: Option<f32>,
    fog: Option<FogData>,
    bloom: Option<BloomSettings>,
    grade: Option<Grade>,
    /// Whether the FXAA pass runs.
    pub fxaa: bool,
}

impl Look {
    /// The look of `scene` in a picture exposed by `exposure` (the
    /// multiplier of `exposure_multiplier`).
    pub(super) fn of(scene: &RenderScene, exposure: f32) -> Self {
        Self {
            sun: sun_disc(scene, exposure),
            sky_light: scene
                .sky_light
                .map(|strength| clean(strength, 0.0, 10.0, 1.0)),
            fog: fog(scene),
            bloom: bloom(scene),
            grade: grade(scene),
            fxaa: scene.settings.antialiasing == Antialiasing::Fxaa,
        }
    }

    /// Which effects are on.
    pub(super) fn effects(&self) -> Effects {
        Effects {
            sky_light: self.sky_light.is_some(),
            sun_disc: self.sun.is_some(),
            fog: self.fog.is_some(),
            bloom: self.bloom.is_some(),
            color_grading: self.grade.is_some(),
            fxaa: self.fxaa,
        }
    }

    /// Whether the sky pass draws more than the gradient (a sun disc, fog).
    pub(super) fn sky(&self) -> bool {
        self.sun.is_some() || self.fog.is_some()
    }

    /// Appends the look's part of the frame uniform: the last six vectors of
    /// `Frame` in `common.wgsl`. An effect that is off writes a flag of 0
    /// and numbers no shader can trip over. `orthographic` says how the
    /// frame's camera looks: under such a camera fog goes by depth.
    pub(super) fn write_frame(&self, frame: &mut Bytes, orthographic: bool) {
        match &self.sun {
            Some(sun) => {
                frame.vec4(sun.radiance, 1.0);
                frame.floats(&[sun.core, sun.inverse_falloff, 0.0, 0.0]);
            }
            None => {
                frame.vec4([0.0; 3], 0.0);
                frame.floats(&[0.0, 1.0, 0.0, 0.0]);
            }
        }
        match self.sky_light {
            Some(strength) => frame.floats(&[1.0, strength, 0.0, 0.0]),
            None => frame.floats(&[0.0, 1.0, 0.0, 0.0]),
        }
        match &self.fog {
            Some(fog) => {
                frame.vec4(fog.colour, 1.0);
                frame.floats(&[fog.density, fog.falloff, fog.base, fog.start]);
                frame.floats(&[fog.limit, if orthographic { 1.0 } else { 0.0 }, 0.0, 0.0]);
            }
            None => {
                frame.vec4([0.0; 3], 0.0);
                frame.floats(&[0.0; 4]);
                frame.floats(&[0.0; 4]);
            }
        }
    }

    /// For a frame with bloom, its levels and numbers for a picture of
    /// `size` pixels.
    pub(super) fn bloom_plan(&self, size: (u32, u32)) -> Option<BloomPlan> {
        self.bloom.map(|settings| BloomPlan::new(settings, size))
    }

    /// For a graded frame, the tone mapping pass's uniform (`Grade` in
    /// `tonemap_pass.wgsl`) for a picture of `size` pixels.
    pub(super) fn grade_uniform(&self, (width, height): (u32, u32)) -> Option<Vec<u8>> {
        let grade = self.grade.as_ref()?;
        let mut bytes = Bytes::default();
        for row in grade.balance {
            bytes.vec4(row, 0.0);
        }
        bytes.floats(&[grade.contrast, grade.saturation, grade.vignette, 0.0]);
        bytes.floats(&[
            1.0 / width.max(1) as f32,
            1.0 / height.max(1) as f32,
            0.0,
            0.0,
        ]);
        bytes.pad_to(GRADE_UNIFORM_SIZE);
        Some(bytes.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balanced(matrix: &[[f32; 3]; 3], colour: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|row| (0..3).map(|k| matrix[row][k] * colour[k]).sum())
    }

    #[test]
    fn no_white_balance_is_the_identity_and_warm_is_warm() {
        // Asked for nothing, the adaptation's own matrix is the identity to
        // rounding (the frame uses the exact one and skips the work).
        let neutral = white_balance(0.0, 0.0);
        for (row, expected) in neutral.iter().zip(IDENTITY) {
            for (cell, expected) in row.iter().zip(expected) {
                assert!((cell - expected).abs() < 1e-6, "{neutral:?}");
            }
        }
        // Warmer: white gains red and loses blue. Cooler: the other way.
        let [r, _, b] = balanced(&white_balance(0.5, 0.0), [1.0; 3]);
        assert!(r > 1.02 && b < 0.9, "warm white is ({r}, {b})");
        let [r, _, b] = balanced(&white_balance(-0.5, 0.0), [1.0; 3]);
        assert!(r < 0.95 && b > 1.05, "cool white is ({r}, {b})");
        // Tint: positive towards magenta (less green), negative towards
        // green.
        let [r, g, b] = balanced(&white_balance(0.0, 0.5), [1.0; 3]);
        assert!(g < r && g < b, "magenta white is ({r}, {g}, {b})");
        let [r, g, b] = balanced(&white_balance(0.0, -0.5), [1.0; 3]);
        assert!(g > r && g > b, "green white is ({r}, {g}, {b})");
        // Every entry is a finite number at the ends of the range.
        for (temperature, tint) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            let matrix = white_balance(temperature, tint);
            assert!(
                matrix.iter().flatten().all(|cell| cell.is_finite()),
                "{temperature}, {tint}: {matrix:?}"
            );
        }
    }

    #[test]
    fn the_inverse_of_a_matrix_undoes_it() {
        let forward = multiply(&XYZ_TO_LMS, &RGB_TO_XYZ);
        let round = multiply(&inverse(&forward), &forward);
        for (row, cells) in round.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let expected = if row == column { 1.0 } else { 0.0 };
                assert!((cell - expected).abs() < 1e-12, "{round:?}");
            }
        }
    }

    #[test]
    fn numbers_that_are_not_numbers_never_reach_a_shader() {
        assert_eq!(clean(f32::NAN, 0.0, 1.0, 0.5), 0.5);
        assert_eq!(clean(f32::INFINITY, 0.0, 1.0, 0.5), 0.5);
        assert_eq!(clean(7.0, 0.0, 1.0, 0.5), 1.0);
        assert_eq!(clean(-7.0, 0.0, 1.0, 0.5), 0.0);
        assert_eq!(clean_rgb([f32::NAN, -1.0, 2.0]), [0.0, 0.0, 2.0]);
    }
}
