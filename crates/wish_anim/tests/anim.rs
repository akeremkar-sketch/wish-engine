//! The animation system (M6 WP6, closing review).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_anim::{AnimationPlayer, SpriteAnimator, SpriteClip};
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

/// A world with one sprite animator on the sheet `sprites/hero.sheet.json`,
/// whose clips are `walk` (4 frames at 8 a second, looping) and `fall`
/// (3 frames at 6 a second, once).
fn sprite_world(clip: &str) -> (World, wish_core::Entity) {
    let mut world = World::new();
    wish_anim::register(&mut world).unwrap();
    world.insert_resource(Time::at(0, 60)).unwrap();
    let sheet = "sprites/hero.sheet.json".to_owned();
    wish_anim::install_sprite_clips(
        &mut world,
        [
            (
                (sheet.clone(), "walk".to_owned()),
                SpriteClip {
                    frames: 4,
                    fps: 8.0,
                    looping: true,
                },
            ),
            (
                (sheet.clone(), "fall".to_owned()),
                SpriteClip {
                    frames: 3,
                    fps: 6.0,
                    looping: false,
                },
            ),
        ],
    );
    let hero = world.spawn().unwrap();
    let sheet = world.intern(&sheet).unwrap();
    let clip = world.intern(clip).unwrap();
    world
        .insert(
            hero,
            SpriteAnimator {
                sheet: wish_core::AssetRef::from_text(sheet),
                clip,
                ..SpriteAnimator::default()
            },
        )
        .unwrap();
    (world, hero)
}

#[test]
fn a_sprite_clip_loops_and_its_frame_follows_the_tick_s_time() {
    let (mut world, hero) = sprite_world("walk");
    let mut frames = Vec::new();
    for _ in 0..40 {
        wish_anim::step(&mut world).unwrap();
        frames.push(world.get::<SpriteAnimator>(hero).unwrap().frame);
    }
    // 8 frames a second at 60 ticks a second: a frame every 7.5 ticks, four
    // frames, then the first again (at tick 30). Ticks on a frame's edge
    // are left out: which side a sum of sixtieths falls on is the float's.
    assert_eq!(&frames[..7], [0, 0, 0, 0, 0, 0, 0]);
    assert_eq!((frames[7], frames[12], frames[16]), (1, 1, 2));
    assert_eq!(
        (frames[25], frames[31], frames[35], frames[37]),
        (3, 0, 0, 1),
        "{frames:?}"
    );
    let animator = world.get::<SpriteAnimator>(hero).unwrap();
    assert!((0.0..0.5).contains(&animator.time), "wrapped: {animator:?}");
    assert!(animator.playing);
}

#[test]
fn a_sprite_clip_that_does_not_loop_stops_on_its_last_frame() {
    let (mut world, hero) = sprite_world("fall");
    // 3 frames at 6 a second: half a second, 30 ticks.
    for _ in 0..29 {
        wish_anim::step(&mut world).unwrap();
    }
    assert!(world.get::<SpriteAnimator>(hero).unwrap().playing);
    assert_eq!(world.events_sent("AnimationFinished").unwrap(), 0);
    wish_anim::step(&mut world).unwrap();
    wish_anim::step(&mut world).unwrap();
    let animator = *world.get::<SpriteAnimator>(hero).unwrap();
    assert!(!animator.playing);
    assert_eq!(animator.frame, 2);
    assert_eq!(animator.time, 0.5);
    assert_eq!(world.events_sent("AnimationFinished").unwrap(), 1);
    // It stays there, and says so once.
    for _ in 0..10 {
        wish_anim::step(&mut world).unwrap();
    }
    assert_eq!(world.events_sent("AnimationFinished").unwrap(), 1);
    assert_eq!(world.get::<SpriteAnimator>(hero).unwrap().frame, 2);
}

#[test]
fn play_switches_the_clip_and_starts_it_over() {
    let (mut world, hero) = sprite_world("walk");
    for _ in 0..20 {
        wish_anim::step(&mut world).unwrap();
    }
    let walking = *world.get::<SpriteAnimator>(hero).unwrap();
    assert_eq!(walking.frame, 2);
    // The same clip again keeps it running.
    let mut same = walking;
    same.play(walking.clip);
    assert_eq!(same, walking);
    // Another clip starts at its beginning.
    let fall = world.intern("fall").unwrap();
    let mut falling = walking;
    falling.play(fall);
    assert_eq!((falling.time, falling.frame, falling.clip), (0.0, 0, fall));
    world.insert(hero, falling).unwrap();
    for _ in 0..12 {
        wish_anim::step(&mut world).unwrap();
    }
    assert_eq!(world.get::<SpriteAnimator>(hero).unwrap().frame, 1);
}

#[test]
fn an_animator_without_a_known_clip_or_with_odd_numbers_stays_calm() {
    // No clip: a still frame, left as written.
    let (mut world, hero) = sprite_world("");
    let mut still = *world.get::<SpriteAnimator>(hero).unwrap();
    still.frame = 5;
    world.insert(hero, still).unwrap();
    // An unknown clip: nothing moves.
    let (mut other, ghost) = sprite_world("swim");
    for _ in 0..10 {
        wish_anim::step(&mut world).unwrap();
        wish_anim::step(&mut other).unwrap();
    }
    assert_eq!(*world.get::<SpriteAnimator>(hero).unwrap(), still);
    assert_eq!(other.get::<SpriteAnimator>(ghost).unwrap().time, 0.0);
    // Numbers game code should not write: not a number, out of range.
    let (mut odd, hero) = sprite_world("walk");
    let mut wild = *odd.get::<SpriteAnimator>(hero).unwrap();
    wild.time = f32::NAN;
    wild.speed = f32::INFINITY;
    odd.insert(hero, wild).unwrap();
    wish_anim::step(&mut odd).unwrap();
    let calm = *odd.get::<SpriteAnimator>(hero).unwrap();
    assert!(calm.time.is_finite() && calm.frame < 4, "{calm:?}");
    wild.time = 1.0e30;
    wild.speed = -1.0e30;
    odd.insert(hero, wild).unwrap();
    wish_anim::step(&mut odd).unwrap();
    let calm = *odd.get::<SpriteAnimator>(hero).unwrap();
    assert!(calm.time.is_finite() && calm.frame < 4, "{calm:?}");
}
