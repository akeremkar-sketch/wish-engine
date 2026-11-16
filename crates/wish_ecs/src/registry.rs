//! The registry of component and resource types a world knows about.

use std::any::TypeId;
use std::collections::BTreeMap;

use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Hash128, Result};
use wish_schema::{
    Component, Event, Plain, Resource, TypeKind, TypeSchema, default_bytes, fingerprint,
};

use crate::internal_error;

/// Identifies a component type within one world. Ids are handed out in
/// registration order, starting at 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentId(u32);

impl ComponentId {
    /// The id's position in registration order.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies a resource type within one world. Ids are handed out in
/// registration order, starting at 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(u32);

impl ResourceId {
    /// The id's position in registration order.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies an event type within one world. Ids are handed out in
/// registration order, starting at 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(u32);

impl EventId {
    /// The id's position in registration order.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Everything the world knows about one registered type.
#[derive(Clone, Debug)]
pub struct TypeInfo {
    schema: TypeSchema,
    fingerprint: Hash128,
    /// Byte ranges `(offset, len)` of the value that belong to fields, merged
    /// and sorted. Only these bytes are ever read as bytes: the rest is
    /// padding and may be uninitialised in storage.
    spans: Vec<(usize, usize)>,
    /// Offsets of every `Text` handle inside a value.
    text_offsets: Vec<usize>,
    default: Vec<u8>,
    type_id: Option<TypeId>,
    /// Kept consistent by the world itself (like the hierarchy links), so it
    /// cannot be written, added or removed directly.
    managed: bool,
}

impl TypeInfo {
    pub(crate) fn new(schema: TypeSchema, type_id: Option<TypeId>) -> Result<Self> {
        schema.validate()?;
        let default = default_bytes(&schema)?;
        let mut spans = Vec::new();
        collect_spans(&schema, 0, &mut spans);
        let mut text_offsets = Vec::new();
        collect_text_offsets(&schema, 0, &mut text_offsets);
        Ok(Self {
            fingerprint: fingerprint(&schema),
            spans: merge_spans(spans),
            text_offsets,
            schema,
            default,
            type_id,
            managed: false,
        })
    }

    /// Whether the world keeps this component consistent itself, so that it
    /// can only be read (for example the hierarchy links).
    pub fn is_managed(&self) -> bool {
        self.managed
    }

    /// The type's name.
    pub fn name(&self) -> &str {
        &self.schema.name
    }

    /// The type's schema.
    pub fn schema(&self) -> &TypeSchema {
        &self.schema
    }

    /// The size of one value in bytes.
    pub fn size(&self) -> usize {
        self.schema.byte_len()
    }

    /// The fingerprint of the type's layout.
    pub fn fingerprint(&self) -> Hash128 {
        self.fingerprint
    }

    /// The bytes of a new value with every field at its default.
    pub fn default_bytes(&self) -> &[u8] {
        &self.default
    }

    /// Whether the type was registered from a Rust type (as opposed to a
    /// schema alone, as script modules do).
    pub fn is_native(&self) -> bool {
        self.type_id.is_some()
    }

    pub(crate) fn spans(&self) -> &[(usize, usize)] {
        &self.spans
    }

    pub(crate) fn text_offsets(&self) -> &[usize] {
        &self.text_offsets
    }
}

fn collect_text_offsets(schema: &TypeSchema, base: usize, out: &mut Vec<usize>) {
    match &schema.kind {
        TypeKind::Text | TypeKind::AssetRef => out.push(base),
        TypeKind::Array { element, len } => {
            if element.contains_text() {
                for index in 0..*len as usize {
                    collect_text_offsets(element, base + index * element.byte_len(), out);
                }
            }
        }
        TypeKind::Struct(fields) => {
            for field in fields {
                collect_text_offsets(&field.ty, base + field.offset as usize, out);
            }
        }
        _ => {}
    }
}

fn collect_spans(schema: &TypeSchema, base: usize, out: &mut Vec<(usize, usize)>) {
    match &schema.kind {
        TypeKind::Array { element, len } => {
            for index in 0..*len as usize {
                collect_spans(element, base + index * element.byte_len(), out);
            }
        }
        TypeKind::Struct(fields) => {
            for field in fields {
                collect_spans(&field.ty, base + field.offset as usize, out);
            }
        }
        _ => {
            if schema.size > 0 {
                out.push((base, schema.byte_len()));
            }
        }
    }
}

fn merge_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (offset, len) in spans {
        match merged.last_mut() {
            Some(last) if last.0 + last.1 == offset => last.1 += len,
            _ => merged.push((offset, len)),
        }
    }
    merged
}

/// One kind of registered type: components or resources.
#[derive(Clone, Debug)]
struct Table {
    /// "component" or "resource", for messages.
    kind: &'static str,
    /// The diagnostic code for an unknown name.
    unknown_code: &'static str,
    infos: Vec<TypeInfo>,
    by_name: BTreeMap<String, u32>,
    by_type: BTreeMap<TypeId, u32>,
}

impl Table {
    fn new(kind: &'static str, unknown_code: &'static str) -> Self {
        Self {
            kind,
            unknown_code,
            infos: Vec::new(),
            by_name: BTreeMap::new(),
            by_type: BTreeMap::new(),
        }
    }

