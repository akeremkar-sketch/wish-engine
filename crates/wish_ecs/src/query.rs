//! Typed queries over the world's archetypes.
//!
//! A query names what it wants per entity, for example
//! `(Entity, &Position, &mut Velocity)`, optionally narrowed by a filter such
//! as `(With<Player>, Without<Frozen>)`. Running it walks every archetype
//! that has the requested components and yields one item per entity.

// Query items are references into column buffers created from raw pointers.
// Every unsafe block states why it is sound.
#![allow(unsafe_code)]

use core::marker::PhantomData;
use std::any::TypeId;

use wish_core::{Diagnostic, Entity, Result};
use wish_schema::Component;

use crate::archetype::Archetype;
use crate::registry::{ComponentId, Registry};

mod sealed {
    /// Prevents implementations of the query traits outside this crate: their
    /// soundness rests on details of the storage.
    pub trait Sealed {}
}

/// The components a query reads and writes, used to reject queries that
/// would hand out aliasing references.
#[derive(Debug, Default)]
pub struct Access {
    entries: Vec<(TypeId, &'static str, bool)>,
}

impl Access {
    fn add(&mut self, type_id: TypeId, name: &'static str, write: bool) {
        self.entries.push((type_id, name, write));
    }

    /// Fails when one component is accessed twice and at least once mutably.
    pub(crate) fn check(&self) -> Result<()> {
        for (index, (type_id, name, write)) in self.entries.iter().enumerate() {
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

/// What a query yields for each entity: [`Entity`], `&T`, `&mut T`, or a
/// tuple of these (up to eight).
///
/// # Safety
///
/// Implemented only inside this crate (the trait is sealed). An
/// implementation must report every component it reads or writes in
/// [`access`](QueryData::access), and [`item`](QueryData::item) must only
/// create references to the row it is given.
pub unsafe trait QueryData: sealed::Sealed {
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

    /// Prepares to read an archetype's rows. `None` when the archetype lacks
    /// a needed component.
    #[doc(hidden)]
    fn fetch(ids: Self::Ids, archetype: &mut Archetype) -> Option<Self::Fetch>;

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

impl sealed::Sealed for Entity {}

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

    fn fetch((): (), archetype: &mut Archetype) -> Option<*const Entity> {
        Some(archetype.entities.as_ptr())
    }

    unsafe fn item<'w>(entities: *const Entity, row: usize) -> Self::Item<'w> {
        // SAFETY: `row` is below the archetype's length (caller), which is the
        // length of its entity list, and the list is unchanged since `fetch`.
        unsafe { *entities.add(row) }
    }
}

impl<T: Component> sealed::Sealed for &T {}

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

    fn fetch(id: ComponentId, archetype: &mut Archetype) -> Option<*mut u8> {
        archetype.column_mut(id).map(|column| column.as_mut_ptr())
    }

    unsafe fn item<'w>(base: *mut u8, row: usize) -> &'w T {
        // SAFETY: the column stores `T` (its id was registered for `T`, and
        // registration checked that the layout matches `size_of::<T>()`), the
        // slot is in bounds and aligned (see `Column`), and it holds a valid
        // `T`. The access check rules out a `&mut T` to the same value, and
        // the caller keeps the archetype unchanged for `'w`.
        unsafe { &*base.add(row * size_of::<T>()).cast::<T>() }
    }
}

impl<T: Component> sealed::Sealed for &mut T {}

// SAFETY: reports a write of `T` and only creates a mutable reference to
// `T`'s value at `row`.
unsafe impl<T: Component> QueryData for &mut T {
    type Item<'w> = &'w mut T;
    type Ids = ComponentId;
    type Fetch = *mut u8;

    fn ids(registry: &Registry) -> Option<ComponentId> {
        registry.component_id_of::<T>()
    }

    fn access(access: &mut Access) {
        access.add(TypeId::of::<T>(), T::NAME, true);
    }

    fn fetch(id: ComponentId, archetype: &mut Archetype) -> Option<*mut u8> {
        archetype.column_mut(id).map(|column| column.as_mut_ptr())
    }

    unsafe fn item<'w>(base: *mut u8, row: usize) -> &'w mut T {
        // SAFETY: as for `&T`. The reference is unique: the access check rules
        // out any other reference to `T` in the same query, the caller allows
        // only one item per row, and the world is mutably borrowed for `'w`.
        unsafe { &mut *base.add(row * size_of::<T>()).cast::<T>() }
    }
}

macro_rules! tuple_query_data {
    ($($name:ident),+) => {
        impl<$($name: QueryData),+> sealed::Sealed for ($($name,)+) {}

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

            fn fetch(ids: Self::Ids, archetype: &mut Archetype) -> Option<Self::Fetch> {
                let ($($name,)+) = ids;
                Some(($($name::fetch($name, archetype)?,)+))
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

/// Narrows a query to archetypes that pass a test, without reading any data.
pub trait QueryFilter {
    /// Whether entities in `archetype` pass.
    fn matches(registry: &Registry, archetype: &Archetype) -> bool;
}

/// Passes entities that have component `T`.
pub struct With<T>(PhantomData<T>);

/// Passes entities that do not have component `T`.
pub struct Without<T>(PhantomData<T>);

impl QueryFilter for () {
    fn matches(_: &Registry, _: &Archetype) -> bool {
        true
    }
}

impl<T: Component> QueryFilter for With<T> {
    fn matches(registry: &Registry, archetype: &Archetype) -> bool {
        registry
            .component_id_of::<T>()
            .is_some_and(|id| archetype.contains(id))
    }
}

impl<T: Component> QueryFilter for Without<T> {
    fn matches(registry: &Registry, archetype: &Archetype) -> bool {
        registry
            .component_id_of::<T>()
            .is_none_or(|id| !archetype.contains(id))
    }
}

macro_rules! tuple_query_filter {
    ($($name:ident),+) => {
        impl<$($name: QueryFilter),+> QueryFilter for ($($name,)+) {
            fn matches(registry: &Registry, archetype: &Archetype) -> bool {
                $($name::matches(registry, archetype))&&+
            }
        }
    };
}

tuple_query_filter!(A);
tuple_query_filter!(A, B);
tuple_query_filter!(A, B, C);
tuple_query_filter!(A, B, C, D);

/// Iterates the results of a query. Borrows the world mutably for `'w`.
pub struct QueryIter<'w, Q: QueryData> {
    /// One entry per matching, non-empty archetype: where to read, and how many rows.
    tables: Vec<(Q::Fetch, usize)>,
    table: usize,
    row: usize,
    remaining: usize,
    world: PhantomData<&'w mut ()>,
}

impl<Q: QueryData> QueryIter<'_, Q> {
    /// Builds the iterator. The caller must hold the world mutably borrowed
    /// for the iterator's lifetime and must have checked the query's access.
    pub(crate) fn new(tables: Vec<(Q::Fetch, usize)>) -> Self {
        let remaining = tables.iter().map(|(_, len)| len).sum();
        Self {
            tables,
            table: 0,
            row: 0,
            remaining,
            world: PhantomData,
        }
    }
}

impl<'w, Q: QueryData> Iterator for QueryIter<'w, Q> {
    type Item = Q::Item<'w>;

    fn next(&mut self) -> Option<Q::Item<'w>> {
        while let Some(&(fetch, len)) = self.tables.get(self.table) {
            if self.row < len {
                let row = self.row;
                self.row += 1;
                self.remaining -= 1;
                // SAFETY: `fetch` was taken from an archetype of the world this
                // iterator mutably borrows for `'w`, so it cannot change while
                // items live; `row < len`, the archetype's length; and each row
                // is visited once, so no two items share a row. The world
                // checked the query's access before creating the iterator.
                return Some(unsafe { Q::item(fetch, row) });
            }
            self.table += 1;
            self.row = 0;
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<Q: QueryData> ExactSizeIterator for QueryIter<'_, Q> {}
