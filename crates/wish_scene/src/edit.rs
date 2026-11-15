//! Edits to a scene document: transactions, revisions, undo and redo.
//!
//! Every change to a document is a [`Transaction`]: a list of [`EditOp`]s
//! made by one [`Actor`]. A transaction is applied to a copy of the document
//! and only replaces the original if every operation succeeds and the result
//! is valid, so a rejected transaction changes nothing. Each operation
//! computes its own inverse while it runs; undoing a transaction applies
//! those inverses in reverse order, and gives back the exact previous
//! content (a despawned subtree comes back with every component, note,
//! order and parent link).
//!
//! The same machinery serves people (the editor), AI agents (MCP) and files
//! changed on disk, so there is one history with who did what in it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;

use crate::document::{
    MAX_SCENE_ID_LEN, Overrides, SceneDocument, SceneEntity, check_override_target,
    is_valid_scene_id,
};
use crate::schema_check::{self, merge};

/// How many transactions can be undone. Older ones are forgotten.
pub const MAX_UNDO: usize = 1000;

/// How many entries the activity log keeps.
pub const MAX_LOG: usize = 1000;

/// Who made a change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Actor {
    /// A person, through the editor or the command line.
    Human,
    /// An AI agent, named by the agent or the tool that drives it.
    Agent(String),
    /// The file was changed on disk, outside the engine.
    File,
}

impl Actor {
    /// A short label for messages and the activity feed.
    pub fn label(&self) -> String {
        match self {
            Self::Human => "human".to_owned(),
            Self::Agent(name) => format!("agent:{name}"),
            Self::File => "file".to_owned(),
        }
    }
}

/// One change to a scene document.
#[derive(Clone, Debug, PartialEq)]
pub enum EditOp {
    /// Adds an entity.
    Spawn {
        /// The new entity's id.
        id: String,
        /// Its parent, if any.
        parent: Option<String>,
        /// Its sibling order.
        order: i64,
        /// Its components, by name, as patches on each type's defaults.
        components: BTreeMap<String, Value>,
        /// Notes for people.
        notes: Option<String>,
    },
    /// Places a prefab: adds an entity that is an instance of it.
    Instantiate {
        /// The new instance's id. The prefab's other entities get the ids
        /// `id/<prefab entity id>` when the scene is expanded.
        id: String,
        /// The prefab's project-relative path, as loaded in the document's
        /// prefab library.
        prefab: String,
        /// The instance's parent, if any.
        parent: Option<String>,
        /// Its sibling order.
        order: i64,
        /// Patches on the prefab root's components.
        components: BTreeMap<String, Value>,
        /// Patches on the components of the prefab's other entities.
        overrides: Overrides,
        /// Notes for people.
        notes: Option<String>,
    },
    /// Removes an entity and everything below it in the hierarchy.
    Despawn {
        /// The entity.
        id: String,
    },
    /// Changes a component by merging `value` into it: the fields `value`
    /// names change, the others stay. Adds the component if it is missing.
    ///
    /// On a prefab instance, `Set`, `Replace` and `Remove` work on the
    /// instance's patches of the prefab root, not on the prefab itself:
    /// removing a patch brings back the prefab's value.
    Set {
        /// The entity.
        id: String,
        /// The component's name.
        component: String,
        /// The fields to change.
        value: Value,
    },
    /// Sets a component to exactly `value` (still a patch on the type's
    /// defaults), adding it if it is missing.
    Replace {
        /// The entity.
        id: String,
        /// The component's name.
        component: String,
        /// The component's new JSON.
        value: Value,
    },
    /// Removes a component.
    Remove {
        /// The entity.
        id: String,
        /// The component's name.
        component: String,
    },
    /// Moves an entity under another parent (or to the top level).
    Reparent {
        /// The entity.
        id: String,
        /// The new parent; `None` for the top level.
        parent: Option<String>,
        /// The new sibling order; `None` keeps the current one.
        order: Option<i64>,
    },
    /// Changes an entity's id; children follow.
    Rename {
        /// The current id.
        id: String,
        /// The new id.
        to: String,
    },
    /// Merges `value` into an instance's patch of one prefab entity's
    /// component, adding the patch if it is missing.
    SetOverride {
        /// The instance.
        id: String,
        /// The prefab entity (not `root`).
        entity: String,
        /// The component's name.
        component: String,
        /// The fields to change.
        value: Value,
    },
    /// Sets an instance's patch of one prefab entity's component to
    /// exactly `value`.
    ReplaceOverride {
        /// The instance.
        id: String,
        /// The prefab entity (not `root`).
        entity: String,
        /// The component's name.
        component: String,
        /// The patch's new JSON.
        value: Value,
    },
    /// Removes an instance's patch of one prefab entity's component, so
    /// that the prefab's value applies again.
    RemoveOverride {
        /// The instance.
        id: String,
        /// The prefab entity.
        entity: String,
        /// The component's name.
        component: String,
    },
    /// Sets or clears an entity's notes.
    SetNotes {
        /// The entity.
        id: String,
        /// The new notes; `None` removes them.
        notes: Option<String>,
    },
    /// Merges `value` into a scene resource, adding it if it is missing.
    SetResource {
        /// The resource's name.
        name: String,
        /// The fields to change.
        value: Value,
    },
    /// Sets a scene resource to exactly `value`.
    ReplaceResource {
        /// The resource's name.
        name: String,
        /// The resource's new JSON.
        value: Value,
    },
    /// Removes a scene resource.
    RemoveResource {
        /// The resource's name.
        name: String,
    },
}

