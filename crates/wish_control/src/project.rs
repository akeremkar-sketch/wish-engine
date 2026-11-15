//! The project file, `wish.json`: what a folder needs to be a project.
//!
//! ```json
//! {
//!   "engine": "0.1.0",
//!   "format": 1,
//!   "name": "My game",
//!   "scene": "scenes/main.scene.json",
//!   "types": { "example": "stress" }
//! }
//! ```
//!
//! `types.example` names the built-in example whose component types the
//! project's scenes use, until game modules bring a project's own types
//! (M5). Like every engine file it is canonical JSON.

use std::path::Path;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Result};

/// The project file's name.
pub const PROJECT_FILE: &str = "wish.json";
/// The format version this engine writes and reads.
pub const PROJECT_FORMAT: u64 = 1;

const KEYS: &[&str] = &["engine", "format", "name", "scene", "types"];
const TYPE_KEYS: &[&str] = &["example"];

/// What `wish.json` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectConfig {
    /// The project's name, for people.
    pub name: String,
    /// The engine version that wrote the file.
    pub engine: String,
    /// The scene tools use when none is named (project-relative).
    pub scene: String,
    /// The built-in example whose types the scenes use, if any.
    pub example: Option<String>,
}

impl ProjectConfig {
    /// A new project's configuration.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            engine: env!("CARGO_PKG_VERSION").to_owned(),
            scene: "scenes/main.scene.json".to_owned(),
            example: None,
        }
    }

    /// The file's JSON form.
    pub fn to_value(&self) -> Value {
        let mut types = Map::new();
        if let Some(example) = &self.example {
            types.insert("example".to_owned(), Value::String(example.clone()));
        }
        let mut object = Map::new();
        object.insert("engine".to_owned(), Value::String(self.engine.clone()));
        object.insert("format".to_owned(), Value::from(PROJECT_FORMAT));
        object.insert("name".to_owned(), Value::String(self.name.clone()));
        object.insert("scene".to_owned(), Value::String(self.scene.clone()));
        object.insert("types".to_owned(), Value::Object(types));
        Value::Object(object)
    }

    /// The file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        wish_scene::json::to_canonical(&self.to_value())
    }
}

fn problem(
    node: &wish_scene::json::Node,
    pointer: &str,
    message: String,
    hint: &str,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::error("invalid_project_file", message, hint)
        .at_file(PROJECT_FILE)
        .at_path(pointer);
    if let Some(found) = node.locate(pointer) {
        let span = found.key.unwrap_or(found.value);
        diagnostic = diagnostic.at_line(span.line, Some(span.column));
    }
    diagnostic
}

fn unknown_keys(
    node: &wish_scene::json::Node,
    object: &Map<String, Value>,
    prefix: &str,
    keys: &[&str],
) -> Result<()> {
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(problem(
                node,
                &format!("{prefix}/{key}"),
                format!("'{PROJECT_FILE}' has no setting '{key}'."),
                &wish_core::suggest::unknown_name_hint(key, keys),
            )
            .with_candidates(keys.iter().copied()));
        }
    }
    Ok(())
}

fn text(
    node: &wish_scene::json::Node,
    object: &Map<String, Value>,
    key: &str,
    hint: &str,
) -> Result<String> {
    match object.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(_) => Err(problem(
            node,
            &format!("/{key}"),
            format!("'{key}' in '{PROJECT_FILE}' must be a non-empty string."),
            hint,
        )),
        None => Err(problem(
            node,
            "",
            format!("'{PROJECT_FILE}' has no '{key}'."),
            hint,
        )),
    }
}

