//! Hashing whose output is stable across runs, platforms and releases.
//!
//! The standard library's hashers are randomly seeded and their algorithm may
//! change between Rust releases, so they must never be used for world hashes,
//! registry fingerprints or anything written to disk. Everything here is
//! XXH3 (128-bit, seed 0) with integers encoded explicitly as little-endian.

use core::fmt;
use core::str::FromStr;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::diagnostic::Diagnostic;

/// A 128-bit stable hash value. Its text form is 32 lowercase hex digits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash128(u128);

impl Hash128 {
    /// Wraps a raw 128-bit value.
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }

    /// The raw 128-bit value.
    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

impl fmt::Display for Hash128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl fmt::Debug for Hash128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash128({self})")
    }
}

impl FromStr for Hash128 {
    type Err = Diagnostic;

    /// Parses exactly 32 lowercase hex digits, the form [`Hash128`] displays as.
    fn from_str(text: &str) -> crate::Result<Self> {
        let canonical = text.len() == 32
            && text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        let parsed = if canonical {
            u128::from_str_radix(text, 16).ok()
        } else {
            None
        };
        parsed.map(Self).ok_or_else(|| {
            Diagnostic::error(
                "invalid_hash",
                format!("'{text}' is not a valid hash."),
                "A hash is exactly 32 lowercase hex digits, for example '99aa06d3014798d86001c324468d497f'.",
            )
        })
    }
}

/// Hashes a byte slice in one call.
pub fn hash_bytes(bytes: &[u8]) -> Hash128 {
    Hash128(xxh3_128(bytes))
}

/// A streaming hasher with a stable, documented output.
///
/// Feeding the same sequence of bytes always gives the same [`Hash128`],
/// however the bytes are split across calls to [`StableHasher::write_bytes`].
/// Use the length-prefixed methods for variable-length data so that adjacent
/// fields cannot run into each other.
#[derive(Clone)]
pub struct StableHasher {
    inner: Xxh3,
}

impl StableHasher {
    /// Creates a hasher in its initial state.
    pub fn new() -> Self {
        Self { inner: Xxh3::new() }
    }

    /// Feeds raw bytes, with no length prefix.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// Feeds one byte.
    pub fn write_u8(&mut self, value: u8) {
        self.write_bytes(&[value]);
    }

    /// Feeds a 32-bit integer as little-endian bytes.
    pub fn write_u32(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Feeds a 64-bit integer as little-endian bytes.
    pub fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Feeds a byte slice preceded by its length, so that `["ab", "c"]` and
    /// `["a", "bc"]` hash differently.
    pub fn write_len_prefixed(&mut self, bytes: &[u8]) {
        self.write_u64(bytes.len() as u64);
        self.write_bytes(bytes);
    }

    /// Feeds a string, length-prefixed, as UTF-8.
    pub fn write_str(&mut self, text: &str) {
        self.write_len_prefixed(text.as_bytes());
    }

    /// The hash of everything fed so far. The hasher can keep being used.
    pub fn finish(&self) -> Hash128 {
        Hash128(self.inner.digest128())
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StableHasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StableHasher").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published XXH3-128 value for empty input with seed 0. If this ever
    /// fails, the hash algorithm changed and every stored hash is invalid.
    #[test]
    fn empty_input_matches_the_published_xxh3_value() {
        let expected = "99aa06d3014798d86001c324468d497f";
        assert_eq!(hash_bytes(b"").to_string(), expected);
        assert_eq!(StableHasher::new().finish().to_string(), expected);
    }

    /// Values recorded from this implementation on 2026-11-15, covering each
    /// input-length code path of XXH3 (short, medium and long). They lock the
    /// output: if a dependency update changes any of them, stored world hashes
    /// and fingerprints would silently stop matching.
    #[test]
    fn recorded_values_never_change() {
        let long: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        let actual = [
            hash_bytes(b"wish engine"),
            hash_bytes(&long[..200]),
            hash_bytes(&long),
        ]
        .map(|hash| hash.to_string());
        assert_eq!(
            actual,
            [
                "fc182db33ce753b12683e1d204b00be9",
                "cab4b6c1d7f66c432a1a7281f2883dff",
                "17a0f7b2ab909d68404e1417800eb486",
            ]
        );
    }

    #[test]
    fn streaming_matches_one_shot_however_bytes_are_split() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        let expected = hash_bytes(&data);
        for chunk_size in [1, 3, 64, 257, 1000] {
            let mut hasher = StableHasher::new();
            for chunk in data.chunks(chunk_size) {
                hasher.write_bytes(chunk);
            }
            assert_eq!(hasher.finish(), expected, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn integers_are_fed_little_endian() {
        let mut by_integer = StableHasher::new();
        by_integer.write_u32(0x0403_0201);
        by_integer.write_u64(0x0c0b_0a09_0807_0605);
        by_integer.write_u8(0x0d);
        let by_bytes = hash_bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        assert_eq!(by_integer.finish(), by_bytes);
    }

    #[test]
    fn length_prefix_keeps_adjacent_strings_apart() {
        let mut first = StableHasher::new();
        first.write_str("ab");
        first.write_str("c");
        let mut second = StableHasher::new();
        second.write_str("a");
        second.write_str("bc");
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn finish_does_not_consume_the_hasher() {
        let mut hasher = StableHasher::new();
        hasher.write_str("wish");
        let before = hasher.finish();
        assert_eq!(hasher.finish(), before);
        hasher.write_str("engine");
        assert_ne!(hasher.finish(), before);
    }

    #[test]
    fn text_round_trip() {
        let hash = hash_bytes(b"wish engine");
        let text = hash.to_string();
        assert_eq!(text.len(), 32);
        assert_eq!(text.parse::<Hash128>().unwrap(), hash);
        assert_eq!(Hash128::from_u128(hash.as_u128()), hash);
        assert_eq!(Hash128::default().to_string(), "0".repeat(32));
    }

    #[test]
    fn rejects_invalid_text() {
        for text in [
            "",
            "99aa",
            "99AA06D3014798D86001C324468D497F",
            "+9aa06d3014798d86001c324468d497f",
            "zzaa06d3014798d86001c324468d497f",
        ] {
            let error = text.parse::<Hash128>().unwrap_err();
            assert_eq!(error.code, "invalid_hash", "input: {text:?}");
        }
    }
}
