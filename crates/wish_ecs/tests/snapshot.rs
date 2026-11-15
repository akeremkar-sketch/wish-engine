//! World snapshots and the world hash.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::math::Vec3;
use wish_core::{Entity, Text};
use wish_ecs::World;
use wish_schema::{component, event, resource, schema};

#[schema]
pub enum Team {
    Red,
    Blue,
}

/// Padded: bool, enum, then an f64.
#[component]
pub struct Unit {
    pub alive: bool,
    pub team: Team,
    pub health: f64,
}

#[component]
pub struct Position {
    pub value: Vec3,
}

#[component]
pub struct Name {
    pub label: Text,
}

#[resource]
pub struct Score {
    pub value: u32,
}

#[event]
pub struct Ping {
    pub from: Entity,
}

fn sample() -> World {
    let mut world = World::new();
    let parent = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    world
        .insert(
            parent,
            Unit {
                alive: true,
                team: Team::Blue,
                health: 75.5,
            },
        )
        .unwrap();
    world
        .insert(
            parent,
            Position {
                value: Vec3::new(1.0, 2.0, 3.0),
            },
        )
        .unwrap();
    world.insert(child, Position { value: Vec3::X }).unwrap();
    world
        .set_json(child, "Unit", &json!({ "health": 10 }))
        .unwrap();
    let label = world.intern("hero").unwrap();
    world.insert(child, Name { label }).unwrap();
    world.set_parent(child, parent).unwrap();
    world.insert_resource(Score { value: 3 }).unwrap();
    world.send(Ping { from: parent }).unwrap();
    let doomed = world.spawn().unwrap();
    world.despawn(doomed).unwrap();
    world
}

/// Recorded on 2026-11-15. Locks the hash definition: if this changes,
/// stored hashes (replays, test expectations) no longer match. Change it
/// only together with the hash version string in `World::hash`.
#[test]
fn recorded_hash_of_a_fixed_world() {
    assert_eq!(
        sample().hash().to_string(),
        "06af2ccecabc968175109079f8f6fce0"
    );
}

#[test]
fn the_same_operations_give_the_same_hash() {
    assert_eq!(sample().hash(), sample().hash());
    assert_eq!(sample().hash(), sample().clone().hash());
}

/// A named change to apply to a world.
type Change<'a> = (&'static str, Box<dyn Fn(&mut World) + 'a>);

#[test]
fn every_kind_of_state_changes_the_hash() {
    let base = sample().hash();
    let parent = Entity::new(0, 0);
    let child = Entity::new(1, 0);
    let changes: Vec<Change<'_>> = vec![
        (
            "component value",
            Box::new(|w| w.get_mut::<Position>(parent).unwrap().value.x = 9.0),
        ),
        (
            "component removed",
            Box::new(|w| {
                w.remove::<Name>(child).unwrap();
            }),
        ),
        (
            "entity spawned",
            Box::new(|w| {
                w.spawn().unwrap();
            }),
        ),
        ("hierarchy", Box::new(|w| w.remove_parent(child).unwrap())),
        (
            "resource",
            Box::new(|w| w.resource_mut::<Score>().unwrap().value = 4),
        ),
        ("event", Box::new(|w| w.send(Ping { from: child }).unwrap())),
        ("event rotation", Box::new(|w| w.update_events())),
        (
            "text table",
            Box::new(|w| {
                w.intern("villain").unwrap();
            }),
        ),
        (
            "change tick",
            Box::new(|w| {
                w.advance_change_tick();
            }),
        ),
        (
            "touched without a new value",
            Box::new(|w| {
                let value = *w.get::<Position>(parent).unwrap();
                w.advance_change_tick();
                *w.get_mut::<Position>(parent).unwrap() = value;
            }),
        ),
        (
            "type registered",
            Box::new(|w| {
                w.register_component::<Team2>().unwrap();
            }),
        ),
    ];
    for (what, change) in changes {
        let mut world = sample();
        change(&mut world);
        assert_ne!(world.hash(), base, "{what} should change the hash");
    }
}

#[component]
pub struct Team2 {
    pub team: Team,
}

#[test]
fn padding_never_reaches_the_hash() {
    // One world stores the value with zeroed padding (insert encodes it),
    // the other assigns a whole value through `&mut`, which may leave
    // padding uninitialised. Same fields, same ticks: same hash.
    let value = Unit {
        alive: true,
        team: Team::Red,
        health: 12.25,
    };
    let mut encoded = World::new();
    let entity = encoded.spawn().unwrap();
    encoded.insert(entity, value).unwrap();

    let mut assigned = World::new();
    let entity = assigned.spawn().unwrap();
    assigned
        .insert(
            entity,
            Unit {
                alive: false,
                team: Team::Blue,
                health: 0.0,
            },
        )
        .unwrap();
    *assigned.get_mut::<Unit>(entity).unwrap() = value;

    assert_eq!(encoded.hash(), assigned.hash());
}

#[test]
fn restore_returns_to_the_exact_state() {
    let mut world = sample();
    let snapshot = world.snapshot();
    let before_hash = world.hash();
    let before_json: Vec<_> = world
        .entities()
        .map(|e| world.entity_json(e).unwrap())
        .collect();
    assert_eq!(snapshot.entity_count(), 2);

    let parent = Entity::new(0, 0);
    world.despawn(parent).unwrap();
    world.spawn().unwrap();
    world.resource_mut::<Score>().unwrap().value = 99;
    world.update_events();
    world.advance_change_tick();
    assert_ne!(world.hash(), before_hash);

    world.restore(&snapshot).unwrap();
    assert_eq!(world.hash(), before_hash);
    let after_json: Vec<_> = world
        .entities()
        .map(|e| world.entity_json(e).unwrap())
        .collect();
    assert_eq!(after_json, before_json);
    assert_eq!(world.events::<Ping>().unwrap().len(), 1);
    assert_eq!(world.resource::<Score>().unwrap().value, 3);
    assert_eq!(
        world.text(world.get::<Name>(Entity::new(1, 0)).unwrap().label),
        Some("hero")
    );

    // The restored world keeps working: the same next spawn as the original.
    let mut original = sample();
    assert_eq!(world.spawn().unwrap(), original.spawn().unwrap());
    assert_eq!(world.hash(), original.hash());
}

#[test]
fn types_registered_after_the_snapshot_do_not_block_restoring() {
    let mut world = sample();
    let snapshot = world.snapshot();
    world.register_component::<Team2>().unwrap();
    let entity = world.spawn().unwrap();
    world.insert(entity, Team2 { team: Team::Red }).unwrap();
    world.restore(&snapshot).unwrap();
    assert!(!world.contains(entity));
    assert!(
        world.registry().component_id_of::<Team2>().is_some(),
        "registration survives"
    );
}

#[test]
fn snapshots_refuse_worlds_with_different_types() {
    let snapshot = sample().snapshot();
    let mut other = World::new();
    other.register_component::<Position>().unwrap();
    let error = other.restore(&snapshot).unwrap_err();
    assert_eq!(error.code, "snapshot_mismatch");
    assert!(!error.hint.is_empty());
}
