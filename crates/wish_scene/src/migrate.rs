//! Keeping a file's meaning when defaults change (ADR-027, rule 1; M12
//! package 9, the look lock).
//!
//! A component or resource in a scene or prefab file is a patch on its
//! type's defaults: the fields left out take the default. A file therefore
//! depends on the defaults of the engine that wrote it without containing
//! them. A default may change only in a planned window, and then the file
//! format number goes up by one; a file of the older number keeps its
//! meaning by having every value it relied on written into it. That is
//! [`pin`]:
//!
//! - a field a component or resource leaves out, whose default the new
//!   engine changed, gets the old default written in;
//! - a resource the file does not set at all, which the engine then uses at
//!   its defaults (the renderer draws a scene without `Fog` with
//!   `Fog::default()`), is added with the old values of the fields whose
//!   default changed;
//! - a part of a prefab instance takes its values from the prefab file
//!   (which is pinned on its own), so it is pinned only where the prefab
//!   does not give that entity the component;
//! - nothing else is touched: a field the file sets keeps its value, a field
//!   whose default did not change stays out, and with no changed default a
//!   file is not changed at all.
//!
//! What the defaults were is data: [`Defaults`], one full value per engine
//! type, recorded for each format in `defaults/format-<n>.json` and kept
//! equal to the engine's own by a test (`wish_control`'s
//! `the_recorded_defaults_are_the_engines`) until the format changes.
//! [`STEPS`] lists the migrations this build can make: one, from format 1
//! (the defaults before the look lock of 1.0) to format 2. The loader runs
//! them in memory on an older file as it reads it (`crate::document`), and
//! `wen migrate` writes the result.
//!
//! Design: `docs/design/M12-look-lock.md`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Result};
use wish_ecs::{Registry, TextTable};

/// The defaults scene and prefab files of format 1 were written against:
/// every engine component and resource, every field written out.
pub const FORMAT_1_DEFAULTS: &str = include_str!("defaults/format-1.json");

/// One migration: files of format `from` become format `from + 1`.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    /// The format it migrates from.
    pub from: u32,
    /// The recorded defaults of that format ([`Defaults::parse`]).
    pub defaults: &'static str,
}

/// The defaults scene and prefab files of format 2 are written against:
/// the default look of 1.0 ("Natural"). Kept equal to the engine's own by a
/// test until the format changes again.
pub const FORMAT_2_DEFAULTS: &str = include_str!("defaults/format-2.json");

/// The record of the format this build writes ([`crate::FORMAT`]).
pub const CURRENT_DEFAULTS: &str = FORMAT_2_DEFAULTS;

/// The migrations this build makes, oldest first. One: format 1 to 2, the
/// switch of the default look (`docs/design/M12-look-lock.md`, "Switched").
/// A later step is added after it, and [`crate::FORMAT`] goes up with it.
pub const STEPS: &[Step] = &[Step {
    from: 1,
    defaults: FORMAT_1_DEFAULTS,
}];

/// Whether a file of format `from`, older than [`crate::FORMAT`], can be
/// brought up to it: every step on the way exists.
pub fn can_upgrade_from(from: u32) -> bool {
    from < crate::FORMAT
        && (from..crate::FORMAT).all(|format| STEPS.iter().any(|step| step.from == format))
}

/// The oldest format this build reads.
pub fn oldest_readable() -> u32 {
    (0..crate::FORMAT)
        .rev()
        .take_while(|format| can_upgrade_from(*format))
        .last()
        .unwrap_or(crate::FORMAT)
}

/// What kind of file is pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A scene (`*.scene.json`): entities and resources.
    Scene,
    /// A prefab (`*.prefab.json`): entities only.
    Prefab,
}

/// The default value of every type, by name: a JSON object with every field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defaults {
    /// Component types.
    pub components: BTreeMap<String, Value>,
    /// Resource types.
    pub resources: BTreeMap<String, Value>,
}

impl Defaults {
    /// The defaults of every component and resource `registry` holds.
    pub fn of_registry(registry: &Registry) -> Result<Self> {
        let texts = TextTable::default();
        let full = |info: &wish_ecs::TypeInfo| {
            wish_schema::to_json(info.schema(), info.default_bytes(), &texts)
                .map(|value| (info.name().to_owned(), value))
        };
        Ok(Self {
            components: registry
                .components()
                .map(|(_, info)| full(info))
                .collect::<Result<_>>()?,
            resources: registry
                .resources()
                .map(|(_, info)| full(info))
                .collect::<Result<_>>()?,
        })
    }

