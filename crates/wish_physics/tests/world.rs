//! Physics in a real app: bodies fall and rest, the ECS stays the source of
//! truth (teleports, set speeds, removed colliders), events, characters, 2D
//! locks, and snapshots that continue bit for bit (ADR-026).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_app::{App, AppConfig, System};
use wish_core::Entity;
use wish_core::math::Vec3;
use wish_physics::{
    BodyImpulse, BodyKind, BodyVelocity, CharacterController, CharacterMotion, CharacterState,
    Collider, ColliderShape, CollisionStarted, RigidBody,
};
use wish_transform::Transform;

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    wish_physics::register(app.world_mut()).unwrap();
    app.add_system(wish_physics::system()).unwrap();
    app.add_system(
        System::new("transform_propagation", |world, _| {
            wish_transform::update_global_transforms(world)
        })
        .after(wish_physics::SYSTEM),
    )
    .unwrap();
    app
}

fn floor(app: &mut App) -> Entity {
    let world = app.world_mut();
    let floor = world.spawn().unwrap();
    world
        .insert(
            floor,
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
        )
        .unwrap();
    world
        .insert(
            floor,
            Collider {
                size: Vec3::new(40.0, 1.0, 40.0),
                ..Collider::default()
            },
        )
        .unwrap();
    floor
}

fn crate_at(app: &mut App, at: Vec3) -> Entity {
    let world = app.world_mut();
    let entity = world.spawn().unwrap();
    world
        .insert(entity, Transform::from_translation(at))
        .unwrap();
    world.insert(entity, Collider::default()).unwrap();
    world
        .insert(
            entity,
            RigidBody {
                kind: BodyKind::Dynamic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    entity
}

fn y(app: &App, entity: Entity) -> f32 {
    app.world().get::<Transform>(entity).unwrap().translation.y
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }
}

fn pile(app: &mut App) {
    floor(app);
    for i in 0..10 {
        crate_at(
            app,
            Vec3::new((i % 3) as f32 * 0.3, 1.0 + i as f32 * 1.1, 0.0),
        );
    }
}

#[test]
fn a_crate_falls_and_comes_to_rest_on_the_floor() {
    let mut app = app();
    floor(&mut app);
    let crate_ = crate_at(&mut app, Vec3::new(0.0, 5.0, 0.0));
    run(&mut app, 30);
    assert!(y(&app, crate_) < 5.0 && y(&app, crate_) > 0.5, "falling");
    assert!(app.world().get::<BodyVelocity>(crate_).unwrap().linear.y < -1.0);
    run(&mut app, 240);
    assert!(
        (y(&app, crate_) - 0.5).abs() < 0.02,
        "rests at {}",
        y(&app, crate_)
    );
    assert!(
        app.world()
            .get::<BodyVelocity>(crate_)
            .unwrap()
            .linear
            .length()
            < 0.01
    );
}

#[test]
fn going_back_and_continuing_is_the_same_as_never_going_back() {
    let mut app = app();
    pile(&mut app);
    run(&mut app, 40);
    let snapshot = app.snapshot();
    run(&mut app, 200);
    let straight = app.hash();
    app.restore(&snapshot).unwrap();
    run(&mut app, 200);
    assert_eq!(app.hash(), straight, "restore then continue");

    // The same scene in another app gives the same hash.
    let mut other = self::app();
    pile(&mut other);
    run(&mut other, 240);
    assert_eq!(other.hash(), straight, "same scene, same hash");
}

#[test]
fn the_ecs_is_the_source_of_truth() {
    let mut app = app();
    floor(&mut app);
    let crate_ = crate_at(&mut app, Vec3::new(0.0, 0.5, 0.0));
    run(&mut app, 10);
    // A teleport: the body goes where the Transform says.
    app.world_mut()
        .insert(
            crate_,
            Transform::from_translation(Vec3::new(3.0, 4.0, 0.0)),
        )
        .unwrap();
    run(&mut app, 1);
    let t = app.world().get::<Transform>(crate_).unwrap().translation;
    assert!((t.x - 3.0).abs() < 1e-3 && t.y > 3.5, "{t:?}");
    // A set speed: the body takes it.
    app.world_mut()
        .insert(
            crate_,
            BodyVelocity {
                linear: Vec3::new(5.0, 0.0, 0.0),
                angular: Vec3::ZERO,
            },
        )
        .unwrap();
    run(&mut app, 1);
    assert!(app.world().get::<BodyVelocity>(crate_).unwrap().linear.x > 4.0);
    // An impulse kicks it up (a 1000 kg crate: 5000 N*s is 5 m/s).
    let before = app.world().get::<BodyVelocity>(crate_).unwrap().linear.y;
    app.world_mut()
        .send(BodyImpulse {
            entity: crate_,
            linear: Vec3::new(0.0, 5000.0, 0.0),
            angular: Vec3::ZERO,
        })
        .unwrap();
    run(&mut app, 1);
    assert!(app.world().get::<BodyVelocity>(crate_).unwrap().linear.y > before + 4.0);
    // A changed collider is rebuilt (and the sleeping body woken): a small
    // box falls a little and rests lower.
    run(&mut app, 300);
    app.world_mut()
        .insert(
            crate_,
            Collider {
                size: Vec3::splat(0.5),
                ..Collider::default()
            },
        )
        .unwrap();
    run(&mut app, 300);
    assert!((y(&app, crate_) - 0.25).abs() < 0.02, "{}", y(&app, crate_));
    // Removing the collider takes the entity out of physics: it stays put.
    app.world_mut().remove::<Collider>(crate_).unwrap();
    app.world_mut()
        .insert(
            crate_,
            Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)),
        )
        .unwrap();
    run(&mut app, 30);
    assert_eq!(y(&app, crate_), 10.0);
}

