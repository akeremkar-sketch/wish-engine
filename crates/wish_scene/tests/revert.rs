//! Selective undo (M8b WP1): one earlier transaction taken back on its own,
//! as a card of the activity feed does, without losing later work.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use serde_json::{Value, json};
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_scene::{Action, Actor, EditOp, SceneDocument, Transaction};
use wish_schema::component;

#[component]
pub struct Transform {
    pub translation: Vec3,
}

#[component]
pub struct Player;

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Transform>().unwrap();
    world.register_component::<Player>().unwrap();
    world
}

fn agent() -> Actor {
    Actor::Agent("helper".to_owned())
}

fn at(x: f32) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "Transform".to_owned(),
        json!({ "translation": [x, 0.0, 0.0] }),
    )])
}

fn spawn(id: &str, x: f32) -> EditOp {
    EditOp::Spawn {
        id: id.to_owned(),
        parent: None,
        order: 0,
        components: at(x),
        notes: None,
    }
}

fn set_x(id: &str, x: f32) -> EditOp {
    EditOp::Set {
        id: id.to_owned(),
        component: "Transform".to_owned(),
        value: json!({ "translation": [x, 0.0, 0.0] }),
    }
}

fn x_of(doc: &SceneDocument, id: &str) -> Option<f64> {
    let entity = doc.entities().find(|(name, _)| *name == id)?.1;
    entity.components()["Transform"]["translation"][0].as_f64()
}

fn apply(doc: &mut SceneDocument, actor: Actor, ops: Vec<EditOp>, reason: Option<&str>) -> u64 {
    let world = world();
    let mut tx = Transaction::new(actor, ops);
    tx.reason = reason.map(str::to_owned);
    doc.apply(&tx, world.registry()).unwrap().revision
}

#[test]
fn an_earlier_change_is_taken_back_and_later_work_is_kept() {
    let world = world();
    let mut doc = SceneDocument::new();
    let agents = apply(
        &mut doc,
        agent(),
        vec![spawn("crate", 1.0)],
        Some("a crate to jump on"),
    );
    let humans = apply(&mut doc, Actor::Human, vec![spawn("lamp", 5.0)], None);
    // The agent's card: taken back; the person's lamp stays.
    let reverted = doc
        .revert(agents, Actor::Human, None, world.registry())
        .unwrap();
    assert!(x_of(&doc, "crate").is_none());
    assert_eq!(x_of(&doc, "lamp"), Some(5.0));
    assert!(doc.is_reverted(agents));
    assert!(!doc.is_reverted(humans));
    let log: Vec<_> = doc.log().collect();
    assert_eq!(log[0].reason.as_deref(), Some("a crate to jump on"));
    let last = log.last().unwrap();
    assert_eq!(last.action, Action::Revert);
    assert_eq!(last.reverts, Some(agents));
    assert_eq!(last.revision, reverted.revision);
    // Not twice.
    let again = doc
        .revert(agents, Actor::Human, None, world.registry())
        .unwrap_err();
    assert_eq!(again.code, "already_reverted");
    // The revert is undone like any change: the crate is back, and the
    // card can be taken back again.
    doc.undo(Actor::Human, world.registry()).unwrap();
    assert_eq!(x_of(&doc, "crate"), Some(1.0));
    assert!(!doc.is_reverted(agents));
    doc.revertible(agents).unwrap();
    // Redo the revert: gone again.
    doc.redo(Actor::Human, world.registry()).unwrap();
    assert!(x_of(&doc, "crate").is_none());
    assert!(doc.is_reverted(agents));
}

#[test]
fn a_change_that_later_work_touched_is_not_taken_back() {
    let world = world();
    let mut doc = SceneDocument::new();
    let agents = apply(&mut doc, agent(), vec![spawn("crate", 1.0)], None);
    let humans = apply(&mut doc, Actor::Human, vec![set_x("crate", 3.0)], None);
    let refused = doc
        .revert(agents, Actor::Human, None, world.registry())
        .unwrap_err();
    assert_eq!(refused.code, "revert_conflict");
    assert!(
        refused.message.contains(&format!("r{humans} by human")),
        "{}",
        refused.message
    );
    assert!(!refused.hint.is_empty());
    assert_eq!(x_of(&doc, "crate"), Some(3.0), "nothing changed");
    // Newest first: the person's change, then the agent's.
    doc.revert(humans, Actor::Human, None, world.registry())
        .unwrap();
    assert_eq!(x_of(&doc, "crate"), Some(1.0));
    // With the person's change taken back, the crate is as the agent left
    // it: the agent's card can be taken back now.
    doc.revert(agents, Actor::Human, None, world.registry())
        .unwrap();
    assert!(x_of(&doc, "crate").is_none());
}

#[test]
fn a_component_and_its_entity_overlap_but_neighbours_do_not() {
    let world = world();
    let _ = world.registry();
    let mut doc = SceneDocument::new();
    apply(
        &mut doc,
        Actor::Human,
        vec![spawn("crate", 0.0), spawn("crate_lid", 0.0)],
        None,
    );
    let agents = apply(
        &mut doc,
        agent(),
        vec![EditOp::Set {
            id: "crate".to_owned(),
            component: "Player".to_owned(),
            value: json!({}),
        }],
        None,
    );
    // A change to "crate_lid" is not a change to "crate".
    apply(&mut doc, Actor::Human, vec![set_x("crate_lid", 9.0)], None);
    doc.revertible(agents).unwrap();
    // A change to the crate's Transform does not touch its Player either.
    apply(&mut doc, Actor::Human, vec![set_x("crate", 2.0)], None);
    doc.revertible(agents).unwrap();
    // Deleting the crate does.
    apply(
        &mut doc,
        Actor::Human,
        vec![EditOp::Despawn {
            id: "crate".to_owned(),
        }],
        None,
    );
    assert_eq!(doc.revertible(agents).unwrap_err().code, "revert_conflict");
}

#[test]
fn undone_changes_and_unknown_revisions_are_not_revertible() {
    let world = world();
    let mut doc = SceneDocument::new();
    let first = apply(&mut doc, agent(), vec![spawn("crate", 1.0)], None);
    doc.undo(Actor::Human, world.registry()).unwrap();
    assert_eq!(doc.revertible(first).unwrap_err().code, "not_revertible");
    assert_eq!(doc.revertible(99).unwrap_err().code, "not_revertible");
    // A change made outside replaces the content: earlier cards close.
    let second = apply(&mut doc, agent(), vec![spawn("lamp", 1.0)], None);
    let other = doc.clone();
    doc.replace_content(&other, Actor::File);
    assert_eq!(doc.revertible(second).unwrap_err().code, "not_revertible");
}
