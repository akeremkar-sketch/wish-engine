//! Walls from tilemaps (design note 1.1-tilemaps §7): a tilemap's solid
//! tiles are one fixed body with one compound collider; a top-down
//! character walks into them and stops, slides along them, and contacts
//! name the tilemap's entity once; the result is deterministic and
//! continues bit for bit after a snapshot.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_app::{App, AppConfig, System};
use wish_core::math::{Quat, Vec3};
use wish_core::{AssetRef, Entity};
use wish_physics::{
    BodyKind, CharacterController, CharacterMotion, Collider, ColliderShape, CollisionStarted,
    MapShapes, OverlapShape, PhysicsProfile, PhysicsSettings, RigidBody, install_tile_shapes,
    query_state,
};
use wish_transform::Transform;
use wish_types::tilemap::Tilemap;

const MAP: &str = "maps/room.tilemap.json";

/// A room of 12 x 8 cells of 0.5 m, its walls one cell thick, a door of two
/// cells in its south wall (cells 5 and 6), as the box merging of
/// `wish_asset::tiles` gives them, and a slope in the north-east corner.
fn room() -> MapShapes {
    MapShapes {
        boxes: vec![
            [0, 0, 5, 1],
            [7, 0, 12, 1],
            [0, 1, 1, 8],
            [11, 1, 12, 8],
            [1, 7, 11, 8],
        ],
        outlines: vec![([10, 6], vec![[0.0, 1.0], [1.0, 0.0], [1.0, 1.0]])],
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    wish_physics::register(app.world_mut()).unwrap();
    app.world_mut().register_component::<Tilemap>().unwrap();
    app.world_mut()
        .insert_resource(PhysicsSettings {
            profile: PhysicsProfile::TopDown2D,
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
    install_tile_shapes(app.world_mut(), [(MAP.to_owned(), room())]);
    app
}

/// The tilemap entity: the room's bottom left corner at (-3, -2), drawn on
/// another depth than the characters.
fn tilemap(app: &mut App) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .insert(e, Transform::from_translation(Vec3::new(-3.0, -2.0, -1.0)))
        .unwrap();
    let map = world.intern(MAP).unwrap();
    world
        .insert(
            e,
            Tilemap {
                map: AssetRef::from_text(map),
                cell_size: [0.5, 0.5],
                ..Tilemap::default()
            },
        )
        .unwrap();
    e
}

fn character(app: &mut App, centre: Vec3, velocity: Vec3) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world
        .insert(e, Transform::from_translation(centre))
        .unwrap();
    world
        .insert(
            e,
            Collider {
                shape: ColliderShape::Sphere,
                size: Vec3::splat(0.4),
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

fn run(app: &mut App, ticks: usize) {
    contacts(app, ticks, None);
}

/// Runs `ticks` ticks; how many contacts started and ended (the tests that
/// count have only the map and one character, so every contact is theirs).
fn contacts(app: &mut App, ticks: usize, with: Option<Entity>) -> (usize, usize) {
    let sent = |app: &App, name: &str| app.world().events_sent(name).unwrap() as usize;
    let (started, ended) = (sent(app, "CollisionStarted"), sent(app, "CollisionEnded"));
    for _ in 0..ticks {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
    }
    if let Some(with) = with {
        for event in app.world().events::<CollisionStarted>().unwrap() {
            assert!(event.a == with || event.b == with, "{event:?}");
        }
    }
    (
        sent(app, "CollisionStarted") - started,
        sent(app, "CollisionEnded") - ended,
    )
}

fn at(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Transform>(entity).unwrap().translation
}

#[test]
fn tile_walls_stop_a_top_down_character_and_it_slides_along_them() {
    let mut app = app();
    let map = tilemap(&mut app);
    // Inside the room (x -2.5 to 2.5, y -1.5 to 1.5), walking east.
    let walker = character(&mut app, Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
    let (started, _) = contacts(&mut app, 120, Some(map));
    let stopped = at(&app, walker);
    // The east wall's inner face is at x = -3 + 11 * 0.5 = 2.5; the
    // character is 0.2 m round.
    assert!(
        (2.2..2.31).contains(&stopped.x),
        "stopped at the east wall: {stopped}"
    );
    assert_eq!(stopped.z, 0.0, "its own depth kept");
    // Contact with the map is one pair (one collider), started once.
    assert_eq!(started, 1);
    // North-east: slides along the east wall into the slope.
    app.world_mut()
        .insert(
            walker,
            CharacterMotion {
                velocity: Vec3::new(2.0, 2.0, 0.0),
            },
        )
        .unwrap();
    run(&mut app, 120);
    let corner = at(&app, walker);
    assert!(corner.x < 2.31, "still inside the east wall: {corner}");
    // The slope (cell 10, 6: x 2 to 2.5, y 1 to 1.5, solid above the
    // diagonal) keeps it out of the corner's last half cell.
    assert!(corner.y < 1.31, "stopped below the north wall: {corner}");
    assert!(
        corner.x + corner.y < 3.5 + 0.2,
        "the slope's diagonal holds it back: {corner}"
    );
    // South, through the door (x -0.5 to 0.5) and out.
    app.world_mut()
        .insert(walker, Transform::from_translation(Vec3::ZERO))
        .unwrap();
    app.world_mut()
        .insert(
            walker,
            CharacterMotion {
                velocity: Vec3::new(0.0, -3.0, 0.0),
            },
        )
        .unwrap();
    let (_, ended) = contacts(&mut app, 90, Some(map));
    assert!(
        at(&app, walker).y < -2.5,
        "out through the door: {}",
        at(&app, walker)
    );
    assert_eq!(
        ended, 1,
        "the contact with the walls ended when it moved away"
    );
}

#[test]
fn queries_see_tile_walls_and_name_the_map() {
    let mut app = app();
    let map = tilemap(&mut app);
    let state = query_state(app.world()).unwrap();
    // A ray from the middle of the room west hits the west wall's inner
    // face at x = -3 + 0.5 = -2.5.
    let hit = state
        .raycast(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0), 100.0, u32::MAX, None)
        .unwrap()
        .expect("a hit");
    assert_eq!(hit.entity, map);
    assert!((hit.point.x + 2.5).abs() < 1e-4, "{hit:?}");
    // Nothing in the doorway.
    let door = state
        .overlap(
            OverlapShape::Box(Vec3::splat(0.2)),
            Vec3::new(0.0, -1.75, 0.0),
            Quat::IDENTITY,
            u32::MAX,
        )
        .unwrap();
    assert!(door.is_empty(), "{door:?}");
    let wall = state
        .overlap(
            OverlapShape::Box(Vec3::splat(0.2)),
            Vec3::new(-1.75, -1.75, 0.0),
            Quat::IDENTITY,
            u32::MAX,
        )
        .unwrap();
    assert_eq!(wall, [map]);
}

fn walkers(app: &mut App) {
    tilemap(app);
    for i in 0..6 {
        let angle = i as f32 * 1.1;
        character(
            app,
            Vec3::new(-1.5 + i as f32 * 0.6, 0.0, 0.0),
            Vec3::new(angle.cos() * 3.0, angle.sin() * 3.0, 0.0),
        );
    }
}

#[test]
fn a_tile_world_is_deterministic_and_continues_after_a_snapshot() {
    let mut app = app();
    walkers(&mut app);
    run(&mut app, 100);
    let snapshot = app.snapshot();
    run(&mut app, 200);
    let straight = app.hash();
    app.restore(&snapshot).unwrap();
    run(&mut app, 200);
    assert_eq!(app.hash(), straight, "restore then continue");
    let mut other = self::app();
    walkers(&mut other);
    run(&mut other, 300);
    assert_eq!(other.hash(), straight, "same map, same hash");
}

#[test]
fn a_map_without_known_shapes_or_moved_is_handled() {
    let mut app = app();
    let map = tilemap(&mut app);
    run(&mut app, 1);
    assert_eq!(query_state(app.world()).unwrap().len(), 1);
    // Moving the map moves its walls.
    app.world_mut()
        .insert(map, Transform::from_translation(Vec3::new(7.0, -2.0, 0.0)))
        .unwrap();
    run(&mut app, 1);
    let state = query_state(app.world()).unwrap();
    let hit = state
        .raycast(
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            100.0,
            u32::MAX,
            None,
        )
        .unwrap()
        .expect("a hit");
    // The west wall's inner face: 7 + 0.5.
    assert!((hit.point.x - 7.5).abs() < 1e-4, "{hit:?}");
    // A map whose file is not known has no walls.
    let unknown = app.world_mut().intern("maps/other.tilemap.json").unwrap();
    app.world_mut()
        .insert(
            map,
            Tilemap {
                map: AssetRef::from_text(unknown),
                ..Tilemap::default()
            },
        )
        .unwrap();
    run(&mut app, 1);
    assert_eq!(query_state(app.world()).unwrap().len(), 0);
}
