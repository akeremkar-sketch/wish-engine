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

use serde_json::{Map, Value, json};
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
    /// Why, in the actor's words (shown on the change's card in the
    /// activity feed).
    pub reason: Option<String>,
}

impl Transaction {
    /// A transaction with no revision check.
    pub fn new(actor: Actor, ops: Vec<EditOp>) -> Self {
        Self {
            actor,
            base_revision: None,
            ops,
            reason: None,
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
    /// One earlier transaction taken back on its own (selective undo).
    Revert,
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
    /// Why, if the actor said.
    pub reason: Option<String>,
    /// For a revert: the revision of the transaction it took back.
    pub reverts: Option<u64>,
}

#[derive(Clone, Debug)]
struct Entry {
    actor: Actor,
    forward: Vec<EditOp>,
    inverse: Vec<EditOp>,
    /// The revision it produced (when it was applied).
    revision: u64,
    /// For a revert: the revision it took back.
    reverts: Option<u64>,
    /// Taken back by a revert (that has not been undone).
    reverted: bool,
    /// What each changed thing was right after it: taking it back on its
    /// own is safe while every one of them is still so.
    after: Vec<(String, Option<Value>)>,
}

/// The value at a change's JSON pointer, as precisely as the pointer says:
/// a resource, a component, an override, an entity's parent, or (for any
/// other pointer into an entity) the whole entity.
fn value_at(doc: &SceneDocument, pointer: &str) -> Option<Value> {
    if let Some(name) = pointer.strip_prefix("/resources/") {
        return doc.resources.get(name).cloned();
    }
    let rest = pointer.strip_prefix("/entities/")?;
    let (id, inner) = rest.split_once('/').unwrap_or((rest, ""));
    let entity = doc.entities.get(id)?;
    let parts: Vec<&str> = inner.split('/').collect();
    match parts.as_slice() {
        ["components", component] => entity.components.get(*component).cloned(),
        ["parent"] => Some(json!(entity.parent)),
        ["overrides", part, "components", component] => entity
            .overrides
            .get(*part)
            .and_then(|all| all.get(*component))
            .cloned(),
        _ => Some(json!({
            "parent": entity.parent,
            "order": entity.order,
            "components": entity.components,
            "notes": entity.notes,
            "prefab": entity.prefab,
            "overrides": entity.overrides,
        })),
    }
}

fn values_after(doc: &SceneDocument, changes: &[String]) -> Vec<(String, Option<Value>)> {
    changes
        .iter()
        .map(|pointer| (pointer.clone(), value_at(doc, pointer)))
        .collect()
}

/// Whether two JSON pointers touch the same thing: equal, or one inside the
/// other (`/entities/x` and `/entities/x/components/T`).
fn overlaps(a: &str, b: &str) -> bool {
    let inside = |outer: &str, inner: &str| {
        inner.len() > outer.len()
            && inner.starts_with(outer)
            && inner.as_bytes().get(outer.len()) == Some(&b'/')
    };
    a == b || inside(a, b) || inside(b, a)
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
        let applied = self.record(
            transaction.actor.clone(),
            Action::Edit,
            changes,
            transaction.reason.clone(),
            None,
        );
        let after = values_after(self, &applied.changes);
        self.push_entry(Entry {
            actor: transaction.actor.clone(),
            forward: transaction.ops.clone(),
            inverse,
            revision: applied.revision,
            reverts: None,
            reverted: false,
            after,
        });
        Ok(applied)
    }

    fn push_entry(&mut self, entry: Entry) {
        self.history.redo.clear();
        self.history.undo.push_back(entry);
        if self.history.undo.len() > MAX_UNDO {
            self.history.undo.pop_front();
        }
    }

    /// Marks the transaction of `revision` as reverted or not (a revert
    /// undone puts it back in force).
    fn mark_reverted(&mut self, revision: Option<u64>, reverted: bool) {
        let Some(revision) = revision else {
            return;
        };
        if let Some(entry) = self
            .history
            .undo
            .iter_mut()
            .find(|entry| entry.revision == revision)
        {
            entry.reverted = reverted;
        }
    }

    /// Whether the transaction that produced `revision` can be taken back
    /// on its own now; if not, why (as the refusal [`Self::revert`] gives).
    pub fn revertible(&self, revision: u64) -> Result<()> {
        let Some(entry) = self
            .history
            .undo
            .iter()
            .find(|entry| entry.revision == revision)
        else {
            let known = self.history.log.iter().any(|log| log.revision == revision);
            return Err(Diagnostic::error(
                "not_revertible",
                format!("The change of revision {revision} cannot be taken back on its own."),
                if known {
                    "It was undone already, was itself an undo or redo, or is older than a change made outside the engine (which replaced the scene). Undo or edit by hand instead."
                } else {
                    "No change has that revision in this scene's history; edit_history lists the revisions."
                },
            ));
        };
        if entry.reverted {
            return Err(Diagnostic::error(
                "already_reverted",
                format!("The change of revision {revision} was taken back already."),
                "edit_history lists the revert; undo it to bring the change back.",
            ));
        }
        // Safe while everything it changed is still as it left it; else
        // taking it back would undo part of a later change (named from the
        // log: the later changes that touched what moved).
        let moved: Vec<&String> = entry
            .after
            .iter()
            .filter(|(pointer, after)| value_at(self, pointer) != *after)
            .map(|(pointer, _)| pointer)
            .collect();
        if !moved.is_empty() {
            let mut later: Vec<String> = self
                .history
                .log
                .iter()
                .filter(|log| log.revision > revision)
                .filter(|log| {
                    log.changes
                        .iter()
                        .any(|later| moved.iter().any(|own| overlaps(own, later)))
                })
                .map(|log| format!("r{} by {}", log.revision, log.actor.label()))
                .collect();
            if later.is_empty() {
                later.push("a change no longer in the history".to_owned());
            }
            return Err(Diagnostic::error(
                "revert_conflict",
                format!(
                    "Later changes touched what revision {revision} changed: {}.",
                    later.join(", ")
                ),
                "Take those back first (newest first), or change the scene by hand: reverting now would undo part of them.",
            ));
        }
        if self
            .history
            .log
            .front()
            .is_some_and(|first| first.revision > revision)
        {
            return Err(Diagnostic::error(
                "not_revertible",
                format!(
                    "Revision {revision} is older than the history kept, so later changes cannot be checked."
                ),
                "Edit by hand instead.",
            ));
        }
        Ok(())
    }

    /// Takes back the transaction that produced `revision`, and only it,
    /// if no later change touched what it changed. The revert is a new
    /// transaction by `actor` (it can be undone like any other).
    pub fn revert(
        &mut self,
        revision: u64,
        actor: Actor,
        reason: Option<String>,
        registry: &Registry,
    ) -> Result<Applied> {
        self.revertible(revision)?;
        let Some(entry) = self
            .history
            .undo
            .iter()
            .find(|entry| entry.revision == revision)
            .cloned()
        else {
            return self.revertible(revision).map(|()| Applied {
                revision: self.revision,
                changes: Vec::new(),
            });
        };
        let (content, changes, _) = self.run(&entry.inverse, registry)?;
        self.commit(content);
        let applied = self.record(
            actor.clone(),
            Action::Revert,
            changes,
            reason,
            Some(revision),
        );
        self.mark_reverted(Some(revision), true);
        let after = values_after(self, &applied.changes);
        self.push_entry(Entry {
            actor,
            forward: entry.inverse.clone(),
            inverse: entry.forward.clone(),
            revision: applied.revision,
            reverts: Some(revision),
            reverted: false,
            after,
        });
        Ok(applied)
    }

    /// Whether the transaction of `revision` was taken back by a revert
    /// that is still in force.
    pub fn is_reverted(&self, revision: u64) -> bool {
        self.history
            .undo
            .iter()
            .any(|entry| entry.revision == revision && entry.reverted)
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
                // Undoing a revert puts the change it took back in force.
                self.mark_reverted(entry.reverts, false);
                self.history.redo.push(entry);
                Ok(Some(self.record(actor, Action::Undo, changes, None, None)))
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
                let reverts = entry.reverts;
                self.history.undo.push_back(entry);
                self.mark_reverted(reverts, true);
                Ok(Some(self.record(actor, Action::Redo, changes, None, None)))
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

    /// Replaces the whole content with `other`'s: for a change made outside
    /// the engine that the operations cannot express. It is recorded as one
    /// entry by `actor` touching every entity and resource that differs, and
    /// the revision goes on counting (so `base_revision` checks and "what
    /// changed since" stay right). Earlier changes can no longer be undone:
    /// their inverses do not fit the new content.
    pub fn replace_content(&mut self, other: &SceneDocument, actor: Actor) -> Applied {
        let mut changes = Vec::new();
        let ids: BTreeSet<&String> = self.entities.keys().chain(other.entities.keys()).collect();
        for id in ids {
            if self.entities.get(id) != other.entities.get(id) {
                changes.push(entity_pointer(id));
            }
        }
        let names: BTreeSet<&String> = self
            .resources
            .keys()
            .chain(other.resources.keys())
            .collect();
        for name in names {
            if self.resources.get(name) != other.resources.get(name) {
                changes.push(resource_pointer(name));
            }
        }
        self.entities = other.entities.clone();
        self.resources = other.resources.clone();
        self.history.undo.clear();
        self.history.redo.clear();
        self.record(actor, Action::Edit, changes, None, None)
    }

    fn commit(&mut self, content: SceneDocument) {
        self.entities = content.entities;
        self.resources = content.resources;
    }

    fn record(
        &mut self,
        actor: Actor,
        action: Action,
        changes: Vec<String>,
        reason: Option<String>,
        reverts: Option<u64>,
    ) -> Applied {
        self.revision += 1;
        self.history.log.push_back(LogEntry {
            revision: self.revision,
            actor,
            action,
            changes: changes.clone(),
            reason,
            reverts,
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
                // Report what actually changed; a move to the same place
                // still names the parent link it addressed.
                let order_changed = order.is_some_and(|order| order != old_order);
                let mut changes = Vec::new();
                if *parent != old_parent || !order_changed {
                    changes.push(format!("{}/parent", entity_pointer(id)));
                }
                if order_changed {
                    changes.push(format!("{}/order", entity_pointer(id)));
                }
                Ok(OpOutcome {
                    inverse: vec![EditOp::Reparent {
                        id: id.clone(),
                        parent: old_parent,
                        order: Some(old_order),
                    }],
                    changes,
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
