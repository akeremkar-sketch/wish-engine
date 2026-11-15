//! The world's text table.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Result, Text};
use wish_schema::TextStore;

/// Strings that components refer to through [`Text`] handles.
///
/// Each distinct string is stored once. Handles are indices in insertion
/// order and are never reused, so a handle stays valid for the life of the
/// world. Index 0 is always the empty string ([`Text::EMPTY`]).
#[derive(Clone, Debug)]
pub struct TextTable {
    strings: Vec<String>,
    lookup: BTreeMap<String, u32>,
}

impl Default for TextTable {
    fn default() -> Self {
        Self {
            strings: vec![String::new()],
            lookup: BTreeMap::from([(String::new(), 0)]),
        }
    }
}

impl TextTable {
    /// The number of distinct strings, the empty string included.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the table holds only the empty string.
    pub fn is_empty(&self) -> bool {
        self.strings.len() == 1
    }

    /// The string a handle refers to.
    pub fn get(&self, text: Text) -> Option<&str> {
        self.strings.get(text.index() as usize).map(String::as_str)
    }

    /// The handle for `text`, adding it if it is new.
    pub fn intern(&mut self, text: &str) -> Result<Text> {
        if let Some(index) = self.lookup.get(text) {
            return Ok(Text::from_index(*index));
        }
        let index = u32::try_from(self.strings.len()).map_err(|_| {
            Diagnostic::error(
                "too_many_texts",
                "The world's text table is full.",
                "A world holds at most about four billion distinct strings.",
            )
        })?;
        self.strings.push(text.to_owned());
        self.lookup.insert(text.to_owned(), index);
        Ok(Text::from_index(index))
    }

    /// Every string with its handle, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (Text, &str)> {
        (0u32..)
            .zip(&self.strings)
            .map(|(index, text)| (Text::from_index(index), text.as_str()))
    }
}

impl TextStore for TextTable {
    fn intern(&mut self, text: &str) -> Result<Text> {
        TextTable::intern(self, text)
    }

    fn resolve(&self, text: Text) -> Option<&str> {
        self.get(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_stored_once_and_handles_are_stable() {
        let mut texts = TextTable::default();
        assert!(texts.is_empty());
        assert_eq!(texts.intern("").unwrap(), Text::EMPTY);
        let hero = texts.intern("hero").unwrap();
        let villain = texts.intern("villain").unwrap();
        assert_eq!(texts.intern("hero").unwrap(), hero);
        assert_eq!((hero.index(), villain.index()), (1, 2));
        assert_eq!(texts.get(hero), Some("hero"));
        assert_eq!(texts.get(Text::from_index(9)), None);
        assert_eq!(texts.len(), 3);
        assert_eq!(
            texts.iter().map(|(_, text)| text).collect::<Vec<_>>(),
            ["", "hero", "villain"]
        );
    }
}
