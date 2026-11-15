//! Transform propagation down the hierarchy.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_transform::{
    GlobalTransform, Transform, register, update_global_transforms, world_position,
};

fn world() -> World {
    let mut world = World::new();
    register(&mut world).unwrap();
    world
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_translation(Vec3::new(x, y, z))
}

#[test]
fn a_child_is_placed_relative_to_its_parent() {
    let mut world = world();
    let parent = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    let grandchild = world.spawn().unwrap();
    world.insert(parent, at(1.0, 0.0, 0.0)).unwrap();
    world.insert(child, at(0.0, 2.0, 0.0)).unwrap();
    world.insert(grandchild, at(0.0, 0.0, 3.0)).unwrap();
    world.set_parent(child, parent).unwrap();
    world.set_parent(grandchild, child).unwrap();
    update_global_transforms(&mut world).unwrap();
    let position = |entity| world_position(world.get::<GlobalTransform>(entity).unwrap());
    assert_eq!(position(parent), Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(position(child), Vec3::new(1.0, 2.0, 0.0));
    assert_eq!(position(grandchild), Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn rotation_and_scale_apply_to_children() {
    let mut world = world();
    let parent = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    world
        .insert(
            parent,
            Transform {
                translation: Vec3::new(10.0, 0.0, 0.0),
                // A quarter turn about +Y takes +X to -Z (right-handed).
                rotation: Quat::from_rotation_y(core::f32::consts::FRAC_PI_2),
                scale: Vec3::splat(2.0),
            },
        )
        .unwrap();
    world.insert(child, at(1.0, 0.0, 0.0)).unwrap();
    world.set_parent(child, parent).unwrap();
    update_global_transforms(&mut world).unwrap();
    let position = world_position(world.get::<GlobalTransform>(child).unwrap());
    assert!(
        (position - Vec3::new(10.0, 0.0, -2.0)).length() < 1e-5,
        "{position:?}"
    );
}

#[test]
fn an_entity_without_a_transform_does_not_move_its_children() {
    let mut world = world();
    let root = world.spawn().unwrap();
    let group = world.spawn().unwrap();
    let leaf = world.spawn().unwrap();
    world.insert(root, at(5.0, 0.0, 0.0)).unwrap();
    world.insert(leaf, at(0.0, 1.0, 0.0)).unwrap();
    world.set_parent(group, root).unwrap();
    world.set_parent(leaf, group).unwrap();
    update_global_transforms(&mut world).unwrap();
    assert!(
        !world.has::<GlobalTransform>(group),
        "no Transform, no GlobalTransform"
    );
    let position = world_position(world.get::<GlobalTransform>(leaf).unwrap());
    assert_eq!(position, Vec3::new(5.0, 1.0, 0.0));
}

#[test]
fn updating_changes_only_what_moved() {
    let mut world = world();
    let a = world.spawn().unwrap();
    let b = world.spawn().unwrap();
    world.insert(a, at(1.0, 0.0, 0.0)).unwrap();
    world.insert(b, at(2.0, 0.0, 0.0)).unwrap();
    update_global_transforms(&mut world).unwrap();
    let before = world.hash();
    update_global_transforms(&mut world).unwrap();
    assert_eq!(world.hash(), before, "a second update changes nothing");

    world.get_mut::<Transform>(a).unwrap().translation.x = 7.0;
    update_global_transforms(&mut world).unwrap();
    assert_eq!(
        world_position(world.get::<GlobalTransform>(a).unwrap()).x,
        7.0
    );
    assert_eq!(
        world_position(world.get::<GlobalTransform>(b).unwrap()).x,
        2.0
    );
}

#[test]
fn reparenting_and_removal_are_followed() {
    let mut world = world();
    let a = world.spawn().unwrap();
    let b = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    world.insert(a, at(1.0, 0.0, 0.0)).unwrap();
    world.insert(b, at(100.0, 0.0, 0.0)).unwrap();
    world.insert(child, at(0.0, 1.0, 0.0)).unwrap();
    world.set_parent(child, a).unwrap();
    update_global_transforms(&mut world).unwrap();
    assert_eq!(
        world_position(world.get::<GlobalTransform>(child).unwrap()).x,
        1.0
    );
    world.set_parent(child, b).unwrap();
    update_global_transforms(&mut world).unwrap();
    assert_eq!(
        world_position(world.get::<GlobalTransform>(child).unwrap()).x,
        100.0
    );
    world.remove::<Transform>(child).unwrap();
    update_global_transforms(&mut world).unwrap();
    assert!(
        !world.has::<GlobalTransform>(child),
        "a stale GlobalTransform is removed"
    );
}

#[test]
fn a_bad_rotation_never_spreads_nan() {
    let mut world = world();
    let parent = world.spawn().unwrap();
    let child = world.spawn().unwrap();
    world
        .insert(
            parent,
            Transform {
                rotation: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
                ..at(1.0, 0.0, 0.0)
            },
        )
        .unwrap();
    world.insert(child, at(0.0, 1.0, 0.0)).unwrap();
    world.set_parent(child, parent).unwrap();
    update_global_transforms(&mut world).unwrap();
    let position = world_position(world.get::<GlobalTransform>(child).unwrap());
    assert_eq!(position, Vec3::new(1.0, 1.0, 0.0));
}

#[test]
fn scenes_describe_transforms_with_defaults_and_the_position_alias() {
    let mut world = world();
    let entity = world.spawn().unwrap();
    world
        .set_json(
            entity,
            "Transform",
            &serde_json::json!({ "translation": [1, 2, 3] }),
        )
        .unwrap();
    let value = world.get_json(entity, "Transform").unwrap();
    assert_eq!(value["scale"], serde_json::json!([1.0, 1.0, 1.0]));
    assert_eq!(value["rotation"], serde_json::json!([0.0, 0.0, 0.0, 1.0]));
    let problem = world
        .set_json(
            entity,
            "Transform",
            &serde_json::json!({ "position": [0, 0, 0] }),
        )
        .unwrap_err();
    assert!(problem.hint.contains("translation"), "{}", problem.hint);
}

#[test]
fn the_same_hierarchy_always_gives_the_same_world() {
    let build = || {
        let mut world = world();
        let root = world.spawn().unwrap();
        world.insert(root, at(1.0, 2.0, 3.0)).unwrap();
        for index in 0..5u8 {
            let child = world.spawn().unwrap();
            world.insert(child, at(f32::from(index), 0.0, 0.0)).unwrap();
            world.set_parent(child, root).unwrap();
        }
        update_global_transforms(&mut world).unwrap();
        world.hash()
    };
    assert_eq!(build(), build());
}
