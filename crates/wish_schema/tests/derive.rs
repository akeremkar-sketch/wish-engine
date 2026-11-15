//! End-to-end tests of the attribute macros, the JSON codec, JSON Schema
//! export, schema validation and fingerprints, through the public API only.

// The sample types deliberately mix documented and undocumented items, to
// check that both reach the schema correctly.
#![allow(missing_docs)]
// This whole file is test code. Clippy only recognises `#[test]` functions as
// tests, so the helpers shared between them need the exemption spelled out.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use wish_core::math::{Quat, Vec3};
use wish_core::{Color, Diagnostic, Entity, Result, Text};
use wish_schema::{
    Component, Event, FieldSchema, JsonContext, NoTexts, Plain, Resource, TextStore, TypeKind,
    TypeSchema, VariantSchema, apply_json, component, default_bytes, event, fingerprint, from_json,
    json_schema, resource, schema, to_json, validate_bytes,
};

/// Position, rotation and scale of an entity.
#[component]
pub struct Transform {
    /// Position in world space.
    #[schema(unit = "m", alias = "position")]
    pub translation: Vec3,
    /// Orientation.
    pub rotation: Quat,
    /// Scale factor along each axis.
    #[schema(default = Vec3::ONE)]
    pub scale: Vec3,
}

/// The shape of a collider.
#[schema]
pub enum Shape {
    /// An axis-aligned box.
    Box,
    Sphere,
    /// A cylinder with rounded ends.
    Capsule = 7,
}

/// Moves an entity at a constant speed.
#[component]
pub struct Mover {
    /// Movement speed.
    #[schema(unit = "m/s", range = 0.0..=50.0, default = 5.0)]
    pub speed: f32,
    /// Whether the mover is active.
    #[schema(default = true)]
    pub enabled: bool,
    pub shape: Shape,
    /// The entity to move towards.
    pub target: Entity,
    pub label: Text,
    pub waypoints: [Vec3; 2],
    #[schema(range = 0..=9, default = 3)]
    pub lives: u8,
    pub tint: Color,
}

/// Marks the entity the player controls.
#[component]
pub struct Player;

/// The acceleration applied to every body.
#[resource]
pub struct Gravity {
    #[schema(unit = "m/s^2", default = Vec3::new(0.0, -9.81, 0.0))]
    pub acceleration: Vec3,
}

/// Sent when something is hurt.
#[event]
pub struct Damage {
    pub target: Entity,
    pub amount: f32,
}

#[schema]
pub struct Stats {
    pub health: u16,
    pub precise: f64,
}

#[component]
pub struct Unit {
    pub stats: Stats,
    pub team: [Shape; 3],
}

/// A minimal text table, standing in for the world's.
#[derive(Default)]
struct Texts(Vec<String>);

impl TextStore for Texts {
    fn intern(&mut self, text: &str) -> Result<Text> {
        if text.is_empty() {
            return Ok(Text::EMPTY);
        }
        let position = match self.0.iter().position(|known| known == text) {
            Some(position) => position,
            None => {
                self.0.push(text.to_owned());
                self.0.len() - 1
            }
        };
        Ok(Text::from_index(position as u32 + 1))
    }

    fn resolve(&self, text: Text) -> Option<&str> {
        if text.is_empty() {
            Some("")
        } else {
            self.0.get(text.index() as usize - 1).map(String::as_str)
        }
    }
}

fn encode<T: Plain>(value: &T) -> Vec<u8> {
    let mut bytes = vec![0xAAu8; size_of::<T>()];
    value.encode(&mut bytes);
    bytes
}

fn read<T: Plain>(value: &Value, texts: &mut Texts) -> Result<T> {
    let bytes = from_json(&T::schema(), value, &mut JsonContext::new(texts))?;
    Ok(T::decode(&bytes).expect("from_json produces valid bytes"))
}

fn write<T: Plain>(value: &T, texts: &Texts) -> Value {
    to_json(&T::schema(), &encode(value), texts).expect("a typed value converts to JSON")
}

fn read_error<T: Plain>(value: Value) -> Diagnostic {
    let mut texts = Texts::default();
    match read::<T>(&value, &mut texts) {
        Ok(_) => panic!("expected an error for {value}"),
        Err(diagnostic) => diagnostic,
    }
}

