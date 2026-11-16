//! Queries: the entities that have certain components, and those
//! components' values.
//!
//! ```ignore
//! fn fall(mut bodies: Query<(&mut Transform, &Velocity), With<Player>>, time: Res<Time>) {
//!     for (transform, velocity) in bodies.iter_mut() {
//!         transform.translation += velocity.value * time.delta;
//!     }
//! }
//! ```
//!
//! The engine copies the matching entities' components into the call's
//! frame as column slices; the query decodes them. Components asked for with
//! `&mut` are written back when the query is dropped (at the end of the
//! system), and the engine records as changed only the values that really
//! changed.

use std::marker::PhantomData;

use serde_json::{Value, json};
use wish_core::Entity;
use wish_schema::Component;

use crate::frame::{Section, query_entities, query_layout};
use crate::manifest::TypeTable;
use crate::param::SystemParam;

/// One column a query reads (and maybe writes).
#[derive(Clone, Debug)]
pub struct ColumnAccess {
    /// The component's name.
    pub component: String,
    /// Its size in bytes.
    pub size: usize,
    /// Whether the system writes it.
    pub write: bool,
}

/// What a query asks for: `Entity`, `&T`, `&mut T`, or a tuple of them.
pub trait QueryData: 'static {
    /// The decoded columns.
    type Storage;
    /// What iterating mutably yields.
    type Item<'q>
    where
        Self: 'q;
    /// What iterating yields (`&mut T` becomes `&T`).
    type ReadItem<'q>
    where
        Self: 'q;

    /// Adds the columns this asks for, in order.
    fn describe(types: &mut TypeTable, columns: &mut Vec<ColumnAccess>);
    /// Adds the sizes of those columns, in order (cheap: no schemas).
    fn sizes(out: &mut Vec<usize>);
    /// Decodes this part's columns, starting at column `*next`.
    fn fetch(bytes: &[u8], count: usize, offsets: &[usize], next: &mut usize) -> Self::Storage;
    /// Writes this part's writable columns back.
    fn write_back(storage: &Self::Storage, bytes: &mut [u8], offsets: &[usize], next: &mut usize);
    /// Iterates mutably.
    fn iter_mut<'q>(
        storage: &'q mut Self::Storage,
        entities: &'q [Entity],
    ) -> impl Iterator<Item = Self::Item<'q>>;
    /// Iterates.
    fn iter<'q>(
        storage: &'q Self::Storage,
        entities: &'q [Entity],
    ) -> impl Iterator<Item = Self::ReadItem<'q>>;
    /// One row, mutably.
    fn get_mut<'q>(
        storage: &'q mut Self::Storage,
        entities: &'q [Entity],
        row: usize,
    ) -> Option<Self::Item<'q>>;
    /// One row.
    fn get<'q>(
        storage: &'q Self::Storage,
        entities: &'q [Entity],
        row: usize,
    ) -> Option<Self::ReadItem<'q>>;
}

/// Decodes `count` values of `T` from the column at `offset`.
fn decode_column<T: Component>(bytes: &[u8], count: usize, offset: usize) -> Vec<T> {
    let size = size_of::<T>();
    (0..count)
        .map(|row| {
            let at = offset + row * size;
            bytes
                .get(at..at + size)
                .and_then(T::decode)
                .unwrap_or_else(|| {
                    crate::abi::fail(&format!(
                        "the engine sent a {} value this module cannot read; rebuild the game code with this engine's SDK",
                        T::NAME
                    ))
                })
        })
        .collect()
}

impl QueryData for Entity {
    type Storage = ();
    type Item<'q> = Entity;
    type ReadItem<'q> = Entity;

