//! Material files: `*.mat.json`, one look shared by many entities.
//!
//! ```json
//! {
//!   "format": 1,
//!   "material": {
//!     "base_color_texture": "assets/textures/brick.png",
//!     "roughness": 0.9,
//!     "uv_scale": [4, 4]
//!   }
//! }
//! ```
//!
//! `material` holds one `MaterialFile` (the schema type of
//! `wish_types::render`: numbers, textures and how they are laid on),
//! written the way a scene writes a component: a patch on the type's
//! defaults, so fields left out keep their default. An entity uses the file
//! by naming it in its `Material.file`.
//!
//! A material file is both an engine document and an asset. Like scenes and
//! prefabs it is strict, canonical JSON with a format number: `wen check`,
//! `wen fmt` and `wen migrate` know it. Like a model or a picture it is in
//! the asset index, because other files refer to it, and `wen mv` rewrites
//! both the references to it and the texture references inside it.
//!
//! A material file refers to pictures only, never to another material
//! file: a look is in one place, and references cannot go in circles.
//!
//! The file is untrusted text. Everything wrong with it is a diagnostic with
//! the file, the line and a hint: a syntax error, a key or field that does
//! not exist, a value of the wrong type or out of range, a number JSON
//! cannot hold, a reference that leaves the project.

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::TextTable;
use wish_scene::json::{self, Json, Node};
use wish_schema::{JsonContext, Plain, TextStore, TypeSchema, from_json};
use wish_types::render::MaterialFile;

use crate::index::AssetIndex;
use crate::refs::RefUse;

/// The material file format this build reads and writes.
pub const MATERIAL_FORMAT: u32 = 1;

/// What a material file's name ends with.
pub const MATERIAL_SUFFIX: &str = ".mat.json";

/// The largest material file that is read, in bytes. A material is a few
/// hundred bytes; anything near this is not a material.
pub const MAX_MATERIAL_BYTES: u64 = 256 * 1024;

/// The top-level keys of a material file.
const KEYS: [&str; 2] = ["format", "material"];

/// Whether a file name is a material file's.
pub fn is_material(name: &str) -> bool {
    name.ends_with(MATERIAL_SUFFIX)
}

/// The schema of what a material file holds: `MaterialFile`.
pub fn material_schema() -> TypeSchema {
    MaterialFile::schema()
}

/// A material file as a document: what it writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialDocument {
    /// The fields the file writes, by name: a patch on the defaults of
    /// `MaterialFile`, checked against its schema.
    pub material: Map<String, Value>,
}

fn at(node: &Node, file: &str, diagnostic: Diagnostic) -> Diagnostic {
    let diagnostic = diagnostic.at_line(node.span.line, Some(node.span.column));
    if file.is_empty() {
        diagnostic
    } else {
        diagnostic.at_file(file)
    }
}

/// Puts the line and column of the value a diagnostic's pointer names on it,
/// and the file.
fn locate(root: &Node, file: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    if let Some(located) = diagnostic
        .location
        .path
        .as_deref()
        .and_then(|pointer| root.locate(pointer))
    {
        let span = located.key.unwrap_or(located.value);
        diagnostic = diagnostic.at_line(span.line, Some(span.column));
    }
    if file.is_empty() {
        diagnostic
    } else {
        diagnostic.at_file(file)
    }
}

/// Reads a material file's text and checks it against the `MaterialFile`
/// schema. `file` is the project-relative path shown in diagnostics (or
/// `""`). The first problem found is returned.
///
/// Whether the textures it names exist is a separate question
/// ([`check_material_refs`]): this needs no project.
pub fn parse_material(source: &str, file: &str) -> Result<MaterialDocument> {
    let root = json::parse(source, file)?;
    let Json::Object(members) = &root.value else {
        return Err(at(
            &root,
            file,
            Diagnostic::error(
                "type_mismatch",
                "A material file must be an object.",
                "Write { \"format\": 1, \"material\": { \"base_color\": [1, 0, 0, 1] } }.",
            ),
        ));
    };
    let mut format_seen = false;
    let mut material = Map::new();
    for member in members {
        match member.key.as_str() {
            "format" => {
                format_seen = true;
                let supported = matches!(&member.value.value, Json::Number(n) if n.as_u64() == Some(u64::from(MATERIAL_FORMAT)));
                if !supported {
                    return Err(at(
                        &member.value,
                        file,
                        Diagnostic::error(
                            "unsupported_format",
                            format!("This material file is not format {MATERIAL_FORMAT}."),
                            "Run `wen migrate` to upgrade an older file, or update the engine for a newer one.",
                        ),
                    ));
                }
            }
            "material" => match member.value.to_value() {
                Value::Object(map) => material = map,
                _ => {
                    return Err(at(
                        &member.value,
                        file,
                        Diagnostic::error(
                            "type_mismatch",
                            "\"material\" must be an object.",
                            "Write the look as { \"field\": value, ... } with the fields of the MaterialFile type (schema_get MaterialFile lists them).",
                        ),
                    ));
                }
            },
            other => {
                let mut diagnostic = Diagnostic::error(
                    "unknown_key",
                    format!("A material file has no key '{other}'."),
                    format!(
                        "{} The fields of the look (base_color, roughness, the textures) go inside \"material\".",
                        unknown_name_hint(other, &KEYS)
                    ),
                )
                .at_line(member.key_span.line, Some(member.key_span.column))
                .with_candidates(KEYS);
                if !file.is_empty() {
                    diagnostic = diagnostic.at_file(file);
                }
                return Err(diagnostic);
            }
        }
    }
    if !format_seen {
        return Err(at(
            &root,
            file,
            Diagnostic::error(
                "missing_format",
                "The material file has no \"format\" key.",
                format!("Add \"format\": {MATERIAL_FORMAT} at the top level."),
            ),
        ));
    }
    let parsed = MaterialDocument { material };
    parsed
        .check()
        .map_err(|problem| locate(&root, file, problem))?;
    Ok(parsed)
}

