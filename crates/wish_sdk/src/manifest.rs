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
}

impl TypeTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    fn add<T: Plain>(&mut self, role: Role) -> String {
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