    fn describe(_: &mut TypeTable, _: &mut Vec<ColumnAccess>) {}
    fn sizes(_: &mut Vec<usize>) {}
    fn fetch(_: &[u8], _: usize, _: &[usize], _: &mut usize) {}
    fn write_back(_: &(), _: &mut [u8], _: &[usize], _: &mut usize) {}
    fn iter_mut<'q>(_: &'q mut (), entities: &'q [Entity]) -> impl Iterator<Item = Entity> {
        entities.iter().copied()
    }
    fn iter<'q>(_: &'q (), entities: &'q [Entity]) -> impl Iterator<Item = Entity> {
        entities.iter().copied()
    }
    fn get_mut<'q>(_: &'q mut (), entities: &'q [Entity], row: usize) -> Option<Entity> {
        entities.get(row).copied()
    }
    fn get<'q>(_: &'q (), entities: &'q [Entity], row: usize) -> Option<Entity> {
        entities.get(row).copied()
    }
}

impl<T: Component> QueryData for &'static T {
    type Storage = Vec<T>;
    type Item<'q> = &'q T;
    type ReadItem<'q> = &'q T;

    fn describe(types: &mut TypeTable, columns: &mut Vec<ColumnAccess>) {
        columns.push(ColumnAccess {
            component: types.component::<T>(),
            size: size_of::<T>(),
            write: false,
        });
    }
    fn sizes(out: &mut Vec<usize>) {
        out.push(size_of::<T>());
    }
    fn fetch(bytes: &[u8], count: usize, offsets: &[usize], next: &mut usize) -> Vec<T> {
        let offset = offsets.get(*next).copied().unwrap_or(usize::MAX);
        *next += 1;
        decode_column(bytes, count, offset)
    }
    fn write_back(_: &Vec<T>, _: &mut [u8], _: &[usize], next: &mut usize) {
        *next += 1;
    }
    fn iter_mut<'q>(storage: &'q mut Vec<T>, _: &'q [Entity]) -> impl Iterator<Item = &'q T> {
        storage.iter()
    }
    fn iter<'q>(storage: &'q Vec<T>, _: &'q [Entity]) -> impl Iterator<Item = &'q T> {
        storage.iter()
    }
    fn get_mut<'q>(storage: &'q mut Vec<T>, _: &'q [Entity], row: usize) -> Option<&'q T> {
        storage.get(row)
    }
    fn get<'q>(storage: &'q Vec<T>, _: &'q [Entity], row: usize) -> Option<&'q T> {
        storage.get(row)
    }
}

impl<T: Component> QueryData for &'static mut T {
    type Storage = Vec<T>;
    type Item<'q> = &'q mut T;
    type ReadItem<'q> = &'q T;

    fn describe(types: &mut TypeTable, columns: &mut Vec<ColumnAccess>) {
        columns.push(ColumnAccess {
            component: types.component::<T>(),
            size: size_of::<T>(),
            write: true,
        });
    }
    fn sizes(out: &mut Vec<usize>) {
        out.push(size_of::<T>());
    }
    fn fetch(bytes: &[u8], count: usize, offsets: &[usize], next: &mut usize) -> Vec<T> {
        let offset = offsets.get(*next).copied().unwrap_or(usize::MAX);
        *next += 1;
        decode_column(bytes, count, offset)
    }
    fn write_back(storage: &Vec<T>, bytes: &mut [u8], offsets: &[usize], next: &mut usize) {
        let offset = offsets.get(*next).copied().unwrap_or(usize::MAX);
        *next += 1;
        let size = size_of::<T>();
        for (row, value) in storage.iter().enumerate() {
            let at = offset + row * size;
            if let Some(slot) = bytes.get_mut(at..at + size) {
                value.encode(slot);
            }
        }
    }
    fn iter_mut<'q>(storage: &'q mut Vec<T>, _: &'q [Entity]) -> impl Iterator<Item = &'q mut T> {
        storage.iter_mut()
    }
    fn iter<'q>(storage: &'q Vec<T>, _: &'q [Entity]) -> impl Iterator<Item = &'q T> {
        storage.iter()
    }
    fn get_mut<'q>(storage: &'q mut Vec<T>, _: &'q [Entity], row: usize) -> Option<&'q mut T> {
        storage.get_mut(row)
    }
    fn get<'q>(storage: &'q Vec<T>, _: &'q [Entity], row: usize) -> Option<&'q T> {
        storage.get(row)
    }
}

