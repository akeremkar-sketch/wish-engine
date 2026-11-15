//! Converting between stored bytes and JSON by walking a schema.
//!
//! Everything here is safe code over byte slices. It works for any valid
//! schema, including one that arrived from a script module rather than from a
//! Rust type, which is what lets AI agents, the editor and scene files all
//! read and write components they have never seen before.
//!
//! # JSON forms
//!
//! | Kind | JSON |
//! |---|---|
//! | `bool` | `true` / `false` |
//! | integers | integer number |
//! | `f32`, `f64` | number |
//! | `Vec2`, `Vec3`, `Vec4`, `Quat` | `[x, y]`, `[x, y, z]`, `[x, y, z, w]` |
//! | `Color` | `[r, g, b, a]` |
//! | `Mat4` | 16 numbers, column-major |
//! | `Entity` | `"12v3"` or `"null"` |
//! | `Text` | string |
//! | `AssetRef` | string: a project-relative file path, optionally `#sub`; `""` is none |
//! | enum | variant name |
//! | array | array of the same length |
//! | struct | object; fields left out keep their current value |
//!
//! Floating-point numbers that are not finite cannot be written in JSON.
//! Reading them is rejected; writing them produces the strings `"NaN"`,
//! `"Infinity"` and `"-Infinity"` so that a broken world can still be
//! inspected.

use serde_json::{Map, Number, Value};
use wish_core::asset::validate_asset_ref;
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Entity, Result, Text};

use crate::model::{FieldSchema, TypeKind, TypeSchema};
use crate::plain::{entity_from_image, entity_to_image};

/// Where [`Text`](wish_core::Text) handles get their strings from.
///
/// A world's text table implements this. Schema code only needs these two
/// operations, so it does not depend on the world.
pub trait TextStore {
    /// Returns the handle for `text`, adding it to the store if it is new.
    fn intern(&mut self, text: &str) -> Result<Text>;

    /// The string a handle refers to, or `None` for a handle this store never issued.
    fn resolve(&self, text: Text) -> Option<&str>;
}

/// A text store that holds only the empty string.
///
/// For places with no world to hand, such as computing a type's defaults.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoTexts;

impl TextStore for NoTexts {
    fn intern(&mut self, text: &str) -> Result<Text> {
        if text.is_empty() {
            Ok(Text::EMPTY)
        } else {
            Err(Diagnostic::error(
                "text_unavailable",
                format!("The text \"{text}\" cannot be stored here: there is no world to hold it."),
                "Only the empty string can be used outside a world, for example as a default value.",
            ))
        }
    }

    fn resolve(&self, text: Text) -> Option<&str> {
        text.is_empty().then_some("")
    }
}

/// What [`apply_json`] needs besides the schema: where text goes, and where
/// in the surrounding document the value sits (for error locations).
pub struct JsonContext<'a> {
    texts: &'a mut dyn TextStore,
    /// JSON pointer to the value being read.
    path: String,
    /// Dotted name of the value being read, e.g. `Mover.speed`.
    label: String,
}

impl<'a> JsonContext<'a> {
    /// A context for a value at the root of its document.
    pub fn new(texts: &'a mut dyn TextStore) -> Self {
        Self {
            texts,
            path: String::new(),
            label: String::new(),
        }
    }

    /// A context for a value located at `path` (a JSON pointer such as
    /// `/entities/player/components/Mover`) in a larger document. Errors
    /// report locations below that path.
    pub fn at_path(texts: &'a mut dyn TextStore, path: impl Into<String>) -> Self {
        Self {
            texts,
            path: path.into(),
            label: String::new(),
        }
    }

    fn enter<T>(
        &mut self,
        pointer_segment: &str,
        label_segment: &str,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let (path_len, label_len) = (self.path.len(), self.label.len());
        self.path.push('/');
        for character in pointer_segment.chars() {
            match character {
                '~' => self.path.push_str("~0"),
                '/' => self.path.push_str("~1"),
                other => self.path.push(other),
            }
        }
        self.label.push_str(label_segment);
        let result = body(self);
        self.path.truncate(path_len);
        self.label.truncate(label_len);
        result
    }

