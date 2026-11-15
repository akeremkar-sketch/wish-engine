//! The strict JSON parser, source positions and the canonical writer.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use wish_scene::json::{MAX_DEPTH, Span, is_canonical, parse, to_canonical};

/// The line and column of the first occurrence of `needle` in `source`,
/// counted the way the parser counts: from 1, in characters.
fn position_of(source: &str, needle: &str) -> Span {
    let offset = source.find(needle).expect("needle is in the source");
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32 + 1;
    let column = before.rsplit('\n').next().unwrap().chars().count() as u32 + 1;
    Span { line, column }
}

#[test]
fn values_and_keys_remember_where_they_are() {
    let source = "{\n  \"entities\": {\n    \"player\": {\n      \"components\": { \"Transform\": { \"translation\": [0, 1, 0] } }\n    }\n  }\n}\n";
    let node = parse(source, "scenes/main.scene.json").unwrap();

    let located = node
        .locate("/entities/player/components/Transform/translation")
        .unwrap();
    assert_eq!(located.key, Some(position_of(source, "\"translation\"")));
    assert_eq!(located.value, position_of(source, "[0, 1, 0]"));

    let item = node
        .locate("/entities/player/components/Transform/translation/1")
        .unwrap();
    assert_eq!(item.key, None);
    assert_eq!(item.value, position_of(source, "1, 0]"));

    let root = node.locate("").unwrap();
    assert_eq!((root.key, root.value), (None, Span { line: 1, column: 1 }));

    assert!(node.locate("/entities/nobody").is_none());
    assert!(
        node.locate("/entities/player/components/Transform/translation/3")
            .is_none()
    );
    assert!(
        node.locate("entities").is_none(),
        "a pointer starts with '/'"
    );
    assert!(
        node.locate("/entities/player/components/Transform/translation/x")
            .is_none()
    );
}

#[test]
fn pointer_escapes_are_resolved() {
    let node = parse("{\"a/b\": {\"c~d\": 1}}", "").unwrap();
    let located = node.locate("/a~1b/c~0d").unwrap();
    assert_eq!(
        located.value,
        Span {
            line: 1,
            column: 17
        }
    );
}

#[test]
fn columns_count_characters_not_bytes() {
    let source = "{\"é€\": x}";
    let error = parse(source, "a.json").unwrap_err();
    // `{` is column 1, the key `"é€"` takes columns 2 to 5, `:` is 6, a space 7, `x` is 8.
    assert_eq!(error.location.line, Some(1));
    assert_eq!(error.location.column, Some(8));
}

#[test]
fn a_byte_order_mark_is_accepted_but_not_canonical() {
    let node = parse("\u{feff}{\"a\": 1}", "").unwrap();
    assert_eq!(node.span, Span { line: 1, column: 1 });
    assert!(!is_canonical("\u{feff}{\n  \"a\": 1\n}\n"));
    assert!(is_canonical("{\n  \"a\": 1\n}\n"));
}

#[test]
fn the_canonical_form_is_exactly_this() {
    let input = r#"{"b": [1,2,3], "a": {"z": null, "y": []}, "c": [ {"k": 1}, "s" ], "d": {}, "e": [1.0, 1e2, 0.10]}"#;
    let expected = "{
  \"a\": {
    \"y\": [],
    \"z\": null
  },
  \"b\": [1, 2, 3],
  \"c\": [
    {
      \"k\": 1
    },
    \"s\"
  ],
  \"d\": {},
  \"e\": [1.0, 100.0, 0.1]
}
";
    let node = parse(input, "").unwrap();
    assert_eq!(to_canonical(&node.to_value()).unwrap(), expected);
    assert!(is_canonical(expected));
    assert!(!is_canonical(input));
    assert!(
        !is_canonical(&expected.replace('\n', "\r\n")),
        "CRLF is not canonical"
    );
    assert!(
        !is_canonical(expected.trim_end()),
        "the final newline is required"
    );
    assert!(!is_canonical("{"), "invalid JSON is not canonical");
}

