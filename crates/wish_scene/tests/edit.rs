//! Transactions, undo, redo and the activity log.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use proptest::prelude::*;
use serde_json::{Value, json};
use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_scene::{Action, Actor, EditOp, SceneDocument, Transaction, load};
use wish_schema::{component, resource};

#[component]
pub struct Transform {
    #[schema(alias = "position")]
    pub translation: Vec3,
    pub rotation: Quat,
    #[schema(default = Vec3::ONE)]
    pub scale: Vec3,
}

#[component]
pub struct Camera {
    #[schema(range = 1.0..=179.0, default = 60.0)]
    pub fov_y_deg: f32,
}

#[component]
pub struct Player;

#[resource]
pub struct Gravity {
    pub value: Vec3,
}

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Transform>().unwrap();
    world.register_component::<Camera>().unwrap();
    world.register_component::<Player>().unwrap();
    world.register_resource::<Gravity>().unwrap();
    world
}

fn agent() -> Actor {
    Actor::Agent("test".to_owned())
}

fn tx(ops: Vec<EditOp>) -> Transaction {
    Transaction::new(agent(), ops)
}

fn components(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn spawn(id: &str, parent: Option<&str>, entries: &[(&str, Value)]) -> EditOp {
    EditOp::Spawn {
        id: id.to_owned(),
        parent: parent.map(str::to_owned),
        order: 0,
        components: components(entries),
        notes: None,
    }
}

/// A small scene: player with a camera child and a sword grandchild, and a
/// separate light.
fn sample(world: &World) -> SceneDocument {
    let mut document = SceneDocument::new();
    document
        .apply(
            &tx(vec![
                spawn(
                    "player",
                    None,
                    &[
                        ("Transform", json!({ "translation": [0, 1, 0] })),
                        ("Player", json!({})),
                    ],
                ),
                spawn(
                    "camera",
                    Some("player"),
                    &[("Camera", json!({ "fov_y_deg": 75 }))],
                ),
                spawn("sword", Some("camera"), &[]),
                spawn("light", None, &[]),
                EditOp::SetResource {
                    name: "Gravity".to_owned(),
                    value: json!({ "value": [0, -9.81, 0] }),
                },
            ]),
            world.registry(),
        )
        .unwrap();
    document
}

fn text(document: &SceneDocument) -> String {
    document.to_canonical().unwrap()
}

#[test]
fn reparent_reports_the_parent_and_order_it_changed() {
    let world = world();
    let mut document = sample(&world);
    let reparent = |parent: Option<&str>, order: Option<i64>| {
        tx(vec![EditOp::Reparent {
            id: "sword".into(),
            parent: parent.map(str::to_owned),
            order,
        }])
    };
    let only_order = document
        .apply(&reparent(Some("camera"), Some(4)), world.registry())
        .unwrap();
    assert_eq!(only_order.changes, ["/entities/sword/order"]);
    let both = document
        .apply(&reparent(Some("player"), Some(1)), world.registry())
        .unwrap();
    assert_eq!(
        both.changes,
        ["/entities/sword/order", "/entities/sword/parent"]
    );
    let only_parent = document
        .apply(&reparent(None, None), world.registry())
        .unwrap();
    assert_eq!(only_parent.changes, ["/entities/sword/parent"]);
}

#[test]
fn a_transaction_applies_in_order_and_reports_what_changed() {
    let world = world();
    let document = sample(&world);
    assert_eq!(document.revision(), 1);
    assert_eq!(document.children("player"), ["camera"]);
    assert_eq!(document.children("camera"), ["sword"]);
    let log: Vec<_> = document.log().collect();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].actor, agent());
    assert_eq!(log[0].action, Action::Edit);
    assert_eq!(
        log[0].changes,
        [
            "/entities/camera",
            "/entities/light",
            "/entities/player",
            "/entities/sword",
            "/resources/Gravity"
        ]
    );
    // What was written is a valid scene file.
    let reloaded = load(&text(&document), "", world.registry());
    assert!(
        reloaded.diagnostics.is_empty(),
        "{:?}",
        reloaded.diagnostics
    );
    assert_eq!(reloaded.document.unwrap(), document);
}

