//! Runtime type schemas.
//!
//! Everything that enters a world (components, resources, events) is *plain
//! data* described by a [`TypeSchema`]. One definition drives everything else:
//!
//! - the storage layout (size, alignment, field offsets),
//! - JSON reading and writing ([`to_json`], [`apply_json`], [`from_json`]),
//! - the JSON Schema published to AI agents and editors ([`json_schema()`]),
//! - defaults, units and ranges shown in inspectors,
//! - the fingerprint that ties a snapshot to the types it was taken with.
//!
//! Types are declared with the attribute macros and nothing else:
//!
//! ```
//! use wish_schema::{component, Plain};
//!
//! /// Moves an entity at a constant speed.
//! #[component]
//! pub struct Mover {
//!     /// Movement speed.
//!     #[schema(unit = "m/s", range = 0.0..=50.0, default = 5.0)]
//!     pub speed: f32,
//! }
//!
//! assert_eq!(Mover::default().speed, 5.0);
//! assert_eq!(Mover::schema().size, 4);
//! ```
//!
//! See `docs/design/M1-cekirdek.md` for why storage is the type's `repr(C)`
//! layout and what the [`Plain`] contract guarantees.

// Macro-generated code refers to this crate as `::wish_schema`, including from
// this crate's own tests.
extern crate self as wish_schema;

#[cfg(not(target_endian = "little"))]
compile_error!(
    "Wish Engine stores component data in little-endian byte order and supports little-endian targets only."
);

pub mod fingerprint;
pub mod json;
pub mod json_schema;
pub mod model;
pub mod plain;
pub mod wire;

pub use fingerprint::{fingerprint, write_fingerprint};
pub use json::{
    JsonContext, NoTexts, TextStore, apply_json, default_bytes, from_json, to_json, validate_bytes,
};
pub use json_schema::json_schema;
pub use model::{
    FieldSchema, MAX_DEPTH, MAX_NODES, MAX_VALUE_SIZE, NumberRange, TypeKind, TypeSchema,
    VariantSchema,
};
pub use plain::{Component, Event, Plain, Resource};
pub use wish_schema_macros::{component, event, resource, schema};

/// Support code for the attribute macros. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    use crate::Plain;

    /// Clears a value's byte image so that padding is written as zero.
    pub fn encode_begin(out: &mut [u8]) {
        out.fill(0);
    }

    /// Writes one field into its slot of the containing value's byte image.
    pub fn encode_field<T: Plain>(out: &mut [u8], offset: usize, value: &T) {
        if let Some(slot) = out.get_mut(offset..offset + size_of::<T>()) {
            value.encode(slot);
        }
    }

    /// Reads one field from the containing value's byte image.
    pub fn decode_field<T: Plain>(bytes: &[u8], offset: usize) -> Option<T> {
        T::decode(bytes.get(offset..offset + size_of::<T>())?)
    }
}
