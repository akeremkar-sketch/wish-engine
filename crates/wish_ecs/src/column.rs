//! Type-erased storage for one component across the rows of an archetype.

// This module and `query`/`world` are the only places in the engine that view
// stored bytes as typed values. Every unsafe block states why it is sound.
#![allow(unsafe_code)]

use core::mem::MaybeUninit;
use core::ptr;

use wish_core::Result;

use crate::internal_error;

/// Values of one type, one per row, stored back to back, with the change
/// ticks of each row.
///
/// The buffer is made of `u64` words, so its start is aligned to 8, the
/// largest alignment a stored type may have; every slot `row * item_size` is
/// then aligned for the type, because a type's size is a multiple of its
/// alignment.
///
/// The words are `MaybeUninit` because padding inside a value may be left
/// uninitialised when native code assigns a whole value through `&mut T`.
/// Field bytes are always initialised, and only field bytes are ever read as
/// bytes ([`Column::read_into`]); whole slots are only copied raw.
///
/// Invariants: every slot below `len` holds a valid value of the column's
/// type; `added` and `changed` both have exactly `len` entries.
#[derive(Clone, Debug)]
pub(crate) struct Column {
    words: Vec<MaybeUninit<u64>>,
    item_size: usize,
    len: usize,
    /// The change tick at which each row's value was added.
    added: Vec<u64>,
    /// The change tick at which each row's value was last written.
    changed: Vec<u64>,
}

impl Column {
    pub(crate) fn new(item_size: usize) -> Self {
        Self {
            words: Vec::new(),
            item_size,
            len: 0,
            added: Vec::new(),
            changed: Vec::new(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Grows the buffer to hold at least `rows` values.
    fn ensure_rows(&mut self, rows: usize) -> Result<()> {
        let bytes = rows
            .checked_mul(self.item_size)
            .ok_or_else(|| internal_error("column size overflow"))?;
        let words = bytes.div_ceil(8);
        if self.words.len() < words {
            self.words.resize(words, MaybeUninit::uninit());
        }
        Ok(())
    }

    /// The start of the value buffer. Valid until the column is next resized.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.words.as_mut_ptr().cast()
    }

    fn as_ptr(&self) -> *const u8 {
        self.words.as_ptr().cast()
    }

    /// The start of the "last written" ticks. Valid until the column is next resized.
    pub(crate) fn changed_ptr(&mut self) -> *mut u64 {
        self.changed.as_mut_ptr()
    }

    /// The start of the "added" ticks. Valid until the column is next resized.
    pub(crate) fn added_ptr(&mut self) -> *mut u64 {
        self.added.as_mut_ptr()
    }

    /// `(added, changed)` ticks of a row.
    pub(crate) fn ticks(&self, row: usize) -> Option<(u64, u64)> {
        Some((*self.added.get(row)?, *self.changed.get(row)?))
    }

    fn check_row(&self, row: usize) -> Result<()> {
        if row < self.len {
            Ok(())
        } else {
            Err(internal_error(format!(
                "row {row} is past the end of a column of {} rows",
                self.len
            )))
        }
    }

    fn check_size(&self, len: usize) -> Result<()> {
        if len == self.item_size {
            Ok(())
        } else {
            Err(internal_error(format!(
                "{len} bytes given for a column of {}-byte values",
                self.item_size
            )))
        }
    }

    /// Appends a value added at `tick`. `bytes` must be a valid value of the
    /// column's type.
    pub(crate) fn push_bytes(&mut self, bytes: &[u8], tick: u64) -> Result<()> {
        self.check_size(bytes.len())?;
        self.ensure_rows(self.len + 1)?;
        let offset = self.len * self.item_size;
        // SAFETY: `ensure_rows` made the buffer at least `(len + 1) * item_size`
        // bytes long, so `offset..offset + item_size` is in bounds. `bytes`
        // cannot point into the buffer, because we hold `&mut self`.
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.as_mut_ptr().add(offset),
                self.item_size,
            );
        }
        self.len += 1;
        self.added.push(tick);
        self.changed.push(tick);
        Ok(())
    }

