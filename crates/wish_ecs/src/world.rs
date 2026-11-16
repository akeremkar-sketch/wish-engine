//! The world: entities, their components, resources, events and text.

// Typed accessors view column bytes as component values.
#![allow(unsafe_code)]

use core::fmt;
use std::collections::BTreeMap;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Entity, Hash128, Result, StableHasher, Text};
use wish_schema::{
    Component, Event, JsonContext, Resource, TypeSchema, apply_json, to_json, validate_bytes,
};

use crate::archetype::Archetype;
use crate::column::Column;
use crate::entities::{Entities, EntityLocation};
use crate::events::{EventCursor, EventQueue};
use crate::extension::{Extension, Extensions};
use crate::internal_error;
use crate::query::{Access, ChangeTick, Mut, QueryData, QueryFilter, QueryIter};
use crate::registry::{ComponentId, EventId, Registry, ResourceId, TypeInfo};
use crate::text::{PendingTexts, TextTable};

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
    /// Indexed by event id; created on the first event of a type.
    events: Vec<Option<EventQueue>>,
    texts: TextTable,
    /// Working state of subsystems (ADR-026); not part of the world hash.
    extensions: Extensions,
    /// Recorded on every write; starts at 1 so that tick 0 precedes everything.
    change_tick: u64,
}

/// A copy of a world's state, to return to later.
///
/// Holds everything that changes while the world runs. It records the
/// layouts of the types registered when it was taken, and can only be
/// restored into a world whose registry starts with exactly those types.
#[derive(Clone, Debug)]
pub struct WorldSnapshot {
    state: WorldState,
    /// How many component, resource and event types were registered.
    counts: [usize; 3],
    /// The fingerprint of those types.
    fingerprint: Hash128,
}

impl WorldSnapshot {
    /// The number of entities in the snapshot.
    pub fn entity_count(&self) -> usize {
        self.state.entities.alive()
    }

    /// The change tick when the snapshot was taken.
    pub fn change_tick(&self) -> ChangeTick {
        ChangeTick(self.state.change_tick)
    }
}

/// What changing a type's layout did to the values stored in a world
/// (`World::retype_component` and its kin).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Retyped {
    /// How many values were moved to the new layout.
    pub values: usize,
    /// Fields whose values could not be kept, each named once: gone from
    /// the new layout, or holding a value the new field does not accept.
    pub dropped: Vec<String>,
}

/// Entities, their components, resources, events and text.
///
/// Typed access (`insert`, `get`, `query`) works with Rust types declared
/// with `#[component]`, `#[resource]` and `#[event]`; they are registered on
/// first use. Dynamic access (`set_json`, `get_json`, `insert_bytes`) works
/// with any registered type by name, which is how AI agents, the editor and
/// script modules reach the world.
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
            .field("events", &self.registry.events().len())
            .field("change_tick", &self.state.change_tick)
            .finish_non_exhaustive()
    }
}

pub(crate) fn entity_not_found(entity: Entity) -> Diagnostic {
    Diagnostic::error(
        "entity_not_found",
        format!("Entity {entity} does not exist."),
        "It was despawned, or it was never spawned in this world. List the entities that exist now and use one of those.",
    )
}

