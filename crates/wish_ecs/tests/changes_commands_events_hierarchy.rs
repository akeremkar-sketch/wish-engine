//! Change detection, deferred commands, events and the hierarchy.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::Entity;
use wish_core::math::Vec3;
use wish_ecs::{Added, ChangeTick, Changed, Commands, EventCursor, Hierarchy, Target, With, World};
use wish_schema::{component, event};

#[component]
pub struct Position {
    pub value: Vec3,
}

#[component]
pub struct Health {
    pub points: f32,
}

#[component]
pub struct Doomed;

#[event]
pub struct Hit {
    pub target: Entity,
    pub damage: f32,
}

fn error_code<T>(result: wish_core::Result<T>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(diagnostic) => diagnostic.code.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Change detection
// ---------------------------------------------------------------------------

#[test]
fn writes_record_the_change_tick() {
    let mut world = World::new();
    let a = world.spawn().unwrap();
    let b = world.spawn().unwrap();
    world.insert(a, Position { value: Vec3::ZERO }).unwrap();
    world.insert(b, Position { value: Vec3::ZERO }).unwrap();
    let id = world.component_id("Position").unwrap();
    assert_eq!(
        world.component_ticks(a, id).unwrap(),
        Some((
            ChangeTick::default().max(world.change_tick()),
            world.change_tick()
        ))
    );

    let since = world.change_tick();
    world.advance_change_tick();
    assert_eq!(
        world
            .query_since::<Entity, Changed<Position>>(since)
            .unwrap()
            .count(),
        0
    );

    // Reading through `Mut` does not count as a change; writing does.
    for position in world.query::<&mut Position>().unwrap() {
        let _ = position.value;
    }
    assert_eq!(
        world
            .query_since::<Entity, Changed<Position>>(since)
            .unwrap()
            .count(),
        0
    );
    world.get_mut::<Position>(b).unwrap().value.x = 1.0;
    let changed: Vec<Entity> = world
        .query_since::<Entity, Changed<Position>>(since)
        .unwrap()
        .collect();
    assert_eq!(changed, [b]);
    let (added, last_written) = world.component_ticks(b, id).unwrap().unwrap();
    assert!(added <= since && last_written > since);

    // Overwriting through set_json and insert also counts.
    world
        .set_json(a, "Position", &json!({ "value": [2, 2, 2] }))
        .unwrap();
    assert_eq!(
        world
            .query_since::<Entity, Changed<Position>>(since)
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        world
            .query_since::<Entity, Added<Position>>(since)
            .unwrap()
            .count(),
        0
    );

    // Adding a component counts as both added and changed; moving an
    // entity to another archetype keeps the ticks of what it already had.
    let since = world.change_tick();
    world.advance_change_tick();
    world.insert(a, Health { points: 3.0 }).unwrap();
    assert_eq!(
        world
            .query_since::<Entity, Added<Health>>(since)
            .unwrap()
            .collect::<Vec<_>>(),
        [a]
    );
    assert_eq!(
        world
            .query_since::<Entity, Changed<Position>>(since)
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        world
            .query_filtered::<Entity, Changed<Position>>()
            .unwrap()
            .count(),
        2,
        "since tick 0"
    );
}

#[test]
fn changed_filter_combines_with_mutable_access() {
    let mut world = World::new();
    let entities: Vec<Entity> = (0..4)
        .map(|index| {
            let entity = world.spawn().unwrap();
            world
                .insert(
                    entity,
                    Health {
                        points: index as f32,
                    },
                )
                .unwrap();
            entity
        })
        .collect();
    let since = world.change_tick();
    world.advance_change_tick();
    world.get_mut::<Health>(entities[1]).unwrap().points = 10.0;
    world.get_mut::<Health>(entities[3]).unwrap().points = 30.0;
    // Halve only what changed; the filter reads each row before handing it out.
    for mut health in world
        .query_since::<&mut Health, Changed<Health>>(since)
        .unwrap()
    {
        health.points /= 2.0;
    }
    let points: Vec<f32> = entities
        .iter()
        .map(|e| world.get::<Health>(*e).unwrap().points)
        .collect();
    assert_eq!(points, [0.0, 5.0, 2.0, 15.0]);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[test]
fn commands_apply_in_order_after_a_query() {
    let mut world = World::new();
    world.register_component::<Health>().unwrap();
    let entities: Vec<Entity> = (0..4)
        .map(|index| {
            let entity = world.spawn().unwrap();
            world
                .insert(
                    entity,
                    Health {
                        points: index as f32 - 1.0,
                    },
                )
                .unwrap();
            entity
        })
        .collect();

    let mut commands = Commands::new();
    for (entity, health) in world.query::<(Entity, &Health)>().unwrap() {
        if health.points <= 0.0 {
            commands.despawn(entity);
        }
    }
    let spawned = commands.spawn();
    commands.insert(spawned, Position { value: Vec3::Y });
    commands.set_json(spawned, "Health", json!({ "points": 9 }));
    commands.set_parent(spawned, entities[3]);
    assert_eq!(commands.len(), 6);

    let report = world.apply(commands);
    assert!(report.is_ok(), "{:?}", report.diagnostics);
    let new_entity = report.spawned[0].unwrap();
    assert!(!world.contains(entities[0]));
    assert!(!world.contains(entities[1]));
    assert!(world.contains(entities[2]));
    assert_eq!(world.get::<Position>(new_entity).unwrap().value, Vec3::Y);
    assert_eq!(world.get::<Health>(new_entity).unwrap().points, 9.0);
    assert_eq!(world.parent(new_entity), Some(entities[3]));
}

#[test]
fn failing_commands_are_reported_and_the_rest_still_run() {
    let mut world = World::new();
    let entity = world.spawn().unwrap();
    let mut commands = Commands::new();
    commands.despawn(entity);
    commands.despawn(entity);
    commands.insert(Target::Spawned(7), Doomed);
    let spawned = commands.spawn();
    commands.set_json(spawned, "Nope", json!({}));
    commands.insert(spawned, Doomed);

    let report = world.apply(commands);
    let codes: Vec<(&str, Option<u32>)> = report
        .diagnostics
        .iter()
        .map(|d| (d.code.as_ref(), d.location.op))
        .collect();
    assert_eq!(
        codes,
        [
            ("entity_not_found", Some(1)),
            ("spawn_target_missing", Some(2)),
            ("unknown_component", Some(4)),
        ]
    );
    assert!(world.has::<Doomed>(report.spawned[0].unwrap()));
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn events_are_buffered_for_two_updates_and_cursors_see_each_once() {
    let mut world = World::new();
    assert!(world.events::<Hit>().unwrap().is_empty());
    let target = world.spawn().unwrap();
    let mut cursor = EventCursor::<Hit>::default();

    world
        .send(Hit {
            target,
            damage: 1.0,
        })
        .unwrap();
    world
        .send(Hit {
            target,
            damage: 2.0,
        })
        .unwrap();
    let first: Vec<f32> = world
        .read_events(&mut cursor)
        .unwrap()
        .iter()
        .map(|hit| hit.damage)
        .collect();
    assert_eq!(first, [1.0, 2.0]);
    assert!(
        world.read_events(&mut cursor).unwrap().is_empty(),
        "each event is read once"
    );

    world.update_events();
    world.send_json("Hit", &json!({ "damage": 3 })).unwrap();
    assert_eq!(
        world.events::<Hit>().unwrap().len(),
        3,
        "both buffers are readable"
    );
    let second: Vec<f32> = world
        .read_events(&mut cursor)
        .unwrap()
        .iter()
        .map(|hit| hit.damage)
        .collect();
    assert_eq!(second, [3.0]);
    assert_eq!(
        world.events_json("Hit").unwrap(),
        json!([
            { "target": target.to_string(), "damage": 1.0 },
            { "target": target.to_string(), "damage": 2.0 },
            { "target": "null", "damage": 3.0 }
        ])
    );

    world.update_events();
    assert_eq!(
        world.events::<Hit>().unwrap().len(),
        1,
        "the first update's events are gone"
    );
    world.update_events();
    assert!(world.events::<Hit>().unwrap().is_empty());

    // A late reader skips what was dropped and sees nothing twice.
    let mut late = EventCursor::<Hit>::default();
    world
        .send(Hit {
            target,
            damage: 4.0,
        })
        .unwrap();
    let late_events: Vec<f32> = world
        .read_events(&mut late)
        .unwrap()
        .iter()
        .map(|hit| hit.damage)
        .collect();
    assert_eq!(late_events, [4.0]);
    assert_eq!(late.position(), 4);

    let unknown = world.send_json("Hti", &json!({})).unwrap_err();
    assert_eq!(unknown.code, "unknown_event");
    assert_eq!(unknown.hint, "Did you mean 'Hit'?");
}

// ---------------------------------------------------------------------------
// Hierarchy
// ---------------------------------------------------------------------------

#[test]
fn children_keep_their_order_and_links_stay_consistent() {
    let mut world = World::new();
    let root = world.spawn().unwrap();
    let kids: Vec<Entity> = (0..4).map(|_| world.spawn().unwrap()).collect();
    for kid in &kids {
        world.set_parent(*kid, root).unwrap();
    }
    assert_eq!(world.children(root), kids);
    assert_eq!(world.parent(kids[2]), Some(root));
    assert_eq!(world.parent(root), None);

    // Remove from the middle, the front and the back.
    world.remove_parent(kids[1]).unwrap();
    assert_eq!(world.children(root), [kids[0], kids[2], kids[3]]);
    world.remove_parent(kids[0]).unwrap();
    world.remove_parent(kids[3]).unwrap();
    assert_eq!(world.children(root), [kids[2]]);
    let links = *world.get::<Hierarchy>(root).unwrap();
    assert_eq!(
        (links.first_child(), links.last_child()),
        (Some(kids[2]), Some(kids[2]))
    );
    let only = *world.get::<Hierarchy>(kids[2]).unwrap();
    assert_eq!((only.prev_sibling(), only.next_sibling()), (None, None));

    // Moving to another parent appends at the end; the same parent is a no-op.
    world.set_parent(kids[0], root).unwrap();
    world.set_parent(kids[2], root).unwrap();
    assert_eq!(world.children(root), [kids[2], kids[0]]);
    world.set_parent(kids[1], kids[0]).unwrap();
    world.set_parent(kids[3], kids[1]).unwrap();
    assert_eq!(
        world.descendants(root),
        [kids[2], kids[0], kids[1], kids[3]]
    );
}

#[test]
fn cycles_are_refused() {
    let mut world = World::new();
    let a = world.spawn().unwrap();
    let b = world.spawn().unwrap();
    let c = world.spawn().unwrap();
    world.set_parent(b, a).unwrap();
    world.set_parent(c, b).unwrap();
    assert_eq!(error_code(world.set_parent(a, a)), "hierarchy_cycle");
    assert_eq!(error_code(world.set_parent(a, c)), "hierarchy_cycle");
    assert_eq!(
        world.descendants(a),
        [b, c],
        "a refused change leaves the links alone"
    );
    let dead = world.spawn().unwrap();
    world.despawn(dead).unwrap();
    assert_eq!(error_code(world.set_parent(a, dead)), "entity_not_found");
}

#[test]
fn despawning_removes_the_whole_subtree() {
    let mut world = World::new();
    let root = world.spawn().unwrap();
    let branch = world.spawn().unwrap();
    let leaf = world.spawn().unwrap();
    let sibling = world.spawn().unwrap();
    world.set_parent(branch, root).unwrap();
    world.set_parent(sibling, root).unwrap();
    world.set_parent(leaf, branch).unwrap();

    world.despawn(branch).unwrap();
    assert!(!world.contains(branch));
    assert!(!world.contains(leaf));
    assert_eq!(
        world.children(root),
        [sibling],
        "the parent's list skips the despawned child"
    );
    assert_eq!(
        world.get::<Hierarchy>(sibling).unwrap().prev_sibling(),
        None
    );
}

#[test]
fn the_hierarchy_can_be_read_but_not_written_directly() {
    let mut world = World::new();
    let parent = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    world.set_parent(child, parent).unwrap();

    assert_eq!(
        world.get_json(child, "Hierarchy").unwrap()["parent"],
        json!(parent.to_string())
    );
    assert!(
        world
            .registry()
            .component(world.component_id("Hierarchy").unwrap())
            .unwrap()
            .is_managed()
    );
    assert_eq!(
        error_code(world.set_json(child, "Hierarchy", &json!({ "parent": "null" }))),
        "managed_component"
    );
    assert_eq!(
        error_code(world.insert(child, Hierarchy::default())),
        "managed_component"
    );
    assert_eq!(
        error_code(world.remove::<Hierarchy>(child)),
        "managed_component"
    );
    assert_eq!(
        error_code(world.remove_json(child, "Hierarchy")),
        "managed_component"
    );
    let id = world.component_id("Hierarchy").unwrap();
    let bytes = world.component_bytes(child, id).unwrap().unwrap();
    assert_eq!(
        error_code(world.insert_bytes(child, id, &bytes)),
        "managed_component"
    );
    assert!(world.get_mut::<Hierarchy>(child).is_none());
    assert_eq!(
        error_code(world.query::<&mut Hierarchy>()),
        "managed_component"
    );
    assert_eq!(
        world
            .query_filtered::<&Hierarchy, With<Hierarchy>>()
            .unwrap()
            .count(),
        2
    );
    assert_eq!(
        world.parent(child),
        Some(parent),
        "nothing above changed the links"
    );
}
