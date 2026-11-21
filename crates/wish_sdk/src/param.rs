//! What a system can ask for besides queries: resources, commands, events.

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use serde_json::{Value, json};
use wish_core::Entity;
use wish_schema::{Component, Event, Resource};

use crate::abi::{next_spawn, push_command};
use crate::command::{Command, Target};
use crate::frame::{FrameReader, Section, section_count};
use crate::manifest::TypeTable;

/// A system parameter: something a `#[system]` function takes. The engine
/// fills one frame section per manifest entry, in order; most parameters
/// have one entry, a few (such as [`PlayerActions`]) several.
pub trait SystemParam: Sized {
    /// The parameter's entry in the manifest.
    fn describe(types: &mut TypeTable) -> Value;
    /// Reads the parameter from its section.
    fn fetch(section: Section) -> Self;
    /// Every manifest entry of the parameter, in order (`#[system]` calls
    /// this; the default is the one entry of [`SystemParam::describe`]).
    fn describe_into(types: &mut TypeTable, entries: &mut Vec<Value>) {
        entries.push(Self::describe(types));
    }
    /// Reads the parameter from the frame, one section per entry
    /// (`#[system]` calls this; the default reads one with
    /// [`SystemParam::fetch`]).
    fn fetch_from(frame: &mut FrameReader) -> Self {
        Self::fetch(frame.next_section())
    }
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

/// The player's input this tick, as actions (`input.json` names them).
///
/// ```ignore
/// #[system]
/// fn walk(actions: Actions, mut players: Query<&mut Transform, With<Player>>, time: Res<Time>) {
///     let walk = actions.axis2("move");
///     for transform in players.iter_mut() {
///         transform.translation += Vec3::new(walk.x, 0.0, -walk.y) * 4.0 * time.delta;
///     }
///     if actions.pressed("jump") { /* ... */ }
/// }
/// ```
///
/// A name the project's `input.json` does not have reads as at rest, so a
/// mistyped name shows as an action that never happens; `script_build`'s
/// companion `check` lists the names the project has.
pub struct Actions {
    state: wish_types::ActionState,
}

impl Actions {
    fn slot(&self, name: &str) -> Option<&wish_types::ActionSlot> {
        self.state.slot(name)
    }

    /// Whether the action is active now (a button is down, an axis is away
    /// from zero).
    pub fn held(&self, name: &str) -> bool {
        self.slot(name).is_some_and(|slot| slot.held)
    }

    /// Whether the action became active this tick.
    pub fn pressed(&self, name: &str) -> bool {
        self.slot(name).is_some_and(|slot| slot.pressed)
    }

    /// Whether the action stopped being active this tick.
    pub fn released(&self, name: &str) -> bool {
        self.slot(name).is_some_and(|slot| slot.released)
    }

    /// The value of a one-axis (or button) action, from -1 to 1.
    pub fn axis(&self, name: &str) -> f32 {
        self.slot(name).map_or(0.0, |slot| slot.x)
    }

    /// The value of a two-axis action: x to the right, y up.
    pub fn axis2(&self, name: &str) -> wish_core::math::Vec2 {
        self.slot(name).map_or(wish_core::math::Vec2::ZERO, |slot| {
            wish_core::math::Vec2::new(slot.x, slot.y)
        })
    }
}

impl SystemParam for Actions {
    fn describe(types: &mut TypeTable) -> Value {
        json!({ "res": { "type": types.resource::<wish_types::ActionState>(), "write": false } })
    }
    fn fetch(section: Section) -> Self {
        Self {
            state: decode(section.bytes(), "ActionState"),
        }
    }
}

/// Every player's input this tick, for a game with more than one player
/// (a session started with several players, release 1.1). Player 0 is the
/// first player, the same as [`Actions`]; in a single-player game there is
/// only player 0, so the same code runs alone and with others.
///
/// ```ignore
/// #[system]
/// fn walk(players: PlayerActions, mut bodies: Query<(&Avatar, &mut Transform)>, time: Res<Time>) {
///     for (avatar, transform) in bodies.iter_mut() {
///         let walk = players.of(avatar.player).axis2("move");
///         transform.translation += Vec3::new(walk.x, 0.0, -walk.y) * 4.0 * time.delta;
///     }
/// }
/// ```
///
/// It reads three resources (`ActionState`, `Players`, `PlayerActionState`,
/// about 8 KiB in all), so ask for it only in systems that need other
/// players than player 0.
pub struct PlayerActions {
    first: wish_types::ActionState,
    count: u32,
    all: wish_types::PlayerActionState,
}

impl PlayerActions {
    /// How many players the session has (1 in a single-player game).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The players' ids, in order: 0 to `count() - 1`.
    pub fn ids(&self) -> impl Iterator<Item = wish_types::PlayerId> + use<> {
        0..self.count
    }

    /// The actions of `player`, read like [`Actions`]. An id that is not a
    /// player of the session reads as at rest.
    pub fn of(&self, player: wish_types::PlayerId) -> Actions {
        if player == 0 {
            return Actions { state: self.first };
        }
        let slots = (player < self.count)
            .then(|| self.all.of(player))
            .flatten()
            .map(|entry| entry.slots)
            .unwrap_or_default();
        Actions {
            state: wish_types::ActionState { slots },
        }
    }
}

impl SystemParam for PlayerActions {
    fn describe(types: &mut TypeTable) -> Value {
        Actions::describe(types)
    }
    /// From one section only player 0 can be read; `#[system]` uses
    /// [`SystemParam::fetch_from`], which reads all three.
    fn fetch(section: Section) -> Self {
        Self {
            first: Actions::fetch(section).state,
            count: 1,
            all: wish_types::PlayerActionState::default(),
        }
    }
    fn describe_into(types: &mut TypeTable, entries: &mut Vec<Value>) {
        entries.push(Actions::describe(types));
        entries.push(Res::<wish_types::Players>::describe(types));
        entries.push(Res::<wish_types::PlayerActionState>::describe(types));
    }
    fn fetch_from(frame: &mut FrameReader) -> Self {
        let first = Actions::fetch(frame.next_section()).state;
        let players: wish_types::Players = decode(frame.next_section().bytes(), "Players");
        let all = decode(frame.next_section().bytes(), "PlayerActionState");
        Self {
            first,
            count: players.count.clamp(1, wish_types::MAX_PLAYERS as u32),
            all,
        }
    }
}
