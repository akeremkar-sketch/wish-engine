//! JSON Schema export.
//!
//! [`json_schema`] turns a [`TypeSchema`] into a JSON Schema (draft 2020-12)
//! object describing the JSON form that [`to_json`](crate::to_json) writes and
//! [`apply_json`](crate::apply_json) reads. AI agents receive these through
//! the control plane, so they never have to guess a field's name, type, unit
//! or range; editors use them for validation and completion.
//!
//! Besides the standard keywords, three extension keywords are emitted:
//!
//! - `x-wish-type`: the engine type name, e.g. `"Vec3"`.
//! - `x-unit`: the unit a field is measured in, e.g. `"m/s"`.
//! - `x-aliases`: names people try instead of the real field name.

use serde_json::{Map, Value, json};

use crate::model::{FieldSchema, TypeKind, TypeSchema};

/// The pattern entity id strings match: `"null"` or `"<index>v<generation>"`.
const ENTITY_PATTERN: &str = "^(null|(0|[1-9][0-9]*)v(0|[1-9][0-9]*))$";

fn number_array(count: usize) -> Value {
    json!({
        "type": "array",
        "items": { "type": "number" },
        "minItems": count,
        "maxItems": count
    })
}

fn integer(min: i128, max: i128) -> Value {
    // Bounds beyond what a JSON number holds exactly are left out.
    match (i64::try_from(min), u64::try_from(max)) {
        (Ok(min), Ok(max)) => json!({ "type": "integer", "minimum": min, "maximum": max }),
        _ => json!({ "type": "integer" }),
    }
}

fn base(schema: &TypeSchema) -> Value {
    match &schema.kind {
        TypeKind::Bool => json!({ "type": "boolean" }),
        TypeKind::I8 => integer(i128::from(i8::MIN), i128::from(i8::MAX)),
        TypeKind::I16 => integer(i128::from(i16::MIN), i128::from(i16::MAX)),
        TypeKind::I32 => integer(i128::from(i32::MIN), i128::from(i32::MAX)),
        TypeKind::I64 => integer(i128::from(i64::MIN), i128::from(i64::MAX)),
        TypeKind::U8 => integer(0, i128::from(u8::MAX)),
        TypeKind::U16 => integer(0, i128::from(u16::MAX)),
        TypeKind::U32 => integer(0, i128::from(u32::MAX)),
        TypeKind::U64 => integer(0, i128::from(u64::MAX)),
        TypeKind::F32 | TypeKind::F64 => json!({ "type": "number" }),
        TypeKind::Vec2 => number_array(2),
        TypeKind::Vec3 => number_array(3),
        TypeKind::Vec4 | TypeKind::Quat | TypeKind::Color => number_array(4),
        TypeKind::Mat4 => number_array(16),
        TypeKind::Entity => json!({ "type": "string", "pattern": ENTITY_PATTERN }),
        TypeKind::Text => json!({ "type": "string" }),
        TypeKind::AssetRef => json!({
            "type": "string",
            "maxLength": wish_core::asset::MAX_ASSET_PATH_LEN,
            "x-asset": true,
            "description": "A project-relative file path, with an optional #sub-object. Empty means no asset.",
        }),
        TypeKind::Enum(variants) => {
            let choices: Vec<Value> = variants
                .iter()
                .map(|variant| {
                    if variant.doc.is_empty() {
                        json!({ "const": variant.name })
                    } else {
                        json!({ "const": variant.name, "description": variant.doc })
                    }
                })
                .collect();
            json!({ "oneOf": choices })
        }
        TypeKind::Array { element, len } => json!({
            "type": "array",
            "items": json_schema(element),
            "minItems": len,
            "maxItems": len
        }),
        TypeKind::Struct(fields) => {
            let properties: Map<String, Value> = fields
                .iter()
                .map(|field| (field.name.clone(), field_schema(field)))
                .collect();
            json!({
                "type": "object",
                "properties": properties,
                "additionalProperties": false
            })
        }
    }
}

/// A range bound as JSON: a whole number for integer fields, so that the
/// schema reads `"minimum": 0` rather than `"minimum": 0.0`.
fn bound(value: f64, integral: bool) -> Value {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53: integers up to here are exact in f64.
    if integral && value.fract() == 0.0 && value.abs() <= EXACT {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn field_schema(field: &FieldSchema) -> Value {
    let mut value = json_schema(&field.ty);
    let Value::Object(object) = &mut value else {
        return value;
    };
    if !field.doc.is_empty() {
        object.insert("description".to_owned(), Value::String(field.doc.clone()));
    }
    if let Some(default) = &field.default {
        object.insert("default".to_owned(), default.clone());
    }
    if let Some(range) = field.range {
        let integral =
            field.ty.kind.is_number() && !matches!(field.ty.kind, TypeKind::F32 | TypeKind::F64);
        object.insert("minimum".to_owned(), bound(range.min, integral));
        object.insert("maximum".to_owned(), bound(range.max, integral));
    }
    if let Some(unit) = &field.unit {
        object.insert("x-unit".to_owned(), Value::String(unit.clone()));
    }
    if !field.aliases.is_empty() {
        object.insert("x-aliases".to_owned(), json!(field.aliases));
    }
    value
}

/// The JSON Schema of a type's JSON form.
///
/// No field is listed as required: fields left out of a new value take their
/// default, and fields left out of a patch keep their current value.
pub fn json_schema(schema: &TypeSchema) -> Value {
    let mut value = base(schema);
    if let Value::Object(object) = &mut value {
        object.insert("x-wish-type".to_owned(), Value::String(schema.name.clone()));
        if matches!(schema.kind, TypeKind::Struct(_) | TypeKind::Enum(_)) {
            object.insert("title".to_owned(), Value::String(schema.name.clone()));
        }
        if !schema.doc.is_empty() {
            object.insert("description".to_owned(), Value::String(schema.doc.clone()));
        }
    }
    value
}