fn field<'a>(schema: &'a TypeSchema, name: &str) -> &'a FieldSchema {
    let TypeKind::Struct(fields) = &schema.kind else {
        panic!("{} is not a struct", schema.name);
    };
    fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("{} has no field {name}", schema.name))
}

fn sample_mover(texts: &mut Texts) -> Mover {
    Mover {
        speed: 12.5,
        enabled: false,
        shape: Shape::Capsule,
        target: Entity::new(12, 3),
        label: texts.intern("hero").unwrap(),
        waypoints: [Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.1, -0.2, 0.3)],
        lives: 9,
        tint: Color::rgba(0.25, 0.5, 0.75, 1.0),
    }
}

#[test]
fn layout_comes_from_the_compiler() {
    let transform = Transform::schema();
    assert_eq!(transform.name, "Transform");
    assert_eq!(transform.doc, "Position, rotation and scale of an entity.");
    assert_eq!((transform.size, transform.align), (40, 4));
    assert_eq!(transform.byte_len(), size_of::<Transform>());
    assert_eq!(field(&transform, "translation").offset, 0);
    assert_eq!(field(&transform, "rotation").offset, 12);
    assert_eq!(field(&transform, "scale").offset, 28);
    assert!(transform.is_padding_free());
    assert!(transform.is_raw_hashable());
    assert!(!transform.needs_validation());

    let mover = Mover::schema();
    assert_eq!(mover.byte_len(), size_of::<Mover>());
    assert_eq!(mover.align as usize, align_of::<Mover>());
    assert_eq!(
        field(&mover, "target").offset as usize,
        core::mem::offset_of!(Mover, target)
    );
    assert_eq!(
        field(&mover, "tint").offset as usize,
        core::mem::offset_of!(Mover, tint)
    );
    assert!(
        !mover.is_padding_free(),
        "Mover has padding after `shape` and `lives`"
    );
    assert!(!mover.is_raw_hashable());
    assert!(mover.needs_validation());
    assert!(mover.contains_text());

    let unit = Unit::schema();
    assert_eq!((unit.size, unit.align), (24, 8));
    assert_eq!(field(&unit, "stats").ty.name, "Stats");
    assert_eq!(field(&unit, "team").ty.name, "[Shape; 3]");

    let player = Player::schema();
    assert_eq!(player.size, 0);
    assert_eq!(player.kind, TypeKind::Struct(Vec::new()));
    assert_eq!(player.doc, "Marks the entity the player controls.");
}

#[test]
fn every_generated_schema_is_valid() {
    for schema in [
        Transform::schema(),
        Mover::schema(),
        Player::schema(),
        Gravity::schema(),
        Damage::schema(),
        Stats::schema(),
        Unit::schema(),
        Shape::schema(),
    ] {
        assert_eq!(schema.validate(), Ok(()), "schema of {}", schema.name);
    }
}

#[test]
fn roles_give_the_registered_name() {
    assert_eq!(<Transform as Component>::NAME, "Transform");
    assert_eq!(<Player as Component>::NAME, "Player");
    assert_eq!(<Gravity as Resource>::NAME, "Gravity");
    assert_eq!(<Damage as Event>::NAME, "Damage");
}

#[test]
fn field_options_reach_the_schema() {
    let transform = Transform::schema();
    let translation = field(&transform, "translation");
    assert_eq!(translation.doc, "Position in world space.");
    assert_eq!(translation.unit.as_deref(), Some("m"));
    assert_eq!(translation.aliases, ["position"]);
    assert_eq!(translation.range, None);

    let mover = Mover::schema();
    let speed = field(&mover, "speed");
    assert_eq!(speed.unit.as_deref(), Some("m/s"));
    assert_eq!(
        speed.range.map(|range| (range.min, range.max)),
        Some((0.0, 50.0))
    );
    assert_eq!(speed.default, Some(json!(5.0)));
    assert_eq!(
        field(&mover, "lives")
            .range
            .map(|range| (range.min, range.max)),
        Some((0.0, 9.0))
    );
    assert_eq!(field(&mover, "shape").doc, "");
}