impl MaterialDocument {
    /// Checks the fields against the `MaterialFile` schema: names, types,
    /// ranges, well-formed references. The diagnostic's pointer starts at
    /// `/material`; it has no file or line.
    pub fn check(&self) -> Result<()> {
        self.decode().map(|_| ())
    }

    /// The file as JSON, in the form the file holds: `material` is left out
    /// when no field is written, so the canonical file of the default look
    /// is just its format number.
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("format".to_owned(), Value::from(MATERIAL_FORMAT));
        if !self.material.is_empty() {
            object.insert("material".to_owned(), Value::Object(self.material.clone()));
        }
        Value::Object(object)
    }

    /// The material file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        json::to_canonical(&self.to_value())
    }

    /// The look the file describes (defaults filled in), with the table
    /// that holds the paths of its texture fields. Fails with the first
    /// field that is not valid; the diagnostic's pointer starts at
    /// `/material`.
    pub fn decode(&self) -> Result<(MaterialFile, TextTable)> {
        let schema = material_schema();
        let mut texts = TextTable::default();
        let bytes = {
            let mut context = JsonContext::at_path(&mut texts, "/material");
            from_json(&schema, &Value::Object(self.material.clone()), &mut context)?
        };
        let material = MaterialFile::decode(&bytes).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                "A checked material could not be read back.",
                "This is a bug in the engine. Please report it.",
            )
        })?;
        Ok((material, texts))
    }

    /// The whole look as JSON: every field of `MaterialFile` with its value,
    /// defaults included (what the file means, not what it writes).
    pub fn effective(&self) -> Result<Value> {
        let schema = material_schema();
        let mut texts = TextTable::default();
        let bytes = {
            let mut context = JsonContext::at_path(&mut texts, "/material");
            from_json(&schema, &Value::Object(self.material.clone()), &mut context)?
        };
        wish_schema::to_json(&schema, &bytes, &texts as &dyn TextStore)
    }

    /// The textures the file names: each non-empty reference with its JSON
    /// pointer (`/material/base_color_texture`), in field order.
    pub fn textures(&self) -> Vec<RefUse> {
        crate::refs::refs_in_material(&self.to_value())
    }
}

/// What kind of file a reference of a material must name, by where the
/// reference is written (its JSON pointer): the `file` of a `Material`
/// component in a scene or prefab a material file, a texture field of a
/// material file a PNG. `None` when the reference fits, or is not one of a
/// material's.
///
/// The existence of the file is checked separately; this is about a file
/// that exists but cannot be what the field needs.
pub(crate) fn kind_problem(pointer: &str, reference: &str) -> Option<Diagnostic> {
    let (parent, field) = pointer.rsplit_once('/')?;
    if reference.is_empty() {
        return None;
    }
    let path = crate::refs::path_and_suffix(reference).0;
    let of_component = parent.ends_with("/components/Material");
    let of_file = parent == "/material";
    if of_component && field == "file" && !is_material(path) {
        return Some(Diagnostic::error(
            "not_a_material_file",
            format!(
                "'{path}' is not a material file: Material.file needs a file whose name ends in {MATERIAL_SUFFIX}."
            ),
            "Name a material file such as \"assets/materials/brick.mat.json\" (asset_create makes one). A picture goes into a material file's base_color_texture, not here.",
        ));
    }
    if of_file && field.ends_with("_texture") && !path.to_ascii_lowercase().ends_with(".png") {
        return Some(Diagnostic::warning(
            "texture_not_png",
            format!("'{path}' is not a PNG file: the texture '{field}' will not be drawn."),
            "Textures outside models are read from PNG files only. Convert the picture to PNG (8-bit) and name that file.",
        ));
    }
    None
}