pub(crate) fn managed_component(name: &str) -> Diagnostic {
    Diagnostic::error(
        "managed_component",
        format!("'{name}' is kept consistent by the engine and cannot be changed directly."),
        "Change the hierarchy with set_parent and remove_parent; they keep parent and sibling links in step.",
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

/// Applies a JSON value to `bytes` without touching the text table. Returns
/// the new strings the value refers to, to be committed only if the whole
/// value was accepted, so that a rejected value leaves the world unchanged.
fn apply_json_pending(
    schema: &TypeSchema,
    value: &Value,
    bytes: &mut [u8],
    texts: &TextTable,
) -> Result<Vec<String>> {
    let mut pending = PendingTexts::new(texts);
    apply_json(schema, value, bytes, &mut JsonContext::new(&mut pending))?;
    Ok(pending.into_added())
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
    /// An empty world. The hierarchy component is registered first.
    pub fn new() -> Self {
        let mut world = Self {
            registry: Registry::default(),
            state: WorldState {
                entities: Entities::default(),
                archetypes: vec![Archetype::empty()],
                archetype_ids: BTreeMap::from([(Vec::new(), EMPTY_ARCHETYPE)]),
                resources: Vec::new(),
                events: Vec::new(),
                texts: TextTable::default(),
                extensions: Extensions::default(),
                change_tick: 1,
            },
        };
        // Registering a built-in, macro-generated type cannot fail; if it ever
        // did, hierarchy operations would report the problem when used.
        let registered = world.hierarchy_id().is_ok();
        debug_assert!(registered, "the hierarchy component failed to register");
        world
    }

    /// Stores an extension (ADR-026), replacing one of the same type.
    pub fn insert_extension<T: Extension>(&mut self, value: T) {
        self.state.extensions.insert(value);
    }

    /// The extension of type `T`, if there is one.
    pub fn extension<T: Extension>(&self) -> Option<&T> {
        self.state.extensions.get()
    }

    /// The extension of type `T`, to change. Not tracked: extensions are
    /// working state, never gameplay state.
    pub fn extension_mut<T: Extension>(&mut self) -> Option<&mut T> {
        self.state.extensions.get_mut()
    }

    /// Removes the extension of type `T`; whether there was one.
    pub fn remove_extension<T: Extension>(&mut self) -> bool {
        self.state.extensions.remove::<T>()
    }

    /// The registered component, resource and event types.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    /// The archetypes, in creation order.
    pub fn archetypes(&self) -> &[Archetype] {
        &self.state.archetypes
    }

    // -----------------------------------------------------------------------
    // Change ticks
    // -----------------------------------------------------------------------

    /// The tick recorded by writes made now.
    pub fn change_tick(&self) -> ChangeTick {
        ChangeTick(self.state.change_tick)
    }

    /// Moves to the next change tick and returns it. Writes made before this
    /// call are "before" writes made after it.
    pub fn advance_change_tick(&mut self) -> ChangeTick {
        self.state.change_tick += 1;
        self.change_tick()
    }

    /// The ticks at which an entity's component was added and last written.
    /// `Ok(None)` when the entity exists but does not have it.
    pub fn component_ticks(
        &self,
        entity: Entity,
        id: ComponentId,
    ) -> Result<Option<(ChangeTick, ChangeTick)>> {
        let location = self.location(entity)?;
        let Some(column) = self.archetype(location.archetype)?.column(id) else {
            return Ok(None);
        };
        let (added, changed) = column
            .ticks(location.row)
            .ok_or_else(|| internal_error("a row has no change ticks"))?;
        Ok(Some((ChangeTick(added), ChangeTick(changed))))
    }

    // -----------------------------------------------------------------------
    // Snapshots and hashing
    // -----------------------------------------------------------------------

    /// A copy of the world's state. Cheap enough to take every tick for
    /// small worlds; it copies component data as raw memory.
    pub fn snapshot(&self) -> WorldSnapshot {
        let counts = self.registry.counts();
        WorldSnapshot {
            state: self.state.clone(),
            counts,
            fingerprint: self.registry.fingerprint(),
        }
    }

    /// Returns the world to a snapshot. Types registered after the snapshot
    /// was taken stay registered; nothing else survives.
    ///
    /// Fails, changing nothing, if the snapshot's types do not match the
    /// first types registered in this world.
    pub fn restore(&mut self, snapshot: &WorldSnapshot) -> Result<()> {
        if self.registry.fingerprint_of(snapshot.counts) != Some(snapshot.fingerprint) {
            return Err(Diagnostic::error(
                "snapshot_mismatch",
                "The snapshot was taken in a world whose component, resource or event types differ from this world's.",
                "Restore a snapshot only into the world it came from, or into one that registers the same types in the same order (for example after the same code has run).",
            ));
        }
        self.state = snapshot.state.clone();
        Ok(())
    }

    /// A stable hash of the world's whole state.
    ///
    /// Equal hashes mean the worlds will behave the same from here on, given
    /// the same inputs: everything that affects the future is included
    /// (registered types, entities and the order handles will be reused in,
    /// archetype and row order, component values and change ticks,
    /// resources, buffered events, the text table, the change tick).
    /// Padding bytes are never included.
    ///
    /// It is a hash of *state*, not of meaning: two worlds that contain the
    /// same things but were built in a different order can hash differently.
    pub fn hash(&self) -> Hash128 {
        let mut hasher = StableHasher::new();
        hasher.write_str("wish-world-hash-v1");
        hasher.write_bytes(&self.registry.fingerprint().as_u128().to_le_bytes());
        hasher.write_u64(self.state.change_tick);
        self.state.entities.hash_into(&mut hasher);

        hasher.write_u64(self.state.archetypes.len() as u64);
        for archetype in &self.state.archetypes {
            hasher.write_u64(archetype.components().len() as u64);
            for id in archetype.components() {
                hasher.write_u64(id.index() as u64);
            }
            hasher.write_u64(archetype.entities().len() as u64);
            for entity in archetype.entities() {
                hasher.write_u64(entity.to_bits());
            }
            for (id, column) in archetype.components().iter().zip(&archetype.columns) {
                let spans = self
                    .registry
                    .component(*id)
                    .map_or(&[][..], TypeInfo::spans);
                column.hash_into(spans, &mut hasher);
            }
        }

        hasher.write_u64(self.state.resources.len() as u64);
        for (index, slot) in self.state.resources.iter().enumerate() {
            match slot {
                Some(column) => {
                    hasher.write_u8(1);
                    let spans = self
                        .registry
                        .resources()
                        .nth(index)
                        .map_or(&[][..], |(_, info)| info.spans());
                    column.hash_into(spans, &mut hasher);
                }
                None => hasher.write_u8(0),
            }
        }

        hasher.write_u64(self.state.events.len() as u64);
        for (index, slot) in self.state.events.iter().enumerate() {
            match slot {
                Some(queue) => {
                    hasher.write_u8(1);
                    let spans = self
                        .registry
                        .events()
                        .nth(index)
                        .map_or(&[][..], |(_, info)| info.spans());
                    queue.hash_into(spans, &mut hasher);
                }
                None => hasher.write_u8(0),
            }
        }

        hasher.write_u64(self.state.texts.len() as u64);
        for (_, text) in self.state.texts.iter() {
            hasher.write_str(text);
        }
        hasher.finish()
    }

    // -----------------------------------------------------------------------
    // Registration
    // -----------------------------------------------------------------------

    /// Registers a component type. Inserting a value registers its type too,
    /// so this is only needed to fix the order of component ids, or to make
    /// the type known by name before any value exists.
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

    /// Registers an event type.
    pub fn register_event<T: Event>(&mut self) -> Result<EventId> {
        self.registry.register_event::<T>()
    }

    /// Registers an event type known only by its schema.
    pub fn register_event_schema(&mut self, schema: TypeSchema) -> Result<EventId> {
        self.registry.register_event_schema(schema)
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

    /// Removes an entity, all its components and all its descendants in the
    /// hierarchy.
    pub fn despawn(&mut self, entity: Entity) -> Result<()> {
        self.location(entity)?;
        let descendants = self.descendants(entity);
        self.detach(entity)?;
        self.despawn_one(entity)?;
        for descendant in descendants {
            self.despawn_one(descendant)?;
        }
        Ok(())
    }

    /// Removes one entity's row, ignoring the hierarchy.
    fn despawn_one(&mut self, entity: Entity) -> Result<()> {
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
        let tick = self.state.change_tick;
        let (source, destination) = pair_mut(&mut self.state.archetypes, from.archetype, target)?;
        let new_row = destination.len();
        for (id, column) in destination
            .components()
            .to_vec()
            .into_iter()
            .zip(&mut destination.columns)
        {
            match added {
                Some((added_id, bytes)) if added_id == id => column.push_bytes(bytes, tick)?,
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

    /// Adds or overwrites a component. `bytes` must be a valid value. Does
    /// not check whether the component is managed.
    pub(crate) fn put_component(
        &mut self,
        entity: Entity,
        id: ComponentId,
        bytes: &[u8],
    ) -> Result<()> {
        let tick = self.state.change_tick;
        let location = self.location(entity)?;
        let Some(archetype) = self.state.archetypes.get_mut(location.archetype) else {
            return Err(internal_error("an entity points at a missing archetype"));
        };
        if let Some(column) = archetype.column_mut(id) {
            return column.write_bytes(location.row, bytes, tick);
        }
        let mut components = archetype.components().to_vec();
        if let Err(position) = components.binary_search(&id) {
            components.insert(position, id);
        }
        let target = self.archetype_for(components)?;
        self.move_entity(entity, location, target, Some((id, bytes)))
    }

    fn remove_component(&mut self, entity: Entity, id: ComponentId) -> Result<bool> {
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

    /// Fails if the component is managed by the world.
    fn check_writable(&self, id: ComponentId) -> Result<()> {
        let info = self.component_info(id)?;
        if info.is_managed() {
            Err(managed_component(info.name()))
        } else {
            Ok(())
        }
    }

    /// Removes a component by id. Returns whether the entity had it.
    pub fn remove_by_id(&mut self, entity: Entity, id: ComponentId) -> Result<bool> {
        self.check_writable(id)?;
        self.remove_component(entity, id)
    }

    // -----------------------------------------------------------------------
    // Typed component access
    // -----------------------------------------------------------------------

    /// Adds a component to an entity, or replaces the one it has.
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) -> Result<()> {
        let id = self.registry.register_component::<T>()?;
        self.check_writable(id)?;
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
        self.check_writable(id)?;
        let Some(bytes) = self.component_bytes(entity, id)? else {
            return Ok(None);
        };
        self.remove_component(entity, id)?;
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

    /// The entity's component, mutably. Writing through the result records
    /// the component as changed.
    ///
    /// `None` also for components managed by the world (such as the
    /// hierarchy), which can only be read.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<'_, T>> {
        let id = self.registry.component_id_of::<T>()?;
        if self.registry.component(id)?.is_managed() {
            return None;
        }
        let tick = self.state.change_tick;
        let location = self.state.entities.location(entity)?;
        let column = self
            .state
            .archetypes
            .get_mut(location.archetype)?
            .column_mut(id)?;
        // SAFETY: as in `get`. The references borrow the world mutably, so
        // nothing else can reach the column while they live.
        let (value, changed) = unsafe { column.get_mut::<T>(location.row) };
        Some(Mut::new(value, changed, tick))
    }

    /// Runs a query over every entity that has the requested components.
    ///
    /// Fails if the query asks for a component mutably and also asks for it
    /// again, or asks for a managed component mutably. Components that were
    /// never registered simply match nothing.
    pub fn query<Q: QueryData>(&mut self) -> Result<QueryIter<'_, Q>> {
        self.query_since::<Q, ()>(ChangeTick::ZERO)
    }

    /// Runs a query narrowed by a filter, such as `With<Player>` or
    /// `(With<Player>, Without<Frozen>)`. `Changed` and `Added` filters
    /// compare against tick 0 here, so they pass anything ever written or
    /// added; use [`World::query_since`] to look for recent changes.
    pub fn query_filtered<Q: QueryData, F: QueryFilter>(&mut self) -> Result<QueryIter<'_, Q, F>> {
        self.query_since::<Q, F>(ChangeTick::ZERO)
    }

    /// Runs a filtered query whose `Changed` and `Added` filters pass only
    /// components written or added after `since`.
    pub fn query_since<Q: QueryData, F: QueryFilter>(
        &mut self,
        since: ChangeTick,
    ) -> Result<QueryIter<'_, Q, F>> {
        let mut access = Access::default();
        Q::access(&mut access);
        access.check(&self.registry)?;
        let Some(ids) = Q::ids(&self.registry) else {
            return Ok(QueryIter::new(Vec::new()));
        };
        let tick = self.state.change_tick;
        let mut tables = Vec::new();
        for archetype in &mut self.state.archetypes {
            if archetype.is_empty() {
                continue;
            }
            let Some(filter) = F::fetch(&self.registry, archetype, since.0) else {
                continue;
            };
            if let Some(fetch) = Q::fetch(ids, archetype, tick) {
                tables.push((fetch, filter, archetype.len()));
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

    /// Adds the strings collected while converting an accepted JSON value.
    /// They get exactly the handles the value was given (see `PendingTexts`).
    fn commit_texts(&mut self, added: Vec<String>) -> Result<()> {
        for text in added {
            self.state.texts.intern(&text)?;
        }
        Ok(())
    }

    /// Checks raw bytes from outside native code against a type: `bool` and
    /// enum fields must hold valid bytes, and text handles must be in this
    /// world's text table.
    fn check_outside_bytes(&self, info: &TypeInfo, bytes: &[u8]) -> Result<()> {
        validate_bytes(info.schema(), bytes)?;
        for offset in info.text_offsets() {
            let handle = bytes
                .get(*offset..offset + 4)
                .and_then(|slot| <[u8; 4]>::try_from(slot).ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| internal_error("a text offset lies outside its value"))?;
            if handle as usize >= self.state.texts.len() {
                return Err(Diagnostic::error(
                    "invalid_text_handle",
                    format!(
                        "The value refers to text handle {handle}, but this world's text table has {} entries.",
                        self.state.texts.len()
                    ),
                    "Text handles only have meaning in the world that issued them. Intern the string in this world first, or set the component through JSON with the string itself.",
                ));
            }
        }
        Ok(())
    }

    /// Adds or replaces a component from raw bytes, for data from outside
    /// native code (script modules). The bytes are checked first: a value
    /// whose `bool` or enum fields hold invalid bytes, or whose text handles
    /// are not in this world's text table, is rejected.
    pub fn insert_bytes(&mut self, entity: Entity, id: ComponentId, bytes: &[u8]) -> Result<()> {
        self.check_writable(id)?;
        let info = self.component_info(id)?;
        self.check_outside_bytes(info, bytes)?;
        self.put_component(entity, id, bytes)
    }

    /// Copies the entity's component into `out` (exactly the component's
    /// size), padding zeroed. Returns `Ok(false)` when the entity exists but
    /// does not have it. Unlike [`World::component_bytes`] it allocates
    /// nothing, for copying many values in a row.
    pub fn component_bytes_into(
        &self,
        entity: Entity,
        id: ComponentId,
        out: &mut [u8],
    ) -> Result<bool> {
        let location = self.location(entity)?;
        let Some(column) = self.archetype(location.archetype)?.column(id) else {
            return Ok(false);
        };
        let info = self.component_info(id)?;
        out.fill(0);
        column.read_into(location.row, info.spans(), out)?;
        Ok(true)
    }

    /// Appends component `id` of every entity in archetype `archetype` (the
    /// index into [`World::archetypes`]) to `out`, packed in row order,
    /// padding zeroed: one column slice in one call, for passing many values
    /// to a game module (M5 WP7). The entities are the archetype's
    /// [`Archetype::entities`], in the same order.
    pub fn archetype_column_bytes(
        &self,
        archetype: usize,
        id: ComponentId,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        let info = self.component_info(id)?;
        let column = self
            .archetype(archetype)?
            .column(id)
            .ok_or_else(|| internal_error("the archetype does not store that component"))?;
        let size = info.size();
        let start = out.len();
        out.resize(start + column.len() * size, 0);
        for row in 0..column.len() {
            let at = start + row * size;
            column.read_into(row, info.spans(), &mut out[at..at + size])?;
        }
        Ok(())
    }

    /// Writes back component `id` of every entity in archetype `archetype`
    /// from `bytes` (packed in row order, as [`World::archetype_column_bytes`]
    /// gave them): only the values whose bytes differ are written, and only
    /// those count as changed. Values are checked like
    /// [`World::insert_bytes`] when the type has fields that need it.
    /// Returns how many values changed.
    pub fn write_archetype_column(
        &mut self,
        archetype: usize,
        id: ComponentId,
        bytes: &[u8],
    ) -> Result<usize> {
        self.check_writable(id)?;
        let info = self.component_info(id)?.clone();
        let size = info.size();
        let check = info.schema().needs_validation() || !info.text_offsets().is_empty();
        let rows = self
            .archetype(archetype)?
            .column(id)
            .map(Column::len)
            .ok_or_else(|| internal_error("the archetype does not store that component"))?;
        if bytes.len() != rows * size {
            return Err(internal_error("column bytes have the wrong length"));
        }
        // Find the changed rows first (and check them), then write: a value
        // that is refused leaves the whole column as it was.
        let mut current = vec![0u8; size];
        let mut changed = Vec::new();
        {
            let column = self
                .archetype(archetype)?
                .column(id)
                .ok_or_else(|| internal_error("the archetype does not store that component"))?;
            for row in 0..rows {
                let new = &bytes[row * size..(row + 1) * size];
                current.fill(0);
                column.read_into(row, info.spans(), &mut current)?;
                if new != current.as_slice() {
                    if check {
                        self.check_outside_bytes(&info, new)?;
                    }
                    changed.push(row);
                }
            }
        }
        let tick = self.state.change_tick;
        let column = self
            .state
            .archetypes
            .get_mut(archetype)
            .and_then(|a| a.column_mut(id))
            .ok_or_else(|| internal_error("the archetype does not store that component"))?;
        for row in &changed {
            column.write_bytes(*row, &bytes[row * size..(row + 1) * size], tick)?;
        }
        Ok(changed.len())
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
        self.check_writable(id)?;
        let current = self.component_bytes(entity, id)?;
        let Self { registry, state } = self;
        let info = registry
            .component(id)
            .ok_or_else(|| internal_error("a component id has no type information"))?;
        let mut bytes = current.unwrap_or_else(|| info.default_bytes().to_vec());
        let added = apply_json_pending(info.schema(), value, &mut bytes, &state.texts)?;
        self.commit_texts(added)?;
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
    // Changing a type's layout (game code reloaded during a game)
    // -----------------------------------------------------------------------

    /// One value moved from `old` to `new` by field name: fields with the
    /// same name keep their value if it fits the new field; new fields take
    /// their defaults; the rest are named in `dropped`.
    fn moved_value(
        &mut self,
        old: &TypeSchema,
        old_bytes: &[u8],
        new: &TypeInfo,
        dropped: &mut std::collections::BTreeSet<String>,
    ) -> Result<Vec<u8>> {
        let value = to_json(old, old_bytes, &self.state.texts)?;
        let mut bytes = new.default_bytes().to_vec();
        let patches: Vec<(Option<String>, Value)> = match (&value, &new.schema().kind) {
            (Value::Object(fields), wish_schema::TypeKind::Struct(new_fields)) => fields
                .iter()
                .map(|(name, field)| {
                    let patch = if new_fields.iter().any(|f| &f.name == name) {
                        Value::Object(Map::from_iter([(name.clone(), field.clone())]))
                    } else {
                        Value::Null
                    };
                    (Some(name.clone()), patch)
                })
                .collect(),
            _ => vec![(None, value)],
        };
        for (name, patch) in patches {
            let label = name.unwrap_or_else(|| "(value)".to_owned());
            if patch.is_null() {
                dropped.insert(label);
                continue;
            }
            let mut attempt = bytes.clone();
            match apply_json_pending(new.schema(), &patch, &mut attempt, &self.state.texts) {
                Ok(added) => {
                    self.commit_texts(added)?;
                    bytes = attempt;
                }
                Err(_) => {
                    dropped.insert(label);
                }
            }
        }
        Ok(bytes)
    }

    fn check_retype(info: &TypeInfo, schema: &TypeSchema) -> Result<TypeInfo> {
        if info.is_native() || info.is_managed() || info.name() != schema.name {
            return Err(Diagnostic::error(
                "retype_refused",
                format!(
                    "'{}' is the engine's own type; its layout cannot change while the world exists.",
                    info.name()
                ),
                "Only types declared by game code can change their fields during a game. Rename the game's type if it clashes with an engine type.",
            ));
        }
        TypeInfo::new(schema.clone(), None)
    }

    /// Changes the layout of a component declared by game code, moving every
    /// stored value field by field (same name, a value that fits: kept; new
    /// fields: their defaults; the others: reported in
    /// [`Retyped::dropped`]). When each value was added and last changed is
    /// kept. Engine types (bound to a Rust type) are refused.
    pub fn retype_component(&mut self, id: ComponentId, schema: TypeSchema) -> Result<Retyped> {
        schema.validate()?;
        let old = self.component_info(id)?.clone();
        let new = Self::check_retype(&old, &schema)?;
        let mut stored = Vec::new();
        for (a, archetype) in self.state.archetypes.iter().enumerate() {
            let Some(c) = archetype.components().iter().position(|x| *x == id) else {
                continue;
            };
            let column = archetype
                .columns
                .get(c)
                .ok_or_else(|| internal_error("an archetype lacks a column"))?;
            let mut rows = Vec::with_capacity(column.len());
            for row in 0..column.len() {
                let mut bytes = vec![0u8; old.size()];
                column.read_into(row, old.spans(), &mut bytes)?;
                let (added, changed) = column
                    .ticks(row)
                    .ok_or_else(|| internal_error("a row has no ticks"))?;
                rows.push((bytes, added, changed));
            }
            stored.push((a, c, rows));
        }
        let mut dropped = std::collections::BTreeSet::new();
        let mut values = 0;
        let mut columns = Vec::with_capacity(stored.len());
        for (a, c, rows) in stored {
            let mut column = Column::new(new.size());
            for (bytes, added, changed) in rows {
                let moved = self.moved_value(old.schema(), &bytes, &new, &mut dropped)?;
                column.push_with_ticks(&moved, added, changed)?;
                values += 1;
            }
            columns.push((a, c, column));
        }
        self.registry.replace_component_schema(id, schema)?;
        for (a, c, column) in columns {
            if let Some(slot) = self
                .state
                .archetypes
                .get_mut(a)
                .and_then(|archetype| archetype.columns.get_mut(c))
            {
                *slot = column;
            }
        }
        Ok(Retyped {
            values,
            dropped: dropped.into_iter().collect(),
        })
    }

    /// Changes the layout of a resource declared by game code, moving its
    /// value like [`World::retype_component`].
    pub fn retype_resource(&mut self, id: ResourceId, schema: TypeSchema) -> Result<Retyped> {
        schema.validate()?;
        let old = self.resource_info(id)?.clone();
        let new = Self::check_retype(&old, &schema)?;
        let current = match self.resource_column(id) {
            Some(column) => {
                let mut bytes = vec![0u8; old.size()];
                column.read_into(0, old.spans(), &mut bytes)?;
                let ticks = column
                    .ticks(0)
                    .ok_or_else(|| internal_error("a resource has no ticks"))?;
                Some((bytes, ticks))
            }
            None => None,
        };
        let mut dropped = std::collections::BTreeSet::new();
        let moved = match &current {
            Some((bytes, (added, changed))) => {
                let moved = self.moved_value(old.schema(), bytes, &new, &mut dropped)?;
                let mut column = Column::new(new.size());
                column.push_with_ticks(&moved, *added, *changed)?;
                Some(column)
            }
            None => None,
        };
        self.registry.replace_resource_schema(id, schema)?;
        let values = usize::from(moved.is_some());
        if let Some(column) = moved
            && let Some(slot) = self.state.resources.get_mut(id.index())
        {
            *slot = Some(column);
        }
        Ok(Retyped {
            values,
            dropped: dropped.into_iter().collect(),
        })
    }

    /// Changes the layout of an event declared by game code. Events still
    /// buffered in the old layout are dropped (they were for the old code).
    pub fn retype_event(&mut self, id: EventId, schema: TypeSchema) -> Result<Retyped> {
        schema.validate()?;
        let old = self.event_info(id)?.clone();
        Self::check_retype(&old, &schema)?;
        self.registry.replace_event_schema(id, schema)?;
        let had = self
            .state
            .events
            .get_mut(id.index())
            .and_then(Option::take)
            .is_some();
        Ok(Retyped {
            values: 0,
            dropped: if had {
                vec!["(buffered events)".to_owned()]
            } else {
                Vec::new()
            },
        })
    }

    // -----------------------------------------------------------------------
    // Resources
    // -----------------------------------------------------------------------

    fn put_resource(&mut self, id: ResourceId, bytes: &[u8]) -> Result<()> {
        let tick = self.state.change_tick;
        let index = id.index();
        if self.state.resources.len() <= index {
            self.state.resources.resize_with(index + 1, || None);
        }
        match self.state.resources.get_mut(index) {
            Some(Some(column)) => column.write_bytes(0, bytes, tick),
            Some(slot @ None) => {
                let mut column = Column::new(bytes.len());
                column.push_bytes(bytes, tick)?;
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

    /// The resource's value, mutably. Writing records it as changed.
    pub fn resource_mut<T: Resource>(&mut self) -> Option<Mut<'_, T>> {
        let id = self.registry.resource_id_of::<T>()?;
        let tick = self.state.change_tick;
        let column = self.state.resources.get_mut(id.index())?.as_mut()?;
        // SAFETY: as in `resource`; the references borrow the world mutably.
        let (value, changed) = unsafe { column.get_mut::<T>(0) };
        Some(Mut::new(value, changed, tick))
    }

    fn resource_info(&self, id: ResourceId) -> Result<&TypeInfo> {
        self.registry.resource(id).ok_or_else(|| {
            Diagnostic::error(
                "unknown_resource_id",
                format!("No resource has the id {}.", id.index()),
                "Look resource ids up by name in this world; ids are not shared between worlds.",
            )
        })
    }

    /// A copy of the resource's value as bytes, padding zeroed; `Ok(None)`
    /// when it has no value.
    pub fn resource_bytes(&self, id: ResourceId) -> Result<Option<Vec<u8>>> {
        let info = self.resource_info(id)?;
        let Some(column) = self.resource_column(id) else {
            return Ok(None);
        };
        let mut bytes = vec![0u8; info.size()];
        column.read_into(0, info.spans(), &mut bytes)?;
        Ok(Some(bytes))
    }

    /// Sets a resource from raw bytes, for data from outside native code
    /// (script modules), checked like [`World::insert_bytes`].
    pub fn insert_resource_bytes(&mut self, id: ResourceId, bytes: &[u8]) -> Result<()> {
        let info = self.resource_info(id)?;
        if bytes.len() != info.size() {
            return Err(internal_error("resource bytes have the wrong length"));
        }
        self.check_outside_bytes(info, bytes)?;
        self.put_resource(id, bytes)
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

    /// Removes a resource's value, by name. Returns whether it had one.
    pub fn remove_resource_json(&mut self, name: &str) -> Result<bool> {
        let id = self
            .registry
            .resource_id(name)
            .ok_or_else(|| self.registry.unknown_resource(name))?;
        Ok(self
            .state
            .resources
            .get_mut(id.index())
            .and_then(Option::take)
            .is_some())
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
        let added = apply_json_pending(info.schema(), value, &mut bytes, &state.texts)?;
        self.commit_texts(added)?;
        self.put_resource(id, &bytes)
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    fn push_event(&mut self, id: EventId, bytes: &[u8]) -> Result<()> {
        let tick = self.state.change_tick;
        let index = id.index();
        if self.state.events.len() <= index {
            self.state.events.resize_with(index + 1, || None);
        }
        let Some(slot) = self.state.events.get_mut(index) else {
            return Err(internal_error("event table did not grow"));
        };
        slot.get_or_insert_with(|| EventQueue::new(bytes.len()))
            .push(bytes, tick)
    }

    /// Sends an event. It can be read during this update and the next (see
    /// [`World::update_events`]).
    pub fn send<E: Event>(&mut self, event: E) -> Result<()> {
        let id = self.registry.register_event::<E>()?;
        let mut bytes = vec![0u8; size_of::<E>()];
        event.encode(&mut bytes);
        self.push_event(id, &bytes)
    }

    /// Sends an event from JSON, by type name; fields left out take their
    /// default values.
    pub fn send_json(&mut self, name: &str, value: &Value) -> Result<()> {
        let id = self
            .registry
            .event_id(name)
            .ok_or_else(|| self.registry.unknown_event(name))?;
        let Self { registry, state } = self;
        let info = registry
            .event(id)
            .ok_or_else(|| internal_error("an event id has no type information"))?;
        let mut bytes = info.default_bytes().to_vec();
        let added = apply_json_pending(info.schema(), value, &mut bytes, &state.texts)?;
        self.commit_texts(added)?;
        self.push_event(id, &bytes)
    }

    /// The buffered events of one type with a sequence number of at least
    /// `from`, as bytes, oldest first.
    fn event_bytes(&self, id: EventId, from: u64) -> Result<Vec<Vec<u8>>> {
        let Some(queue) = self.state.events.get(id.index()).and_then(Option::as_ref) else {
            return Ok(Vec::new());
        };
        let info = self
            .registry
            .event(id)
            .ok_or_else(|| internal_error("an event id has no type information"))?;
        queue
            .rows_from(from)
            .map(|(column, row)| {
                let mut bytes = vec![0u8; info.size()];
                column.read_into(row, info.spans(), &mut bytes)?;
                Ok(bytes)
            })
            .collect()
    }

    fn decode_events<E: Event>(byte_lists: Vec<Vec<u8>>) -> Result<Vec<E>> {
        byte_lists
            .iter()
            .map(|bytes| {
                E::decode(bytes).ok_or_else(|| internal_error("a stored event did not decode"))
            })
            .collect()
    }

    fn event_info(&self, id: EventId) -> Result<&TypeInfo> {
        self.registry.event(id).ok_or_else(|| {
            Diagnostic::error(
                "unknown_event_id",
                format!("No event has the id {}.", id.index()),
                "Look event ids up by name in this world; ids are not shared between worlds.",
            )
        })
    }

    /// Sends an event from raw bytes, for data from outside native code
    /// (script modules), checked like [`World::insert_bytes`].
    pub fn send_bytes(&mut self, id: EventId, bytes: &[u8]) -> Result<()> {
        let info = self.event_info(id)?;
        if bytes.len() != info.size() {
            return Err(internal_error("event bytes have the wrong length"));
        }
        self.check_outside_bytes(info, bytes)?;
        self.push_event(id, bytes)
    }

    /// The buffered events of type `id` sent after change tick `since`, as
    /// bytes (padding zeroed), oldest first. For readers that remember where
    /// they are as a change tick: a game module's system sees the events
    /// sent since its previous run (`SystemContext::last_run`), and that
    /// position is part of the app's snapshot.
    pub fn event_bytes_since(&self, id: EventId, since: ChangeTick) -> Result<Vec<Vec<u8>>> {
        let info = self.event_info(id)?;
        let Some(queue) = self.state.events.get(id.index()).and_then(Option::as_ref) else {
            return Ok(Vec::new());
        };
        queue
            .rows_from(0)
            .filter(|(column, row)| column.ticks(*row).is_some_and(|(added, _)| added > since.0))
            .map(|(column, row)| {
                let mut bytes = vec![0u8; info.size()];
                column.read_into(row, info.spans(), &mut bytes)?;
                Ok(bytes)
            })
            .collect()
    }

    /// Every buffered event of type `E`, oldest first.
    pub fn events<E: Event>(&self) -> Result<Vec<E>> {
        let Some(id) = self.registry.event_id_of::<E>() else {
            return Ok(Vec::new());
        };
        Self::decode_events(self.event_bytes(id, 0)?)
    }

    /// The events of type `E` the cursor has not seen yet, oldest first; the
    /// cursor moves past them. Events dropped by [`World::update_events`]
    /// before the cursor read them are skipped.
    pub fn read_events<E: Event>(&self, cursor: &mut EventCursor<E>) -> Result<Vec<E>> {
        let Some(id) = self.registry.event_id_of::<E>() else {
            return Ok(Vec::new());
        };
        let events = Self::decode_events(self.event_bytes(id, cursor.next)?)?;
        if let Some(queue) = self.state.events.get(id.index()).and_then(Option::as_ref) {
            *cursor = EventCursor::at(queue.next_sequence());
        }
        Ok(events)
    }

    /// Every buffered event of a type, by name, as a JSON array, oldest first.
    pub fn events_json(&self, name: &str) -> Result<Value> {
        let id = self
            .registry
            .event_id(name)
            .ok_or_else(|| self.registry.unknown_event(name))?;
        let info = self
            .registry
            .event(id)
            .ok_or_else(|| internal_error("an event id has no type information"))?;
        let events = self
            .event_bytes(id, 0)?
            .iter()
            .map(|bytes| to_json(info.schema(), bytes, &self.state.texts))
            .collect::<Result<Vec<_>>>()?;
        Ok(Value::Array(events))
    }

    /// How many events of the type called `name` have been sent since the
    /// world was created (a running total: buffered ones dropped by
    /// [`World::update_events`] still count). Taking the difference of two
    /// readings says how many were sent in between.
    pub fn events_sent(&self, name: &str) -> Result<u64> {
        let id = self
            .registry
            .event_id(name)
            .ok_or_else(|| self.registry.unknown_event(name))?;
        Ok(self
            .state
            .events
            .get(id.index())
            .and_then(Option::as_ref)
            .map_or(0, EventQueue::next_sequence))
    }

    /// Ends an update for events: events sent before the previous call are
    /// dropped. Call it once per update (the scheduler does).
    pub fn update_events(&mut self) {
        for queue in self.state.events.iter_mut().flatten() {
            queue.update();
        }
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