    /// Reads a record: `{ "format": n, "components": {...}, "resources":
    /// {...} }`.
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = |message: String| {
            Diagnostic::error(
                "defaults_record_invalid",
                message,
                "The engine's records of defaults (wish_scene/src/defaults) are damaged; reinstall the engine.",
            )
        };
        let root: Value = serde_json::from_str(text)
            .map_err(|error| invalid(format!("A record of defaults is not JSON: {error}.")))?;
        // A missing table would read as "no defaults", and a migration would
        // then pin nothing: refused instead.
        let table = |key: &str| -> Result<BTreeMap<String, Value>> {
            root.get(key)
                .and_then(Value::as_object)
                .map(|types| {
                    types
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect()
                })
                .ok_or_else(|| {
                    invalid(format!(
                        "A record of defaults has no \"{key}\" object of types."
                    ))
                })
        };
        Ok(Self {
            components: table("components")?,
            resources: table("resources")?,
        })
    }

    /// These defaults without the types whose value differs from `record`'s
    /// (or that `record` lacks).
    ///
    /// A file is pinned by type name, and only the engine's own types have
    /// recorded defaults. A registry may hold another type of the same name
    /// (a small tool's own `Camera`, a test's `Collider`): its defaults are
    /// not the record's, and the old engine's values must never be written
    /// into it. The engine's own types agree with the record of the current
    /// format (a test keeps them equal), so for them nothing is left out.
    pub fn agreeing_with(&self, record: &Defaults) -> Defaults {
        let agreeing = |own: &BTreeMap<String, Value>, recorded: &BTreeMap<String, Value>| {
            own.iter()
                .filter(|(name, value)| recorded.get(*name) == Some(*value))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        };
        Defaults {
            components: agreeing(&self.components, &record.components),
            resources: agreeing(&self.resources, &record.resources),
        }
    }

    /// The record of these defaults as format `format`'s.
    pub fn to_value(&self, format: u32) -> Value {
        let table = |types: &BTreeMap<String, Value>| {
            Value::Object(
                types
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
            )
        };
        let mut root = Map::new();
        root.insert("format".to_owned(), Value::from(format));
        root.insert("components".to_owned(), table(&self.components));
        root.insert("resources".to_owned(), table(&self.resources));
        Value::Object(root)
    }
}

/// Writes into `patch` (a component's or resource's JSON in a file) every
/// value it relied on in `old` whose default `new` changed. Records the JSON
/// pointer of each value written.
fn pin_patch(
    patch: &mut Map<String, Value>,
    old: &Value,
    new: &Value,
    at: &str,
    out: &mut Vec<String>,
) {
    let (Some(old), Some(new)) = (old.as_object(), new.as_object()) else {
        return;
    };
    for (field, was) in old {
        // A field the new engine no longer has is a removal, which needs a
        // step of its own; it is never written back.
        let Some(now) = new.get(field) else { continue };
        let pointer = format!("{at}/{}", escape(field));
        match patch.get_mut(field) {
            Some(Value::Object(inner)) if was.is_object() && now.is_object() => {
                pin_patch(inner, was, now, &pointer, out);
            }
            Some(_) => {}
            None => {
                if now != was {
                    patch.insert(field.clone(), was.clone());
                    out.push(pointer);
                }
            }
        }
    }
}

/// A JSON pointer token (`~` and `/` escaped).
pub fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Pins the components of one entity's `components` (or one part's in an
/// `overrides`), skipping those `skip` names.
fn pin_components(
    components: &mut Map<String, Value>,
    old: &Defaults,
    new: &Defaults,
    at: &str,
    skip: &dyn Fn(&str) -> bool,
    out: &mut Vec<String>,
) {
    for (name, patch) in components.iter_mut() {
        if skip(name) {
            continue;
        }
        let (Some(was), Some(now), Value::Object(patch)) =
            (old.components.get(name), new.components.get(name), patch)
        else {
            continue;
        };
        pin_patch(patch, was, now, &format!("{at}/{}", escape(name)), out);
    }
}