    fn get(&self, index: u32) -> Option<&TypeInfo> {
        self.infos.get(index as usize)
    }

    fn by_name(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    fn by_type(&self, type_id: TypeId) -> Option<u32> {
        self.by_type.get(&type_id).copied()
    }

    fn register_typed<T: Plain>(&mut self) -> Result<u32> {
        let type_id = TypeId::of::<T>();
        if let Some(index) = self.by_type(type_id) {
            return Ok(index);
        }
        let schema = T::schema();
        // Typed access views stored bytes as `T`, so the layout the column is
        // built from must be exactly `T`'s. The `Plain` contract promises
        // this; checking it once here costs nothing.
        if size_of::<T>() != schema.byte_len()
            || align_of::<T>() != schema.align as usize
            || align_of::<T>() > 8
        {
            return Err(internal_error(format!(
                "the schema of '{}' does not describe the Rust type's layout",
                schema.name
            )));
        }
        schema.validate()?;
        if let Some(index) = self.by_name(&schema.name) {
            // Binding a Rust type to storage registered from a schema decides
            // whether those bytes may be viewed as `T`, so the layouts are
            // compared structurally, never by hash.
            if let Some(info) = self.infos.get_mut(index as usize)
                && info.type_id.is_none()
                && info.schema.same_layout(&schema)
            {
                // Registered earlier from its schema alone; now the Rust type is known too.
                info.type_id = Some(type_id);
                self.by_type.insert(type_id, index);
                return Ok(index);
            }
            return Err(self.conflict(&schema.name));
        }
        self.push(TypeInfo::new(schema, Some(type_id))?)
    }

    fn register_schema(&mut self, schema: TypeSchema) -> Result<u32> {
        // Validate before anything else walks the schema: it may come from an
        // untrusted script module.
        schema.validate()?;
        if let Some(index) = self.by_name(&schema.name) {
            return match self.get(index) {
                Some(info) if info.schema.same_layout(&schema) => Ok(index),
                _ => Err(self.conflict(&schema.name)),
            };
        }
        self.push(TypeInfo::new(schema, None)?)
    }

    /// The index of the type `T` registered as `name`: bound to `T` by type
    /// id, or registered from a schema alone with exactly `T`'s layout (so
    /// that a world built from schemas behaves the same as one built from
    /// Rust types).
    fn lookup<T: Plain>(&self, name: &str) -> Option<u32> {
        if let Some(index) = self.by_type(TypeId::of::<T>()) {
            return Some(index);
        }
        let index = self.by_name(name)?;
        let info = self.get(index)?;
        let fits = info.type_id.is_none()
            && size_of::<T>() == info.size()
            && align_of::<T>() == info.schema.align as usize
            && align_of::<T>() <= 8
            && info.schema.same_layout(&T::schema());
        fits.then_some(index)
    }

    fn push(&mut self, info: TypeInfo) -> Result<u32> {
        let index = u32::try_from(self.infos.len())
            .map_err(|_| internal_error("too many registered types"))?;
        self.by_name.insert(info.name().to_owned(), index);
        if let Some(type_id) = info.type_id {
            self.by_type.insert(type_id, index);
        }
        self.infos.push(info);
        Ok(index)
    }

    /// Replaces the description of a type registered from a schema alone
    /// (game code). Types bound to a Rust type, and managed ones, keep
    /// theirs: native code views their bytes directly.
    fn replace_schema(&mut self, index: u32, schema: TypeSchema) -> Result<()> {
        schema.validate()?;
        let Some(info) = self.infos.get_mut(index as usize) else {
            return Err(internal_error("replacing an unregistered type"));
        };
        if info.type_id.is_some() || info.managed || info.name() != schema.name {
            return Err(Diagnostic::error(
                "retype_refused",
                format!(
                    "The {} '{}' is the engine's own type; its layout cannot change while the world exists.",
                    self.kind,
                    info.name()
                ),
                "Only types declared by game code can change their fields during a game. Rename the game's type if it clashes with an engine type.",
            ));
        }
        *info = TypeInfo::new(schema, None)?;
        Ok(())
    }

    fn conflict(&self, name: &str) -> Diagnostic {
        Diagnostic::error(
            "type_name_conflict",
            format!(
                "A different {} named '{name}' is already registered.",
                self.kind
            ),
            "Two types with the same name but different fields cannot both be registered. Rename one of them.",
        )
    }

    fn unknown(&self, name: &str) -> Diagnostic {
        let names: Vec<&str> = self.infos.iter().map(TypeInfo::name).collect();
        Diagnostic::error(
            self.unknown_code,
            format!("No {} named '{name}' is registered.", self.kind),
            unknown_name_hint(name, &names),
        )
        .with_candidates(names)
    }
}

/// The component and resource types a world knows about.
///
/// Types are registered on first use (inserting a value of a Rust type
/// registers it) or explicitly from a schema. Registration only ever adds.
#[derive(Clone, Debug)]
pub struct Registry {
    components: Table,
    resources: Table,
    events: Table,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            components: Table::new("component", "unknown_component"),
            resources: Table::new("resource", "unknown_resource"),
            events: Table::new("event", "unknown_event"),
        }
    }
}

impl Registry {
    /// Information about a registered component.
    pub fn component(&self, id: ComponentId) -> Option<&TypeInfo> {
        self.components.get(id.0)
    }