#[test]
fn set_merges_and_replace_overwrites() {
    let world = world();
    let mut document = sample(&world);
    let before = text(&document);
    document
        .apply(
            &tx(vec![EditOp::Set {
                id: "player".into(),
                component: "Transform".into(),
                value: json!({ "scale": [2, 2, 2] }),
            }]),
            world.registry(),
        )
        .unwrap();
    assert_eq!(
        document.entity("player").unwrap().components()["Transform"],
        json!({ "translation": [0, 1, 0], "scale": [2, 2, 2] }),
        "the other fields stay"
    );
    document
        .apply(
            &tx(vec![EditOp::Replace {
                id: "player".into(),
                component: "Transform".into(),
                value: json!({ "scale": [3, 3, 3] }),
            }]),
            world.registry(),
        )
        .unwrap();
    assert_eq!(
        document.entity("player").unwrap().components()["Transform"],
        json!({ "scale": [3, 3, 3] })
    );

    document
        .undo(Actor::Human, world.registry())
        .unwrap()
        .unwrap();
    document
        .undo(Actor::Human, world.registry())
        .unwrap()
        .unwrap();
    assert_eq!(text(&document), before);
}

#[test]
fn despawn_takes_the_subtree_and_undo_brings_it_all_back() {
    let world = world();
    let mut document = sample(&world);
    document
        .apply(
            &tx(vec![EditOp::SetNotes {
                id: "sword".into(),
                notes: Some("Sharp.".into()),
            }]),
            world.registry(),
        )
        .unwrap();
    let before = text(&document);
    let applied = document
        .apply(
            &tx(vec![EditOp::Despawn {
                id: "player".into(),
            }]),
            world.registry(),
        )
        .unwrap();
    assert_eq!(
        applied.changes,
        ["/entities/camera", "/entities/player", "/entities/sword"]
    );
    let left: Vec<&str> = document.entities().map(|(id, _)| id).collect();
    assert_eq!(left, ["light"]);
    document
        .undo(Actor::Human, world.registry())
        .unwrap()
        .unwrap();
    assert_eq!(
        text(&document),
        before,
        "every component, note and link is back"
    );
}

#[test]
fn rename_carries_the_children_along() {
    let world = world();
    let mut document = sample(&world);
    let before = text(&document);
    let applied = document
        .apply(
            &tx(vec![EditOp::Rename {
                id: "player".into(),
                to: "hero".into(),
            }]),
            world.registry(),
        )
        .unwrap();
    assert!(document.entity("player").is_none());
    assert_eq!(document.children("hero"), ["camera"]);
    assert_eq!(document.entity("camera").unwrap().parent(), Some("hero"));
    assert_eq!(
        applied.changes,
        [
            "/entities/camera/parent",
            "/entities/hero",
            "/entities/player"
        ]
    );
    document
        .undo(Actor::Human, world.registry())
        .unwrap()
        .unwrap();
    assert_eq!(text(&document), before);
}

#[test]
fn a_rejected_transaction_changes_nothing() {
    let world = world();
    let mut document = sample(&world);
    let before = text(&document);
    let error = document
        .apply(
            &tx(vec![
                spawn("enemy", None, &[]),
                EditOp::Set {
                    id: "enemy".into(),
                    component: "Transform".into(),
                    value: json!({ "translation": [5, 0, 0] }),
                },
                EditOp::Set {
                    id: "enemy".into(),
                    component: "Camera".into(),
                    value: json!({ "fov_y_deg": 500 }),
                },
            ]),
            world.registry(),
        )
        .unwrap_err();
    assert_eq!(error.code, "value_out_of_range");
    assert_eq!(error.location.op, Some(2));
    assert_eq!(
        error.location.path.as_deref(),
        Some("/entities/enemy/components/Camera/fov_y_deg")
    );
    assert_eq!(text(&document), before);
    assert_eq!(document.revision(), 1);
    assert_eq!(document.log().count(), 1);
}

