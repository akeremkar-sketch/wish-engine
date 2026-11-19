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
//! - An entity with `prefab` is an *instance* of that prefab; its
//!   `components` patch the prefab root's, and `overrides` patch the other
//!   prefab entities' (see [`crate::prefab`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;

use crate::edit::History;
use crate::json::{self, Json, Member, Node, Span};
use crate::prefab::{PREFAB_ROOT, PrefabLibrary};
use crate::schema_check;

/// Component patches for the entities of a prefab instance, by prefab
/// entity id, then by component name.
pub type Overrides = BTreeMap<String, BTreeMap<String, Value>>;

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
    pub(crate) parent: Option<String>,
    pub(crate) order: i64,
    pub(crate) components: BTreeMap<String, Value>,
    pub(crate) notes: Option<String>,
    pub(crate) prefab: Option<String>,
    pub(crate) overrides: Overrides,
}

impl SceneEntity {
    /// The path of the prefab this entity is an instance of, if any.
    pub fn prefab(&self) -> Option<&str> {
        self.prefab.as_deref()
    }

    /// For an instance, the component patches of the prefab's other
    /// entities.
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

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

    pub(crate) fn to_value(&self) -> Value {
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
        if let Some(prefab) = &self.prefab {
            object.insert("prefab".to_owned(), Value::String(prefab.clone()));
        }
        if !self.overrides.is_empty() {
            object.insert(
                "overrides".to_owned(),
                Value::Object(
                    self.overrides
                        .iter()
                        .map(|(inner, components)| {
                            let components: Map<String, Value> = components
                                .iter()
                                .map(|(name, value)| (name.clone(), value.clone()))
                                .collect();
                            let mut patch = Map::new();
                            patch.insert("components".to_owned(), Value::Object(components));
                            (inner.clone(), Value::Object(patch))
                        })
                        .collect(),
                ),
            );
        }
        Value::Object(object)
    }
}

/// A scene, ready to be edited, saved or turned into a world.
///
/// Two documents are equal when their content (entities and resources) is;
/// revision numbers, edit history and the prefab library are not compared.
#[derive(Clone, Debug, Default)]
pub struct SceneDocument {
    pub(crate) entities: BTreeMap<String, SceneEntity>,
    pub(crate) resources: BTreeMap<String, Value>,
    pub(crate) revision: u64,
    pub(crate) history: History,
    pub(crate) prefabs: Arc<PrefabLibrary>,
}

impl PartialEq for SceneDocument {
    fn eq(&self, other: &Self) -> bool {
        self.entities == other.entities && self.resources == other.resources
    }
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

    /// The prefabs this scene's instances refer to.
    pub fn prefabs(&self) -> &PrefabLibrary {
        &self.prefabs
    }

    /// Replaces the prefab library, for example after a prefab file
    /// changed. The content is not checked again here: [`Self::expand`]
    /// reports instances whose prefab is missing.
    pub fn set_prefabs(&mut self, prefabs: Arc<PrefabLibrary>) {
        self.prefabs = prefabs;
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

/// What kind of file is being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Scene,
    Prefab,
}

/// Reads and validates a scene file that places no prefabs (an instance is
/// reported as `prefab_not_found`). See [`load_with_prefabs`].
///
/// `file` is the project-relative path shown in diagnostics (or `""`).
/// Syntax errors stop at the first one; after that, every semantic problem
/// is collected, so that one run shows everything that needs fixing.
pub fn load(source: &str, file: &str, registry: &Registry) -> LoadResult {
    load_with(source, file, registry, Arc::default(), FileKind::Scene)
}

/// Reads and validates a scene file whose instances refer to the prefabs
/// in `prefabs`. The document keeps the library for editing and
/// expanding.
pub fn load_with_prefabs(
    source: &str,
    file: &str,
    registry: &Registry,
    prefabs: Arc<PrefabLibrary>,
) -> LoadResult {
    load_with(source, file, registry, prefabs, FileKind::Scene)
}

pub(crate) fn load_with(
    source: &str,
    file: &str,
    registry: &Registry,
    prefabs: Arc<PrefabLibrary>,
    kind: FileKind,
) -> LoadResult {
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
        prefabs: &prefabs,
        kind,
        diagnostics: Vec::new(),
    };
    let document = validator.run().map(|mut document| {
        document.prefabs = Arc::clone(&prefabs);
        document
    });
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
    prefabs: &'a PrefabLibrary,
    kind: FileKind,
    diagnostics: Vec<Diagnostic>,
}

/// An entity read from the file, before its parent link is checked.
struct Pending {
    parent: Option<(String, Span)>,
    order: i64,
    components: BTreeMap<String, Value>,
    notes: Option<String>,
    prefab: Option<String>,
    overrides: Overrides,
}