#[test]
fn defaults_have_one_source() {
    let transform = Transform::default();
    assert_eq!(transform.translation, Vec3::ZERO);
    assert_eq!(transform.rotation, Quat::IDENTITY);
    assert_eq!(transform.scale, Vec3::ONE);

    let mover = Mover::default();
    assert_eq!(mover.speed, 5.0);
    assert!(mover.enabled);
    assert_eq!(mover.shape, Shape::Box);
    assert_eq!(mover.target, Entity::NULL);
    assert_eq!(mover.label, Text::EMPTY);
    assert_eq!(mover.lives, 3);
    assert_eq!(mover.tint, Color::WHITE);
    assert_eq!(Gravity::default().acceleration, Vec3::new(0.0, -9.81, 0.0));
    assert_eq!(Shape::default(), Shape::Box);

    // The schema's JSON defaults are the Rust defaults.
    let schema = Transform::schema();
    assert_eq!(
        field(&schema, "scale").default,
        Some(json!([1.0, 1.0, 1.0]))
    );
    assert_eq!(
        field(&schema, "rotation").default,
        Some(json!([0.0, 0.0, 0.0, 1.0]))
    );

    // Building a value from the schema alone gives the same bytes as the Rust default.
    assert_eq!(
        default_bytes(&Transform::schema()).unwrap(),
        encode(&Transform::default())
    );
    assert_eq!(
        default_bytes(&Mover::schema()).unwrap(),
        encode(&Mover::default())
    );
    assert_eq!(
        default_bytes(&Gravity::schema()).unwrap(),
        encode(&Gravity::default())
    );
    assert_eq!(
        default_bytes(&Damage::schema()).unwrap(),
        encode(&Damage::default())
    );
    assert_eq!(
        default_bytes(&Unit::schema()).unwrap(),
        encode(&Unit::default())
    );
    assert_eq!(default_bytes(&Player::schema()).unwrap(), Vec::<u8>::new());
}

#[test]
fn built_in_type_defaults_match_their_rust_defaults() {
    fn check<T: Plain>() {
        assert_eq!(
            default_bytes(&T::schema()).unwrap(),
            encode(&T::default_value()),
            "default of {}",
            T::schema().name
        );
    }
    check::<bool>();
    check::<i32>();
    check::<f64>();
    check::<Vec3>();
    check::<Quat>();
    check::<wish_core::math::Mat4>();
    check::<Color>();
    check::<Entity>();
    check::<Text>();
    check::<[Quat; 3]>();
    check::<Shape>();
}

#[test]
fn enums_record_values_and_documentation() {
    let shape = Shape::schema();
    assert_eq!(shape.doc, "The shape of a collider.");
    assert_eq!((shape.size, shape.align), (1, 1));
    assert_eq!(
        shape.kind,
        TypeKind::Enum(vec![
            VariantSchema::new("Box", "An axis-aligned box.", 0),
            VariantSchema::new("Sphere", "", 1),
            VariantSchema::new("Capsule", "A cylinder with rounded ends.", 7),
        ])
    );
    assert_eq!(Shape::decode(&[7]), Some(Shape::Capsule));
    assert_eq!(Shape::decode(&[2]), None);
    assert_eq!(Shape::decode(&[]), None);
    assert_eq!(encode(&Shape::Capsule), [7]);
}

#[test]
fn encode_zeroes_padding_and_decode_checks_validity() {
    let mut texts = Texts::default();
    let mover = sample_mover(&mut texts);
    let bytes = encode(&mover);
    assert_eq!(Mover::decode(&bytes), Some(mover));

    // Bytes that belong to no field were written as zero, not left as 0xAA.
    let schema = Mover::schema();
    let TypeKind::Struct(fields) = &schema.kind else {
        panic!("Mover is a struct");
    };
    let mut covered = vec![false; bytes.len()];
    for field in fields {
        let start = field.offset as usize;
        covered[start..start + field.ty.byte_len()].fill(true);
    }
    let padding: Vec<usize> = (0..bytes.len()).filter(|index| !covered[*index]).collect();
    assert_eq!(padding.len(), 5);
    assert!(padding.iter().all(|index| bytes[*index] == 0));

    assert_eq!(validate_bytes(&schema, &bytes), Ok(()));

    let mut bad_bool = bytes.clone();
    bad_bool[core::mem::offset_of!(Mover, enabled)] = 2;
    assert_eq!(Mover::decode(&bad_bool), None);
    let problem = validate_bytes(&schema, &bad_bool).unwrap_err();
    assert_eq!(problem.code, "invalid_bytes");
    assert!(problem.message.contains("'enabled'"), "{}", problem.message);

    let mut bad_enum = bytes.clone();
    bad_enum[core::mem::offset_of!(Mover, shape)] = 9;
    assert_eq!(Mover::decode(&bad_enum), None);
    let problem = validate_bytes(&schema, &bad_enum).unwrap_err();
    assert!(problem.message.contains("'shape'"), "{}", problem.message);

    assert_eq!(Mover::decode(&bytes[1..]), None);
    assert_eq!(
        validate_bytes(&schema, &bytes[1..]).unwrap_err().code,
        "layout_mismatch"
    );
}