impl EditOp {
    /// An operation that adds an entity with no parent and no components.
    pub fn spawn(id: &str) -> Self {
        Self::Spawn {
            id: id.to_owned(),
            parent: None,
            order: 0,
            components: BTreeMap::new(),
            notes: None,
        }
    }

    /// An operation that places `prefab` as a top-level instance with no
    /// changes.
    pub fn instantiate(id: &str, prefab: &str) -> Self {
        Self::Instantiate {
            id: id.to_owned(),
            prefab: prefab.to_owned(),
            parent: None,
            order: 0,
            components: BTreeMap::new(),
            overrides: BTreeMap::new(),
            notes: None,
        }
    }
}

/// Changes made together, by one actor, that succeed or fail as a whole.
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    /// Who makes the change.
    pub actor: Actor,
    /// The revision the changes were prepared against. If the document has
    /// moved on since, the transaction is refused instead of overwriting
    /// someone else's work. `None` skips the check.
    pub base_revision: Option<u64>,
    /// The changes, applied in order.
    pub ops: Vec<EditOp>,
}

impl Transaction {
    /// A transaction with no revision check.
    pub fn new(actor: Actor, ops: Vec<EditOp>) -> Self {
        Self {
            actor,
            base_revision: None,
            ops,
        }
    }
}

/// What a transaction, undo or redo did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The document's revision afterwards.
    pub revision: u64,
    /// JSON pointers of everything that changed, sorted.
    pub changes: Vec<String>,
}

/// The kind of an activity log entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A transaction.
    Edit,
    /// An undo.
    Undo,
    /// A redo.
    Redo,
}

/// One entry of the activity log: who changed what, when.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// The revision the change produced.
    pub revision: u64,
    /// Who made it.
    pub actor: Actor,
    /// What kind of change it was.
    pub action: Action,
    /// JSON pointers of everything that changed.
    pub changes: Vec<String>,
}

#[derive(Clone, Debug)]
struct Entry {
    actor: Actor,
    forward: Vec<EditOp>,
    inverse: Vec<EditOp>,
}

/// A document's undo and redo stacks and activity log.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    log: VecDeque<LogEntry>,
}

fn entity_pointer(id: &str) -> String {
    format!("/entities/{id}")
}

fn component_pointer(id: &str, component: &str) -> String {
    format!("/entities/{id}/components/{component}")
}

fn override_pointer(id: &str, entity: &str, component: &str) -> String {
    format!("/entities/{id}/overrides/{entity}/components/{component}")
}

fn resource_pointer(name: &str) -> String {
    format!("/resources/{name}")
}

/// Checks a component value: the type is registered and writable, and the
/// JSON fits its schema.
fn checked_value(registry: &Registry, component: &str, value: &Value, pointer: &str) -> Result<()> {
    let info = schema_check::component(registry, component)
        .map_err(|problem| problem.at_path(pointer.to_owned()))?;
    schema_check::check_value(info.schema(), value, pointer)
}

