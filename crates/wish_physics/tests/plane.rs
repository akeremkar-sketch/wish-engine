//! The 2D profiles (design note 1.1-physics-2d): walls stop a top-down
//! character, a side-view character jumps and lands, everything stays in
//! the plane whatever its depth, queries are asked in the plane, and a 2D
//! world is as deterministic as a 3D one (same hash twice, snapshot and
//! restore bit for bit, a recorded hash checked on every platform by CI).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_app::{App, AppConfig, System};
use wish_core::Entity;
use wish_core::math::{Quat, Vec3};
use wish_physics::{
    BodyImpulse, BodyKind, BodyVelocity, CharacterController, CharacterMotion, CharacterState,
    Collider, ColliderShape, OverlapShape, PhysicsProfile, PhysicsSettings, RigidBody, query_state,
};
use wish_transform::Transform;

fn app(profile: PhysicsProfile) -> App {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    wish_physics::register(app.world_mut()).unwrap();
    app.world_mut()
        .insert_resource(PhysicsSettings {
            profile,
            ..PhysicsSettings::default()
        })
        .unwrap();
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

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }
}

fn at(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Transform>(entity).unwrap().translation
}

/// A fixed box: a wall, a floor.
fn block(app: &mut App, centre: Vec3, size: Vec3) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .insert(e, Transform::from_translation(centre))
        .unwrap();
    world
        .insert(
            e,
            Collider {
                size,
                ..Collider::default()
            },
        )
        .unwrap();
    e
}

fn dynamic(app: &mut App, centre: Vec3, shape: ColliderShape, size: f32) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .insert(e, Transform::from_translation(centre))
        .unwrap();
    world
        .insert(
            e,
            Collider {
                shape,
                size: Vec3::splat(size),
                ..Collider::default()
            },
        )
        .unwrap();
    world
        .insert(
            e,
            RigidBody {
                kind: BodyKind::Dynamic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    e
}

/// A character: a kinematic body with a controller, asking for `velocity`.
fn character(app: &mut App, centre: Vec3, shape: ColliderShape, velocity: Vec3) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .insert(e, Transform::from_translation(centre))
        .unwrap();
    world
        .insert(
            e,
            Collider {
                shape,
                // A capsule 1.2 m long, or a circle 0.6 m across.
                size: if shape == ColliderShape::Capsule {
                    Vec3::new(0.6, 1.2, 0.6)
                } else {
                    Vec3::splat(0.6)
                },
                ..Collider::default()
            },
        )
        .unwrap();
    world
        .insert(
            e,
            RigidBody {
                kind: BodyKind::Kinematic,
                ..RigidBody::default()
            },
        )
        .unwrap();
    world.insert(e, CharacterController::default()).unwrap();
    world.insert(e, CharacterMotion { velocity }).unwrap();
    e
}

fn walk(app: &mut App, who: Entity, velocity: Vec3) {
    app.world_mut()
        .insert(who, CharacterMotion { velocity })
        .unwrap();
}

/// A closed room 10 x 8 m around the origin, walls 1 m thick, placed at
/// depth `z` (2D ignores it).
fn room(app: &mut App, z: f32) {
    block(app, Vec3::new(0.0, 4.5, z), Vec3::new(12.0, 1.0, 1.0));
    block(app, Vec3::new(0.0, -4.5, z), Vec3::new(12.0, 1.0, 1.0));
    block(app, Vec3::new(5.5, 0.0, z), Vec3::new(1.0, 8.0, 1.0));
    block(app, Vec3::new(-5.5, 0.0, z), Vec3::new(1.0, 8.0, 1.0));
}