    fn enter_field<T>(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.enter(name, &format!(".{name}"), body)
    }

    fn enter_index<T>(
        &mut self,
        index: usize,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.enter(&index.to_string(), &format!("[{index}]"), body)
    }

    /// Adds this context's location to a diagnostic that has none.
    fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        if diagnostic.location.path.is_none() && !self.path.is_empty() {
            diagnostic.at_path(self.path.clone())
        } else {
            diagnostic
        }
    }

    fn error(&self, code: &'static str, message: String, hint: impl Into<String>) -> Diagnostic {
        self.locate(Diagnostic::error(code, message, hint))
    }

    fn type_mismatch(&self, schema: &TypeSchema, value: &Value) -> Diagnostic {
        let form = schema.kind.json_form();
        self.error(
            "type_mismatch",
            format!(
                "Expected {form} for '{}', found {}.",
                self.label,
                describe(value)
            ),
            format!(
                "'{}' has type {}, which is written as {form}.",
                self.label, schema.name
            ),
        )
    }

    fn wrong_length(&self, schema: &TypeSchema, expected: usize, found: usize) -> Diagnostic {
        self.error(
            "wrong_length",
            format!(
                "Expected {expected} items for '{}', found {found}.",
                self.label
            ),
            format!(
                "'{}' has type {}, which is written as {}.",
                self.label,
                schema.name,
                schema.kind.json_form()
            ),
        )
    }

    fn check_range(&self, field: Option<&FieldSchema>, value: f64) -> Result<()> {
        let Some(field) = field else {
            return Ok(());
        };
        let Some(range) = field.range else {
            return Ok(());
        };
        if value >= range.min && value <= range.max {
            return Ok(());
        }
        let unit = field
            .unit
            .as_ref()
            .map(|unit| format!(" ({unit})"))
            .unwrap_or_default();
        Err(self.error(
            "value_out_of_range",
            format!(
                "{} = {value} is outside the allowed range {}..={}{unit}.",
                self.label, range.min, range.max
            ),
            "Use a value within the range, or widen the range where the type is defined.",
        ))
    }
}

/// A short description of a JSON value's type, for messages.
fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// The error for bytes whose length does not match the schema. This is never
/// caused by user input: it means engine code paired a schema with the wrong
/// data.
fn layout_mismatch(schema: &TypeSchema, found: usize) -> Diagnostic {
    Diagnostic::error(
        "layout_mismatch",
        format!(
            "A value of type '{}' takes {} bytes, but {found} bytes were given.",
            schema.name, schema.size
        ),
        "This is a bug in the engine or in a script module's type description, not in your data. Please report it.",
    )
}

fn invalid_bytes(schema: &TypeSchema, detail: String) -> Diagnostic {
    Diagnostic::error(
        "invalid_bytes",
        format!(
            "Stored data for '{}' is not a valid value: {detail}",
            schema.name
        ),
        "The data was corrupted or written by code that does not match the type. If a script module wrote it, check that it was built against this version of the type.",
    )
}

fn store(schema: &TypeSchema, bytes: &mut [u8], source: &[u8]) -> Result<()> {
    if bytes.len() == source.len() {
        bytes.copy_from_slice(source);
        Ok(())
    } else {
        Err(layout_mismatch(schema, bytes.len()))
    }
}

fn array<const N: usize>(schema: &TypeSchema, bytes: &[u8]) -> Result<[u8; N]> {
    bytes
        .try_into()
        .map_err(|_| layout_mismatch(schema, bytes.len()))
}

fn slot<'b>(schema: &TypeSchema, bytes: &'b [u8], offset: usize, len: usize) -> Result<&'b [u8]> {
    bytes
        .get(offset..offset + len)
        .ok_or_else(|| layout_mismatch(schema, bytes.len()))
}