    /// The id of the component registered under `name`.
    pub fn component_id(&self, name: &str) -> Option<ComponentId> {
        self.components.by_name(name).map(ComponentId)
    }

    /// The id of the component type `T`, if it has been registered (as `T`,
    /// or from a schema with exactly `T`'s name and layout).
    pub fn component_id_of<T: Component>(&self) -> Option<ComponentId> {
        self.components.lookup::<T>(T::NAME).map(ComponentId)
    }

    /// Every registered component, in registration order.
    pub fn components(&self) -> impl ExactSizeIterator<Item = (ComponentId, &TypeInfo)> {
        // Registration keeps the count within u32 (see `Table::push`).
        self.components
            .infos
            .iter()
            .enumerate()
            .map(|(index, info)| (ComponentId(index as u32), info))
    }

    /// Information about a registered resource.
    pub fn resource(&self, id: ResourceId) -> Option<&TypeInfo> {
        self.resources.get(id.0)
    }

    /// The id of the resource registered under `name`.
    pub fn resource_id(&self, name: &str) -> Option<ResourceId> {
        self.resources.by_name(name).map(ResourceId)
    }

    /// The id of the resource type `T`, if it has been registered (as `T`,
    /// or from a schema with exactly `T`'s name and layout).
    pub fn resource_id_of<T: Resource>(&self) -> Option<ResourceId> {
        self.resources.lookup::<T>(T::NAME).map(ResourceId)
    }

    /// Every registered resource, in registration order.
    pub fn resources(&self) -> impl ExactSizeIterator<Item = (ResourceId, &TypeInfo)> {
        // Registration keeps the count within u32 (see `Table::push`).
        self.resources
            .infos
            .iter()
            .enumerate()
            .map(|(index, info)| (ResourceId(index as u32), info))
    }

    pub(crate) fn register_component<T: Component>(&mut self) -> Result<ComponentId> {
        self.components.register_typed::<T>().map(ComponentId)
    }

    pub(crate) fn register_component_schema(&mut self, schema: TypeSchema) -> Result<ComponentId> {
        self.components.register_schema(schema).map(ComponentId)
    }

    pub(crate) fn register_resource<T: Resource>(&mut self) -> Result<ResourceId> {
        self.resources.register_typed::<T>().map(ResourceId)
    }

    pub(crate) fn register_resource_schema(&mut self, schema: TypeSchema) -> Result<ResourceId> {
        self.resources.register_schema(schema).map(ResourceId)
    }

    pub(crate) fn unknown_component(&self, name: &str) -> Diagnostic {
        self.components.unknown(name)
    }

    pub(crate) fn unknown_resource(&self, name: &str) -> Diagnostic {
        self.resources.unknown(name)
    }

    /// Information about a registered event type.
    pub fn event(&self, id: EventId) -> Option<&TypeInfo> {
        self.events.get(id.0)
    }

    /// The id of the event type registered under `name`.
    pub fn event_id(&self, name: &str) -> Option<EventId> {
        self.events.by_name(name).map(EventId)
    }

    /// The id of the event type `T`, if it has been registered (as `T`,
    /// or from a schema with exactly `T`'s name and layout).
    pub fn event_id_of<T: Event>(&self) -> Option<EventId> {
        self.events.lookup::<T>(T::NAME).map(EventId)
    }

    /// Every registered event type, in registration order.
    pub fn events(&self) -> impl ExactSizeIterator<Item = (EventId, &TypeInfo)> {
        // Registration keeps the count within u32 (see `Table::push`).
        self.events
            .infos
            .iter()
            .enumerate()
            .map(|(index, info)| (EventId(index as u32), info))
    }

