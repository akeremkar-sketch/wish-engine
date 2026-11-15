//! The single diagnostic format used across the engine.
//!
//! Compiler errors, scene validation errors, runtime warnings and MCP errors
//! all use [`Diagnostic`]. Every diagnostic carries a stable machine-readable
//! `code` and a `hint` that says how to fix the problem, so that an AI agent
//! (or a person) can act on it without guessing.

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The result type used by engine APIs: failures are [`Diagnostic`]s.
pub type Result<T, E = Diagnostic> = core::result::Result<T, E>;

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational; nothing is wrong.
    Info,
    /// Something looks wrong, but the operation went ahead.
    Warning,
    /// The operation failed.
    Error,
}

impl Severity {
    /// The lowercase name, as it appears in JSON output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Where a diagnostic points. Every part is optional.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Project-relative file path, with forward slashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// JSON pointer into a document, e.g. `/entities/player/components/Transform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based line number in a text file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column number in a text file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// 0-based index of the operation in a batch that caused the problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<u32>,
}

impl Location {
    /// Whether no part of the location is set.
    pub fn is_empty(&self) -> bool {
        self.file.is_none()
            && self.path.is_none()
            && self.line.is_none()
            && self.column.is_none()
            && self.op.is_none()
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        match (&self.file, self.line, self.column) {
            (Some(file), Some(line), Some(column)) => parts.push(format!("{file}:{line}:{column}")),
            (Some(file), Some(line), None) => parts.push(format!("{file}:{line}")),
            (Some(file), None, _) => parts.push(file.clone()),
            (None, Some(line), Some(column)) => parts.push(format!("line {line}, column {column}")),
            (None, Some(line), None) => parts.push(format!("line {line}")),
            (None, None, _) => {}
        }
        if let Some(path) = &self.path {
            parts.push(path.clone());
        }
        if let Some(op) = self.op {
            parts.push(format!("(operation {op})"));
        }
        f.write_str(&parts.join(" "))
    }
}

/// An error, warning or note, in the one format used everywhere in the engine.
///
/// # JSON form
///
/// ```json
/// {
///   "severity": "error",
///   "code": "unknown_field",
///   "message": "Component 'Transform' has no field 'position'.",
///   "location": { "file": "scenes/main.scene.json", "path": "/entities/player/components/Transform/position" },
///   "hint": "Did you mean 'translation'?",
///   "candidates": ["translation", "rotation", "scale"],
///   "docs": "wish://docs/diagnostics/unknown_field"
/// }
/// ```
///
/// `location` and `candidates` are omitted when empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// How serious the diagnostic is.
    pub severity: Severity,
    /// Stable, documented, machine-readable identifier in `snake_case`.
    pub code: Cow<'static, str>,
    /// What is wrong, in one or two sentences.
    pub message: String,
    /// Where the problem is.
    #[serde(default, skip_serializing_if = "Location::is_empty")]
    pub location: Location,
    /// How to fix it. Never empty: a diagnostic without a hint is unfinished.
    pub hint: String,
    /// Valid alternatives for the value that was rejected, when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// Address of the documentation page for this diagnostic code.
    pub docs: String,
}

impl Diagnostic {
    /// Creates a diagnostic. The documentation address is derived from `code`.
    pub fn new(
        severity: Severity,
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        let code = code.into();
        let docs = format!("wish://docs/diagnostics/{code}");
        Self {
            severity,
            code,
            message: message.into(),
            location: Location::default(),
            hint: hint.into(),
            candidates: Vec::new(),
            docs,
        }
    }

