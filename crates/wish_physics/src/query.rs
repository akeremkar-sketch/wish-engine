//! Questions about the physics world (design note M6 §3.4): what a ray
//! hits, what overlaps a shape, what touches an entity. The control plane's
//! `physics_query` tool and game code's `raycast` both come here.
//!
//! Queries see the world as of the last physics step: in a running app
//! that is the state at the start of the tick, the same for every system,
//! so answers do not depend on system order. A world that has not stepped
//! yet (a scene, a session at tick 0) gets a state built for the question:
//! its colliders where the components say, nothing moved.

use std::borrow::Cow;

use rapier3d::prelude as rp;
use wish_core::math::{Quat, Vec3};
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::World;

use crate::plane::{self, HALF_DEPTH, PhysicsProfile};
use crate::{Collider, PhysicsSettings, PhysicsState, entity_of, pose, rv, sync_entity, wv};

/// What a ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The entity whose collider was hit.
    pub entity: Entity,
    /// Where, in world space.
    pub point: Vec3,
    /// The surface normal there, pointing out of the collider.
    pub normal: Vec3,
    /// How far along the ray, in meters.
    pub distance: f32,
}

/// A shape to test for overlaps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlapShape {
    /// A ball of this radius.
    Sphere(f32),
    /// A box of these half extents.
    Box(Vec3),
}

/// The physics state of `world` ready for queries: its own after a step,
/// or one built for the question.
pub fn query_state(world: &World) -> Result<Cow<'_, PhysicsState>> {
    if let Some(state) = world.extension::<PhysicsState>() {
        return Ok(Cow::Borrowed(state));
    }
    let profile = world
        .resource::<PhysicsSettings>()
        .map_or(PhysicsProfile::Full3D, |settings| settings.profile);
    let mut state = PhysicsState::for_profile(profile);
    let mut present: Vec<Entity> = world
        .entities()
        .filter(|entity| world.has::<Collider>(*entity))
        .collect();
    present.sort_unstable();
    for entity in present {
        sync_entity(world, &mut state, entity)?;
    }
    // Collision detection only: places the colliders in the query tree and
    // finds contacts; nothing moves.
    // The contacts found here are reported by the first step (Rapier reports
    // a contact once, when it is first found).
    let collected = crate::Collected::default();
    let PhysicsState {
        islands,
        broad,
        narrow,
        bodies,
        colliders,
        ..
    } = &mut state;
    rp::CollisionPipeline::new().step(
        0.002,
        islands,
        broad,
        narrow,
        bodies,
        colliders,
        &(),
        &collected,
    );
    state.pending = collected.events.into_inner().unwrap_or_default();
    Ok(Cow::Owned(state))
}

fn groups(mask: u32) -> rp::InteractionGroups {
    rp::InteractionGroups::new(
        rp::Group::ALL,
        rp::Group::from_bits_retain(mask),
        rp::InteractionTestMode::And,
    )
}

fn invalid(what: &str) -> Diagnostic {
    Diagnostic::error(
        "invalid_parameter",
        format!("The physics query is not valid: {what}."),
        "A ray needs a finite origin, a direction that is not zero, and a positive maximum distance; a shape needs a positive size.",
    )
}

