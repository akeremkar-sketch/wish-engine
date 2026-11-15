//! The [`Plain`] contract and its implementations for built-in field types.

// This module declares an unsafe trait and implements it for built-in types.
// It contains no unsafe blocks: the trait is a promise about layout that the
// ECS relies on when it views stored bytes as typed references.
#![allow(unsafe_code)]

use wish_core::math::{Mat4, Quat, Vec2, Vec3, Vec4};
use wish_core::{Color, Entity, Text};

use crate::model::{TypeKind, TypeSchema};

/// Marks a type as plain data that can be stored in a world.
///
/// Implement it with `#[component]`, `#[resource]`, `#[event]` or
/// `#[schema]`, never by hand.
///
/// # Safety
///
/// The ECS views stored bytes as `&Self` and `&mut Self` without copying, so
/// an implementation promises all of the following:
///
/// - `Self` has a defined layout (`repr(C)`, `repr(transparent)`, a primitive,
///   or a fieldless `repr(u8)` enum) and contains no pointers, references or
///   anything that is not itself `Plain`.
/// - [`schema`](Plain::schema) reports the compiler's size, alignment and
///   field offsets exactly.
/// - [`encode`](Plain::encode) writes bytes that, placed at a suitably aligned
///   address, are a valid `Self`; padding bytes are written as zero.
/// - [`decode`](Plain::decode) returns `Some` exactly when the given bytes are
///   a valid `Self`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be stored in a world: it is not plain data",
    label = "not plain data",
    note = "fields of components, resources and events must be bool, an integer, f32 or f64, Vec2, Vec3, Vec4, Quat, Mat4, Color, Entity, Text, a fixed-size array of these, or a type marked #[schema]",
    note = "to hold text use `Text`; to refer to another entity use `Entity`"
)]
pub unsafe trait Plain: Copy + 'static {
    /// Describes the type's layout and meaning.
    fn schema() -> TypeSchema;

    /// The value a new instance starts with.
    fn default_value() -> Self;

    /// Writes the value's bytes in storage layout.
    ///
    /// `out` must be exactly `size_of::<Self>()` bytes long. Padding bytes
    /// are written as zero.
    fn encode(&self, out: &mut [u8]);

    /// Reads a value from bytes in storage layout.
    ///
    /// Returns `None` when `bytes` has the wrong length or does not hold a
    /// valid value (for example a `bool` byte other than 0 or 1).
    fn decode(bytes: &[u8]) -> Option<Self>;
}

/// Data attached to entities. Implemented by `#[component]`.
pub trait Component: Plain {
    /// The name the component is registered and written in JSON under.
    const NAME: &'static str;
}

/// A single value stored once per world. Implemented by `#[resource]`.
pub trait Resource: Plain {
    /// The name the resource is registered and written in JSON under.
    const NAME: &'static str;
}

/// A message sent between systems. Implemented by `#[event]`.
pub trait Event: Plain {
    /// The name the event is registered and written in JSON under.
    const NAME: &'static str;
}

/// Copies `source` into `out`. Encoders are always handed a slot of exactly
/// the right size; a mismatch is a caller bug and leaves `out` untouched.
fn put(out: &mut [u8], source: &[u8]) {
    debug_assert_eq!(out.len(), source.len(), "encode slot has the wrong size");
    if out.len() == source.len() {
        out.copy_from_slice(source);
    }
}

fn put_floats(out: &mut [u8], values: &[f32]) {
    debug_assert_eq!(
        out.len(),
        values.len() * 4,
        "encode slot has the wrong size"
    );
    for (slot, value) in out.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        *slot = value.to_le_bytes();
    }
}

fn get_floats<const N: usize>(bytes: &[u8]) -> Option<[f32; N]> {
    if bytes.len() != N * 4 {
        return None;
    }
    let mut values = [0.0; N];
    for (value, chunk) in values.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *value = f32::from_le_bytes(*chunk);
    }
    Some(values)
}

/// The stored bytes of an entity handle: index, then generation.
pub(crate) fn entity_to_image(entity: Entity) -> [u8; 8] {
    let mut image = [0u8; 8];
    let (index, generation) = image.split_at_mut(4);
    index.copy_from_slice(&entity.index().to_le_bytes());
    generation.copy_from_slice(&entity.generation().to_le_bytes());
    image
}

/// The entity handle stored as `image`.
pub(crate) fn entity_from_image(image: [u8; 8]) -> Entity {
    let [a, b, c, d, e, f, g, h] = image;
    Entity::new(
        u32::from_le_bytes([a, b, c, d]),
        u32::from_le_bytes([e, f, g, h]),
    )
}

fn mat4_from_columns(columns: [f32; 16]) -> Mat4 {
    Mat4::from_cols_array(&columns)
}

