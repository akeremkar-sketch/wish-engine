//! The JSON form of scene edits (`edit_apply.ops`), read into
//! `wish_scene::EditOp`s.
//!
//! ```json
//! [
//!   { "op": "spawn", "id": "crate", "parent": "level", "components": {
//!       "Transform": { "translation": [0, 1, 0], "euler_deg": [0, 45, 0] },
//!       "Primitive": { "shape": "Cube" } } },
//!   { "op": "set", "id": "sun", "component": "DirectionalLight", "value": { "illuminance_lux": 2000 } },
//!   { "op": "despawn", "id": "old_crate" }
//! ]
//! ```
//!
//! `euler_deg` is a convenience inside `Transform` values: `[x, y, z]`
//! degrees, turned into the `rotation` quaternion before the edit is made
//! (yaw about Y first, then pitch about X, then roll about Z), so files stay
//! canonical and hold only quaternions.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use wish_core::math::{Quat, Vec3};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_scene::EditOp;

/// The operation names.
pub const OPS: &[&str] = &[
    "spawn",
    "instantiate",
    "despawn",
    "set",
    "replace",
    "remove",
    "reparent",
    "rename",
    "set_override",
    "replace_override",
    "remove_override",
    "set_notes",
    "set_resource",
    "replace_resource",
    "remove_resource",
];

/// The keys each operation takes (besides `op`); the first ones listed are
/// required.
fn keys_of(op: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match op {
        "spawn" => (&["id"], &["parent", "order", "components", "notes"]),
        "instantiate" => (
            &["id", "prefab"],
            &["parent", "order", "components", "overrides", "notes"],
        ),
        "despawn" => (&["id"], &[]),
        "set" | "replace" => (&["id", "component", "value"], &[]),
        "remove" => (&["id", "component"], &[]),
        "reparent" => (&["id"], &["parent", "order"]),
        "rename" => (&["id", "to"], &[]),
        "set_override" | "replace_override" => (&["id", "entity", "component", "value"], &[]),
        "remove_override" => (&["id", "entity", "component"], &[]),
        "set_notes" => (&["id"], &["notes"]),
        "set_resource" | "replace_resource" => (&["name", "value"], &[]),
        "remove_resource" => (&["name"], &[]),
        _ => (&[], &[]),
    }
}

fn op_error(index: usize, message: String, hint: String) -> Diagnostic {
    Diagnostic::error("invalid_op", message, hint)
        .at_op(u32::try_from(index).unwrap_or(u32::MAX))
        .at_path(format!("/ops/{index}"))
}

/// `[x, y, z]` degrees as a quaternion: yaw (Y), then pitch (X), then roll
/// (Z), the order a camera or a character is usually turned in.
pub fn euler_deg_to_quat(degrees: [f64; 3]) -> Quat {
    let [x, y, z] = degrees.map(|d| (d as f32).to_radians());
    Quat::from_axis_angle(Vec3::Y, y)
        * Quat::from_axis_angle(Vec3::X, x)
        * Quat::from_axis_angle(Vec3::Z, z)
}

/// Replaces `euler_deg` in a `Transform` value by `rotation`.
fn expand_euler(component: &str, value: &mut Value, index: usize, pointer: &str) -> Result<()> {
    if component != "Transform" {
        return Ok(());
    }
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(euler) = object.remove("euler_deg") else {
        return Ok(());
    };
    let numbers: Option<Vec<f64>> = euler
        .as_array()
        .filter(|items| items.len() == 3)
        .and_then(|items| items.iter().map(Value::as_f64).collect());
    let Some(numbers) = numbers.filter(|n| n.iter().all(|x| x.is_finite())) else {
        return Err(op_error(
            index,
            "'euler_deg' must be three numbers: degrees about X, Y and Z.".to_owned(),
            "Write it like \"euler_deg\": [0, 90, 0] (yaw 90 degrees).".to_owned(),
        )
        .at_path(format!("{pointer}/euler_deg")));
    };
    if object.contains_key("rotation") {
        return Err(op_error(
            index,
            "A Transform value has both 'rotation' and 'euler_deg'.".to_owned(),
            "Give one of them: 'euler_deg' is turned into 'rotation'.".to_owned(),
        )
        .at_path(pointer.to_owned()));
    }
    let quat = euler_deg_to_quat([numbers[0], numbers[1], numbers[2]]);
    // Rounded so that files do not fill up with float noise.
    let round = |v: f32| (f64::from(v) * 1e7).round() / 1e7;
    object.insert(
        "rotation".to_owned(),
        Value::from(vec![
            round(quat.x),
            round(quat.y),
            round(quat.z),
            round(quat.w),
        ]),
    );
    Ok(())
}

