//! Archetypes: tables of the entities that have exactly the same components.

use wish_core::{Entity, Result};

use crate::column::Column;
use crate::internal_error;
use crate::registry::{ComponentId, Registry};

/// The entities that have exactly one set of components, stored as a table:
/// one column per component, one row per entity.
#[derive(Clone, Debug)]
pub struct Archetype {
    /// Sorted, without repeats.
    components: Vec<ComponentId>,
    /// `columns[i]` stores `components[i]`.
    pub(crate) columns: Vec<Column>,
    pub(crate) entities: Vec<Entity>,
}

impl Archetype {
    /// The archetype of entities with no components.
    pub(crate) fn empty() -> Self {
        Self {
            components: Vec::new(),
            columns: Vec::new(),
            entities: Vec::new(),
        }
    }

    /// An archetype for `components`, which must be sorted and without repeats.
    pub(crate) fn new(components: Vec<ComponentId>, registry: &Registry) -> Result<Self> {
        let columns = components
            .iter()
            .map(|id| {
                registry
                    .component(*id)
                    .map(|info| Column::new(info.size()))
                    .ok_or_else(|| internal_error("an archetype names an unregistered component"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            components,
            columns,
            entities: Vec::new(),
        })
    }

    /// The components every entity here has, sorted by id.
    pub fn components(&self) -> &[ComponentId] {
        &self.components
    }

    /// The entities stored here, in row order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The number of entities stored here.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether no entities are stored here.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Whether entities here have the component.
    pub fn contains(&self, id: ComponentId) -> bool {
        self.components.binary_search(&id).is_ok()
    }

    pub(crate) fn column(&self, id: ComponentId) -> Option<&Column> {
        let index = self.components.binary_search(&id).ok()?;
        self.columns.get(index)
    }

    pub(crate) fn column_mut(&mut self, id: ComponentId) -> Option<&mut Column> {
        let index = self.components.binary_search(&id).ok()?;
        self.columns.get_mut(index)
    }

    /// Removes a row by moving the last row into it. Returns the entity that
    /// now occupies `row`, if one was moved.
    pub(crate) fn swap_remove(&mut self, row: usize) -> Result<Option<Entity>> {
        if row >= self.entities.len() {
            return Err(internal_error(
                "removing a row past the end of an archetype",
            ));
        }
        for column in &mut self.columns {
            column.swap_remove(row)?;
        }
        self.entities.swap_remove(row);
        Ok(self.entities.get(row).copied())
    }
}
