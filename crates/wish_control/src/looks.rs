//! Named looks (M12 package 9, the look lock): a full set of values for
//! every look setting, applied to a scene in one step with the `edit_apply`
//! operation `apply_look`.
//!
//! A look is data (`looks.json`): for each look resource (`Ambient`,
//! `Bloom`, `ColorGrading`, `Fog`, `RenderSettings`, `Sky`) the fields that
//! make up the look, every one of them written out. Applying a look is one
//! `set_resource` per resource with those values, so:
//!
//! - the scene file says what it looks like, whatever the schema defaults
//!   become later (ADR-027, rule 1: a look applied before the default look
//!   changes keeps its picture after it, with no migration);
//! - the fields a look does not own stay as the scene had them: the sky's
//!   colours and brightness, the flat ambient light's colour and strength,
//!   whether shadows are drawn at all and the shadow map's size (a cost
//!   choice), the exposure;
//! - it is an ordinary edit: one entry of the history, undone in one step,
//!   checked by `dry_run`, made by hand from the editor's scene resources.
//!
//! Design: `docs/design/M12-look-lock.md`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_scene::EditOp;

/// The looks, as data.
const LOOKS_JSON: &str = include_str!("looks.json");

/// One named look.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// Its id, as `apply_look` takes it (`natural`).
    pub id: String,
    /// Its name for people (English; the editor translates it).
    pub name: String,
    /// What it looks like, in a sentence or two.
    pub description: String,
    /// For each look resource, the value of every field the look sets.
    pub resources: BTreeMap<String, Map<String, Value>>,
}

fn broken(message: String) -> Diagnostic {
    Diagnostic::error(
        "looks_invalid",
        message,
        "The engine's list of looks (wish_control/src/looks.json) is damaged; reinstall the engine.",
    )
}

/// Every look, in the order they are offered.
pub fn looks() -> Result<Vec<Look>> {
    let root: Value = serde_json::from_str(LOOKS_JSON)
        .map_err(|error| broken(format!("The list of looks is not JSON: {error}.")))?;
    let items = root
        .get("looks")
        .and_then(Value::as_array)
        .ok_or_else(|| broken("The list of looks has no \"looks\" array.".to_owned()))?;
    items
        .iter()
        .map(|item| {
            let text = |key: &str| {
                item.get(key)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| broken(format!("A look has no \"{key}\".")))
            };
            let resources = item
                .get("resources")
                .and_then(Value::as_object)
                .ok_or_else(|| broken("A look has no \"resources\".".to_owned()))?
                .iter()
                .map(|(name, fields)| {
                    fields
                        .as_object()
                        .map(|fields| (name.clone(), fields.clone()))
                        .ok_or_else(|| broken(format!("A look's '{name}' is not an object.")))
                })
                .collect::<Result<_>>()?;
            Ok(Look {
                id: text("id")?,
                name: text("name")?,
                description: text("description")?,
                resources,
            })
        })
        .collect()
}

/// The look called `id`, or `unknown_look` with the names there are.
pub fn find(id: &str) -> Result<Look> {
    let all = looks()?;
    let ids: Vec<String> = all.iter().map(|look| look.id.clone()).collect();
    all.into_iter().find(|look| look.id == id).ok_or_else(|| {
        let names: Vec<&str> = ids.iter().map(String::as_str).collect();
        Diagnostic::error(
            "unknown_look",
            format!("There is no look called '{id}'."),
            unknown_name_hint(id, &names),
        )
        .with_candidates(names.iter().copied())
    })
}

/// The scene edits that apply `look`: one `set_resource` per resource, in
/// name order, with every field the look owns written out.
pub fn ops_for(look: &Look) -> Vec<EditOp> {
    look.resources
        .iter()
        .map(|(name, fields)| EditOp::SetResource {
            name: name.clone(),
            value: Value::Object(fields.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_looks_read_and_each_sets_the_same_fields() {
        let all = looks().unwrap();
        assert_eq!(
            all.iter().map(|look| look.id.as_str()).collect::<Vec<_>>(),
            ["plain", "natural", "vivid"]
        );
        let fields = |look: &Look| -> Vec<String> {
            look.resources
                .iter()
                .flat_map(|(name, fields)| {
                    fields.keys().map(move |field| format!("{name}.{field}"))
                })
                .collect()
        };
        for look in &all {
            assert_eq!(
                fields(look),
                fields(&all[0]),
                "{} sets other fields",
                look.id
            );
            assert!(!look.description.is_empty() && !look.name.is_empty());
        }
        assert_eq!(fields(&all[0]).len(), 20);
    }

    #[test]
    fn an_unknown_look_names_the_ones_there_are() {
        let problem = find("natrual").unwrap_err();
        assert_eq!(problem.code, "unknown_look");
        assert_eq!(problem.hint, "Did you mean 'natural'?");
        assert_eq!(problem.candidates, ["plain", "natural", "vivid"]);
        assert_eq!(ops_for(&find("vivid").unwrap()).len(), 6);
    }
}
