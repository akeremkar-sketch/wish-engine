//! Prefabs: prefab files, instances in scenes, expansion and edits.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Value, json};
use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_scene::json::Span;
use wish_scene::{
    Actor, EditOp, ExpandedEntity, LoadResult, PrefabLibrary, SceneDocument, Transaction, load,
    load_prefab, load_with_prefabs,
};
use wish_schema::component;

#[component]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    #[schema(default = Vec3::ONE)]
    pub scale: Vec3,
}

#[component]
pub struct Damage {
    #[schema(range = 0.0..=1000.0, default = 10.0)]
    pub amount: f32,
}

#[component]
pub struct Player;

const ENEMY_PATH: &str = "prefabs/enemy.prefab.json";
const SCENE_PATH: &str = "scenes/main.scene.json";

const ENEMY: &str = r#"{
  "format": 1,
  "entities": {
    "root": { "components": { "Transform": { "scale": [2, 2, 2] } } },
    "body": {
      "parent": "root",
      "components": { "Transform": { "translation": [0, 1, 0] } }
    },
    "weapon": { "parent": "body", "components": { "Damage": { "amount": 5 } } }
  }
}"#;

const CRATE: &str = r#"{ "format": 1, "entities": { "root": {} } }"#;

const SCENE: &str = r#"{
  "format": 1,
  "entities": {
    "enemy_1": {
      "prefab": "prefabs/enemy.prefab.json",
      "components": { "Transform": { "translation": [4, 0, 0] } },
      "overrides": { "weapon": { "components": { "Damage": { "amount": 20 } } } }
    },
    "enemy_2": { "prefab": "prefabs/enemy.prefab.json", "order": 1 },
    "marker": { "parent": "enemy_1", "components": { "Player": {} } }
  }
}"#;

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Transform>().unwrap();
    world.register_component::<Damage>().unwrap();
    world.register_component::<Player>().unwrap();
    world
}

fn library(world: &World) -> Arc<PrefabLibrary> {
    let mut library = PrefabLibrary::new();
    for (path, source) in [(ENEMY_PATH, ENEMY), ("prefabs/crate.prefab.json", CRATE)] {
        let result = load_prefab(source, path, world.registry());
        assert!(!result.has_errors(), "{path}: {:?}", result.diagnostics);
        library.insert(path, result.document.unwrap());
    }
    Arc::new(library)
}

fn load_scene(source: &str, world: &World) -> LoadResult {
    load_with_prefabs(source, SCENE_PATH, world.registry(), library(world))
}

fn scene(world: &World) -> SceneDocument {
    let result = load_scene(SCENE, world);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap()
}

fn text(document: &SceneDocument) -> String {
    document.to_canonical().unwrap()
}

fn tx(ops: Vec<EditOp>) -> Transaction {
    Transaction::new(Actor::Agent("test".to_owned()), ops)
}

/// The line and column of the `nth` occurrence of `needle`.
fn position_of(source: &str, needle: &str, nth: usize) -> Span {
    let offset = source
        .match_indices(needle)
        .nth(nth)
        .expect("needle is in the source")
        .0;
    let before = &source[..offset];
    Span {
        line: before.matches('\n').count() as u32 + 1,
        column: before.rsplit('\n').next().unwrap().chars().count() as u32 + 1,
    }
}

/// An expected error: its code, the text it points at (and which
/// occurrence of it), and a word its hint must contain.
type Expected = (&'static str, &'static str, usize, &'static str);

/// Checks that `result` has an error `code` at the `nth` `needle`, whose
/// hint mentions `word`.
fn expect_error(
    name: &str,
    result: &LoadResult,
    source: &str,
    file: &str,
    (code, needle, nth, word): Expected,
) {
    assert!(
        result.document.is_none(),
        "{name}: an error gives no document"
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error() && diagnostic.code == code)
        .unwrap_or_else(|| panic!("{name}: no {code} among {:?}", result.diagnostics));
    let expected = position_of(source, needle, nth);
    assert_eq!(
        (diagnostic.location.line, diagnostic.location.column),
        (Some(expected.line), Some(expected.column)),
        "{name}: wrong position: {diagnostic}"
    );
    assert_eq!(diagnostic.location.file.as_deref(), Some(file), "{name}");
    assert!(
        diagnostic.hint.contains(word),
        "{name}: the hint {:?} should contain {word:?}",
        diagnostic.hint
    );
}

fn expanded<'a>(entities: &'a [ExpandedEntity], id: &str) -> &'a ExpandedEntity {
    entities
        .iter()
        .find(|entity| entity.id == id)
        .unwrap_or_else(|| panic!("no expanded entity {id}"))
}