    pub(crate) fn register_event<T: Event>(&mut self) -> Result<EventId> {
        self.events.register_typed::<T>().map(EventId)
    }

    pub(crate) fn register_event_schema(&mut self, schema: TypeSchema) -> Result<EventId> {
        self.events.register_schema(schema).map(EventId)
    }

    pub(crate) fn replace_component_schema(
        &mut self,
        id: ComponentId,
        schema: TypeSchema,
    ) -> Result<()> {
        self.components.replace_schema(id.0, schema)
    }

    pub(crate) fn replace_resource_schema(
        &mut self,
        id: ResourceId,
        schema: TypeSchema,
    ) -> Result<()> {
        self.resources.replace_schema(id.0, schema)
    }

    pub(crate) fn replace_event_schema(&mut self, id: EventId, schema: TypeSchema) -> Result<()> {
        self.events.replace_schema(id.0, schema)
    }

    pub(crate) fn unknown_event(&self, name: &str) -> Diagnostic {
        self.events.unknown(name)
    }

    /// How many component, resource and event types are registered.
    pub fn counts(&self) -> [usize; 3] {
        [
            self.components.infos.len(),
            self.resources.infos.len(),
            self.events.infos.len(),
        ]
    }

    /// The fingerprint of every registered type's layout, in registration
    /// order. Two registries with the same fingerprint interpret stored
    /// bytes the same way.
    pub fn fingerprint(&self) -> Hash128 {
        self.fingerprint_of(self.counts()).unwrap_or_default()
    }

    /// The fingerprint of the first `counts` types of each kind, or `None`
    /// if fewer are registered. Registration only appends, so a snapshot
    /// taken before more types were registered still matches this prefix.
    pub fn fingerprint_of(&self, counts: [usize; 3]) -> Option<Hash128> {
        let mut hasher = wish_core::StableHasher::new();
        hasher.write_str("wish-registry-v1");
        for (table, count) in [&self.components, &self.resources, &self.events]
            .into_iter()
            .zip(counts)
        {
            hasher.write_str(table.kind);
            hasher.write_u64(count as u64);
            for info in table.infos.get(..count)? {
                hasher.write_bytes(&info.fingerprint.as_u128().to_le_bytes());
                hasher.write_u8(u8::from(info.managed));
            }
        }
        Some(hasher.finish())
    }

    /// Marks a component as kept consistent by the world itself.
    pub(crate) fn mark_managed(&mut self, id: ComponentId) {
        if let Some(info) = self.components.infos.get_mut(id.index()) {
            info.managed = true;
        }
    }

    /// Whether the component type `T` is registered and managed.
    pub(crate) fn is_managed_type(&self, type_id: TypeId) -> bool {
        self.components
            .by_type(type_id)
            .and_then(|index| self.components.get(index))
            .is_some_and(TypeInfo::is_managed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cover_fields_and_skip_padding() {
        let schema = TypeSchema::structure(
            "Padded",
            "",
            16,
            8,
            vec![
                wish_schema::FieldSchema::new("flag", "", 0, bool::schema()),
                wish_schema::FieldSchema::new("small", "", 2, u16::schema()),
                wish_schema::FieldSchema::new("wide", "", 8, f64::schema()),
            ],
        );
        let info = TypeInfo::new(schema, None).unwrap();
        assert_eq!(info.spans(), [(0, 1), (2, 2), (8, 8)]);

        let array = TypeInfo::new(<[u32; 3]>::schema(), None).unwrap();
        assert_eq!(array.spans(), [(0, 12)]);

        let empty =
            TypeInfo::new(TypeSchema::structure("Empty", "", 0, 1, Vec::new()), None).unwrap();
        assert!(empty.spans().is_empty());
    }

    #[test]
    fn schema_registration_is_idempotent_and_detects_conflicts() {
        let mut registry = Registry::default();
        let first = registry.register_component_schema(u32::schema()).unwrap();
        let again = registry.register_component_schema(u32::schema()).unwrap();
        assert_eq!(first, again);
        let different = TypeSchema {
            name: "u32".to_owned(),
            ..u64::schema()
        };
        let conflict = registry.register_component_schema(different).unwrap_err();
        assert_eq!(conflict.code, "type_name_conflict");
    }

    #[test]
    fn unknown_names_suggest_registered_ones() {
        let mut registry = Registry::default();
        registry
            .register_component_schema(TypeSchema::structure("Transform", "", 0, 1, Vec::new()))
            .unwrap();
        let unknown = registry.unknown_component("Transfrom");
        assert_eq!(unknown.code, "unknown_component");
        assert_eq!(unknown.hint, "Did you mean 'Transform'?");
        assert_eq!(unknown.candidates, ["Transform"]);
        assert_eq!(
            registry.unknown_resource("Gravity").code,
            "unknown_resource"
        );
    }
}