/// Walks several iterators in step (a zip that yields a flat tuple).
#[doc(hidden)]
pub struct Together<I>(I);

macro_rules! tuple_data {
    ($($name:ident $index:tt),+) => {
        impl<$($name: QueryData),+> QueryData for ($($name,)+) {
            type Storage = ($($name::Storage,)+);
            type Item<'q> = ($($name::Item<'q>,)+) where Self: 'q;
            type ReadItem<'q> = ($($name::ReadItem<'q>,)+) where Self: 'q;

            fn describe(types: &mut TypeTable, columns: &mut Vec<ColumnAccess>) {
                $($name::describe(types, columns);)+
            }
            fn sizes(out: &mut Vec<usize>) {
                $($name::sizes(out);)+
            }
            fn fetch(bytes: &[u8], count: usize, offsets: &[usize], next: &mut usize) -> Self::Storage {
                ($($name::fetch(bytes, count, offsets, next),)+)
            }
            fn write_back(storage: &Self::Storage, bytes: &mut [u8], offsets: &[usize], next: &mut usize) {
                $($name::write_back(&storage.$index, bytes, offsets, next);)+
            }
            fn iter_mut<'q>(storage: &'q mut Self::Storage, entities: &'q [Entity]) -> impl Iterator<Item = Self::Item<'q>> {
                Together(($($name::iter_mut(&mut storage.$index, entities),)+))
            }
            fn iter<'q>(storage: &'q Self::Storage, entities: &'q [Entity]) -> impl Iterator<Item = Self::ReadItem<'q>> {
                Together(($($name::iter(&storage.$index, entities),)+))
            }
            fn get_mut<'q>(storage: &'q mut Self::Storage, entities: &'q [Entity], row: usize) -> Option<Self::Item<'q>> {
                Some(($($name::get_mut(&mut storage.$index, entities, row)?,)+))
            }
            fn get<'q>(storage: &'q Self::Storage, entities: &'q [Entity], row: usize) -> Option<Self::ReadItem<'q>> {
                Some(($($name::get(&storage.$index, entities, row)?,)+))
            }
        }

        impl<$($name: Iterator),+> Iterator for Together<($($name,)+)> {
            type Item = ($($name::Item,)+);
            fn next(&mut self) -> Option<Self::Item> {
                Some(($(self.0.$index.next()?,)+))
            }
        }
    };
}

tuple_data!(A 0);
tuple_data!(A 0, B 1);
tuple_data!(A 0, B 1, C 2);
tuple_data!(A 0, B 1, C 2, D 3);
tuple_data!(A 0, B 1, C 2, D 3, E 4);
tuple_data!(A 0, B 1, C 2, D 3, E 4, F 5);
tuple_data!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple_data!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

/// Narrows a query without reading data: `With<T>`, `Without<T>`, or a tuple.
pub trait QueryFilter {
    /// Adds the components that must be present and that must be absent.
    fn describe(types: &mut TypeTable, with: &mut Vec<String>, without: &mut Vec<String>);
}

impl QueryFilter for () {
    fn describe(_: &mut TypeTable, _: &mut Vec<String>, _: &mut Vec<String>) {}
}

/// Only entities that have `T`.
pub struct With<T>(PhantomData<T>);

/// Only entities that do not have `T`.
pub struct Without<T>(PhantomData<T>);

impl<T: Component> QueryFilter for With<T> {
    fn describe(types: &mut TypeTable, with: &mut Vec<String>, _: &mut Vec<String>) {
        with.push(types.component::<T>());
    }
}