#[test]
fn a_scene_expands_its_instances_with_derived_ids() {
    let world = world();
    let document = scene(&world);
    let entities = document.expand().unwrap();
    let ids: Vec<&str> = entities.iter().map(|entity| entity.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "enemy_1",
            "enemy_1/body",
            "enemy_1/weapon",
            "marker",
            "enemy_2",
            "enemy_2/body",
            "enemy_2/weapon",
        ]
    );

    // The instance is the prefab root, with the instance's patch merged in.
    let enemy = expanded(&entities, "enemy_1");
    assert_eq!(enemy.parent, None);
    assert_eq!(
        enemy.components["Transform"],
        json!({ "scale": [2, 2, 2], "translation": [4, 0, 0] })
    );
    // Prefab entities hang below the instance, under derived ids.
    let body = expanded(&entities, "enemy_1/body");
    assert_eq!(body.parent.as_deref(), Some("enemy_1"));
    let weapon = expanded(&entities, "enemy_1/weapon");
    assert_eq!(weapon.parent.as_deref(), Some("enemy_1/body"));
    assert_eq!(weapon.components["Damage"], json!({ "amount": 20 }));
    // The other instance keeps the prefab's values.
    assert_eq!(
        expanded(&entities, "enemy_2/weapon").components["Damage"],
        json!({ "amount": 5 })
    );
    assert_eq!(expanded(&entities, "enemy_2").order, 1);
    // Scene children of an instance stay scene entities.
    assert_eq!(
        expanded(&entities, "marker").parent.as_deref(),
        Some("enemy_1")
    );
}

#[test]
fn an_instance_round_trips_through_the_canonical_form() {
    let world = world();
    let document = scene(&world);
    let canonical = text(&document);
    assert!(canonical.contains("\"prefab\": \"prefabs/enemy.prefab.json\""));
    assert!(canonical.contains("\"overrides\""));
    let again = load_scene(&canonical, &world);
    assert!(
        again.diagnostics.is_empty(),
        "the canonical file loads silently: {:?}",
        again.diagnostics
    );
    let again = again.document.unwrap();
    assert_eq!(again, document);
    assert_eq!(text(&again), canonical);
    assert_eq!(again.prefabs().paths(), document.prefabs().paths());
}

#[test]
fn a_scene_loaded_without_prefabs_names_the_missing_one() {
    let world = world();
    let result = load(SCENE, SCENE_PATH, world.registry());
    expect_error(
        "no library",
        &result,
        SCENE,
        SCENE_PATH,
        (
            "prefab_not_found",
            "\"prefabs/enemy.prefab.json\"",
            0,
            "No prefabs are loaded",
        ),
    );
}

#[test]
fn prefab_file_rules_are_reported_where_they_are_broken() {
    let world = world();
    let file = "prefabs/bad.prefab.json";
    let cases: [(&str, &str, Expected); 5] = [
        (
            "no root",
            r#"{ "format": 1, "entities": {} }"#,
            ("prefab_without_root", "\"entities\"", 0, "root"),
        ),
        (
            "root with a parent",
            r#"{ "format": 1, "entities": { "other": {}, "root": { "parent": "other" } } }"#,
            ("prefab_root_with_parent", "\"other\"", 1, "Remove"),
        ),
        (
            "entity outside the root",
            r#"{ "format": 1, "entities": { "loose": {}, "root": {} } }"#,
            ("prefab_entity_outside_root", "\"loose\"", 0, "parent"),
        ),
        (
            "resources",
            r#"{ "format": 1, "entities": { "root": {} }, "resources": {} }"#,
            ("prefab_with_resources", "\"resources\"", 0, "scene"),
        ),
        (
            "nested prefab",
            r#"{ "format": 1, "entities": { "root": { "prefab": "prefabs/enemy.prefab.json" } } }"#,
            ("nested_prefab", "\"prefab\"", 0, "one level"),
        ),
    ];
    for (name, source, expected) in cases {
        let result = load_prefab(source, file, world.registry());
        expect_error(name, &result, source, file, expected);
    }
}

