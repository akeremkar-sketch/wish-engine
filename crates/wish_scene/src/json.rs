//! Strict JSON with source positions, and the canonical writer.
//!
//! # Why not `serde_json`'s parser
//!
//! Scene files are edited by hand and by AI agents, and every mistake must be
//! reported at the place it was made. `serde_json` reports syntax errors with
//! a line and column, but a value that parses fine and is rejected later (an
//! unknown field, a number out of range) has no position any more. [`parse`]
//! therefore keeps the position of every value and object key in a [`Node`]
//! tree, and [`Node::locate`] turns a JSON pointer (which is what schema
//! diagnostics carry) back into a line and column.
//!
//! It is also stricter: duplicate object keys are an error (`serde_json`
//! silently keeps the last one), and there is no leniency for comments,
//! trailing commas or single quotes.
//!
//! # Canonical form
//!
//! [`to_canonical`] writes the one form every scene file is kept in:
//!
//! - UTF-8 without byte order mark, LF line endings, one final newline;
//! - two-space indentation;
//! - object keys sorted by their UTF-8 bytes;
//! - arrays of numbers on one line (`[0, 1, 0]`), other arrays one item per
//!   line; empty arrays and objects as `[]` and `{}`;
//! - numbers in the shortest form that reads back as the same number;
//! - in strings only `"`, `\` and control characters are escaped.
//!
//! Writing the parsed form of a canonical file gives back exactly the same
//! bytes.

use std::collections::BTreeSet;

use serde_json::{Map, Number, Value};
use wish_core::{Diagnostic, Result};

/// How deeply arrays and objects may nest. Parsing and writing recurse, so
/// this keeps hostile input from exhausting the stack.
pub const MAX_DEPTH: usize = 128;

/// A position in a text file. Both numbers start at 1, and the column counts
/// characters, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    /// The line, from 1.
    pub line: u32,
    /// The column, from 1.
    pub column: u32,
}

/// A JSON value and where it starts in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Where the value starts.
    pub span: Span,
    /// The value.
    pub value: Json,
}

/// A JSON value whose children remember their positions.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, in the exact form [`serde_json`] reads it.
    Number(Number),
    /// A string, with escapes resolved.
    String(String),
    /// An array.
    Array(Vec<Node>),
    /// An object, with its members in the order they appear in the file.
    Object(Vec<Member>),
}

/// One `"key": value` pair of an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// The key, with escapes resolved.
    pub key: String,
    /// Where the key's opening quote is.
    pub key_span: Span,
    /// The value.
    pub value: Node,
}

/// Where a value was found: the position of its key (for object members) and
/// of the value itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Located {
    /// The key's position; `None` for the root and for array items.
    pub key: Option<Span>,
    /// The value's position.
    pub value: Span,
}

impl Node {
    /// The same value without positions.
    pub fn to_value(&self) -> Value {
        match &self.value {
            Json::Null => Value::Null,
            Json::Bool(flag) => Value::Bool(*flag),
            Json::Number(number) => Value::Number(number.clone()),
            Json::String(text) => Value::String(text.clone()),
            Json::Array(items) => Value::Array(items.iter().map(Node::to_value).collect()),
            Json::Object(members) => Value::Object(
                members
                    .iter()
                    .map(|member| (member.key.clone(), member.value.to_value()))
                    .collect::<Map<String, Value>>(),
            ),
        }
    }

    /// Finds the value a JSON pointer (RFC 6901, such as
    /// `/entities/player/components/Transform`) refers to, with its position.
    /// `None` if the pointer does not lead to a value.
    pub fn locate(&self, pointer: &str) -> Option<Located> {
        let mut node = self;
        let mut key = None;
        if !pointer.is_empty() {
            for segment in pointer.strip_prefix('/')?.split('/') {
                let segment = segment.replace("~1", "/").replace("~0", "~");
                key = None;
                match &node.value {
                    Json::Object(members) => {
                        let member = members.iter().find(|member| member.key == segment)?;
                        key = Some(member.key_span);
                        node = &member.value;
                    }
                    Json::Array(items) => {
                        node = items.get(segment.parse::<usize>().ok()?)?;
                    }
                    _ => return None,
                }
            }
        }
        Some(Located {
            key,
            value: node.span,
        })
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    source: &'a str,
    file: &'a str,
    offset: usize,
    line: u32,
    column: u32,
}

/// How a character is shown in a message.
fn describe(character: Option<char>) -> String {
    match character {
        None => "the end of the file".to_owned(),
        Some(character) if character.is_control() => {
            format!("the character U+{:04X}", character as u32)
        }
        Some(character) => format!("'{character}'"),
    }
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, file: &'a str) -> Self {
        // A byte order mark is invisible and not part of the first column.
        let offset = if source.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        Self {
            source,
            file,
            offset,
            line: 1,
            column: 1,
        }
    }