#[test]
fn touching_and_sensors_send_events() {
    let mut app = app();
    let floor = floor(&mut app);
    let crate_ = crate_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
    let zone = {
        let world = app.world_mut();
        let zone = world.spawn().unwrap();
        world
            .insert(zone, Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)))
            .unwrap();
        world
            .insert(
                zone,
                Collider {
                    sensor: true,
                    size: Vec3::splat(0.5),
                    ..Collider::default()
                },
            )
            .unwrap();
        zone
    };
    let mut started = Vec::new();
    for _ in 0..90 {
        app.step().unwrap();
        for event in app.world().events::<CollisionStarted>().unwrap() {
            if !started.contains(&event) {
                started.push(event);
            }
        }
    }
    let pairs: Vec<(Entity, Entity, bool)> = started.iter().map(|e| (e.a, e.b, e.sensor)).collect();
    assert!(pairs.contains(&(floor, crate_, false)), "{pairs:?}");
    let sensor = if crate_ < zone {
        (crate_, zone, true)
    } else {
        (zone, crate_, true)
    };
    assert!(pairs.contains(&sensor), "{pairs:?}");
}

#[test]
fn a_character_walks_up_a_step_and_is_stopped_by_a_wall() {
    let mut app = app();
    floor(&mut app);
    let world = app.world_mut();
    // A 0.2 m step from x = 2, a wall from x = 6.
    for (at, size) in [
        (Vec3::new(4.0, 0.1, 0.0), Vec3::new(4.0, 0.2, 4.0)),
        (Vec3::new(6.5, 1.5, 0.0), Vec3::new(1.0, 3.0, 4.0)),
    ] {
        let block = world.spawn().unwrap();
        world
            .insert(block, Transform::from_translation(at))
            .unwrap();
        world
            .insert(
                block,
                Collider {
                    size,
                    ..Collider::default()
                },
            )
            .unwrap();
    }
    let hero = world.spawn().unwrap();
    world
        .insert(hero, Transform::from_translation(Vec3::new(0.0, 0.92, 0.0)))
        .unwrap();
    world
        .insert(
            hero,
            Collider {
                shape: ColliderShape::Capsule,
                size: Vec3::new(0.6, 1.8, 0.6),
                ..Collider::default()
            },
        )
        .unwrap();
    world
        .insert(
            hero,
            RigidBody {
                kind: BodyKind::Kinematic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    world.insert(hero, CharacterController::default()).unwrap();
    world
        .insert(
            hero,
            CharacterMotion {
                velocity: Vec3::new(2.0, -2.0, 0.0),
            },
        )
        .unwrap();
    run(&mut app, 30);
    let state = *app.world().get::<CharacterState>(hero).unwrap();
    assert!(state.grounded, "{state:?}");
    run(&mut app, 150);
    let t = app.world().get::<Transform>(hero).unwrap().translation;
    assert!(t.y > 1.05, "climbed the step: {t:?}");
    assert!(t.x < 5.75, "the wall stops it: {t:?}");
    let state = *app.world().get::<CharacterState>(hero).unwrap();
    assert!(state.blocked, "{state:?}");
}

#[test]
fn locked_axes_make_a_2d_game() {
    let mut app = app();
    floor(&mut app);
    let ball = crate_at(&mut app, Vec3::new(0.0, 3.0, 0.0));
    app.world_mut()
        .insert(
            ball,
            RigidBody {
                kind: BodyKind::Dynamic,
                lock_translation: [false, false, true],
                lock_rotation: [true, true, false],
                ..RigidBody::default()
            },
        )
        .unwrap();
    app.world_mut()
        .insert(
            ball,
            BodyVelocity {
                linear: Vec3::new(1.0, 0.0, 3.0),
                angular: Vec3::new(2.0, 2.0, 0.0),
            },
        )
        .unwrap();
    run(&mut app, 60);
    let t = *app.world().get::<Transform>(ball).unwrap();
    assert_eq!(t.translation.z, 0.0, "{t:?}");
    assert!(t.translation.x > 0.5);
    // Only a turn about Z: the quaternion has no X or Y part.
    assert!(
        t.rotation.x.abs() < 1e-4 && t.rotation.y.abs() < 1e-4,
        "{t:?}"
    );
}

#[test]
fn scenes_write_physics_as_json() {
    let mut app = app();
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .set_json(
            e,
            "Collider",
            &json!({ "shape": "Sphere", "size": [2, 2, 2] }),
        )
        .unwrap();
    world
        .set_json(
            e,
            "RigidBody",
            &json!({ "kind": "Dynamic", "gravity_scale": 0 }),
        )
        .unwrap();
    world.set_json(e, "Transform", &json!({})).unwrap();
    run(&mut app, 10);
    assert_eq!(y(&app, e), 0.0, "gravity scale 0 floats");
}

/// The world after a pile of crates settles, hashed: the same on every
/// platform (CI runs it on Windows and Linux). If it changes, ask first
/// whether a deliberate change to physics or the world hash explains it.
#[test]
fn the_recorded_world_hash_holds() {
    let mut app = app();
    pile(&mut app);
    run(&mut app, 600);
    let hash = app.world().hash().to_string();
    println!("pile after 600 ticks: {hash}");
    assert_eq!(hash, RECORDED_PILE);
}

const RECORDED_PILE: &str = "ffb4e84321e450fe594a7d4cbde3a7d7";

#[test]
fn rays_overlaps_and_contacts_answer_questions() {
    use wish_core::math::Quat;
    use wish_physics::{OverlapShape, query_state};
    let mut app = app();
    let floor = floor(&mut app);
    let crate_ = crate_at(&mut app, Vec3::new(2.0, 0.5, 0.0));
    // Before any step: a state is built for the question.
    {
        let state = query_state(app.world()).unwrap();
        let hit = state
            .raycast(Vec3::new(2.0, 5.0, 0.0), Vec3::NEG_Y, 10.0, u32::MAX, None)
            .unwrap()
            .unwrap();
        assert_eq!(hit.entity, crate_);
        assert!((hit.distance - 4.0).abs() < 1e-4, "{hit:?}");
        assert!((hit.normal - Vec3::Y).length() < 1e-4);
    }
    run(&mut app, 5);
    let state = query_state(app.world()).unwrap();
    // Excluding the crate, the ray reaches the floor.
    let hit = state
        .raycast(
            Vec3::new(2.0, 5.0, 0.0),
            Vec3::NEG_Y,
            10.0,
            u32::MAX,
            Some(crate_),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.entity, floor);
    assert!((hit.point.y).abs() < 1e-3, "{hit:?}");
    // Nothing above.
    assert!(
        state
            .raycast(Vec3::new(2.0, 5.0, 0.0), Vec3::Y, 10.0, u32::MAX, None)
            .unwrap()
            .is_none()
    );
    // A sphere around the crate overlaps it and the floor.
    let found = state
        .overlap(
            OverlapShape::Sphere(0.8),
            Vec3::new(2.0, 0.5, 0.0),
            Quat::IDENTITY,
            u32::MAX,
        )
        .unwrap();
    assert_eq!(found, {
        let mut v = vec![floor, crate_];
        v.sort();
        v
    });
    // The crate touches the floor.
    assert_eq!(state.touching(crate_), vec![floor]);
    // Bad questions are explained.
    let bad = state
        .raycast(Vec3::ZERO, Vec3::ZERO, 10.0, u32::MAX, None)
        .unwrap_err();
    assert_eq!(bad.code, "invalid_parameter");
    assert!(!bad.hint.is_empty());
}

#[test]
fn a_bouncy_ball_bounces_off_a_dull_floor() {
    // Restitution 1 on the ball alone: the bouncier of the two decides, so
    // it comes back up most of the way (M6 closing review).
    let mut app = app();
    floor(&mut app);
    let ball = crate_at(&mut app, Vec3::new(0.0, 4.0, 0.0));
    app.world_mut()
        .insert(
            ball,
            Collider {
                shape: ColliderShape::Sphere,
                restitution: 1.0,
                ..Collider::default()
            },
        )
        .unwrap();
    let mut touched = false;
    let mut highest = 0.0f32;
    for _ in 0..240 {
        run(&mut app, 1);
        let height = y(&app, ball);
        if height < 0.6 {
            touched = true;
        } else if touched {
            highest = highest.max(height);
        }
    }
    assert!(touched);
    assert!(highest > 3.0, "bounced back up to {highest}");
}

#[test]
fn contacts_that_exist_at_the_start_are_reported() {
    // A crate already in a sensor zone when the session prepares physics
    // (M6 closing review): the first tick still says it started touching.
    let mut app = app();
    let zone = {
        let world = app.world_mut();
        let zone = world.spawn().unwrap();
        world.insert(zone, Transform::default()).unwrap();
        world
            .insert(
                zone,
                Collider {
                    size: Vec3::splat(4.0),
                    sensor: true,
                    ..Collider::default()
                },
            )
            .unwrap();
        zone
    };
    let inside = crate_at(&mut app, Vec3::ZERO);
    app.world_mut()
        .insert(
            inside,
            RigidBody {
                kind: BodyKind::Kinematic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    wish_transform::update_global_transforms(app.world_mut()).unwrap();
    wish_physics::prepare(app.world_mut()).unwrap();
    run(&mut app, 1);
    assert_eq!(app.world().events_sent("CollisionStarted").unwrap(), 1);
    let _ = zone;
}

#[test]
fn removing_a_touching_collider_says_they_stopped_touching() {
    let mut app = app();
    let ground = floor(&mut app);
    let resting = crate_at(&mut app, Vec3::new(0.0, 0.5, 0.0));
    run(&mut app, 30);
    assert_eq!(app.world().events_sent("CollisionEnded").unwrap(), 0);
    app.world_mut().despawn(resting).unwrap();
    run(&mut app, 1);
    assert_eq!(
        app.world().events_sent("CollisionEnded").unwrap(),
        1,
        "the floor hears that the crate is gone"
    );
    let _ = ground;
}

#[test]
fn a_walking_character_touches_the_ground_once() {
    // The controller keeps a skin of air under the character; walking must
    // not start and end the contact with the floor every tick (M7 friction
    // log, F4).
    let mut app = app();
    floor(&mut app);
    let world = app.world_mut();
    let hero = world.spawn().unwrap();
    world
        .insert(hero, Transform::from_translation(Vec3::new(0.0, 0.92, 0.0)))
        .unwrap();
    world
        .insert(
            hero,
            Collider {
                shape: ColliderShape::Capsule,
                size: Vec3::new(0.6, 1.8, 0.6),
                ..Collider::default()
            },
        )
        .unwrap();
    world
        .insert(
            hero,
            RigidBody {
                kind: BodyKind::Kinematic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    world.insert(hero, CharacterController::default()).unwrap();
    world
        .insert(
            hero,
            CharacterMotion {
                velocity: Vec3::new(3.0, -1.0, 0.0),
            },
        )
        .unwrap();
    run(&mut app, 120);
    let world = app.world();
    assert!(world.get::<CharacterState>(hero).unwrap().grounded);
    assert_eq!(world.events_sent("CollisionStarted").unwrap(), 1);
    assert_eq!(world.events_sent("CollisionEnded").unwrap(), 0);
}
