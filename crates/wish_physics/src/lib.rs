//! Physics for Wish Engine (ADR-014, design note M6 §3). Rapier lives behind
//! this crate; the rest of the engine sees components, events and queries
//! (`wish_types::physics`).
//!
//! Every tick, [`step`]:
//! 1. brings Rapier up to date with the ECS (ECS is the source of truth):
//!    new, changed and removed colliders and bodies; a `Transform` or
//!    `BodyVelocity` written by anyone but physics moves or speeds the body;
//! 2. moves character controllers;
//! 3. steps the simulation (deterministic: `enhanced-determinism`, one
//!    thread, entities always visited in entity order);
//! 4. writes every moving body's place and speed back into its `Transform`
//!    and `BodyVelocity`, and sends collision events.
//!
//! Rapier's own state (broad phase, contact caches, warm starting) is a
//! world extension (ADR-026): snapshots carry it, the world hash does not
//! need it because everything gameplay sees is written back to components.

use std::collections::BTreeMap;
use std::sync::Mutex;

mod query;

pub use query::{OverlapShape, RayHit, query_state};

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude as rp;
use wish_core::math::{Mat4, Quat, Vec3};
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{ChangeTick, World};
pub use wish_types::physics::{
    BodyForce, BodyImpulse, BodyKind, BodyVelocity, CharacterController, CharacterMotion,
    CharacterState, Collider, ColliderShape, CollisionEnded, CollisionStarted, PhysicsSettings,
    RigidBody,
};
use wish_types::time::Time;
use wish_types::transform::{GlobalTransform, Transform};

/// The name of the physics system in an app.
pub const SYSTEM: &str = "physics";

/// Builds the physics state of a world that has none (a session about to
/// start): colliders placed, contacts found, nothing moved. The first tick
/// then continues from it, and queries during that tick see the scene.
pub fn prepare(world: &mut World) -> Result<()> {
    if world.extension::<PhysicsState>().is_none() {
        let state = query_state(world)?.into_owned();
        world.insert_extension(state);
    }
    Ok(())
}

/// The physics system, for `App::add_system`. Order it after the systems
/// that move things and before transform propagation.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, context| step(world, context.last_run))
}

/// Registers the physics components, resources and events.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<RigidBody>()?;
    world.register_component::<Collider>()?;
    world.register_component::<BodyVelocity>()?;
    world.register_component::<BodyForce>()?;
    world.register_component::<CharacterController>()?;
    world.register_component::<CharacterMotion>()?;
    world.register_component::<CharacterState>()?;
    world.register_resource::<PhysicsSettings>()?;
    world.register_event::<BodyImpulse>()?;
    world.register_event::<CollisionStarted>()?;
    world.register_event::<CollisionEnded>()?;
    Ok(())
}

// --------------------------------------------------------------------------
// Conversions: our math (glam 0.34) and Rapier's (glam 0.33) meet as arrays.
// --------------------------------------------------------------------------

fn rv(v: Vec3) -> rp::Vector {
    rp::Vector::new(v.x, v.y, v.z)
}

