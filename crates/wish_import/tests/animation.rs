//! Skins and animation clips (M6 WP6), on a bar made in code.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::Vec3;
use wish_import::{Interpolation, Property, load_model_bytes, samples};

fn bar() -> wish_import::Model {
    load_model_bytes(samples::bending_bar().as_bytes(), "assets/bar.gltf").unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn skins_and_clips_are_read() {
    let model = bar();
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    assert_eq!(model.skins.len(), 1);
    assert_eq!(model.skins[0].joints, [0, 1]);
    assert_eq!(model.animations.len(), 2);
    let bend = &model.animations[0];
    assert_eq!(bend.name.as_deref(), Some("bend"));
    assert_eq!(bend.duration, 1.0);
    assert_eq!(bend.channels[0].property, Property::Rotation);
    assert_eq!(bend.channels[0].interpolation, Interpolation::Linear);
    assert_eq!(model.animation_index("still"), Some(1));
    assert_eq!(model.animation_index(""), Some(0));
    assert_eq!(model.animation_index("run"), None);
    let primitive = &model.meshes[0].primitives[0];
    assert_eq!(primitive.joints.len(), 6);
    assert_eq!(primitive.weights[2], [0.5, 0.5, 0.0, 0.0]);
}

#[test]
fn the_bar_bends_as_worked_out_by_hand() {
    let model = bar();
    let placed = model.placed()[0];
    // Rest pose: the vertices where the file puts them.
    let rest = model.pose(None, 0.0);
    let (positions, _) = model.skin(&placed, &rest).unwrap();
    assert!(
        close(positions[5], Vec3::new(0.1, 2.0, 0.0)),
        "{positions:?}"
    );
    // After one second the elbow has turned 90° about +Z: the top of the
    // bar points along -X from the elbow at (0, 1, 0).
    let bend = model.animation_index("bend");
    let bent = model.pose(bend, 1.0);
    let (positions, normals) = model.skin(&placed, &bent).unwrap();
    assert!(
        close(positions[4], Vec3::new(-1.0, 0.9, 0.0)),
        "{:?}",
        positions[4]
    );
    assert!(
        close(positions[5], Vec3::new(-1.0, 1.1, 0.0)),
        "{:?}",
        positions[5]
    );
    // The bottom row follows the root, which does not move.
    assert!(close(positions[0], Vec3::new(-0.1, 0.0, 0.0)));
    assert!(close(normals[5], Vec3::Z));
    // Half way: 45°, and past the end the last key holds.
    let half = model.pose(bend, 0.5);
    let (positions, _) = model.skin(&placed, &half).unwrap();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let expected = Vec3::new(0.0, 1.0, 0.0) + Vec3::new(0.1 * s - s, 0.1 * s + s, 0.0);
    assert!(
        close(positions[5], expected),
        "{:?} vs {expected:?}",
        positions[5]
    );
    assert_eq!(model.pose(bend, 5.0), model.pose(bend, 1.0));
    // The same time, the same pose (to the bit).
    assert_eq!(model.pose(bend, 0.37), model.pose(bend, 0.37));
}

#[test]
fn broken_animation_data_is_refused() {
    let text = samples::bending_bar().replace(
        r#""count": 2, "type": "MAT4""#,
        r#""count": 1, "type": "MAT4""#,
    );
    let error = load_model_bytes(text.as_bytes(), "assets/bar.gltf").unwrap_err();
    assert!(error.code == "gltf_invalid", "{error:?}");
    assert!(!error.hint.is_empty());
}

#[test]
fn clips_are_found_by_name_or_by_number() {
    let model = bar();
    assert_eq!(model.animation_index(""), Some(0));
    assert_eq!(model.animation_index("still"), Some(1));
    // The name the tools give clips without one in the file.
    assert_eq!(model.animation_index("#1"), Some(1));
    assert_eq!(model.animation_index("#7"), None);
    assert_eq!(model.animation_index("missing"), None);
}
