//! The scene document and its validation.
//!
//! A [`SceneDocument`] is a scene as people and agents edit it: entities
//! named by *scene id* (`"player"`), components held as JSON. It does not
//! contain a world. [`load`] reads a file, checks it against the registered
//! component and resource types, and reports every problem it finds with the
//! file, line and column of the offending key or value.
//!
//! # File format (version 1)
//!
//! ```json
//! {
//!   "entities": {
//!     "camera": { "parent": "player", "components": { "Camera": { "fov_y_deg": 60.0 } } },
//!     "player": { "components": { "Transform": { "translation": [0, 1, 0] } } }
//!   },
//!   "format": 1,
//!   "resources": { "Gravity": { "value": [0, -9.81, 0] } }
//! }
//! ```
//!
//! - `format` is required. The other top-level keys are optional.
//! - An entity has `components`, and optionally `parent` (another entity's
//!   id), `order` (an integer that sorts siblings; ties break by id) and
//!   `notes` (free text for people; JSON has no comments).
//! - The hierarchy is described only by `parent` and `order`. Nothing lists
//!   an entity's children, so there is one place to change.
//! - A component's JSON is a *patch* on the type's defaults: fields left out
//!   keep their default.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TextTable};
use wish_schema::{JsonContext, from_json};

use crate::json::{self, Json, Member, Node, Span};

/// The scene file format this build reads and writes.
pub const FORMAT: u32 = 1;

/// The longest scene id.
pub const MAX_SCENE_ID_LEN: usize = 64;

/// Whether `id` is a valid scene id: a lowercase letter followed by
/// lowercase letters, digits and underscores, at most [`MAX_SCENE_ID_LEN`]
/// characters. Such ids need no escaping in file paths, JSON pointers or on
/// the command line.
pub fn is_valid_scene_id(id: &str) -> bool {
    let mut characters = id.chars();
    matches!(characters.next(), Some('a'..='z'))
        && id.len() <= MAX_SCENE_ID_LEN
        && characters.all(|character| matches!(character, 'a'..='z' | '0'..='9' | '_'))
}

/// One entity of a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneEntity {
    parent: Option<String>,
    order: i64,
    components: BTreeMap<String, Value>,
    notes: Option<String>,
}

impl SceneEntity {
    /// The id of this entity's parent, if it has one.
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    /// The sibling order; siblings sort by this, then by id.
    pub fn order(&self) -> i64 {
        self.order
    }

    /// The components, by name, as the patch JSON the file holds.
    pub fn components(&self) -> &BTreeMap<String, Value> {
        &self.components
    }