#[test]
fn json_form_of_a_component() {
    let mut texts = Texts::default();
    let mover = sample_mover(&mut texts);
    let value = write(&mover, &texts);
    assert_eq!(
        value,
        json!({
            "speed": 12.5,
            "enabled": false,
            "shape": "Capsule",
            "target": "12v3",
            "label": "hero",
            "waypoints": [[1.0, 2.0, 3.0], [0.1, -0.2, 0.3]],
            "lives": 9,
            "tint": [0.25, 0.5, 0.75, 1.0]
        })
    );
    assert_eq!(read::<Mover>(&value, &mut texts).unwrap(), mover);

    let unit = Unit {
        stats: Stats {
            health: 80,
            precise: 0.1,
        },
        team: [Shape::Box, Shape::Capsule, Shape::Sphere],
    };
    let value = write(&unit, &texts);
    assert_eq!(
        value,
        json!({ "stats": { "health": 80, "precise": 0.1 }, "team": ["Box", "Capsule", "Sphere"] })
    );
    assert_eq!(read::<Unit>(&value, &mut texts).unwrap(), unit);

    assert_eq!(write(&Player, &texts), json!({}));
    assert_eq!(write(&Entity::NULL, &texts), json!("null"));
}

#[test]
fn a_new_value_starts_from_defaults_and_a_patch_keeps_the_rest() {
    let mut texts = Texts::default();
    let mover: Mover = read(&json!({ "speed": 2 }), &mut texts).unwrap();
    assert_eq!(
        mover,
        Mover {
            speed: 2.0,
            ..Mover::default()
        }
    );

    let original = sample_mover(&mut texts);
    let mut bytes = encode(&original);
    let patch = json!({ "lives": 1, "waypoints": [[9.0, 9.0, 9.0], [8.0, 8.0, 8.0]] });
    apply_json(
        &Mover::schema(),
        &patch,
        &mut bytes,
        &mut JsonContext::new(&mut texts),
    )
    .unwrap();
    assert_eq!(
        Mover::decode(&bytes),
        Some(Mover {
            lives: 1,
            waypoints: [Vec3::splat(9.0), Vec3::splat(8.0)],
            ..original
        })
    );
}

#[test]
fn unknown_fields_say_what_was_meant() {
    let aliased = read_error::<Transform>(json!({ "position": [0, 1, 0] }));
    assert_eq!(aliased.code, "unknown_field");
    assert_eq!(aliased.message, "'Transform' has no field 'position'.");
    assert_eq!(aliased.hint, "Did you mean 'translation'?");
    assert_eq!(aliased.candidates, ["translation", "rotation", "scale"]);
    assert_eq!(aliased.location.path.as_deref(), Some("/position"));

    let typo = read_error::<Transform>(json!({ "translaton": [0, 1, 0] }));
    assert_eq!(typo.hint, "Did you mean 'translation'?");

    let unrelated = read_error::<Transform>(json!({ "color": [0, 1, 0] }));
    assert_eq!(
        unrelated.hint,
        "Valid names: 'rotation', 'scale', 'translation'."
    );

    // Inside a larger document, the location continues from where the value sits.
    let mut texts = Texts::default();
    let mut ctx = JsonContext::at_path(&mut texts, "/entities/player/components/Transform");
    let located = from_json(
        &Transform::schema(),
        &json!({ "position": [0, 1, 0] }),
        &mut ctx,
    )
    .unwrap_err();
    assert_eq!(
        located.location.path.as_deref(),
        Some("/entities/player/components/Transform/position")
    );
}

