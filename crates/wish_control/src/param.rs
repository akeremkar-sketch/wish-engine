//! Tool parameters: one table per tool that gives the MCP input schema, the
//! command-line flags and the parser, so the three can never disagree.
//!
//! Arguments are checked here, before a tool runs, and every mistake comes
//! back as a diagnostic that says what was expected and how to write it:
//! an agent should be able to fix its call on the first retry.

use serde_json::{Map, Value, json};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};

/// What kind of value a parameter takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Any string.
    String,
    /// A whole number within the bounds (both included).
    Integer {
        /// Smallest allowed value.
        min: i64,
        /// Largest allowed value.
        max: i64,
    },
    /// A number within the bounds (both included).
    Number {
        /// Smallest allowed value.
        min: f64,
        /// Largest allowed value.
        max: f64,
    },
    /// `true` or `false`.
    Boolean,
    /// A list of strings.
    StringList,
    /// A JSON object (its shape is checked by the tool).
    Object,
    /// A JSON array (its items are checked by the tool).
    Array,
    /// One of a fixed set of strings.
    Choice(&'static [&'static str]),
}

impl Kind {
    /// How the kind is described in messages.
    fn describe(&self) -> String {
        match self {
            Self::String => "a string".to_owned(),
            Self::Integer { min, max } => format!("a whole number from {min} to {max}"),
            Self::Number { min, max } => format!("a number from {min} to {max}"),
            Self::Boolean => "true or false".to_owned(),
            Self::StringList => "a list of strings".to_owned(),
            Self::Object => "a JSON object".to_owned(),
            Self::Array => "a JSON array".to_owned(),
            Self::Choice(choices) => format!("one of {}", quoted(choices)),
        }
    }

    /// An example value, for hints.
    fn example(&self) -> String {
        match self {
            Self::String => "\"text\"".to_owned(),
            Self::Integer { min, max } => (*min).max(0).min(*max).to_string(),
            Self::Number { min, max } => format!("{}", 1f64.max(*min).min(*max)),
            Self::Boolean => "true".to_owned(),
            Self::StringList => "[\"a\", \"b\"]".to_owned(),
            Self::Object => "{ ... }".to_owned(),
            Self::Array => "[ ... ]".to_owned(),
            Self::Choice(choices) => format!("\"{}\"", choices.first().copied().unwrap_or("")),
        }
    }

    fn schema(&self) -> Value {
        match self {
            Self::String => json!({ "type": "string" }),
            Self::Integer { min, max } => {
                json!({ "type": "integer", "minimum": min, "maximum": max })
            }
            Self::Number { min, max } => {
                let mut schema = Map::new();
                schema.insert("type".to_owned(), json!("number"));
                if min.is_finite() {
                    schema.insert("minimum".to_owned(), json!(min));
                }
                if max.is_finite() {
                    schema.insert("maximum".to_owned(), json!(max));
                }
                Value::Object(schema)
            }
            Self::Boolean => json!({ "type": "boolean" }),
            Self::StringList => json!({ "type": "array", "items": { "type": "string" } }),
            Self::Object => json!({ "type": "object" }),
            Self::Array => json!({ "type": "array" }),
            Self::Choice(choices) => json!({ "type": "string", "enum": choices }),
        }
    }
}

fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One parameter of a tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    /// The name, in `snake_case`; on the command line `--name`.
    pub name: &'static str,
    /// What it takes.
    pub kind: Kind,
    /// Whether a call must give it.
    pub required: bool,
    /// The value used when it is left out, as JSON text (`None`: no value).
    pub default: Option<&'static str>,
    /// What it means, for the schema and the help.
    pub doc: &'static str,
}

impl Param {
    /// An optional parameter with no default.
    pub const fn optional(name: &'static str, kind: Kind, doc: &'static str) -> Self {
        Self {
            name,
            kind,
            required: false,
            default: None,
            doc,
        }
    }

    /// A parameter every call must give.
    pub const fn required(name: &'static str, kind: Kind, doc: &'static str) -> Self {
        Self {
            name,
            kind,
            required: true,
            default: None,
            doc,
        }
    }