/// Reads `source` (the text of `wish.json`).
pub fn parse_project(source: &str) -> Result<ProjectConfig> {
    let node = wish_scene::json::parse(source, PROJECT_FILE)?;
    let value = node.to_value();
    let Some(object) = value.as_object() else {
        return Err(problem(
            &node,
            "",
            format!("'{PROJECT_FILE}' must be a JSON object."),
            "Start it with { \"format\": 1, \"name\": \"...\", \"scene\": \"scenes/main.scene.json\" }, or run `wen new` to write one.",
        ));
    };
    unknown_keys(&node, object, "", KEYS)?;
    match object.get("format").and_then(Value::as_u64) {
        Some(PROJECT_FORMAT) => {}
        Some(other) => {
            return Err(problem(
                &node,
                "/format",
                format!(
                    "'{PROJECT_FILE}' has format {other}; this engine reads format {PROJECT_FORMAT}."
                ),
                "Open the project with the engine version that wrote it, or run `wen migrate`.",
            ));
        }
        None => {
            return Err(problem(
                &node,
                "",
                format!("'{PROJECT_FILE}' needs \"format\": {PROJECT_FORMAT}."),
                "Add \"format\": 1.",
            ));
        }
    }
    let name = text(&node, object, "name", "Add \"name\": \"My game\".")?;
    let scene = text(
        &node,
        object,
        "scene",
        "Add \"scene\": \"scenes/main.scene.json\": the scene tools use when none is named.",
    )?;
    if !scene.ends_with(".scene.json") {
        return Err(problem(
            &node,
            "/scene",
            format!("'scene' must name a .scene.json file, but is '{scene}'."),
            "Write a project-relative path such as \"scenes/main.scene.json\".",
        ));
    }
    let engine = object
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_owned();
    let mut example = None;
    match object.get("types") {
        None => {}
        Some(Value::Object(types)) => {
            unknown_keys(&node, types, "/types", TYPE_KEYS)?;
            match types.get("example") {
                None => {}
                Some(Value::String(name)) if wish_examples::NAMES.contains(&name.as_str()) => {
                    example = Some(name.clone());
                }
                Some(other) => {
                    let given = other.as_str().unwrap_or("");
                    return Err(problem(
                        &node,
                        "/types/example",
                        format!("'types.example' must name a built-in example, but is {other}."),
                        &wish_core::suggest::unknown_name_hint(given, wish_examples::NAMES),
                    )
                    .with_candidates(wish_examples::NAMES.iter().copied()));
                }
            }
        }
        Some(_) => {
            return Err(problem(
                &node,
                "/types",
                "'types' must be an object.".to_owned(),
                "Write \"types\": { \"example\": \"stress\" }, or {} for the engine's types only.",
            ));
        }
    }
    Ok(ProjectConfig {
        name,
        engine,
        scene,
        example,
    })
}

/// Reads `root/wish.json`; `Ok(None)` if the folder has none.
pub fn read_project(root: &Path) -> Result<Option<ProjectConfig>> {
    let path = root.join(PROJECT_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{PROJECT_FILE}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
        .at_file(PROJECT_FILE)
    })?;
    parse_project(&source).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_file_reads_back_the_same() {
        let mut config = ProjectConfig::new("Test");
        config.example = Some("stress".to_owned());
        let text = config.to_canonical().unwrap();
        assert_eq!(parse_project(&text).unwrap(), config);
        assert!(wish_scene::json::is_canonical(&text));
    }

    #[test]
    fn mistakes_point_at_the_setting_with_a_hint() {
        let cases = [
            ("[]", ""),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"sceen\": 1 }",
                "/sceen",
            ),
            (
                "{ \"format\": 2, \"name\": \"x\", \"scene\": \"a.scene.json\" }",
                "/format",
            ),
            (
                "{ \"format\": 1, \"name\": \"\", \"scene\": \"a.scene.json\" }",
                "/name",
            ),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.json\" }",
                "/scene",
            ),
            (
                "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"types\": { \"example\": \"bonce\" } }",
                "/types/example",
            ),
        ];
        for (source, pointer) in cases {
            let problem = parse_project(source).unwrap_err();
            assert_eq!(problem.code, "invalid_project_file", "{source}");
            assert_eq!(problem.location.path.as_deref(), Some(pointer), "{source}");
            assert!(!problem.hint.is_empty());
        }
        let problem = parse_project(
            "{ \"format\": 1, \"name\": \"x\", \"scene\": \"a.scene.json\", \"types\": { \"example\": \"bonce\" } }",
        )
        .unwrap_err();
        assert_eq!(problem.hint, "Did you mean 'bounce'?");
        assert_eq!(problem.location.line, Some(1));
        assert_eq!(parse_project("{").unwrap_err().code, "json_syntax");
    }
}
