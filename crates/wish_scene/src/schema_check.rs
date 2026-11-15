//! Checks shared by loading and editing: which types a name refers to, and
//! whether a JSON value is valid for a type. One place, so that a file and
//! an edit that make the same mistake get the same diagnostic.

use serde_json::Value;
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TextTable, TypeInfo};
use wish_schema::{JsonContext, TypeSchema, from_json};

/// Components the engine derives and adds itself, with what to tell someone
/// who writes one in a scene. They are ordinary components in the world, but
/// a scene file cannot set them: the engine would overwrite the value.
const ENGINE_WRITTEN: &[(&str, &str)] = &[
    (
        crate::instantiate::SCENE_ID,
        "added by the engine when a scene is loaded. Remove it: every entity gets a SceneId from its key in the scene file.",
    ),
    (
        "GlobalTransform",
        "computed by the engine from the entity's Transform and its parents'. Remove it and set \"Transform\" instead.",
    ),
];

fn engine_written(name: &str) -> Option<&'static str> {
    ENGINE_WRITTEN
        .iter()
        .find(|(written, _)| *written == name)
        .map(|(_, hint)| *hint)
}

/// The component type registered as `name`, if scenes may write it.
pub(crate) fn component<'r>(registry: &'r Registry, name: &str) -> Result<&'r TypeInfo> {
    let Some(info) = registry
        .component_id(name)
        .and_then(|id| registry.component(id))
    else {
        let names: Vec<&str> = registry
            .components()
            .filter(|(_, info)| !info.is_managed() && engine_written(info.name()).is_none())
            .map(|(_, info)| info.name())
            .collect();
        return Err(Diagnostic::error(
            "unknown_component",
            format!("No component named '{name}' is registered."),
            unknown_name_hint(name, &names),
        )
        .with_candidates(names.iter().copied()));
    };
    if let Some(hint) = engine_written(name) {
        return Err(Diagnostic::error(
            "managed_component",
            format!("'{name}' is written by the engine and cannot be set in a scene."),
            format!("'{name}' is {hint}"),
        ));
    }
    if info.is_managed() {
        return Err(Diagnostic::error(
            "managed_component",
            format!("'{name}' is kept consistent by the engine and cannot be written in a scene."),
            "The hierarchy comes from each entity's \"parent\" and \"order\"; remove this component.",
        ));
    }
    Ok(info)
}

/// The resource type registered as `name`.
pub(crate) fn resource<'r>(registry: &'r Registry, name: &str) -> Result<&'r TypeInfo> {
    registry
        .resource_id(name)
        .and_then(|id| registry.resource(id))
        .ok_or_else(|| {
            let names: Vec<&str> = registry.resources().map(|(_, info)| info.name()).collect();
            Diagnostic::error(
                "unknown_resource",
                format!("No resource named '{name}' is registered."),
                unknown_name_hint(name, &names),
            )
            .with_candidates(names.iter().copied())
        })
}

/// Merges `patch` into `target`: objects merge key by key, anything else
/// is replaced. This mirrors how component JSON patches a value, so a patch
/// merged into a valid patch is still valid.
pub(crate) fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                match target.get_mut(key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, patch) => *target = patch.clone(),
    }
}

/// Checks a JSON value (a patch on the type's defaults) against a schema.
/// Diagnostics carry JSON pointers below `pointer`.
pub(crate) fn check_value(schema: &TypeSchema, value: &Value, pointer: &str) -> Result<()> {
    // Strings go to a table that is thrown away: only the checking matters.
    let mut texts = TextTable::default();
    let mut context = JsonContext::at_path(&mut texts, pointer);
    from_json(schema, value, &mut context).map(|_| ())
}
