//! Handles to text stored in a world.

use core::fmt;

/// A handle to a string in a world's text table.
///
/// Components are plain data and cannot own a `String`, so they hold text by
/// handle. A handle only has meaning together with the world it came from:
/// the world interns the string and gives back a `Text`, and resolves a
/// `Text` back to the string.
///
/// Plain data: 4 bytes, alignment 4. [`Text::EMPTY`] is the empty string in
/// every world, and is the default.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Text(u32);

impl Text {
    /// The empty string. Valid in every world.
    pub const EMPTY: Self = Self(0);

    /// Creates a handle from its index in a text table.
    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// The index in the text table.
    pub const fn index(self) -> u32 {
        self.0
    }

    /// Whether this is [`Text::EMPTY`].
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Text({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_one_u32() {
        assert_eq!((size_of::<Text>(), align_of::<Text>()), (4, 4));
    }

    #[test]
    fn empty_is_index_zero_and_the_default() {
        assert_eq!(Text::EMPTY.index(), 0);
        assert!(Text::default().is_empty());
        assert!(!Text::from_index(3).is_empty());
        assert_eq!(format!("{:?}", Text::from_index(3)), "Text(3)");
    }
}