fn slot_mut<'b>(
    schema: &TypeSchema,
    bytes: &'b mut [u8],
    offset: usize,
    len: usize,
) -> Result<&'b mut [u8]> {
    let found = bytes.len();
    bytes
        .get_mut(offset..offset + len)
        .ok_or_else(|| layout_mismatch(schema, found))
}

// ---------------------------------------------------------------------------
// Bytes to JSON
// ---------------------------------------------------------------------------

fn non_finite_to_json(value: f64) -> Value {
    let text = if value.is_nan() {
        "NaN"
    } else if value > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    };
    Value::String(text.to_owned())
}

fn f64_to_json(value: f64) -> Value {
    Number::from_f64(value).map_or_else(|| non_finite_to_json(value), Value::Number)
}

fn f32_to_json(value: f32) -> Value {
    if !value.is_finite() {
        return non_finite_to_json(f64::from(value));
    }
    // Go through the shortest decimal that identifies this f32, so that JSON
    // shows 0.1 rather than 0.10000000149011612. Reading it back with
    // `f32_from_json` gives the same f32 bit for bit.
    let widened = value.to_string().parse::<f64>().unwrap_or(f64::from(value));
    f64_to_json(widened)
}

/// Converts stored bytes to JSON.
///
/// `bytes` must hold a valid value of the schema's type; see
/// [`validate_bytes`] for data from an untrusted source.
pub fn to_json(schema: &TypeSchema, bytes: &[u8], texts: &dyn TextStore) -> Result<Value> {
    if bytes.len() != schema.byte_len() {
        return Err(layout_mismatch(schema, bytes.len()));
    }
    Ok(match &schema.kind {
        TypeKind::Bool => Value::Bool(array::<1>(schema, bytes)? != [0]),
        TypeKind::I8 => Value::from(i8::from_le_bytes(array(schema, bytes)?)),
        TypeKind::I16 => Value::from(i16::from_le_bytes(array(schema, bytes)?)),
        TypeKind::I32 => Value::from(i32::from_le_bytes(array(schema, bytes)?)),
        TypeKind::I64 => Value::from(i64::from_le_bytes(array(schema, bytes)?)),
        TypeKind::U8 => Value::from(u8::from_le_bytes(array(schema, bytes)?)),
        TypeKind::U16 => Value::from(u16::from_le_bytes(array(schema, bytes)?)),
        TypeKind::U32 => Value::from(u32::from_le_bytes(array(schema, bytes)?)),
        TypeKind::U64 => Value::from(u64::from_le_bytes(array(schema, bytes)?)),
        TypeKind::F32 => f32_to_json(f32::from_le_bytes(array(schema, bytes)?)),
        TypeKind::F64 => f64_to_json(f64::from_le_bytes(array(schema, bytes)?)),
        TypeKind::Vec2
        | TypeKind::Vec3
        | TypeKind::Vec4
        | TypeKind::Quat
        | TypeKind::Mat4
        | TypeKind::Color => {
            let (floats, _) = bytes.as_chunks::<4>();
            Value::Array(
                floats
                    .iter()
                    .map(|chunk| f32_to_json(f32::from_le_bytes(*chunk)))
                    .collect(),
            )
        }
        TypeKind::Entity => Value::String(entity_from_image(array(schema, bytes)?).to_string()),
        TypeKind::Text | TypeKind::AssetRef => {
            let handle = Text::from_index(u32::from_le_bytes(array(schema, bytes)?));
            match texts.resolve(handle) {
                Some(text) => Value::String(text.to_owned()),
                None => {
                    return Err(invalid_bytes(
                        schema,
                        format!("text handle {} is not in the text table.", handle.index()),
                    ));
                }
            }
        }
        TypeKind::Enum(variants) => {
            let [stored] = array::<1>(schema, bytes)?;
            match variants.iter().find(|variant| variant.value == stored) {
                Some(variant) => Value::String(variant.name.clone()),
                None => {
                    return Err(invalid_bytes(
                        schema,
                        format!("{stored} is not the value of any variant."),
                    ));
                }
            }
        }
        TypeKind::Array { element, len } => {
            let size = element.byte_len();
            let mut items = Vec::with_capacity(*len as usize);
            for index in 0..*len as usize {
                items.push(to_json(
                    element,
                    slot(schema, bytes, index * size, size)?,
                    texts,
                )?);
            }
            Value::Array(items)
        }
        TypeKind::Struct(fields) => {
            let mut members = Map::new();
            for field in fields {
                let field_bytes = slot(schema, bytes, field.offset as usize, field.ty.byte_len())?;
                members.insert(field.name.clone(), to_json(&field.ty, field_bytes, texts)?);
            }
            Value::Object(members)
        }
    })
}