    /// The same parameter with a default value (JSON text).
    pub const fn or(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }
}

/// The JSON Schema of a tool's arguments (an MCP `inputSchema`).
pub fn input_schema(params: &[Param]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in params {
        let mut schema = param.kind.schema();
        if let Some(object) = schema.as_object_mut() {
            object.insert("description".to_owned(), json!(param.doc));
            if let Some(default) = param
                .default
                .and_then(|text| serde_json::from_str(text).ok())
            {
                object.insert("default".to_owned(), default);
            }
        }
        properties.insert(param.name.to_owned(), schema);
        if param.required {
            required.push(param.name);
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// Checked arguments, with defaults filled in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    values: Map<String, Value>,
}

impl Args {
    /// The raw value of a parameter, if given or defaulted.
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.values.get(name).filter(|value| !value.is_null())
    }

    /// A string parameter.
    pub fn str(&self, name: &str) -> Option<&str> {
        self.value(name).and_then(Value::as_str)
    }

    /// A whole-number parameter.
    pub fn int(&self, name: &str) -> Option<i64> {
        self.value(name).and_then(Value::as_i64)
    }

    /// A number parameter.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.value(name).and_then(Value::as_f64)
    }

    /// A boolean parameter (`false` when absent).
    pub fn flag(&self, name: &str) -> bool {
        self.value(name).and_then(Value::as_bool).unwrap_or(false)
    }

    /// A string-list parameter (empty when absent).
    pub fn strings(&self, name: &str) -> Vec<String> {
        self.value(name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn invalid(tool: &str, param: &Param, got: &Value) -> Diagnostic {
    let shown = match got {
        Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    };
    let shown: String = shown.chars().take(80).collect();
    Diagnostic::error(
        "invalid_parameter",
        format!(
            "'{tool}' parameter '{}' must be {}, but got {shown}.",
            param.name,
            param.kind.describe()
        ),
        format!(
            "Write it like \"{}\": {}. ({})",
            param.name,
            param.kind.example(),
            param.doc
        ),
    )
    .at_path(format!("/{}", param.name))
}

/// A JSON number that is a whole number (`3` or `3.0`), as an integer.fn whole(value: &Value) -> Option<i64> {    value.as_i64().or_else(|| {        value            .as_f64()            .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)            .map(|n| n as i64)    })}
/// A JSON number that is a whole number (`3` or `3.0`), as an integer.
fn whole(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
            .map(|n| n as i64)
    })
}

fn check_value(tool: &str, param: &Param, value: &Value) -> Result<()> {
    let ok = match param.kind {
        Kind::String => value.is_string(),
        Kind::Integer { min, max } => {
            whole(value).is_some_and(|number| (min..=max).contains(&number))
        }
        Kind::Number { min, max } => value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number >= min && number <= max),
        Kind::Boolean => value.is_boolean(),
        Kind::StringList => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
        Kind::Object => value.is_object(),
        Kind::Array => value.is_array(),
        Kind::Choice(choices) => value.as_str().is_some_and(|text| choices.contains(&text)),
    };
    if ok {
        return Ok(());
    }
    let mut problem = invalid(tool, param, value);
    if let (Kind::Choice(choices), Some(text)) = (param.kind, value.as_str()) {
        problem.hint = format!(
            "{} Valid values: {}.",
            unknown_name_hint(text, choices),
            quoted(choices)
        );
        problem = problem.with_candidates(choices.iter().copied());
    }
    Err(problem)
}

