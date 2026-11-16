//! Where entities are: local placement, and the world placement the
//! hierarchy turns it into.
//!
//! - [`Transform`] is what scenes and game code write: translation,
//!   rotation and scale relative to the parent.
//! - [`GlobalTransform`] is what the engine derives: the entity's matrix in
//!   world space. Only the engine's propagation (`wish_transform`) writes
//!   it, and scene files cannot.
//!
//! Conventions (ADR-021): right-handed, +Y up, a camera or light looks along
//! its -Z axis; meters and radians.

use wish_core::math::{Mat4, Quat, Vec3};
use wish_schema::component;

/// An entity's placement relative to its parent (or to the world, if it has
/// none).
#[component]
pub struct Transform {
    /// Position.
    #[schema(unit = "m", alias = "position")]
    pub translation: Vec3,
    /// Orientation as a unit quaternion `[x, y, z, w]`; the identity is
    /// `[0, 0, 0, 1]`.
    pub rotation: Quat,
    /// Scale along each local axis.
    #[schema(default = Vec3::ONE)]
    pub scale: Vec3,
}

impl Transform {
    /// A transform that only moves.
    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::default()
        }
    }

    /// The matrix for this transform. A rotation that is zero or not finite
    /// counts as no rotation, so a bad value never spreads NaNs through the
    /// hierarchy.
    pub fn to_matrix(&self) -> Mat4 {
        let rotation = if self.rotation.is_finite() && self.rotation.length_squared() > 1e-12 {
            self.rotation.normalize()
        } else {
            Quat::IDENTITY
        };
        Mat4::from_scale_rotation_translation(self.scale, rotation, self.translation)
    }
}

/// An entity's placement in world space, derived from its [`Transform`] and
/// those of its ancestors. Written by the engine; do not write it yourself.
#[component]
pub struct GlobalTransform {
    /// The matrix that takes the entity's local space to world space.
    pub matrix: Mat4,
}

/// The name of the [`GlobalTransform`] component, which scene files may not
/// write.
pub const GLOBAL_TRANSFORM: &str = "GlobalTransform";

/// The world position of a global transform.
pub fn world_position(global: &GlobalTransform) -> Vec3 {
    global.matrix.w_axis.truncate()
}