    fn rest(&self) -> &'a str {
        self.source.get(self.offset..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else {
            self.column = self.column.saturating_add(1);
        }
        Some(character)
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn error_at(
        &self,
        span: Span,
        code: &'static str,
        message: String,
        hint: impl Into<String>,
    ) -> Diagnostic {
        let diagnostic =
            Diagnostic::error(code, message, hint).at_line(span.line, Some(span.column));
        if self.file.is_empty() {
            diagnostic
        } else {
            diagnostic.at_file(self.file)
        }
    }

    fn error(&self, code: &'static str, message: String, hint: impl Into<String>) -> Diagnostic {
        self.error_at(self.span(), code, message, hint)
    }

    /// The error for finding something other than what the grammar needs here.
    fn unexpected(&self, expected: &str) -> Diagnostic {
        let found = self.peek();
        let hint = match found {
            None => "The file ends too early; a closing bracket, brace or quote is probably missing.".to_owned(),
            Some('/') => "JSON has no comments. Put explanations in a \"notes\" field instead.".to_owned(),
            Some('\'') => "Strings use double quotes: \"like this\".".to_owned(),
            Some('+' | '.') => "A JSON number cannot start with '+' or '.'; write 0.5, not .5.".to_owned(),
            Some('N' | 'I') => "JSON has no NaN or Infinity.".to_owned(),
            Some(letter) if letter.is_alphabetic() => {
                "Literals are written in lowercase: true, false, null. Any other word must be in double quotes.".to_owned()
            }
            Some(_) => format!("Check the punctuation here: JSON needs {expected}."),
        };
        self.error(
            "json_syntax",
            format!("Expected {expected}, found {}.", describe(found)),
            hint,
        )
    }

    fn parse_value(&mut self, depth: usize) -> Result<Node> {
        self.skip_whitespace();
        let span = self.span();
        let value = match self.peek() {
            Some('{') => self.parse_object(depth)?,
            Some('[') => self.parse_array(depth)?,
            Some('"') => Json::String(self.parse_string()?),
            Some('-' | '0'..='9') => self.parse_number()?,
            Some('t' | 'f' | 'n') => self.parse_literal()?,
            _ => return Err(self.unexpected("a value")),
        };
        Ok(Node { span, value })
    }

    fn parse_literal(&mut self) -> Result<Json> {
        for (word, value) in [
            ("true", Json::Bool(true)),
            ("false", Json::Bool(false)),
            ("null", Json::Null),
        ] {
            if self.rest().starts_with(word) {
                for _ in word.chars() {
                    self.bump();
                }
                return Ok(value);
            }
        }
        Err(self.unexpected("true, false or null"))
    }

    fn digits(&mut self) -> usize {
        let mut count = 0;
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
            count += 1;
        }
        count
    }