/// Friction F19: a session prepares physics before its first tick; a
/// character that game code places during that tick (a player's spawn
/// point) goes there, as it would on any later tick.
#[test]
fn a_character_placed_on_the_first_tick_goes_there() {
    let mut app = app(PhysicsProfile::TopDown2D);
    let who = character(
        &mut app,
        Vec3::new(-50.0, -50.0, 0.0),
        ColliderShape::Sphere,
        Vec3::ZERO,
    );
    wish_physics::prepare(app.world_mut()).unwrap();
    app.add_system(
        System::new("spawn", |world, context| {
            if context.time.tick == 0 {
                let heroes: Vec<Entity> = world
                    .entities()
                    .filter(|e| world.has::<CharacterController>(*e))
                    .collect();
                for hero in heroes {
                    world.insert(hero, Transform::from_translation(Vec3::new(3.0, 2.0, 0.0)))?;
                }
            }
            Ok(())
        })
        .before(wish_physics::SYSTEM),
    )
    .unwrap();
    run(&mut app, 3);
    assert_eq!(at(&app, who), Vec3::new(3.0, 2.0, 0.0));
    walk(&mut app, who, Vec3::new(0.0, 6.0, 0.0));
    run(&mut app, 10);
    assert!(at(&app, who).y > 2.9, "{:?}", at(&app, who));
}

/// Friction F20: characters on a layer whose mask leaves that layer out
/// walk through each other (players in a crowded room), and walls still
/// stop them.
#[test]
fn characters_meet_only_what_their_mask_lets_them() {
    let mut app = app(PhysicsProfile::TopDown2D);
    room(&mut app, 0.0);
    let walker = character(
        &mut app,
        Vec3::new(-2.0, 0.0, 0.0),
        ColliderShape::Sphere,
        Vec3::new(3.0, 0.0, 0.0),
    );
    let standing = character(
        &mut app,
        Vec3::new(0.0, 0.0, 0.0),
        ColliderShape::Sphere,
        Vec3::ZERO,
    );
    for who in [walker, standing] {
        let mut collider = *app.world().get::<Collider>(who).unwrap();
        collider.layer = 2;
        collider.mask = 1;
        app.world_mut().insert(who, collider).unwrap();
    }
    run(&mut app, 200);
    let x = at(&app, walker).x;
    assert!(
        x > 4.6 && x < 4.71,
        "through the other player, to the wall: {x}"
    );
    assert_eq!(at(&app, standing), Vec3::ZERO);
}

#[test]
fn walls_stop_a_top_down_character_and_it_slides_along_them() {
    let mut app = app(PhysicsProfile::TopDown2D);
    // The walls are 3 m behind the character: in 2D depth does not matter.
    room(&mut app, -3.0);
    let hero = character(
        &mut app,
        Vec3::new(0.0, 0.0, 0.25),
        ColliderShape::Sphere,
        Vec3::new(3.0, 0.0, 0.0),
    );
    run(&mut app, 120);
    let p = at(&app, hero);
    // The east wall's inner face is at x = 5, the character's radius 0.3.
    assert!(p.x > 4.6 && p.x < 4.71, "stopped by the wall at {p:?}");
    assert_eq!(p.z, 0.25, "its own depth is kept");
    let state = *app.world().get::<CharacterState>(hero).unwrap();
    assert!(state.blocked && !state.grounded, "{state:?}");
    // No gravity seen from above: it has not moved along y at all.
    assert!(p.y.abs() < 1e-4, "{p:?}");

    // Pushing into the wall and up the map: it slides along the wall.
    walk(&mut app, hero, Vec3::new(3.0, 3.0, 0.0));
    run(&mut app, 60);
    let q = at(&app, hero);
    assert!(q.y > 2.5, "slid along the wall: {q:?}");
    assert!(q.x < 4.71, "not through the wall: {q:?}");
    // ...into the corner, where it stays.
    run(&mut app, 120);
    let r = at(&app, hero);
    assert!(r.x < 4.71 && r.y < 3.71, "in the corner: {r:?}");
    assert!(r.x > 4.6 && r.y > 3.6, "in the corner: {r:?}");

    // Asking for a move along z does nothing.
    walk(&mut app, hero, Vec3::new(0.0, 0.0, -5.0));
    run(&mut app, 30);
    assert_eq!(at(&app, hero), r);
}

