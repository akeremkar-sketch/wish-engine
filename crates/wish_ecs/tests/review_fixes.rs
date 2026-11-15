//! Regression tests for the findings of the M1 closing review.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::Text;
use wish_core::math::Vec3;
use wish_ecs::{Changed, World};
use wish_schema::{FieldSchema, Plain, TypeKind, TypeSchema, component};

#[component]
pub struct Label {
    pub text: Text,
    pub size: f32,
}

#[component]
pub struct Flag {
    pub on: bool,
}

#[component]
pub struct Position {
    pub value: Vec3,
}

/// Finding 2: a rejected JSON value must not grow the text table.
#[test]
fn rejected_json_leaves_the_text_table_and_hash_alone() {
    let mut world = World::new();
    world.register_component::<Label>().unwrap();
    let entity = world.spawn().unwrap();
    let before = world.hash();
    let texts_before = world.texts().len();

    // "text" sorts before "zz", so it is converted first; "zz" then fails.
    let error = world
        .set_json(entity, "Label", &json!({ "text": "new", "zz": 0 }))
        .unwrap_err();
    assert_eq!(error.code, "unknown_field");
    assert_eq!(world.texts().len(), texts_before);
    assert_eq!(world.hash(), before);

    // An accepted value commits its strings with the handles it was given.
    world
        .set_json(entity, "Label", &json!({ "text": "new", "size": 2 }))
        .unwrap();
    let label = *world.get::<Label>(entity).unwrap();
    assert_eq!(world.text(label.text), Some("new"));
    assert_eq!(world.get_json(entity, "Label").unwrap()["text"], "new");
}

/// Finding 4: a world built from schemas behaves like one built from Rust
/// types: typed lookups find components registered by schema alone when the
/// layout matches exactly.
#[test]
fn schema_registered_components_are_found_by_typed_access() {
    let mut native = World::new();
    native.register_component::<Position>().unwrap();
    let mut from_schema = World::new();
    from_schema
        .register_component_schema(Position::schema())
        .unwrap();
    assert_eq!(native.hash(), from_schema.hash());

    for world in [&mut native, &mut from_schema] {
        let entity = world.spawn().unwrap();
        world
            .set_json(entity, "Position", &json!({ "value": [1, 2, 3] }))
            .unwrap();
    }
    assert_eq!(native.hash(), from_schema.hash());
    for world in [&mut native, &mut from_schema] {
        assert_eq!(world.query::<&Position>().unwrap().count(), 1);
        assert_eq!(
            world
                .query_filtered::<&Position, Changed<Position>>()
                .unwrap()
                .count(),
            1
        );
        let entity = world.entities().next().unwrap();
        assert_eq!(
            world.get::<Position>(entity).unwrap().value,
            Vec3::new(1.0, 2.0, 3.0)
        );
        world.get_mut::<Position>(entity).unwrap().value.x = 5.0;
    }
    assert_eq!(native.hash(), from_schema.hash());
}

/// Finding 3: binding compares layouts structurally. A schema with the same
/// name and size but a `u8` where the Rust type has a `bool` must never be
/// viewed as that Rust type, whatever its fingerprint.
#[test]
fn a_look_alike_schema_is_never_viewed_as_the_rust_type() {
    let mut world = World::new();
    let mut look_alike = Flag::schema();
    if let TypeKind::Struct(fields) = &mut look_alike.kind {
        fields[0] = FieldSchema::new("on", "", 0, u8::schema());
    }
    let id = world.register_component_schema(look_alike).unwrap();
    let entity = world.spawn().unwrap();
    world.insert_bytes(entity, id, &[2]).unwrap();

    assert!(
        world.get::<Flag>(entity).is_none(),
        "typed access must not see a 2 as a bool"
    );
    assert_eq!(world.query::<&Flag>().unwrap().count(), 0);
    assert_eq!(
        world.register_component::<Flag>().unwrap_err().code,
        "type_name_conflict"
    );
    assert_eq!(world.get_json(entity, "Flag").unwrap(), json!({ "on": 2 }));
}

/// Finding 7: raw bytes cannot smuggle in a text handle the world never issued.
#[test]
fn insert_bytes_rejects_unknown_text_handles() {
    let mut world = World::new();
    let id = world.register_component::<Label>().unwrap();
    let entity = world.spawn().unwrap();

    let mut bytes = vec![0u8; size_of::<Label>()];
    Label {
        text: Text::from_index(999),
        size: 1.0,
    }
    .encode(&mut bytes);
    let error = world.insert_bytes(entity, id, &bytes).unwrap_err();
    assert_eq!(error.code, "invalid_text_handle");
    assert!(!world.has::<Label>(entity));

    let known = world.intern("ok").unwrap();
    Label {
        text: known,
        size: 1.0,
    }
    .encode(&mut bytes);
    world.insert_bytes(entity, id, &bytes).unwrap();
    assert_eq!(world.get_json(entity, "Label").unwrap()["text"], "ok");
}

/// Finding 1, through the world: a hostile schema is refused at registration
/// and nothing walks it.
#[test]
fn hostile_schemas_are_refused_at_registration() {
    let mut world = World::new();
    let empty = TypeSchema::structure("Empty", "", 0, 1, Vec::new());
    let hostile = TypeSchema::structure(
        "Hostile",
        "",
        0,
        1,
        vec![FieldSchema::new(
            "items",
            "",
            0,
            TypeSchema::array(empty, u32::MAX as usize, 0, 1),
        )],
    );
    assert_eq!(
        world.register_component_schema(hostile).unwrap_err().code,
        "invalid_schema"
    );
    assert!(world.component_id("Hostile").is_err());
}
