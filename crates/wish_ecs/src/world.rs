//! The world: entities, their components, resources and text.

// Typed accessors view column bytes as component values.
#![allow(unsafe_code)]

use core::fmt;
use std::collections::BTreeMap;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Entity, Result, Text};
use wish_schema::{
    Component, JsonContext, Resource, TypeSchema, apply_json, to_json, validate_bytes,
};

use crate::archetype::Archetype;
use crate::column::Column;
use crate::entities::{Entities, EntityLocation};
use crate::internal_error;
use crate::query::{Access, QueryData, QueryFilter, QueryIter};
use crate::registry::{ComponentId, Registry, ResourceId, TypeInfo};
use crate::text::TextTable;

/// The archetype of entities with no components. Always present.
const EMPTY_ARCHETYPE: usize = 0;

/// All dynamic state of a world. Snapshots (WP6) copy exactly this, so
/// anything that changes while the world runs must live here.
#[derive(Clone, Debug)]
struct WorldState {
    entities: Entities,
    /// In creation order; queries walk them in this order.
    archetypes: Vec<Archetype>,
    archetype_ids: BTreeMap<Vec<ComponentId>, usize>,
    /// Indexed by resource id; a present resource is a one-row column.
    resources: Vec<Option<Column>>,
    texts: TextTable,
}

/// Entities, their components, resources and text.
///
/// Typed access (`insert`, `get`, `query`) works with Rust types declared
/// with `#[component]` and `#[resource]`; they are registered on first use.
/// Dynamic access (`set_json`, `get_json`, `insert_bytes`) works with any
/// registered type by name, which is how AI agents, the editor and script
/// modules reach the world.
#[derive(Clone)]
pub struct World {
    registry: Registry,
    state: WorldState,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.entity_count())
            .field("archetypes", &self.state.archetypes.len())
            .field("components", &self.registry.components().len())
            .field("resources", &self.registry.resources().len())
            .finish_non_exhaustive()
    }
}

fn entity_not_found(entity: Entity) -> Diagnostic {
    Diagnostic::error(
        "entity_not_found",
        format!("Entity {entity} does not exist."),
        "It was despawned, or it was never spawned in this world. List the entities that exist now and use one of those.",
    )
}

fn missing_component(entity: Entity, name: &str) -> Diagnostic {
    Diagnostic::error(
        "component_not_found",
        format!("Entity {entity} has no '{name}' component."),
        "Setting the component creates it with default values for the fields you leave out. To see what the entity has, list its components.",
    )
}

fn missing_resource(name: &str) -> Diagnostic {
    Diagnostic::error(
        "resource_not_found",
        format!("The resource '{name}' is registered but has no value in this world."),
        "Set the resource first; fields you leave out take their default values.",
    )
}

/// Two distinct elements of a slice, mutably.
fn pair_mut(
    items: &mut [Archetype],
    a: usize,
    b: usize,
) -> Result<(&mut Archetype, &mut Archetype)> {
    if a == b || a >= items.len() || b >= items.len() {
        return Err(internal_error("moving an entity within one archetype"));
    }
    if a < b {
        let (left, right) = items.split_at_mut(b);
        match (left.get_mut(a), right.first_mut()) {
            (Some(first), Some(second)) => Ok((first, second)),
            _ => Err(internal_error("archetype index out of range")),
        }
    } else {
        let (left, right) = items.split_at_mut(a);
        match (right.first_mut(), left.get_mut(b)) {
            (Some(first), Some(second)) => Ok((first, second)),
            _ => Err(internal_error("archetype index out of range")),
        }
    }
}

impl World {
    /// An empty world.
    pub fn new() -> Self {
        Self {
            registry: Registry::default(),
            state: WorldState {
                entities: Entities::default(),
                archetypes: vec![Archetype::empty()],
                archetype_ids: BTreeMap::from([(Vec::new(), EMPTY_ARCHETYPE)]),
                resources: Vec::new(),
                texts: TextTable::default(),
            },
        }
    }

    /// The registered component and resource types.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The archetypes, in creation order.
    pub fn archetypes(&self) -> &[Archetype] {
        &self.state.archetypes
    }

    // -----------------------------------------------------------------------
    // Registration
    // -----------------------------------------------------------------------

