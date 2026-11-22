//! The manifest: what a module tells the engine about its types and systems
//! (design note M5 §3). Built by `wish_sdk::export!` when the engine calls
//! `wish_manifest`.

use serde_json::{Value, json};
use wish_schema::wire::to_wire;
use wish_schema::{Component, Event, Plain, Resource};

/// What a type is to the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Data attached to entities.
    Component,
    /// A single value per world.
    Resource,
    /// A message between systems.
    Event,
}

impl Role {
    /// The role's name in the manifest.
    pub fn name(self) -> &'static str {
        match self {
            Self::Component => "component",
            Self::Resource => "resource",
            Self::Event => "event",
        }
    }
}

/// The types a module uses, each once, in the order first seen.
#[derive(Debug, Default)]
pub struct TypeTable {
    types: Vec<(Role, String, Value)>,
    /// The Rust types already listed, by role: a type that every system
    /// takes (`Res<Lobby>`, `PlayerActions`) is described once, not once
    /// per parameter (friction F12: a twelve-system game ran out of fuel
    /// building its schemas again and again).
    seen: Vec<(Role, std::any::TypeId, String)>,
}

impl TypeTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    fn add<T: Plain>(&mut self, role: Role) -> String {
        let id = std::any::TypeId::of::<T>();
        if let Some((_, _, name)) = self.seen.iter().find(|(r, t, _)| *r == role && *t == id) {
            return name.clone();
        }
        let schema = T::schema();
        let wire = to_wire(&schema);
        // The same type seen again is listed once; a different type with the
        // same name (two `State`s in two modules, or a component and a
        // resource) is listed too, so the engine can refuse the clash
        // instead of mixing up their values.
        if !self
            .types
            .iter()
            .any(|(r, name, w)| *r == role && *name == schema.name && *w == wire)
        {
            self.types.push((role, schema.name.clone(), wire));
        }
        self.seen.push((role, id, schema.name.clone()));
        schema.name
    }

    /// Adds a component type and returns its name.
    pub fn component<T: Component>(&mut self) -> String {
        self.add::<T>(Role::Component)
    }

    /// Adds a resource type and returns its name.
    pub fn resource<T: Resource>(&mut self) -> String {
        self.add::<T>(Role::Resource)
    }

    /// Adds an event type and returns its name.
    pub fn event<T: Event>(&mut self) -> String {
        self.add::<T>(Role::Event)
    }

    /// The table in manifest form.
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.types
                .iter()
                .map(|(role, _, schema)| json!({ "role": role.name(), "schema": schema }))
                .collect(),
        )
    }
}

/// One system's entry.
pub fn system_json(name: &str, before: &[&str], after: &[&str], params: Vec<Value>) -> Value {
    json!({ "name": name, "before": before, "after": after, "params": params })
}

/// The whole manifest, as JSON text.
pub fn manifest_json(types: &TypeTable, systems: Vec<Value>) -> String {
    json!({
        "abi": crate::ABI_VERSION,
        "sdk": env!("CARGO_PKG_VERSION"),
        "types": types.to_json(),
        "systems": systems,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// Friction F12: a type every system asks for is described once; the
    /// table then answers its name without building its schema again (a
    /// twelve-system game ran out of fuel describing `Lobby` and
    /// `PlayerActionState` once per parameter).
    #[test]
    fn a_type_is_described_once_whatever_asks_for_it() {
        let mut table = TypeTable::new();
        for _ in 0..50 {
            assert_eq!(table.resource::<wish_types::Lobby>(), "Lobby");
            table.resource::<wish_types::PlayerActionState>();
        }
        assert_eq!(table.types.len(), 2);
        assert_eq!(table.seen.len(), 2);
        // The same type in another role is listed again (the engine then
        // refuses the clash).
        table.component::<wish_types::Sprite>();
        assert_eq!(table.types.len(), 3);
    }
}
