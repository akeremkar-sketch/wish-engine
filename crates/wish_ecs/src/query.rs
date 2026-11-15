//! Typed queries over the world's archetypes.
//!
//! A query names what it wants per entity, for example
//! `(Entity, &Position, &mut Velocity)`, optionally narrowed by a filter such
//! as `(With<Player>, Without<Frozen>)` or `Changed<Position>`. Running it
//! walks every archetype that has the requested components and yields one
//! item per entity.

// Query items are references into column buffers created from raw pointers.
// Every unsafe block states why it is sound.
#![allow(unsafe_code)]

use core::fmt;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use std::any::TypeId;

use wish_core::{Diagnostic, Entity, Result};
use wish_schema::Component;

use crate::archetype::Archetype;
use crate::registry::{ComponentId, Registry};

mod sealed {
    /// Prevents implementations of `QueryData` outside this crate: its
    /// soundness rests on details of the storage.
    pub trait Data {}

    /// Prevents implementations of `QueryFilter` outside this crate.
    pub trait Filter {}
}

/// A point in the world's history of changes.
///
/// Every write to a component records the world's current change tick. A
/// query with [`Changed`] or [`Added`] run "since" a tick passes only the
/// components written or added after it. Tick 0 comes before every change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangeTick(pub(crate) u64);

impl ChangeTick {
    /// The tick before any change was made.
    pub const ZERO: Self = Self(0);

    /// The tick's number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A component reached mutably through a query or `World::get_mut`.
///
/// Reading through it changes nothing. Taking it mutably (assigning to a
/// field, calling a `&mut self` method) records the component as changed at
/// the world's current change tick, which is what [`Changed`] looks at.
pub struct Mut<'w, T> {
    value: &'w mut T,
    changed: &'w mut u64,
    tick: u64,
}

impl<'w, T> Mut<'w, T> {
    pub(crate) fn new(value: &'w mut T, changed: &'w mut u64, tick: u64) -> Self {
        Self {
            value,
            changed,
            tick,
        }
    }

    /// The plain mutable reference, recording the component as changed.
    pub fn into_inner(self) -> &'w mut T {
        *self.changed = self.tick;
        self.value
    }
}

impl<T> Deref for Mut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

impl<T> DerefMut for Mut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        *self.changed = self.tick;
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for Mut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

/// The components a query reads and writes, used to reject queries that
/// would hand out aliasing references or write engine-managed components.
#[derive(Debug, Default)]
pub struct Access {
    entries: Vec<(TypeId, &'static str, bool)>,
}

impl Access {
    fn add(&mut self, type_id: TypeId, name: &'static str, write: bool) {
        self.entries.push((type_id, name, write));
    }