struct Reader<'a> {
    index: usize,
    op: &'a str,
    object: &'a Map<String, Value>,
}

impl Reader<'_> {
    fn pointer(&self, key: &str) -> String {
        format!("/ops/{}/{key}", self.index)
    }

    fn wrong(&self, key: &str, what: &str) -> Diagnostic {
        op_error(
            self.index,
            format!("'{key}' of the {} operation must be {what}.", self.op),
            format!("See the {} example in the edit_apply description.", self.op),
        )
        .at_path(self.pointer(key))
    }

    fn text(&self, key: &str) -> Result<String> {
        match self.object.get(key) {
            Some(Value::String(text)) => Ok(text.clone()),
            _ => Err(self.wrong(key, "a string")),
        }
    }

    fn optional_text(&self, key: &str) -> Result<Option<String>> {
        match self.object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(self.wrong(key, "a string or null")),
        }
    }

    fn order(&self) -> Result<Option<i64>> {
        match self.object.get("order") {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_i64()
                .map(Some)
                .ok_or_else(|| self.wrong("order", "a whole number")),
        }
    }

    fn value(&self, component: &str) -> Result<Value> {
        let mut value = self.object.get("value").cloned().unwrap_or(Value::Null);
        if !value.is_object() {
            return Err(self.wrong("value", "a JSON object of fields"));
        }
        expand_euler(component, &mut value, self.index, &self.pointer("value"))?;
        Ok(value)
    }

    fn components(&self) -> Result<BTreeMap<String, Value>> {
        match self.object.get("components") {
            None | Some(Value::Null) => Ok(BTreeMap::new()),
            Some(Value::Object(map)) => {
                let mut out = BTreeMap::new();
                for (name, value) in map {
                    let mut value = value.clone();
                    if !value.is_object() {
                        return Err(self.wrong(
                            &format!("components/{name}"),
                            "a JSON object of fields ({} for the defaults)",
                        ));
                    }
                    expand_euler(
                        name,
                        &mut value,
                        self.index,
                        &self.pointer(&format!("components/{name}")),
                    )?;
                    out.insert(name.clone(), value);
                }
                Ok(out)
            }
            Some(_) => Err(self.wrong("components", "an object of component name to fields")),
        }
    }

    fn overrides(&self) -> Result<BTreeMap<String, BTreeMap<String, Value>>> {
        match self.object.get("overrides") {
            None | Some(Value::Null) => Ok(BTreeMap::new()),
            Some(Value::Object(entities)) => {
                let mut out = BTreeMap::new();
                for (entity, components) in entities {
                    let Some(components) = components.as_object() else {
                        return Err(self.wrong(
                            &format!("overrides/{entity}"),
                            "an object of component name to fields",
                        ));
                    };
                    let mut patches = BTreeMap::new();
                    for (name, value) in components {
                        let mut value = value.clone();
                        expand_euler(
                            name,
                            &mut value,
                            self.index,
                            &self.pointer(&format!("overrides/{entity}/{name}")),
                        )?;
                        patches.insert(name.clone(), value);
                    }
                    out.insert(entity.clone(), patches);
                }
                Ok(out)
            }
            Some(_) => Err(self.wrong("overrides", "an object of prefab entity to components")),
        }
    }
}