#[test]
fn rejected_values_say_what_is_allowed() {
    let range = read_error::<Mover>(json!({ "speed": 500 }));
    assert_eq!(range.code, "value_out_of_range");
    assert_eq!(
        range.message,
        "Mover.speed = 500 is outside the allowed range 0..=50 (m/s)."
    );
    assert_eq!(range.location.path.as_deref(), Some("/speed"));

    let integer_range = read_error::<Mover>(json!({ "lives": 10 }));
    assert_eq!(
        integer_range.message,
        "Mover.lives = 10 is outside the allowed range 0..=9."
    );

    let too_big_for_type = read_error::<Mover>(json!({ "lives": 300 }));
    assert_eq!(too_big_for_type.code, "value_out_of_range");
    assert_eq!(
        too_big_for_type.message,
        "Mover.lives = 300 does not fit in u8, which holds 0..=255."
    );

    let fraction = read_error::<Mover>(json!({ "lives": 1.5 }));
    assert_eq!(fraction.code, "type_mismatch");
    assert_eq!(
        fraction.message,
        "Expected an integer for 'Mover.lives', found the number 1.5."
    );

    let too_big_for_f32 = read_error::<Damage>(json!({ "amount": 1e39 }));
    assert_eq!(too_big_for_f32.code, "value_out_of_range");
    assert!(
        too_big_for_f32.message.contains("too large for f32"),
        "{}",
        too_big_for_f32.message
    );

    let wrong_type = read_error::<Mover>(json!({ "speed": "fast" }));
    assert_eq!(wrong_type.code, "type_mismatch");
    assert_eq!(
        wrong_type.message,
        "Expected a number for 'Mover.speed', found a string."
    );
    assert_eq!(
        wrong_type.hint,
        "'Mover.speed' has type f32, which is written as a number."
    );

    let not_an_object = read_error::<Mover>(json!(5));
    assert_eq!(
        not_an_object.message,
        "Expected an object for 'Mover', found a number."
    );
    assert_eq!(not_an_object.location.path, None);

    let wrong_length = read_error::<Transform>(json!({ "translation": [1, 2] }));
    assert_eq!(wrong_length.code, "wrong_length");
    assert_eq!(
        wrong_length.message,
        "Expected 3 items for 'Transform.translation', found 2."
    );
    assert_eq!(
        wrong_length.hint,
        "'Transform.translation' has type Vec3, which is written as an array [x, y, z]."
    );

    let nested = read_error::<Mover>(json!({ "waypoints": [[0, 0, 0], [0, "x", 0]] }));
    assert_eq!(
        nested.message,
        "Expected a number for 'Mover.waypoints[1][1]', found a string."
    );
    assert_eq!(nested.location.path.as_deref(), Some("/waypoints/1/1"));

    let variant = read_error::<Mover>(json!({ "shape": "Spere" }));
    assert_eq!(variant.code, "unknown_variant");
    assert_eq!(variant.message, "'Spere' is not a variant of 'Shape'.");
    assert_eq!(variant.hint, "Did you mean 'Sphere'?");
    assert_eq!(variant.candidates, ["Box", "Sphere", "Capsule"]);
    assert_eq!(variant.location.path.as_deref(), Some("/shape"));

    let entity = read_error::<Mover>(json!({ "target": "twelve" }));
    assert_eq!(entity.code, "invalid_entity_id");
    assert_eq!(entity.location.path.as_deref(), Some("/target"));

    let no_world = from_json(
        &Mover::schema(),
        &json!({ "label": "hero" }),
        &mut JsonContext::new(&mut NoTexts),
    )
    .unwrap_err();
    assert_eq!(no_world.code, "text_unavailable");
    assert_eq!(no_world.location.path.as_deref(), Some("/label"));

    for diagnostic in [
        range,
        fraction,
        wrong_type,
        wrong_length,
        variant,
        entity,
        no_world,
    ] {
        assert!(
            !diagnostic.hint.is_empty(),
            "{} has a hint",
            diagnostic.code
        );
    }
}

#[test]
fn non_finite_numbers_can_be_inspected_but_not_written() {
    let texts = Texts::default();
    let damage = |amount| Damage {
        target: Entity::NULL,
        amount,
    };
    assert_eq!(write(&damage(f32::NAN), &texts)["amount"], json!("NaN"));
    assert_eq!(
        write(&damage(f32::INFINITY), &texts)["amount"],
        json!("Infinity")
    );
    assert_eq!(
        write(&damage(f32::NEG_INFINITY), &texts)["amount"],
        json!("-Infinity")
    );

    let rejected = read_error::<Damage>(json!({ "amount": "NaN" }));
    assert_eq!(rejected.code, "type_mismatch");
}

