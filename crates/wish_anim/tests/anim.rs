//! The animation system (M6 WP6, closing review).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_anim::AnimationPlayer;
use wish_ecs::World;
use wish_types::render::Model;
use wish_types::time::Time;

#[test]
fn a_model_reference_with_a_sub_part_loops_its_clips() {
    let mut world = World::new();
    wish_anim::register(&mut world).unwrap();
    world.register_component::<Model>().unwrap();
    world.insert_resource(Time::at(0, 60)).unwrap();
    wish_anim::install(
        &mut world,
        [(("models/hero.glb".to_owned(), "walk".to_owned()), 1.0)],
    );
    let hero = world.spawn().unwrap();
    let asset = world.intern("models/hero.glb#Body").unwrap();
    world
        .insert(
            hero,
            Model {
                asset: wish_core::AssetRef::from_text(asset),
            },
        )
        .unwrap();
    let clip = world.intern("walk").unwrap();
    world
        .insert(
            hero,
            AnimationPlayer {
                clip,
                ..AnimationPlayer::default()
            },
        )
        .unwrap();
    for _ in 0..90 {
        wish_anim::step(&mut world).unwrap();
    }
    let time = world.get::<AnimationPlayer>(hero).unwrap().time;
    assert!((0.0..1.0).contains(&time), "it loops: {time}");
}