/// Writes into a scene or prefab file (`file`, its JSON) every value it
/// relied on in `old` whose default is different in `new`, so that under
/// `new` it means what it meant under `old`. `prefab_has(prefab, entity,
/// component)` says whether the prefab file at `prefab` gives its entity
/// `entity` (`root` for the instance itself) that component. Returns the
/// JSON pointers of the values written, in order; empty when nothing
/// changed.
pub fn pin(
    file: &mut Value,
    kind: Kind,
    old: &Defaults,
    new: &Defaults,
    prefab_has: &dyn Fn(&str, &str, &str) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    let Some(root) = file.as_object_mut() else {
        return out;
    };
    if let Some(Value::Object(entities)) = root.get_mut("entities") {
        for (id, entity) in entities.iter_mut() {
            let Value::Object(entity) = entity else {
                continue;
            };
            let prefab = entity
                .get("prefab")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let at = format!("/entities/{}", escape(id));
            if let Some(Value::Object(components)) = entity.get_mut("components") {
                let skip = |component: &str| {
                    prefab
                        .as_deref()
                        .is_some_and(|prefab| prefab_has(prefab, crate::PREFAB_ROOT, component))
                };
                pin_components(
                    components,
                    old,
                    new,
                    &format!("{at}/components"),
                    &skip,
                    &mut out,
                );
            }
            if let (Some(prefab), Some(Value::Object(parts))) =
                (prefab.as_deref(), entity.get_mut("overrides"))
            {
                for (part, patch) in parts.iter_mut() {
                    let Some(Value::Object(components)) = patch.get_mut("components") else {
                        continue;
                    };
                    let skip = |component: &str| prefab_has(prefab, part, component);
                    pin_components(
                        components,
                        old,
                        new,
                        &format!("{at}/overrides/{}/components", escape(part)),
                        &skip,
                        &mut out,
                    );
                }
            }
        }
    }
    // Resources only exist in scene files (a prefab cannot set them).
    if kind == Kind::Prefab {
        return out;
    }
    let mut added = Map::new();
    let resources = match root.get_mut("resources") {
        Some(Value::Object(resources)) => resources,
        // Not an object: the file is broken, and is left as it is
        // ([`upgrade`] refuses it before pinning).
        Some(_) => return out,
        None => &mut added,
    };
    for (name, was) in &old.resources {
        let Some(now) = new.resources.get(name) else {
            continue;
        };
        let at = format!("/resources/{}", escape(name));
        match resources.get_mut(name) {
            Some(Value::Object(patch)) => pin_patch(patch, was, now, &at, &mut out),
            Some(_) => {}
            None => {
                let mut patch = Map::new();
                let before = out.len();
                pin_patch(&mut patch, was, now, &at, &mut out);
                if out.len() > before {
                    resources.insert(name.clone(), Value::Object(patch));
                }
            }
        }
    }
    if !added.is_empty() {
        root.insert("resources".to_owned(), Value::Object(added));
    }
    out
}