#[test]
fn json_schema_describes_the_json_form() {
    assert_eq!(
        json_schema(&Mover::schema()),
        json!({
            "type": "object",
            "title": "Mover",
            "x-wish-type": "Mover",
            "description": "Moves an entity at a constant speed.",
            "additionalProperties": false,
            "properties": {
                "speed": {
                    "type": "number",
                    "x-wish-type": "f32",
                    "description": "Movement speed.",
                    "default": 5.0,
                    "minimum": 0.0,
                    "maximum": 50.0,
                    "x-unit": "m/s"
                },
                "enabled": {
                    "type": "boolean",
                    "x-wish-type": "bool",
                    "description": "Whether the mover is active.",
                    "default": true
                },
                "shape": {
                    "oneOf": [
                        { "const": "Box", "description": "An axis-aligned box." },
                        { "const": "Sphere" },
                        { "const": "Capsule", "description": "A cylinder with rounded ends." }
                    ],
                    "title": "Shape",
                    "x-wish-type": "Shape",
                    "description": "The shape of a collider.",
                    "default": "Box"
                },
                "target": {
                    "type": "string",
                    "pattern": "^(null|(0|[1-9][0-9]*)v(0|[1-9][0-9]*))$",
                    "x-wish-type": "Entity",
                    "description": "The entity to move towards.",
                    "default": "null"
                },
                "label": {
                    "type": "string",
                    "x-wish-type": "Text",
                    "default": ""
                },
                "waypoints": {
                    "type": "array",
                    "items": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "x-wish-type": "Vec3"
                    },
                    "minItems": 2,
                    "maxItems": 2,
                    "x-wish-type": "[Vec3; 2]",
                    "default": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]
                },
                "lives": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 9,
                    "x-wish-type": "u8",
                    "default": 3
                },
                "tint": {
                    "type": "array",
                    "items": { "type": "number" },
                    "minItems": 4,
                    "maxItems": 4,
                    "x-wish-type": "Color",
                    "default": [1.0, 1.0, 1.0, 1.0]
                }
            }
        })
    );

    let translation = &json_schema(&Transform::schema())["properties"]["translation"];
    assert_eq!(translation["x-unit"], json!("m"));
    assert_eq!(translation["x-aliases"], json!(["position"]));
}

/// Schemas from untrusted sources (script modules) must not be able to make
/// any walk over a value run for ever, allocate without bound or overflow the
/// stack. Each of these is rejected quickly by `validate`.
#[test]
fn hostile_schemas_are_rejected_quickly() {
    let empty = TypeSchema::structure("Empty", "", 0, 1, Vec::new());
    let zero_size_array = TypeSchema::array(empty, u32::MAX as usize, 0, 1);
    let nested_zero_size = TypeSchema::array(zero_size_array.clone(), u32::MAX as usize, 0, 1);
    let huge = TypeSchema::array(u8::schema(), 1 << 21, 1 << 21, 1);
    let mut deep = u8::schema();
    for _ in 0..40 {
        deep = TypeSchema::array(deep, 1, 1, 1);
    }
    let wide = TypeSchema::array(
        TypeSchema::array(u8::schema(), 1024, 1024, 1),
        1024,
        1 << 20,
        1,
    );
    for (case, schema) in [
        ("zero-size elements", zero_size_array),
        ("nested zero-size elements", nested_zero_size),
        ("too large", huge),
        ("too deep", deep),
        ("too many nodes", wide),
    ] {
        let started = std::time::Instant::now();
        let problem = schema.validate().expect_err(case);
        assert_eq!(problem.code, "invalid_schema", "{case}");
        assert!(
            started.elapsed().as_secs() < 1,
            "{case} took too long to reject"
        );
    }
}

#[test]
fn same_layout_ignores_documentation_but_not_layout() {
    let original = Mover::schema();
    let mut documented = original.clone();
    documented.doc = "Other words.".to_owned();
    if let TypeKind::Struct(fields) = &mut documented.kind {
        fields[0].unit = Some("km/h".to_owned());
        fields[0].default = None;
        fields[0].range = None;
    }
    assert!(original.same_layout(&documented));

    let mut retyped = original.clone();
    if let TypeKind::Struct(fields) = &mut retyped.kind {
        // `enabled` is a bool; a u8 has the same size but other valid values.
        let enabled = fields
            .iter_mut()
            .find(|field| field.name == "enabled")
            .unwrap();
        enabled.ty = u8::schema();
    }
    assert!(!original.same_layout(&retyped));
    assert!(!Shape::schema().same_layout(&u8::schema()));
}

