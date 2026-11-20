//! Finding asset references in scene, prefab and material files.
//!
//! The component schemas say which fields are [`wish_core::AssetRef`]s, so a
//! file is searched field by field, not by looking for strings that happen to
//! resemble paths.

use serde_json::Value;
use wish_core::asset::split_asset_ref;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;
use wish_scene::json;
use wish_schema::{TypeKind, TypeSchema};

use crate::index::AssetIndex;

/// One asset reference found in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefUse {
    /// The JSON pointer of the string that holds it.
    pub pointer: String,
    /// The reference as written, including any `#sub` part.
    pub reference: String,
}

type Visitor<'a> = dyn FnMut(&str, &mut String) + 'a;

fn walk(schema: &TypeSchema, value: &mut Value, pointer: &str, visit: &mut Visitor<'_>) {
    if !schema.contains_text() {
        return;
    }
    match (&schema.kind, value) {
        (TypeKind::AssetRef, Value::String(reference)) => visit(pointer, reference),
        (TypeKind::Struct(fields), Value::Object(map)) => {
            for field in fields {
                if let Some(inner) = map.get_mut(&field.name) {
                    walk(
                        &field.ty,
                        inner,
                        &format!("{pointer}/{}", field.name),
                        visit,
                    );
                }
            }
        }
        (TypeKind::Array { element, .. }, Value::Array(items)) => {
            for (index, item) in items.iter_mut().enumerate() {
                walk(element, item, &format!("{pointer}/{index}"), visit);
            }
        }
        _ => {}
    }
}

fn walk_components(
    registry: &Registry,
    components: &mut Value,
    pointer: &str,
    visit: &mut Visitor<'_>,
) {
    let Value::Object(map) = components else {
        return;
    };
    for (name, value) in map {
        let schema = registry
            .component_id(name)
            .and_then(|id| registry.component(id))
            .map(wish_ecs::TypeInfo::schema);
        if let Some(schema) = schema {
            walk(schema, value, &format!("{pointer}/{name}"), visit);
        }
    }
}

/// Calls `visit` with the JSON pointer and a mutable string for every asset
/// reference in a scene or prefab file's JSON: in entity components, in the
/// overrides of prefab instances, and in resources. Components or resources
/// the registry does not know are skipped (loading reports them).
pub(crate) fn visit_document(registry: &Registry, document: &mut Value, visit: &mut Visitor<'_>) {
    if let Some(Value::Object(entities)) = document.get_mut("entities") {
        for (id, entity) in entities {
            if let Some(components) = entity.get_mut("components") {
                walk_components(
                    registry,
                    components,
                    &format!("/entities/{id}/components"),
                    visit,
                );
            }
            if let Some(Value::Object(overrides)) = entity.get_mut("overrides") {
                for (inner, patch) in overrides {
                    if let Some(components) = patch.get_mut("components") {
                        walk_components(
                            registry,
                            components,
                            &format!("/entities/{id}/overrides/{inner}/components"),
                            visit,
                        );
                    }
                }
            }
        }
    }
    if let Some(Value::Object(resources)) = document.get_mut("resources") {
        for (name, value) in resources {
            let schema = registry
                .resource_id(name)
                .and_then(|id| registry.resource(id))
                .map(wish_ecs::TypeInfo::schema);
            if let Some(schema) = schema {
                walk(schema, value, &format!("/resources/{name}"), visit);
            }
        }
    }
}

/// Calls `visit` with the JSON pointer and a mutable string for every asset
/// reference in a material file's JSON: the texture fields of its
/// `material` (and its `file`, which a valid material file leaves empty).
pub(crate) fn visit_material(document: &mut Value, visit: &mut Visitor<'_>) {
    if let Some(material) = document.get_mut("material") {
        walk(
            &crate::material::material_schema(),
            material,
            "/material",
            visit,
        );
    }
}

/// Every non-empty asset reference in a material file's JSON, in field
/// order.
pub fn refs_in_material(document: &Value) -> Vec<RefUse> {
    let mut copy = document.clone();
    let mut found = Vec::new();
    visit_material(&mut copy, &mut |pointer, reference| {
        if !reference.is_empty() {
            found.push(RefUse {
                pointer: pointer.to_owned(),
                reference: reference.clone(),
            });
        }
    });
    found
}

/// Every non-empty asset reference in a scene or prefab file's JSON, in
/// file order.
pub fn refs_in_value(registry: &Registry, document: &Value) -> Vec<RefUse> {
    let mut copy = document.clone();
    let mut found = Vec::new();
    visit_document(registry, &mut copy, &mut |pointer, reference| {
        if !reference.is_empty() {
            found.push(RefUse {
                pointer: pointer.to_owned(),
                reference: reference.clone(),
            });
        }
    });
    found
}

/// Checks every asset reference in a scene or prefab file against the
/// project's assets. Returns one diagnostic per bad reference, with the
/// file, line and column of the string; a syntax error is returned as the
/// single diagnostic. A reference of a `Material` to a file of the wrong
/// kind (`Material.file` to something that is not a material file, a
/// texture field to something that is not a PNG) is reported too.
pub fn check_source(
    source: &str,
    file: &str,
    registry: &Registry,
    index: &AssetIndex,
) -> Vec<Diagnostic> {
    let root = match json::parse(source, file) {
        Ok(root) => root,
        Err(diagnostic) => return vec![diagnostic],
    };
    let document = root.to_value();
    let mut diagnostics = Vec::new();
    for used in refs_in_value(registry, &document) {
        // A file that does not exist is one problem, not two.
        let problem = index
            .check(&used.reference)
            .err()
            .or_else(|| crate::material::kind_problem(&used.pointer, &used.reference));
        let Some(problem) = problem else {
            continue;
        };
        let mut problem = problem.at_path(used.pointer.clone());
        if let Some(located) = root.locate(&used.pointer) {
            problem = problem.at_line(located.value.line, Some(located.value.column));
        }
        if !file.is_empty() {
            problem = problem.at_file(file);
        }
        diagnostics.push(problem);
    }
    diagnostics
}

/// Reads and parses a scene or prefab file's JSON.
pub(crate) fn read_document(source: &str, file: &str) -> Result<Value> {
    Ok(json::parse(source, file)?.to_value())
}

/// The path part of a reference, and what follows it (`#sub` or nothing).
pub(crate) fn path_and_suffix(reference: &str) -> (&str, &str) {
    let (path, _) = split_asset_ref(reference);
    (path, &reference[path.len()..])
}
