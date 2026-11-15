//! Checking WGSL shaders before the GPU sees them.
//!
//! Every shader is parsed and validated with naga first, so that a mistake
//! comes back as a [`Diagnostic`] with the file, line and column of the
//! offending code and the line itself, the same as a broken scene file,
//! instead of as a GPU error with a driver-specific message.

use wish_core::{Diagnostic, Result};

/// A shader that parsed and validated.
#[derive(Debug)]
pub struct CheckedShader {
    /// The parsed module.
    pub module: naga::Module,
}

/// The 1-based character column of byte offset `offset` in `source`, and
/// the text of its line.
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

fn located(
    code: &'static str,
    message: String,
    hint: String,
    source: &str,
    file: &str,
    offset: Option<usize>,
) -> Diagnostic {
    let mut message = message;
    let mut diagnostic_line = None;
    if let Some(offset) = offset {
        let (line, column, text) = char_position(source, offset);
        message = format!("{message}\n  {line} | {}", text.trim());
        diagnostic_line = Some((line, column));
    }
    let mut diagnostic = Diagnostic::error(code, message, hint);
    if let Some((line, column)) = diagnostic_line {
        diagnostic = diagnostic.at_line(line, Some(column));
    }
    if !file.is_empty() {
        diagnostic = diagnostic.at_file(file);
    }
    diagnostic
}

/// Parses and validates a WGSL shader. `file` is the path shown in
/// diagnostics.
pub fn check_wgsl(source: &str, file: &str) -> Result<CheckedShader> {
    let module = naga::front::wgsl::parse_str(source).map_err(|error| {
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
            source,
            file,
            primary.and_then(|(span, _)| span.to_range()).map(|range| range.start),
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
            source,
            file,
            first.and_then(|(span, _)| span.to_range()).map(|range| range.start),
        )
    })?;
    Ok(CheckedShader { module })
}