#[test]
fn a_top_down_character_with_a_capsule_turned_flat_walks_through_a_door() {
    // A capsule turned a quarter about Z lies along X in the plane; a 1.2 m
    // door lets a 0.6 m wide character through, a wall does not.
    let mut app = app(PhysicsProfile::TopDown2D);
    block(
        &mut app,
        Vec3::new(-3.6, 0.0, 0.0),
        Vec3::new(6.0, 1.0, 1.0),
    );
    block(&mut app, Vec3::new(3.6, 0.0, 0.0), Vec3::new(6.0, 1.0, 1.0));
    let hero = character(
        &mut app,
        Vec3::new(0.0, -3.0, 0.0),
        ColliderShape::Capsule,
        Vec3::new(0.0, 2.0, 0.0),
    );
    app.world_mut()
        .insert(
            hero,
            Transform {
                translation: Vec3::new(0.0, -3.0, 0.0),
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                scale: Vec3::ONE,
            },
        )
        .unwrap();
    // Lying down it is 1.2 m long: across the 1.2 m door it does not fit.
    run(&mut app, 120);
    assert!(at(&app, hero).y < -0.4, "{:?}", at(&app, hero));
    // Standing up (no turn) it is 0.6 m wide: it passes.
    app.world_mut()
        .insert(hero, Transform::from_translation(Vec3::new(0.0, -3.0, 0.0)))
        .unwrap();
    run(&mut app, 180);
    assert!(at(&app, hero).y > 2.0, "{:?}", at(&app, hero));
}

/// The jumping rule of the platformer recipe, for every character.
fn jumper() -> System {
    System::new("jumper", |world, _| {
        let now = world
            .resource::<wish_types::time::Time>()
            .map_or(0, |t| t.tick);
        let dt = 1.0 / 60.0;
        let heroes: Vec<Entity> = world
            .entities()
            .filter(|e| world.has::<CharacterController>(*e))
            .collect();
        for hero in heroes {
            let state = world
                .get::<CharacterState>(hero)
                .copied()
                .unwrap_or_default();
            let mut v = world.get::<CharacterMotion>(hero).unwrap().velocity;
            v.y = if state.grounded {
                if now == 60 { 6.0 } else { -1.0 }
            } else {
                v.y - 9.81 * dt
            };
            world.insert(hero, CharacterMotion { velocity: v })?;
        }
        Ok(())
    })
    .before(wish_physics::SYSTEM)
}

#[test]
fn a_side_view_character_jumps_and_lands() {
    let mut app = app(PhysicsProfile::SideView2D);
    app.add_system(jumper()).unwrap();
    block(
        &mut app,
        Vec3::new(0.0, -0.5, 2.0),
        Vec3::new(40.0, 1.0, 1.0),
    );
    let hero = character(
        &mut app,
        Vec3::new(0.0, 0.7, -1.0),
        ColliderShape::Capsule,
        Vec3::new(1.0, -1.0, 0.0),
    );
    run(&mut app, 55);
    assert!(app.world().get::<CharacterState>(hero).unwrap().grounded);
    let standing = at(&app, hero).y;
    let mut highest = standing;
    let mut left_the_ground = false;
    for _ in 0..120 {
        run(&mut app, 1);
        highest = highest.max(at(&app, hero).y);
        left_the_ground |= !app.world().get::<CharacterState>(hero).unwrap().grounded;
    }
    assert!(left_the_ground);
    // 6 m/s up under 9.81 m/s2 rises about 1.8 m.
    assert!(
        highest - standing > 1.5 && highest - standing < 2.0,
        "jumped {}",
        highest - standing
    );
    let p = at(&app, hero);
    assert!(app.world().get::<CharacterState>(hero).unwrap().grounded);
    assert!((p.y - standing).abs() < 0.05, "landed at {p:?}");
    assert!(p.x > 2.5, "walked on: {p:?}");
    assert_eq!(p.z, -1.0, "its own depth is kept");
}

