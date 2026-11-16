//! Extensions (ADR-026): working state that snapshots carry and the world
//! hash ignores.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_ecs::World;

#[derive(Clone, Debug, PartialEq)]
struct Cache {
    steps: Vec<u32>,
}

#[test]
fn extensions_are_stored_changed_and_removed_by_type() {
    let mut world = World::new();
    assert!(world.extension::<Cache>().is_none());
    world.insert_extension(Cache { steps: vec![1] });
    world.extension_mut::<Cache>().unwrap().steps.push(2);
    assert_eq!(world.extension::<Cache>().unwrap().steps, [1, 2]);
    assert!(world.remove_extension::<Cache>());
    assert!(!world.remove_extension::<Cache>());
}

#[test]
fn snapshots_carry_extensions_and_the_hash_leaves_them_out() {
    let mut world = World::new();
    world.insert_extension(Cache { steps: vec![1] });
    let hash = world.hash();
    let snapshot = world.snapshot();
    world.extension_mut::<Cache>().unwrap().steps.push(2);
    assert_eq!(world.hash(), hash, "working state is not gameplay state");
    world.remove_extension::<Cache>();
    world.restore(&snapshot).unwrap();
    assert_eq!(world.extension::<Cache>().unwrap().steps, [1]);
    // A copy of the world has its own copy.
    let mut copy = world.clone();
    copy.extension_mut::<Cache>().unwrap().steps.push(9);
    assert_eq!(world.extension::<Cache>().unwrap().steps, [1]);
}
