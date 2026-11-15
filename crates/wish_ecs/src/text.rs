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

    /// The handle of `text`, if the table already holds it.
    pub fn find(&self, text: &str) -> Option<Text> {
        self.lookup.get(text).map(|index| Text::from_index(*index))
    }

    /// Every string with its handle, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (Text, &str)> {
        (0u32..)
            .zip(&self.strings)
            .map(|(index, text)| (Text::from_index(index), text.as_str()))
    }
}

/// A view of a text table that collects new strings instead of adding them,
/// so that a value can be converted from JSON without touching the table,
/// and the new strings committed only if the whole value is accepted.
///
/// New strings get the handles they will have once committed: the table's
/// length plus their position here.
pub(crate) struct PendingTexts<'a> {
    table: &'a TextTable,
    added: Vec<String>,
}

impl<'a> PendingTexts<'a> {
    pub(crate) fn new(table: &'a TextTable) -> Self {
        Self {
            table,
            added: Vec::new(),
        }
    }

    /// The strings to add, in the order their handles were given out.
    pub(crate) fn into_added(self) -> Vec<String> {
        self.added
    }
}

impl TextStore for PendingTexts<'_> {
    fn intern(&mut self, text: &str) -> Result<Text> {
        if let Some(handle) = self.table.find(text) {
            return Ok(handle);
        }
        let position = match self.added.iter().position(|added| added == text) {
            Some(position) => position,
            None => {
                self.added.push(text.to_owned());
                self.added.len() - 1
            }
        };
        let index = u32::try_from(self.table.len() + position).map_err(|_| {
            Diagnostic::error(
                "too_many_texts",
                "The world's text table is full.",
                "A world holds at most about four billion distinct strings.",
            )
        })?;
        Ok(Text::from_index(index))
    }

    fn resolve(&self, text: Text) -> Option<&str> {
        let index = text.index() as usize;
        match index.checked_sub(self.table.len()) {
            Some(position) => self.added.get(position).map(String::as_str),
            None => self.table.get(text),
        }
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
    fn pending_texts_hand_out_the_handles_committing_will_give() {
        let mut table = TextTable::default();
        let hero = table.intern("hero").unwrap();
        let mut pending = PendingTexts::new(&table);
        assert_eq!(pending.intern("hero").unwrap(), hero);
        let villain = pending.intern("villain").unwrap();
        let sidekick = pending.intern("sidekick").unwrap();
        assert_eq!(pending.intern("villain").unwrap(), villain);
        assert_eq!(pending.resolve(sidekick), Some("sidekick"));
        assert_eq!(pending.resolve(hero), Some("hero"));
        let added = pending.into_added();
        assert_eq!(table.len(), 2, "nothing was added yet");
        for text in &added {
            table.intern(text).unwrap();
        }
        assert_eq!(table.get(villain), Some("villain"));
        assert_eq!(table.get(sidekick), Some("sidekick"));
    }

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