/// What applying one operation needs to remember.
struct OpOutcome {
    inverse: Vec<EditOp>,
    changes: Vec<String>,
}

impl SceneDocument {
    /// Applies a transaction: every operation in order, all or nothing.
    ///
    /// On success the revision goes up by one, the transaction can be
    /// undone, and anything that was undone before can no longer be redone.
    /// On failure nothing changes; the diagnostic's `location.op` is the
    /// index of the operation that failed.
    pub fn apply(&mut self, transaction: &Transaction, registry: &Registry) -> Result<Applied> {
        if let Some(base) = transaction.base_revision
            && base != self.revision
        {
            return Err(Diagnostic::error(
                "stale_revision",
                format!(
                    "The changes were prepared against revision {base}, but the scene is now at revision {}.",
                    self.revision
                ),
                "Someone else changed the scene in the meantime. Read it again and redo your changes on top of the current revision.",
            ));
        }
        if transaction.ops.is_empty() {
            return Err(Diagnostic::error(
                "empty_transaction",
                "The transaction has no operations.",
                "Add at least one operation, such as spawn, set or remove.",
            ));
        }
        let (content, changes, inverse) = self.run(&transaction.ops, registry)?;
        self.commit(content);
        let entry = Entry {
            actor: transaction.actor.clone(),
            forward: transaction.ops.clone(),
            inverse,
        };
        self.history.redo.clear();
        self.history.undo.push_back(entry);
        if self.history.undo.len() > MAX_UNDO {
            self.history.undo.pop_front();
        }
        Ok(self.record(transaction.actor.clone(), Action::Edit, changes))
    }

    /// What applying a transaction would change, without changing anything.
    pub fn preview(&self, transaction: &Transaction, registry: &Registry) -> Result<Applied> {
        let (_, changes, _) = self.run(&transaction.ops, registry)?;
        Ok(Applied {
            revision: self.revision + 1,
            changes,
        })
    }

    /// Undoes the most recent transaction that has not been undone.
    /// `Ok(None)` when there is nothing to undo.
    pub fn undo(&mut self, actor: Actor, registry: &Registry) -> Result<Option<Applied>> {
        let Some(entry) = self.history.undo.pop_back() else {
            return Ok(None);
        };
        match self.run(&entry.inverse, registry) {
            Ok((content, changes, _)) => {
                self.commit(content);
                self.history.redo.push(entry);
                Ok(Some(self.record(actor, Action::Undo, changes)))
            }
            Err(problem) => {
                self.history.undo.push_back(entry);
                Err(problem)
            }
        }
    }

    /// Redoes the most recently undone transaction.
    /// `Ok(None)` when there is nothing to redo.
    pub fn redo(&mut self, actor: Actor, registry: &Registry) -> Result<Option<Applied>> {
        let Some(entry) = self.history.redo.pop() else {
            return Ok(None);
        };
        match self.run(&entry.forward, registry) {
            Ok((content, changes, _)) => {
                self.commit(content);
                self.history.undo.push_back(entry);
                Ok(Some(self.record(actor, Action::Redo, changes)))
            }
            Err(problem) => {
                self.history.redo.push(entry);
                Err(problem)
            }
        }
    }

    /// Whether there is a transaction to undo.
    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    /// Whether there is a transaction to redo.
    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    /// Who made the transaction that undo would revert next.
    pub fn next_undo_actor(&self) -> Option<&Actor> {
        self.history.undo.back().map(|entry| &entry.actor)
    }

    /// The activity log, oldest first (at most [`MAX_LOG`] entries).
    pub fn log(&self) -> impl Iterator<Item = &LogEntry> {
        self.history.log.iter()
    }

    fn commit(&mut self, content: SceneDocument) {
        self.entities = content.entities;
        self.resources = content.resources;
    }

    fn record(&mut self, actor: Actor, action: Action, changes: Vec<String>) -> Applied {
        self.revision += 1;
        self.history.log.push_back(LogEntry {
            revision: self.revision,
            actor,
            action,
            changes: changes.clone(),
        });
        if self.history.log.len() > MAX_LOG {
            self.history.log.pop_front();
        }
        Applied {
            revision: self.revision,
            changes,
        }
    }