#[test]
fn instance_mistakes_are_reported_where_they_are() {
    let world = world();
    let wrap =
        |entity: &str| format!("{{\n  \"format\": 1,\n  \"entities\": {{\n    {entity}\n  }}\n}}");
    let cases: [(&str, String, Expected); 8] = [
        (
            "misspelt prefab",
            wrap(r#""e": { "prefab": "prefabs/enemy.prefab.jsn" }"#),
            (
                "prefab_not_found",
                "\"prefabs/enemy.prefab.jsn\"",
                0,
                ENEMY_PATH,
            ),
        ),
        (
            "prefab is not a string",
            wrap(r#""e": { "prefab": 7 }"#),
            ("type_mismatch", "7", 0, "string"),
        ),
        (
            "overrides on a plain entity",
            wrap(r#""e": { "overrides": {} }"#),
            ("overrides_without_prefab", "\"overrides\"", 0, "prefab"),
        ),
        (
            "override of the root",
            wrap(r#""e": { "prefab": "prefabs/enemy.prefab.json", "overrides": { "root": {} } }"#),
            ("override_on_root", "\"root\"", 0, "components"),
        ),
        (
            "override of a missing entity",
            wrap(r#""e": { "prefab": "prefabs/enemy.prefab.json", "overrides": { "wepon": {} } }"#),
            ("unknown_prefab_entity", "\"wepon\"", 0, "weapon"),
        ),
        (
            "unknown key in an override",
            wrap(
                r#""e": { "prefab": "prefabs/enemy.prefab.json", "overrides": { "weapon": { "parent": "x" } } }"#,
            ),
            ("unknown_key", "\"parent\"", 0, "components"),
        ),
        (
            "unknown component in an override",
            wrap(
                r#""e": { "prefab": "prefabs/enemy.prefab.json", "overrides": { "weapon": { "components": { "Damag": {} } } } }"#,
            ),
            ("unknown_component", "\"Damag\"", 0, "Damage"),
        ),
        (
            "bad value in an override",
            wrap(
                r#""e": { "prefab": "prefabs/enemy.prefab.json", "overrides": { "weapon": { "components": { "Damage": { "amount": "lots" } } } } }"#,
            ),
            ("type_mismatch", "\"lots\"", 0, ""),
        ),
    ];
    for (name, source, expected) in &cases {
        let result = load_scene(source, &world);
        expect_error(name, &result, source, SCENE_PATH, *expected);
    }
}

#[test]
fn instances_and_overrides_are_edited_and_undone() {
    let world = world();
    let registry = world.registry();
    let mut document = scene(&world);
    let start = text(&document);

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "body".to_owned(),
        [("Transform".to_owned(), json!({ "translation": [0, 3, 0] }))]
            .into_iter()
            .collect(),
    );
    let applied = document
        .apply(
            &tx(vec![
                EditOp::Instantiate {
                    id: "boss".to_owned(),
                    prefab: ENEMY_PATH.to_owned(),
                    parent: None,
                    order: 2,
                    components: BTreeMap::new(),
                    overrides,
                    notes: Some("The last enemy.".to_owned()),
                },
                EditOp::SetOverride {
                    id: "boss".to_owned(),
                    entity: "weapon".to_owned(),
                    component: "Damage".to_owned(),
                    value: json!({ "amount": 99 }),
                },
                EditOp::RemoveOverride {
                    id: "enemy_1".to_owned(),
                    entity: "weapon".to_owned(),
                    component: "Damage".to_owned(),
                },
            ]),
            registry,
        )
        .unwrap();
    assert_eq!(
        applied.changes,
        [
            "/entities/boss",
            "/entities/boss/overrides/weapon/components/Damage",
            "/entities/enemy_1/overrides/weapon/components/Damage",
        ]
    );
    let entities = document.expand().unwrap();
    assert_eq!(
        expanded(&entities, "boss/weapon").components["Damage"],
        json!({ "amount": 99 })
    );
    assert_eq!(
        expanded(&entities, "boss/body").components["Transform"],
        json!({ "translation": [0, 3, 0] })
    );
    // Removing the patch brings back the prefab's value, and the empty
    // override entry is gone from the file.
    assert_eq!(
        expanded(&entities, "enemy_1/weapon").components["Damage"],
        json!({ "amount": 5 })
    );
    assert!(document.entity("enemy_1").unwrap().overrides().is_empty());

    // Despawning an instance and undoing it brings everything back.
    let before_despawn = text(&document);
    document
        .apply(
            &tx(vec![EditOp::Despawn {
                id: "enemy_1".to_owned(),
            }]),
            registry,
        )
        .unwrap();
    assert!(
        document.entity("marker").is_none(),
        "the scene child goes too"
    );
    document.undo(Actor::Human, registry).unwrap().unwrap();
    assert_eq!(text(&document), before_despawn);

    document.undo(Actor::Human, registry).unwrap().unwrap();
    assert_eq!(text(&document), start);
    document.redo(Actor::Human, registry).unwrap().unwrap();
    assert_eq!(text(&document), before_despawn);
}

#[test]
fn removing_an_instance_component_removes_only_the_patch() {
    let world = world();
    let registry = world.registry();
    let mut document = scene(&world);
    document
        .apply(
            &tx(vec![EditOp::Remove {
                id: "enemy_1".to_owned(),
                component: "Transform".to_owned(),
            }]),
            registry,
        )
        .unwrap();
    let entities = document.expand().unwrap();
    assert_eq!(
        expanded(&entities, "enemy_1").components["Transform"],
        json!({ "scale": [2, 2, 2] })
    );
}

#[test]
fn bad_prefab_edits_are_refused_with_the_right_code() {
    let world = world();
    let registry = world.registry();
    let mut document = scene(&world);
    let start = text(&document);
    let set_override =
        |id: &str, entity: &str, component: &str, value: Value| EditOp::SetOverride {
            id: id.to_owned(),
            entity: entity.to_owned(),
            component: component.to_owned(),
            value,
        };
    let cases = [
        (
            EditOp::instantiate("ghost", "prefabs/ghost.prefab.json"),
            "prefab_not_found",
        ),
        (EditOp::instantiate("enemy_1", ENEMY_PATH), "entity_exists"),
        (
            set_override("marker", "weapon", "Damage", json!({})),
            "not_an_instance",
        ),
        (
            set_override("enemy_1", "root", "Damage", json!({})),
            "override_on_root",
        ),
        (
            set_override("enemy_1", "wepon", "Damage", json!({})),
            "unknown_prefab_entity",
        ),
        (
            set_override("enemy_1", "weapon", "Damag", json!({})),
            "unknown_component",
        ),
        (
            set_override("enemy_1", "weapon", "Damage", json!({ "amount": 5000 })),
            "value_out_of_range",
        ),
        (
            EditOp::RemoveOverride {
                id: "enemy_2".to_owned(),
                entity: "weapon".to_owned(),
                component: "Damage".to_owned(),
            },
            "override_not_found",
        ),
    ];
    for (op, code) in cases {
        let problem = document.apply(&tx(vec![op.clone()]), registry).unwrap_err();
        assert_eq!(problem.code, code, "{op:?}: {problem}");
        assert!(!problem.hint.is_empty(), "{op:?}");
        assert_eq!(problem.location.op, Some(0), "{op:?}");
        assert_eq!(text(&document), start, "{op:?} changed nothing");
    }
}

#[test]
fn an_unknown_override_target_in_an_instantiate_is_refused() {
    let world = world();
    let mut document = scene(&world);
    let mut overrides = BTreeMap::new();
    overrides.insert("shield".to_owned(), BTreeMap::new());
    let problem = document
        .apply(
            &tx(vec![EditOp::Instantiate {
                id: "boss".to_owned(),
                prefab: ENEMY_PATH.to_owned(),
                parent: None,
                order: 0,
                components: BTreeMap::new(),
                overrides,
                notes: None,
            }]),
            world.registry(),
        )
        .unwrap_err();
    assert_eq!(problem.code, "unknown_prefab_entity");
    assert_eq!(
        problem.location.path.as_deref(),
        Some("/entities/boss/overrides/shield")
    );
}

// ---------------------------------------------------------------------------
// Model test: random prefab edits, undo all, redo all.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Step {
    Instantiate(usize, Option<u32>),
    SetOverride(usize, usize, usize, u32),
    RemoveOverride(usize, usize, usize),
    SetTranslation(usize, i32),
    RemoveTransform(usize),
    Despawn(usize),
    Rename(usize, usize),
}

const IDS: [&str; 4] = ["e0", "e1", "e2", "e3"];
const TARGETS: [&str; 4] = ["body", "weapon", "root", "wepon"];
const COMPONENTS: [&str; 3] = ["Damage", "Transform", "Damag"];

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..4usize, proptest::option::of(0..1200u32)).prop_map(|(i, a)| Step::Instantiate(i, a)),
        (0..4usize, 0..4usize, 0..3usize, 0..1200u32)
            .prop_map(|(i, t, c, a)| Step::SetOverride(i, t, c, a)),
        (0..4usize, 0..4usize, 0..3usize).prop_map(|(i, t, c)| Step::RemoveOverride(i, t, c)),
        (0..4usize, -10..10i32).prop_map(|(i, x)| Step::SetTranslation(i, x)),
        (0..4usize).prop_map(Step::RemoveTransform),
        (0..4usize).prop_map(Step::Despawn),
        (0..4usize, 0..4usize).prop_map(|(i, j)| Step::Rename(i, j)),
    ]
}