#[test]
fn keys_are_sorted_by_utf8_bytes() {
    let value = json!({ "b": 1, "B": 2, "a": 3, "é": 4, "z": 5, "_": 6 });
    let text = to_canonical(&value).unwrap();
    let keys: Vec<&str> = text
        .lines()
        .skip(1)
        .take(6)
        .map(|line| line.trim().split('"').nth(1).unwrap())
        .collect();
    assert_eq!(keys, ["B", "_", "a", "b", "z", "é"]);
}

#[test]
fn strings_escape_only_what_must_be_escaped() {
    let value = json!("a\"b\\c\n\t\r\u{1}\u{8}\u{c}é€😀/");
    assert_eq!(
        to_canonical(&value).unwrap(),
        "\"a\\\"b\\\\c\\n\\t\\r\\u0001\\b\\fé€😀/\"\n"
    );
    // Escapes of every kind read back to the same string.
    let node = parse(r#""é😀\/A""#, "").unwrap();
    assert_eq!(node.to_value(), json!("é😀/A"));
}

#[test]
fn too_deep_values_are_refused_both_ways() {
    let source = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
    assert_eq!(parse(&source, "").unwrap_err().code, "json_too_deep");
    let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
    assert!(parse(&ok, "").is_ok());

    let mut value = json!(1);
    for _ in 0..=MAX_DEPTH + 1 {
        value = json!([value]);
    }
    assert_eq!(to_canonical(&value).unwrap_err().code, "json_too_deep");
}

/// Every mistake is reported where it was made, with a hint that says what to do.
#[test]
fn syntax_errors_say_where_and_how_to_fix() {
    // (source, code, line, column, a word the hint must contain)
    let cases: &[(&str, &str, u32, u32, &str)] = &[
        (r#"{"a": 1,}"#, "json_syntax", 1, 9, "trailing"),
        ("[1, 2,]", "json_syntax", 1, 7, "trailing"),
        ("// hi\n{}", "json_syntax", 1, 1, "comments"),
        ("{'a': 1}", "json_syntax", 1, 2, "double quotes"),
        (r#"{"a": 1, "a": 2}"#, "json_duplicate_key", 1, 10, "Remove"),
        (r#"{"a": 01}"#, "json_syntax", 1, 8, "07"),
        ("\"abc", "json_syntax", 1, 1, "closing"),
        ("\"a\nb\"", "json_syntax", 1, 3, "\\n"),
        (r#""\x""#, "json_syntax", 1, 2, "Valid escapes"),
        (r#""\ud800""#, "json_syntax", 1, 2, "surrogate"),
        (r#""\udc00""#, "json_syntax", 1, 2, "surrogate"),
        (r#""\ud800A""#, "json_syntax", 1, 2, "surrogate"),
        (r#""\ud800x""#, "json_syntax", 1, 2, "surrogate"),
        (r#""\u12""#, "json_syntax", 1, 2, "four"),
        ("{} x", "json_syntax", 1, 4, "exactly one"),
        ("NaN", "json_syntax", 1, 1, "NaN"),
        (r#"{"a" 1}"#, "json_syntax", 1, 6, ":"),
        ("[1 2]", "json_syntax", 1, 4, "]"),
        ("[1,\n  2,\n  x]", "json_syntax", 3, 3, "lowercase"),
        ("1e999", "json_number_out_of_range", 1, 1, "between"),
        ("{\"a\": .5}", "json_syntax", 1, 7, "0.5"),
        ("{\"a\": 1.}", "json_syntax", 1, 9, "digit"),
        ("{\"a\": 1e}", "json_syntax", 1, 9, "digit"),
        ("{\"a\": tru}", "json_syntax", 1, 7, "lowercase"),
        ("{\"a\":", "json_syntax", 1, 6, "ends too early"),
        ("{1: 2}", "json_syntax", 1, 2, "double-quoted key"),
    ];
    for (source, code, line, column, word) in cases {
        let error = parse(source, "scenes/main.scene.json").unwrap_err();
        assert_eq!(&error.code, code, "{source:?}: {error}");
        assert_eq!(error.location.line, Some(*line), "{source:?} line: {error}");
        assert_eq!(
            error.location.column,
            Some(*column),
            "{source:?} column: {error}"
        );
        assert_eq!(
            error.location.file.as_deref(),
            Some("scenes/main.scene.json")
        );
        assert!(
            error.hint.contains(word),
            "{source:?}: hint {:?} lacks {word:?}",
            error.hint
        );
        assert!(!error.message.is_empty());
    }
}

#[test]
fn empty_input_is_an_error_not_a_crash() {
    for source in ["", "   \n\t ", "\u{feff}"] {
        let error = parse(source, "").unwrap_err();
        assert_eq!(error.code, "json_syntax", "{source:?}");
        assert!(error.message.contains("empty"), "{}", error.message);
        assert_eq!(error.location.file, None);
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn any_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|number| json!(number)),
        any::<f64>()
            .prop_filter("finite", |number| number.is_finite())
            .prop_map(|number| json!(number)),
        any::<String>().prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map(any::<String>(), inner, 0..6)
                .prop_map(|members| Value::Object(members.into_iter().collect())),
        ]
    })
}

proptest! {
    /// Any value written canonically parses back to the same value, and
    /// writing that again gives the same bytes: the canonical form is a
    /// fixed point, which is what makes "load then save" a no-op.
    #[test]
    fn canonical_output_is_a_fixed_point(value in any_value()) {
        let text = to_canonical(&value).unwrap();
        prop_assert!(is_canonical(&text));
        let node = parse(&text, "x.json").unwrap();
        prop_assert_eq!(node.to_value(), value);
        prop_assert_eq!(to_canonical(&node.to_value()).unwrap(), text);
    }

    /// Every position the parser reports points at the value it describes.
    #[test]
    fn located_positions_point_at_their_values(value in any_value()) {
        let text = to_canonical(&value).unwrap();
        let node = parse(&text, "").unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut stack = vec![(String::new(), &node)];
        while let Some((pointer, current)) = stack.pop() {
            let located = node.locate(&pointer).unwrap();
            prop_assert_eq!(located.value, current.span);
            let line = lines[current.span.line as usize - 1];
            let character = line.chars().nth(current.span.column as usize - 1).unwrap();
            let expected = match &current.value {
                wish_scene::json::Json::Object(_) => '{',
                wish_scene::json::Json::Array(_) => '[',
                wish_scene::json::Json::String(_) => '"',
                wish_scene::json::Json::Null => 'n',
                wish_scene::json::Json::Bool(true) => 't',
                wish_scene::json::Json::Bool(false) => 'f',
                wish_scene::json::Json::Number(_) => character,
            };
            prop_assert_eq!(character, expected);
            match &current.value {
                wish_scene::json::Json::Object(members) => {
                    for member in members {
                        let escaped = member.key.replace('~', "~0").replace('/', "~1");
                        stack.push((format!("{pointer}/{escaped}"), &member.value));
                    }
                }
                wish_scene::json::Json::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        stack.push((format!("{pointer}/{index}"), item));
                    }
                }
                _ => {}
            }
        }
    }

    /// The parser never panics, whatever it is given; it either accepts the
    /// text or explains the first problem with a position.
    #[test]
    fn arbitrary_text_never_panics(text in "\\PC{0,60}") {
        match parse(&text, "x.json") {
            Ok(node) => { let _ = to_canonical(&node.to_value()); }
            Err(error) => {
                prop_assert!(error.location.line.is_some());
                prop_assert!(!error.hint.is_empty());
            }
        }
    }
}
