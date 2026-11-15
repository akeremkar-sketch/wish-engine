//! Behaviour of the world through its public API.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::math::{Quat, Vec3};
use wish_core::{Entity, Text};
use wish_ecs::{With, Without, World};
use wish_schema::{FieldSchema, Plain, TypeSchema, component, resource, schema};

#[component]
pub struct Position {
    #[schema(alias = "translation")]
    pub value: Vec3,
}

#[component]
pub struct Velocity {
    pub value: Vec3,
}

#[schema]
pub enum Team {
    Red,
    Blue,
}

/// Has padding: bool and enum (1 byte each), then 6 bytes, then an f64.
#[component]
pub struct Health {
    #[schema(default = true)]
    pub alive: bool,
    pub team: Team,
    #[schema(range = 0.0..=100.0, default = 100.0)]
    pub points: f64,
}

#[component]
pub struct Player;

#[component]
pub struct Name {
    pub label: Text,
}

#[component]
pub struct Spin {
    pub rotation: Quat,
}

#[resource]
pub struct Gravity {
    #[schema(default = Vec3::new(0.0, -9.81, 0.0))]
    pub value: Vec3,
}

fn error_code<T>(result: wish_core::Result<T>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(diagnostic) => diagnostic.code.to_string(),
    }
}

#[test]
fn spawn_insert_get_remove_despawn() {
    let mut world = World::new();
    let a = world.spawn().unwrap();
    let b = world.spawn().unwrap();
    assert_eq!((a, b), (Entity::new(0, 0), Entity::new(1, 0)));
    assert_eq!(world.entity_count(), 2);

    world.insert(a, Position { value: Vec3::X }).unwrap();
    world.insert(a, Velocity { value: Vec3::Y }).unwrap();
    world.insert(b, Velocity { value: Vec3::Z }).unwrap();
    assert_eq!(world.get::<Position>(a).unwrap().value, Vec3::X);
    assert_eq!(world.get::<Velocity>(a).unwrap().value, Vec3::Y);
    assert_eq!(world.get::<Velocity>(b).unwrap().value, Vec3::Z);
    assert!(world.get::<Position>(b).is_none());
    assert!(!world.has::<Player>(a));

    // Overwriting keeps the entity where it is.
    world.insert(a, Position { value: Vec3::NEG_X }).unwrap();
    assert_eq!(world.get::<Position>(a).unwrap().value, Vec3::NEG_X);
    world.get_mut::<Position>(a).unwrap().value.y = 5.0;
    assert_eq!(
        world.get::<Position>(a).unwrap().value,
        Vec3::new(-1.0, 5.0, 0.0)
    );

    assert_eq!(
        world.remove::<Position>(a).unwrap().map(|p| p.value),
        Some(Vec3::new(-1.0, 5.0, 0.0))
    );
    assert_eq!(world.remove::<Position>(a).unwrap(), None);
    assert_eq!(world.remove::<Player>(a).unwrap(), None, "never registered");
    assert_eq!(
        world.get::<Velocity>(a).unwrap().value,
        Vec3::Y,
        "other components survive the move"
    );

    world.despawn(a).unwrap();
    assert!(!world.contains(a));
    assert!(world.get::<Velocity>(a).is_none());
    assert_eq!(world.get::<Velocity>(b).unwrap().value, Vec3::Z);
    assert_eq!(error_code(world.despawn(a)), "entity_not_found");
    assert_eq!(error_code(world.insert(a, Player)), "entity_not_found");
    assert_eq!(error_code(world.remove::<Velocity>(a)), "entity_not_found");

    // The index is reused with a new generation; the old handle stays dead.
    let c = world.spawn().unwrap();
    assert_eq!(c, Entity::new(0, 1));
    assert!(!world.contains(a));
    assert!(world.get::<Velocity>(c).is_none());
}