    /// Applies operations to a copy of the content. Returns the copy, the
    /// sorted changes and the inverse operations (in the order that undoes
    /// them).
    fn run(
        &self,
        ops: &[EditOp],
        registry: &Registry,
    ) -> Result<(SceneDocument, Vec<String>, Vec<EditOp>)> {
        let mut content = SceneDocument {
            entities: self.entities.clone(),
            resources: self.resources.clone(),
            prefabs: Arc::clone(&self.prefabs),
            ..SceneDocument::default()
        };
        let mut changes = BTreeSet::new();
        let mut inverses: Vec<Vec<EditOp>> = Vec::with_capacity(ops.len());
        for (index, op) in ops.iter().enumerate() {
            let outcome = content
                .apply_op(op, registry)
                .map_err(|problem| problem.at_op(u32::try_from(index).unwrap_or(u32::MAX)))?;
            changes.extend(outcome.changes);
            inverses.push(outcome.inverse);
        }
        let inverse = inverses.into_iter().rev().flatten().collect();
        Ok((content, changes.into_iter().collect(), inverse))
    }

    fn unknown_entity(&self, id: &str) -> Diagnostic {
        let ids: Vec<&str> = self.entities.keys().map(String::as_str).collect();
        Diagnostic::error(
            "unknown_entity",
            format!("No entity has the id '{id}'."),
            unknown_name_hint(id, &ids),
        )
        .with_candidates(ids.iter().copied())
        .at_path(entity_pointer(id))
    }

    fn require(&self, id: &str) -> Result<&SceneEntity> {
        self.entities.get(id).ok_or_else(|| self.unknown_entity(id))
    }

    fn check_new_id(&self, id: &str) -> Result<()> {
        if !is_valid_scene_id(id) {
            return Err(Diagnostic::error(
                "invalid_scene_id",
                format!("'{id}' is not a valid entity id."),
                format!(
                    "An id starts with a lowercase letter and then has lowercase letters, digits and underscores, at most {MAX_SCENE_ID_LEN} characters. For example: \"player_1\"."
                ),
            ));
        }
        if self.entities.contains_key(id) {
            return Err(Diagnostic::error(
                "entity_exists",
                format!("An entity with the id '{id}' already exists."),
                "Choose another id, or change the existing entity instead.",
            )
            .at_path(entity_pointer(id)));
        }
        Ok(())
    }

    /// Checks that `parent` can become the parent of `id`: it exists, and
    /// `id` is not `parent` or one of its ancestors.
    fn check_parent(&self, id: &str, parent: Option<&str>) -> Result<()> {
        let Some(parent) = parent else {
            return Ok(());
        };
        if !self.entities.contains_key(parent) {
            let ids: Vec<&str> = self.entities.keys().map(String::as_str).collect();
            return Err(Diagnostic::error(
                "unknown_parent",
                format!("The parent '{parent}' does not exist."),
                unknown_name_hint(parent, &ids),
            )
            .with_candidates(ids.iter().copied()));
        }
        let mut current = Some(parent);
        let mut steps = 0;
        while let Some(ancestor) = current {
            if ancestor == id {
                return Err(Diagnostic::error(
                    "hierarchy_cycle",
                    format!(
                        "'{parent}' cannot become the parent of '{id}': '{id}' is '{parent}' itself or one of its ancestors."
                    ),
                    "A hierarchy cannot contain loops. Choose a parent that is not the entity itself or below it.",
                ));
            }
            steps += 1;
            if steps > self.entities.len() {
                break;
            }
            current = self
                .entities
                .get(ancestor)
                .and_then(|entity| entity.parent.as_deref());
        }
        Ok(())
    }