    /// Overwrites a value at `tick`. `bytes` must be a valid value of the
    /// column's type.
    pub(crate) fn write_bytes(&mut self, row: usize, bytes: &[u8], tick: u64) -> Result<()> {
        self.check_row(row)?;
        self.check_size(bytes.len())?;
        let offset = row * self.item_size;
        // SAFETY: `row < len`, so the slot is inside the buffer. `bytes`
        // cannot point into the buffer, because we hold `&mut self`.
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.as_mut_ptr().add(offset),
                self.item_size,
            );
        }
        if let Some(changed) = self.changed.get_mut(row) {
            *changed = tick;
        }
        Ok(())
    }

    /// Copies a value's field bytes into `out`; padding bytes come out as zero.
    pub(crate) fn read_into(
        &self,
        row: usize,
        spans: &[(usize, usize)],
        out: &mut [u8],
    ) -> Result<()> {
        self.check_row(row)?;
        self.check_size(out.len())?;
        out.fill(0);
        let base = row * self.item_size;
        for &(offset, len) in spans {
            if offset
                .checked_add(len)
                .is_none_or(|end| end > self.item_size)
            {
                return Err(internal_error("a field span lies outside its value"));
            }
            // SAFETY: the span lies inside the row's slot (checked above), and
            // the slot is inside the buffer because `row < len`. Spans cover
            // only field bytes, which are initialised: every value stored is
            // valid, and a valid value has all its field bytes initialised.
            // `out` has exactly `item_size` bytes, so the destination is in
            // bounds, and it is a separate allocation from the buffer.
            unsafe {
                ptr::copy_nonoverlapping(
                    self.as_ptr().add(base + offset),
                    out.as_mut_ptr().add(offset),
                    len,
                );
            }
        }
        Ok(())
    }

    /// Appends a copy of `source`'s value at `row`, ticks included.
    pub(crate) fn push_from(&mut self, source: &Column, row: usize) -> Result<()> {
        self.check_size(source.item_size)?;
        source.check_row(row)?;
        let (added, changed) = source
            .ticks(row)
            .ok_or_else(|| internal_error("a row has no change ticks"))?;
        self.ensure_rows(self.len + 1)?;
        let size = self.item_size;
        // SAFETY: the source slot is in bounds (`row < source.len`) and the
        // destination slot is in bounds (`ensure_rows`). The columns are
        // distinct (`&mut self` and `&Column` cannot alias), so the ranges do
        // not overlap. Copying raw bytes, uninitialised padding included, is
        // allowed between `MaybeUninit` buffers, and the copy is a valid value
        // because the source is.
        unsafe {
            ptr::copy_nonoverlapping(
                source.as_ptr().add(row * size),
                self.as_mut_ptr().add(self.len * size),
                size,
            );
        }
        self.len += 1;
        self.added.push(added);
        self.changed.push(changed);
        Ok(())
    }

    /// Removes a value by moving the last value into its slot.
    pub(crate) fn swap_remove(&mut self, row: usize) -> Result<()> {
        self.check_row(row)?;
        let last = self.len - 1;
        if row != last && self.item_size > 0 {
            let size = self.item_size;
            let base = self.as_mut_ptr();
            // SAFETY: `row < last < len`, so both slots are in bounds, and they
            // are different slots of `size > 0` bytes, so they do not overlap.
            unsafe {
                ptr::copy_nonoverlapping(base.add(last * size), base.add(row * size), size);
            }
        }
        self.len = last;
        if row < self.added.len() && row < self.changed.len() {
            self.added.swap_remove(row);
            self.changed.swap_remove(row);
            Ok(())
        } else {
            Err(internal_error("change ticks are out of step with values"))
        }
    }

    /// Views a value as `&T`.
    ///
    /// # Safety
    ///
    /// The column must store values of `T` (so `item_size == size_of::<T>()`
    /// and `align_of::<T>() <= 8`), and `row` must be below `len`.
    pub(crate) unsafe fn get<T>(&self, row: usize) -> &T {
        debug_assert_eq!(self.item_size, size_of::<T>());
        debug_assert!(row < self.len);
        // SAFETY: by the caller's guarantees the slot is in bounds and holds a
        // valid `T`; it is aligned for `T` (see the type's documentation). The
        // reference borrows `self`, so the buffer cannot move while it lives.
        unsafe { &*self.as_ptr().add(row * self.item_size).cast::<T>() }
    }

    /// Views a value as `&mut T`, together with its "last written" tick.
    ///
    /// # Safety
    ///
    /// The same as [`Column::get`].
    pub(crate) unsafe fn get_mut<T>(&mut self, row: usize) -> (&mut T, &mut u64) {
        debug_assert_eq!(self.item_size, size_of::<T>());
        debug_assert!(row < self.len);
        let size = self.item_size;
        let value = self.as_mut_ptr();
        let Self { changed, .. } = self;
        // SAFETY: as in `get`. The value lives in `words` and the tick in
        // `changed`, separate allocations, so the two references do not
        // alias; both borrow `self` mutably, so nothing else can reach the
        // column while they live. Writes through the value store a valid `T`,
        // which keeps the column's invariant. `row < len == changed.len()`.
        unsafe {
            (
                &mut *value.add(row * size).cast::<T>(),
                &mut *changed.as_mut_ptr().add(row),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(column: &Column, row: usize) -> Vec<u8> {
        let mut out = vec![0xFF; column.item_size];
        let spans = [(0, column.item_size)];
        column.read_into(row, &spans, &mut out).unwrap();
        out
    }

    #[test]
    fn push_write_read_and_swap_remove() {
        let mut column = Column::new(3);
        for (tick, value) in [[1, 2, 3], [4, 5, 6], [7, 8, 9]].iter().enumerate() {
            column.push_bytes(value, tick as u64 + 1).unwrap();
        }
        assert_eq!(column.len(), 3);
        assert_eq!(read(&column, 1), [4, 5, 6]);
        column.write_bytes(1, &[40, 50, 60], 9).unwrap();
        assert_eq!(read(&column, 1), [40, 50, 60]);
        assert_eq!(column.ticks(1), Some((2, 9)));
        column.swap_remove(0).unwrap();
        assert_eq!(column.len(), 2);
        assert_eq!(read(&column, 0), [7, 8, 9]);
        assert_eq!(column.ticks(0), Some((3, 3)), "ticks move with their value");
        assert_eq!(read(&column, 1), [40, 50, 60]);
        column.swap_remove(1).unwrap();
        assert_eq!(column.len(), 1);
        assert_eq!(read(&column, 0), [7, 8, 9]);
        assert_eq!(column.ticks(1), None);
    }

    #[test]
    fn padding_reads_as_zero() {
        let mut column = Column::new(4);
        column.push_bytes(&[1, 2, 3, 4], 1).unwrap();
        let mut out = [0xFF; 4];
        column.read_into(0, &[(1, 2)], &mut out).unwrap();
        assert_eq!(out, [0, 2, 3, 0]);
    }

    #[test]
    fn copies_between_columns() {
        let mut source = Column::new(8);
        source.push_bytes(&7u64.to_le_bytes(), 1).unwrap();
        source.push_bytes(&9u64.to_le_bytes(), 2).unwrap();
        let mut destination = Column::new(8);
        destination.push_from(&source, 1).unwrap();
        assert_eq!(read(&destination, 0), 9u64.to_le_bytes());
        assert_eq!(destination.ticks(0), Some((2, 2)));
        // SAFETY: the column stores `u64` values and row 0 exists.
        assert_eq!(unsafe { *destination.get::<u64>(0) }, 9);
        // SAFETY: as above.
        let (value, changed) = unsafe { destination.get_mut::<u64>(0) };
        *value += 1;
        *changed = 5;
        assert_eq!(read(&destination, 0), 10u64.to_le_bytes());
        assert_eq!(destination.ticks(0), Some((2, 5)));
    }

    #[test]
    fn zero_sized_values_only_count_rows() {
        let mut column = Column::new(0);
        column.push_bytes(&[], 1).unwrap();
        column.push_bytes(&[], 2).unwrap();
        assert_eq!(column.len(), 2);
        // SAFETY: the column stores `()` values and row 1 exists.
        assert_eq!(unsafe { *column.get::<()>(1) }, ());
        column.swap_remove(0).unwrap();
        assert_eq!(column.len(), 1);
        assert_eq!(column.ticks(0), Some((2, 2)));
    }

    #[test]
    fn misuse_is_an_internal_error_not_a_crash() {
        let mut column = Column::new(4);
        assert_eq!(
            column.push_bytes(&[1, 2], 1).unwrap_err().code,
            "internal_error"
        );
        assert_eq!(
            column.write_bytes(0, &[1, 2, 3, 4], 1).unwrap_err().code,
            "internal_error"
        );
        assert_eq!(column.swap_remove(0).unwrap_err().code, "internal_error");
        column.push_bytes(&[1, 2, 3, 4], 1).unwrap();
        let mut out = [0; 4];
        assert_eq!(
            column.read_into(0, &[(2, 4)], &mut out).unwrap_err().code,
            "internal_error"
        );
        let other = Column::new(8);
        assert_eq!(
            column.push_from(&other, 0).unwrap_err().code,
            "internal_error"
        );
    }
}