macro_rules! plain_numbers {
    ($($ty:ty => $kind:ident),* $(,)?) => {$(
        // SAFETY: a primitive number has no padding and every bit pattern is a
        // valid value. The schema's size and alignment are checked against the
        // compiler's by the `built_in_layouts_match_the_compiler` test.
        unsafe impl Plain for $ty {
            fn schema() -> TypeSchema {
                TypeSchema::primitive(stringify!($ty), TypeKind::$kind)
            }

            fn default_value() -> Self {
                <$ty>::default()
            }

            fn encode(&self, out: &mut [u8]) {
                put(out, &self.to_le_bytes());
            }

            fn decode(bytes: &[u8]) -> Option<Self> {
                Some(<$ty>::from_le_bytes(bytes.try_into().ok()?))
            }
        }
    )*};
}

plain_numbers!(
    i8 => I8, i16 => I16, i32 => I32, i64 => I64,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64,
    f32 => F32, f64 => F64,
);

macro_rules! plain_float_vectors {
    ($($ty:ty => $kind:ident, $count:literal, $to_array:ident, $from_array:expr);* $(;)?) => {$(
        // SAFETY: a `repr(C)` struct of `f32`s with alignment 4 and no padding
        // (SIMD is disabled in glam; see `wish_core::math`). Every bit pattern
        // is a valid value. The layout is checked against the compiler's by
        // the `built_in_layouts_match_the_compiler` test.
        unsafe impl Plain for $ty {
            fn schema() -> TypeSchema {
                TypeSchema::primitive(stringify!($ty), TypeKind::$kind)
            }

            fn default_value() -> Self {
                <$ty>::default()
            }

            fn encode(&self, out: &mut [u8]) {
                put_floats(out, &self.$to_array());
            }

            fn decode(bytes: &[u8]) -> Option<Self> {
                let values: [f32; $count] = get_floats(bytes)?;
                Some($from_array(values))
            }
        }
    )*};
}

plain_float_vectors!(
    Vec2 => Vec2, 2, to_array, Vec2::from_array;
    Vec3 => Vec3, 3, to_array, Vec3::from_array;
    Vec4 => Vec4, 4, to_array, Vec4::from_array;
    Quat => Quat, 4, to_array, Quat::from_array;
    Color => Color, 4, to_array, Color::from_array;
    Mat4 => Mat4, 16, to_cols_array, mat4_from_columns;
);

// SAFETY: `bool` is one byte with no padding. `encode` writes 0 or 1 and
// `decode` accepts only 0 and 1, the two valid values.
unsafe impl Plain for bool {
    fn schema() -> TypeSchema {
        TypeSchema::primitive("bool", TypeKind::Bool)
    }

    fn default_value() -> Self {
        false
    }

    fn encode(&self, out: &mut [u8]) {
        put(out, &[u8::from(*self)]);
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0] => Some(false),
            [1] => Some(true),
            _ => None,
        }
    }
}

// SAFETY: `Entity` is `repr(C)` with two `u32` fields, index then generation:
// 8 bytes, alignment 4, no padding, every bit pattern valid.
unsafe impl Plain for Entity {
    fn schema() -> TypeSchema {
        TypeSchema::primitive("Entity", TypeKind::Entity)
    }

    fn default_value() -> Self {
        Entity::NULL
    }

    fn encode(&self, out: &mut [u8]) {
        put(out, &entity_to_image(*self));
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        Some(entity_from_image(bytes.try_into().ok()?))
    }
}

// SAFETY: `Text` is `repr(transparent)` over a `u32`: 4 bytes, alignment 4,
// no padding, every bit pattern valid.
unsafe impl Plain for Text {
    fn schema() -> TypeSchema {
        TypeSchema::primitive("Text", TypeKind::Text)
    }

    fn default_value() -> Self {
        Text::EMPTY
    }

    fn encode(&self, out: &mut [u8]) {
        put(out, &self.index().to_le_bytes());
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        Some(Text::from_index(u32::from_le_bytes(bytes.try_into().ok()?)))
    }
}

// SAFETY: `[T; N]` is `N` values of `T` back to back, with `T`'s alignment and
// no padding beyond what `T` itself has. It is valid exactly when each
// element is, which `decode` checks element by element.
unsafe impl<T: Plain, const N: usize> Plain for [T; N] {
    fn schema() -> TypeSchema {
        TypeSchema::array(T::schema(), N, size_of::<Self>(), align_of::<Self>())
    }

    fn default_value() -> Self {
        [T::default_value(); N]
    }

