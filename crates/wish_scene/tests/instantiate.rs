//! Turning scene documents into worlds.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::json;
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_scene::{PrefabLibrary, SceneDocument, load, load_prefab, load_with_prefabs};
use wish_schema::{component, resource};

#[component]
pub struct Transform {
    pub translation: Vec3,
}

#[component]
pub struct Damage {
    #[schema(default = 10.0)]
    pub amount: f32,
}

#[resource]
pub struct Gravity {
    pub value: Vec3,
}

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Transform>().unwrap();
    world.register_component::<Damage>().unwrap();
    world.register_resource::<Gravity>().unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
}

const ENEMY: &str = r#"{
  "format": 1,
  "entities": {
    "root": {},
    "body": { "parent": "root", "components": { "Transform": { "translation": [0, 1, 0] } } },
    "weapon": { "parent": "body", "components": { "Damage": { "amount": 5 } } }
  }
}"#;

const SCENE: &str = r#"{
  "format": 1,
  "entities": {
    "camera": { "order": 1, "parent": "player" },
    "enemy_1": {
      "overrides": { "weapon": { "components": { "Damage": { "amount": 20 } } } },
      "prefab": "enemy.prefab.json"
    },
    "player": { "components": { "Transform": { "translation": [0, 1, 0] } } },
    "sword": { "order": 0, "parent": "player" }
  },
  "resources": { "Gravity": { "value": [0, -9.81, 0] } }
}"#;

fn document(world: &World) -> SceneDocument {
    let prefab = load_prefab(ENEMY, "enemy.prefab.json", world.registry());
    assert!(!prefab.has_errors(), "{:?}", prefab.diagnostics);
    let mut library = PrefabLibrary::new();
    library.insert("enemy.prefab.json", prefab.document.unwrap());
    let result = load_with_prefabs(
        SCENE,
        "main.scene.json",
        world.registry(),
        Arc::new(library),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result.document.unwrap()
}

fn scene_id(world: &World, entity: wish_core::Entity) -> String {
    let value = world.get_json(entity, "SceneId").unwrap();
    value["id"].as_str().unwrap().to_owned()
}

#[test]
fn a_scene_becomes_entities_with_ids_hierarchy_and_resources() {
    let mut world = world();
    let entities = document(&world).instantiate(&mut world).unwrap();
    // player, camera, sword, enemy_1 and its two prefab entities.
    assert_eq!(world.entity_count(), 6);
    assert_eq!(entities.len(), 6);
    for (id, entity) in &entities {
        assert_eq!(&scene_id(&world, *entity), id);
    }
    // Hierarchy: siblings follow `order`, not the ids' alphabetical order.
    let player = entities["player"];
    let children: Vec<String> = world
        .children(player)
        .into_iter()
        .map(|child| scene_id(&world, child))
        .collect();
    assert_eq!(children, ["sword", "camera"]);
    assert_eq!(
        world.parent(entities["enemy_1/weapon"]),
        Some(entities["enemy_1/body"])
    );
    assert_eq!(
        world.parent(entities["enemy_1/body"]),
        Some(entities["enemy_1"])
    );
    assert_eq!(world.parent(entities["enemy_1"]), None);
    // Values, prefab overrides, resources.
    assert_eq!(
        world
            .get_json(entities["enemy_1/weapon"], "Damage")
            .unwrap(),
        json!({ "amount": 20.0 })
    );
    assert_eq!(
        world.resource_json("Gravity").unwrap(),
        json!({ "value": [0.0, -9.81, 0.0] })
    );
}

#[test]
fn the_same_document_gives_the_same_world_hash() {
    let mut first = world();
    let mut second = world();
    document(&first).instantiate(&mut first).unwrap();
    document(&second).instantiate(&mut second).unwrap();
    assert_eq!(first.hash(), second.hash());
    let mut third = world();
    SceneDocument::new().instantiate(&mut third).unwrap();
    assert_ne!(first.hash(), third.hash());
}

#[test]
fn scenes_cannot_write_the_scene_id() {
    let world = world();
    let source =
        r#"{ "format": 1, "entities": { "a": { "components": { "SceneId": { "id": "x" } } } } }"#;
    let result = load(source, "", world.registry());
    let error = result.diagnostics.iter().find(|d| d.is_error()).unwrap();
    assert_eq!(error.code, "managed_component");
    assert!(error.hint.contains("Remove"));
}
