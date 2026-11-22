//! Line of sight in a room with a door (design note 1.1-lights-2d §4 and
//! §11): the visibility polygon's golden numbers, the same twice; seeing
//! through the door and not through the wall; touching is blocked; an
//! entity's own outline ignored; walls gathered from a tilemap and from
//! occluders in order; the system's extension carried by a snapshot.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_app::{App, AppConfig, System};
use wish_core::math::{Quat, Vec3};
use wish_core::{AssetRef, Entity, StableHasher};
use wish_sight::{MapShapes, Occluder, Occluders, Segment, current, inside, install_tile_shapes};
use wish_transform::Transform;
use wish_types::physics::{Collider, ColliderShape};
use wish_types::tilemap::Tilemap;

const MAP: &str = "maps/room.tilemap.json";

/// A room of 10 x 8 cells of 1 m, its walls one cell thick, a door of two
/// cells in its east wall (y 3 to 5), as the box merging of
/// `wish_asset::tiles` gives them.
fn room() -> MapShapes {
    MapShapes {
        boxes: vec![
            [0, 0, 10, 1],
            [0, 1, 1, 7],
            [9, 1, 10, 3],
            [9, 5, 10, 7],
            [0, 7, 10, 8],
        ],
        outlines: Vec::new(),
    }
}

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    let world = app.world_mut();
    world.register_component::<Tilemap>().unwrap();
    world.register_component::<Collider>().unwrap();
    wish_sight::register(world).unwrap();
    app.add_system(System::new("transform_propagation", |world, _| {
        wish_transform::update_global_transforms(world)
    }))
    .unwrap();
    app.add_system(wish_sight::system().after("transform_propagation"))
        .unwrap();
    install_tile_shapes(app.world_mut(), [(MAP.to_owned(), room())]);
    app
}

fn tilemap(app: &mut App) -> Entity {
    let world = app.world_mut();
    let e = world.spawn().unwrap();
    world.insert(e, Transform::default()).unwrap();
    let map = world.intern(MAP).unwrap();
    world
        .insert(
            e,
            Tilemap {
                map: AssetRef::from_text(map),
                ..Tilemap::default()
            },
        )
        .unwrap();
    e
}

/// The room's walls with nothing else, from the system after one tick.
fn walls() -> (App, Entity) {
    let mut app = app();
    let map = tilemap(&mut app);
    app.step().unwrap();
    (app, map)
}

fn digest(points: &[[f32; 2]]) -> String {
    let mut hasher = StableHasher::new();
    hasher.write_u64(points.len() as u64);
    for [x, y] in points {
        hasher.write_u32(x.to_bits());
        hasher.write_u32(y.to_bits());
    }
    format!("{:016x}", (hasher.finish().as_u128() >> 64) as u64)
}

#[test]
fn the_visibility_polygon_of_a_room_with_a_door_has_golden_numbers() {
    let (app, _) = walls();
    let walls = current(app.world());
    assert_eq!(walls.segments().len(), 20, "five boxes of four edges");
    let polygon = walls.visible_polygon([5.0, 4.0], 6.0);
    let again = walls.visible_polygon([5.0, 4.0], 6.0);
    assert_eq!(polygon, again, "the same twice");
    // A second world built the same way gives the same bits.
    let (other, _) = self::walls();
    assert_eq!(
        current(other.world()).visible_polygon([5.0, 4.0], 6.0),
        polygon
    );
    println!(
        "points {} tested {} digest {}",
        polygon.points.len(),
        polygon.tested,
        digest(&polygon.points)
    );
    // Golden numbers: recorded once, the same in debug and release, on
    // every machine (only IEEE-exact operations, design note §4).
    assert_eq!(
        (
            polygon.points.len(),
            polygon.tested,
            digest(&polygon.points).as_str()
        ),
        (GOLDEN_POINTS, GOLDEN_TESTED, GOLDEN_DIGEST)
    );
    // Inside the room, everything is seen; through the door, the corridor
    // in line with it; behind the east wall, nothing.
    assert!(inside(&polygon.points, [2.0, 2.0]));
    assert!(inside(&polygon.points, [8.5, 6.5]));
    assert!(inside(&polygon.points, [10.5, 4.0]));
    assert!(!inside(&polygon.points, [10.5, 1.5]));
    assert!(!inside(&polygon.points, [10.5, 6.5]));
    // Nothing beyond the walls' outer faces except through the door.
    for [x, y] in &polygon.points {
        let outside = *x > 10.0 || *x < 0.0 || *y > 8.0 || *y < 0.0;
        if outside {
            assert!(
                *x > 10.0 && *y > 2.0 && *y < 6.0,
                "({x}, {y}) is seen past a wall"
            );
        }
    }
}

const GOLDEN_POINTS: usize = 172;
const GOLDEN_TESTED: u64 = 343;
const GOLDEN_DIGEST: &str = "effcbcfa21775dc2";

#[test]
fn a_room_is_seen_through_its_door_and_not_through_its_wall() {
    let (app, map) = walls();
    let walls = current(app.world());
    let see = |a: [f32; 2], b: [f32; 2]| walls.can_see(a, b, [None, None]);
    assert!(see([5.0, 4.0], [12.0, 4.0]).visible, "through the door");
    assert!(see([12.0, 4.5], [3.0, 3.5]).visible, "and back");
    let blocked = see([5.0, 4.0], [12.0, 1.5]);
    assert!(!blocked.visible, "through the wall south of the door");
    assert_eq!(blocked.blocked_by, Some(map));
    assert!(
        !see([5.0, 4.0], [5.0, -3.0]).visible,
        "through the south wall"
    );
    // Along the door's edge: touching the wall counts as blocked.
    assert!(!see([5.0, 3.0], [12.0, 3.0]).visible);
    // Just inside it, free.
    assert!(see([5.0, 3.001], [12.0, 3.001]).visible);
    // A wall's owner can be ignored.
    assert!(
        walls
            .can_see([5.0, 4.0], [12.0, 1.5], [Some(map), None])
            .visible
    );
    // Not a number: not seen.
    assert!(!see([f32::NAN, 0.0], [1.0, 1.0]).visible);
}

