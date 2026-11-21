//! The 2D profiles (design note 1.1-physics-2d): the pinned 3D Rapier held
//! to the X–Y plane, so that 2D and 3D share one backend, one determinism
//! proof and one set of queries.
//!
//! In a 2D profile:
//! - every moving body is locked to the plane (no movement along Z, turning
//!   only about Z), whatever its own `RigidBody` locks say;
//! - every body is placed at z = 0 in Rapier, with only the part of its
//!   rotation that turns about Z (its *twist*); the entity's own z and any
//!   tilt are kept when physics writes it back, so sprites at different
//!   depths still collide;
//! - shapes are their outline in the plane, 1 m deep, and weigh their
//!   density × area × 1 m (a ball would otherwise weigh as a sphere);
//! - character moves and queries are flattened into the plane.

use rapier3d::prelude as rp;
use wish_core::math::{Quat, Vec3};
pub use wish_types::physics::PhysicsProfile;

use crate::{Collider, ColliderShape, RigidBody, dim};

/// Half the depth of a 2D shape: shapes are 1 m deep, so that a 1 m box
/// weighs the same in 2D and 3D.
pub(crate) const HALF_DEPTH: f32 = 0.5;

/// Whether `profile` is one of the 2D ones.
pub(crate) fn is_2d(profile: PhysicsProfile) -> bool {
    profile != PhysicsProfile::Full3D
}

/// The part of `rotation` that turns about Z (identity when it has none,
/// such as a quarter turn about X).
pub(crate) fn twist(rotation: Quat) -> Quat {
    if rotation.x == 0.0 && rotation.y == 0.0 {
        return rotation;
    }
    let twist = Quat::from_xyzw(0.0, 0.0, rotation.z, rotation.w);
    if twist.length_squared() > 1e-12 {
        twist.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// Where Rapier keeps an entity placed at `translation`/`rotation`: as it
/// is in 3D; in 2D on the plane z = 0 and turned only about Z.
pub(crate) fn flatten(profile: PhysicsProfile, translation: Vec3, rotation: Quat) -> (Vec3, Quat) {
    if is_2d(profile) {
        (translation.with_z(0.0), twist(rotation))
    } else {
        (translation, rotation)
    }
}

/// The world placement of an entity that Rapier has at
/// `translation`/`rotation`, given where the entity was before (`was`): in
/// 2D its own z and its tilt are kept.
pub(crate) fn unflatten(
    profile: PhysicsProfile,
    translation: Vec3,
    rotation: Quat,
    was: (Vec3, Quat),
) -> (Vec3, Quat) {
    if !is_2d(profile) {
        return (translation, rotation);
    }
    let (was_translation, was_rotation) = was;
    let rotation = if was_rotation.x == 0.0 && was_rotation.y == 0.0 {
        rotation
    } else {
        // The tilt is what is left of the old rotation once its twist is
        // taken out; it goes on under the new twist.
        let tilt = twist(was_rotation).inverse() * was_rotation;
        (rotation * tilt).normalize()
    };
    (translation.with_z(was_translation.z), rotation)
}

/// The body settings physics uses: in 2D, locked to the plane.
pub(crate) fn effective(profile: PhysicsProfile, desc: Option<RigidBody>) -> Option<RigidBody> {
    let mut desc = desc?;
    if is_2d(profile) {
        desc.lock_translation[2] = true;
        desc.lock_rotation[0] = true;
        desc.lock_rotation[1] = true;
    }
    Some(desc)
}

/// The gravity of a step.
pub(crate) fn gravity(profile: PhysicsProfile, gravity: Vec3) -> Vec3 {
    match profile {
        PhysicsProfile::Full3D => gravity,
        PhysicsProfile::SideView2D => gravity.with_z(0.0),
        PhysicsProfile::TopDown2D => Vec3::ZERO,
    }
}

/// A 2D shape: the outline of `desc` (scaled) in the plane, 1 m deep.
pub(crate) fn shape(desc: &Collider, scale: Vec3) -> rp::SharedShape {
    let size = desc.size * scale.abs();
    let (w, h) = (dim(size.x), dim(size.y));
    match desc.shape {
        ColliderShape::Box | ColliderShape::Cylinder => {
            rp::SharedShape::cuboid(w / 2.0, h / 2.0, HALF_DEPTH)
        }
        ColliderShape::Sphere => rp::SharedShape::ball(w.max(h) / 2.0),
        ColliderShape::Capsule => {
            let radius = w / 2.0;
            rp::SharedShape::capsule_y((h / 2.0 - radius).max(0.0), radius)
        }
    }
}

/// What a 2D shape weighs and how it turns: density × area × 1 m, with the
/// moment of inertia of the flat outline about Z.
pub(crate) fn mass(desc: &Collider, scale: Vec3) -> rp::MassProperties {
    let density = desc.density.clamp(0.001, 100_000.0) * (2.0 * HALF_DEPTH);
    let size = desc.size * scale.abs();
    let (w, h) = (dim(size.x), dim(size.y));
    let (mass, inertia) = match desc.shape {
        ColliderShape::Box | ColliderShape::Cylinder => {
            let mass = density * w * h;
            (mass, mass * (w * w + h * h) / 12.0)
        }
        ColliderShape::Sphere => {
            let r = w.max(h) / 2.0;
            let mass = density * std::f32::consts::PI * r * r;
            (mass, mass * r * r / 2.0)
        }
        ColliderShape::Capsule => {
            // A rectangle between two half discs.
            let r = w / 2.0;
            let length = (h - w).max(0.0);
            let rectangle = density * length * w;
            let disc = density * std::f32::consts::PI * r * r;
            let d = 4.0 * r / (3.0 * std::f32::consts::PI);
            let inertia = rectangle * (length * length + w * w) / 12.0
                + disc * (r * r / 2.0 + length * length / 4.0 + length * d);
            (rectangle + disc, inertia)
        }
    };
    rp::MassProperties::new(rp::Vector::ZERO, mass, rp::Vector::splat(inertia))
}
