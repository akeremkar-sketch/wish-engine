//! What a system can ask for besides queries: resources, commands, events.

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use serde_json::{Value, json};
use wish_core::Entity;
use wish_schema::{Component, Event, Resource};

use crate::abi::{next_spawn, push_command};
use crate::command::{Command, Target};
use crate::frame::{Section, section_count};
use crate::manifest::TypeTable;

/// A system parameter: something a `#[system]` function takes. The engine
/// fills one frame section per parameter, in order.
pub trait SystemParam: Sized {
    /// The parameter's entry in the manifest.
    fn describe(types: &mut TypeTable) -> Value;
    /// Reads the parameter from its section.
    fn fetch(section: Section) -> Self;
}

fn decode<T: wish_schema::Plain>(bytes: &[u8], name: &str) -> T {
    T::decode(bytes.get(..size_of::<T>()).unwrap_or(&[])).unwrap_or_else(|| {
        crate::abi::fail(&format!(
            "the engine sent a {name} value this module cannot read; rebuild the game code with this engine's SDK"
        ))
    })
}

/// A resource, read-only. If the world has none yet, the engine adds the
/// type's default value first.
pub struct Res<T: Resource> {
    value: T,
}

impl<T: Resource> Deref for Res<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Resource> SystemParam for Res<T> {
    fn describe(types: &mut TypeTable) -> Value {
        json!({ "res": { "type": types.resource::<T>(), "write": false } })
    }
    fn fetch(section: Section) -> Self {
        Self {
            value: decode(section.bytes(), T::NAME),
        }
    }
}

/// A resource, writable. Written back when the system ends; recorded as
/// changed only if the value changed.
pub struct ResMut<T: Resource> {
    value: T,
    section: Section,
}

impl<T: Resource> Deref for ResMut<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Resource> DerefMut for ResMut<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Resource> SystemParam for ResMut<T> {
    fn describe(types: &mut TypeTable) -> Value {
        json!({ "res": { "type": types.resource::<T>(), "write": true } })
    }
    fn fetch(section: Section) -> Self {
        Self {
            value: decode(section.bytes(), T::NAME),
            section,
        }
    }
}

impl<T: Resource> Drop for ResMut<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.section.bytes_mut().get_mut(..size_of::<T>()) {
            self.value.encode(slot);
        }
    }
}

/// An entity spawned by this system's [`Commands`]: refer to it in later
/// commands of the same call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewEntity(u32);

impl From<NewEntity> for Target {
    fn from(entity: NewEntity) -> Self {
        Target::New(entity.0)
    }
}

/// Changes to the world the system asks for: spawning, despawning, adding
/// and removing components, parents. They happen after the system returns,
/// in the order given.
pub struct Commands {
    _private: (),
}

fn encoded<T: wish_schema::Plain>(value: &T) -> Vec<u8> {
    let mut bytes = vec![0u8; size_of::<T>()];
    value.encode(&mut bytes);
    bytes
}

impl Commands {
    /// Spawns an entity. Chain [`EntityCommands::insert`] to give it
    /// components.
    pub fn spawn(&mut self) -> EntityCommands<'_> {
        let index = next_spawn();
        push_command(&Command::Spawn { index });
        EntityCommands {
            target: Target::New(index),
            _commands: self,
        }
    }

    /// Commands for an existing entity.
    pub fn entity(&mut self, entity: Entity) -> EntityCommands<'_> {
        EntityCommands {
            target: Target::Existing(entity),
            _commands: self,
        }
    }

    /// Despawns an entity (and its children).
    pub fn despawn(&mut self, entity: impl Into<Target>) {
        push_command(&Command::Despawn(entity.into()));
    }

    /// Adds or replaces a component.
    pub fn insert<T: Component>(&mut self, entity: impl Into<Target>, value: T) {
        push_command(&Command::Insert {
            target: entity.into(),
            component: T::NAME.to_owned(),
            bytes: encoded(&value),
        });
    }

    /// Removes a component.
    pub fn remove<T: Component>(&mut self, entity: impl Into<Target>) {
        push_command(&Command::Remove {
            target: entity.into(),
            component: T::NAME.to_owned(),
        });
    }

    /// Makes `child` a child of `parent`.
    pub fn set_parent(&mut self, child: impl Into<Target>, parent: impl Into<Target>) {
        push_command(&Command::SetParent {
            child: child.into(),
            parent: parent.into(),
        });
    }

    /// Makes an entity a root.
    pub fn remove_parent(&mut self, child: impl Into<Target>) {
        push_command(&Command::RemoveParent(child.into()));
    }
}

impl SystemParam for Commands {
    fn describe(_: &mut TypeTable) -> Value {
        json!({ "commands": {} })
    }
    fn fetch(_: Section) -> Self {
        Self { _private: () }
    }
}

/// Commands for one entity, from [`Commands::spawn`] or [`Commands::entity`].
pub struct EntityCommands<'c> {
    target: Target,
    _commands: &'c mut Commands,
}

impl EntityCommands<'_> {
    /// Adds or replaces a component.
    pub fn insert<T: Component>(self, value: T) -> Self {
        push_command(&Command::Insert {
            target: self.target,
            component: T::NAME.to_owned(),
            bytes: encoded(&value),
        });
        self
    }

    /// Removes a component.
    pub fn remove<T: Component>(self) -> Self {
        push_command(&Command::Remove {
            target: self.target,
            component: T::NAME.to_owned(),
        });
        self
    }

    /// Who the commands are for.
    pub fn id(&self) -> Target {
        self.target
    }
}

/// Events of type `T` sent since this system last ran (by any system,
/// including events it sent itself last time).
pub struct EventReader<T: Event> {
    events: Vec<T>,
}

impl<T: Event> EventReader<T> {
    /// The events, oldest first.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.events.iter()
    }

    /// How many there are.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl<T: Event> SystemParam for EventReader<T> {
    fn describe(types: &mut TypeTable) -> Value {
        json!({ "events": { "type": types.event::<T>() } })
    }
    fn fetch(section: Section) -> Self {
        let bytes = section.bytes();
        let size = size_of::<T>();
        let events = (0..section_count(bytes))
            .map(|row| decode(bytes.get(8 + row * size..).unwrap_or(&[]), T::NAME))
            .collect();
        Self { events }
    }
}

/// Sends events of type `T`; other systems read them with [`EventReader`].
pub struct EventWriter<T: Event> {
    _type: PhantomData<T>,
}

impl<T: Event> EventWriter<T> {
    /// Sends an event (after the system returns, in order with commands).
    pub fn send(&mut self, event: T) {
        push_command(&Command::Send {
            event: T::NAME.to_owned(),
            bytes: encoded(&event),
        });
    }
}

impl<T: Event> SystemParam for EventWriter<T> {
    fn describe(types: &mut TypeTable) -> Value {
        json!({ "send": { "type": types.event::<T>() } })
    }
    fn fetch(_: Section) -> Self {
        Self { _type: PhantomData }
    }
}
