//! The action map and the state it writes into the world.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use wish_ecs::World;
use wish_input::{INPUT_FILE, InputMap, Values, apply, key_names, register};
use wish_types::{ActionState, action_id};

fn keys(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

#[test]
fn the_starter_map_round_trips_and_evaluates_keys() {
    let map = InputMap::starter();
    let text = map.to_canonical().unwrap();
    assert_eq!(InputMap::parse(&text, INPUT_FILE).unwrap(), map);
    assert_eq!(map.names().collect::<Vec<_>>(), ["jump", "move"]);

    let at_rest = map.evaluate(&keys(&[]));
    assert_eq!(at_rest["move"], [0.0, 0.0]);
    let forward = map.evaluate(&keys(&["W"]));
    assert_eq!(forward["move"], [0.0, 1.0]);
    // Opposite keys cancel; the diagonal is normalized.
    assert_eq!(map.evaluate(&keys(&["A", "D"]))["move"], [0.0, 0.0]);
    let diagonal = map.evaluate(&keys(&["W", "D"]))["move"];
    assert!((diagonal[0].hypot(diagonal[1]) - 1.0).abs() < 1e-6);
    assert_eq!(map.evaluate(&keys(&["Space"]))["jump"], [1.0, 0.0]);
    // The arrow keys do the same as WASD.
    assert_eq!(map.evaluate(&keys(&["Up"]))["move"], [0.0, 1.0]);
}

#[test]
fn mistakes_are_explained_where_they_are() {
    let parse = |text: &str| InputMap::parse(text, INPUT_FILE).unwrap_err();
    let bad_key =
        parse(r#"{ "format": 1, "actions": { "jump": { "kind": "button", "keys": ["Spcae"] } } }"#);
    assert_eq!(bad_key.code, "invalid_input_file");
    assert_eq!(bad_key.hint, "Did you mean 'Space'?");
    let unknown_kind = parse(r#"{ "format": 1, "actions": { "x": { "kind": "stick" } } }"#);
    assert!(unknown_kind.message.contains("stick"), "{unknown_kind:?}");
    assert!(unknown_kind.hint.contains("axis2"));
    let unknown_field =
        parse(r#"{ "format": 1, "actions": { "x": { "kind": "button", "key": ["A"] } } }"#);
    assert!(unknown_field.message.contains("key"), "{unknown_field:?}");
    assert_eq!(unknown_field.location.line, Some(1));
    assert!(
        parse(r#"{ "format": 2, "actions": {} }"#)
            .message
            .contains("format 2")
    );
    assert!(
        parse(r#"{ "format": 1, "actions": { "two words": { "kind": "button" } } }"#)
            .message
            .contains("two words")
    );
    let many: String = (0..33)
        .map(|n| format!("\"a{n}\": {{ \"kind\": \"button\" }}"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(
        parse(&format!(r#"{{ "format": 1, "actions": {{ {many} }} }}"#))
            .message
            .contains("33 actions")
    );
    assert!(key_names().contains(&"MouseLeft".to_owned()));
}

#[test]
fn the_world_sees_actions_with_pressed_and_released_edges() {
    let map = InputMap::starter();
    let mut world = World::new();
    register(&mut world).unwrap();
    let step = |world: &mut World, down: &[&str]| {
        let values: Values = map.evaluate(&keys(down));
        apply(world, &map, &values).unwrap();
        *world.resource::<ActionState>().unwrap()
    };

    let state = step(&mut world, &[]);
    assert!(!state.slot("jump").unwrap().held);
    let state = step(&mut world, &["Space", "D"]);
    let jump = state.slot("jump").unwrap();
    assert!(jump.held && jump.pressed && !jump.released);
    assert_eq!(state.slot("move").unwrap().x, 1.0);
    // Still held: no longer "pressed".
    let state = step(&mut world, &["Space", "D"]);
    let jump = state.slot("jump").unwrap();
    assert!(jump.held && !jump.pressed);
    let state = step(&mut world, &[]);
    let jump = state.slot("jump").unwrap();
    assert!(!jump.held && jump.released);
    assert!(state.slot("nothing").is_none());
    assert_eq!(state.slot("jump").unwrap().id, action_id("jump"));

    // Values from outside are cleaned: NaN is rest, the rest is clamped.
    let mut values = Values::new();
    values.insert("move".to_owned(), [f32::NAN, 9.0]);
    apply(&mut world, &map, &values).unwrap();
    let state = *world.resource::<ActionState>().unwrap();
    assert_eq!(
        (state.slot("move").unwrap().x, state.slot("move").unwrap().y),
        (0.0, 1.0)
    );
}