    /// Registers a component type. Inserting a value registers its type too,
    /// so this is only needed to fix the order of component ids.
    pub fn register_component<T: Component>(&mut self) -> Result<ComponentId> {
        self.registry.register_component::<T>()
    }

    /// Registers a component known only by its schema, as script modules
    /// define them. Registering an identical schema again returns the same id.
    pub fn register_component_schema(&mut self, schema: TypeSchema) -> Result<ComponentId> {
        self.registry.register_component_schema(schema)
    }

    /// Registers a resource type.
    pub fn register_resource<T: Resource>(&mut self) -> Result<ResourceId> {
        self.registry.register_resource::<T>()
    }

    /// Registers a resource known only by its schema.
    pub fn register_resource_schema(&mut self, schema: TypeSchema) -> Result<ResourceId> {
        self.registry.register_resource_schema(schema)
    }

    // -----------------------------------------------------------------------
    // Entities
    // -----------------------------------------------------------------------

    fn location(&self, entity: Entity) -> Result<EntityLocation> {
        self.state
            .entities
            .location(entity)
            .ok_or_else(|| entity_not_found(entity))
    }

    fn archetype(&self, index: usize) -> Result<&Archetype> {
        self.state
            .archetypes
            .get(index)
            .ok_or_else(|| internal_error("an entity points at a missing archetype"))
    }

    /// Creates an entity with no components.
    pub fn spawn(&mut self) -> Result<Entity> {
        let WorldState {
            entities,
            archetypes,
            ..
        } = &mut self.state;
        let Some(archetype) = archetypes.get_mut(EMPTY_ARCHETYPE) else {
            return Err(internal_error("the empty archetype is missing"));
        };
        let entity = entities.alloc(EntityLocation {
            archetype: EMPTY_ARCHETYPE,
            row: archetype.len(),
        })?;
        archetype.entities.push(entity);
        Ok(entity)
    }

    /// Removes an entity and all its components.
    pub fn despawn(&mut self, entity: Entity) -> Result<()> {
        let location = self
            .state
            .entities
            .free(entity)
            .ok_or_else(|| entity_not_found(entity))?;
        let Some(archetype) = self.state.archetypes.get_mut(location.archetype) else {
            return Err(internal_error("an entity points at a missing archetype"));
        };
        if let Some(moved) = archetype.swap_remove(location.row)? {
            self.state.entities.set_location(moved, location);
        }
        Ok(())
    }

    /// Whether the entity exists.
    pub fn contains(&self, entity: Entity) -> bool {
        self.state.entities.location(entity).is_some()
    }

    /// The number of entities.
    pub fn entity_count(&self) -> usize {
        self.state.entities.alive()
    }