fn value_for(component: &str, amount: u32) -> Value {
    if component == "Transform" {
        json!({ "translation": [amount, 0, 0] })
    } else {
        json!({ "amount": amount })
    }
}

fn op(step: &Step) -> EditOp {
    match *step {
        Step::Instantiate(i, amount) => {
            let mut overrides = BTreeMap::new();
            if let Some(amount) = amount {
                overrides.insert(
                    "weapon".to_owned(),
                    [("Damage".to_owned(), json!({ "amount": amount }))]
                        .into_iter()
                        .collect(),
                );
            }
            EditOp::Instantiate {
                id: IDS[i].to_owned(),
                prefab: ENEMY_PATH.to_owned(),
                parent: None,
                order: 0,
                components: BTreeMap::new(),
                overrides,
                notes: None,
            }
        }
        Step::SetOverride(i, t, c, amount) => EditOp::SetOverride {
            id: IDS[i].to_owned(),
            entity: TARGETS[t].to_owned(),
            component: COMPONENTS[c].to_owned(),
            value: value_for(COMPONENTS[c], amount),
        },
        Step::RemoveOverride(i, t, c) => EditOp::RemoveOverride {
            id: IDS[i].to_owned(),
            entity: TARGETS[t].to_owned(),
            component: COMPONENTS[c].to_owned(),
        },
        Step::SetTranslation(i, x) => EditOp::Set {
            id: IDS[i].to_owned(),
            component: "Transform".to_owned(),
            value: json!({ "translation": [x, 0, 0] }),
        },
        Step::RemoveTransform(i) => EditOp::Remove {
            id: IDS[i].to_owned(),
            component: "Transform".to_owned(),
        },
        Step::Despawn(i) => EditOp::Despawn {
            id: IDS[i].to_owned(),
        },
        Step::Rename(i, j) => EditOp::Rename {
            id: IDS[i].to_owned(),
            to: IDS[j].to_owned(),
        },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn random_prefab_edits_undo_to_the_start(steps in proptest::collection::vec(step(), 1..60)) {
        let world = world();
        let registry = world.registry();
        let library = library(&world);
        let mut document = SceneDocument::new();
        document.set_prefabs(Arc::clone(&library));
        document.apply(&tx(vec![EditOp::instantiate("e0", ENEMY_PATH)]), registry).unwrap();
        let start = text(&document);
        let mut accepted = 0;
        for step in &steps {
            let before = text(&document);
            match document.apply(&tx(vec![op(step)]), registry) {
                Ok(_) => {
                    accepted += 1;
                    let after = text(&document);
                    let reloaded = load_with_prefabs(&after, SCENE_PATH, registry, Arc::clone(&library));
                    prop_assert!(reloaded.diagnostics.is_empty(), "{:?}: {:?}", step, reloaded.diagnostics);
                    prop_assert_eq!(text(&reloaded.document.unwrap()), after);
                    prop_assert!(document.expand().is_ok());
                }
                Err(_) => prop_assert_eq!(text(&document), before, "{:?}", step),
            }
        }
        let end = text(&document);
        for _ in 0..accepted {
            document.undo(Actor::Human, registry).unwrap().unwrap();
        }
        prop_assert_eq!(text(&document), start.clone());
        for _ in 0..accepted {
            document.redo(Actor::Human, registry).unwrap().unwrap();
        }
        prop_assert_eq!(text(&document), end);
    }
}