    /// Creates an error: the operation failed.
    pub fn error(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Error, code, message, hint)
    }

    /// Creates a warning: something looks wrong, but the operation went ahead.
    pub fn warning(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Warning, code, message, hint)
    }

    /// Creates an informational note.
    pub fn info(
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Info, code, message, hint)
    }

    /// Sets the project-relative file the diagnostic points at.
    #[must_use]
    pub fn at_file(mut self, file: impl Into<String>) -> Self {
        self.location.file = Some(file.into());
        self
    }

    /// Sets the JSON pointer the diagnostic points at.
    #[must_use]
    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.location.path = Some(path.into());
        self
    }

    /// Sets the 1-based line and column the diagnostic points at.
    #[must_use]
    pub fn at_line(mut self, line: u32, column: Option<u32>) -> Self {
        self.location.line = Some(line);
        self.location.column = column;
        self
    }

    /// Sets the 0-based index of the batch operation that caused the problem.
    #[must_use]
    pub fn at_op(mut self, op: u32) -> Self {
        self.location.op = Some(op);
        self
    }

    /// Sets the list of valid alternatives.
    #[must_use]
    pub fn with_candidates<I, S>(mut self, candidates: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.candidates = candidates.into_iter().map(Into::into).collect();
        self
    }

    /// Overrides the documentation address.
    #[must_use]
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = docs.into();
        self
    }

    /// Whether the severity is [`Severity::Error`].
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}]: {}",
            self.severity.as_str(),
            self.code,
            self.message
        )?;
        if !self.location.is_empty() {
            write!(f, "\n  at {}", self.location)?;
        }
        write!(f, "\n  hint: {}", self.hint)
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample() -> Diagnostic {
        Diagnostic::error(
            "unknown_field",
            "Component 'Transform' has no field 'position'.",
            "Did you mean 'translation'?",
        )
        .at_file("scenes/main.scene.json")
        .at_path("/entities/player/components/Transform/position")
        .with_candidates(["translation", "rotation", "scale"])
    }

    #[test]
    fn json_form_matches_the_documented_shape() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(
            value,
            json!({
                "severity": "error",
                "code": "unknown_field",
                "message": "Component 'Transform' has no field 'position'.",
                "location": {
                    "file": "scenes/main.scene.json",
                    "path": "/entities/player/components/Transform/position"
                },
                "hint": "Did you mean 'translation'?",
                "candidates": ["translation", "rotation", "scale"],
                "docs": "wish://docs/diagnostics/unknown_field"
            })
        );
    }

    #[test]
    fn empty_location_and_candidates_are_omitted() {
        let diagnostic = Diagnostic::warning(
            "no_light",
            "The scene has no light.",
            "Add a DirectionalLight.",
        );
        let value = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(
            value,
            json!({
                "severity": "warning",
                "code": "no_light",
                "message": "The scene has no light.",
                "hint": "Add a DirectionalLight.",
                "docs": "wish://docs/diagnostics/no_light"
            })
        );
    }

    #[test]
    fn json_round_trips() {
        let original = sample().at_line(14, Some(9)).at_op(2);
        let text = serde_json::to_string(&original).unwrap();
        let parsed: Diagnostic = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn display_shows_code_location_and_hint() {
        assert_eq!(
            sample().to_string(),
            "error[unknown_field]: Component 'Transform' has no field 'position'.\n  \
             at scenes/main.scene.json /entities/player/components/Transform/position\n  \
             hint: Did you mean 'translation'?"
        );
        let located =
            Diagnostic::error("syntax", "Unexpected token.", "Remove the trailing comma.")
                .at_file("scripts/src/lib.rs")
                .at_line(3, Some(7));
        assert!(located.to_string().contains("at scripts/src/lib.rs:3:7"));
        let batch = Diagnostic::error("bad_op", "Operation failed.", "Fix operation 2.").at_op(2);
        assert!(batch.to_string().contains("at (operation 2)"));
    }

    #[test]
    fn severity_orders_from_info_to_error() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(sample().is_error());
        assert!(!Diagnostic::info("note", "Fine.", "Nothing to do.").is_error());
    }

    #[test]
    fn docs_default_can_be_overridden() {
        let diagnostic = sample().with_docs("wish://docs/components/Transform");
        assert_eq!(diagnostic.docs, "wish://docs/components/Transform");
    }
}