// ---------------------------------------------------------------------------
// JSON to bytes
// ---------------------------------------------------------------------------

fn integer_from_json(
    schema: &TypeSchema,
    field: Option<&FieldSchema>,
    value: &Value,
    min: i128,
    max: i128,
    ctx: &JsonContext<'_>,
) -> Result<i128> {
    let Value::Number(number) = value else {
        return Err(ctx.type_mismatch(schema, value));
    };
    let wide = if let Some(signed) = number.as_i64() {
        i128::from(signed)
    } else if let Some(unsigned) = number.as_u64() {
        i128::from(unsigned)
    } else {
        return Err(ctx.error(
            "type_mismatch",
            format!(
                "Expected an integer for '{}', found the number {number}.",
                ctx.label
            ),
            "Write a whole number without a decimal point or exponent.",
        ));
    };
    if wide < min || wide > max {
        return Err(ctx.error(
            "value_out_of_range",
            format!(
                "{} = {wide} does not fit in {}, which holds {min}..={max}.",
                ctx.label, schema.name
            ),
            format!("Use a whole number from {min} to {max}."),
        ));
    }
    ctx.check_range(field, wide as f64)?;
    Ok(wide)
}

fn f64_from_json(
    schema: &TypeSchema,
    field: Option<&FieldSchema>,
    value: &Value,
    ctx: &JsonContext<'_>,
) -> Result<f64> {
    let Value::Number(number) = value else {
        return Err(ctx.type_mismatch(schema, value));
    };
    let Some(wide) = number.as_f64().filter(|wide| wide.is_finite()) else {
        return Err(ctx.error(
            "value_out_of_range",
            format!("{} = {number} is too large for f64.", ctx.label),
            "Use a finite number.",
        ));
    };
    ctx.check_range(field, wide)?;
    Ok(wide)
}

fn f32_from_json(
    schema: &TypeSchema,
    field: Option<&FieldSchema>,
    value: &Value,
    ctx: &JsonContext<'_>,
) -> Result<f32> {
    let Value::Number(number) = value else {
        return Err(ctx.type_mismatch(schema, value));
    };
    // Parse the number's decimal text straight to f32. Converting the f64 that
    // JSON parsing produced would round twice and could land on a neighbouring
    // f32; the decimal text does not have that problem.
    let narrow = number
        .to_string()
        .parse::<f32>()
        .ok()
        .filter(|narrow| narrow.is_finite());
    let Some(narrow) = narrow else {
        return Err(ctx.error(
            "value_out_of_range",
            format!("{} = {number} is too large for f32.", ctx.label),
            "Use a number between about -3.4e38 and 3.4e38.",
        ));
    };
    if let Some(wide) = number.as_f64() {
        ctx.check_range(field, wide)?;
    }
    Ok(narrow)
}

