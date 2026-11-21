//! Room codes: what a player types to join a match.

use std::fmt;

use wish_core::{Diagnostic, Result};

use crate::crypto::Entropy;

/// The letters and digits a room code is made of: no `I`, `O`, `0` or `1`,
/// which are easy to mix up when read aloud or from a screen. 32 symbols, so
/// every symbol carries five bits.
pub const ROOM_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// The shortest room code.
pub const ROOM_CODE_MIN: usize = 4;
/// The longest room code.
pub const ROOM_CODE_MAX: usize = 12;
/// The length of a generated room code (30 bits).
pub const ROOM_CODE_DEFAULT: usize = 6;

/// A room code: 4 to 12 symbols of [`ROOM_ALPHABET`], kept in capitals.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoomCode(String);

impl RoomCode {
    /// Reads a room code as a person typed it: spaces and dashes are ignored
    /// and small letters count as capitals.
    pub fn parse(text: &str) -> Result<Self> {
        let mut code = String::new();
        for c in text.chars() {
            if c == ' ' || c == '-' {
                continue;
            }
            let upper = c.to_ascii_uppercase();
            if !upper.is_ascii() || !ROOM_ALPHABET.contains(&(upper as u8)) {
                return Err(invalid(
                    text,
                    format!("the room code has the character {c:?}, which room codes do not use"),
                ));
            }
            code.push(upper);
        }
        if !(ROOM_CODE_MIN..=ROOM_CODE_MAX).contains(&code.len()) {
            return Err(invalid(
                text,
                format!(
                    "a room code has {ROOM_CODE_MIN} to {ROOM_CODE_MAX} letters and digits, this one has {}",
                    code.len()
                ),
            ));
        }
        Ok(Self(code))
    }

    /// A new random room code of [`ROOM_CODE_DEFAULT`] symbols.
    pub fn generate(entropy: &mut Entropy) -> Self {
        let bytes: [u8; ROOM_CODE_DEFAULT] = entropy.bytes();
        Self(
            bytes
                .iter()
                .map(|b| char::from(ROOM_ALPHABET[usize::from(b & 31)]))
                .collect(),
        )
    }

    /// The code in capitals, without separators.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid(text: &str, message: String) -> Diagnostic {
    Diagnostic::error(
        "net_room_code_invalid",
        message,
        format!(
            "Type the code the host shows: {ROOM_CODE_MIN} to {ROOM_CODE_MAX} of the letters and digits {} (there is no I, O, 0 or 1).",
            String::from_utf8_lossy(ROOM_ALPHABET)
        ),
    )
    .at_path(format!("/room_code ({text:?})"))
}

impl fmt::Display for RoomCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_read_as_people_type_them() {
        assert_eq!(RoomCode::parse("abc-23 x").unwrap().as_str(), "ABC23X");
        assert!(RoomCode::parse("ABO1").is_err());
        assert!(RoomCode::parse("ABC").is_err());
        assert!(RoomCode::parse("ABCDEFGHJKLMN").is_err());
        assert!(RoomCode::parse("ÄBCD").is_err());
        let error = RoomCode::parse("AB").unwrap_err();
        assert_eq!(error.code, "net_room_code_invalid");
    }

    #[test]
    fn generated_codes_parse_and_differ() {
        let mut entropy = Entropy::seeded(5);
        let a = RoomCode::generate(&mut entropy);
        let b = RoomCode::generate(&mut entropy);
        assert_ne!(a, b);
        assert_eq!(RoomCode::parse(a.as_str()).unwrap(), a);
        assert_eq!(a.as_str().len(), ROOM_CODE_DEFAULT);
    }
}