impl PhysicsState {
    fn filter(&self, mask: u32, exclude: Option<Entity>, sensors: bool) -> rp::QueryFilter<'_> {
        let mut filter = rp::QueryFilter::default().groups(groups(mask));
        if !sensors {
            filter = filter.exclude_sensors();
        }
        if let Some(link) = exclude.and_then(|entity| self.links.get(&entity)) {
            filter = filter.exclude_rigid_body(link.body);
        }
        filter
    }

    /// The first collider a ray from `from` along `direction` hits within
    /// `max` meters. Sensors are not hit; `exclude` (the asker) is skipped;
    /// only colliders whose layer is in `mask` count. In a 2D profile the
    /// ray is laid in the plane (the z of `from` and `direction` is not
    /// used) and the hit is at z = 0.
    pub fn raycast(
        &self,
        from: Vec3,
        direction: Vec3,
        max: f32,
        mask: u32,
        exclude: Option<Entity>,
    ) -> Result<Option<RayHit>> {
        let (from, direction) = if plane::is_2d(self.profile) {
            if from.is_finite() && direction.is_finite() && direction.with_z(0.0).length() < 1e-9 {
                return Err(Diagnostic::error(
                    "invalid_parameter",
                    "The physics query is not valid: in a 2D physics profile a ray goes along the X–Y plane, and this one points along Z.".to_owned(),
                    "Give the ray a direction with an x or y part, such as [1, 0] or [0, -1]; to ask what is at a point, use an overlap with a small circle there.",
                ));
            }
            (from.with_z(0.0), direction.with_z(0.0))
        } else {
            (from, direction)
        };
        let length = direction.length();
        if !from.is_finite() || !direction.is_finite() || length < 1e-9 {
            return Err(invalid("the ray has no direction or is not finite"));
        }
        if !(max.is_finite() && max > 0.0) {
            return Err(invalid("the maximum distance must be a positive number"));
        }
        let direction = direction / length;
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            self.filter(mask, exclude, false),
        );
        let ray = rp::Ray::new(rv(from), rv(direction));
        Ok(queries
            .cast_ray_and_get_normal(&ray, max, true)
            .and_then(|(handle, hit)| {
                let normal = wv(hit.normal);
                Some(RayHit {
                    entity: entity_of(&self.colliders, handle)?,
                    point: from + direction * hit.time_of_impact,
                    normal: if plane::is_2d(self.profile) {
                        normal.with_z(0.0)
                    } else {
                        normal
                    },
                    distance: hit.time_of_impact,
                })
            }))
    }

    /// Every entity whose collider overlaps `shape` placed at `at`, in
    /// entity order. Sensors count. In a 2D profile the shape is its
    /// outline in the plane at `at`'s x and y (a sphere is a circle, a box a
    /// rectangle: its z is not used), turned only about Z.
    pub fn overlap(
        &self,
        shape: OverlapShape,
        at: Vec3,
        rotation: Quat,
        mask: u32,
    ) -> Result<Vec<Entity>> {
        let flat = plane::is_2d(self.profile);
        let shape: rp::SharedShape = match shape {
            OverlapShape::Sphere(radius) if radius.is_finite() && radius > 0.0 => {
                rp::SharedShape::ball(radius)
            }
            OverlapShape::Box(half) if flat && half.is_finite() && half.x > 0.0 && half.y > 0.0 => {
                rp::SharedShape::cuboid(half.x, half.y, HALF_DEPTH)
            }
            OverlapShape::Box(half) if !flat && half.is_finite() && half.min_element() > 0.0 => {
                rp::SharedShape::cuboid(half.x, half.y, half.z)
            }
            _ => return Err(invalid("the shape's size must be positive")),
        };
        if !at.is_finite() {
            return Err(invalid("the position is not finite"));
        }
        let (at, rotation) = plane::flatten(self.profile, at, rotation);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            self.filter(mask, None, true),
        );
        let mut found: Vec<Entity> = queries
            .intersect_shape(pose(at, rotation), shape.as_ref())
            .filter_map(|(handle, _)| entity_of(&self.colliders, handle))
            .collect();
        found.sort_unstable();
        found.dedup();
        Ok(found)
    }

    /// The entities touching `entity` (contacts) or overlapping it (sensors),
    /// in entity order; empty if it has no collider.
    pub fn touching(&self, entity: Entity) -> Vec<Entity> {
        let Some(link) = self.links.get(&entity) else {
            return Vec::new();
        };
        let other = |a: rp::ColliderHandle, b: rp::ColliderHandle| {
            if a == link.collider { b } else { a }
        };
        let mut found: Vec<Entity> = self
            .narrow
            .contact_pairs_with(link.collider)
            .filter(|pair| pair.has_any_active_contact())
            .filter_map(|pair| entity_of(&self.colliders, other(pair.collider1, pair.collider2)))
            .chain(
                self.narrow
                    .intersection_pairs_with(link.collider)
                    .filter(|(_, _, on)| *on)
                    .filter_map(|(a, b, _)| entity_of(&self.colliders, other(a, b))),
            )
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}