/// Writes a JSON value over stored bytes.
///
/// For structs this is a *patch*: fields named in `value` are overwritten and
/// all other fields keep what `bytes` already holds. To build a fresh value,
/// use [`from_json`], which starts from the type's defaults.
///
/// On error `bytes` may have been partly overwritten. Apply to a copy when
/// the original must survive a failure.
pub fn apply_json(
    schema: &TypeSchema,
    value: &Value,
    bytes: &mut [u8],
    ctx: &mut JsonContext<'_>,
) -> Result<()> {
    let label_len = ctx.label.len();
    if ctx.label.is_empty() {
        ctx.label.push_str(&schema.name);
    }
    let result = apply(schema, None, value, bytes, ctx);
    ctx.label.truncate(label_len);
    result
}

fn apply(
    schema: &TypeSchema,
    field: Option<&FieldSchema>,
    value: &Value,
    bytes: &mut [u8],
    ctx: &mut JsonContext<'_>,
) -> Result<()> {
    if bytes.len() != schema.byte_len() {
        return Err(layout_mismatch(schema, bytes.len()));
    }

    macro_rules! apply_integer {
        ($ty:ty) => {{
            let wide = integer_from_json(
                schema,
                field,
                value,
                i128::from(<$ty>::MIN),
                i128::from(<$ty>::MAX),
                ctx,
            )?;
            match <$ty>::try_from(wide) {
                Ok(narrow) => store(schema, bytes, &narrow.to_le_bytes()),
                Err(_) => Err(layout_mismatch(schema, bytes.len())),
            }
        }};
    }

    match &schema.kind {
        TypeKind::Bool => {
            let Value::Bool(flag) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            store(schema, bytes, &[u8::from(*flag)])
        }
        TypeKind::I8 => apply_integer!(i8),
        TypeKind::I16 => apply_integer!(i16),
        TypeKind::I32 => apply_integer!(i32),
        TypeKind::I64 => apply_integer!(i64),
        TypeKind::U8 => apply_integer!(u8),
        TypeKind::U16 => apply_integer!(u16),
        TypeKind::U32 => apply_integer!(u32),
        TypeKind::U64 => apply_integer!(u64),
        TypeKind::F32 => {
            let number = f32_from_json(schema, field, value, ctx)?;
            store(schema, bytes, &number.to_le_bytes())
        }
        TypeKind::F64 => {
            let number = f64_from_json(schema, field, value, ctx)?;
            store(schema, bytes, &number.to_le_bytes())
        }
        TypeKind::Vec2
        | TypeKind::Vec3
        | TypeKind::Vec4
        | TypeKind::Quat
        | TypeKind::Mat4
        | TypeKind::Color => {
            let Value::Array(items) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            let count = bytes.len() / 4;
            if items.len() != count {
                return Err(ctx.wrong_length(schema, count, items.len()));
            }
            let component = TypeSchema::primitive("f32", TypeKind::F32);
            let (floats, _) = bytes.as_chunks_mut::<4>();
            for (index, (item, chunk)) in items.iter().zip(floats).enumerate() {
                let number =
                    ctx.enter_index(index, |ctx| f32_from_json(&component, None, item, ctx))?;
                *chunk = number.to_le_bytes();
            }
            Ok(())
        }
        TypeKind::Entity => {
            let Value::String(text) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            let entity: Entity = text.parse().map_err(|problem| ctx.locate(problem))?;
            store(schema, bytes, &entity_to_image(entity))
        }
        TypeKind::Text => {
            let Value::String(text) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            let handle = ctx
                .texts
                .intern(text)
                .map_err(|problem| ctx.locate(problem))?;
            store(schema, bytes, &handle.index().to_le_bytes())
        }
        TypeKind::AssetRef => {
            let Value::String(reference) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            if let Err(problem) = validate_asset_ref(reference) {
                return Err(ctx.error(
                    "invalid_asset_ref",
                    format!("'{reference}' is not a valid asset reference: {problem}."),
                    "Write a path relative to the project folder with '/' between folders, such as \"assets/models/crate.glb\" or \"assets/models/crate.glb#Mesh0\"; use \"\" for no asset.",
                ));
            }
            let handle = ctx
                .texts
                .intern(reference)
                .map_err(|problem| ctx.locate(problem))?;
            store(schema, bytes, &handle.index().to_le_bytes())
        }
        TypeKind::Enum(variants) => {
            let Value::String(name) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            match variants.iter().find(|variant| variant.name == *name) {
                Some(variant) => store(schema, bytes, &[variant.value]),
                None => {
                    let names: Vec<&str> = variants
                        .iter()
                        .map(|variant| variant.name.as_str())
                        .collect();
                    Err(ctx
                        .error(
                            "unknown_variant",
                            format!("'{name}' is not a variant of '{}'.", schema.name),
                            unknown_name_hint(name, &names),
                        )
                        .with_candidates(names))
                }
            }
        }
        TypeKind::Array { element, len } => {
            let Value::Array(items) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            if items.len() != *len as usize {
                return Err(ctx.wrong_length(schema, *len as usize, items.len()));
            }
            let size = element.byte_len();
            for (index, item) in items.iter().enumerate() {
                let element_bytes = slot_mut(schema, bytes, index * size, size)?;
                ctx.enter_index(index, |ctx| apply(element, None, item, element_bytes, ctx))?;
            }
            Ok(())
        }
        TypeKind::Struct(fields) => {
            let Value::Object(members) = value else {
                return Err(ctx.type_mismatch(schema, value));
            };
            for (key, item) in members {
                let Some(target) = fields.iter().find(|candidate| candidate.name == *key) else {
                    return Err(unknown_field(schema, fields, key, ctx));
                };
                let field_bytes =
                    slot_mut(schema, bytes, target.offset as usize, target.ty.byte_len())?;
                ctx.enter_field(&target.name, |ctx| {
                    apply(&target.ty, Some(target), item, field_bytes, ctx)
                })?;
            }
            Ok(())
        }
    }
}