    fn parse_number(&mut self) -> Result<Json> {
        let start = self.offset;
        let span = self.span();
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.error(
                        "json_syntax",
                        "A number cannot have leading zeros.".to_owned(),
                        "Write 7, not 07.",
                    ));
                }
            }
            Some('1'..='9') => {
                self.digits();
            }
            _ => return Err(self.unexpected("a digit")),
        }
        if self.peek() == Some('.') {
            self.bump();
            if self.digits() == 0 {
                return Err(self.unexpected("a digit after the decimal point"));
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if self.digits() == 0 {
                return Err(self.unexpected("a digit in the exponent"));
            }
        }
        let text = self.source.get(start..self.offset).unwrap_or("");
        text.parse::<Number>().map(Json::Number).map_err(|_| {
            self.error_at(
                span,
                "json_number_out_of_range",
                format!("The number {text} is too large to represent."),
                "Use a number between about -1.8e308 and 1.8e308.",
            )
        })
    }

    fn parse_hex4(&mut self, escape: Span) -> Result<u32> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = self.peek().and_then(|character| character.to_digit(16));
            let Some(digit) = digit else {
                return Err(self.error_at(
                    escape,
                    "json_syntax",
                    "A \\u escape needs exactly four hexadecimal digits.".to_owned(),
                    "Write exactly four hexadecimal digits after \\u, like \\u00e9.",
                ));
            };
            self.bump();
            code = code * 16 + digit;
        }
        Ok(code)
    }

    fn parse_string(&mut self) -> Result<String> {
        let start = self.span();
        self.bump(); // the opening quote
        let mut text = String::new();
        loop {
            let at = self.span();
            let Some(character) = self.bump() else {
                return Err(self.error_at(
                    start,
                    "json_syntax",
                    "This string is never closed.".to_owned(),
                    "Add the closing double quote.",
                ));
            };
            match character {
                '"' => return Ok(text),
                '\\' => text.push(self.parse_escape(at)?),
                control if (control as u32) < 0x20 => {
                    return Err(self.error_at(
                        at,
                        "json_syntax",
                        format!(
                            "A string cannot contain {} directly.",
                            describe(Some(control))
                        ),
                        "Write line breaks and tabs inside strings as \\n and \\t.",
                    ));
                }
                other => text.push(other),
            }
        }
    }

    fn parse_escape(&mut self, backslash: Span) -> Result<char> {
        let Some(escape) = self.bump() else {
            return Err(self.unexpected("an escape character"));
        };
        Ok(match escape {
            '"' => '"',
            '\\' => '\\',
            '/' => '/',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => {
                let code = self.parse_hex4(backslash)?;
                match code {
                    0xD800..=0xDBFF => {
                        let paired = self.rest().starts_with("\\u");
                        if paired {
                            self.bump();
                            self.bump();
                        }
                        let low = if paired {
                            self.parse_hex4(backslash)?
                        } else {
                            0
                        };
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(self.error_at(
                                backslash,
                                "json_syntax",
                                "A high surrogate escape must be followed by a low surrogate escape.".to_owned(),
                                "Write characters beyond U+FFFF as a surrogate pair like \\ud83d\\ude00, or simply as the character itself.",
                            ));
                        }
                        // Both halves are in range now, so this cannot overflow.
                        let combined = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                        char::from_u32(combined).ok_or_else(|| {
                            self.error_at(
                                backslash,
                                "json_syntax",
                                "This surrogate pair is not a valid character.".to_owned(),
                                "Check both halves of the pair.",
                            )
                        })?
                    }
                    0xDC00..=0xDFFF => {
                        return Err(self.error_at(
                            backslash,
                            "json_syntax",
                            "A low surrogate escape cannot stand alone.".to_owned(),
                            "It must follow a high surrogate escape; or write the character itself.",
                        ));
                    }
                    _ => char::from_u32(code).ok_or_else(|| {
                        self.error_at(
                            backslash,
                            "json_syntax",
                            "This escape is not a valid character.".to_owned(),
                            "Check the four hexadecimal digits.",
                        )
                    })?,
                }
            }
            other => {
                return Err(self.error_at(
                    backslash,
                    "json_syntax",
                    format!("'\\{other}' is not a valid escape."),
                    "Valid escapes are \\\" \\\\ \\/ \\b \\f \\n \\r \\t and \\uXXXX.",
                ));
            }
        })
    }

    fn enter(&self, depth: usize) -> Result<usize> {
        if depth >= MAX_DEPTH {
            return Err(self.error(
                "json_too_deep",
                format!("Arrays and objects are nested more than {MAX_DEPTH} levels deep."),
                "Flatten the structure; scene files are never this deeply nested.",
            ));
        }
        Ok(depth + 1)
    }

    fn parse_array(&mut self, depth: usize) -> Result<Json> {
        let depth = self.enter(depth)?;
        self.bump(); // '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.parse_value(depth)?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.bump();
                    self.skip_whitespace();
                    if self.peek() == Some(']') {
                        return Err(self.error(
                            "json_syntax",
                            "There is a comma after the last item of this array.".to_owned(),
                            "Remove the trailing comma; JSON does not allow it.",
                        ));
                    }
                }
                Some(']') => {
                    self.bump();
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.unexpected("',' or ']'")),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Json> {
        let depth = self.enter(depth)?;
        self.bump(); // '{'
        let mut members: Vec<Member> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return Err(self.unexpected("a double-quoted key"));
            }
            let key_span = self.span();
            let key = self.parse_string()?;
            if !seen.insert(key.clone()) {
                return Err(self.error_at(
                    key_span,
                    "json_duplicate_key",
                    format!("The key \"{key}\" appears more than once in this object."),
                    "Remove one of them, or merge their contents into one.",
                ));
            }
            self.skip_whitespace();
            if self.peek() != Some(':') {
                return Err(self.unexpected("':' after the key"));
            }
            self.bump();
            let value = self.parse_value(depth)?;
            members.push(Member {
                key,
                key_span,
                value,
            });
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.bump();
                    self.skip_whitespace();
                    if self.peek() == Some('}') {
                        return Err(self.error(
                            "json_syntax",
                            "There is a comma after the last member of this object.".to_owned(),
                            "Remove the trailing comma; JSON does not allow it.",
                        ));
                    }
                }
                Some('}') => {
                    self.bump();
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.unexpected("',' or '}'")),
            }
        }
    }
}

