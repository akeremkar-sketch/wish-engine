//! Shaders: putting WGSL modules together, and checking them before the GPU
//! sees them.
//!
//! A shader module may start with lines `#import name`, which pull in another
//! module of the same [`ShaderLibrary`]. Each module is included once, after
//! the modules it imports, and a line map remembers where every line of the
//! result came from. (WGSL has no `#` syntax, so the directive cannot clash
//! with shader code. ADR-013: WESL can take this interface over later.)
//!
//! Every shader is parsed and validated with naga, and a mistake comes back
//! as a [`Diagnostic`] with the file, line and column **of the module it is
//! in**, plus the offending line, the same as a broken scene file, instead of
//! as a GPU error with a driver-specific message.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Result};

/// A shader that parsed and validated.
#[derive(Debug)]
pub struct CheckedShader {
    /// The source that was checked (for a composed shader, all its modules
    /// together).
    pub source: String,
    /// The parsed module.
    pub module: naga::Module,
}

/// Where one line of a composed shader came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLine {
    /// The module's file name.
    pub file: String,
    /// The 1-based line in that file.
    pub line: u32,
}

/// Several modules put together into one WGSL source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposedShader {
    /// The WGSL source.
    pub source: String,
    /// For each line of `source`, where it came from.
    pub lines: Vec<SourceLine>,
}

/// A set of named WGSL modules that can import each other.
#[derive(Clone, Debug, Default)]
pub struct ShaderLibrary {
    modules: BTreeMap<String, (String, String)>,
}

/// The import directive.
const IMPORT: &str = "#import";

impl ShaderLibrary {
    /// An empty library.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a module. `file` is the path shown in diagnostics.
    pub fn add(&mut self, name: &str, file: &str, source: &str) {
        self.modules
            .insert(name.to_owned(), (file.to_owned(), source.to_owned()));
    }

    /// The file name a module was added with (the path shown in
    /// diagnostics), if the library has a module of that name.
    pub fn file_of(&self, name: &str) -> Option<&str> {
        self.modules.get(name).map(|(file, _)| file.as_str())
    }

    /// Puts `entry` and everything it imports together, imports first.
    pub fn compose(&self, entry: &str) -> Result<ComposedShader> {
        let mut order = Vec::new();
        let mut stack = Vec::new();
        self.visit(entry, None, &mut order, &mut stack)?;
        let mut source = String::new();
        let mut lines = Vec::new();
        for name in order {
            let Some((file, text)) = self.modules.get(&name) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.trim_start().starts_with(IMPORT) {
                    continue;
                }
                source.push_str(line);
                source.push('\n');
                lines.push(SourceLine {
                    file: file.clone(),
                    line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                });
            }
        }
        Ok(ComposedShader { source, lines })
    }

    /// Composes `entry` and checks the result; mistakes point into the module
    /// that has them.
    pub fn check(&self, entry: &str) -> Result<CheckedShader> {
        let composed = self.compose(entry)?;
        check_mapped(composed.source.clone(), &|output_line| {
            composed
                .lines
                .get(output_line.saturating_sub(1) as usize)
                .map(|source| (source.file.clone(), source.line))
        })
    }

    /// Depth-first: imports before the module, each module once.
    fn visit(
        &self,
        name: &str,
        from: Option<(&str, u32, u32)>,
        order: &mut Vec<String>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        if order.iter().any(|done| done == name) {
            return Ok(());
        }
        if let Some(position) = stack.iter().position(|open| open == name) {
            let mut cycle: Vec<&str> = stack[position..].iter().map(String::as_str).collect();
            cycle.push(name);
            let mut diagnostic = Diagnostic::error(
                "shader_import_cycle",
                format!("Shader modules import each other: {}.", cycle.join(" -> ")),
                "Move what both modules need into a third module that neither of them imports back.",
            );
            if let Some((file, line, column)) = from {
                diagnostic = diagnostic.at_file(file).at_line(line, Some(column));
            }
            return Err(diagnostic);
        }
        let Some((file, text)) = self.modules.get(name) else {
            let names: Vec<&str> = self.modules.keys().map(String::as_str).collect();
            let mut diagnostic = Diagnostic::error(
                "shader_import_unknown",
                format!("There is no shader module '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &names),
            );
            if let Some((file, line, column)) = from {
                diagnostic = diagnostic.at_file(file).at_line(line, Some(column));
            }
            return Err(diagnostic);
        };
        stack.push(name.to_owned());
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix(IMPORT) else {
                continue;
            };
            let imported = rest.trim();
            let column = line.len() - trimmed.len()
                + IMPORT.len()
                + (rest.len() - rest.trim_start().len())
                + 1;
            let at = (
                file.as_str(),
                u32::try_from(index + 1).unwrap_or(u32::MAX),
                u32::try_from(column).unwrap_or(u32::MAX),
            );
            if imported.is_empty() || imported.contains(char::is_whitespace) {
                return Err(Diagnostic::error(
                    "shader_import_syntax",
                    format!("'{}' is not a valid import line.", line.trim()),
                    "Write one module name after #import, for example: #import brdf",
                )
                .at_file(at.0)
                .at_line(at.1, Some(at.2)));
            }
            self.visit(imported, Some(at), order, stack)?;
        }
        stack.pop();
        order.push(name.to_owned());
        Ok(())
    }
}

