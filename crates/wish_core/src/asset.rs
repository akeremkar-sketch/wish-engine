//! References to asset files.

use core::fmt;

use crate::Text;

/// The longest asset path, in bytes.
pub const MAX_ASSET_PATH_LEN: usize = 512;

/// A reference to an asset file: a project-relative path with `/` as the
/// separator and an optional sub-object after `#`, such as
/// `"assets/models/crate.glb#Mesh0"`.
///
/// Like [`Text`], it is a handle into a world's text table (4 bytes,
/// alignment 4); the empty path means "no asset". Unlike plain text, the
/// schema knows it is a file reference, so tools can check that the file
/// exists, offer a picker and update the path when a file moves.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct AssetRef(Text);

impl AssetRef {
    /// No asset. Valid in every world.
    pub const NONE: Self = Self(Text::EMPTY);

    /// Wraps the text handle that holds the path.
    pub const fn from_text(text: Text) -> Self {
        Self(text)
    }

    /// The text handle that holds the path.
    pub const fn text(self) -> Text {
        self.0
    }

    /// Whether this is [`AssetRef::NONE`].
    pub const fn is_none(self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for AssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AssetRef({})", self.0.index())
    }
}

/// Splits an asset reference into its file path and sub-object.
pub fn split_asset_ref(reference: &str) -> (&str, Option<&str>) {
    match reference.split_once('#') {
        Some((path, sub)) => (path, Some(sub)),
        None => (reference, None),
    }
}

/// Checks that `reference` is well formed: empty (no asset), or a relative
/// path of non-empty segments separated by `/`, with no `.` or `..` segment,
/// no backslash, drive letter or control character, and at most one `#` that
/// is followed by a non-empty sub-object name.
///
/// Returns the problem as a short phrase for a message. Whether the file
/// exists is a separate question (`wish_asset`).
pub fn validate_asset_ref(reference: &str) -> Result<(), &'static str> {
    if reference.is_empty() {
        return Ok(());
    }
    if reference.len() > MAX_ASSET_PATH_LEN {
        return Err("it is longer than 512 bytes");
    }
    if reference.chars().any(char::is_control) {
        return Err("it contains a control character");
    }
    let (path, sub) = split_asset_ref(reference);
    if sub.is_some_and(|sub| sub.is_empty() || sub.contains('#')) {
        return Err("the part after '#' must be one non-empty name");
    }
    if path.contains('\\') {
        return Err("it uses '\\' (write '/' between folders)");
    }
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return Err("it is absolute (write it relative to the project folder)");
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err("it has an empty folder name (a leading, trailing or doubled '/')"),
            "." | ".." => return Err("it contains '.' or '..'"),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_one_u32_and_none_is_the_default() {
        assert_eq!((size_of::<AssetRef>(), align_of::<AssetRef>()), (4, 4));
        assert!(AssetRef::default().is_none());
    }

    #[test]
    fn good_references_pass() {
        for good in ["", "a.png", "assets/models/crate.glb#Mesh0", "a b/c.txt"] {
            assert_eq!(validate_asset_ref(good), Ok(()), "{good}");
        }
    }

    #[test]
    fn bad_references_say_why() {
        let cases = [
            ("/abs/x.png", "absolute"),
            ("C:/x.png", "absolute"),
            ("a\\b.png", "'\\'"),
            ("a/../b.png", "'..'"),
            ("./a.png", "'.'"),
            ("a//b.png", "empty folder"),
            ("a/", "empty folder"),
            ("a.glb#", "non-empty"),
            ("a.glb#x#y", "non-empty"),
            ("a\u{0}b", "control"),
        ];
        for (bad, word) in cases {
            let problem = validate_asset_ref(bad).unwrap_err();
            assert!(problem.contains(word), "{bad:?}: {problem}");
        }
        assert!(validate_asset_ref(&"a".repeat(513)).is_err());
    }
}
