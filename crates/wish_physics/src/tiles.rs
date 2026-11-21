//! Walls from tilemaps (design note `docs/design/1.1-tilemaps.md` §7).
//!
//! What a tilemap's solid tiles are is read from its file by whoever reads
//! the project (the control plane), merged into boxes there, and handed to
//! the world as the extension [`TileShapes`] when a session starts. Physics
//! gives every entity with a `Tilemap` whose map has shapes one fixed body
//! with one compound collider: its boxes (1 m deep, in the map's plane) and
//! its outlined tiles, scaled by the cell size and the entity's scale. One
//! collider per map keeps contacts and their events one per pair of
//! entities, as for any other collider.

use std::collections::BTreeMap;
use std::sync::Arc;

use rapier3d::prelude as rp;
use wish_core::math::{Quat, Vec3};
use wish_core::{Entity, Result};
use wish_ecs::World;
use wish_types::tilemap::Tilemap;

use crate::plane::{self, HALF_DEPTH};
use crate::{Collider, PhysicsState, groups, pose, rv, world_placement};

/// The collision of one tilemap file, in cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapShapes {
    /// Boxes of whole solid cells: lowest x and y, then one past the highest
    /// x and y.
    pub boxes: Vec<[i32; 4]>,
    /// Cells with an outlined solid tile: the cell, and the outline in tile
    /// units from the cell's bottom left corner.
    pub outlines: Vec<([i32; 2], Vec<[f32; 2]>)>,
}

impl MapShapes {
    /// Whether there is nothing solid.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.outlines.is_empty()
    }
}

/// The collision of the project's tilemaps by file path: a world extension
/// (ADR-026), filled when a session starts ([`install_tile_shapes`]).
#[derive(Clone, Debug, Default)]
pub struct TileShapes {
    maps: BTreeMap<String, Arc<MapShapes>>,
}

impl TileShapes {
    /// The shapes of the tilemap file `path`, if known.
    pub fn get(&self, path: &str) -> Option<&Arc<MapShapes>> {
        self.maps.get(path)
    }
}

/// Tells the world what the solid parts of its tilemaps are (replacing what
/// it knew of the same files).
pub fn install_tile_shapes(world: &mut World, maps: impl IntoIterator<Item = (String, MapShapes)>) {
    let mut known = world.take_extension::<TileShapes>().unwrap_or_default();
    for (path, shapes) in maps {
        known.maps.insert(path, Arc::new(shapes));
    }
    world.insert_extension(known);
}

/// What physics remembers about one tilemap entity.
#[derive(Clone, Debug)]
pub(crate) struct TileLink {
    pub(crate) body: rp::RigidBodyHandle,
    pub(crate) collider: rp::ColliderHandle,
    /// What the collider was built from: the shapes, the cell size, the
    /// world scale and the profile.
    shapes: Arc<MapShapes>,
    cell: [f32; 2],
    scale: Vec3,
}

/// The tilemap entities that take part, in entity order, with their shapes.
pub(crate) fn present(world: &World) -> Result<Vec<(Entity, Arc<MapShapes>)>> {
    let Some(known) = world.extension::<TileShapes>() else {
        return Ok(Vec::new());
    };
    // A world with no walls of tilemaps (most of them) costs nothing here.
    if known.maps.is_empty() || world.registry().component_id_of::<Tilemap>().is_none() {
        return Ok(Vec::new());
    }
    let mut found: Vec<(Entity, Arc<MapShapes>)> = world
        .entities()
        .filter_map(|entity| {
            let tilemap = world.get::<Tilemap>(entity)?;
            let path = world.text(tilemap.map.text())?;
            let path = path.split('#').next().unwrap_or(path);
            let shapes = known.get(path)?;
            (!shapes.is_empty()).then(|| (entity, Arc::clone(shapes)))
        })
        .collect();
    found.sort_unstable_by_key(|(entity, _)| *entity);
    Ok(found)
}