fn unknown_field(
    schema: &TypeSchema,
    fields: &[FieldSchema],
    key: &str,
    ctx: &mut JsonContext<'_>,
) -> Diagnostic {
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    let aliased = fields
        .iter()
        .find(|field| field.aliases.iter().any(|alias| alias == key));
    let hint = match aliased {
        Some(field) => format!("Did you mean '{}'?", field.name),
        None => unknown_name_hint(key, &names),
    };
    let message = format!("'{}' has no field '{key}'.", schema.name);
    // The location points at the unknown key itself.
    let located: Result<()> =
        ctx.enter_field(key, |ctx| Err(ctx.error("unknown_field", message, hint)));
    match located {
        Err(diagnostic) => diagnostic.with_candidates(names),
        Ok(()) => layout_mismatch(schema, schema.byte_len()),
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

fn fill_defaults(schema: &TypeSchema, bytes: &mut [u8]) -> Result<()> {
    if bytes.len() != schema.byte_len() {
        return Err(layout_mismatch(schema, bytes.len()));
    }
    match &schema.kind {
        TypeKind::Quat => {
            // The identity rotation: x, y, z, w = 0, 0, 0, 1.
            store(
                schema,
                slot_mut(schema, bytes, 12, 4)?,
                &1.0f32.to_le_bytes(),
            )
        }
        TypeKind::Mat4 => {
            // The identity matrix: ones on the diagonal.
            for diagonal in [0, 5, 10, 15] {
                store(
                    schema,
                    slot_mut(schema, bytes, diagonal * 4, 4)?,
                    &1.0f32.to_le_bytes(),
                )?;
            }
            Ok(())
        }
        TypeKind::Color => {
            // Opaque white.
            for chunk in bytes.as_chunks_mut::<4>().0 {
                *chunk = 1.0f32.to_le_bytes();
            }
            Ok(())
        }
        TypeKind::Entity => store(schema, bytes, &entity_to_image(Entity::NULL)),
        TypeKind::Enum(variants) => match variants.first() {
            Some(first) => store(schema, bytes, &[first.value]),
            None => Ok(()),
        },
        TypeKind::Array { element, len } => {
            let size = element.byte_len();
            for index in 0..*len as usize {
                fill_defaults(element, slot_mut(schema, bytes, index * size, size)?)?;
            }
            Ok(())
        }
        TypeKind::Struct(fields) => {
            for field in fields {
                let field_bytes =
                    slot_mut(schema, bytes, field.offset as usize, field.ty.byte_len())?;
                fill_defaults(&field.ty, field_bytes)?;
                apply_field_default(&schema.name, field, field_bytes)?;
            }
            Ok(())
        }
        // Zero is the default of every other kind.
        _ => Ok(()),
    }
}

fn apply_field_default(type_name: &str, field: &FieldSchema, bytes: &mut [u8]) -> Result<()> {
    let Some(default) = &field.default else {
        return Ok(());
    };
    let mut texts = NoTexts;
    let mut ctx = JsonContext::new(&mut texts);
    ctx.label = format!("{type_name}.{}", field.name);
    apply(&field.ty, Some(field), default, bytes, &mut ctx)
}

/// Checks that a field's default, if it has one, is a valid value of the
/// field's type and within its range.
pub(crate) fn check_default(type_name: &str, field: &FieldSchema) -> Result<()> {
    let mut bytes = vec![0u8; field.ty.byte_len()];
    fill_defaults(&field.ty, &mut bytes)?;
    apply_field_default(type_name, field, &mut bytes)
}

/// The bytes of a new value of the schema's type, with every field at its default.
pub fn default_bytes(schema: &TypeSchema) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; schema.byte_len()];
    fill_defaults(schema, &mut bytes)?;
    Ok(bytes)
}

