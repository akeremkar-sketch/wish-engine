//! The `<asset>.meta.json` file next to an asset.
//!
//! ```json
//! {
//!   "format": 1,
//!   "import": { "generate_mipmaps": true }
//! }
//! ```
//!
//! `import` holds the asset's import settings. What settings exist depends on
//! the asset type and is defined by the importers (M3); this crate keeps the
//! object as it is. An asset without a meta file uses default settings.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_scene::json::{self, Json, Node};

/// The meta file format this build reads and writes.
pub const META_FORMAT: u32 = 1;

/// An asset's meta file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssetMeta {
    /// Import settings, by name.
    pub import: Map<String, Value>,
}

/// The meta file's project-relative path for an asset.
pub fn meta_path(asset: &str) -> String {
    format!("{asset}.meta.json")
}

fn problem(
    node: &Node,
    file: &str,
    code: &'static str,
    message: String,
    hint: String,
) -> Diagnostic {
    let diagnostic =
        Diagnostic::error(code, message, hint).at_line(node.span.line, Some(node.span.column));
    if file.is_empty() {
        diagnostic
    } else {
        diagnostic.at_file(file)
    }
}

/// Reads a meta file's text.
pub fn parse_meta(source: &str, file: &str) -> Result<AssetMeta> {
    let root = json::parse(source, file)?;
    let Json::Object(members) = &root.value else {
        return Err(problem(
            &root,
            file,
            "type_mismatch",
            "A meta file must be an object.".to_owned(),
            "Write { \"format\": 1 }.".to_owned(),
        ));
    };
    let mut format_seen = false;
    let mut meta = AssetMeta::default();
    for member in members {
        match member.key.as_str() {
            "format" => {
                format_seen = true;
                let supported = matches!(&member.value.value, Json::Number(n) if n.as_u64() == Some(u64::from(META_FORMAT)));
                if !supported {
                    return Err(problem(
                        &member.value,
                        file,
                        "unsupported_format",
                        format!("This meta file is not format {META_FORMAT}."),
                        "Run `wen migrate` to upgrade an older file, or update the engine for a newer one."
                            .to_owned(),
                    ));
                }
            }
            "import" => match member.value.to_value() {
                Value::Object(map) => meta.import = map,
                _ => {
                    return Err(problem(
                        &member.value,
                        file,
                        "type_mismatch",
                        "\"import\" must be an object.".to_owned(),
                        "Write the import settings as { \"name\": value, ... }.".to_owned(),
                    ));
                }
            },
            other => {
                let mut diagnostic = Diagnostic::error(
                    "unknown_key",
                    format!("A meta file has no key '{other}'."),
                    unknown_name_hint(other, &["format", "import"]),
                )
                .at_line(member.key_span.line, Some(member.key_span.column))
                .with_candidates(["format", "import"]);
                if !file.is_empty() {
                    diagnostic = diagnostic.at_file(file);
                }
                return Err(diagnostic);
            }
        }
    }
    if !format_seen {
        return Err(problem(
            &root,
            file,
            "missing_format",
            "The meta file has no \"format\" key.".to_owned(),
            format!("Add \"format\": {META_FORMAT} at the top level."),
        ));
    }
    Ok(meta)
}

impl AssetMeta {
    /// The meta file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        let mut object = Map::new();
        object.insert("format".to_owned(), Value::from(META_FORMAT));
        if !self.import.is_empty() {
            object.insert("import".to_owned(), Value::Object(self.import.clone()));
        }
        json::to_canonical(&Value::Object(object))
    }
}

/// Reads the meta file of `asset` (a project-relative path) below `root`.
/// `Ok(None)` when the asset has none.
pub fn read_meta(root: &Path, asset: &str) -> Result<Option<AssetMeta>> {
    let relative = meta_path(asset);
    let path = root.join(&relative);
    if !path.is_file() {
        return Ok(None);
    }
    let source = fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{relative}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
    })?;
    parse_meta(&source, &relative).map(Some)
}

/// Writes the meta file of `asset` in canonical form.
pub fn write_meta(root: &Path, asset: &str, meta: &AssetMeta) -> Result<()> {
    let relative = meta_path(asset);
    fs::write(root.join(&relative), meta.to_canonical()?).map_err(|error| {
        Diagnostic::error(
            "file_unwritable",
            format!("Could not write '{relative}': {error}."),
            "Check that the folder exists and can be written to.",
        )
    })
}