#[test]
fn bodies_stay_in_the_plane_and_collide_at_any_depth() {
    let mut app = app(PhysicsProfile::SideView2D);
    let floor = block(
        &mut app,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(40.0, 1.0, 1.0),
    );
    // A box 3 m in front of the floor (a sprite on another layer) falls
    // onto it, spinning and thrown along Z and about X: only what is in the
    // plane takes.
    let crate_ = dynamic(&mut app, Vec3::new(0.0, 3.0, 3.0), ColliderShape::Box, 1.0);
    app.world_mut()
        .insert(
            crate_,
            BodyVelocity {
                linear: Vec3::new(1.0, 0.0, 5.0),
                angular: Vec3::new(4.0, 4.0, 2.0),
            },
        )
        .unwrap();
    run(&mut app, 240);
    let t = *app.world().get::<Transform>(crate_).unwrap();
    assert_eq!(t.translation.z, 3.0, "{t:?}");
    assert!(
        (t.translation.y - 0.5).abs() < 0.05,
        "rests on the floor: {t:?}"
    );
    assert!(t.rotation.x == 0.0 && t.rotation.y == 0.0, "{t:?}");
    let v = *app.world().get::<BodyVelocity>(crate_).unwrap();
    assert_eq!(v.linear.z, 0.0);
    assert_eq!((v.angular.x, v.angular.y), (0.0, 0.0));
    let state = query_state(app.world()).unwrap();
    assert_eq!(state.touching(crate_), vec![floor]);
}

#[test]
fn a_tilted_sprite_keeps_its_tilt() {
    // A body tilted about X (a sprite leaning towards the camera) falls in
    // the plane and keeps its tilt; physics turns only its twist about Z.
    let mut app = app(PhysicsProfile::SideView2D);
    block(
        &mut app,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(40.0, 1.0, 1.0),
    );
    let tilted = dynamic(
        &mut app,
        Vec3::new(0.0, 2.0, 0.0),
        ColliderShape::Sphere,
        1.0,
    );
    let tilt = Quat::from_rotation_x(0.4);
    app.world_mut()
        .insert(
            tilted,
            Transform {
                translation: Vec3::new(0.0, 2.0, 0.0),
                rotation: tilt,
                scale: Vec3::ONE,
            },
        )
        .unwrap();
    run(&mut app, 120);
    let t = *app.world().get::<Transform>(tilted).unwrap();
    assert!((t.translation.y - 0.5).abs() < 0.05, "{t:?}");
    assert!(t.rotation.angle_between(tilt) < 1e-3, "{t:?}");
}

#[test]
fn seen_from_above_nothing_falls() {
    let mut app = app(PhysicsProfile::TopDown2D);
    let crate_ = dynamic(&mut app, Vec3::new(1.0, 2.0, 0.0), ColliderShape::Box, 1.0);
    run(&mut app, 60);
    assert_eq!(at(&app, crate_), Vec3::new(1.0, 2.0, 0.0));
}

#[test]
fn a_2d_body_weighs_its_area() {
    // A disc 1 m across at 1000 kg/m3 weighs 1000 * pi * 0.25 * 1 m:
    // that impulse makes it go 1 m/s (as a 3D ball it would weigh 524 kg).
    let mut app = app(PhysicsProfile::TopDown2D);
    let disc = dynamic(&mut app, Vec3::ZERO, ColliderShape::Sphere, 1.0);
    run(&mut app, 1);
    app.world_mut()
        .send(BodyImpulse {
            entity: disc,
            linear: Vec3::new(1000.0 * std::f32::consts::PI * 0.25, 0.0, 0.0),
            angular: Vec3::ZERO,
        })
        .unwrap();
    run(&mut app, 1);
    let v = app.world().get::<BodyVelocity>(disc).unwrap().linear;
    assert!((v.x - 1.0).abs() < 1e-3, "{v:?}");
}