/// Reads one operation.
pub fn parse_op(index: usize, value: &Value) -> Result<EditOp> {
    let Some(object) = value.as_object() else {
        return Err(op_error(
            index,
            format!("Operation {index} is not a JSON object."),
            "Write each operation like { \"op\": \"spawn\", \"id\": \"crate\" }.".to_owned(),
        ));
    };
    let Some(op) = object.get("op").and_then(Value::as_str) else {
        return Err(op_error(
            index,
            format!("Operation {index} has no \"op\" name."),
            format!("Add \"op\": one of {}.", OPS.join(", ")),
        )
        .with_candidates(OPS.iter().copied()));
    };
    if !OPS.contains(&op) {
        return Err(op_error(
            index,
            format!("There is no operation '{op}'."),
            unknown_name_hint(op, OPS),
        )
        .at_path(format!("/ops/{index}/op"))
        .with_candidates(OPS.iter().copied()));
    }
    let (required, optional) = keys_of(op);
    for key in object.keys() {
        if key != "op" && !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            let valid: Vec<&str> = required.iter().chain(optional).copied().collect();
            return Err(op_error(
                index,
                format!("The {op} operation has no key '{key}'."),
                unknown_name_hint(key, &valid),
            )
            .at_path(format!("/ops/{index}/{key}"))
            .with_candidates(valid));
        }
    }
    for key in required {
        if !object.contains_key(*key) {
            return Err(op_error(
                index,
                format!("The {op} operation needs '{key}'."),
                format!(
                    "Add \"{key}\". The {op} operation takes {}.",
                    required
                        .iter()
                        .chain(optional)
                        .map(|k| format!("'{k}'"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    let read = Reader { index, op, object };
    let id = || read.text("id");
    Ok(match op {
        "spawn" => EditOp::Spawn {
            id: id()?,
            parent: read.optional_text("parent")?,
            order: read.order()?.unwrap_or(0),
            components: read.components()?,
            notes: read.optional_text("notes")?,
        },
        "instantiate" => EditOp::Instantiate {
            id: id()?,
            prefab: read.text("prefab")?,
            parent: read.optional_text("parent")?,
            order: read.order()?.unwrap_or(0),
            components: read.components()?,
            overrides: read.overrides()?,
            notes: read.optional_text("notes")?,
        },
        "despawn" => EditOp::Despawn { id: id()? },
        "set" | "replace" => {
            let component = read.text("component")?;
            let value = read.value(&component)?;
            if op == "set" {
                EditOp::Set {
                    id: id()?,
                    component,
                    value,
                }
            } else {
                EditOp::Replace {
                    id: id()?,
                    component,
                    value,
                }
            }
        }
        "remove" => EditOp::Remove {
            id: id()?,
            component: read.text("component")?,
        },
        "reparent" => EditOp::Reparent {
            id: id()?,
            parent: read.optional_text("parent")?,
            order: read.order()?,
        },
        "rename" => EditOp::Rename {
            id: id()?,
            to: read.text("to")?,
        },
        "set_override" | "replace_override" => {
            let component = read.text("component")?;
            let value = read.value(&component)?;
            let (id, entity) = (id()?, read.text("entity")?);
            if op == "set_override" {
                EditOp::SetOverride {
                    id,
                    entity,
                    component,
                    value,
                }
            } else {
                EditOp::ReplaceOverride {
                    id,
                    entity,
                    component,
                    value,
                }
            }
        }
        "remove_override" => EditOp::RemoveOverride {
            id: id()?,
            entity: read.text("entity")?,
            component: read.text("component")?,
        },
        "set_notes" => EditOp::SetNotes {
            id: id()?,
            notes: read.optional_text("notes")?,
        },
        "set_resource" | "replace_resource" => {
            let name = read.text("name")?;
            let value = read.value(&name)?;
            if op == "set_resource" {
                EditOp::SetResource { name, value }
            } else {
                EditOp::ReplaceResource { name, value }
            }
        }
        _ => EditOp::RemoveResource {
            name: read.text("name")?,
        },
    })
}

/// Reads a list of operations.
pub fn parse_ops(value: &Value) -> Result<Vec<EditOp>> {
    let Some(items) = value.as_array() else {
        return Err(Diagnostic::error(
            "invalid_parameter",
            "'ops' must be a list of operations.",
            "Write it like \"ops\": [{ \"op\": \"spawn\", \"id\": \"crate\" }].",
        )
        .at_path("/ops"));
    };
    if items.is_empty() {
        return Err(Diagnostic::error(
            "empty_transaction",
            "'ops' is empty.",
            "Add at least one operation, such as spawn, set or despawn.",
        )
        .at_path("/ops"));
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_op(index, item))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_operation_reads() {
        let ops = parse_ops(&json!([
            { "op": "spawn", "id": "a", "parent": "p", "order": 2, "components": { "Camera": {} }, "notes": "n" },
            { "op": "instantiate", "id": "b", "prefab": "x.prefab.json", "overrides": { "wheel": { "Transform": { "scale": [2, 2, 2] } } } },
            { "op": "despawn", "id": "c" },
            { "op": "set", "id": "a", "component": "Camera", "value": { "near": 1 } },
            { "op": "replace", "id": "a", "component": "Camera", "value": {} },
            { "op": "remove", "id": "a", "component": "Camera" },
            { "op": "reparent", "id": "a", "parent": null },
            { "op": "rename", "id": "a", "to": "z" },
            { "op": "set_override", "id": "b", "entity": "wheel", "component": "Transform", "value": {} },
            { "op": "replace_override", "id": "b", "entity": "wheel", "component": "Transform", "value": {} },
            { "op": "remove_override", "id": "b", "entity": "wheel", "component": "Transform" },
            { "op": "set_notes", "id": "a" },
            { "op": "set_resource", "name": "Sky", "value": { "luminance": 1 } },
            { "op": "replace_resource", "name": "Sky", "value": {} },
            { "op": "remove_resource", "name": "Sky" }
        ]))
        .unwrap();
        assert_eq!(ops.len(), OPS.len());
        assert!(matches!(
            &ops[6],
            EditOp::Reparent {
                parent: None,
                order: None,
                ..
            }
        ));
    }

    #[test]
    fn euler_degrees_become_a_quaternion() {
        let ops = parse_ops(
            &json!([{ "op": "set", "id": "cam", "component": "Transform",
            "value": { "euler_deg": [0, 90, 0] } }]),
        )
        .unwrap();
        let EditOp::Set { value, .. } = &ops[0] else {
            panic!()
        };
        let rotation = value["rotation"].as_array().unwrap();
        let q: Vec<f64> = rotation.iter().map(|v| v.as_f64().unwrap()).collect();
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (q[1] - half).abs() < 1e-6 && (q[3] - half).abs() < 1e-6,
            "{q:?}"
        );
        assert!(value.get("euler_deg").is_none());
        // Yaw then pitch: looking 90 degrees left and 30 down.
        let quat = euler_deg_to_quat([-30.0, 90.0, 0.0]);
        let forward = quat * -Vec3::Z;
        assert!(
            (forward - Vec3::new(-0.866_025, -0.5, 0.0)).length() < 1e-4,
            "{forward:?}"
        );
    }

    #[test]
    fn mistakes_name_the_operation_and_the_key() {
        let cases = [
            (
                json!([{ "op": "spwan", "id": "a" }]),
                "/ops/0/op",
                "Did you mean 'spawn'?",
            ),
            (json!([{ "op": "spawn" }]), "/ops/0", ""),
            (
                json!([{ "op": "set", "id": "a", "component": "C", "valeu": {} }]),
                "/ops/0/valeu",
                "Did you mean 'value'?",
            ),
            (
                json!([{ "op": "despawn", "id": "x" }, { "op": "set", "id": "a", "component": "C", "value": 3 }]),
                "/ops/1/value",
                "",
            ),
            (
                json!([{ "op": "set", "id": "a", "component": "Transform", "value": { "euler_deg": [1, 2] } }]),
                "/ops/0/value/euler_deg",
                "",
            ),
            (json!([{ "op": "spawn", "id": 3 }]), "/ops/0/id", ""),
        ];
        for (ops, path, hint) in cases {
            let problem = parse_ops(&ops).unwrap_err();
            assert_eq!(problem.code, "invalid_op", "{ops}");
            assert_eq!(problem.location.path.as_deref(), Some(path), "{ops}");
            assert!(!problem.hint.is_empty());
            if !hint.is_empty() {
                assert_eq!(problem.hint, hint);
            }
        }
        assert_eq!(parse_ops(&json!([])).unwrap_err().code, "empty_transaction");
        let second =
            parse_ops(&json!([{ "op": "despawn", "id": "x" }, { "op": "bad" }])).unwrap_err();
        assert_eq!(second.location.op, Some(1));
    }
}