#[test]
fn inconsistent_schemas_are_rejected() {
    let overlapping = TypeSchema::structure(
        "Bad",
        "",
        8,
        4,
        vec![
            FieldSchema::new("a", "", 0, f32::schema()),
            FieldSchema::new("b", "", 2, u16::schema()),
        ],
    );
    let past_the_end = TypeSchema::structure(
        "Bad",
        "",
        4,
        4,
        vec![FieldSchema::new("a", "", 4, f32::schema())],
    );
    let misaligned = TypeSchema::structure(
        "Bad",
        "",
        8,
        4,
        vec![FieldSchema::new("a", "", 2, f32::schema())],
    );
    let under_aligned = TypeSchema::structure(
        "Bad",
        "",
        8,
        4,
        vec![FieldSchema::new("a", "", 0, f64::schema())],
    );
    let range_on_bool = TypeSchema::structure(
        "Bad",
        "",
        1,
        1,
        vec![FieldSchema::new("a", "", 0, bool::schema()).with_range(0.0, 1.0)],
    );
    let empty_range = TypeSchema::structure(
        "Bad",
        "",
        4,
        4,
        vec![FieldSchema::new("a", "", 0, f32::schema()).with_range(2.0, 1.0)],
    );
    let default_out_of_range = TypeSchema::structure(
        "Bad",
        "",
        4,
        4,
        vec![
            FieldSchema::new("a", "", 0, f32::schema())
                .with_range(0.0, 1.0)
                .with_default(json!(5.0)),
        ],
    );
    let default_of_wrong_type = TypeSchema::structure(
        "Bad",
        "",
        4,
        4,
        vec![FieldSchema::new("a", "", 0, f32::schema()).with_default(json!("five"))],
    );
    let repeated_field = TypeSchema::structure(
        "Bad",
        "",
        8,
        4,
        vec![
            FieldSchema::new("a", "", 0, f32::schema()),
            FieldSchema::new("a", "", 4, f32::schema()),
        ],
    );
    let alias_is_a_field = TypeSchema::structure(
        "Bad",
        "",
        8,
        4,
        vec![
            FieldSchema::new("a", "", 0, f32::schema()).with_alias("b"),
            FieldSchema::new("b", "", 4, f32::schema()),
        ],
    );
    let repeated_enum_value = TypeSchema::enumeration(
        "Bad",
        "",
        vec![
            VariantSchema::new("A", "", 0),
            VariantSchema::new("B", "", 0),
        ],
    );
    let empty_enum = TypeSchema::enumeration("Bad", "", Vec::new());
    let wrong_primitive_size = TypeSchema {
        size: 8,
        ..f32::schema()
    };
    let bad_alignment = TypeSchema {
        align: 3,
        size: 3,
        ..TypeSchema::structure("Bad", "", 3, 3, Vec::new())
    };
    let size_not_multiple_of_alignment = TypeSchema::structure("Bad", "", 6, 4, Vec::new());
    let wrong_array_size = TypeSchema::array(f32::schema(), 3, 8, 4);
    let unnamed = TypeSchema::structure("", "", 0, 1, Vec::new());

    for (case, schema) in [
        ("overlapping fields", overlapping),
        ("field past the end", past_the_end),
        ("misaligned field", misaligned),
        ("under-aligned struct", under_aligned),
        ("range on a bool", range_on_bool),
        ("empty range", empty_range),
        ("default out of range", default_out_of_range),
        ("default of the wrong type", default_of_wrong_type),
        ("repeated field name", repeated_field),
        ("alias that is a field", alias_is_a_field),
        ("repeated enum value", repeated_enum_value),
        ("empty enum", empty_enum),
        ("wrong primitive size", wrong_primitive_size),
        ("unsupported alignment", bad_alignment),
        (
            "size not a multiple of alignment",
            size_not_multiple_of_alignment,
        ),
        ("wrong array size", wrong_array_size),
        ("unnamed type", unnamed),
    ] {
        let problem = schema.validate().expect_err(case);
        assert_eq!(problem.code, "invalid_schema", "{case}");
        assert!(problem.location.path.is_some(), "{case} has a location");
    }
}