#[test]
fn questions_are_asked_in_the_plane() {
    let mut app = app(PhysicsProfile::TopDown2D);
    let wall = block(
        &mut app,
        Vec3::new(4.0, 0.0, -2.0),
        Vec3::new(1.0, 4.0, 1.0),
    );
    let crate_ = dynamic(
        &mut app,
        Vec3::new(0.0, 3.0, 5.0),
        ColliderShape::Sphere,
        1.0,
    );
    for stepped in [false, true] {
        if stepped {
            run(&mut app, 2);
        }
        let state = query_state(app.world()).unwrap();
        assert_eq!(state.profile(), PhysicsProfile::TopDown2D);
        // A ray at another depth, along X: it meets the wall in the plane.
        let hit = state
            .raycast(
                Vec3::new(0.0, 0.0, 7.0),
                Vec3::new(1.0, 0.0, 0.3),
                10.0,
                u32::MAX,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.entity, wall);
        assert!((hit.distance - 3.5).abs() < 1e-4, "{hit:?}");
        assert_eq!(hit.point.z, 0.0);
        assert!((hit.normal - Vec3::NEG_X).length() < 1e-4, "{hit:?}");
        // A ray along Z has no direction in the plane: explained.
        let bad = state
            .raycast(Vec3::ZERO, Vec3::NEG_Z, 10.0, u32::MAX, None)
            .unwrap_err();
        assert_eq!(bad.code, "invalid_parameter");
        assert!(bad.hint.contains("x or y"), "{}", bad.hint);
        // A circle at the crate's x and y, any depth; a flat box (no depth)
        // over the wall.
        let found = state
            .overlap(
                OverlapShape::Sphere(0.2),
                Vec3::new(0.0, 3.0, -9.0),
                Quat::IDENTITY,
                u32::MAX,
            )
            .unwrap();
        assert_eq!(found, vec![crate_]);
        let found = state
            .overlap(
                OverlapShape::Box(Vec3::new(0.2, 0.2, 0.0)),
                Vec3::new(4.0, 1.5, 0.0),
                Quat::IDENTITY,
                u32::MAX,
            )
            .unwrap();
        assert_eq!(found, vec![wall]);
    }
}

#[test]
fn changing_the_profile_builds_physics_again() {
    let mut app = app(PhysicsProfile::Full3D);
    let crate_ = dynamic(&mut app, Vec3::new(0.0, 5.0, 2.0), ColliderShape::Box, 1.0);
    run(&mut app, 10);
    let falling = at(&app, crate_);
    assert!(falling.y < 5.0);
    app.world_mut()
        .resource_mut::<PhysicsSettings>()
        .unwrap()
        .profile = PhysicsProfile::TopDown2D;
    app.world_mut()
        .insert(crate_, BodyVelocity::default())
        .unwrap();
    run(&mut app, 30);
    let now = at(&app, crate_);
    assert!((now - falling).length() < 1e-4, "{falling:?} -> {now:?}");
    let state = query_state(app.world()).unwrap();
    assert_eq!(state.profile(), PhysicsProfile::TopDown2D);
}