/// The 1-based line and character column of byte offset `offset` in
/// `source`, and the text of its line.
fn char_position(source: &str, offset: usize) -> (u32, u32, &str) {
    let offset = offset.min(source.len());
    let mut at = offset;
    while !source.is_char_boundary(at) {
        at -= 1;
    }
    let line_start = source[..at].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[at..]
        .find('\n')
        .map_or(source.len(), |index| at + index);
    let line = source[..at].matches('\n').count() + 1;
    let column = source[line_start..at].chars().count() + 1;
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
        source[line_start..line_end].trim_end_matches('\r'),
    )
}

/// Maps a line of the checked source to a file and a line in it.
type LineMap<'a> = dyn Fn(u32) -> Option<(String, u32)> + 'a;

fn located(
    code: &'static str,
    message: String,
    hint: String,
    source: &str,
    map: &LineMap<'_>,
    offset: Option<usize>,
) -> Diagnostic {
    let Some(offset) = offset else {
        return Diagnostic::error(code, message, hint);
    };
    let (line, column, text) = char_position(source, offset);
    let (file, line) = map(line).unwrap_or_default();
    let mut diagnostic =
        Diagnostic::error(code, format!("{message}\n  {line} | {}", text.trim()), hint)
            .at_line(line, Some(column));
    if !file.is_empty() {
        diagnostic = diagnostic.at_file(file);
    }
    diagnostic
}

/// Parses and validates one WGSL source. `file` is the path shown in
/// diagnostics.
pub fn check_wgsl(source: &str, file: &str) -> Result<CheckedShader> {
    check_mapped(source.to_owned(), &|line| Some((file.to_owned(), line)))
}

fn check_mapped(source: String, map: &LineMap<'_>) -> Result<CheckedShader> {
    let module = naga::front::wgsl::parse_str(&source).map_err(|error| {
        let primary = error.labels().next();
        let detail = primary
            .map(|(_, label)| label)
            .filter(|label| !label.is_empty())
            .map(|label| format!(" ({label})"))
            .unwrap_or_default();
        let notes: Vec<&str> = error.notes().collect();
        let hint = if notes.is_empty() {
            "Fix the WGSL at this position. The WGSL specification lists the exact syntax: https://www.w3.org/TR/WGSL/".to_owned()
        } else {
            notes.join(" ")
        };
        located(
            "shader_syntax",
            format!("{}{detail}", error.message()),
            hint,
            &source,
            map,
            primary
                .and_then(|(span, _)| span.to_range())
                .map(|range| range.start),
        )
    })?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    validator.validate(&module).map_err(|error| {
        // naga lists the enclosing items too (the function, then the
        // statement); the narrowest span is where the mistake is.
        let first = error
            .spans()
            .filter_map(|span_context| {
                let range = span_context.0.to_range()?;
                Some((range.len(), span_context))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, span_context)| span_context);
        let detail = first
            .map(|(_, context)| context.as_str())
            .filter(|context| !context.is_empty())
            .map(|context| format!(" ({context})"))
            .unwrap_or_default();
        let mut chain = vec![error.as_inner().to_string()];
        let mut source_error = std::error::Error::source(error.as_inner());
        while let Some(inner) = source_error {
            chain.push(inner.to_string());
            source_error = inner.source();
        }
        located(
            "shader_validation",
            format!("{}{detail}", chain.join(": ")),
            "The shader is valid WGSL syntax but breaks a typing or resource rule; the message names the rule. Check the types on this line.".to_owned(),
            &source,
            map,
            first
                .and_then(|(span, _)| span.to_range())
                .map(|range| range.start),
        )
    })?;
    Ok(CheckedShader { source, module })
}