impl<T: Component> QueryFilter for Without<T> {
    fn describe(types: &mut TypeTable, _: &mut Vec<String>, without: &mut Vec<String>) {
        without.push(types.component::<T>());
    }
}

macro_rules! tuple_filter {
    ($($name:ident),+) => {
        impl<$($name: QueryFilter),+> QueryFilter for ($($name,)+) {
            fn describe(types: &mut TypeTable, with: &mut Vec<String>, without: &mut Vec<String>) {
                $($name::describe(types, with, without);)+
            }
        }
    };
}

tuple_filter!(A);
tuple_filter!(A, B);
tuple_filter!(A, B, C);
tuple_filter!(A, B, C, D);

/// The entities matching `F` that have everything `D` asks for, with those
/// components. See the module documentation.
pub struct Query<D: QueryData, F: QueryFilter = ()> {
    entities: Vec<Entity>,
    storage: D::Storage,
    section: Section,
    offsets: Vec<usize>,
    _filter: PhantomData<F>,
}

impl<D: QueryData, F: QueryFilter> Query<D, F> {
    /// Iterates over the matches, in the engine's stable order.
    pub fn iter(&self) -> impl Iterator<Item = D::ReadItem<'_>> {
        D::iter(&self.storage, &self.entities)
    }

    /// Iterates over the matches, with `&mut` components writable.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = D::Item<'_>> {
        D::iter_mut(&mut self.storage, &self.entities)
    }

    /// How many entities match.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether no entity matches.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// The matching entities, in order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The match for one entity, if it matches.
    pub fn get(&self, entity: Entity) -> Option<D::ReadItem<'_>> {
        let row = self.entities.iter().position(|e| *e == entity)?;
        D::get(&self.storage, &self.entities, row)
    }

    /// The match for one entity, writable, if it matches.
    pub fn get_mut(&mut self, entity: Entity) -> Option<D::Item<'_>> {
        let row = self.entities.iter().position(|e| *e == entity)?;
        D::get_mut(&mut self.storage, &self.entities, row)
    }

    /// The only match, if exactly one entity matches.
    pub fn single(&self) -> Option<D::ReadItem<'_>> {
        if self.entities.len() == 1 {
            D::get(&self.storage, &self.entities, 0)
        } else {
            None
        }
    }

    /// The only match, writable, if exactly one entity matches.
    pub fn single_mut(&mut self) -> Option<D::Item<'_>> {
        if self.entities.len() == 1 {
            D::get_mut(&mut self.storage, &self.entities, 0)
        } else {
            None
        }
    }
}

impl<D: QueryData, F: QueryFilter> SystemParam for Query<D, F> {
    fn describe(types: &mut TypeTable) -> Value {
        let mut columns = Vec::new();
        D::describe(types, &mut columns);
        let (mut with, mut without) = (Vec::new(), Vec::new());
        F::describe(types, &mut with, &mut without);
        json!({ "query": {
            "columns": columns.iter().map(|c| json!({ "type": c.component, "write": c.write })).collect::<Vec<_>>(),
            "with": with,
            "without": without,
        } })
    }

    fn fetch(section: Section) -> Self {
        let bytes = section.bytes();
        let entities: Vec<Entity> = query_entities(bytes)
            .into_iter()
            .map(Entity::from_bits)
            .collect();
        let mut sizes = Vec::new();
        D::sizes(&mut sizes);
        let (_, offsets) = query_layout(entities.len(), &sizes);
        let storage = D::fetch(bytes, entities.len(), &offsets, &mut 0);
        Self {
            entities,
            storage,
            section,
            offsets,
            _filter: PhantomData,
        }
    }
}

impl<D: QueryData, F: QueryFilter> Drop for Query<D, F> {
    fn drop(&mut self) {
        D::write_back(
            &self.storage,
            self.section.bytes_mut(),
            &self.offsets,
            &mut 0,
        );
    }
}
