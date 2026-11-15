//! Vector, quaternion and matrix types.
//!
//! These are [`glam`]'s `f32` types, built with SIMD turned off so that every
//! one of them is a plain `repr(C)` struct of `f32`s with alignment 4:
//!
//! | Type | Size | Alignment |
//! |---|---|---|
//! | [`Vec2`] | 8 | 4 |
//! | [`Vec3`] | 12 | 4 |
//! | [`Vec4`] | 16 | 4 |
//! | [`Quat`] | 16 | 4 |
//! | [`Mat4`] | 64 | 4 |
//!
//! Components store these directly, so their layout is part of the engine's
//! data format. The tests below fail if a dependency change ever alters it.
//!
//! Conventions (ADR-021): right-handed, +Y up, cameras look along -Z; meters,
//! seconds, kilograms; angles in radians.

pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4, camera};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_plain_f32_structs() {
        assert_eq!((size_of::<Vec2>(), align_of::<Vec2>()), (8, 4));
        assert_eq!((size_of::<Vec3>(), align_of::<Vec3>()), (12, 4));
        assert_eq!((size_of::<Vec4>(), align_of::<Vec4>()), (16, 4));
        assert_eq!((size_of::<Quat>(), align_of::<Quat>()), (16, 4));
        assert_eq!((size_of::<Mat4>(), align_of::<Mat4>()), (64, 4));
    }

    #[test]
    fn array_forms_are_in_declaration_order() {
        assert_eq!(Vec3::new(1.0, 2.0, 3.0).to_array(), [1.0, 2.0, 3.0]);
        assert_eq!(
            Quat::from_xyzw(1.0, 2.0, 3.0, 4.0).to_array(),
            [1.0, 2.0, 3.0, 4.0]
        );
        assert_eq!(Quat::IDENTITY.to_array(), [0.0, 0.0, 0.0, 1.0]);
        let columns = Mat4::IDENTITY.to_cols_array();
        assert_eq!(columns[0], 1.0);
        assert_eq!(columns[5], 1.0);
        assert_eq!(columns[1], 0.0);
    }

    #[test]
    fn defaults_are_zero_vectors_and_identity_rotations() {
        assert_eq!(Vec3::default(), Vec3::ZERO);
        assert_eq!(Quat::default(), Quat::IDENTITY);
        assert_eq!(Mat4::default(), Mat4::IDENTITY);
    }
}