    /// Every entity, in ascending index order.
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.state.entities.iter()
    }

    /// The components an entity has, sorted by id.
    pub fn components_of(&self, entity: Entity) -> Result<&[ComponentId]> {
        let location = self.location(entity)?;
        Ok(self.archetype(location.archetype)?.components())
    }

    // -----------------------------------------------------------------------
    // Structural changes
    // -----------------------------------------------------------------------

    fn archetype_for(&mut self, components: Vec<ComponentId>) -> Result<usize> {
        if let Some(&index) = self.state.archetype_ids.get(&components) {
            return Ok(index);
        }
        let archetype = Archetype::new(components.clone(), &self.registry)?;
        let index = self.state.archetypes.len();
        self.state.archetypes.push(archetype);
        self.state.archetype_ids.insert(components, index);
        Ok(index)
    }

    /// Moves an entity's row to another archetype, carrying over the
    /// components both have, adding `added` and dropping the rest.
    fn move_entity(
        &mut self,
        entity: Entity,
        from: EntityLocation,
        target: usize,
        added: Option<(ComponentId, &[u8])>,
    ) -> Result<()> {
        let (source, destination) = pair_mut(&mut self.state.archetypes, from.archetype, target)?;
        let new_row = destination.len();
        for (id, column) in destination
            .components()
            .to_vec()
            .into_iter()
            .zip(&mut destination.columns)
        {
            match added {
                Some((added_id, bytes)) if added_id == id => column.push_bytes(bytes)?,
                _ => {
                    let old = source
                        .column(id)
                        .ok_or_else(|| internal_error("a carried-over component is missing"))?;
                    column.push_from(old, from.row)?;
                }
            }
        }
        destination.entities.push(entity);
        let moved = source.swap_remove(from.row)?;
        if let Some(moved) = moved {
            self.state.entities.set_location(moved, from);
        }
        self.state.entities.set_location(
            entity,
            EntityLocation {
                archetype: target,
                row: new_row,
            },
        );
        Ok(())
    }

    /// Adds or overwrites a component. `bytes` must be a valid value.
    fn put_component(&mut self, entity: Entity, id: ComponentId, bytes: &[u8]) -> Result<()> {
        let location = self.location(entity)?;
        let Some(archetype) = self.state.archetypes.get_mut(location.archetype) else {
            return Err(internal_error("an entity points at a missing archetype"));
        };
        if let Some(column) = archetype.column_mut(id) {
            return column.write_bytes(location.row, bytes);
        }
        let mut components = archetype.components().to_vec();
        if let Err(position) = components.binary_search(&id) {
            components.insert(position, id);
        }
        let target = self.archetype_for(components)?;
        self.move_entity(entity, location, target, Some((id, bytes)))
    }

    /// Removes a component by id. Returns whether the entity had it.
    pub fn remove_by_id(&mut self, entity: Entity, id: ComponentId) -> Result<bool> {
        let location = self.location(entity)?;
        let archetype = self.archetype(location.archetype)?;
        if !archetype.contains(id) {
            return Ok(false);
        }
        let components: Vec<ComponentId> = archetype
            .components()
            .iter()
            .copied()
            .filter(|component| *component != id)
            .collect();
        let target = self.archetype_for(components)?;
        self.move_entity(entity, location, target, None)?;
        Ok(true)
    }

    // -----------------------------------------------------------------------
    // Typed component access
    // -----------------------------------------------------------------------

    /// Adds a component to an entity, or replaces the one it has.
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) -> Result<()> {
        let id = self.registry.register_component::<T>()?;
        let mut bytes = vec![0u8; size_of::<T>()];
        value.encode(&mut bytes);
        self.put_component(entity, id, &bytes)
    }

    /// Removes a component from an entity and returns it. `Ok(None)` when
    /// the entity exists but does not have it.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<Option<T>> {
        let Some(id) = self.registry.component_id_of::<T>() else {
            self.location(entity)?;
            return Ok(None);
        };
        let Some(bytes) = self.component_bytes(entity, id)? else {
            return Ok(None);
        };
        self.remove_by_id(entity, id)?;
        T::decode(&bytes)
            .map(Some)
            .ok_or_else(|| internal_error("a stored component did not decode"))
    }

    /// Whether the entity exists and has the component.
    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.get::<T>(entity).is_some()
    }

    /// The entity's component, if the entity exists and has it.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let id = self.registry.component_id_of::<T>()?;
        let location = self.state.entities.location(entity)?;
        let column = self.state.archetypes.get(location.archetype)?.column(id)?;
        // SAFETY: the column stores component `id`, which the registry maps
        // from `T`'s type id; registration checked that `T`'s size and
        // alignment match the column's layout. The entity's location is kept
        // current by every structural change, so `location.row` is in bounds.
        Some(unsafe { column.get::<T>(location.row) })
    }

    /// The entity's component, mutably.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let id = self.registry.component_id_of::<T>()?;
        let location = self.state.entities.location(entity)?;
        let column = self
            .state
            .archetypes
            .get_mut(location.archetype)?
            .column_mut(id)?;
        // SAFETY: as in `get`. The reference borrows the world mutably, so
        // nothing else can reach the column while it lives.
        Some(unsafe { column.get_mut::<T>(location.row) })
    }

    /// Runs a query over every entity that has the requested components.
    ///
    /// Fails if the query asks for a component mutably and also asks for it
    /// again. Components that were never registered simply match nothing.
    pub fn query<Q: QueryData>(&mut self) -> Result<QueryIter<'_, Q>> {
        self.query_filtered::<Q, ()>()
    }

    /// Runs a query narrowed by a filter, such as `With<Player>` or
    /// `(With<Player>, Without<Frozen>)`.
    pub fn query_filtered<Q: QueryData, F: QueryFilter>(&mut self) -> Result<QueryIter<'_, Q>> {
        let mut access = Access::default();
        Q::access(&mut access);
        access.check()?;
        let Some(ids) = Q::ids(&self.registry) else {
            return Ok(QueryIter::new(Vec::new()));
        };
        let mut tables = Vec::new();
        for archetype in &mut self.state.archetypes {
            if archetype.is_empty() || !F::matches(&self.registry, archetype) {
                continue;
            }
            if let Some(fetch) = Q::fetch(ids, archetype) {
                tables.push((fetch, archetype.len()));
            }
        }
        Ok(QueryIter::new(tables))
    }

    // -----------------------------------------------------------------------
    // Dynamic component access
    // -----------------------------------------------------------------------

    /// The id of the component registered under `name`; the error suggests
    /// the closest registered name.
    pub fn component_id(&self, name: &str) -> Result<ComponentId> {
        self.registry
            .component_id(name)
            .ok_or_else(|| self.registry.unknown_component(name))
    }

    fn component_info(&self, id: ComponentId) -> Result<&TypeInfo> {
        self.registry.component(id).ok_or_else(|| {
            Diagnostic::error(
                "unknown_component_id",
                format!("No component has the id {}.", id.index()),
                "Look component ids up by name in this world; ids are not shared between worlds.",
            )
        })
    }

    /// A copy of the entity's component as bytes, padding zeroed. `Ok(None)`
    /// when the entity exists but does not have it.
    pub fn component_bytes(&self, entity: Entity, id: ComponentId) -> Result<Option<Vec<u8>>> {
        let location = self.location(entity)?;
        let Some(column) = self.archetype(location.archetype)?.column(id) else {
            return Ok(None);
        };
        let info = self.component_info(id)?;
        let mut bytes = vec![0u8; info.size()];
        column.read_into(location.row, info.spans(), &mut bytes)?;
        Ok(Some(bytes))
    }

    /// Adds or replaces a component from raw bytes, for data from outside
    /// native code (script modules). The bytes are checked first: a value
    /// whose `bool` or enum fields hold invalid bytes is rejected.
    pub fn insert_bytes(&mut self, entity: Entity, id: ComponentId, bytes: &[u8]) -> Result<()> {
        validate_bytes(self.component_info(id)?.schema(), bytes)?;
        self.put_component(entity, id, bytes)
    }

    /// The entity's component in JSON form.
    pub fn get_json(&self, entity: Entity, name: &str) -> Result<Value> {
        let id = self.component_id(name)?;
        let Some(bytes) = self.component_bytes(entity, id)? else {
            return Err(missing_component(entity, name));
        };
        to_json(self.component_info(id)?.schema(), &bytes, &self.state.texts)
    }

    /// Sets a component from JSON.
    ///
    /// If the entity has the component, `value` is a patch: fields it names
    /// change and the rest keep their values. Otherwise the component is
    /// added, with fields `value` leaves out at their defaults. Nothing
    /// changes when `value` is rejected.
    pub fn set_json(&mut self, entity: Entity, name: &str, value: &Value) -> Result<()> {
        let id = self.component_id(name)?;
        let current = self.component_bytes(entity, id)?;
        let Self { registry, state } = self;
        let info = registry
            .component(id)
            .ok_or_else(|| internal_error("a component id has no type information"))?;
        let mut bytes = current.unwrap_or_else(|| info.default_bytes().to_vec());
        apply_json(
            info.schema(),
            value,
            &mut bytes,
            &mut JsonContext::new(&mut state.texts),
        )?;
        self.put_component(entity, id, &bytes)
    }

    /// Removes a component by name. Returns whether the entity had it.
    pub fn remove_json(&mut self, entity: Entity, name: &str) -> Result<bool> {
        let id = self.component_id(name)?;
        self.remove_by_id(entity, id)
    }

    /// All of an entity's components in JSON form, keyed by component name.
    pub fn entity_json(&self, entity: Entity) -> Result<Value> {
        let mut components = Map::new();
        for id in self.components_of(entity)? {
            let info = self.component_info(*id)?;
            let bytes = self
                .component_bytes(entity, *id)?
                .ok_or_else(|| internal_error("an archetype component is missing"))?;
            components.insert(
                info.name().to_owned(),
                to_json(info.schema(), &bytes, &self.state.texts)?,
            );
        }
        Ok(Value::Object(components))
    }

    // -----------------------------------------------------------------------
    // Resources
    // -----------------------------------------------------------------------

    fn put_resource(&mut self, id: ResourceId, bytes: &[u8]) -> Result<()> {
        let index = id.index();
        if self.state.resources.len() <= index {
            self.state.resources.resize_with(index + 1, || None);
        }
        match self.state.resources.get_mut(index) {
            Some(Some(column)) => column.write_bytes(0, bytes),
            Some(slot @ None) => {
                let mut column = Column::new(bytes.len());
                column.push_bytes(bytes)?;
                *slot = Some(column);
                Ok(())
            }
            None => Err(internal_error("resource table did not grow")),
        }
    }

    fn resource_column(&self, id: ResourceId) -> Option<&Column> {
        self.state.resources.get(id.index())?.as_ref()
    }

    /// Sets a resource, replacing any previous value.
    pub fn insert_resource<T: Resource>(&mut self, value: T) -> Result<()> {
        let id = self.registry.register_resource::<T>()?;
        let mut bytes = vec![0u8; size_of::<T>()];
        value.encode(&mut bytes);
        self.put_resource(id, &bytes)
    }

    /// The resource's value, if it has one.
    pub fn resource<T: Resource>(&self) -> Option<&T> {
        let id = self.registry.resource_id_of::<T>()?;
        let column = self.resource_column(id)?;
        // SAFETY: a present resource is a one-row column storing resource `id`,
        // which the registry maps from `T`'s type id; registration checked the
        // layout. Row 0 exists.
        Some(unsafe { column.get::<T>(0) })
    }

    /// The resource's value, mutably.
    pub fn resource_mut<T: Resource>(&mut self) -> Option<&mut T> {
        let id = self.registry.resource_id_of::<T>()?;
        let column = self.state.resources.get_mut(id.index())?.as_mut()?;
        // SAFETY: as in `resource`; the reference borrows the world mutably.
        Some(unsafe { column.get_mut::<T>(0) })
    }

    /// Removes the resource's value and returns it.
    pub fn remove_resource<T: Resource>(&mut self) -> Option<T> {
        let id = self.registry.resource_id_of::<T>()?;
        let info = self.registry.resource(id)?;
        let column = self.state.resources.get_mut(id.index())?.take()?;
        let mut bytes = vec![0u8; info.size()];
        column.read_into(0, info.spans(), &mut bytes).ok()?;
        T::decode(&bytes)
    }

    /// A resource in JSON form, by name.
    pub fn resource_json(&self, name: &str) -> Result<Value> {
        let id = self
            .registry
            .resource_id(name)
            .ok_or_else(|| self.registry.unknown_resource(name))?;
        let info = self
            .registry
            .resource(id)
            .ok_or_else(|| internal_error("a resource id has no type information"))?;
        let column = self
            .resource_column(id)
            .ok_or_else(|| missing_resource(name))?;
        let mut bytes = vec![0u8; info.size()];
        column.read_into(0, info.spans(), &mut bytes)?;
        to_json(info.schema(), &bytes, &self.state.texts)
    }

    /// Sets a resource from JSON, by name: a patch if it has a value,
    /// otherwise a new value with omitted fields at their defaults.
    pub fn set_resource_json(&mut self, name: &str, value: &Value) -> Result<()> {
        let id = self
            .registry
            .resource_id(name)
            .ok_or_else(|| self.registry.unknown_resource(name))?;
        let Self { registry, state } = self;
        let info = registry
            .resource(id)
            .ok_or_else(|| internal_error("a resource id has no type information"))?;
        let mut bytes = match state.resources.get(id.index()).and_then(Option::as_ref) {
            Some(column) => {
                let mut current = vec![0u8; info.size()];
                column.read_into(0, info.spans(), &mut current)?;
                current
            }
            None => info.default_bytes().to_vec(),
        };
        apply_json(
            info.schema(),
            value,
            &mut bytes,
            &mut JsonContext::new(&mut state.texts),
        )?;
        self.put_resource(id, &bytes)
    }

    // -----------------------------------------------------------------------
    // Text
    // -----------------------------------------------------------------------

    /// The handle for a string, adding it to the world's text table if new.
    pub fn intern(&mut self, text: &str) -> Result<Text> {
        self.state.texts.intern(text)
    }

    /// The string a handle refers to.
    pub fn text(&self, text: Text) -> Option<&str> {
        self.state.texts.get(text)
    }

    /// The world's text table.
    pub fn texts(&self) -> &TextTable {
        &self.state.texts
    }
}
