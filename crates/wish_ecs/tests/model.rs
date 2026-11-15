//! Model-based test: random sequences of operations run against the world and
//! against a plain `BTreeMap` model; after every step the two must agree.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use serde_json::json;
use wish_core::Entity;
use wish_core::Text;
use wish_core::math::Vec3;
use wish_ecs::{With, Without, World};
use wish_schema::{component, schema};

#[component]
pub struct Pos {
    pub value: Vec3,
}

#[schema]
pub enum Kind {
    Light,
    Heavy,
}

/// Has padding between `kind` and `weight`.
#[component]
pub struct Flags {
    pub on: bool,
    pub kind: Kind,
    pub weight: f64,
}

#[component]
pub struct Tag;

#[component]
pub struct Named {
    pub label: Text,
    pub id: u32,
}

const LABELS: [&str; 4] = ["", "a", "hero", "b"];

#[derive(Clone, Debug)]
enum Op {
    Spawn,
    Despawn(usize),
    InsertPos(usize, i16),
    InsertFlags(usize, bool, bool, i16),
    InsertTag(usize),
    InsertNamed(usize, usize, u32),
    RemovePos(usize),
    RemoveFlags(usize),
    RemoveTag(usize),
    RemoveNamed(usize),
    PatchWeight(usize, i16),
    ScalePos(i8),
    SetParent(usize, usize),
    RemoveParent(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => Just(Op::Spawn),
        1 => any::<usize>().prop_map(Op::Despawn),
        3 => (any::<usize>(), any::<i16>()).prop_map(|(e, v)| Op::InsertPos(e, v)),
        2 => (any::<usize>(), any::<bool>(), any::<bool>(), any::<i16>())
            .prop_map(|(e, on, heavy, w)| Op::InsertFlags(e, on, heavy, w)),
        2 => any::<usize>().prop_map(Op::InsertTag),
        2 => (any::<usize>(), 0..LABELS.len(), any::<u32>()).prop_map(|(e, l, id)| Op::InsertNamed(e, l, id)),
        1 => any::<usize>().prop_map(Op::RemovePos),
        1 => any::<usize>().prop_map(Op::RemoveFlags),
        1 => any::<usize>().prop_map(Op::RemoveTag),
        1 => any::<usize>().prop_map(Op::RemoveNamed),
        1 => (any::<usize>(), any::<i16>()).prop_map(|(e, w)| Op::PatchWeight(e, w)),
        1 => (-2i8..=2).prop_map(Op::ScalePos),
        3 => (any::<usize>(), any::<usize>()).prop_map(|(c, p)| Op::SetParent(c, p)),
        1 => any::<usize>().prop_map(Op::RemoveParent),
    ]
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Model {
    pos: Option<Vec3>,
    flags: Option<Flags>,
    tag: bool,
    named: Option<(&'static str, u32)>,
    parent: Option<Entity>,
    children: Vec<Entity>,
}

/// A world that knows every component up front, as a game's world does:
/// dynamic access by name needs the name to be registered.
fn new_world() -> World {
    let mut world = World::new();
    world.register_component::<Pos>().unwrap();
    world.register_component::<Flags>().unwrap();
    world.register_component::<Tag>().unwrap();
    world.register_component::<Named>().unwrap();
    world
}

fn kind(heavy: bool) -> Kind {
    if heavy { Kind::Heavy } else { Kind::Light }
}

fn pick(model: &BTreeMap<Entity, Model>, index: usize) -> Option<Entity> {
    if model.is_empty() {
        None
    } else {
        model.keys().nth(index % model.len()).copied()
    }
}

/// Everything below `entity` in the model, depth first, siblings in order.
fn model_descendants(model: &BTreeMap<Entity, Model>, entity: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack: Vec<Entity> = model[&entity].children.iter().rev().copied().collect();
    while let Some(next) = stack.pop() {
        out.push(next);
        stack.extend(model[&next].children.iter().rev().copied());
    }
    out
}

fn model_detach(model: &mut BTreeMap<Entity, Model>, child: Entity) {
    if let Some(parent) = model.get_mut(&child).unwrap().parent.take() {
        model
            .get_mut(&parent)
            .unwrap()
            .children
            .retain(|c| *c != child);
    }
}

fn apply(world: &mut World, model: &mut BTreeMap<Entity, Model>, dead: &mut Vec<Entity>, op: &Op) {
    match *op {
        Op::SetParent(c, p) => {
            let (Some(child), Some(parent)) = (pick(model, c), pick(model, p)) else {
                return;
            };
            let mut ancestors = vec![parent];
            while let Some(above) = model[ancestors.last().unwrap()].parent {
                ancestors.push(above);
            }
            let result = world.set_parent(child, parent);
            if ancestors.contains(&child) {
                assert_eq!(result.unwrap_err().code, "hierarchy_cycle");
            } else {
                result.unwrap();
                if model[&child].parent != Some(parent) {
                    model_detach(model, child);
                    model.get_mut(&child).unwrap().parent = Some(parent);
                    model.get_mut(&parent).unwrap().children.push(child);
                }
            }
        }
        Op::RemoveParent(c) => {
            let Some(child) = pick(model, c) else {
                return;
            };
            world.remove_parent(child).unwrap();
            model_detach(model, child);
        }
        Op::Spawn => {
            let entity = world.spawn().unwrap();
            assert!(model.insert(entity, Model::default()).is_none());
        }
        Op::ScalePos(factor) => {
            for mut pos in world.query::<&mut Pos>().unwrap() {
                pos.value *= f32::from(factor);
            }
            for entry in model.values_mut() {
                if let Some(pos) = &mut entry.pos {
                    *pos *= f32::from(factor);
                }
            }
        }
        _ => {
            let index = match *op {
                Op::Despawn(i)
                | Op::InsertPos(i, _)
                | Op::InsertFlags(i, ..)
                | Op::InsertTag(i)
                | Op::InsertNamed(i, ..)
                | Op::RemovePos(i)
                | Op::RemoveFlags(i)
                | Op::RemoveTag(i)
                | Op::RemoveNamed(i)
                | Op::PatchWeight(i, _) => i,
                Op::Spawn | Op::ScalePos(_) | Op::SetParent(..) | Op::RemoveParent(_) => {
                    unreachable!()
                }
            };
            let Some(entity) = pick(model, index) else {
                return;
            };
            let entry = model.get_mut(&entity).unwrap();
            match *op {
                Op::Despawn(_) => {
                    world.despawn(entity).unwrap();
                    let subtree = model_descendants(model, entity);
                    model_detach(model, entity);
                    for gone in std::iter::once(entity).chain(subtree) {
                        model.remove(&gone);
                        dead.push(gone);
                    }
                }
                Op::InsertPos(_, v) => {
                    let value = Vec3::new(f32::from(v), -f32::from(v), f32::from(v) * 0.5);
                    world.insert(entity, Pos { value }).unwrap();
                    entry.pos = Some(value);
                }
                Op::InsertFlags(_, on, heavy, w) => {
                    let flags = Flags {
                        on,
                        kind: kind(heavy),
                        weight: f64::from(w) / 4.0,
                    };
                    world.insert(entity, flags).unwrap();
                    entry.flags = Some(flags);
                }
                Op::InsertTag(_) => {
                    world.insert(entity, Tag).unwrap();
                    entry.tag = true;
                }
                Op::InsertNamed(_, label, id) => {
                    let text = world.intern(LABELS[label]).unwrap();
                    world.insert(entity, Named { label: text, id }).unwrap();
                    entry.named = Some((LABELS[label], id));
                }
                Op::RemovePos(_) => {
                    assert_eq!(
                        world.remove::<Pos>(entity).unwrap().map(|p| p.value),
                        entry.pos.take()
                    );
                }
                Op::RemoveFlags(_) => {
                    assert_eq!(world.remove::<Flags>(entity).unwrap(), entry.flags.take());
                }
                Op::RemoveTag(_) => {
                    assert_eq!(world.remove::<Tag>(entity).unwrap().is_some(), entry.tag);
                    entry.tag = false;
                }
                Op::RemoveNamed(_) => {
                    let removed = world.remove::<Named>(entity).unwrap();
                    assert_eq!(removed.is_some(), entry.named.is_some());
                    entry.named = None;
                }
                Op::PatchWeight(_, w) => {
                    let weight = f64::from(w) / 2.0;
                    world
                        .set_json(entity, "Flags", &json!({ "weight": weight }))
                        .unwrap();
                    let flags = entry.flags.get_or_insert(Flags::default());
                    flags.weight = weight;
                }
                Op::Spawn | Op::ScalePos(_) | Op::SetParent(..) | Op::RemoveParent(_) => {
                    unreachable!()
                }
            }
        }
    }
}

fn check(world: &mut World, model: &BTreeMap<Entity, Model>, dead: &[Entity]) {
    assert_eq!(world.entity_count(), model.len());
    assert_eq!(
        world.entities().collect::<Vec<_>>(),
        model.keys().copied().collect::<Vec<_>>()
    );
    for entity in dead {
        assert!(!world.contains(*entity), "{entity} was despawned");
    }
    for (entity, entry) in model {
        assert!(world.contains(*entity));
        assert_eq!(
            world.get::<Pos>(*entity).map(|p| p.value),
            entry.pos,
            "{entity} Pos"
        );
        assert_eq!(
            world.get::<Flags>(*entity).copied(),
            entry.flags,
            "{entity} Flags"
        );
        assert_eq!(world.has::<Tag>(*entity), entry.tag, "{entity} Tag");
        assert_eq!(world.parent(*entity), entry.parent, "{entity} parent");
        assert_eq!(world.children(*entity), entry.children, "{entity} children");
        let named = world
            .get::<Named>(*entity)
            .map(|named| (world.text(named.label).unwrap().to_owned(), named.id));
        assert_eq!(
            named,
            entry.named.map(|(label, id)| (label.to_owned(), id)),
            "{entity} Named"
        );
        if let Some(flags) = entry.flags {
            let json = world.get_json(*entity, "Flags").unwrap();
            assert_eq!(json["weight"], json!(flags.weight));
            assert_eq!(json["on"], json!(flags.on));
        }
    }

    let mut positions: Vec<(Entity, Vec3)> = world
        .query::<(Entity, &Pos)>()
        .unwrap()
        .map(|(entity, pos)| (entity, pos.value))
        .collect();
    positions.sort_by_key(|(entity, _)| *entity);
    let expected: Vec<(Entity, Vec3)> = model
        .iter()
        .filter_map(|(entity, entry)| entry.pos.map(|pos| (*entity, pos)))
        .collect();
    assert_eq!(positions, expected);

    let tagged_unflagged: BTreeSet<Entity> = world
        .query_filtered::<Entity, (With<Tag>, Without<Flags>)>()
        .unwrap()
        .collect();
    let expected: BTreeSet<Entity> = model
        .iter()
        .filter(|(_, entry)| entry.tag && entry.flags.is_none())
        .map(|(entity, _)| *entity)
        .collect();
    assert_eq!(tagged_unflagged, expected);

    let pairs = world.query::<(&Flags, &Named)>().unwrap().count();
    let expected = model
        .values()
        .filter(|entry| entry.flags.is_some() && entry.named.is_some())
        .count();
    assert_eq!(pairs, expected);
}

fn config() -> ProptestConfig {
    ProptestConfig {
        // Miri runs the same test ~1000 times slower; a few cases still
        // exercise every unsafe path.
        cases: if cfg!(miri) { 4 } else { 256 },
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn max_ops() -> usize {
    if cfg!(miri) { 24 } else { 80 }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn world_matches_model(ops in prop::collection::vec(op(), 1..max_ops())) {
        let mut world = new_world();
        let mut model = BTreeMap::new();
        let mut dead = Vec::new();
        for op in &ops {
            apply(&mut world, &mut model, &mut dead, op);
            check(&mut world, &model, &dead);
        }
    }

    /// The same operations give the same world, down to entity handles and
    /// query order: nothing depends on addresses, hashing or time.
    #[test]
    fn same_operations_same_world(ops in prop::collection::vec(op(), 1..max_ops())) {
        let run = || {
            let mut world = new_world();
            let mut model = BTreeMap::new();
            let mut dead = Vec::new();
            for op in &ops {
                apply(&mut world, &mut model, &mut dead, op);
            }
            let order: Vec<Entity> = world.query::<Entity>().unwrap().collect();
            let json: Vec<_> = order.iter().map(|entity| world.entity_json(*entity).unwrap()).collect();
            (order, json)
        };
        prop_assert_eq!(run(), run());
    }
}
