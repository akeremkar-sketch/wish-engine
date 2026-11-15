//! Deferred changes to a world.
//!
//! While a query runs, the world is borrowed and cannot change shape:
//! entities cannot be spawned or despawned, components cannot be added or
//! removed. Such changes are recorded in [`Commands`] and applied afterwards
//! with `World::apply`, in the order they were recorded.

use serde_json::Value;
use wish_core::{Diagnostic, Entity, Result};
use wish_schema::Component;

use crate::World;

/// The entity a command acts on: one that exists, or one spawned by an
/// earlier command in the same buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// An entity that already exists.
    Entity(Entity),
    /// The entity created by the buffer's `n`-th spawn command (from 0).
    Spawned(u32),
}

impl From<Entity> for Target {
    fn from(entity: Entity) -> Self {
        Self::Entity(entity)
    }
}

type InsertFn = fn(&mut World, Entity, &[u8]) -> Result<()>;
type RemoveFn = fn(&mut World, Entity) -> Result<()>;

#[derive(Clone)]
enum Command {
    Spawn,
    Despawn(Target),
    Insert {
        target: Target,
        insert: InsertFn,
        bytes: Vec<u8>,
    },
    Remove {
        target: Target,
        remove: RemoveFn,
    },
    SetJson {
        target: Target,
        component: String,
        value: Value,
    },
    RemoveJson {
        target: Target,
        component: String,
    },
    SetParent {
        child: Target,
        parent: Target,
    },
    RemoveParent(Target),
}

fn insert_typed<T: Component>(world: &mut World, entity: Entity, bytes: &[u8]) -> Result<()> {
    let value = T::decode(bytes)
        .ok_or_else(|| crate::internal_error("a recorded component did not decode"))?;
    world.insert(entity, value)
}

fn remove_typed<T: Component>(world: &mut World, entity: Entity) -> Result<()> {
    world.remove::<T>(entity).map(|_| ())
}

/// A list of changes to apply to a world later, in order.
#[derive(Clone, Default)]
pub struct Commands {
    commands: Vec<Command>,
    spawns: u32,
}

impl core::fmt::Debug for Commands {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Commands")
            .field("len", &self.commands.len())
            .field("spawns", &self.spawns)
            .finish()
    }
}

/// What applying a command buffer did.
#[derive(Debug, Default)]
pub struct CommandReport {
    /// The entities created by the buffer's spawn commands, in order. An
    /// entry is `None` if that spawn failed.
    pub spawned: Vec<Option<Entity>>,
    /// One diagnostic per command that failed, located by the command's index
    /// in the buffer. The other commands were still applied.
    pub diagnostics: Vec<Diagnostic>,
}

impl CommandReport {
    /// Whether every command succeeded.
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl Commands {
    /// An empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of recorded commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether nothing is recorded.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Records spawning an entity; the returned target refers to it in later
    /// commands of this buffer.
    pub fn spawn(&mut self) -> Target {
        let target = Target::Spawned(self.spawns);
        self.spawns += 1;
        self.commands.push(Command::Spawn);
        target
    }

    /// Records despawning an entity, and its children.
    pub fn despawn(&mut self, target: impl Into<Target>) {
        self.commands.push(Command::Despawn(target.into()));
    }

    /// Records adding or replacing a component.
    pub fn insert<T: Component>(&mut self, target: impl Into<Target>, value: T) {
        let mut bytes = vec![0u8; size_of::<T>()];
        value.encode(&mut bytes);
        self.commands.push(Command::Insert {
            target: target.into(),
            insert: insert_typed::<T>,
            bytes,
        });
    }

    /// Records removing a component.
    pub fn remove<T: Component>(&mut self, target: impl Into<Target>) {
        self.commands.push(Command::Remove {
            target: target.into(),
            remove: remove_typed::<T>,
        });
    }

    /// Records setting a component from JSON, by name (see `World::set_json`).
    pub fn set_json(&mut self, target: impl Into<Target>, component: &str, value: Value) {
        self.commands.push(Command::SetJson {
            target: target.into(),
            component: component.to_owned(),
            value,
        });
    }

    /// Records removing a component by name.
    pub fn remove_json(&mut self, target: impl Into<Target>, component: &str) {
        self.commands.push(Command::RemoveJson {
            target: target.into(),
            component: component.to_owned(),
        });
    }

    /// Records making `child` the last child of `parent`.
    pub fn set_parent(&mut self, child: impl Into<Target>, parent: impl Into<Target>) {
        self.commands.push(Command::SetParent {
            child: child.into(),
            parent: parent.into(),
        });
    }

    /// Records detaching `child` from its parent.
    pub fn remove_parent(&mut self, child: impl Into<Target>) {
        self.commands.push(Command::RemoveParent(child.into()));
    }
}

fn resolve(target: Target, spawned: &[Option<Entity>]) -> Result<Entity> {
    match target {
        Target::Entity(entity) => Ok(entity),
        Target::Spawned(index) => spawned
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                Diagnostic::error(
                    "spawn_target_missing",
                    format!("Spawn command {index} did not create an entity."),
                    "Check the diagnostic of that spawn command; commands that refer to its entity cannot run.",
                )
            }),
    }
}

impl World {
    /// Applies a command buffer, in order. A failing command is reported and
    /// skipped; the others still run.
    pub fn apply(&mut self, commands: Commands) -> CommandReport {
        let mut report = CommandReport::default();
        for (index, command) in commands.commands.into_iter().enumerate() {
            let result =
                match command {
                    Command::Spawn => {
                        let spawned = self.spawn();
                        report.spawned.push(spawned.as_ref().ok().copied());
                        spawned.map(|_| ())
                    }
                    Command::Despawn(target) => {
                        resolve(target, &report.spawned).and_then(|entity| self.despawn(entity))
                    }
                    Command::Insert {
                        target,
                        insert,
                        bytes,
                    } => resolve(target, &report.spawned)
                        .and_then(|entity| insert(self, entity, &bytes)),
                    Command::Remove { target, remove } => {
                        resolve(target, &report.spawned).and_then(|entity| remove(self, entity))
                    }
                    Command::SetJson {
                        target,
                        component,
                        value,
                    } => resolve(target, &report.spawned)
                        .and_then(|entity| self.set_json(entity, &component, &value)),
                    Command::RemoveJson { target, component } => resolve(target, &report.spawned)
                        .and_then(|entity| self.remove_json(entity, &component).map(|_| ())),
                    Command::SetParent { child, parent } => resolve(child, &report.spawned)
                        .and_then(|child| {
                            let parent = resolve(parent, &report.spawned)?;
                            self.set_parent(child, parent)
                        }),
                    Command::RemoveParent(child) => {
                        resolve(child, &report.spawned).and_then(|child| self.remove_parent(child))
                    }
                };
            if let Err(diagnostic) = result {
                let op = u32::try_from(index).unwrap_or(u32::MAX);
                report.diagnostics.push(diagnostic.at_op(op));
            }
        }
        report
    }
}