/// Checks that an instance of `prefab` (at `path`) may override the prefab
/// entity `inner`: it exists and is not the root, whose changes go in the
/// instance's own components.
pub(crate) fn check_override_target(prefab: &SceneDocument, path: &str, inner: &str) -> Result<()> {
    if inner == PREFAB_ROOT {
        return Err(Diagnostic::error(
            "override_on_root",
            "The prefab's root cannot be overridden.",
            "Changes to the prefab's root go in the instance's own \"components\".",
        ));
    }
    if prefab.entity(inner).is_none() {
        let ids: Vec<&str> = prefab
            .entities()
            .map(|(id, _)| id)
            .filter(|id| *id != PREFAB_ROOT)
            .collect();
        let hint = if ids.is_empty() {
            // Only a root: overrides have nothing to point at.
            "This prefab has only its root entity. Change the instance itself instead: give it \"components\" when you instantiate it, or use set / replace on the instance's id.".to_owned()
        } else {
            format!(
                "{} Overrides name one of the prefab's own entities ({}); the instance's root is changed through its own components instead.",
                unknown_name_hint(inner, &ids),
                ids.join(", ")
            )
        };
        return Err(Diagnostic::error(
            "unknown_prefab_entity",
            format!("The prefab '{path}' has no entity '{inner}'."),
            hint,
        )
        .with_candidates(ids.iter().copied()));
    }
    Ok(())
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

impl<'a> Validator<'a> {
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
        let example = format!("{{ \"format\": {FORMAT} }}");
        let members = self.members(root, "", "a scene file", &example)?;

        self.check_format(root, members);
        let mut entities = BTreeMap::new();
        let mut resources = BTreeMap::new();
        let mut entities_span = root.span;
        for member in members {
            match member.key.as_str() {
                "format" => {}
                "entities" => {
                    entities_span = member.key_span;
                    entities = self.read_entities(member);
                }
                "resources" if self.kind == FileKind::Prefab => self.push(
                    Diagnostic::error(
                        "prefab_with_resources",
                        "A prefab file cannot set resources.",
                        "Resources belong to the scene: move them to the scene file that places this prefab.",
                    ),
                    member.key_span,
                    "/resources",
                ),
                "resources" => resources = self.read_resources(member),
                _ => self.unknown_key(
                    member,
                    "",
                    "A scene file",
                    &["entities", "format", "resources"],
                ),
            }
        }
        if self.kind == FileKind::Prefab && !entities.contains_key(PREFAB_ROOT) {
            self.push(
                Diagnostic::error(
                    "prefab_without_root",
                    "A prefab file needs an entity called \"root\".",
                    "Name the prefab's top entity \"root\" and put the others below it; a scene places the prefab through its root.",
                ),
                entities_span,
                "/entities",
            );
        }
        Some(SceneDocument {
            entities,
            resources,
            ..SceneDocument::default()
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
                if self.kind == FileKind::Prefab {
                    self.check_prefab_entity(entry, &entity_pointer, &read);
                }
                pending.insert(entry.key.clone(), read);
            }
        }
        self.resolve_parents(pending)
    }

    /// In a prefab file, the root has no parent and every other entity has
    /// one, so that everything hangs below the root.
    fn check_prefab_entity(&mut self, entry: &Member, pointer: &str, read: &Pending) {
        match (&read.parent, entry.key == PREFAB_ROOT) {
            (Some((_, span)), true) => self.push(
                Diagnostic::error(
                    "prefab_root_with_parent",
                    "The prefab's \"root\" entity has a parent.",
                    "Remove its \"parent\": the root is the top of the prefab, and the scene decides where each instance goes.",
                ),
                *span,
                &join(pointer, "parent"),
            ),
            (None, false) => self.push(
                Diagnostic::error(
                    "prefab_entity_outside_root",
                    format!(
                        "Entity '{}' has no parent, so it is not part of the prefab's root.",
                        entry.key
                    ),
                    "Give it \"parent\": \"root\" (or an entity below the root): everything in a prefab hangs below its root.",
                ),
                entry.key_span,
                pointer,
            ),
            _ => {}
        }
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
            prefab: None,
            overrides: BTreeMap::new(),
        };
        // The prefab first: checking overrides needs it.
        let prefab_member = members.iter().find(|member| member.key == "prefab");
        let prefab = prefab_member.and_then(|member| {
            let read = self.read_prefab(member, &join(pointer, "prefab"));
            pending.prefab = read.as_ref().map(|(path, _)| path.clone());
            read
        });
        for member in members {
            let member_pointer = join(pointer, &member.key);
            match member.key.as_str() {
                "prefab" => {}
                "overrides" if prefab_member.is_none() => self.push(
                    Diagnostic::error(
                        "overrides_without_prefab",
                        format!(
                            "Entity '{}' has \"overrides\" but is not a prefab instance.",
                            entry.key
                        ),
                        "Overrides change the entities of a placed prefab. Add \"prefab\" with the prefab's path, or move the changes into \"components\".",
                    ),
                    member.key_span,
                    &member_pointer,
                ),
                "overrides" => {
                    if let Some((path, prefab)) = &prefab {
                        pending.overrides =
                            self.read_overrides(path, prefab, member, &member_pointer);
                    }
                }
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
                    &[
                        "components",
                        "notes",
                        "order",
                        "overrides",
                        "parent",
                        "prefab",
                    ],
                ),
            }
        }
        Some(pending)
    }

    /// Reads an entity's `prefab` key. Returns the path and the prefab when
    /// it can be placed here.
    fn read_prefab(
        &mut self,
        member: &Member,
        pointer: &str,
    ) -> Option<(String, &'a SceneDocument)> {
        let Json::String(path) = &member.value.value else {
            self.mismatch(
                &member.value,
                pointer,
                "\"prefab\"",
                "a string",
                "\"prefabs/enemy.prefab.json\"",
            );
            return None;
        };
        if self.kind == FileKind::Prefab {
            self.push(
                Diagnostic::error(
                    "nested_prefab",
                    "A prefab cannot place another prefab.",
                    "Prefabs are one level deep for now: copy the other prefab's entities into this one, or place both prefabs in the scene.",
                ),
                member.key_span,
                pointer,
            );
            return None;
        }
        let library = self.prefabs;
        let Some(prefab) = library.get(path) else {
            let paths = library.paths();
            let diagnostic = Diagnostic::error(
                "prefab_not_found",
                format!("No prefab '{path}' is loaded."),
                if paths.is_empty() {
                    "No prefabs are loaded. Check the path (project-relative, such as \"prefabs/enemy.prefab.json\") and that the file exists.".to_owned()
                } else {
                    unknown_name_hint(path, &paths)
                },
            )
            .with_candidates(paths.iter().copied());
            self.push(diagnostic, member.value.span, pointer);
            return None;
        };
        Some((path.clone(), prefab))
    }

    /// Reads an instance's `overrides`: component patches for the prefab's
    /// entities other than the root.
    fn read_overrides(
        &mut self,
        path: &str,
        prefab: &SceneDocument,
        member: &Member,
        pointer: &str,
    ) -> Overrides {
        let Some(entries) = self.members(
            &member.value,
            pointer,
            "\"overrides\"",
            "{ \"weapon\": { \"components\": {} } }",
        ) else {
            return BTreeMap::new();
        };
        let mut overrides = BTreeMap::new();
        for entry in entries {
            let entry_pointer = join(pointer, &entry.key);
            if let Err(diagnostic) = check_override_target(prefab, path, &entry.key) {
                self.push(diagnostic, entry.key_span, &entry_pointer);
                continue;
            }
            let Some(fields) = self.members(
                &entry.value,
                &entry_pointer,
                &format!("the override of '{}'", entry.key),
                "{ \"components\": {} }",
            ) else {
                continue;
            };
            let mut components = BTreeMap::new();
            for field in fields {
                if field.key == "components" {
                    components = self.read_components(
                        &entry.key,
                        field,
                        &join(&entry_pointer, "components"),
                    );
                } else {
                    self.unknown_key(
                        field,
                        &entry_pointer,
                        &format!("The override of '{}'", entry.key),
                        &["components"],
                    );
                }
            }
            if !components.is_empty() {
                overrides.insert(entry.key.clone(), components);
            }
        }
        overrides
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
        match schema_check::component(self.registry, &entry.key) {
            Ok(info) => self.check_value(info.schema(), &entry.value, pointer),
            Err(diagnostic) => {
                self.push(diagnostic, entry.key_span, pointer);
                false
            }
        }
    }

    /// Checks a JSON value against a schema, with every problem located at
    /// the key or value it concerns. Returns whether it is valid.
    fn check_value(
        &mut self,
        schema: &wish_schema::TypeSchema,
        node: &Node,
        pointer: &str,
    ) -> bool {
        match schema_check::check_value(schema, &node.to_value(), pointer) {
            Ok(()) => true,
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
            let info = match schema_check::resource(self.registry, &entry.key) {
                Ok(info) => info,
                Err(diagnostic) => {
                    self.push(diagnostic, entry.key_span, &resource_pointer);
                    continue;
                }
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
                        prefab: entity.prefab,
                        overrides: entity.overrides,
                    },
                )
            })
            .collect()
    }
}