/// Each mistake gets its own code, the index of the operation and a hint.
#[test]
fn mistakes_are_explained() {
    let world = world();
    let set = |id: &str, component: &str, value: Value| EditOp::Set {
        id: id.into(),
        component: component.into(),
        value,
    };
    // (operations, expected code, failing operation, a word the hint contains)
    let cases: Vec<(Vec<EditOp>, &str, u32, &str)> = vec![
        (
            vec![EditOp::Despawn { id: "playr".into() }],
            "unknown_entity",
            0,
            "Did you mean 'player'?",
        ),
        (
            vec![spawn("player", None, &[])],
            "entity_exists",
            0,
            "another id",
        ),
        (
            vec![spawn("Player", None, &[])],
            "invalid_scene_id",
            0,
            "lowercase",
        ),
        (
            vec![spawn("x", Some("nobody"), &[])],
            "unknown_parent",
            0,
            "Valid names",
        ),
        (
            vec![EditOp::Reparent {
                id: "player".into(),
                parent: Some("sword".into()),
                order: None,
            }],
            "hierarchy_cycle",
            0,
            "loops",
        ),
        (
            vec![EditOp::Reparent {
                id: "light".into(),
                parent: Some("light".into()),
                order: None,
            }],
            "hierarchy_cycle",
            0,
            "loops",
        ),
        (
            vec![set("light", "Hierarchy", json!({}))],
            "managed_component",
            0,
            "parent",
        ),
        (
            vec![set("light", "Transfrom", json!({}))],
            "unknown_component",
            0,
            "Did you mean 'Transform'?",
        ),
        (
            vec![set("light", "Transform", json!({ "position": [0, 0, 0] }))],
            "unknown_field",
            0,
            "Did you mean 'translation'?",
        ),
        (
            vec![
                spawn("ok", None, &[]),
                EditOp::Remove {
                    id: "light".into(),
                    component: "Camera".into(),
                },
            ],
            "component_not_found",
            1,
            "names",
        ),
        (
            vec![EditOp::RemoveResource {
                name: "Gravty".into(),
            }],
            "resource_not_found",
            0,
            "Did you mean 'Gravity'?",
        ),
        (
            vec![EditOp::SetResource {
                name: "Gravty".into(),
                value: json!({}),
            }],
            "unknown_resource",
            0,
            "Did you mean 'Gravity'?",
        ),
        (
            vec![EditOp::Rename {
                id: "light".into(),
                to: "player".into(),
            }],
            "entity_exists",
            0,
            "another id",
        ),
    ];
    for (ops, code, op, word) in cases {
        let mut document = sample(&world);
        let before = text(&document);
        let error = document
            .apply(&tx(ops.clone()), world.registry())
            .unwrap_err();
        assert_eq!(error.code, code, "{ops:?}: {error}");
        assert_eq!(error.location.op, Some(op), "{ops:?}");
        assert!(
            error.hint.contains(word),
            "{ops:?}: hint {:?} lacks {word:?}",
            error.hint
        );
        assert_eq!(text(&document), before, "{ops:?} changed the document");
    }

    let mut document = sample(&world);
    assert_eq!(
        document
            .apply(&tx(Vec::new()), world.registry())
            .unwrap_err()
            .code,
        "empty_transaction"
    );
}

#[test]
fn stale_revisions_are_refused() {
    let world = world();
    let mut document = sample(&world);
    let mut stale = tx(vec![spawn("a", None, &[])]);
    stale.base_revision = Some(0);
    let error = document.apply(&stale, world.registry()).unwrap_err();
    assert_eq!(error.code, "stale_revision");
    assert!(error.message.contains("revision 1"), "{}", error.message);

    let mut current = tx(vec![spawn("a", None, &[])]);
    current.base_revision = Some(document.revision());
    assert_eq!(
        document.apply(&current, world.registry()).unwrap().revision,
        2
    );
}

