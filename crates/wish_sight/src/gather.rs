//! The walls of a world (design note §3): tilemaps' solid tiles, then
//! entities with an enabled `Occluder` and a `Collider`, each in entity
//! order, as segments in the world's X–Y plane.

use wish_core::Entity;
use wish_core::math::{Mat4, Vec3};
use wish_ecs::World;
use wish_types::lights2d::Occluder;
use wish_types::physics::{Collider, ColliderShape};
use wish_types::tilemap::Tilemap;
use wish_types::transform::{GlobalTransform, Transform};

use crate::geometry::{CIRCLE, CIRCLE_STEPS};
use crate::occluders::{Occluders, Segment, outline_segments};
use crate::tiles::TileShapes;

/// The matrix that places `entity` in the world: its `GlobalTransform`, else
/// its `Transform`, else none.
pub fn placement(world: &World, entity: Entity) -> Mat4 {
    if let Some(global) = world.get::<GlobalTransform>(entity) {
        return global.matrix;
    }
    world
        .get::<Transform>(entity)
        .map_or(Mat4::IDENTITY, Transform::to_matrix)
}

/// Every entity with a component of type `T`, in entity order.
fn with<T: wish_schema::Component>(world: &World) -> Vec<Entity> {
    if world.registry().component_id_of::<T>().is_none() {
        return Vec::new();
    }
    let mut found: Vec<Entity> = world
        .entities()
        .filter(|e| world.get::<T>(*e).is_some())
        .collect();
    found.sort_unstable();
    found
}

/// The X and Y of `matrix` applied to the point `(x, y, 0)`.
fn place(matrix: &Mat4, x: f32, y: f32) -> [f64; 2] {
    let p = matrix.transform_point3(Vec3::new(x, y, 0.0));
    [f64::from(p.x), f64::from(p.y)]
}

/// The outline of a collider in the X–Y plane, in the entity's own space
/// (the 2D physics profile's reading of each shape), with the shape's
/// offset.
pub fn collider_outline(collider: &Collider) -> Vec<[f32; 2]> {
    let size = collider.size;
    let [ox, oy] = [collider.offset.x, collider.offset.y];
    if !(size.is_finite() && ox.is_finite() && oy.is_finite()) {
        return Vec::new();
    }
    let rectangle = |w: f32, h: f32| {
        let (hw, hh) = (w.abs() * 0.5, h.abs() * 0.5);
        vec![
            [ox - hw, oy - hh],
            [ox + hw, oy - hh],
            [ox + hw, oy + hh],
            [ox - hw, oy + hh],
        ]
    };
    match collider.shape {
        ColliderShape::Box | ColliderShape::Cylinder => rectangle(size.x, size.y),
        ColliderShape::Sphere => {
            let r = (size.x.abs().max(size.y.abs()) * 0.5) as f64;
            // A 32-sided polygon: every fourth direction of the table.
            CIRCLE
                .iter()
                .step_by(4)
                .map(|[c, s]| {
                    [
                        (f64::from(ox) + c * r) as f32,
                        (f64::from(oy) + s * r) as f32,
                    ]
                })
                .collect()
        }
        ColliderShape::Capsule => {
            let r = f64::from(size.x.abs() * 0.5);
            let half = (f64::from(size.y.abs()) * 0.5 - r).max(0.0);
            // The top half circle (directions 0 to 64), raised by `half`,
            // then the bottom one (64 to 128), lowered.
            let step = 4;
            let mut points = Vec::new();
            for (i, [c, s]) in CIRCLE.iter().enumerate().step_by(step) {
                if i <= CIRCLE_STEPS / 2 {
                    points.push([
                        (f64::from(ox) + c * r) as f32,
                        (f64::from(oy) + half + s * r) as f32,
                    ]);
                }
            }
            for (i, [c, s]) in CIRCLE.iter().enumerate().step_by(step) {
                if i >= CIRCLE_STEPS / 2 {
                    points.push([
                        (f64::from(ox) + c * r) as f32,
                        (f64::from(oy) - half + s * r) as f32,
                    ]);
                }
            }
            // And back to the start, closing the right side.
            points.push([(f64::from(ox) + r) as f32, (f64::from(oy) - half) as f32]);
            points
        }
    }
}

/// The walls of `world` as they are now (design note §3). Tilemaps count
/// only when the world knows their shapes ([`crate::install_tile_shapes`]).
pub fn gather(world: &World) -> Occluders {
    let mut segments: Vec<Segment> = Vec::new();
    if let Some(known) = world.extension::<TileShapes>()
        && !known.is_empty()
    {
        for entity in with::<Tilemap>(world) {
            let Some(tilemap) = world.get::<Tilemap>(entity) else {
                continue;
            };
            let Some(path) = world.text(tilemap.map.text()) else {
                continue;
            };
            let path = path.split('#').next().unwrap_or(path);
            let Some(shapes) = known.get(path) else {
                continue;
            };
            let matrix = placement(world, entity);
            let [cw, ch] = tilemap.cell();
            for &[x0, y0, x1, y1] in &shapes.boxes {
                let (l, b) = (x0 as f32 * cw, y0 as f32 * ch);
                let (r, t) = (x1 as f32 * cw, y1 as f32 * ch);
                let corners = [
                    place(&matrix, l, b),
                    place(&matrix, r, b),
                    place(&matrix, r, t),
                    place(&matrix, l, t),
                ];
                segments.extend(outline_segments(&corners, entity));
            }
            for ([x, y], outline) in &shapes.outlines {
                let points: Vec<[f64; 2]> = outline
                    .iter()
                    .map(|[px, py]| place(&matrix, (*x as f32 + px) * cw, (*y as f32 + py) * ch))
                    .collect();
                segments.extend(outline_segments(&points, entity));
            }
        }
    }
    for entity in with::<Occluder>(world) {
        if !world.get::<Occluder>(entity).is_some_and(|o| o.enabled) {
            continue;
        }
        let Some(collider) = world.get::<Collider>(entity) else {
            continue;
        };
        let matrix = placement(world, entity);
        let points: Vec<[f64; 2]> = collider_outline(collider)
            .into_iter()
            .map(|[x, y]| place(&matrix, x, y))
            .collect();
        segments.extend(outline_segments(&points, entity));
    }
    Occluders::new(segments)
}

/// Whether `world` has anything that could block sight: a tilemap whose
/// shapes it knows, or an occluder.
pub fn has_walls(world: &World) -> bool {
    let tiles = world
        .extension::<TileShapes>()
        .is_some_and(|known| !known.is_empty())
        && world
            .registry()
            .component_id_of::<Tilemap>()
            .is_some_and(|id| {
                world
                    .archetypes()
                    .iter()
                    .any(|archetype| archetype.contains(id) && !archetype.is_empty())
            });
    tiles
        || world
            .registry()
            .component_id_of::<Occluder>()
            .is_some_and(|id| {
                world
                    .archetypes()
                    .iter()
                    .any(|archetype| archetype.contains(id) && !archetype.is_empty())
            })
}