/// Checks a material file's text as part of a project: everything
/// [`parse_material`] checks, that the textures it names exist in `index`
/// and are PNG files, and that the text is canonical (a warning).
///
/// Returns the file, when it has no errors, and every problem found.
pub fn check_material(
    source: &str,
    file: &str,
    index: &AssetIndex,
) -> (Option<MaterialDocument>, Vec<Diagnostic>) {
    let parsed = match parse_material(source, file) {
        Ok(parsed) => parsed,
        Err(problem) => return (None, vec![problem]),
    };
    let mut diagnostics = check_material_refs(source, file, index);
    diagnostics.extend(check_uv(&parsed, source, file));
    if parsed.to_canonical().ok().as_deref() != Some(source) {
        let warning = Diagnostic::warning(
            "not_canonical",
            "This file is not in canonical form.",
            "Run `wen fmt` to rewrite it. Saving through the engine does this automatically.",
        )
        .at_line(1, Some(1));
        diagnostics.push(if file.is_empty() {
            warning
        } else {
            warning.at_file(file)
        });
    }
    let failed = diagnostics.iter().any(Diagnostic::is_error);
    ((!failed).then_some(parsed), diagnostics)
}

/// The largest `uv_scale` or `uv_offset` the renderer uses as written; a
/// larger one is clamped to it.
pub const MAX_UV: f32 = 4096.0;

/// Warnings for `uv_scale` and `uv_offset` numbers the renderer cannot use
/// as written (`material_uv_unusable`): a scale of 0, which it draws as 1,
/// and numbers beyond [`MAX_UV`], which it clamps.
fn check_uv(parsed: &MaterialDocument, source: &str, file: &str) -> Vec<Diagnostic> {
    let (Ok((material, _)), Ok(root)) = (parsed.decode(), json::parse(source, file)) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for (field, values) in [
        ("uv_scale", material.uv_scale),
        ("uv_offset", material.uv_offset),
    ] {
        for (axis, value) in values.into_iter().enumerate() {
            let what = if field == "uv_scale" && value == 0.0 {
                "is 0, which would squeeze the whole picture into one point; it is drawn as 1"
            } else if value.abs() > MAX_UV {
                "is beyond 4096 either way; it is drawn as 4096"
            } else {
                continue;
            };
            diagnostics.push(locate(
                &root,
                file,
                Diagnostic::warning(
                    "material_uv_unusable",
                    format!("{field}[{axis}] {what}."),
                    "Repeats are counted in pictures across a face: [1, 1] shows the picture once, [4, 4] tiles it 4 by 4, a negative number mirrors it. Use a number other than 0, between -4096 and 4096.",
                )
                .at_path(format!("/material/{field}/{axis}")),
            ));
        }
    }
    diagnostics
}

/// Checks every texture reference of a material file's text against the
/// project's assets: one diagnostic for each reference whose file does not
/// exist (`asset_not_found`) or is not a PNG (`texture_not_png`), with the
/// line and column of the string. A file that does not parse gives nothing
/// here ([`parse_material`] reports it).
pub fn check_material_refs(source: &str, file: &str, index: &AssetIndex) -> Vec<Diagnostic> {
    let Ok(root) = json::parse(source, file) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for used in crate::refs::refs_in_material(&root.to_value()) {
        let problems = [
            index.check(&used.reference).err(),
            kind_problem(&used.pointer, &used.reference),
        ];
        // A file that does not exist is one problem, not two.
        if let Some(problem) = problems.into_iter().flatten().next() {
            diagnostics.push(locate(&root, file, problem.at_path(used.pointer.clone())));
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_is_the_material_file_type() {
        let schema = material_schema();
        assert_eq!(schema.name, "MaterialFile");
        assert!(schema.contains_text(), "it has texture references");
    }

    #[test]
    fn only_a_material_s_own_references_have_a_kind() {
        assert!(kind_problem("/entities/a/components/Model/asset", "a.glb").is_none());
        assert!(kind_problem("/entities/a/components/Material/file", "m.mat.json").is_none());
        assert_eq!(
            kind_problem("/entities/a/components/Material/file", "m.png")
                .unwrap()
                .code,
            "not_a_material_file"
        );
        assert_eq!(
            kind_problem("/material/normal_texture", "n.JPG")
                .unwrap()
                .code,
            "texture_not_png"
        );
        assert!(kind_problem("/material/normal_texture", "n.PNG").is_none());
        assert!(kind_problem("/material/normal_texture", "").is_none());
        // A component that only happens to have such a field is not judged.
        assert!(kind_problem("/entities/a/components/Decal/normal_texture", "n.jpg").is_none());
    }
}