#[test]
fn an_occluder_blocks_while_enabled_and_never_hides_itself() {
    let (mut app, map) = walls();
    // A door: a 0.2 x 2 m box in the opening.
    let door = {
        let world = app.world_mut();
        let e = world.spawn().unwrap();
        world
            .insert(e, Transform::from_translation(Vec3::new(9.5, 4.0, 0.0)))
            .unwrap();
        world
            .insert(
                e,
                Collider {
                    shape: ColliderShape::Box,
                    size: Vec3::new(0.2, 2.0, 1.0),
                    ..Collider::default()
                },
            )
            .unwrap();
        world.insert(e, Occluder { enabled: true }).unwrap();
        e
    };
    app.step().unwrap();
    let closed = current(app.world());
    assert_eq!(closed.segments().len(), 24, "the room, then the door");
    assert!(closed.segments()[..20].iter().all(|s| s.owner == map));
    assert!(closed.segments()[20..].iter().all(|s| s.owner == door));
    let line = closed.can_see([5.0, 4.0], [12.0, 4.0], [None, None]);
    assert_eq!((line.visible, line.blocked_by), (false, Some(door)));
    // The door seen from inside, its own outline ignored.
    assert!(
        closed
            .can_see([5.0, 4.0], [9.5, 4.0], [Some(door), None])
            .visible
    );
    let polygon = closed.visible_polygon([5.0, 4.0], 6.0);
    assert!(
        !inside(&polygon.points, [10.5, 4.0]),
        "the closed door hides the corridor"
    );
    // Opened.
    app.world_mut()
        .insert(door, Occluder { enabled: false })
        .unwrap();
    app.step().unwrap();
    assert!(
        current(app.world())
            .can_see([5.0, 4.0], [12.0, 4.0], [None, None])
            .visible
    );
    // A round occluder is a 32-sided polygon; a capsule is closed too.
    for shape in [ColliderShape::Sphere, ColliderShape::Capsule] {
        let outline = wish_sight::collider_outline(&Collider {
            shape,
            size: Vec3::new(1.0, 3.0, 1.0),
            ..Collider::default()
        });
        assert!(outline.len() >= 32, "{shape:?}: {}", outline.len());
        assert!(inside(&outline, [0.0, 0.0]));
        assert!(!inside(&outline, [0.0, 1.6]) || shape == ColliderShape::Sphere);
    }
}

#[test]
fn walls_follow_their_tilemap_and_a_snapshot_keeps_them() {
    let (mut app, map) = walls();
    let before = current(app.world()).segments().to_vec();
    let snapshot = app.world().snapshot();
    // The map turned a quarter about Z and moved: its walls with it.
    app.world_mut()
        .insert(
            map,
            Transform {
                translation: Vec3::new(100.0, 0.0, 0.0),
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                scale: Vec3::ONE,
            },
        )
        .unwrap();
    app.step().unwrap();
    let moved = current(app.world());
    assert!(moved.segments()[0].a[0] > 90.0);
    assert!(moved.can_see([5.0, 4.0], [5.0, -3.0], [None, None]).visible);
    app.world_mut().restore(&snapshot).unwrap();
    assert_eq!(current(app.world()).segments(), &before[..]);
    // Without walls the extension goes away.
    app.world_mut().despawn(map).unwrap();
    app.step().unwrap();
    assert!(app.world().extension::<Occluders>().is_none());
    assert!(current(app.world()).is_empty());
}

#[test]
fn questions_have_limits() {
    let owner = Entity::default();
    // A wall with an end that is not a number is left out.
    let walls = Occluders::new([
        Segment {
            a: [0.0, 0.0],
            b: [f64::NAN, 1.0],
            owner,
        },
        Segment {
            a: [2.0, -1.0],
            b: [2.0, 1.0],
            owner,
        },
    ]);
    assert_eq!(walls.segments().len(), 1);
    // A radius of 0, or not a number: nothing.
    assert!(walls.visible_polygon([0.0, 0.0], 0.0).points.is_empty());
    assert!(
        walls
            .visible_polygon([0.0, 0.0], f32::NAN)
            .points
            .is_empty()
    );
    // The radius is at most 1000 m.
    let far = walls.visible_polygon([0.0, 0.0], 1e9);
    assert!(far.points.iter().all(|[x, y]| x.hypot(*y) <= 1000.001));
    // Over the segment budget the rest is left out and counted.
    let many = Occluders::new((0..wish_sight::MAX_SEGMENTS + 5).map(|i| Segment {
        a: [i as f64, 0.0],
        b: [i as f64, 1.0],
        owner,
    }));
    assert_eq!(many.left_out(), 5);
    // A polygon over many walls stays within its budget.
    let crowded = many.visible_polygon([500.5, 0.5], 600.0);
    assert!(crowded.points.len() <= 8 * wish_sight::MAX_AREA_SEGMENTS + 128);
    // Wall segments far apart go through the grid; a very long one too.
    let long = Occluders::new([Segment {
        a: [-1e6, 5.0],
        b: [1e6, 5.0],
        owner,
    }]);
    assert!(!long.can_see([0.0, 0.0], [0.0, 10.0], [None, None]).visible);
    assert!(long.can_see([0.0, 0.0], [10.0, 1.0], [None, None]).visible);
}