#[test]
fn preview_changes_nothing() {
    let world = world();
    let document = sample(&world);
    let before = text(&document);
    let preview = document
        .preview(
            &tx(vec![EditOp::Despawn {
                id: "camera".into(),
            }]),
            world.registry(),
        )
        .unwrap();
    assert_eq!(preview.changes, ["/entities/camera", "/entities/sword"]);
    assert_eq!(preview.revision, 2);
    assert_eq!(text(&document), before);
    assert_eq!(document.revision(), 1);
}

#[test]
fn undo_redo_and_the_log_record_who_did_what() {
    let world = world();
    let mut document = sample(&world);
    let registry = world.registry();
    document
        .apply(
            &Transaction::new(Actor::Human, vec![spawn("a", None, &[])]),
            registry,
        )
        .unwrap();
    document
        .apply(
            &Transaction::new(Actor::File, vec![spawn("b", None, &[])]),
            registry,
        )
        .unwrap();
    assert_eq!(document.next_undo_actor(), Some(&Actor::File));

    document.undo(agent(), registry).unwrap().unwrap();
    assert!(document.entity("b").is_none());
    assert!(document.can_redo());
    document.redo(Actor::Human, registry).unwrap().unwrap();
    assert!(document.entity("b").is_some());

    // A new edit after an undo makes the undone change impossible to redo.
    document.undo(Actor::Human, registry).unwrap().unwrap();
    document
        .apply(
            &Transaction::new(Actor::Human, vec![spawn("c", None, &[])]),
            registry,
        )
        .unwrap();
    assert!(!document.can_redo());
    assert_eq!(document.redo(Actor::Human, registry).unwrap(), None);

    let log: Vec<(u64, String, Action)> = document
        .log()
        .map(|entry| (entry.revision, entry.actor.label(), entry.action))
        .collect();
    assert_eq!(
        log,
        [
            (1, "agent:test".to_owned(), Action::Edit),
            (2, "human".to_owned(), Action::Edit),
            (3, "file".to_owned(), Action::Edit),
            (4, "agent:test".to_owned(), Action::Undo),
            (5, "human".to_owned(), Action::Redo),
            (6, "human".to_owned(), Action::Undo),
            (7, "human".to_owned(), Action::Edit),
        ]
    );

    // Undo all the way back, then nothing more to undo.
    while document.undo(Actor::Human, registry).unwrap().is_some() {}
    assert_eq!(document, SceneDocument::new());
    assert!(!document.can_undo());
}

// ---------------------------------------------------------------------------
// The M2 exit criterion: random edits, then undo them all, gives back the
// starting file byte for byte; redo them all gives the final file.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Seed {
    Spawn(Option<usize>, i8, Option<i16>),
    Despawn(usize),
    SetX(usize, i16),
    SetFov(usize, u8),
    Remove(usize, usize),
    Reparent(usize, Option<usize>, Option<i8>),
    Rename(usize),
    Notes(usize, Option<u8>),
    Gravity(i16),
    RemoveGravity,
}

fn seed() -> impl Strategy<Value = Seed> {
    prop_oneof![
        4 => (any::<Option<usize>>(), any::<i8>(), any::<Option<i16>>()).prop_map(|(p, o, x)| Seed::Spawn(p, o, x)),
        1 => any::<usize>().prop_map(Seed::Despawn),
        3 => (any::<usize>(), any::<i16>()).prop_map(|(e, x)| Seed::SetX(e, x)),
        2 => (any::<usize>(), any::<u8>()).prop_map(|(e, f)| Seed::SetFov(e, f)),
        1 => (any::<usize>(), any::<usize>()).prop_map(|(e, c)| Seed::Remove(e, c)),
        2 => (any::<usize>(), any::<Option<usize>>(), any::<Option<i8>>()).prop_map(|(e, p, o)| Seed::Reparent(e, p, o)),
        1 => any::<usize>().prop_map(Seed::Rename),
        1 => (any::<usize>(), any::<Option<u8>>()).prop_map(|(e, n)| Seed::Notes(e, n)),
        1 => any::<i16>().prop_map(Seed::Gravity),
        1 => Just(Seed::RemoveGravity),
    ]
}