#[test]
fn swap_remove_keeps_moved_entities_reachable() {
    let mut world = World::new();
    let entities: Vec<Entity> = (0..5)
        .map(|index| {
            let entity = world.spawn().unwrap();
            world
                .insert(
                    entity,
                    Position {
                        value: Vec3::splat(index as f32),
                    },
                )
                .unwrap();
            entity
        })
        .collect();
    // Removing from the middle moves the last row into the hole.
    world.despawn(entities[1]).unwrap();
    world.remove::<Position>(entities[0]).unwrap();
    for (index, entity) in entities.iter().enumerate().skip(2) {
        assert_eq!(
            world.get::<Position>(*entity).unwrap().value,
            Vec3::splat(index as f32)
        );
    }
    assert!(world.contains(entities[0]));
    assert_eq!(world.components_of(entities[0]).unwrap(), []);
}

#[test]
fn queries_visit_matching_entities_in_a_stable_order() {
    let mut world = World::new();
    let mut spawned = Vec::new();
    for index in 0..6 {
        let entity = world.spawn().unwrap();
        world
            .insert(
                entity,
                Position {
                    value: Vec3::splat(index as f32),
                },
            )
            .unwrap();
        if index % 2 == 0 {
            world.insert(entity, Velocity { value: Vec3::X }).unwrap();
        }
        if index % 3 == 0 {
            world.insert(entity, Player).unwrap();
        }
        spawned.push(entity);
    }

    for (mut position, velocity) in world.query::<(&mut Position, &Velocity)>().unwrap() {
        position.value += velocity.value;
    }
    let moved: Vec<(Entity, Vec3)> = world
        .query::<(Entity, &Position)>()
        .unwrap()
        .map(|(entity, position)| (entity, position.value))
        .collect();
    assert_eq!(moved.len(), 6);
    for (entity, value) in &moved {
        let position = spawned.iter().position(|e| e == entity).unwrap();
        let index = position as f32;
        let expected = if position.is_multiple_of(2) {
            Vec3::new(index + 1.0, index, index)
        } else {
            Vec3::splat(index)
        };
        assert_eq!(*value, expected);
    }

    // Archetypes in creation order, rows in arrival order: the same every run.
    let order: Vec<Entity> = world.query::<Entity>().unwrap().collect();
    let again: Vec<Entity> = world.query::<Entity>().unwrap().collect();
    assert_eq!(order, again);
    assert_eq!(order.len(), 6);

    let players: Vec<Entity> = world
        .query_filtered::<Entity, With<Player>>()
        .unwrap()
        .collect();
    assert_eq!(players, [spawned[0], spawned[3]]);
    let still: Vec<Entity> = world
        .query_filtered::<Entity, (With<Position>, Without<Velocity>)>()
        .unwrap()
        .collect();
    let mut still_sorted = still.clone();
    still_sorted.sort();
    assert_eq!(still_sorted, [spawned[1], spawned[3], spawned[5]]);

    assert_eq!(world.query::<&Position>().unwrap().count(), 6);
    assert_eq!(world.query::<&Player>().unwrap().count(), 2);
}

