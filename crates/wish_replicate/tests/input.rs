//! Input frames written by the host give exactly what `wish_input` writes
//! for the same values: cleaning, edges and all.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_ecs::World;
use wish_input::{InputMap, Values};
use wish_replicate::input::{ensure_players, write_players};
use wish_types::{ActionState, PlayerActionState, action_id};

fn map() -> InputMap {
    InputMap::starter()
}

fn world() -> World {
    let mut world = World::new();
    wish_input::register(&mut world).unwrap();
    world
}

#[test]
fn host_written_actions_equal_wish_inputs() {
    let map = map();
    let mut by_input = world();
    let mut by_host = world();
    let steps: Vec<[Values; 3]> = (0..6)
        .map(|step| {
            let mut a = Values::new();
            let mut b = Values::new();
            let mut c = Values::new();
            if step % 2 == 0 {
                a.insert("jump".to_owned(), [1.0, 0.0]);
            }
            b.insert("move".to_owned(), [f32::NAN, 7.0]);
            if step > 2 {
                c.insert("move".to_owned(), [-0.5, 0.25]);
                c.insert("jump".to_owned(), [1.0, 0.0]);
            }
            [a, b, c]
        })
        .collect();
    for values in &steps {
        wish_input::apply_players(&mut by_input, &map, values).unwrap();

        // The host: its own player through wish_input, the others as
        // frames of action ids (what clients send).
        wish_input::apply(&mut by_host, &map, &values[0]).unwrap();
        ensure_players(&mut by_host, 3).unwrap();
        let frames: Vec<Vec<(u32, [f32; 2])>> = values
            .iter()
            .map(|v| v.iter().map(|(name, xy)| (action_id(name), *xy)).collect())
            .collect();
        write_players(&mut by_host, &|player| frames.get(player as usize).cloned()).unwrap();

        assert_eq!(
            by_input.resource::<PlayerActionState>(),
            by_host.resource::<PlayerActionState>()
        );
        assert_eq!(
            by_input.resource::<ActionState>(),
            by_host.resource::<ActionState>()
        );
        assert_eq!(by_input.hash(), by_host.hash());
    }
}

#[test]
fn one_player_writes_nothing_new() {
    let map = map();
    let mut world = world();
    wish_input::apply(&mut world, &map, &Values::new()).unwrap();
    let before = world.hash();
    write_players(&mut world, &|_| None).unwrap();
    assert_eq!(world.hash(), before);
}