fn wv(v: rp::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rq(q: Quat) -> rp::Rotation {
    rp::Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

fn wq(q: rp::Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

fn pose(translation: Vec3, rotation: Quat) -> rp::Pose {
    rp::Pose::from_parts(rv(translation), rq(rotation))
}

/// A rotation that is safe to hand to Rapier: unit length, finite.
fn clean(rotation: Quat) -> Quat {
    if rotation.is_finite() && rotation.length_squared() > 1e-12 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    }
}

// --------------------------------------------------------------------------
// State
// --------------------------------------------------------------------------

/// What physics remembers about one entity.
#[derive(Clone, Debug)]
struct Link {
    body: rp::RigidBodyHandle,
    collider: rp::ColliderHandle,
    /// The body settings last given to Rapier (`None`: no `RigidBody`, a
    /// fixed collider).
    body_desc: Option<RigidBody>,
    /// The collider settings last given to Rapier, and the world scale.
    collider_desc: Collider,
    scale: Vec3,
    /// The `Transform` and `BodyVelocity` physics wrote last; anything else
    /// found there next tick was written by someone else (a teleport).
    written: Option<(Transform, BodyVelocity)>,
}

/// Rapier's state for one world: a world extension (ADR-026).
pub struct PhysicsState {
    /// Scratch memory of a step; not carried by snapshots (a new one is
    /// equivalent, proven by `wish_physics/tests/proof.rs`).
    pipeline: rp::PhysicsPipeline,
    islands: rp::IslandManager,
    broad: rp::BroadPhaseBvh,
    narrow: rp::NarrowPhase,
    bodies: rp::RigidBodySet,
    colliders: rp::ColliderSet,
    impulse_joints: rp::ImpulseJointSet,
    multibody_joints: rp::MultibodyJointSet,
    soft: rp::SoftBodySet,
    ccd: rp::CCDSolver,
    links: BTreeMap<Entity, Link>,
    /// Collision events found while the state was prepared (before the
    /// first step): sent with the first step's own, so that contacts that
    /// exist at the start are reported like any other.
    pending: Vec<rp::CollisionEvent>,
}

impl Clone for PhysicsState {
    fn clone(&self) -> Self {
        Self {
            pipeline: rp::PhysicsPipeline::new(),
            islands: self.islands.clone(),
            broad: self.broad.clone(),
            narrow: self.narrow.clone(),
            bodies: self.bodies.clone(),
            colliders: self.colliders.clone(),
            impulse_joints: self.impulse_joints.clone(),
            multibody_joints: self.multibody_joints.clone(),
            soft: self.soft.clone(),
            ccd: self.ccd.clone(),
            links: self.links.clone(),
            pending: self.pending.clone(),
        }
    }
}

impl Default for PhysicsState {
    fn default() -> Self {
        Self {
            pipeline: rp::PhysicsPipeline::new(),
            islands: rp::IslandManager::new(),
            broad: rp::BroadPhaseBvh::new(),
            narrow: rp::NarrowPhase::new(),
            bodies: rp::RigidBodySet::new(),
            colliders: rp::ColliderSet::new(),
            impulse_joints: rp::ImpulseJointSet::new(),
            multibody_joints: rp::MultibodyJointSet::new(),
            soft: rp::SoftBodySet::new(),
            ccd: rp::CCDSolver::new(),
            links: BTreeMap::new(),
            pending: Vec::new(),
        }
    }
}

impl PhysicsState {
    /// How many entities physics knows about.
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Whether physics knows about no entity.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// Whether the entity's body is asleep (at rest, costing nothing).
    pub fn is_sleeping(&self, entity: Entity) -> Option<bool> {
        let link = self.links.get(&entity)?;
        self.bodies.get(link.body).map(rp::RigidBody::is_sleeping)
    }

    /// Forgets `entity`; returns its collider's handle, so that events about
    /// it found in this step can still name it.
    fn remove(&mut self, entity: Entity) -> Option<rp::ColliderHandle> {
        let link = self.links.remove(&entity)?;
        {
            self.bodies.remove(
                link.body,
                &mut self.islands,
                &mut self.colliders,
                &mut self.impulse_joints,
                &mut self.multibody_joints,
                &mut self.soft,
                true,
            );
        }
        Some(link.collider)
    }
}

// --------------------------------------------------------------------------
// Building Rapier objects from components
// --------------------------------------------------------------------------

fn body_type(desc: Option<&RigidBody>) -> rp::RigidBodyType {
    match desc.map(|d| d.kind) {
        Some(BodyKind::Dynamic) => rp::RigidBodyType::Dynamic,
        Some(BodyKind::Kinematic) => rp::RigidBodyType::KinematicPositionBased,
        Some(BodyKind::Fixed) | None => rp::RigidBodyType::Fixed,
    }
}

fn locked_axes(desc: &RigidBody) -> rp::LockedAxes {
    let mut axes = rp::LockedAxes::empty();
    let flags = [
        (
            desc.lock_translation[0],
            rp::LockedAxes::TRANSLATION_LOCKED_X,
        ),
        (
            desc.lock_translation[1],
            rp::LockedAxes::TRANSLATION_LOCKED_Y,
        ),
        (
            desc.lock_translation[2],
            rp::LockedAxes::TRANSLATION_LOCKED_Z,
        ),
        (desc.lock_rotation[0], rp::LockedAxes::ROTATION_LOCKED_X),
        (desc.lock_rotation[1], rp::LockedAxes::ROTATION_LOCKED_Y),
        (desc.lock_rotation[2], rp::LockedAxes::ROTATION_LOCKED_Z),
    ];
    for (on, flag) in flags {
        if on {
            axes |= flag;
        }
    }
    axes
}

/// A speed with the locked axes taken out: Rapier keeps a locked axis still
/// only for what the solver does, not for a speed set from outside.
fn masked(velocity: BodyVelocity, desc: Option<&RigidBody>) -> BodyVelocity {
    let Some(desc) = desc else {
        return velocity;
    };
    let mask = |v: Vec3, locks: [bool; 3]| {
        Vec3::new(
            if locks[0] { 0.0 } else { v.x },
            if locks[1] { 0.0 } else { v.y },
            if locks[2] { 0.0 } else { v.z },
        )
    };
    BodyVelocity {
        linear: mask(velocity.linear, desc.lock_translation),
        angular: mask(velocity.angular, desc.lock_rotation),
    }
}

fn set_velocity(body: &mut rp::RigidBody, velocity: BodyVelocity, desc: Option<&RigidBody>) {
    if !(velocity.linear.is_finite() && velocity.angular.is_finite()) {
        return;
    }
    let velocity = masked(velocity, desc);
    body.set_linvel(rv(velocity.linear), true);
    body.set_angvel(rv(velocity.angular), true);
}

fn apply_body(body: &mut rp::RigidBody, desc: Option<&RigidBody>) {
    body.set_body_type(body_type(desc), true);
    let defaults = RigidBody {
        kind: BodyKind::Fixed,
        ..RigidBody::default()
    };
    let desc = desc.unwrap_or(&defaults);
    body.set_linear_damping(desc.linear_damping.max(0.0));
    body.set_angular_damping(desc.angular_damping.max(0.0));
    body.set_gravity_scale(desc.gravity_scale, true);
    body.enable_ccd(desc.ccd);
    body.set_locked_axes(locked_axes(desc), true);
    let mut activation = *body.activation();
    if desc.can_sleep {
        activation.normalized_linear_threshold =
            rp::RigidBodyActivation::default_normalized_linear_threshold();
        activation.angular_threshold = rp::RigidBodyActivation::default_angular_threshold();
    } else {
        activation.normalized_linear_threshold = -1.0;
        activation.angular_threshold = -1.0;
    }
    *body.activation_mut() = activation;
}

/// A size that Rapier accepts: finite, at least a millimetre.
fn dim(value: f32) -> f32 {
    if value.is_finite() {
        value.abs().max(0.001)
    } else {
        0.001
    }
}

fn shape_of(desc: &Collider, scale: Vec3) -> rp::SharedShape {
    let size = desc.size * scale.abs();
    match desc.shape {
        ColliderShape::Box => {
            rp::SharedShape::cuboid(dim(size.x) / 2.0, dim(size.y) / 2.0, dim(size.z) / 2.0)
        }
        ColliderShape::Sphere => {
            let diameter = dim(size.x).max(dim(size.y)).max(dim(size.z));
            rp::SharedShape::ball(diameter / 2.0)
        }
        ColliderShape::Capsule => {
            let radius = dim(size.x.max(size.z)) / 2.0;
            let half_segment = (dim(size.y) / 2.0 - radius).max(0.0);
            rp::SharedShape::capsule_y(half_segment, radius)
        }
        ColliderShape::Cylinder => {
            rp::SharedShape::cylinder(dim(size.y) / 2.0, dim(size.x.max(size.z)) / 2.0)
        }
    }
}

fn groups(desc: &Collider) -> rp::InteractionGroups {
    rp::InteractionGroups::new(
        rp::Group::from_bits_retain(desc.layer),
        rp::Group::from_bits_retain(desc.mask),
        rp::InteractionTestMode::And,
    )
}

fn apply_collider(collider: &mut rp::Collider, desc: &Collider, scale: Vec3) {
    collider.set_shape(shape_of(desc, scale));
    collider.set_translation_wrt_parent(rv(desc.offset * scale));
    collider.set_friction(desc.friction.clamp(0.0, 10.0));
    collider.set_restitution(desc.restitution.clamp(0.0, 1.0));
    // The bouncier of two decides, as `Collider.restitution` says: a ball
    // of 1 bounces back to its height off a floor of 0 (Rapier's default
    // averages them, which would halve it).
    collider.set_restitution_combine_rule(rp::CoefficientCombineRule::Max);
    collider.set_density(desc.density.clamp(0.001, 100_000.0));
    collider.set_sensor(desc.sensor);
    collider.set_collision_groups(groups(desc));
    collider.set_active_events(rp::ActiveEvents::COLLISION_EVENTS);
    collider.set_active_collision_types(rp::ActiveCollisionTypes::all());
}

// --------------------------------------------------------------------------
// Where an entity is in the world
// --------------------------------------------------------------------------

/// The parent's world matrix (identity for a root).
fn parent_matrix(world: &World, entity: Entity) -> Mat4 {
    world
        .parent(entity)
        .and_then(|parent| world.get::<GlobalTransform>(parent))
        .map_or(Mat4::IDENTITY, |global| global.matrix)
}

/// The entity's world placement: scale, rotation, translation.
fn world_placement(world: &World, entity: Entity) -> (Vec3, Quat, Vec3) {
    let Some(local) = world.get::<Transform>(entity).copied() else {
        let matrix = world
            .get::<GlobalTransform>(entity)
            .map_or(Mat4::IDENTITY, |g| g.matrix);
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        return (scale, clean(rotation), translation);
    };
    if world.parent(entity).is_none() {
        return (local.scale, clean(local.rotation), local.translation);
    }
    let matrix = parent_matrix(world, entity) * local.to_matrix();
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    (scale, clean(rotation), translation)
}

/// The local `Transform` that puts `entity` at `translation`/`rotation` in
/// the world, keeping its own scale.
fn local_for(world: &World, entity: Entity, translation: Vec3, rotation: Quat) -> Transform {
    let current = world.get::<Transform>(entity).copied().unwrap_or_default();
    if world.parent(entity).is_none() {
        return Transform {
            translation,
            rotation,
            scale: current.scale,
        };
    }
    let parent = parent_matrix(world, entity);
    let world_matrix = Mat4::from_rotation_translation(rotation, translation);
    let (_, rotation, translation) =
        (parent.inverse() * world_matrix).to_scale_rotation_translation();
    Transform {
        translation,
        rotation,
        scale: current.scale,
    }
}

// --------------------------------------------------------------------------
// The step
// --------------------------------------------------------------------------

/// Collects Rapier's collision events during a step (in the order Rapier
/// reports them, which is deterministic).
#[derive(Default)]
pub(crate) struct Collected {
    pub(crate) events: Mutex<Vec<rp::CollisionEvent>>,
}

impl rp::EventHandler for Collected {
    fn handle_collision_event(
        &self,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        event: rp::CollisionEvent,
        _contact_pair: Option<&rp::ContactPair>,
    ) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: f32,
        _bodies: &rp::RigidBodySet,
        _colliders: &rp::ColliderSet,
        _contact_pair: &rp::ContactPair,
        _total_force_magnitude: f32,
    ) {
    }

    fn handle_soft_body_tear_event(
        &self,
        _soft_bodies: &rp::SoftBodySet,
        _event: &rp::SoftBodyTearEvent,
    ) {
    }
}

fn entity_of(colliders: &rp::ColliderSet, handle: rp::ColliderHandle) -> Option<Entity> {
    colliders
        .get(handle)
        .map(|collider| Entity::from_bits(collider.user_data as u64))
}

fn internal(what: &str) -> Diagnostic {
    Diagnostic::error(
        "internal_error",
        format!("Physics lost track of its state: {what}."),
        "This is a bug in Wish Engine; restarting the session recovers. Please report it.",
    )
}

/// One physics tick for `world`: the system [`SYSTEM`]. `since` is where
/// the system last ran (events sent after it are read).
pub fn step(world: &mut World, since: ChangeTick) -> Result<()> {
    // Taken out while it works (not copied), put back at the end (also on
    // failure: the state stays consistent with what it last did).
    let mut state = world.take_extension::<PhysicsState>().unwrap_or_default();
    let result = run(world, &mut state, since);
    world.insert_extension(state);
    result
}

fn run(world: &mut World, state: &mut PhysicsState, since: ChangeTick) -> Result<()> {
    let settings = world
        .resource::<PhysicsSettings>()
        .copied()
        .unwrap_or_default();
    let delta = world.resource::<Time>().map_or(1.0 / 60.0, |t| t.delta);

    // 1. Who takes part, in entity order.
    let mut present: Vec<Entity> = world
        .query::<(Entity, &Collider)>()?
        .map(|(entity, _)| entity)
        .collect();
    present.sort_unstable();
    let gone: Vec<Entity> = state
        .links
        .keys()
        .filter(|entity| present.binary_search(entity).is_err())
        .copied()
        .collect();
    // Colliders removed this tick, so that "stopped touching" events about
    // them still reach the entity that stays (M6 closing review).
    let mut removed: Vec<(rp::ColliderHandle, Entity)> = Vec::new();
    for entity in gone {
        if let Some(handle) = state.remove(entity) {
            removed.push((handle, entity));
        }
    }

    // 2. Bring Rapier up to date.
    for &entity in &present {
        sync_entity(world, state, entity)?;
    }

    // 3. Kicks sent since the last run.
    for impulse in world.events_since::<BodyImpulse>(since)? {
        let Some(link) = state.links.get(&impulse.entity) else {
            continue;
        };
        if let Some(body) = state.bodies.get_mut(link.body) {
            if impulse.linear.is_finite() {
                body.apply_impulse(rv(impulse.linear), true);
            }
            if impulse.angular.is_finite() {
                body.apply_torque_impulse(rv(impulse.angular), true);
            }
        }
    }

    // Where the moving (kinematic, not character) bodies are before the
    // step: what they carry is how far they really went (M7 friction log).
    let before: Vec<(rp::RigidBodyHandle, Mat4)> = state
        .links
        .iter()
        .filter(|(entity, link)| {
            link.body_desc.map(|d| d.kind) == Some(BodyKind::Kinematic)
                && !world.has::<CharacterController>(**entity)
        })
        .filter_map(|(_, link)| {
            let body = state.bodies.get(link.body)?;
            let at = body.position();
            Some((
                link.body,
                Mat4::from_rotation_translation(wq(at.rotation), wv(at.translation)),
            ))
        })
        .collect();

    // Who stands on one of them, and where, found now: the query tree
    // still knows where everything was last tick (M7 closing review).
    let standing = standing_on(world, state, &before);

    // 4. Step.
    let substeps = settings.substeps.clamp(1, 8);
    let params = rp::IntegrationParameters {
        dt: delta / substeps as f32,
        ..rp::IntegrationParameters::default()
    };
    let gravity = if settings.gravity.is_finite() {
        rv(settings.gravity)
    } else {
        rp::Vector::new(0.0, -9.81, 0.0)
    };
    let collected = Collected::default();
    for _ in 0..substeps {
        state.pipeline.step(
            gravity,
            &params,
            &mut state.islands,
            &mut state.broad,
            &mut state.narrow,
            &mut state.bodies,
            &mut state.colliders,
            &mut state.impulse_joints,
            &mut state.multibody_joints,
            &mut state.soft,
            &mut state.ccd,
            &(),
            &collected,
        );
    }

    // 5. Characters, against the stepped world: the query tree only knows
    // colliders after a step, and moving first would let a character fall
    // through a floor added this tick.
    move_characters(world, state, delta, &before, &standing)?;

    // 6. Write back.
    for &entity in &present {
        write_back(world, state, entity)?;
    }

    // 7. Events, a before b (those found when the state was prepared first).
    let mut events = std::mem::take(&mut state.pending);
    events.extend(collected.events.into_inner().unwrap_or_default());
    for event in events {
        let (h1, h2, started, sensor) = match event {
            rp::CollisionEvent::Started(a, b, flags) => {
                (a, b, true, flags.contains(rp::CollisionEventFlags::SENSOR))
            }
            rp::CollisionEvent::Stopped(a, b, flags) => {
                (a, b, false, flags.contains(rp::CollisionEventFlags::SENSOR))
            }
        };
        let named = |handle: rp::ColliderHandle| {
            entity_of(&state.colliders, handle)
                .or_else(|| removed.iter().find(|(h, _)| *h == handle).map(|(_, e)| *e))
        };
        let (Some(e1), Some(e2)) = (named(h1), named(h2)) else {
            continue;
        };
        let (a, b) = if e1 <= e2 { (e1, e2) } else { (e2, e1) };
        if started {
            world.send(CollisionStarted { a, b, sensor })?;
        } else {
            world.send(CollisionEnded { a, b, sensor })?;
        }
    }
    Ok(())
}

/// How far from a character contacts count: twice the controller's skin.
/// The controller keeps the character a skin away from what it stands on,
/// so walking does not start and end the contact with the ground every tick
/// (M7 friction log). Set from the first tick, so the contacts found when
/// the state is prepared agree with later ones (M7 closing review). A side
/// effect: bodies resting against a character keep that gap from it.
fn contact_skin(world: &World, entity: Entity) -> f32 {
    world
        .get::<CharacterController>(entity)
        .map_or(0.0, |controller| controller.skin.clamp(0.001, 0.5) * 2.0)
}

fn sync_entity(world: &World, state: &mut PhysicsState, entity: Entity) -> Result<()> {
    let Some(collider_desc) = world.get::<Collider>(entity).copied() else {
        return Ok(());
    };
    let body_desc = world.get::<RigidBody>(entity).copied();
    let (scale, rotation, translation) = world_placement(world, entity);
    let link = match state.links.get(&entity) {
        Some(link) => link.clone(),
        None => {
            let mut body = rp::RigidBodyBuilder::new(body_type(body_desc.as_ref()))
                .pose(pose(translation, rotation))
                .user_data(u128::from(entity.to_bits()))
                .build();
            apply_body(&mut body, body_desc.as_ref());
            if let (Some(BodyKind::Dynamic), Some(velocity)) =
                (body_desc.map(|d| d.kind), world.get::<BodyVelocity>(entity))
            {
                set_velocity(&mut body, *velocity, body_desc.as_ref());
            }
            let handle = state.bodies.insert(body);
            let mut collider = rp::ColliderBuilder::new(shape_of(&collider_desc, scale))
                .user_data(u128::from(entity.to_bits()))
                .build();
            apply_collider(&mut collider, &collider_desc, scale);
            collider.set_contact_skin(contact_skin(world, entity));
            let collider = state
                .colliders
                .insert_with_parent(collider, handle, &mut state.bodies);
            let link = Link {
                body: handle,
                collider,
                body_desc,
                collider_desc,
                scale,
                written: None,
            };
            state.links.insert(entity, link.clone());
            return Ok(());
        }
    };
    let PhysicsState {
        bodies, colliders, ..
    } = state;
    let body = bodies
        .get_mut(link.body)
        .ok_or_else(|| internal("a body is missing"))?;
    if link.body_desc != body_desc {
        apply_body(body, body_desc.as_ref());
    }
    let kind = body_desc.map_or(BodyKind::Fixed, |d| d.kind);
    let moved_by_others = match &link.written {
        Some((transform, _)) => world.get::<Transform>(entity) != Some(transform),
        None => false,
    };
    match kind {
        BodyKind::Dynamic => {
            if moved_by_others {
                body.set_position(pose(translation, rotation), true);
            }
            let velocity = world.get::<BodyVelocity>(entity).copied();
            let written = link.written.map(|(_, v)| v);
            if let Some(velocity) = velocity
                && Some(velocity) != written
            {
                set_velocity(body, velocity, body_desc.as_ref());
            }
            body.reset_forces(false);
            body.reset_torques(false);
            if let Some(force) = world.get::<BodyForce>(entity)
                && force.force.is_finite()
                && force.torque.is_finite()
            {
                body.add_force(rv(force.force), true);
                body.add_torque(rv(force.torque), true);
            }
        }
        BodyKind::Kinematic => {
            if !world.has::<CharacterController>(entity) {
                body.set_next_kinematic_position(pose(translation, rotation));
            } else if moved_by_others {
                body.set_position(pose(translation, rotation), true);
            }
        }
        BodyKind::Fixed => {
            let current = body.position();
            if wv(current.translation) != translation || wq(current.rotation) != rotation {
                body.set_position(pose(translation, rotation), true);
            }
        }
    }
    if link.collider_desc != collider_desc || link.scale != scale {
        let collider = colliders
            .get_mut(link.collider)
            .ok_or_else(|| internal("a collider is missing"))?;
        apply_collider(collider, &collider_desc, scale);
        // A sleeping body would not notice its new shape.
        if let Some(body) = bodies.get_mut(link.body) {
            body.wake_up(true);
        }
    }
    let skin = contact_skin(world, entity);
    if let Some(collider) = colliders.get_mut(link.collider)
        && (collider.contact_skin() - skin).abs() > f32::EPSILON
    {
        collider.set_contact_skin(skin);
    }
    if let Some(stored) = state.links.get_mut(&entity) {
        stored.body_desc = body_desc;
        stored.collider_desc = collider_desc;
        stored.scale = scale;
    }
    Ok(())
}

/// A character standing on a moving body last tick: the body, and the point
/// under its feet.
type Standing = (Entity, rp::RigidBodyHandle, Vec3);

/// Characters that stood on a moving (kinematic) body last tick, found
/// before the step: the query tree then still has every collider where it
/// was, so the ray meets the platform where the character stood on it, not
/// where it went (a fast or sinking platform would leave the ray's reach).
fn standing_on(
    world: &World,
    state: &PhysicsState,
    platforms: &[(rp::RigidBodyHandle, Mat4)],
) -> Vec<Standing> {
    let mut standing = Vec::new();
    if platforms.is_empty() {
        return standing;
    }
    for (entity, link) in &state.links {
        if link.body_desc.map(|d| d.kind) != Some(BodyKind::Kinematic)
            || !world.has::<CharacterController>(*entity)
            || !world
                .get::<CharacterState>(*entity)
                .is_some_and(|state| state.grounded)
        {
            continue;
        }
        let (Some(body), Some(collider)) = (
            state.bodies.get(link.body),
            state.colliders.get(link.collider),
        ) else {
            continue;
        };
        // The body is where the character was put last tick; the collider
        // follows it only at the next step.
        let at = *body.position() * collider.position_wrt_parent().copied().unwrap_or_default();
        let feet = -collider.shape().compute_local_aabb().mins.y;
        let filter = rp::QueryFilter::default()
            .exclude_rigid_body(link.body)
            .exclude_sensors();
        let queries = state.broad.as_query_pipeline(
            state.narrow.query_dispatcher(),
            &state.bodies,
            &state.colliders,
            filter,
        );
        let ray = rp::Ray::new(at.translation, rp::Vector::new(0.0, -1.0, 0.0));
        let Some((hit, distance)) = queries.cast_ray(&ray, feet + 0.2, true) else {
            continue;
        };
        let Some(platform) = state.colliders.get(hit).and_then(|c| c.parent()) else {
            continue;
        };
        if platforms.iter().any(|(handle, _)| *handle == platform) {
            let point = wv(at.translation) - Vec3::new(0.0, distance, 0.0);
            standing.push((*entity, platform, point));
        }
    }
    standing
}

fn move_characters(
    world: &mut World,
    state: &mut PhysicsState,
    delta: f32,
    platforms: &[(rp::RigidBodyHandle, Mat4)],
    standing: &[Standing],
) -> Result<()> {
    // Without characters there is nothing to carry, and kinematic bodies
    // are left alone (switching their type keeps them from sleeping).
    if !state
        .links
        .keys()
        .any(|entity| world.has::<CharacterController>(*entity))
    {
        return Ok(());
    }
    // Rapier's controller follows a kinematic body it stands on by that
    // body's velocity, and only partly: a character on an accelerating
    // platform falls behind and off (M7 friction log, F8). Platforms are
    // still while characters move, and each character is carried by exactly
    // how far the platform under it went; their speeds come back after
    // (dynamic bodies on them are carried by the solver as before).
    // (Rapier ignores speeds written to a position-driven kinematic body, so
    // it is driven by speed for the moment.)
    let mut speeds: Vec<(rp::RigidBodyHandle, rp::Vector, rp::AngVector)> = Vec::new();
    for (handle, _) in platforms {
        if let Some(body) = state.bodies.get_mut(*handle) {
            speeds.push((*handle, body.linvel(), body.angvel()));
            body.set_body_type(rp::RigidBodyType::KinematicVelocityBased, false);
            body.set_linvel(rp::Vector::ZERO, false);
            body.set_angvel(rp::AngVector::ZERO, false);
        }
    }
    let moved = move_each_character(world, state, delta, platforms, standing);
    for (handle, linvel, angvel) in speeds {
        if let Some(body) = state.bodies.get_mut(handle) {
            body.set_linvel(linvel, false);
            body.set_angvel(angvel, false);
            body.set_body_type(rp::RigidBodyType::KinematicPositionBased, false);
        }
    }
    moved
}

fn move_each_character(
    world: &mut World,
    state: &mut PhysicsState,
    delta: f32,
    platforms: &[(rp::RigidBodyHandle, Mat4)],
    standing: &[Standing],
) -> Result<()> {
    let characters: Vec<(Entity, CharacterController, Vec3)> = state
        .links
        .iter()
        .filter(|(_, link)| link.body_desc.map(|d| d.kind) == Some(BodyKind::Kinematic))
        .filter_map(|(entity, _)| {
            let controller = world.get::<CharacterController>(*entity).copied()?;
            let motion = world
                .get::<CharacterMotion>(*entity)
                .map_or(Vec3::ZERO, |m| m.velocity);
            Some((*entity, controller, motion))
        })
        .collect();
    for (entity, controller, velocity) in characters {
        let Some(link) = state.links.get(&entity).cloned() else {
            continue;
        };
        let desired = if velocity.is_finite() {
            velocity * delta
        } else {
            Vec3::ZERO
        };
        let (shape, position) = {
            let collider = state
                .colliders
                .get(link.collider)
                .ok_or_else(|| internal("a character's collider is missing"))?;
            (collider.shared_shape().clone(), *collider.position())
        };
        let kcc = KinematicCharacterController {
            up: rp::Vector::Y,
            offset: CharacterLength::Absolute(controller.skin.clamp(0.001, 0.5)),
            slide: true,
            autostep: (controller.step_height > 0.0).then(|| CharacterAutostep {
                max_height: CharacterLength::Absolute(controller.step_height.min(2.0)),
                // Far enough onto the step for a round bottom (a capsule) to
                // rest on it rather than slide back off the edge.
                min_width: CharacterLength::Relative(0.8),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: controller.max_slope_deg.clamp(0.0, 89.0).to_radians(),
            min_slope_slide_angle: controller.max_slope_deg.clamp(0.0, 89.0).to_radians(),
            snap_to_ground: (controller.snap_to_ground > 0.0)
                .then(|| CharacterLength::Absolute(controller.snap_to_ground.min(2.0))),
            ..KinematicCharacterController::default()
        };
        let filter = rp::QueryFilter::default()
            .exclude_rigid_body(link.body)
            .exclude_sensors();
        let queries = state.broad.as_query_pipeline(
            state.narrow.query_dispatcher(),
            &state.bodies,
            &state.colliders,
            filter,
        );
        // When it stood last tick on a body that moved this step (a moving
        // platform), it goes along by as much: the point under its feet,
        // moved as the platform moved (a turning platform turns it too).
        let carry = standing
            .iter()
            .find(|(standing, _, _)| *standing == entity)
            .and_then(|(_, platform, point)| {
                let (_, was) = platforms.iter().find(|(h, _)| h == platform)?;
                let now = state.bodies.get(*platform)?.position();
                let now = Mat4::from_rotation_translation(wq(now.rotation), wv(now.translation));
                let carried = now.transform_point3(was.inverse().transform_point3(*point)) - *point;
                if !carried.is_finite() || carried == Vec3::ZERO {
                    return None;
                }
                // Not into a wall or a ceiling the platform passes: the shape
                // goes along only as far as it is free to (the platform itself
                // is left out: it moved under the feet).
                let not_platform = |_: rp::ColliderHandle, collider: &rp::Collider| {
                    collider.parent() != Some(*platform)
                };
                let blockers = state.broad.as_query_pipeline(
                    state.narrow.query_dispatcher(),
                    &state.bodies,
                    &state.colliders,
                    rp::QueryFilter::default()
                        .exclude_rigid_body(link.body)
                        .exclude_sensors()
                        .predicate(&not_platform),
                );
                let options = rapier3d::parry::query::ShapeCastOptions {
                    max_time_of_impact: 1.0,
                    target_distance: 0.0,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: false,
                };
                let free = blockers
                    .cast_shape(&position, rv(carried), shape.as_ref(), options)
                    .map_or(1.0, |(_, hit)| hit.time_of_impact.clamp(0.0, 1.0));
                Some(carried * free)
            })
            .unwrap_or(Vec3::ZERO);
        // The character rides along rigidly (as if it were part of the
        // platform), then makes its own move from there.
        let position = rp::Pose::from_parts(position.translation + rv(carry), position.rotation);
        // Two moves: along the ground first (sliding, stepping up), then
        // up or down. One combined move with gravity in it presses the shape
        // into the floor and Rapier then refuses the sideways part.
        let mut blocked = false;
        let mut is_wall = |collision: rapier3d::control::CharacterCollision| {
            // A hit whose normal is not ground-like is a wall or ceiling.
            if collision.hit.normal1.y.abs() < 0.7 {
                blocked = true;
            }
        };
        let across = kcc.move_shape(
            delta,
            &queries,
            shape.as_ref(),
            &position,
            rv(Vec3::new(desired.x, 0.0, desired.z)),
            &mut is_wall,
        );
        let mut after = position;
        after.translation += across.translation;
        // While climbing a step the character is still over its edge: going
        // down in the same tick would slide it back off.
        let climbing = across.translation.y > 1e-4;
        let down = if climbing && desired.y < 0.0 {
            0.0
        } else {
            desired.y
        };
        let vertical = KinematicCharacterController {
            autostep: None,
            ..kcc
        };
        let along = vertical.move_shape(
            delta,
            &queries,
            shape.as_ref(),
            &after,
            rv(Vec3::new(0.0, down, 0.0)),
            |_| {},
        );
        let total = rv(carry) + across.translation + along.translation;
        let moved = wv(total);
        if let Some(body) = state.bodies.get_mut(link.body) {
            let current = *body.position();
            let next = rp::Pose::from_parts(current.translation + total, current.rotation);
            body.set_position(next, true);
        }
        // Going up (a jump) is never standing on the ground, however close the
        // ground still is.
        let grounded = down <= 0.0 && (along.grounded || (down == 0.0 && across.grounded));
        let outcome = CharacterState {
            grounded,
            blocked,
            moved,
        };
        if world.get::<CharacterState>(entity) != Some(&outcome) {
            world.insert(entity, outcome)?;
        }
    }
    Ok(())
}

fn write_back(world: &mut World, state: &mut PhysicsState, entity: Entity) -> Result<()> {
    let Some(link) = state.links.get(&entity).cloned() else {
        return Ok(());
    };
    let kind = link.body_desc.map_or(BodyKind::Fixed, |d| d.kind);
    if kind == BodyKind::Fixed {
        return Ok(());
    }
    let body = state
        .bodies
        .get(link.body)
        .ok_or_else(|| internal("a body is missing"))?;
    let position = body.position();
    let transform = local_for(
        world,
        entity,
        wv(position.translation),
        wq(position.rotation),
    );
    let velocity = BodyVelocity {
        linear: wv(body.linvel()),
        angular: wv(body.angvel()),
    };
    if world.get::<Transform>(entity) != Some(&transform) {
        world.insert(entity, transform)?;
    }
    if kind == BodyKind::Dynamic && world.get::<BodyVelocity>(entity) != Some(&velocity) {
        world.insert(entity, velocity)?;
    }
    let written_velocity = world
        .get::<BodyVelocity>(entity)
        .copied()
        .unwrap_or_default();
    if let Some(stored) = state.links.get_mut(&entity) {
        stored.written = Some((transform, written_velocity));
    }
    Ok(())
}