/// Checks `args` (a JSON object, or `null` for none) against `params` and
/// fills in defaults.
pub fn parse(tool: &str, params: &[Param], args: &Value) -> Result<Args> {
    let given = match args {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        other => {
            return Err(Diagnostic::error(
                "invalid_arguments",
                format!(
                    "The arguments of '{tool}' must be a JSON object, but got {}.",
                    kind_of(other)
                ),
                format!(
                    "Pass an object such as {{ \"{}\": ... }}, or {{}} for none.",
                    params.first().map_or("name", |param| param.name)
                ),
            ));
        }
    };
    let names: Vec<&str> = params.iter().map(|param| param.name).collect();
    for key in given.keys() {
        if !names.contains(&key.as_str()) {
            let hint = if names.is_empty() {
                format!("'{tool}' takes no parameters; pass {{}}.")
            } else {
                unknown_name_hint(key, &names)
            };
            return Err(Diagnostic::error(
                "unknown_parameter",
                format!("'{tool}' has no parameter '{key}'."),
                hint,
            )
            .at_path(format!("/{key}"))
            .with_candidates(names.iter().copied()));
        }
    }
    let mut values = Map::new();
    for param in params {
        match given.get(param.name) {
            Some(value) if !value.is_null() => {
                check_value(tool, param, value)?;
                // 640.0 is 640: store whole numbers as integers.
                let value = match (param.kind, whole(value)) {
                    (Kind::Integer { .. }, Some(number)) => Value::from(number),
                    _ => value.clone(),
                };
                values.insert(param.name.to_owned(), value);
            }
            _ if param.required => {
                return Err(Diagnostic::error(
                    "missing_parameter",
                    format!("'{tool}' needs the parameter '{}'.", param.name),
                    format!(
                        "Add \"{}\": {} ({}).",
                        param.name,
                        param.kind.example(),
                        param.doc
                    ),
                )
                .at_path(format!("/{}", param.name)));
            }
            _ => {
                if let Some(default) = param
                    .default
                    .and_then(|text| serde_json::from_str(text).ok())
                {
                    values.insert(param.name.to_owned(), default);
                }
            }
        }
    }
    Ok(Args { values })
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Turns command-line flags (`--name value`, already split into pairs) into
/// an argument object for [`parse`]. Strings stay strings; numbers, booleans
/// and JSON values are read by the parameter's kind; a list takes either
/// JSON (`["a","b"]`) or comma-separated text.
pub fn from_flags(tool: &str, params: &[Param], flags: &[(String, String)]) -> Result<Value> {
    let names: Vec<&str> = params.iter().map(|param| param.name).collect();
    let mut object = Map::new();
    for (flag, text) in flags {
        let name = flag.trim_start_matches('-').replace('-', "_");
        let Some(param) = params.iter().find(|param| param.name == name) else {
            let flags: Vec<String> = names.iter().map(|name| format!("--{name}")).collect();
            let flag_refs: Vec<&str> = flags.iter().map(String::as_str).collect();
            return Err(Diagnostic::error(
                "unknown_parameter",
                format!("'{tool}' has no parameter '{flag}'."),
                if flags.is_empty() {
                    format!("'{tool}' takes no parameters.")
                } else {
                    unknown_name_hint(flag, &flag_refs)
                },
            )
            .with_candidates(flag_refs));
        };
        let value = match param.kind {
            Kind::String | Kind::Choice(_) => Value::String(text.clone()),
            Kind::StringList if !text.trim_start().starts_with('[') => Value::Array(
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| Value::String(item.to_owned()))
                    .collect(),
            ),
            Kind::Boolean if text.is_empty() => Value::Bool(true),
            _ => serde_json::from_str(text).map_err(|_| {
                invalid(tool, param, &Value::String(text.clone())).at_path(format!("/{name}"))
            })?,
        };
        object.insert(name, value);
    }
    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAMS: &[Param] = &[
        Param::required("scene", Kind::String, "The scene file."),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 100 },
            "At most this many.",
        )
        .or("10"),
        Param::optional(
            "detail",
            Kind::Choice(&["summary", "normal", "full"]),
            "How much.",
        )
        .or("\"summary\""),
        Param::optional("names", Kind::StringList, "Which ones."),
        Param::optional("dry_run", Kind::Boolean, "Only preview."),
    ];

    #[test]
    fn a_whole_number_written_with_a_point_is_a_whole_number() {
        // Agents and JSON libraries sometimes write 640 as 640.0.
        let args = parse("t", PARAMS, &json!({ "scene": "s", "limit": 5.0 })).unwrap();
        assert_eq!(args.int("limit"), Some(5));
        assert!(parse("t", PARAMS, &json!({ "scene": "s", "limit": 5.5 })).is_err());
    }

    #[test]
    fn good_arguments_get_their_defaults() {
        let args = parse("t", PARAMS, &json!({ "scene": "a.scene.json" })).unwrap();
        assert_eq!(args.str("scene"), Some("a.scene.json"));
        assert_eq!(args.int("limit"), Some(10));
        assert_eq!(args.str("detail"), Some("summary"));
        assert!(args.strings("names").is_empty());
        assert!(!args.flag("dry_run"));
    }

    #[test]
    fn every_mistake_says_what_was_expected() {
        let cases = [
            (json!({}), "missing_parameter", "/scene"),
            (
                json!({ "scene": "s", "scen": 1 }),
                "unknown_parameter",
                "/scen",
            ),
            (json!({ "scene": 3 }), "invalid_parameter", "/scene"),
            (
                json!({ "scene": "s", "limit": 0 }),
                "invalid_parameter",
                "/limit",
            ),
            (
                json!({ "scene": "s", "limit": 2.5 }),
                "invalid_parameter",
                "/limit",
            ),
            (
                json!({ "scene": "s", "detail": "ful" }),
                "invalid_parameter",
                "/detail",
            ),
            (
                json!({ "scene": "s", "names": [1] }),
                "invalid_parameter",
                "/names",
            ),
        ];
        for (args, code, path) in cases {
            let problem = parse("t", PARAMS, &args).unwrap_err();
            assert_eq!(problem.code, code, "{args}");
            assert_eq!(problem.location.path.as_deref(), Some(path), "{args}");
            assert!(!problem.hint.is_empty(), "{args}");
        }
        let problem = parse("t", PARAMS, &json!({ "scene": "s", "scen": 1 })).unwrap_err();
        assert_eq!(problem.hint, "Did you mean 'scene'?");
        let problem = parse("t", PARAMS, &json!({ "scene": "s", "detail": "ful" })).unwrap_err();
        assert!(
            problem.hint.starts_with("Did you mean 'full'?"),
            "{}",
            problem.hint
        );
        assert_eq!(problem.candidates, ["summary", "normal", "full"]);
        let problem = parse("t", PARAMS, &json!([1])).unwrap_err();
        assert_eq!(problem.code, "invalid_arguments");
        assert!(
            parse("t", PARAMS, &Value::Null).is_err(),
            "scene is required"
        );
        assert!(parse("t", &[], &Value::Null).is_ok());
    }

    #[test]
    fn flags_become_the_same_arguments() {
        let flags = [
            ("--scene".to_owned(), "a.scene.json".to_owned()),
            ("--limit".to_owned(), "5".to_owned()),
            ("--names".to_owned(), "Transform, Camera".to_owned()),
            ("--dry-run".to_owned(), String::new()),
        ];
        let value = from_flags("t", PARAMS, &flags).unwrap();
        assert_eq!(
            value,
            json!({ "scene": "a.scene.json", "limit": 5, "names": ["Transform", "Camera"], "dry_run": true })
        );
        let json_list = [("--names".to_owned(), "[\"A\",\"B\"]".to_owned())];
        assert_eq!(
            from_flags("t", PARAMS, &json_list).unwrap(),
            json!({ "names": ["A", "B"] })
        );
        let problem =
            from_flags("t", PARAMS, &[("--limt".to_owned(), "1".to_owned())]).unwrap_err();
        assert_eq!(problem.hint, "Did you mean '--limit'?");
        let problem =
            from_flags("t", PARAMS, &[("--limit".to_owned(), "x".to_owned())]).unwrap_err();
        assert_eq!(problem.code, "invalid_parameter");
    }

    #[test]
    fn the_schema_lists_every_parameter_and_what_is_required() {
        let schema = input_schema(PARAMS);
        assert_eq!(schema["required"], json!(["scene"]));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["properties"]["limit"]["maximum"], json!(100));
        assert_eq!(schema["properties"]["limit"]["default"], json!(10));
        assert_eq!(
            schema["properties"]["detail"]["enum"],
            json!(["summary", "normal", "full"])
        );
        assert_eq!(
            schema["properties"]["scene"]["description"],
            json!("The scene file.")
        );
    }
}