/// Builds the bytes of a new value: the type's defaults, overwritten by `value`.
pub fn from_json(schema: &TypeSchema, value: &Value, ctx: &mut JsonContext<'_>) -> Result<Vec<u8>> {
    let mut bytes = default_bytes(schema)?;
    apply_json(schema, value, &mut bytes, ctx)?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Validation of untrusted bytes
// ---------------------------------------------------------------------------

/// Checks that `bytes` hold a valid value of the schema's type.
///
/// Most bytes are valid for most types. The exceptions are `bool`, which
/// accepts only the bytes 0 and 1, and enums, which accept only their declared
/// variants. Viewing any other byte as one of those types from native code
/// would be undefined behaviour, so data written by anything other than
/// engine code, such as a script module, goes through this before it is stored.
pub fn validate_bytes(schema: &TypeSchema, bytes: &[u8]) -> Result<()> {
    if bytes.len() != schema.byte_len() {
        return Err(layout_mismatch(schema, bytes.len()));
    }
    match &schema.kind {
        TypeKind::Bool => match array::<1>(schema, bytes)? {
            [0 | 1] => Ok(()),
            [other] => Err(invalid_bytes(
                schema,
                format!("a bool must be 0 or 1, found {other}."),
            )),
        },
        TypeKind::Enum(variants) => {
            let [stored] = array::<1>(schema, bytes)?;
            if variants.iter().any(|variant| variant.value == stored) {
                Ok(())
            } else {
                Err(invalid_bytes(
                    schema,
                    format!("{stored} is not the value of any variant."),
                ))
            }
        }
        TypeKind::Array { element, len } => {
            if !element.needs_validation() {
                return Ok(());
            }
            let size = element.byte_len();
            for index in 0..*len as usize {
                validate_bytes(element, slot(schema, bytes, index * size, size)?)?;
            }
            Ok(())
        }
        TypeKind::Struct(fields) => {
            for field in fields {
                validate_bytes(
                    &field.ty,
                    slot(schema, bytes, field.offset as usize, field.ty.byte_len())?,
                )
                .map_err(|problem| {
                    invalid_bytes(
                        schema,
                        format!("in field '{}': {}", field.name, problem.message),
                    )
                })?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