/// Migrates a scene or prefab file (`file`, its JSON) from its format to
/// `to` through `steps`, the newest defaults being `current` (the running
/// engine's, [`Defaults::of_registry`]). Returns the values written
/// ([`pin`]). A file already at `to` is left as it is; one newer than `to`
/// or older than the first step is refused.
pub fn upgrade(
    file: &mut Value,
    kind: Kind,
    to: u32,
    steps: &[Step],
    current: &Defaults,
    prefab_has: &dyn Fn(&str, &str, &str) -> bool,
) -> Result<Vec<String>> {
    let from = file
        .get("format")
        .and_then(Value::as_u64)
        .and_then(|format| u32::try_from(format).ok())
        .ok_or_else(|| {
            Diagnostic::error(
                "missing_format",
                "The file has no numeric \"format\" key.",
                format!("Add \"format\": {to} at the top level."),
            )
        })?;
    if from == to {
        return Ok(Vec::new());
    }
    if from > to {
        return Err(Diagnostic::error(
            "unsupported_format",
            format!("The file is format {from}, newer than this engine reads ({to})."),
            "Update the engine; a newer file cannot be downgraded.",
        ));
    }
    // A file whose top level the loader would refuse is not migrated: its
    // values cannot be known, and writing into it would hide the problem.
    for key in ["entities", "resources"] {
        if file.get(key).is_some_and(|value| !value.is_object()) {
            return Err(Diagnostic::error(
                "migration_refused",
                format!("The file's \"{key}\" is not an object, so it cannot be migrated."),
                "Fix the file first (wen check names the problem), then migrate it.",
            )
            .at_path(format!("/{key}")));
        }
    }
    let mut written = Vec::new();
    for format in from..to {
        let Some(index) = steps.iter().position(|step| step.from == format) else {
            return Err(Diagnostic::error(
                "no_migration",
                format!("The file is format {format}, and there is no migration from it."),
                "Restore the file from version control, or open it with the engine version that wrote it.",
            ));
        };
        let old = Defaults::parse(steps[index].defaults)?;
        let next;
        let new = match steps.get(index + 1) {
            Some(step) if step.from == format + 1 => {
                next = Defaults::parse(step.defaults)?;
                &next
            }
            _ => current,
        };
        written.extend(pin(file, kind, &old, new, prefab_has));
        if let Some(root) = file.as_object_mut() {
            root.insert("format".to_owned(), Value::from(format + 1));
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defaults(resources: Value, components: Value) -> Defaults {
        let table = |value: Value| {
            value
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        Defaults {
            components: table(components),
            resources: table(resources),
        }
    }

    fn no_prefab(_: &str, _: &str, _: &str) -> bool {
        false
    }

    #[test]
    fn only_what_a_file_relied_on_and_what_changed_is_written() {
        let old = defaults(
            json!({ "Fog": { "density": 0.0, "start": 0.0 }, "Sky": { "sun": 0.0, "lum": 1.0 }, "Gone": { "a": 1 } }),
            json!({ "Camera": { "fov": 60.0, "near": 0.1, "inner": { "a": 1, "b": 2 } } }),
        );
        let new = defaults(
            json!({ "Fog": { "density": 0.01, "start": 0.0, "added": 3 }, "Sky": { "sun": 0.0, "lum": 1.0 } }),
            json!({ "Camera": { "fov": 50.0, "near": 0.1, "inner": { "a": 1, "b": 5 } } }),
        );
        let mut file = json!({
            "format": 1,
            "entities": {
                "a": { "components": { "Camera": { "near": 0.2 } } },
                "b": { "components": { "Camera": { "fov": 70.0, "inner": { "a": 3 } } } },
                "c": { "components": { "Unknown": {} } }
            }
        });
        let written = pin(&mut file, Kind::Scene, &old, &new, &no_prefab);
        assert_eq!(
            written,
            [
                "/entities/a/components/Camera/fov",
                "/entities/a/components/Camera/inner",
                "/entities/b/components/Camera/inner/b",
                "/resources/Fog/density"
            ]
        );
        assert_eq!(
            file,
            json!({
                "format": 1,
                "entities": {
                    "a": { "components": { "Camera": { "near": 0.2, "fov": 60.0, "inner": { "a": 1, "b": 2 } } } },
                    "b": { "components": { "Camera": { "fov": 70.0, "inner": { "a": 3, "b": 2 } } } },
                    "c": { "components": { "Unknown": {} } }
                },
                "resources": { "Fog": { "density": 0.0 } }
            })
        );
        // Again: nothing more to write.
        assert!(pin(&mut file, Kind::Scene, &old, &new, &no_prefab).is_empty());
        // No changed default: nothing is written at all.
        let mut same =
            json!({ "format": 1, "entities": { "a": { "components": { "Camera": {} } } } });
        let before = same.clone();
        assert!(pin(&mut same, Kind::Scene, &old, &old, &no_prefab).is_empty());
        assert_eq!(same, before);
    }

    #[test]
    fn instances_lean_on_their_prefab_and_prefab_files_get_no_resources() {
        let old = defaults(
            json!({ "Fog": { "density": 0.0 } }),
            json!({ "Camera": { "fov": 60.0 } }),
        );
        let new = defaults(
            json!({ "Fog": { "density": 0.5 } }),
            json!({ "Camera": { "fov": 50.0 } }),
        );
        let prefab_has = |prefab: &str, entity: &str, component: &str| {
            prefab == "p.prefab.json" && entity == "root" && component == "Camera"
        };
        let mut scene = json!({
            "format": 1,
            "entities": {
                "i": { "prefab": "p.prefab.json", "components": { "Camera": {} },
                       "overrides": { "lens": { "components": { "Camera": {} } } } }
            },
            "resources": { "Fog": { "density": 0.2 } }
        });
        let written = pin(&mut scene, Kind::Scene, &old, &new, &prefab_has);
        assert_eq!(
            written,
            ["/entities/i/overrides/lens/components/Camera/fov"]
        );
        let mut prefab =
            json!({ "format": 1, "entities": { "root": { "components": { "Camera": {} } } } });
        let written = pin(&mut prefab, Kind::Prefab, &old, &new, &no_prefab);
        assert_eq!(written, ["/entities/root/components/Camera/fov"]);
        assert!(prefab.get("resources").is_none(), "{prefab}");
    }

    #[test]
    fn an_upgrade_goes_step_by_step_and_refuses_what_it_cannot() {
        const OLD: &str =
            r#"{ "format": 1, "components": {}, "resources": { "Fog": { "density": 0.0 } } }"#;
        let current = defaults(json!({ "Fog": { "density": 0.5 } }), json!({}));
        let steps = [Step {
            from: 1,
            defaults: OLD,
        }];
        let mut file = json!({ "format": 1 });
        let written = upgrade(&mut file, Kind::Scene, 2, &steps, &current, &no_prefab).unwrap();
        assert_eq!(written, ["/resources/Fog/density"]);
        assert_eq!(
            file,
            json!({ "format": 2, "resources": { "Fog": { "density": 0.0 } } })
        );
        assert!(
            upgrade(&mut file, Kind::Scene, 2, &steps, &current, &no_prefab)
                .unwrap()
                .is_empty()
        );
        let codes: Vec<String> = [json!({ "format": 0 }), json!({ "format": 3 }), json!({})]
            .into_iter()
            .map(|mut file| {
                upgrade(&mut file, Kind::Scene, 2, &steps, &current, &no_prefab)
                    .unwrap_err()
                    .code
                    .to_string()
            })
            .collect();
        assert_eq!(
            codes,
            ["no_migration", "unsupported_format", "missing_format"]
        );
        // A file whose resources are not an object is refused, not
        // overwritten; pin alone leaves it as it is.
        let mut broken = json!({ "format": 1, "resources": [1, 2] });
        let problem =
            upgrade(&mut broken, Kind::Scene, 2, &steps, &current, &no_prefab).unwrap_err();
        assert_eq!(problem.code, "migration_refused");
        assert!(
            pin(
                &mut broken,
                Kind::Scene,
                &current,
                &defaults(json!({}), json!({})),
                &no_prefab
            )
            .is_empty()
        );
        let old = defaults(json!({ "Fog": { "density": 0.0 } }), json!({}));
        assert!(pin(&mut broken, Kind::Scene, &old, &current, &no_prefab).is_empty());
        assert_eq!(broken, json!({ "format": 1, "resources": [1, 2] }));
        // A record without its tables is refused, never read as "no defaults".
        for record in [
            r#"{ "format": 1, "components": {} }"#,
            r#"{ "format": 1, "resources": {} }"#,
            r#"{ "format": 1, "components": [], "resources": {} }"#,
            "not json",
        ] {
            assert_eq!(
                Defaults::parse(record).unwrap_err().code,
                "defaults_record_invalid",
                "{record}"
            );
        }
        // The build's own steps: format 1 is read, format 2 is current, and
        // the two records differ where the look lock changed a default.
        assert_eq!(crate::FORMAT, 2);
        assert!(can_upgrade_from(1) && !can_upgrade_from(0) && !can_upgrade_from(2));
        assert_eq!(oldest_readable(), 1);
        let recorded = Defaults::parse(FORMAT_1_DEFAULTS).unwrap();
        let now = Defaults::parse(FORMAT_2_DEFAULTS).unwrap();
        assert_eq!(recorded.resources["Fog"]["density"], 0.0);
        assert_eq!(now.resources["Fog"]["density"], 0.0015);
        assert_eq!(recorded.resources["Ambient"]["source"], "Color");
        assert_eq!(now.resources["Ambient"]["source"], "Sky");
        assert_eq!(
            recorded.components, now.components,
            "no component default changed"
        );
        // A type of the same name with other defaults is not the engine's:
        // it is left out, so nothing of the old record is written into it.
        let mut other = now.clone();
        other
            .resources
            .insert("Fog".to_owned(), json!({ "density": 7 }));
        other.resources.insert("Mine".to_owned(), json!({ "a": 1 }));
        let kept = other.agreeing_with(&now);
        assert!(!kept.resources.contains_key("Fog") && !kept.resources.contains_key("Mine"));
        assert_eq!(kept.resources.len(), now.resources.len() - 1);
        assert_eq!(kept.components, now.components);
    }
}
