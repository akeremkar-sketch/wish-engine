//! Every engine type's schema survives the wire form game modules use
//! (`wish_schema::wire`) unchanged, and malformed descriptions are refused.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::math::{Mat4, Quat, Vec2, Vec3, Vec4};
use wish_core::{AssetRef, Color, Entity, Text};
use wish_schema::wire::{from_wire, to_wire};
use wish_schema::{Plain, TypeSchema, component, schema};
use wish_types::*;

/// A component with every kind of field.
#[component]
struct Everything {
    /// A flag.
    flag: bool,
    /// Small.
    #[schema(range = -5..=5)]
    small: i8,
    /// A count.
    #[schema(unit = "items", alias = "number")]
    count: u64,
    /// A ratio.
    #[schema(range = 0.0..=1.0, default = 0.5)]
    ratio: f64,
    /// Kinds of vectors.
    a: Vec2,
    /// Another.
    b: Vec3,
    /// Another.
    c: Vec4,
    /// A turn.
    q: Quat,
    /// A matrix.
    m: Mat4,
    /// A colour.
    color: Color,
    /// Someone.
    who: Entity,
    /// A name.
    name: Text,
    /// A file.
    file: AssetRef,
    /// A choice.
    mode: Mode,
    /// A few.
    list: [u16; 3],
    /// Nested.
    inner: Inner,
}

/// A choice.
#[schema]
enum Mode {
    /// First.
    On,
    /// Second.
    Off,
}

/// Nested data.
#[schema]
struct Inner {
    /// Deep.
    depth: i32,
}

fn round_trip(schema: TypeSchema) {
    let wire = to_wire(&schema);
    let text = serde_json::to_string(&wire).unwrap();
    let back = from_wire(&serde_json::from_str(&text).unwrap()).unwrap();
    assert_eq!(back, schema, "{}", schema.name);
    assert_eq!(
        wish_schema::fingerprint(&back),
        wish_schema::fingerprint(&schema)
    );
}

#[test]
fn every_engine_type_round_trips() {
    for schema in [
        Transform::schema(),
        GlobalTransform::schema(),
        Camera::schema(),
        DirectionalLight::schema(),
        Primitive::schema(),
        Material::schema(),
        Model::schema(),
        Sky::schema(),
        Ambient::schema(),
        Exposure::schema(),
        RenderSettings::schema(),
        Fog::schema(),
        ColorGrading::schema(),
        Time::schema(),
        Everything::schema(),
    ] {
        round_trip(schema);
    }
}

#[test]
fn malformed_descriptions_are_refused() {
    let good = to_wire(&Camera::schema());
    let cases = [
        json!(null),
        json!({ "name": "X" }),
        json!({ "name": "X", "doc": "", "size": 4, "align": 4, "kind": "float" }),
        json!({ "name": "X", "doc": "", "size": 4, "align": 4, "kind": { "union": [] } }),
        json!({ "name": "X", "doc": "", "size": -1, "align": 4, "kind": "f32" }),
        // A field outside the value.
        json!({ "name": "X", "doc": "", "size": 4, "align": 4, "kind": { "struct": [
            { "name": "a", "doc": "", "offset": 8, "ty": { "name": "f32", "doc": "", "size": 4, "align": 4, "kind": "f32" } } ] } }),
        // A primitive whose size is a lie.
        json!({ "name": "X", "doc": "", "size": 2, "align": 4, "kind": "f32" }),
    ];
    for case in cases {
        let error = from_wire(&case).unwrap_err();
        assert!(!error.hint.is_empty(), "{case}: {error:?}");
    }
    // Nested deeper than any real type: refused. (Descriptions arrive as
    // text, and serde_json itself stops parsing at 128 levels.)
    let mut deep = r#"{ "name": "u8", "doc": "", "size": 1, "align": 1, "kind": "u8" }"#.to_owned();
    for _ in 0..40 {
        deep = format!(
            r#"{{ "name": "A", "doc": "", "size": 1, "align": 1, "kind": {{ "array": {{ "element": {deep}, "len": 1 }} }} }}"#
        );
    }
    let error = from_wire(&serde_json::from_str(&deep).unwrap()).unwrap_err();
    assert!(error.message.contains("deep"), "{error:?}");
    assert!(from_wire(&good).is_ok());
}