    /// `id` and everything below it, each parent before its children.
    fn subtree(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_owned()];
        let mut index = 0;
        while let Some(current) = out.get(index).cloned() {
            for child in self.children(&current) {
                out.push(child.to_owned());
            }
            index += 1;
            if out.len() > self.entities.len() {
                break;
            }
        }
        out
    }

    fn checked_component(
        &self,
        registry: &Registry,
        id: &str,
        component: &str,
        value: &Value,
    ) -> Result<()> {
        checked_value(
            registry,
            component,
            value,
            &component_pointer(id, component),
        )
    }

    /// The prefab at `path` in the document's library.
    fn prefab(&self, path: &str) -> Result<&SceneDocument> {
        self.prefabs.get(path).ok_or_else(|| {
            let paths = self.prefabs.paths();
            Diagnostic::error(
                "prefab_not_found",
                format!("No prefab '{path}' is loaded."),
                if paths.is_empty() {
                    "No prefabs are loaded for this scene. Load the prefab file first, then place it.".to_owned()
                } else {
                    unknown_name_hint(path, &paths)
                },
            )
            .with_candidates(paths.iter().copied())
        })
    }

    /// Checks the instance `id` may override the prefab entity `entity`.
    /// Returns the instance's prefab path.
    fn override_target(&self, id: &str, entity: &str) -> Result<String> {
        let instance = self.require(id)?;
        let Some(path) = instance.prefab.clone() else {
            return Err(Diagnostic::error(
                "not_an_instance",
                format!("Entity '{id}' is not a prefab instance, so it has no overrides."),
                "Change its components directly, or use an instance of a prefab.",
            )
            .at_path(entity_pointer(id)));
        };
        let prefab = self.prefab(&path)?;
        check_override_target(prefab, &path, entity).map_err(|problem| {
            problem.at_path(format!("{}/overrides/{entity}", entity_pointer(id)))
        })?;
        Ok(path)
    }

    fn apply_op(&mut self, op: &EditOp, registry: &Registry) -> Result<OpOutcome> {
        match op {
            EditOp::Spawn {
                id,
                parent,
                order,
                components,
                notes,
            } => {
                self.check_new_id(id)?;
                self.check_parent(id, parent.as_deref())?;
                for (name, value) in components {
                    self.checked_component(registry, id, name, value)?;
                }
                self.entities.insert(
                    id.clone(),
                    SceneEntity {
                        parent: parent.clone(),
                        order: *order,
                        components: components.clone(),
                        notes: notes.clone(),
                        prefab: None,
                        overrides: BTreeMap::new(),
                    },
                );
                Ok(OpOutcome {
                    inverse: vec![EditOp::Despawn { id: id.clone() }],
                    changes: vec![entity_pointer(id)],
                })
            }
            EditOp::Instantiate {
                id,
                prefab,
                parent,
                order,
                components,
                overrides,
                notes,
            } => {
                self.check_new_id(id)?;
                self.check_parent(id, parent.as_deref())?;
                let source = self.prefab(prefab)?;
                for (name, value) in components {
                    self.checked_component(registry, id, name, value)?;
                }
                for (inner, patches) in overrides {
                    check_override_target(source, prefab, inner).map_err(|problem| {
                        problem.at_path(format!("{}/overrides/{inner}", entity_pointer(id)))
                    })?;
                    for (name, value) in patches {
                        checked_value(registry, name, value, &override_pointer(id, inner, name))?;
                    }
                }
                // Empty patch lists are not kept, so the file stays canonical.
                let overrides: Overrides = overrides
                    .iter()
                    .filter(|(_, patches)| !patches.is_empty())
                    .map(|(inner, patches)| (inner.clone(), patches.clone()))
                    .collect();
                self.entities.insert(
                    id.clone(),
                    SceneEntity {
                        parent: parent.clone(),
                        order: *order,
                        components: components.clone(),
                        notes: notes.clone(),
                        prefab: Some(prefab.clone()),
                        overrides,
                    },
                );
                Ok(OpOutcome {
                    inverse: vec![EditOp::Despawn { id: id.clone() }],
                    changes: vec![entity_pointer(id)],
                })
            }
            EditOp::Despawn { id } => {
                self.require(id)?;
                let subtree = self.subtree(id);
                let mut inverse = Vec::with_capacity(subtree.len());
                let mut changes = Vec::with_capacity(subtree.len());
                for gone in &subtree {
                    if let Some(entity) = self.entities.remove(gone) {
                        inverse.push(match entity.prefab {
                            Some(prefab) => EditOp::Instantiate {
                                id: gone.clone(),
                                prefab,
                                parent: entity.parent,
                                order: entity.order,
                                components: entity.components,
                                overrides: entity.overrides,
                                notes: entity.notes,
                            },
                            None => EditOp::Spawn {
                                id: gone.clone(),
                                parent: entity.parent,
                                order: entity.order,
                                components: entity.components,
                                notes: entity.notes,
                            },
                        });
                        changes.push(entity_pointer(gone));
                    }
                }
                Ok(OpOutcome { inverse, changes })
            }
            EditOp::SetOverride {
                id,
                entity,
                component,
                value,
            }
            | EditOp::ReplaceOverride {
                id,
                entity,
                component,
                value,
            } => {
                self.override_target(id, entity)?;
                let pointer = override_pointer(id, entity, component);
                let previous = self
                    .entities
                    .get(id)
                    .and_then(|instance| instance.overrides.get(entity))
                    .and_then(|patches| patches.get(component))
                    .cloned();
                let next = match (op, &previous) {
                    (EditOp::SetOverride { .. }, Some(existing)) => {
                        let mut merged = existing.clone();
                        merge(&mut merged, value);
                        merged
                    }
                    _ => value.clone(),
                };
                checked_value(registry, component, &next, &pointer)?;
                if let Some(instance) = self.entities.get_mut(id) {
                    instance
                        .overrides
                        .entry(entity.clone())
                        .or_default()
                        .insert(component.clone(), next);
                }
                let inverse = match previous {
                    Some(old) => EditOp::ReplaceOverride {
                        id: id.clone(),
                        entity: entity.clone(),
                        component: component.clone(),
                        value: old,
                    },
                    None => EditOp::RemoveOverride {
                        id: id.clone(),
                        entity: entity.clone(),
                        component: component.clone(),
                    },
                };
                Ok(OpOutcome {
                    inverse: vec![inverse],
                    changes: vec![pointer],
                })
            }
            EditOp::RemoveOverride {
                id,
                entity,
                component,
            } => {
                let pointer = override_pointer(id, entity, component);
                let instance = self.require(id)?;
                let patches = instance.overrides.get(entity);
                let Some(old) = patches.and_then(|patches| patches.get(component)).cloned() else {
                    let names: Vec<&str> = patches
                        .map(|patches| patches.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    return Err(Diagnostic::error(
                        "override_not_found",
                        format!(
                            "Instance '{id}' does not override the '{component}' component of '{entity}'."
                        ),
                        if names.is_empty() {
                            format!("'{id}' has no overrides for '{entity}'.")
                        } else {
                            unknown_name_hint(component, &names)
                        },
                    )
                    .with_candidates(names.iter().copied())
                    .at_path(pointer));
                };
                if let Some(instance) = self.entities.get_mut(id)
                    && let Some(patches) = instance.overrides.get_mut(entity)
                {
                    patches.remove(component);
                    if patches.is_empty() {
                        instance.overrides.remove(entity);
                    }
                }
                Ok(OpOutcome {
                    inverse: vec![EditOp::ReplaceOverride {
                        id: id.clone(),
                        entity: entity.clone(),
                        component: component.clone(),
                        value: old,
                    }],
                    changes: vec![pointer],
                })
            }
            EditOp::Set {
                id,
                component,
                value,
            }
            | EditOp::Replace {
                id,
                component,
                value,
            } => {
                let previous = self.require(id)?.components.get(component).cloned();
                let next = match (op, &previous) {
                    (EditOp::Set { .. }, Some(existing)) => {
                        let mut merged = existing.clone();
                        merge(&mut merged, value);
                        merged
                    }
                    _ => value.clone(),
                };
                self.checked_component(registry, id, component, &next)?;
                if let Some(entity) = self.entities.get_mut(id) {
                    entity.components.insert(component.clone(), next);
                }
                let inverse = match previous {
                    Some(old) => EditOp::Replace {
                        id: id.clone(),
                        component: component.clone(),
                        value: old,
                    },
                    None => EditOp::Remove {
                        id: id.clone(),
                        component: component.clone(),
                    },
                };
                Ok(OpOutcome {
                    inverse: vec![inverse],
                    changes: vec![component_pointer(id, component)],
                })
            }
            EditOp::Remove { id, component } => {
                let entity = self.require(id)?;
                if !entity.components.contains_key(component) {
                    let names: Vec<&str> = entity.components.keys().map(String::as_str).collect();
                    return Err(Diagnostic::error(
                        "component_not_found",
                        format!("Entity '{id}' has no '{component}' component."),
                        unknown_name_hint(component, &names),
                    )
                    .with_candidates(names.iter().copied())
                    .at_path(component_pointer(id, component)));
                }
                let old = self
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.components.remove(component))
                    .unwrap_or(Value::Object(Map::new()));
                Ok(OpOutcome {
                    inverse: vec![EditOp::Replace {
                        id: id.clone(),
                        component: component.clone(),
                        value: old,
                    }],
                    changes: vec![component_pointer(id, component)],
                })
            }
            EditOp::Reparent { id, parent, order } => {
                let entity = self.require(id)?;
                let (old_parent, old_order) = (entity.parent.clone(), entity.order);
                self.check_parent(id, parent.as_deref())?;
                if let Some(entity) = self.entities.get_mut(id) {
                    entity.parent = parent.clone();
                    if let Some(order) = order {
                        entity.order = *order;
                    }
                }
                Ok(OpOutcome {
                    inverse: vec![EditOp::Reparent {
                        id: id.clone(),
                        parent: old_parent,
                        order: Some(old_order),
                    }],
                    changes: vec![format!("{}/parent", entity_pointer(id))],
                })
            }
            EditOp::Rename { id, to } => {
                self.require(id)?;
                self.check_new_id(to)?;
                let mut changes = vec![entity_pointer(id), entity_pointer(to)];
                if let Some(entity) = self.entities.remove(id) {
                    self.entities.insert(to.clone(), entity);
                }
                for (child, entity) in &mut self.entities {
                    if entity.parent.as_deref() == Some(id.as_str()) {
                        entity.parent = Some(to.clone());
                        changes.push(format!("{}/parent", entity_pointer(child)));
                    }
                }
                Ok(OpOutcome {
                    inverse: vec![EditOp::Rename {
                        id: to.clone(),
                        to: id.clone(),
                    }],
                    changes,
                })
            }
            EditOp::SetNotes { id, notes } => {
                let old = self.require(id)?.notes.clone();
                if let Some(entity) = self.entities.get_mut(id) {
                    entity.notes = notes.clone();
                }
                Ok(OpOutcome {
                    inverse: vec![EditOp::SetNotes {
                        id: id.clone(),
                        notes: old,
                    }],
                    changes: vec![format!("{}/notes", entity_pointer(id))],
                })
            }
            EditOp::SetResource { name, value } | EditOp::ReplaceResource { name, value } => {
                let info = schema_check::resource(registry, name)
                    .map_err(|problem| problem.at_path(resource_pointer(name)))?;
                let previous = self.resources.get(name).cloned();
                let next = match (op, &previous) {
                    (EditOp::SetResource { .. }, Some(existing)) => {
                        let mut merged = existing.clone();
                        merge(&mut merged, value);
                        merged
                    }
                    _ => value.clone(),
                };
                schema_check::check_value(info.schema(), &next, &resource_pointer(name))?;
                self.resources.insert(name.clone(), next);
                let inverse = match previous {
                    Some(old) => EditOp::ReplaceResource {
                        name: name.clone(),
                        value: old,
                    },
                    None => EditOp::RemoveResource { name: name.clone() },
                };
                Ok(OpOutcome {
                    inverse: vec![inverse],
                    changes: vec![resource_pointer(name)],
                })
            }
            EditOp::RemoveResource { name } => {
                let Some(old) = self.resources.remove(name) else {
                    let names: Vec<&str> = self.resources.keys().map(String::as_str).collect();
                    return Err(Diagnostic::error(
                        "resource_not_found",
                        format!("The scene sets no '{name}' resource."),
                        unknown_name_hint(name, &names),
                    )
                    .with_candidates(names.iter().copied())
                    .at_path(resource_pointer(name)));
                };
                Ok(OpOutcome {
                    inverse: vec![EditOp::ReplaceResource {
                        name: name.clone(),
                        value: old,
                    }],
                    changes: vec![resource_pointer(name)],
                })
            }
        }
    }
}
