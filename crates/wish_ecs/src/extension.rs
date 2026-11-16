//! Extensions: simulation state that does not fit in component columns
//! (ADR-026). A physics engine's broad-phase tree is the example: it is
//! working memory derived from, and kept next to, the ECS data that decides
//! gameplay.
//!
//! The rules that keep this honest:
//! - Everything gameplay can see lives in components and resources; an
//!   extension holds only derived or cached state. It is therefore left out
//!   of [`World::hash`](crate::World::hash).
//! - Snapshots copy extensions and restoring puts them back, so going back
//!   and continuing is bit-for-bit the same as never having gone back.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;

/// A value an extension store can hold: plain `Clone` data.
pub trait Extension: Any + Send + Sync {
    /// A copy, for snapshots.
    fn clone_boxed(&self) -> Box<dyn Extension>;
    /// For downcasting.
    fn as_any(&self) -> &dyn Any;
    /// For downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// For taking it out by value.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Any + Clone + Send + Sync> Extension for T {
    fn clone_boxed(&self) -> Box<dyn Extension> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// The extensions of a world, one per type.
#[derive(Default)]
pub(crate) struct Extensions {
    items: BTreeMap<TypeId, Box<dyn Extension>>,
}

impl Clone for Extensions {
    fn clone(&self) -> Self {
        Self {
            items: self
                .items
                .iter()
                .map(|(id, item)| (*id, (**item).clone_boxed()))
                .collect(),
        }
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Extensions({})", self.items.len())
    }
}

impl Extensions {
    pub(crate) fn insert<T: Extension>(&mut self, value: T) -> Option<Box<dyn Extension>> {
        self.items.insert(TypeId::of::<T>(), Box::new(value))
    }

    pub(crate) fn get<T: Extension>(&self) -> Option<&T> {
        self.items
            .get(&TypeId::of::<T>())
            .and_then(|item| (**item).as_any().downcast_ref())
    }

    pub(crate) fn get_mut<T: Extension>(&mut self) -> Option<&mut T> {
        self.items
            .get_mut(&TypeId::of::<T>())
            .and_then(|item| (**item).as_any_mut().downcast_mut())
    }

    pub(crate) fn remove<T: Extension>(&mut self) -> bool {
        self.items.remove(&TypeId::of::<T>()).is_some()
    }

    pub(crate) fn take<T: Extension>(&mut self) -> Option<T> {
        let item = self.items.remove(&TypeId::of::<T>())?;
        item.into_any().downcast::<T>().ok().map(|boxed| *boxed)
    }
}