/// Parses a JSON document strictly, remembering where every value is.
///
/// `file` is the project-relative path shown in diagnostics; pass `""` when
/// there is none. A leading byte order mark is accepted (and makes the file
/// non-canonical, see [`is_canonical`]).
pub fn parse(source: &str, file: &str) -> Result<Node> {
    let mut parser = Parser::new(source, file);
    parser.skip_whitespace();
    if parser.peek().is_none() {
        return Err(parser.error(
            "json_syntax",
            "The file is empty; it needs a JSON value.".to_owned(),
            "A scene file contains at least {\"format\": 1}.",
        ));
    }
    let node = parser.parse_value(0)?;
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(parser.error(
            "json_syntax",
            format!(
                "Unexpected {} after the end of the JSON value.",
                describe(parser.peek())
            ),
            "A file holds exactly one JSON value. Remove the extra content, or check for a missing comma or bracket above.",
        ));
    }
    Ok(node)
}

// ---------------------------------------------------------------------------
// Canonical writer
// ---------------------------------------------------------------------------

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

fn indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_value(value: &Value, level: usize, out: &mut String) -> Result<()> {
    if level > MAX_DEPTH {
        return Err(Diagnostic::error(
            "json_too_deep",
            format!("The value is nested more than {MAX_DEPTH} levels deep and cannot be written."),
            "Flatten the structure.",
        ));
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => write_string(text, out),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) if items.iter().all(Value::is_number) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_value(item, level + 1, out)?;
            }
            out.push(']');
        }
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(level + 1, out);
                write_value(item, level + 1, out)?;
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            indent(level, out);
            out.push(']');
        }
        Value::Object(members) if members.is_empty() => out.push_str("{}"),
        Value::Object(members) => {
            // Sorted explicitly: the order of `Map` iteration depends on a
            // `serde_json` feature that another crate may switch on.
            let mut sorted: Vec<(&String, &Value)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push_str("{\n");
            for (index, (key, item)) in sorted.iter().enumerate() {
                indent(level + 1, out);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, level + 1, out)?;
                out.push_str(if index + 1 < sorted.len() {
                    ",\n"
                } else {
                    "\n"
                });
            }
            indent(level, out);
            out.push('}');
        }
    }
    Ok(())
}

/// Writes a value in the canonical form described in the module
/// documentation, ending with a newline.
pub fn to_canonical(value: &Value) -> Result<String> {
    let mut out = String::new();
    write_value(value, 0, &mut out)?;
    out.push('\n');
    Ok(out)
}

/// Whether `source` is exactly the canonical form of what it contains.
/// `false` for text that is not valid JSON.
pub fn is_canonical(source: &str) -> bool {
    parse(source, "")
        .and_then(|node| to_canonical(&node.to_value()))
        .is_ok_and(|canonical| canonical == source)
}