    /// Fails when one component is accessed twice and at least once mutably,
    /// or when a managed component is accessed mutably.
    pub(crate) fn check(&self, registry: &Registry) -> Result<()> {
        for (index, (type_id, name, write)) in self.entries.iter().enumerate() {
            if *write && registry.is_managed_type(*type_id) {
                return Err(crate::world::managed_component(name));
            }
            let conflict = self.entries[index + 1..]
                .iter()
                .any(|(other, _, other_write)| other == type_id && (*write || *other_write));
            if conflict {
                return Err(Diagnostic::error(
                    "conflicting_query_access",
                    format!(
                        "The query accesses component '{name}' more than once, and at least once mutably."
                    ),
                    format!(
                        "Ask for each component once per query: use `&mut {name}` alone if you need to change it."
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// What a query yields for each entity: [`Entity`], `&T`, `&mut T` (as
/// [`Mut<T>`]), or a tuple of these (up to eight).
///
/// # Safety
///
/// Implemented only inside this crate (the trait is sealed). An
/// implementation must report every component it reads or writes in
/// [`access`](QueryData::access), and [`item`](QueryData::item) must only
/// create references to the row it is given.
pub unsafe trait QueryData: sealed::Data {
    /// The value yielded per entity.
    type Item<'w>;
    /// The component ids the query needs, looked up once per run.
    #[doc(hidden)]
    type Ids: Copy;
    /// Where to read one archetype's rows from.
    #[doc(hidden)]
    type Fetch: Copy;

    /// Looks up the component ids. `None` when a needed component was never
    /// registered, in which case no entity can match.
    #[doc(hidden)]
    fn ids(registry: &Registry) -> Option<Self::Ids>;

    /// Records the components read and written.
    #[doc(hidden)]
    fn access(access: &mut Access);

    /// Prepares to read an archetype's rows; `tick` is the world's current
    /// change tick. `None` when the archetype lacks a needed component.
    #[doc(hidden)]
    fn fetch(ids: Self::Ids, archetype: &mut Archetype, tick: u64) -> Option<Self::Fetch>;

    /// The item for one row.
    ///
    /// # Safety
    ///
    /// `fetch` must come from an archetype that has not changed since, `row`
    /// must be below that archetype's length, the archetype must stay
    /// unchanged for `'w`, and no other item for the same row may be alive.
    #[doc(hidden)]
    unsafe fn item<'w>(fetch: Self::Fetch, row: usize) -> Self::Item<'w>;
}

impl sealed::Data for Entity {}

// SAFETY: reads only the archetype's entity list, which no query item can
// write, and only at `row`.
unsafe impl QueryData for Entity {
    type Item<'w> = Entity;
    type Ids = ();
    type Fetch = *const Entity;

    fn ids(_: &Registry) -> Option<()> {
        Some(())
    }

    fn access(_: &mut Access) {}

    fn fetch((): (), archetype: &mut Archetype, _: u64) -> Option<*const Entity> {
        Some(archetype.entities.as_ptr())
    }

    unsafe fn item<'w>(entities: *const Entity, row: usize) -> Self::Item<'w> {
        // SAFETY: `row` is below the archetype's length (caller), which is the
        // length of its entity list, and the list is unchanged since `fetch`.
        unsafe { *entities.add(row) }
    }
}

impl<T: Component> sealed::Data for &T {}

// SAFETY: reports a read of `T` and only creates a shared reference to `T`'s
// value at `row`.
unsafe impl<T: Component> QueryData for &T {
    type Item<'w> = &'w T;
    type Ids = ComponentId;
    type Fetch = *mut u8;

    fn ids(registry: &Registry) -> Option<ComponentId> {
        registry.component_id_of::<T>()
    }

    fn access(access: &mut Access) {
        access.add(TypeId::of::<T>(), T::NAME, false);
    }

    fn fetch(id: ComponentId, archetype: &mut Archetype, _: u64) -> Option<*mut u8> {
        archetype.column_mut(id).map(|column| column.as_mut_ptr())
    }

    unsafe fn item<'w>(base: *mut u8, row: usize) -> Self::Item<'w> {
        // SAFETY: the column stores `T` (its id was registered for `T`, and
        // registration checked that the layout matches `size_of::<T>()`), the
        // slot is in bounds and aligned (see `Column`), and it holds a valid
        // `T`. The access check rules out a `&mut T` to the same value, and
        // the caller keeps the archetype unchanged for `'w`.
        unsafe { &*base.add(row * size_of::<T>()).cast::<T>() }
    }
}

impl<T: Component> sealed::Data for &mut T {}

// SAFETY: reports a write of `T` and only creates mutable references to
// `T`'s value and its "last written" tick at `row`.
unsafe impl<T: Component> QueryData for &mut T {
    type Item<'w> = Mut<'w, T>;
    type Ids = ComponentId;
    type Fetch = (*mut u8, *mut u64, u64);

    fn ids(registry: &Registry) -> Option<ComponentId> {
        registry.component_id_of::<T>()
    }

    fn access(access: &mut Access) {
        access.add(TypeId::of::<T>(), T::NAME, true);
    }

    fn fetch(id: ComponentId, archetype: &mut Archetype, tick: u64) -> Option<Self::Fetch> {
        let column = archetype.column_mut(id)?;
        let values = column.as_mut_ptr();
        let changed = column.changed_ptr();
        Some((values, changed, tick))
    }

    unsafe fn item<'w>((values, changed, tick): Self::Fetch, row: usize) -> Self::Item<'w> {
        // SAFETY: as for `&T`, and the tick list has one entry per row. Both
        // references are unique: the access check rules out any other
        // reference to `T` in the same query, the caller allows only one item
        // per row, and the world is mutably borrowed for `'w`. Filters read
        // other rows' ticks only through the raw pointer, never this row's
        // after this item exists.
        unsafe {
            Mut::new(
                &mut *values.add(row * size_of::<T>()).cast::<T>(),
                &mut *changed.add(row),
                tick,
            )
        }
    }
}

macro_rules! tuple_query_data {
    ($($name:ident),+) => {
        impl<$($name: QueryData),+> sealed::Data for ($($name,)+) {}

        // SAFETY: each element reports its own access and creates references
        // only to `row`; together they report everything the tuple touches.
        #[allow(non_snake_case)]
        unsafe impl<$($name: QueryData),+> QueryData for ($($name,)+) {
            type Item<'w> = ($($name::Item<'w>,)+);
            type Ids = ($($name::Ids,)+);
            type Fetch = ($($name::Fetch,)+);

            fn ids(registry: &Registry) -> Option<Self::Ids> {
                Some(($($name::ids(registry)?,)+))
            }

            fn access(access: &mut Access) {
                $($name::access(access);)+
            }

            fn fetch(ids: Self::Ids, archetype: &mut Archetype, tick: u64) -> Option<Self::Fetch> {
                let ($($name,)+) = ids;
                Some(($($name::fetch($name, archetype, tick)?,)+))
            }

            unsafe fn item<'w>(fetch: Self::Fetch, row: usize) -> Self::Item<'w> {
                let ($($name,)+) = fetch;
                // SAFETY: the caller's guarantees hold for every element.
                ($(unsafe { $name::item($name, row) },)+)
            }
        }
    };
}

tuple_query_data!(A);
tuple_query_data!(A, B);
tuple_query_data!(A, B, C);
tuple_query_data!(A, B, C, D);
tuple_query_data!(A, B, C, D, E);
tuple_query_data!(A, B, C, D, E, F);
tuple_query_data!(A, B, C, D, E, F, G);
tuple_query_data!(A, B, C, D, E, F, G, H);

/// Narrows a query: [`With`], [`Without`], [`Changed`], [`Added`], or a tuple
/// of these (up to four), which must all pass.
///
/// # Safety
///
/// Implemented only inside this crate (the trait is sealed).
/// [`matches`](QueryFilter::matches) may only read the row it is given.
pub unsafe trait QueryFilter: sealed::Filter {
    /// What the filter reads per archetype.
    #[doc(hidden)]
    type Fetch: Copy;

    /// Prepares to test an archetype's rows; `since` is the tick the query
    /// looks for changes after. `None` when no row of the archetype can pass.
    #[doc(hidden)]
    fn fetch(registry: &Registry, archetype: &mut Archetype, since: u64) -> Option<Self::Fetch>;

    /// Whether one row passes.
    ///
    /// # Safety
    ///
    /// The same as [`QueryData::item`], except that it creates no reference
    /// that outlives the call.
    #[doc(hidden)]
    unsafe fn matches(fetch: Self::Fetch, row: usize) -> bool;
}

/// Passes entities that have component `T`.
pub struct With<T>(PhantomData<T>);

/// Passes entities that do not have component `T`.
pub struct Without<T>(PhantomData<T>);

/// Passes entities whose component `T` was written after the query's
/// "since" tick (adding it counts as writing it).
pub struct Changed<T>(PhantomData<T>);

/// Passes entities whose component `T` was added after the query's "since" tick.
pub struct Added<T>(PhantomData<T>);

impl sealed::Filter for () {}

// SAFETY: reads nothing.
unsafe impl QueryFilter for () {
    type Fetch = ();

    fn fetch(_: &Registry, _: &mut Archetype, _: u64) -> Option<()> {
        Some(())
    }

    unsafe fn matches((): (), _: usize) -> bool {
        true
    }
}

impl<T: Component> sealed::Filter for With<T> {}

// SAFETY: decides per archetype; reads no row.
unsafe impl<T: Component> QueryFilter for With<T> {
    type Fetch = ();

    fn fetch(registry: &Registry, archetype: &mut Archetype, _: u64) -> Option<()> {
        let id = registry.component_id_of::<T>()?;
        archetype.contains(id).then_some(())
    }

    unsafe fn matches((): (), _: usize) -> bool {
        true
    }
}

impl<T: Component> sealed::Filter for Without<T> {}

// SAFETY: decides per archetype; reads no row.
unsafe impl<T: Component> QueryFilter for Without<T> {
    type Fetch = ();

    fn fetch(registry: &Registry, archetype: &mut Archetype, _: u64) -> Option<()> {
        let contains = registry
            .component_id_of::<T>()
            .is_some_and(|id| archetype.contains(id));
        (!contains).then_some(())
    }

    unsafe fn matches((): (), _: usize) -> bool {
        true
    }
}

/// Reads one tick at `row` from a tick list.
///
/// # Safety
///
/// `ticks` must point at a list of at least `row + 1` ticks that no live
/// reference covers at `row`.
unsafe fn tick_after(ticks: *const u64, row: usize, since: u64) -> bool {
    // SAFETY: by the caller's guarantee the read is in bounds and does not
    // conflict with a live `&mut`.
    unsafe { *ticks.add(row) > since }
}

impl<T: Component> sealed::Filter for Changed<T> {}

// SAFETY: reads only the "last written" tick at `row`, before any item for
// that row exists (the iterator tests a row before creating its item).
unsafe impl<T: Component> QueryFilter for Changed<T> {
    type Fetch = (*const u64, u64);

    fn fetch(registry: &Registry, archetype: &mut Archetype, since: u64) -> Option<Self::Fetch> {
        let column = archetype.column_mut(registry.component_id_of::<T>()?)?;
        Some((column.changed_ptr().cast_const(), since))
    }

    unsafe fn matches((ticks, since): Self::Fetch, row: usize) -> bool {
        // SAFETY: `row` is below the archetype's length, which is the length
        // of the tick list; no item for this row exists yet.
        unsafe { tick_after(ticks, row, since) }
    }
}

impl<T: Component> sealed::Filter for Added<T> {}

// SAFETY: reads only the "added" tick at `row`; no item ever references the
// "added" ticks.
unsafe impl<T: Component> QueryFilter for Added<T> {
    type Fetch = (*const u64, u64);

    fn fetch(registry: &Registry, archetype: &mut Archetype, since: u64) -> Option<Self::Fetch> {
        let column = archetype.column_mut(registry.component_id_of::<T>()?)?;
        Some((column.added_ptr().cast_const(), since))
    }

    unsafe fn matches((ticks, since): Self::Fetch, row: usize) -> bool {
        // SAFETY: `row` is below the archetype's length, which is the length
        // of the tick list.
        unsafe { tick_after(ticks, row, since) }
    }
}

macro_rules! tuple_query_filter {
    ($($name:ident),+) => {
        impl<$($name: QueryFilter),+> sealed::Filter for ($($name,)+) {}

        // SAFETY: each element reads only `row`.
        #[allow(non_snake_case)]
        unsafe impl<$($name: QueryFilter),+> QueryFilter for ($($name,)+) {
            type Fetch = ($($name::Fetch,)+);

            fn fetch(registry: &Registry, archetype: &mut Archetype, since: u64) -> Option<Self::Fetch> {
                Some(($($name::fetch(registry, archetype, since)?,)+))
            }

            unsafe fn matches(fetch: Self::Fetch, row: usize) -> bool {
                let ($($name,)+) = fetch;
                // SAFETY: the caller's guarantees hold for every element.
                true $(&& unsafe { $name::matches($name, row) })+
            }
        }
    };
}

tuple_query_filter!(A);
tuple_query_filter!(A, B);
tuple_query_filter!(A, B, C);
tuple_query_filter!(A, B, C, D);

/// Iterates the results of a query. Borrows the world mutably for `'w`.
pub struct QueryIter<'w, Q: QueryData, F: QueryFilter = ()> {
    /// One entry per candidate archetype: where to read, what the filter
    /// reads, and how many rows.
    tables: Vec<(Q::Fetch, F::Fetch, usize)>,
    table: usize,
    row: usize,
    remaining: usize,
    world: PhantomData<&'w mut ()>,
}

impl<Q: QueryData, F: QueryFilter> QueryIter<'_, Q, F> {
    /// Builds the iterator. The caller must hold the world mutably borrowed
    /// for the iterator's lifetime and must have checked the query's access.
    pub(crate) fn new(tables: Vec<(Q::Fetch, F::Fetch, usize)>) -> Self {
        let remaining = tables.iter().map(|(_, _, len)| len).sum();
        Self {
            tables,
            table: 0,
            row: 0,
            remaining,
            world: PhantomData,
        }
    }
}

impl<'w, Q: QueryData, F: QueryFilter> Iterator for QueryIter<'w, Q, F> {
    type Item = Q::Item<'w>;

    fn next(&mut self) -> Option<Q::Item<'w>> {
        while let Some(&(fetch, filter, len)) = self.tables.get(self.table) {
            while self.row < len {
                let row = self.row;
                self.row += 1;
                self.remaining -= 1;
                // SAFETY: `filter` and `fetch` were taken from an archetype of
                // the world this iterator mutably borrows for `'w`, so it
                // cannot change while items live; `row < len`, the archetype's
                // length; each row is visited once, so no two items share a
                // row, and the filter reads the row before its item exists.
                // The world checked the query's access before creating the
                // iterator.
                unsafe {
                    if F::matches(filter, row) {
                        return Some(Q::item(fetch, row));
                    }
                }
            }
            self.table += 1;
            self.row = 0;
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}