/// The compound shape of `shapes` for cells of `cell` meters and a world
/// scale of `scale`; `None` when nothing in it can be built.
fn compound(shapes: &MapShapes, cell: [f32; 2], scale: Vec3) -> Option<rp::SharedShape> {
    let (sx, sy) = (cell[0] * scale.x, cell[1] * scale.y);
    if !(sx.is_finite() && sy.is_finite()) || sx == 0.0 || sy == 0.0 {
        return None;
    }
    let mut parts: Vec<(rp::Pose, rp::SharedShape)> = Vec::new();
    for &[x0, y0, x1, y1] in &shapes.boxes {
        let centre = Vec3::new(
            (x0 + x1) as f32 * 0.5 * sx,
            (y0 + y1) as f32 * 0.5 * sy,
            0.0,
        );
        let half = (
            ((x1 - x0) as f32 * 0.5 * sx).abs().max(0.0005),
            ((y1 - y0) as f32 * 0.5 * sy).abs().max(0.0005),
        );
        parts.push((
            pose(centre, Quat::IDENTITY),
            rp::SharedShape::cuboid(half.0, half.1, HALF_DEPTH),
        ));
    }
    for ([x, y], outline) in &shapes.outlines {
        let points: Vec<rp::Vector> = outline
            .iter()
            .flat_map(|[px, py]| {
                let at = Vec3::new((*x as f32 + px) * sx, (*y as f32 + py) * sy, 0.0);
                [rv(at.with_z(-HALF_DEPTH)), rv(at.with_z(HALF_DEPTH))]
            })
            .collect();
        // An outline is checked when its tileset is read; the hull is built
        // by an outside library all the same, so a surprise there costs
        // that one tile, not the session.
        let hull = std::panic::catch_unwind(|| rp::SharedShape::convex_hull(&points))
            .ok()
            .flatten();
        if let Some(hull) = hull {
            parts.push((pose(Vec3::ZERO, Quat::IDENTITY), hull));
        }
    }
    (!parts.is_empty()).then(|| rp::SharedShape::compound(parts))
}

/// Brings the tilemap bodies up to date with the world: new ones built,
/// changed ones (other shapes, cell size or scale) built again, moved ones
/// moved, gone ones removed. Removed colliders are added to `removed` with
/// their entity, so that "stopped touching" events about them still name
/// it.
pub(crate) fn sync(
    world: &World,
    state: &mut PhysicsState,
    removed: &mut Vec<(rp::ColliderHandle, Entity)>,
) -> Result<()> {
    let present = present(world)?;
    let gone: Vec<Entity> = state
        .tiles
        .keys()
        .filter(|entity| present.binary_search_by_key(*entity, |(e, _)| *e).is_err())
        .copied()
        .collect();
    for entity in gone {
        if let Some(handle) = remove(state, entity) {
            removed.push((handle, entity));
        }
    }
    for (entity, shapes) in present {
        let Some(tilemap) = world.get::<Tilemap>(entity).copied() else {
            continue;
        };
        let cell = tilemap.cell();
        let (scale, rotation, translation) = world_placement(world, entity);
        let (translation, rotation) = plane::flatten(state.profile, translation, rotation);
        let rebuild = match state.tiles.get(&entity) {
            Some(link) => {
                !Arc::ptr_eq(&link.shapes, &shapes) || link.cell != cell || link.scale != scale
            }
            None => true,
        };
        if rebuild {
            if let Some(handle) = remove(state, entity) {
                removed.push((handle, entity));
            }
            let Some(shape) = compound(&shapes, cell, scale) else {
                continue;
            };
            let body = rp::RigidBodyBuilder::fixed()
                .pose(pose(translation, rotation))
                .user_data(u128::from(entity.to_bits()))
                .build();
            let body = state.bodies.insert(body);
            let defaults = Collider::default();
            let collider = rp::ColliderBuilder::new(shape)
                .user_data(u128::from(entity.to_bits()))
                .friction(defaults.friction.clamp(0.0, 10.0))
                .restitution(defaults.restitution.clamp(0.0, 1.0))
                .restitution_combine_rule(rp::CoefficientCombineRule::Max)
                .collision_groups(groups(&defaults))
                .active_events(rp::ActiveEvents::COLLISION_EVENTS)
                .active_collision_types(rp::ActiveCollisionTypes::all())
                .build();
            let collider = state
                .colliders
                .insert_with_parent(collider, body, &mut state.bodies);
            state.tiles.insert(
                entity,
                TileLink {
                    body,
                    collider,
                    shapes,
                    cell,
                    scale,
                },
            );
        } else if let Some(link) = state.tiles.get(&entity)
            && let Some(body) = state.bodies.get_mut(link.body)
        {
            let current = body.position();
            if current.translation != rv(translation) || crate::wq(current.rotation) != rotation {
                body.set_position(pose(translation, rotation), true);
            }
        }
    }
    Ok(())
}

/// Forgets the tilemap body of `entity`; its collider's handle, if it had
/// one.
fn remove(state: &mut PhysicsState, entity: Entity) -> Option<rp::ColliderHandle> {
    let link = state.tiles.remove(&entity)?;
    state.bodies.remove(
        link.body,
        &mut state.islands,
        &mut state.colliders,
        &mut state.impulse_joints,
        &mut state.multibody_joints,
        &mut state.soft,
        true,
    );
    Some(link.collider)
}