#[test]
fn fingerprints_follow_layout_not_documentation() {
    let original = Transform::schema();
    assert_eq!(fingerprint(&original), fingerprint(&Transform::schema()));
    assert_ne!(fingerprint(&original), fingerprint(&Mover::schema()));

    let mut documented = original.clone();
    documented.doc = "Something else.".to_owned();
    if let TypeKind::Struct(fields) = &mut documented.kind {
        fields[0].doc = "Changed.".to_owned();
        fields[0].unit = Some("cm".to_owned());
        fields[0].aliases.clear();
        fields[2].default = Some(json!([2.0, 2.0, 2.0]));
    }
    assert_eq!(fingerprint(&documented), fingerprint(&original));

    let mut renamed = original.clone();
    if let TypeKind::Struct(fields) = &mut renamed.kind {
        fields[0].name = "position".to_owned();
    }
    assert_ne!(fingerprint(&renamed), fingerprint(&original));

    let mut moved = original.clone();
    if let TypeKind::Struct(fields) = &mut moved.kind {
        fields.swap(0, 2);
        fields[0].offset = 0;
        fields[2].offset = 28;
    }
    assert_ne!(fingerprint(&moved), fingerprint(&original));

    let mut renumbered = Shape::schema();
    if let TypeKind::Enum(variants) = &mut renumbered.kind {
        variants[1].value = 2;
    }
    assert_ne!(fingerprint(&renumbered), fingerprint(&Shape::schema()));
}

fn through_text(value: &Value) -> Value {
    let text = serde_json::to_string(value).expect("JSON values serialize");
    serde_json::from_str(&text).expect("serialized JSON parses")
}

proptest! {
    /// Every finite f32 survives bytes -> JSON -> text -> JSON -> bytes exactly.
    #[test]
    fn finite_f32_round_trips_bit_for_bit(bits in any::<u32>()) {
        let value = f32::from_bits(bits);
        prop_assume!(value.is_finite());
        let schema = f32::schema();
        let json = to_json(&schema, &value.to_le_bytes(), &NoTexts).unwrap();
        let bytes = from_json(&schema, &through_text(&json), &mut JsonContext::new(&mut NoTexts)).unwrap();
        prop_assert_eq!(bytes, value.to_le_bytes().to_vec());
    }

    /// Every finite f64 survives the same trip exactly.
    #[test]
    fn finite_f64_round_trips_bit_for_bit(bits in any::<u64>()) {
        let value = f64::from_bits(bits);
        prop_assume!(value.is_finite());
        let schema = f64::schema();
        let json = to_json(&schema, &value.to_le_bytes(), &NoTexts).unwrap();
        let bytes = from_json(&schema, &through_text(&json), &mut JsonContext::new(&mut NoTexts)).unwrap();
        prop_assert_eq!(bytes, value.to_le_bytes().to_vec());
    }

    /// Integers of every width survive the trip.
    #[test]
    fn integers_round_trip(small in any::<i8>(), wide in any::<i64>(), unsigned in any::<u64>()) {
        for (schema, bytes) in [
            (i8::schema(), small.to_le_bytes().to_vec()),
            (i64::schema(), wide.to_le_bytes().to_vec()),
            (u64::schema(), unsigned.to_le_bytes().to_vec()),
        ] {
            let json = to_json(&schema, &bytes, &NoTexts).unwrap();
            let back = from_json(&schema, &through_text(&json), &mut JsonContext::new(&mut NoTexts)).unwrap();
            prop_assert_eq!(back, bytes);
        }
    }

    /// A whole component survives the trip, text and padding included.
    #[test]
    fn components_round_trip(
        speed in 0.0f32..=50.0,
        enabled: bool,
        shape in 0usize..3,
        index: u32,
        generation: u32,
        coordinates in prop::array::uniform6(-1.0e6f32..1.0e6),
        lives in 0u8..=9,
        tint in prop::array::uniform4(0.0f32..4.0),
        label in "[a-z]{0,8}",
    ) {
        let mut texts = Texts::default();
        let [ax, ay, az, bx, by, bz] = coordinates;
        let value = Mover {
            speed,
            enabled,
            shape: [Shape::Box, Shape::Sphere, Shape::Capsule][shape],
            target: Entity::new(index, generation),
            label: texts.intern(&label).unwrap(),
            waypoints: [Vec3::new(ax, ay, az), Vec3::new(bx, by, bz)],
            lives,
            tint: Color::from_array(tint),
        };
        let json = through_text(&write(&value, &texts));
        let bytes = from_json(&Mover::schema(), &json, &mut JsonContext::new(&mut texts)).unwrap();
        prop_assert_eq!(&bytes, &encode(&value));
        prop_assert_eq!(Mover::decode(&bytes), Some(value));
    }
}
