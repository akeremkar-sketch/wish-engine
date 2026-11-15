//! Events: plain-data messages between systems.

use core::fmt;
use core::marker::PhantomData;

use wish_core::Result;

use crate::column::Column;

/// The events of one type, double-buffered.
///
/// Events sent during an update are readable during that update and the
/// next; [`EventQueue::update`] drops the older half. Every event has a
/// sequence number (per type, starting at 0), which readers use to see each
/// event once.
#[derive(Clone, Debug)]
pub(crate) struct EventQueue {
    item_size: usize,
    previous: Column,
    current: Column,
    /// Sequence number of the first event in `previous`.
    previous_start: u64,
    /// Sequence number of the first event in `current`.
    current_start: u64,
}

impl EventQueue {
    pub(crate) fn new(item_size: usize) -> Self {
        Self {
            item_size,
            previous: Column::new(item_size),
            current: Column::new(item_size),
            previous_start: 0,
            current_start: 0,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8], tick: u64) -> Result<()> {
        self.current.push_bytes(bytes, tick)
    }

    /// Drops the older buffer; the current one becomes the older one.
    pub(crate) fn update(&mut self) {
        let fresh = Column::new(self.item_size);
        self.previous = core::mem::replace(&mut self.current, fresh);
        self.previous_start = self.current_start;
        self.current_start += self.previous.len() as u64;
    }

    /// The sequence number the next event will get.
    pub(crate) fn next_sequence(&self) -> u64 {
        self.current_start + self.current.len() as u64
    }

    /// Every buffered event with a sequence number of at least `from`, oldest
    /// first, as `(column, row)`.
    pub(crate) fn rows_from(&self, from: u64) -> impl Iterator<Item = (&Column, usize)> {
        let previous = (0..self.previous.len())
            .filter(move |row| self.previous_start + *row as u64 >= from)
            .map(|row| (&self.previous, row));
        let current = (0..self.current.len())
            .filter(move |row| self.current_start + *row as u64 >= from)
            .map(|row| (&self.current, row));
        previous.chain(current)
    }
}

/// Remembers how far a reader has read one event type, so that each event
/// is seen once. Start with `EventCursor::default()`.
pub struct EventCursor<E> {
    pub(crate) next: u64,
    event: PhantomData<fn() -> E>,
}

impl<E> EventCursor<E> {
    pub(crate) fn at(next: u64) -> Self {
        Self {
            next,
            event: PhantomData,
        }
    }

    /// The sequence number of the next event this cursor will read.
    pub fn position(&self) -> u64 {
        self.next
    }
}

impl<E> Default for EventCursor<E> {
    fn default() -> Self {
        Self::at(0)
    }
}

impl<E> Clone for EventCursor<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for EventCursor<E> {}

impl<E> fmt::Debug for EventCursor<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCursor")
            .field("next", &self.next)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequences(queue: &EventQueue, from: u64) -> Vec<u8> {
        queue
            .rows_from(from)
            .map(|(column, row)| {
                let mut out = [0u8; 1];
                column.read_into(row, &[(0, 1)], &mut out).unwrap();
                out[0]
            })
            .collect()
    }

    #[test]
    fn events_live_for_two_updates() {
        let mut queue = EventQueue::new(1);
        queue.push(&[10], 1).unwrap();
        queue.push(&[11], 1).unwrap();
        assert_eq!(sequences(&queue, 0), [10, 11]);
        queue.update();
        queue.push(&[12], 2).unwrap();
        assert_eq!(sequences(&queue, 0), [10, 11, 12]);
        assert_eq!(sequences(&queue, 1), [11, 12]);
        assert_eq!(queue.next_sequence(), 3);
        queue.update();
        assert_eq!(
            sequences(&queue, 0),
            [12],
            "the first update's events are gone"
        );
        queue.update();
        assert!(sequences(&queue, 0).is_empty());
        assert_eq!(queue.next_sequence(), 3);
    }
}