    /// The notes for people, if any.
    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    fn to_value(&self) -> Value {
        let mut object = Map::new();
        if !self.components.is_empty() {
            object.insert(
                "components".to_owned(),
                Value::Object(
                    self.components
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(parent) = &self.parent {
            object.insert("parent".to_owned(), Value::String(parent.clone()));
        }
        if self.order != 0 {
            object.insert("order".to_owned(), Value::from(self.order));
        }
        if let Some(notes) = &self.notes {
            object.insert("notes".to_owned(), Value::String(notes.clone()));
        }
        Value::Object(object)
    }
}

/// A scene, ready to be edited, saved or turned into a world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneDocument {
    entities: BTreeMap<String, SceneEntity>,
    resources: BTreeMap<String, Value>,
    revision: u64,
}

impl SceneDocument {
    /// An empty scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// The entities, sorted by scene id.
    pub fn entities(&self) -> impl Iterator<Item = (&str, &SceneEntity)> {
        self.entities
            .iter()
            .map(|(id, entity)| (id.as_str(), entity))
    }

    /// One entity.
    pub fn entity(&self, id: &str) -> Option<&SceneEntity> {
        self.entities.get(id)
    }

    /// The scene's resource values, by name, as patch JSON.
    pub fn resources(&self) -> &BTreeMap<String, Value> {
        &self.resources
    }

    /// How many edits the document has had since it was created or loaded.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The entities with no parent, in sibling order.
    pub fn roots(&self) -> Vec<&str> {
        self.siblings(None)
    }

    /// The children of `parent`, in sibling order.
    pub fn children(&self, parent: &str) -> Vec<&str> {
        self.siblings(Some(parent))
    }

    fn siblings(&self, parent: Option<&str>) -> Vec<&str> {
        let mut siblings: Vec<(i64, &str)> = self
            .entities
            .iter()
            .filter(|(_, entity)| entity.parent.as_deref() == parent)
            .map(|(id, entity)| (entity.order, id.as_str()))
            .collect();
        siblings.sort_unstable();
        siblings.into_iter().map(|(_, id)| id).collect()
    }

    /// The document as JSON, in the form the file holds.
    ///
    /// Empty sections are left out, so that the canonical file of an empty
    /// scene is just its format number.
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("format".to_owned(), Value::from(FORMAT));
        if !self.entities.is_empty() {
            object.insert(
                "entities".to_owned(),
                Value::Object(
                    self.entities
                        .iter()
                        .map(|(id, entity)| (id.clone(), entity.to_value()))
                        .collect(),
                ),
            );
        }
        if !self.resources.is_empty() {
            object.insert(
                "resources".to_owned(),
                Value::Object(
                    self.resources
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                ),
            );
        }
        Value::Object(object)
    }

    /// The document as canonical file text.
    pub fn to_canonical(&self) -> Result<String> {
        json::to_canonical(&self.to_value())
    }
}

/// What [`load`] found.
#[derive(Debug)]
pub struct LoadResult {
    /// The document, if the file has no errors. Warnings do not prevent it.
    pub document: Option<SceneDocument>,
    /// Every problem found, in file order, errors and warnings.
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadResult {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Reads and validates a scene file.
///
/// `file` is the project-relative path shown in diagnostics (or `""`).
/// Syntax errors stop at the first one; after that, every semantic problem
/// is collected, so that one run shows everything that needs fixing.
pub fn load(source: &str, file: &str, registry: &Registry) -> LoadResult {
    let root = match json::parse(source, file) {
        Ok(root) => root,
        Err(diagnostic) => {
            return LoadResult {
                document: None,
                diagnostics: vec![diagnostic],
            };
        }
    };
    let mut validator = Validator {
        file,
        root: &root,
        registry,
        diagnostics: Vec::new(),
    };
    let document = validator.run();
    let mut diagnostics = validator.diagnostics;
    let document = if diagnostics.iter().any(Diagnostic::is_error) {
        None
    } else {
        document
    };
    if let Some(document) = &document
        && document.to_canonical().ok().as_deref() != Some(source)
    {
        let warning = Diagnostic::warning(
            "not_canonical",
            "This file is not in canonical form.",
            "Run `wen fmt` to rewrite it. Saving from the editor or through the engine does this automatically.",
        )
        .at_line(1, Some(1));
        diagnostics.push(if file.is_empty() {
            warning
        } else {
            warning.at_file(file)
        });
    }
    LoadResult {
        document,
        diagnostics,
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

struct Validator<'a> {
    file: &'a str,
    root: &'a Node,
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
}

/// An entity read from the file, before its parent link is checked.
struct Pending {
    parent: Option<(String, Span)>,
    order: i64,
    components: BTreeMap<String, Value>,
    notes: Option<String>,
}

fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn join(pointer: &str, segment: &str) -> String {
    format!("{pointer}/{}", escape(segment))
}

fn kind_name(node: &Node) -> &'static str {
    match node.value {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

impl Validator<'_> {
    fn push(&mut self, diagnostic: Diagnostic, span: Span, pointer: &str) {
        let mut diagnostic = diagnostic.at_line(span.line, Some(span.column));
        if !pointer.is_empty() {
            diagnostic = diagnostic.at_path(pointer);
        }
        if !self.file.is_empty() {
            diagnostic = diagnostic.at_file(self.file);
        }
        self.diagnostics.push(diagnostic);
    }

    fn mismatch(&mut self, node: &Node, pointer: &str, what: &str, expected: &str, example: &str) {
        let diagnostic = Diagnostic::error(
            "type_mismatch",
            format!("Expected {expected} for {what}, found {}.", kind_name(node)),
            format!("Write {what} as {expected}, for example {example}."),
        );
        self.push(diagnostic, node.span, pointer);
    }

    fn members<'n>(
        &mut self,
        node: &'n Node,
        pointer: &str,
        what: &str,
        example: &str,
    ) -> Option<&'n [Member]> {
        if let Json::Object(members) = &node.value {
            Some(members)
        } else {
            self.mismatch(node, pointer, what, "an object", example);
            None
        }
    }

    fn unknown_key(&mut self, member: &Member, pointer: &str, owner: &str, valid: &[&str]) {
        let diagnostic = Diagnostic::error(
            "unknown_key",
            format!("{owner} has no key '{}'.", member.key),
            unknown_name_hint(&member.key, valid),
        )
        .with_candidates(valid.iter().copied());
        self.push(diagnostic, member.key_span, &join(pointer, &member.key));
    }

    fn run(&mut self) -> Option<SceneDocument> {
        let root = self.root;
        let members = self.members(root, "", "a scene file", "{ \"format\": 1 }")?;

        self.check_format(root, members);
        let mut entities = BTreeMap::new();
        let mut resources = BTreeMap::new();
        for member in members {
            match member.key.as_str() {
                "format" => {}
                "entities" => entities = self.read_entities(member),
                "resources" => resources = self.read_resources(member),
                _ => self.unknown_key(
                    member,
                    "",
                    "A scene file",
                    &["entities", "format", "resources"],
                ),
            }
        }
        Some(SceneDocument {
            entities,
            resources,
            revision: 0,
        })
    }

    fn check_format(&mut self, root: &Node, members: &[Member]) {
        let Some(member) = members.iter().find(|member| member.key == "format") else {
            self.push(
                Diagnostic::error(
                    "missing_format",
                    "The file has no \"format\" key.",
                    format!("Add \"format\": {FORMAT} at the top level."),
                ),
                root.span,
                "",
            );
            return;
        };
        let value = &member.value;
        match &value.value {
            Json::Number(number) if number.as_u64() == Some(u64::from(FORMAT)) => {}
            Json::Number(number) => {
                let diagnostic = Diagnostic::error(
                    "unsupported_format",
                    format!("This file is format {number}; this version reads format {FORMAT}."),
                    "Run `wen migrate` to upgrade an older file, or update the engine for a newer one.",
                );
                self.push(diagnostic, value.span, "/format");
            }
            _ => self.mismatch(
                value,
                "/format",
                "\"format\"",
                "a number",
                &FORMAT.to_string(),
            ),
        }
    }

    fn read_entities(&mut self, member: &Member) -> BTreeMap<String, SceneEntity> {
        let pointer = "/entities";
        let Some(entries) = self.members(
            &member.value,
            pointer,
            "\"entities\"",
            "{ \"player\": { \"components\": {} } }",
        ) else {
            return BTreeMap::new();
        };

        let mut pending: BTreeMap<String, Pending> = BTreeMap::new();
        for entry in entries {
            let entity_pointer = join(pointer, &entry.key);
            if !is_valid_scene_id(&entry.key) {
                let diagnostic = Diagnostic::error(
                    "invalid_scene_id",
                    format!("'{}' is not a valid entity id.", entry.key),
                    format!(
                        "An id starts with a lowercase letter and then has lowercase letters, digits and underscores, at most {MAX_SCENE_ID_LEN} characters. For example: \"player_1\"."
                    ),
                );
                self.push(diagnostic, entry.key_span, &entity_pointer);
                continue;
            }
            if let Some(read) = self.read_entity(entry, &entity_pointer) {
                pending.insert(entry.key.clone(), read);
            }
        }
        self.resolve_parents(pending)
    }

    fn read_entity(&mut self, entry: &Member, pointer: &str) -> Option<Pending> {
        let members = self.members(
            &entry.value,
            pointer,
            &format!("entity '{}'", entry.key),
            "{ \"components\": {} }",
        )?;
        let mut pending = Pending {
            parent: None,
            order: 0,
            components: BTreeMap::new(),
            notes: None,
        };
        for member in members {
            let member_pointer = join(pointer, &member.key);
            match member.key.as_str() {
                "components" => {
                    pending.components = self.read_components(&entry.key, member, &member_pointer);
                }
                "parent" => match &member.value.value {
                    Json::String(parent) => {
                        pending.parent = Some((parent.clone(), member.value.span));
                    }
                    _ => self.mismatch(
                        &member.value,
                        &member_pointer,
                        "\"parent\"",
                        "a string",
                        "\"player\"",
                    ),
                },
                "order" => match &member.value.value {
                    Json::Number(number) if number.as_i64().is_some() => {
                        pending.order = number.as_i64().unwrap_or(0);
                    }
                    _ => self.mismatch(
                        &member.value,
                        &member_pointer,
                        "\"order\"",
                        "a whole number",
                        "3",
                    ),
                },
                "notes" => match &member.value.value {
                    Json::String(notes) => pending.notes = Some(notes.clone()),
                    _ => self.mismatch(
                        &member.value,
                        &member_pointer,
                        "\"notes\"",
                        "a string",
                        "\"Moved by the input system.\"",
                    ),
                },
                _ => self.unknown_key(
                    member,
                    pointer,
                    &format!("Entity '{}'", entry.key),
                    &["components", "notes", "order", "parent"],
                ),
            }
        }
        Some(pending)
    }

    fn read_components(
        &mut self,
        entity: &str,
        member: &Member,
        pointer: &str,
    ) -> BTreeMap<String, Value> {
        let Some(entries) = self.members(
            &member.value,
            pointer,
            &format!("the components of '{entity}'"),
            "{ \"Transform\": {} }",
        ) else {
            return BTreeMap::new();
        };
        let mut components = BTreeMap::new();
        for entry in entries {
            let component_pointer = join(pointer, &entry.key);
            if self.check_component(entry, &component_pointer) {
                components.insert(entry.key.clone(), entry.value.to_value());
            }
        }
        components
    }

    /// Checks one component of an entity. Returns whether it is valid.
    fn check_component(&mut self, entry: &Member, pointer: &str) -> bool {
        let Some(info) = self
            .registry
            .component_id(&entry.key)
            .and_then(|id| self.registry.component(id))
        else {
            let names: Vec<&str> = self
                .registry
                .components()
                .filter(|(_, info)| !info.is_managed())
                .map(|(_, info)| info.name())
                .collect();
            let diagnostic = Diagnostic::error(
                "unknown_component",
                format!("No component named '{}' is registered.", entry.key),
                unknown_name_hint(&entry.key, &names),
            )
            .with_candidates(names.iter().copied());
            self.push(diagnostic, entry.key_span, pointer);
            return false;
        };
        if info.is_managed() {
            let diagnostic = Diagnostic::error(
                "managed_component",
                format!(
                    "'{}' is kept consistent by the engine and cannot be written in a scene file.",
                    entry.key
                ),
                "The hierarchy comes from each entity's \"parent\" and \"order\"; remove this component.",
            );
            self.push(diagnostic, entry.key_span, pointer);
            return false;
        }
        self.check_value(info.schema(), &entry.value, pointer)
    }

    /// Checks a JSON value against a schema, with every problem located at
    /// the key or value it concerns. Returns whether it is valid.
    fn check_value(
        &mut self,
        schema: &wish_schema::TypeSchema,
        node: &Node,
        pointer: &str,
    ) -> bool {
        // Strings go to a table that is thrown away: only the checking matters.
        let mut texts = TextTable::default();
        let mut context = JsonContext::at_path(&mut texts, pointer);
        match from_json(schema, &node.to_value(), &mut context) {
            Ok(_) => true,
            Err(diagnostic) => {
                let located = diagnostic
                    .location
                    .path
                    .as_deref()
                    .and_then(|path| self.root.locate(path));
                let span = located.map_or(node.span, |located| {
                    if diagnostic.code == "unknown_field" {
                        located.key.unwrap_or(located.value)
                    } else {
                        located.value
                    }
                });
                let path = diagnostic.location.path.clone().unwrap_or_default();
                self.push(diagnostic, span, &path);
                false
            }
        }
    }

    fn read_resources(&mut self, member: &Member) -> BTreeMap<String, Value> {
        let pointer = "/resources";
        let Some(entries) = self.members(
            &member.value,
            pointer,
            "\"resources\"",
            "{ \"Gravity\": {} }",
        ) else {
            return BTreeMap::new();
        };
        let mut resources = BTreeMap::new();
        for entry in entries {
            let resource_pointer = join(pointer, &entry.key);
            let Some(info) = self
                .registry
                .resource_id(&entry.key)
                .and_then(|id| self.registry.resource(id))
            else {
                let names: Vec<&str> = self
                    .registry
                    .resources()
                    .map(|(_, info)| info.name())
                    .collect();
                let diagnostic = Diagnostic::error(
                    "unknown_resource",
                    format!("No resource named '{}' is registered.", entry.key),
                    unknown_name_hint(&entry.key, &names),
                )
                .with_candidates(names.iter().copied());
                self.push(diagnostic, entry.key_span, &resource_pointer);
                continue;
            };
            if self.check_value(info.schema(), &entry.value, &resource_pointer) {
                resources.insert(entry.key.clone(), entry.value.to_value());
            }
        }
        resources
    }

    /// Checks every parent link and builds the entities.
    fn resolve_parents(
        &mut self,
        pending: BTreeMap<String, Pending>,
    ) -> BTreeMap<String, SceneEntity> {
        let ids: Vec<&str> = pending.keys().map(String::as_str).collect();
        let mut parents: BTreeMap<&str, &str> = BTreeMap::new();
        let mut unknown: Vec<(String, Span, String)> = Vec::new();
        for (id, entity) in &pending {
            let Some((parent, span)) = &entity.parent else {
                continue;
            };
            if pending.contains_key(parent) {
                parents.insert(id, parent);
            } else {
                let hint = unknown_name_hint(parent, &ids);
                unknown.push((id.clone(), *span, hint));
            }
        }
        for (id, span, hint) in unknown {
            let parent = pending
                .get(&id)
                .and_then(|entity| entity.parent.as_ref())
                .map_or("", |(parent, _)| parent.as_str());
            let diagnostic = Diagnostic::error(
                "unknown_parent",
                format!("The parent '{parent}' of entity '{id}' does not exist."),
                hint,
            )
            .with_candidates(ids.iter().copied());
            self.push(diagnostic, span, &join(&join("/entities", &id), "parent"));
        }

        // Find loops. Each is reported once, at the parent link of its smallest id.
        let mut visited: BTreeSet<&str> = BTreeSet::new();
        for start in pending.keys().map(String::as_str) {
            let mut path: Vec<&str> = Vec::new();
            let mut current = start;
            loop {
                if let Some(position) = path.iter().position(|id| *id == current) {
                    let cycle = &path[position..];
                    let first = cycle.iter().copied().min().unwrap_or(current);
                    let span = pending
                        .get(first)
                        .and_then(|entity| entity.parent.as_ref())
                        .map_or(Span { line: 1, column: 1 }, |(_, span)| *span);
                    let (message, hint) = if cycle.len() == 1 {
                        (
                            format!("Entity '{first}' cannot be its own parent."),
                            "Remove the \"parent\" key, or name a different entity.".to_owned(),
                        )
                    } else {
                        let mut route: Vec<&str> = cycle.to_vec();
                        route.push(cycle[0]);
                        (
                            format!(
                                "These entities are each other's ancestors: {}.",
                                route.join(" -> ")
                            ),
                            "Remove one of the \"parent\" links in the loop.".to_owned(),
                        )
                    };
                    let diagnostic = Diagnostic::error("hierarchy_cycle", message, hint)
                        .with_candidates(cycle.iter().copied());
                    self.push(diagnostic, span, &join(&join("/entities", first), "parent"));
                    visited.extend(cycle.iter().copied());
                    break;
                }
                if !visited.insert(current) {
                    break;
                }
                path.push(current);
                match parents.get(current) {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }

        pending
            .into_iter()
            .map(|(id, entity)| {
                (
                    id,
                    SceneEntity {
                        parent: entity.parent.map(|(parent, _)| parent),
                        order: entity.order,
                        components: entity.components,
                        notes: entity.notes,
                    },
                )
            })
            .collect()
    }
}
