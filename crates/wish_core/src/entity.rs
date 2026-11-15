//! Entity identifiers.

use core::fmt;
use core::str::FromStr;

use crate::diagnostic::Diagnostic;

/// A handle to an entity in a world.
///
/// An entity is an index into the world's entity table plus a generation
/// counter. When an entity is despawned its index may be reused, but the
/// generation is bumped, so a stale handle never refers to the new entity.
///
/// `Entity` is plain data (`repr(C)`, 8 bytes, alignment 4) and may be stored
/// in components. Its text form is `"<index>v<generation>"`, for example
/// `"12v3"`; the null entity is written `"null"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// The null entity: refers to nothing. Worlds never allocate it.
    pub const NULL: Self = Self {
        index: u32::MAX,
        generation: 0,
    };

    /// Creates an entity handle from its index and generation.
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// The index into the world's entity table.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation of the index this handle was created for.
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Whether this is [`Entity::NULL`].
    pub const fn is_null(self) -> bool {
        self.index == Self::NULL.index && self.generation == Self::NULL.generation
    }

    /// Packs the handle into 64 bits: generation in the high half, index in the low half.
    pub const fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    /// Unpacks a handle produced by [`Entity::to_bits`].
    pub const fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

impl Default for Entity {
    /// The default entity is [`Entity::NULL`].
    fn default() -> Self {
        Self::NULL
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            f.write_str("null")
        } else {
            write!(f, "{}v{}", self.index, self.generation)
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({self})")
    }
}

impl FromStr for Entity {
    type Err = Diagnostic;

    /// Parses the canonical text form. Non-canonical spellings such as
    /// `"+12v3"` or `"012v3"` are rejected so that text round-trips exactly.
    fn from_str(text: &str) -> crate::Result<Self> {
        let parsed = if text == "null" {
            Some(Self::NULL)
        } else {
            text.split_once('v').and_then(|(index, generation)| {
                Some(Self::new(index.parse().ok()?, generation.parse().ok()?))
            })
        };
        parsed
            .filter(|entity| entity.to_string() == text)
            .ok_or_else(|| {
                Diagnostic::error(
                    "invalid_entity_id",
                    format!("'{text}' is not a valid entity id."),
                    "Entity ids look like '12v3' (index 12, generation 3). The null entity is written 'null'.",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_plain_eight_bytes() {
        assert_eq!(size_of::<Entity>(), 8);
        assert_eq!(align_of::<Entity>(), 4);
    }

    #[test]
    fn bits_round_trip() {
        for entity in [
            Entity::new(0, 0),
            Entity::new(12, 3),
            Entity::new(u32::MAX - 1, u32::MAX),
            Entity::NULL,
        ] {
            assert_eq!(Entity::from_bits(entity.to_bits()), entity);
        }
        assert_eq!(Entity::new(12, 3).to_bits(), (3u64 << 32) | 12);
    }

    #[test]
    fn text_round_trip() {
        for entity in [Entity::new(0, 0), Entity::new(12, 3), Entity::NULL] {
            let text = entity.to_string();
            assert_eq!(text.parse::<Entity>().unwrap(), entity);
        }
        assert_eq!(Entity::new(12, 3).to_string(), "12v3");
        assert_eq!(Entity::NULL.to_string(), "null");
        assert_eq!(format!("{:?}", Entity::new(12, 3)), "Entity(12v3)");
    }

    #[test]
    fn rejects_invalid_and_non_canonical_text() {
        for text in [
            "", "12", "v3", "12v", "a12v3", "12v3x", "+12v3", "012v3", "12V3", "NULL", " 12v3",
        ] {
            let error = text.parse::<Entity>().unwrap_err();
            assert_eq!(error.code, "invalid_entity_id", "input: {text:?}");
            assert!(!error.hint.is_empty());
        }
    }

    #[test]
    fn null_is_only_the_exact_null_value() {
        assert!(Entity::NULL.is_null());
        assert!(Entity::default().is_null());
        assert!(!Entity::new(u32::MAX, 1).is_null());
        assert!(!Entity::new(0, 0).is_null());
    }

    #[test]
    fn ordering_is_by_index_then_generation() {
        assert!(Entity::new(1, 9) < Entity::new(2, 0));
        assert!(Entity::new(2, 0) < Entity::new(2, 1));
    }
}