/// Turns a seed into an operation on the document as it is now. Some of the
/// operations are invalid on purpose (out-of-range values, loops, missing
/// components); those must be refused without changing anything.
fn op(seed: &Seed, document: &SceneDocument, counter: usize) -> EditOp {
    let ids: Vec<String> = document.entities().map(|(id, _)| id.to_owned()).collect();
    let pick = |index: usize| -> String {
        if ids.is_empty() {
            "missing".to_owned()
        } else {
            ids[index % ids.len()].clone()
        }
    };
    match *seed {
        Seed::Spawn(parent, order, x) => EditOp::Spawn {
            id: format!("e{counter}"),
            parent: parent.filter(|_| !ids.is_empty()).map(pick),
            order: i64::from(order),
            components: x
                .map(|x| components(&[("Transform", json!({ "translation": [x, 0, 0] }))]))
                .unwrap_or_default(),
            notes: None,
        },
        Seed::Despawn(entity) => EditOp::Despawn { id: pick(entity) },
        Seed::SetX(entity, x) => EditOp::Set {
            id: pick(entity),
            component: "Transform".into(),
            value: json!({ "translation": [x, 1, 2] }),
        },
        Seed::SetFov(entity, fov) => EditOp::Set {
            id: pick(entity),
            component: "Camera".into(),
            value: json!({ "fov_y_deg": fov }),
        },
        Seed::Remove(entity, which) => EditOp::Remove {
            id: pick(entity),
            component: ["Transform", "Camera", "Player"][which % 3].into(),
        },
        Seed::Reparent(entity, parent, order) => EditOp::Reparent {
            id: pick(entity),
            parent: parent.map(pick),
            order: order.map(i64::from),
        },
        Seed::Rename(entity) => EditOp::Rename {
            id: pick(entity),
            to: format!("r{counter}"),
        },
        Seed::Notes(entity, note) => EditOp::SetNotes {
            id: pick(entity),
            notes: note.map(|n| format!("note {n}")),
        },
        Seed::Gravity(y) => EditOp::SetResource {
            name: "Gravity".into(),
            value: json!({ "value": [0, y, 0] }),
        },
        Seed::RemoveGravity => EditOp::RemoveResource {
            name: "Gravity".into(),
        },
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_edits_then_undo_all_gives_back_the_start(seeds in prop::collection::vec(seed(), 1..=100)) {
        let world = world();
        let registry = world.registry();
        let mut document = sample(&world);
        let start = text(&document);
        let mut applied = 0;
        for (counter, seed) in seeds.iter().enumerate() {
            let before = text(&document);
            let revision = document.revision();
            let edit = op(seed, &document, counter);
            match document.apply(&tx(vec![edit.clone()]), registry) {
                Ok(result) => {
                    applied += 1;
                    prop_assert_eq!(result.revision, revision + 1);
                    // The document is always a valid scene.
                    let reloaded = load(&text(&document), "", registry);
                    prop_assert!(!reloaded.has_errors(), "{:?} made the scene invalid: {:?}", edit, reloaded.diagnostics);
                }
                Err(error) => {
                    prop_assert_eq!(text(&document), before, "{:?} failed with {} but changed the document", edit, error.code);
                    prop_assert_eq!(document.revision(), revision);
                    prop_assert!(!error.hint.is_empty());
                }
            }
        }
        let end = text(&document);

        for _ in 0..applied {
            prop_assert!(document.undo(Actor::Human, registry).unwrap().is_some());
        }
        // `sample` itself is one transaction; undoing everything we added
        // leaves exactly the starting file.
        prop_assert_eq!(text(&document), start);

        for _ in 0..applied {
            prop_assert!(document.redo(Actor::Human, registry).unwrap().is_some());
        }
        prop_assert_eq!(text(&document), end);
        prop_assert!(!document.can_redo());
    }
}