#[test]
fn unregistered_components_match_nothing() {
    let mut world = World::new();
    let entity = world.spawn().unwrap();
    world
        .insert(entity, Position { value: Vec3::ZERO })
        .unwrap();
    assert_eq!(world.query::<&Spin>().unwrap().count(), 0);
    assert_eq!(world.query::<(&Position, &Spin)>().unwrap().count(), 0);
    assert_eq!(
        world
            .query_filtered::<&Position, With<Spin>>()
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        world
            .query_filtered::<&Position, Without<Spin>>()
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn aliasing_queries_are_rejected() {
    let mut world = World::new();
    assert_eq!(
        error_code(world.query::<(&mut Position, &Position)>()),
        "conflicting_query_access"
    );
    assert_eq!(
        error_code(world.query::<(&mut Position, &mut Position)>()),
        "conflicting_query_access"
    );
    assert!(
        world.query::<(&Position, &Position)>().is_ok(),
        "two shared reads are fine"
    );
    assert!(world.query::<(&mut Position, &mut Velocity)>().is_ok());
}

#[test]
fn padded_components_round_trip_through_every_path() {
    let mut world = World::new();
    let entity = world.spawn().unwrap();
    world
        .insert(
            entity,
            Health {
                alive: false,
                team: Team::Blue,
                points: 42.5,
            },
        )
        .unwrap();
    world.insert(entity, Player).unwrap();
    // A whole-value assignment through `&mut` may leave padding uninitialised;
    // reading the component back as bytes must still work.
    *world.get_mut::<Health>(entity).unwrap() = Health {
        alive: true,
        team: Team::Red,
        points: 7.0,
    };
    for mut health in world.query::<&mut Health>().unwrap() {
        *health = Health {
            points: health.points + 1.0,
            ..*health
        };
    }
    let id = world.component_id("Health").unwrap();
    let bytes = world.component_bytes(entity, id).unwrap().unwrap();
    assert_eq!(
        Health::decode(&bytes),
        Some(Health {
            alive: true,
            team: Team::Red,
            points: 8.0
        })
    );
    assert_eq!(bytes[2..8], [0; 6], "padding reads as zero");
    assert_eq!(
        world.get_json(entity, "Health").unwrap(),
        json!({ "alive": true, "team": "Red", "points": 8.0 })
    );
    // Moving between archetypes copies the value along.
    world.remove::<Player>(entity).unwrap();
    assert_eq!(world.get::<Health>(entity).unwrap().points, 8.0);
}

#[test]
fn json_access_creates_patches_and_explains_mistakes() {
    let mut world = World::new();
    world.register_component::<Position>().unwrap();
    world.register_component::<Health>().unwrap();
    world.register_component::<Name>().unwrap();
    let entity = world.spawn().unwrap();

    world
        .set_json(entity, "Health", &json!({ "points": 30 }))
        .unwrap();
    assert_eq!(
        world.get::<Health>(entity),
        Some(&Health {
            alive: true,
            team: Team::Red,
            points: 30.0
        }),
        "fields left out take their defaults"
    );
    world
        .set_json(entity, "Health", &json!({ "team": "Blue" }))
        .unwrap();
    assert_eq!(
        world.get::<Health>(entity),
        Some(&Health {
            alive: true,
            team: Team::Blue,
            points: 30.0
        }),
        "an existing component is patched"
    );

    let rejected = world
        .set_json(entity, "Health", &json!({ "points": 500 }))
        .unwrap_err();
    assert_eq!(rejected.code, "value_out_of_range");
    assert_eq!(
        world.get::<Health>(entity).unwrap().points,
        30.0,
        "a rejected patch changes nothing"
    );

    let unknown = world.set_json(entity, "Helth", &json!({})).unwrap_err();
    assert_eq!(unknown.code, "unknown_component");
    assert_eq!(unknown.hint, "Did you mean 'Health'?");

    let alias = world
        .set_json(entity, "Position", &json!({ "translation": [1, 2, 3] }))
        .unwrap_err();
    assert_eq!(alias.code, "unknown_field");
    assert_eq!(alias.hint, "Did you mean 'value'?");
    assert!(
        world.get::<Position>(entity).is_none(),
        "a rejected value adds nothing"
    );

    let missing = world.get_json(entity, "Position").unwrap_err();
    assert_eq!(missing.code, "component_not_found");
    assert!(!missing.hint.is_empty());

    world
        .set_json(entity, "Name", &json!({ "label": "hero" }))
        .unwrap();
    let label = world.get::<Name>(entity).unwrap().label;
    assert_eq!(world.text(label), Some("hero"));
    assert_eq!(
        world.intern("hero").unwrap(),
        label,
        "the same string gets the same handle"
    );

    assert_eq!(
        world.entity_json(entity).unwrap(),
        json!({
            "Health": { "alive": true, "team": "Blue", "points": 30.0 },
            "Name": { "label": "hero" }
        })
    );

    assert!(world.remove_json(entity, "Name").unwrap());
    assert!(!world.remove_json(entity, "Name").unwrap());
    assert_eq!(
        error_code(world.get_json(Entity::new(9, 0), "Health")),
        "entity_not_found"
    );
}

#[test]
fn schema_only_components_work_by_name_and_bytes() {
    let mut world = World::new();
    let schema = TypeSchema::structure(
        "Score",
        "Points scored.",
        8,
        4,
        vec![
            FieldSchema::new("value", "", 0, u32::schema()),
            FieldSchema::new("visible", "", 4, bool::schema()).with_default(json!(true)),
        ],
    );
    let id = world.register_component_schema(schema.clone()).unwrap();
    assert_eq!(world.register_component_schema(schema).unwrap(), id);
    assert!(!world.registry().component(id).unwrap().is_native());

    let entity = world.spawn().unwrap();
    world
        .set_json(entity, "Score", &json!({ "value": 12 }))
        .unwrap();
    assert_eq!(
        world.get_json(entity, "Score").unwrap(),
        json!({ "value": 12, "visible": true })
    );

    let mut bytes = world.component_bytes(entity, id).unwrap().unwrap();
    bytes[4] = 2; // not a valid bool
    let invalid = world.insert_bytes(entity, id, &bytes).unwrap_err();
    assert_eq!(invalid.code, "invalid_bytes");
    bytes[4] = 0;
    world.insert_bytes(entity, id, &bytes).unwrap();
    assert_eq!(
        world.get_json(entity, "Score").unwrap(),
        json!({ "value": 12, "visible": false })
    );
    assert_eq!(
        error_code(world.insert_bytes(entity, id, &bytes[..3])),
        "layout_mismatch"
    );
}

#[test]
fn resources() {
    let mut world = World::new();
    assert!(world.resource::<Gravity>().is_none());
    assert!(world.resource_json("Gravity").is_err());
    world.insert_resource(Gravity::default()).unwrap();
    assert_eq!(
        world.resource::<Gravity>().unwrap().value,
        Vec3::new(0.0, -9.81, 0.0)
    );
    world.resource_mut::<Gravity>().unwrap().value.y = -1.62;
    assert_eq!(
        world.resource_json("Gravity").unwrap(),
        json!({ "value": [0.0, -1.62, 0.0] })
    );
    world
        .set_resource_json("Gravity", &json!({ "value": [0, -3.7, 0] }))
        .unwrap();
    assert_eq!(world.resource::<Gravity>().unwrap().value.y, -3.7);
    assert_eq!(
        world.remove_resource::<Gravity>().map(|g| g.value.y),
        Some(-3.7)
    );
    assert!(world.resource::<Gravity>().is_none());
    assert_eq!(
        error_code(world.resource_json("Gravity")),
        "resource_not_found"
    );
    world.set_resource_json("Gravity", &json!({})).unwrap();
    assert_eq!(
        world.resource::<Gravity>().unwrap().value.y,
        -9.81,
        "a new value starts from defaults"
    );
    let unknown = world.resource_json("Gravty").unwrap_err();
    assert_eq!(unknown.code, "unknown_resource");
    assert_eq!(unknown.hint, "Did you mean 'Gravity'?");
}

#[test]
fn zero_sized_components_are_tags() {
    let mut world = World::new();
    let entities: Vec<Entity> = (0..3).map(|_| world.spawn().unwrap()).collect();
    world.insert(entities[0], Player).unwrap();
    world.insert(entities[2], Player).unwrap();
    assert!(world.has::<Player>(entities[0]));
    assert!(!world.has::<Player>(entities[1]));
    assert_eq!(world.query::<(Entity, &mut Player)>().unwrap().count(), 2);
    assert_eq!(world.get_json(entities[2], "Player").unwrap(), json!({}));
    world.despawn(entities[0]).unwrap();
    assert_eq!(world.query::<&Player>().unwrap().count(), 1);
}

#[test]
fn cloned_worlds_are_independent() {
    let mut world = World::new();
    let entity = world.spawn().unwrap();
    world.insert(entity, Position { value: Vec3::X }).unwrap();
    let mut copy = world.clone();
    copy.get_mut::<Position>(entity).unwrap().value = Vec3::Y;
    assert_eq!(world.get::<Position>(entity).unwrap().value, Vec3::X);
    assert_eq!(copy.get::<Position>(entity).unwrap().value, Vec3::Y);
}
