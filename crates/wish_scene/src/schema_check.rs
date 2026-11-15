//! Checks shared by loading and editing: which types a name refers to, and
//! whether a JSON value is valid for a type. One place, so that a file and
//! an edit that make the same mistake get the same diagnostic.

use serde_json::Value;
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TextTable, TypeInfo};
use wish_schema::{JsonContext, TypeSchema, from_json};

/// The component type registered as `name`, if scenes may write it.
pub(crate) fn component<'r>(registry: &'r Registry, name: &str) -> Result<&'r TypeInfo> {
    let Some(info) = registry
        .component_id(name)
        .and_then(|id| registry.component(id))
    else {
        let names: Vec<&str> = registry
            .components()
            .filter(|(_, info)| !info.is_managed())
            .map(|(_, info)| info.name())
            .collect();
        return Err(Diagnostic::error(
            "unknown_component",
            format!("No component named '{name}' is registered."),
            unknown_name_hint(name, &names),
        )
        .with_candidates(names.iter().copied()));
    };
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

/// Checks a JSON value (a patch on the type's defaults) against a schema.
/// Diagnostics carry JSON pointers below `pointer`.
pub(crate) fn check_value(schema: &TypeSchema, value: &Value, pointer: &str) -> Result<()> {
    // Strings go to a table that is thrown away: only the checking matters.
    let mut texts = TextTable::default();
    let mut context = JsonContext::at_path(&mut texts, pointer);
    from_json(schema, value, &mut context).map(|_| ())
}