/// The top-down measuring map of the design note, scaled by `crates`: tile
/// walls with rooms and doors, ten walking characters, dynamic crates and
/// balls. Characters change direction every 1.5 s from a fixed sequence.
fn station(app: &mut App, crates: usize, moving: bool) -> usize {
    let (w, h) = (48i32, 32i32);
    let mut tiles = Vec::new();
    for x in 0..w {
        tiles.push((x, 0));
        tiles.push((x, h - 1));
    }
    for y in 1..h - 1 {
        tiles.push((0, y));
        tiles.push((w - 1, y));
    }
    for x in [12, 24, 36] {
        for y in 1..h - 1 {
            if ![5, 6, 16, 17, 26, 27].contains(&y) {
                tiles.push((x, y));
            }
        }
    }
    for y in [11, 21] {
        for x in 1..w - 1 {
            if x % 12 != 0 && x % 12 != 6 && x % 12 != 7 {
                tiles.push((x, y));
            }
        }
    }
    tiles.sort_unstable();
    tiles.dedup();
    for (x, y) in &tiles {
        block(
            app,
            Vec3::new(*x as f32 + 0.5, *y as f32 + 0.5, 0.0),
            Vec3::ONE,
        );
    }
    for i in 0..crates {
        let room = (i % 4, (i / 4) % 3);
        let x = room.0 as f32 * 12.0 + 2.5 + ((i * 7) % 8) as f32 + (i / 12 % 2) as f32 * 0.5;
        let y = room.1 as f32 * 10.5 + 2.5 + ((i * 3) % 6) as f32 + (i / 24 % 2) as f32 * 0.5;
        let shape = if i % 2 == 0 {
            ColliderShape::Sphere
        } else {
            ColliderShape::Box
        };
        let e = dynamic(app, Vec3::new(x, y, 0.0), shape, 0.7);
        let mut body = *app.world().get::<RigidBody>(e).unwrap();
        body.linear_damping = if moving { 0.0 } else { 2.0 };
        body.angular_damping = body.linear_damping;
        app.world_mut().insert(e, body).unwrap();
        if moving {
            let angle = (i as f32 * 2.399) % std::f32::consts::TAU;
            app.world_mut()
                .insert(
                    e,
                    BodyVelocity {
                        linear: Vec3::new(angle.cos(), angle.sin(), 0.0) * 6.0,
                        angular: Vec3::ZERO,
                    },
                )
                .unwrap();
            let mut collider = *app.world().get::<Collider>(e).unwrap();
            collider.restitution = 0.8;
            app.world_mut().insert(e, collider).unwrap();
        }
    }
    for i in 0..10 {
        let x = (i % 4) as f32 * 12.0 + 6.0;
        let y = ((i / 4) % 3) as f32 * 10.5 + 5.5;
        character(app, Vec3::new(x, y, 0.0), ColliderShape::Sphere, Vec3::ZERO);
    }
    app.add_system(
        System::new("walkers", |world, _| {
            let tick = world
                .resource::<wish_types::time::Time>()
                .map_or(0, |t| t.tick);
            let heroes: Vec<Entity> = world
                .entities()
                .filter(|e| world.has::<CharacterController>(*e))
                .collect();
            for (i, hero) in heroes.into_iter().enumerate() {
                let turn = (i as u64 + 1) * 7919 + tick / 90 * 104_729;
                let angle = (turn % 3600) as f32 / 3600.0 * std::f32::consts::TAU;
                world.insert(
                    hero,
                    CharacterMotion {
                        velocity: Vec3::new(angle.cos(), angle.sin(), 0.0) * 3.5,
                    },
                )?;
            }
            Ok(())
        })
        .before(wish_physics::SYSTEM),
    )
    .unwrap();
    tiles.len() + crates + 10
}

#[test]
fn a_2d_world_continues_bit_for_bit_after_a_snapshot() {
    let mut app = app(PhysicsProfile::TopDown2D);
    station(&mut app, 50, true);
    run(&mut app, 100);
    let snapshot = app.snapshot();
    run(&mut app, 200);
    let straight = app.hash();
    app.restore(&snapshot).unwrap();
    run(&mut app, 200);
    assert_eq!(app.hash(), straight, "restore then continue");

    let mut other = self::app(PhysicsProfile::TopDown2D);
    station(&mut other, 50, true);
    run(&mut other, 300);
    assert_eq!(other.hash(), straight, "same map, same hash");
}

/// The map after 600 ticks, hashed: the same on every platform (CI runs it
/// on Windows and Linux) and in debug and release. If it changes, ask first
/// whether a deliberate change to physics or the world hash explains it.
#[test]
fn the_recorded_2d_hash_holds() {
    let mut app = app(PhysicsProfile::TopDown2D);
    station(&mut app, 50, true);
    run(&mut app, 600);
    let hash = app.world().hash().to_string();
    println!("station after 600 ticks: {hash}");
    assert_eq!(hash, RECORDED_STATION);
}

const RECORDED_STATION: &str = "b3fa11a53f26b7c011c908f573737727";

/// `cargo test --release -p wish_physics --test plane -- --ignored --nocapture`
#[test]
#[ignore = "measurement"]
fn what_a_2d_tick_costs() {
    for (crates, moving) in [(50, false), (300, true)] {
        let mut app = app(PhysicsProfile::TopDown2D);
        let bodies = station(&mut app, crates, moving);
        run(&mut app, 60);
        let mut best = f64::MAX;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            run(&mut app, 600);
            best = best.min(start.elapsed().as_secs_f64() * 1000.0 / 600.0);
        }
        println!(
            "{bodies} bodies ({crates} dynamic, {}): {best:.3} ms per tick through the engine",
            if moving { "bouncing" } else { "at rest" }
        );
    }
}