    fn encode(&self, out: &mut [u8]) {
        let size = size_of::<T>();
        for (index, item) in self.iter().enumerate() {
            if let Some(slot) = out.get_mut(index * size..(index + 1) * size) {
                item.encode(slot);
            }
        }
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let size = size_of::<T>();
        if bytes.len() != size * N {
            return None;
        }
        let mut items = [T::default_value(); N];
        for (index, item) in items.iter_mut().enumerate() {
            *item = T::decode(bytes.get(index * size..(index + 1) * size)?)?;
        }
        Some(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_layout<T: Plain>() {
        let schema = T::schema();
        assert_eq!(schema.byte_len(), size_of::<T>(), "size of {}", schema.name);
        assert_eq!(
            schema.align as usize,
            align_of::<T>(),
            "alignment of {}",
            schema.name
        );
        assert!(
            schema.validate().is_ok(),
            "schema of {} is valid",
            schema.name
        );
    }

    #[test]
    fn built_in_layouts_match_the_compiler() {
        check_layout::<bool>();
        check_layout::<i8>();
        check_layout::<i16>();
        check_layout::<i32>();
        check_layout::<i64>();
        check_layout::<u8>();
        check_layout::<u16>();
        check_layout::<u32>();
        check_layout::<u64>();
        check_layout::<f32>();
        check_layout::<f64>();
        check_layout::<Vec2>();
        check_layout::<Vec3>();
        check_layout::<Vec4>();
        check_layout::<Quat>();
        check_layout::<Mat4>();
        check_layout::<Color>();
        check_layout::<Entity>();
        check_layout::<Text>();
        check_layout::<[Vec3; 5]>();
        check_layout::<[[u16; 3]; 2]>();
        check_layout::<[u64; 0]>();
    }

    fn round_trip<T: Plain + PartialEq + core::fmt::Debug>(value: T) {
        let mut bytes = vec![0xAAu8; size_of::<T>()];
        value.encode(&mut bytes);
        assert_eq!(T::decode(&bytes), Some(value));
    }

    #[test]
    fn encode_then_decode_gives_the_value_back() {
        round_trip(true);
        round_trip(-5i8);
        round_trip(-12345i16);
        round_trip(i32::MIN);
        round_trip(i64::MAX);
        round_trip(200u8);
        round_trip(u16::MAX);
        round_trip(u32::MAX);
        round_trip(u64::MAX);
        round_trip(-0.0f32);
        round_trip(f64::MIN_POSITIVE);
        round_trip(Vec2::new(1.0, -2.0));
        round_trip(Vec3::new(1.0, -2.0, 3.5));
        round_trip(Vec4::new(1.0, -2.0, 3.5, 4.25));
        round_trip(Quat::from_xyzw(0.1, 0.2, 0.3, 0.9));
        round_trip(Mat4::from_cols_array(&core::array::from_fn(|index| {
            index as f32
        })));
        round_trip(Color::rgba(0.1, 0.2, 0.3, 0.4));
        round_trip(Entity::new(12, 3));
        round_trip(Entity::NULL);
        round_trip(Text::from_index(7));
        round_trip([Vec3::X, Vec3::Y]);
        round_trip([[1u16, 2, 3], [4, 5, 6]]);
    }

    #[test]
    fn bytes_are_little_endian_in_field_order() {
        let mut bytes = [0u8; 8];
        Entity::new(0x0403_0201, 0x0807_0605).encode(&mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8]);

        let mut bytes = [0u8; 12];
        Vec3::new(1.0, 2.0, 3.0).encode(&mut bytes);
        assert_eq!(&bytes[..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[8..], &3.0f32.to_le_bytes());
    }

    #[test]
    fn decode_rejects_wrong_lengths_and_invalid_bools() {
        assert_eq!(bool::decode(&[2]), None);
        assert_eq!(bool::decode(&[]), None);
        assert_eq!(bool::decode(&[0, 0]), None);
        assert_eq!(u32::decode(&[1, 2, 3]), None);
        assert_eq!(Vec3::decode(&[0; 11]), None);
        assert_eq!(Entity::decode(&[0; 9]), None);
        assert_eq!(<[bool; 2]>::decode(&[1, 2]), None);
        assert_eq!(<[bool; 2]>::decode(&[1, 0]), Some([true, false]));
    }

    #[test]
    fn defaults_are_the_expected_values() {
        assert_eq!(Quat::default_value(), Quat::IDENTITY);
        assert_eq!(Mat4::default_value(), Mat4::IDENTITY);
        assert_eq!(Color::default_value(), Color::WHITE);
        assert_eq!(Entity::default_value(), Entity::NULL);
        assert_eq!(Text::default_value(), Text::EMPTY);
        assert_eq!(<[u8; 40]>::default_value(), [0u8; 40]);
    }
}
