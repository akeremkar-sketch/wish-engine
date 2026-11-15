//! Layout fingerprints.
//!
//! A fingerprint is a stable hash of everything that decides how a type's
//! bytes are interpreted: names, sizes, alignments, field offsets and enum
//! values. A snapshot records the fingerprint of the types it was taken with
//! and refuses to load into a world whose types differ, because the same
//! bytes would mean something else there.
//!
//! Documentation, units, ranges, defaults and aliases are deliberately left
//! out: changing them does not change what stored bytes mean.

use wish_core::{Hash128, StableHasher};

use crate::model::{TypeKind, TypeSchema};

/// A number for each kind. These are part of the fingerprint format: never
/// renumber them, only add new ones.
fn kind_tag(kind: &TypeKind) -> u8 {
    match kind {
        TypeKind::Bool => 1,
        TypeKind::I8 => 2,
        TypeKind::I16 => 3,
        TypeKind::I32 => 4,
        TypeKind::I64 => 5,
        TypeKind::U8 => 6,
        TypeKind::U16 => 7,
        TypeKind::U32 => 8,
        TypeKind::U64 => 9,
        TypeKind::F32 => 10,
        TypeKind::F64 => 11,
        TypeKind::Vec2 => 12,
        TypeKind::Vec3 => 13,
        TypeKind::Vec4 => 14,
        TypeKind::Quat => 15,
        TypeKind::Mat4 => 16,
        TypeKind::Color => 17,
        TypeKind::Entity => 18,
        TypeKind::Text => 19,
        TypeKind::Enum(_) => 20,
        TypeKind::Array { .. } => 21,
        TypeKind::Struct(_) => 22,
    }
}

fn write_len(hasher: &mut StableHasher, len: usize) {
    hasher.write_u64(len as u64);
}

/// Feeds a type's layout into a hasher.
pub fn write_fingerprint(schema: &TypeSchema, hasher: &mut StableHasher) {
    hasher.write_str(&schema.name);
    hasher.write_u32(schema.size);
    hasher.write_u32(schema.align);
    hasher.write_u8(kind_tag(&schema.kind));
    match &schema.kind {
        TypeKind::Enum(variants) => {
            write_len(hasher, variants.len());
            for variant in variants {
                hasher.write_str(&variant.name);
                hasher.write_u8(variant.value);
            }
        }
        TypeKind::Array { element, len } => {
            hasher.write_u32(*len);
            write_fingerprint(element, hasher);
        }
        TypeKind::Struct(fields) => {
            write_len(hasher, fields.len());
            for field in fields {
                hasher.write_str(&field.name);
                hasher.write_u32(field.offset);
                write_fingerprint(&field.ty, hasher);
            }
        }
        _ => {}
    }
}

/// The fingerprint of a single type's layout.
pub fn fingerprint(schema: &TypeSchema) -> Hash128 {
    let mut hasher = StableHasher::new();
    write_fingerprint(schema, &mut hasher);
    hasher.finish()
}
