//! Changing the layout of a game-declared type while the world exists
//! (game code reloaded mid-game, M5): values move by field name, ticks are
//! kept, what does not fit is reported, engine types are refused.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_ecs::World;
use wish_schema::Plain;

mod v1 {
    use wish_schema::{component, event, resource};

    /// A coin.
    #[component]
    pub struct Coin {
        /// Points.
        pub value: u32,
        /// Going away.
        pub old: bool,
        /// A name.
        pub label: wish_core::Text,
    }

    /// The score.
    #[resource]
    pub struct Score {
        /// Points.
        pub points: u32,
    }

    /// Picked up.
    #[event]
    pub struct Picked {
        /// Points.
        pub value: u32,
    }
}

mod v2 {
    use wish_schema::{component, event, resource};

    /// A coin, version two.
    #[component]
    pub struct Coin {
        /// A name, now first.
        pub label: wish_core::Text,
        /// Points, now a float.
        pub value: f32,
        /// New, with a default.
        #[schema(default = 2.5)]
        pub bonus: f32,
    }

    /// The score, version two.
    #[resource]
    pub struct Score {
        /// Points.
        pub points: u64,
        /// New.
        pub best: u64,
    }

    /// Picked up, version two.
    #[event]
    pub struct Picked {
        /// Points.
        pub value: f64,
    }
}

#[test]
fn a_game_type_changes_its_fields_and_keeps_its_values() {
    let mut world = World::new();
    let coin = world.register_component_schema(v1::Coin::schema()).unwrap();
    let score = world.register_resource_schema(v1::Score::schema()).unwrap();
    let picked = world.register_event_schema(v1::Picked::schema()).unwrap();
    let mut coins = Vec::new();
    for (value, label) in [(5, "gold"), (1, "copper")] {
        let entity = world.spawn().unwrap();
        world
            .set_json(
                entity,
                "Coin",
                &json!({ "value": value, "old": true, "label": label }),
            )
            .unwrap();
        coins.push(entity);
    }
    world.advance_change_tick();
    world
        .set_json(coins[1], "Coin", &json!({ "value": 3 }))
        .unwrap();
    let ticks_before: Vec<_> = coins
        .iter()
        .map(|e| world.component_ticks(*e, coin).unwrap())
        .collect();
    world
        .set_resource_json("Score", &json!({ "points": 9 }))
        .unwrap();
    world.send_json("Picked", &json!({ "value": 1 })).unwrap();

    let moved = world.retype_component(coin, v2::Coin::schema()).unwrap();
    assert_eq!(moved.values, 2);
    assert_eq!(moved.dropped, ["old"]);
    assert_eq!(
        world.get_json(coins[0], "Coin").unwrap(),
        json!({ "label": "gold", "value": 5.0, "bonus": 2.5 })
    );
    assert_eq!(
        world.get_json(coins[1], "Coin").unwrap(),
        json!({ "label": "copper", "value": 3.0, "bonus": 2.5 })
    );
    let ticks_after: Vec<_> = coins
        .iter()
        .map(|e| world.component_ticks(*e, coin).unwrap())
        .collect();
    assert_eq!(ticks_after, ticks_before, "when values changed is kept");
    // The world now stores the new layout.
    assert_eq!(
        world.registry().component(coin).unwrap().schema(),
        &v2::Coin::schema()
    );
    let entity = world.spawn().unwrap();
    world
        .set_json(entity, "Coin", &json!({ "bonus": 1 }))
        .unwrap();

    let moved = world.retype_resource(score, v2::Score::schema()).unwrap();
    assert_eq!(moved.values, 1);
    assert_eq!(
        world.resource_json("Score").unwrap(),
        json!({ "points": 9, "best": 0 })
    );
    let moved = world.retype_event(picked, v2::Picked::schema()).unwrap();
    assert_eq!(moved.dropped, ["(buffered events)"]);
    assert_eq!(world.events_json("Picked").unwrap(), json!([]));
    world.send_json("Picked", &json!({ "value": 0.5 })).unwrap();

    // The same changes twice give the same world.
    let again = {
        let mut world = World::new();
        let coin = world.register_component_schema(v1::Coin::schema()).unwrap();
        for (value, label) in [(5, "gold"), (1, "copper")] {
            let entity = world.spawn().unwrap();
            world
                .set_json(
                    entity,
                    "Coin",
                    &json!({ "value": value, "old": true, "label": label }),
                )
                .unwrap();
        }
        world.retype_component(coin, v2::Coin::schema()).unwrap();
        world.hash()
    };
    let twice = {
        let mut world = World::new();
        let coin = world.register_component_schema(v1::Coin::schema()).unwrap();
        for (value, label) in [(5, "gold"), (1, "copper")] {
            let entity = world.spawn().unwrap();
            world
                .set_json(
                    entity,
                    "Coin",
                    &json!({ "value": value, "old": true, "label": label }),
                )
                .unwrap();
        }
        world.retype_component(coin, v2::Coin::schema()).unwrap();
        world.hash()
    };
    assert_eq!(again, twice);
}

#[test]
fn engine_types_and_renames_are_refused() {
    let mut world = World::new();
    // Bound to a Rust type: native code reads its bytes directly.
    let coin = world.register_component::<v1::Coin>().unwrap();
    let refused = world
        .retype_component(coin, v2::Coin::schema())
        .unwrap_err();
    assert_eq!(refused.code, "retype_refused");
    assert!(!refused.hint.is_empty());
    let mut other = World::new();
    let score = other.register_resource_schema(v1::Score::schema()).unwrap();
    let mut renamed = v2::Score::schema();
    renamed.name = "Points".to_owned();
    assert_eq!(
        other.retype_resource(score, renamed).unwrap_err().code,
        "retype_refused"
    );
}
