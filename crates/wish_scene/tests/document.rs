//! Loading and validating scene files.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use wish_core::math::{Quat, Vec3};
use wish_ecs::World;
use wish_scene::json::Span;
use wish_scene::{LoadResult, SceneDocument, is_valid_scene_id, load};
use wish_schema::{component, resource, schema};

/// Position, rotation and scale.
#[component]
pub struct Transform {
    #[schema(unit = "m", alias = "position")]
    pub translation: Vec3,
    pub rotation: Quat,
    #[schema(default = Vec3::ONE)]
    pub scale: Vec3,
}

/// A camera.
#[component]
pub struct Camera {
    #[schema(unit = "deg", range = 1.0..=179.0, default = 60.0)]
    pub fov_y_deg: f32,
}

#[schema]
pub enum Shape {
    Box,
    Sphere,
}

/// A collider.
#[component]
pub struct Collider {
    pub shape: Shape,
}

/// Marks the player.
#[component]
pub struct Player;

/// World gravity.
#[resource]
pub struct Gravity {
    #[schema(default = Vec3::new(0.0, -9.81, 0.0))]
    pub value: Vec3,
}

fn world() -> World {
    let mut world = World::new();
    world.register_component::<Transform>().unwrap();
    world.register_component::<Camera>().unwrap();
    world.register_component::<Collider>().unwrap();
    world.register_component::<Player>().unwrap();
    world.register_resource::<Gravity>().unwrap();
    world
}

fn load_in(source: &str, world: &World) -> LoadResult {
    load(source, "scenes/main.scene.json", world.registry())
}

/// The line and column of the `nth` occurrence of `needle`, counted the way
/// the parser counts: from 1, in characters.
fn position_of(source: &str, needle: &str, nth: usize) -> Span {
    let offset = source
        .match_indices(needle)
        .nth(nth)
        .expect("needle is in the source")
        .0;
    let before = &source[..offset];
    Span {
        line: before.matches('\n').count() as u32 + 1,
        column: before.rsplit('\n').next().unwrap().chars().count() as u32 + 1,
    }
}

const SAMPLE: &str = r#"{
  "format": 2,
  "entities": {
    "player": {
      "components": {
        "Transform": { "translation": [0, 1, 0] },
        "Player": {}
      },
      "notes": "Moved by the input system."
    },
    "camera": {
      "parent": "player",
      "components": { "Camera": { "fov_y_deg": 75 }, "Transform": { "translation": [0, 2, 6] } }
    },
    "light": { "components": {}, "order": -1 },
    "crate": { "parent": "player", "order": -5, "components": { "Collider": { "shape": "Box" } } }
  },
  "resources": { "Gravity": { "value": [0, -9.81, 0] } }
}
"#;

#[test]
fn a_valid_scene_loads() {
    let world = world();
    let result = load_in(SAMPLE, &world);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let document = result.document.unwrap();

    let ids: Vec<&str> = document.entities().map(|(id, _)| id).collect();
    assert_eq!(ids, ["camera", "crate", "light", "player"]);
    let player = document.entity("player").unwrap();
    assert_eq!(player.notes(), Some("Moved by the input system."));
    assert_eq!(player.parent(), None);
    assert_eq!(
        player.components()["Transform"],
        json!({ "translation": [0, 1, 0] })
    );
    assert!(player.components().contains_key("Player"));
    assert_eq!(document.entity("light").unwrap().order(), -1);
    assert_eq!(
        document.resources()["Gravity"],
        json!({ "value": [0, -9.81, 0] })
    );
    assert_eq!(document.revision(), 0);

    // Siblings sort by `order`, then by id.
    assert_eq!(document.roots(), ["light", "player"]);
    assert_eq!(document.children("player"), ["crate", "camera"]);
    assert!(document.children("camera").is_empty());
}

#[test]
fn loose_files_load_with_a_warning_and_canonical_files_are_stable() {
    let world = world();
    let result = load_in(SAMPLE, &world);
    // SAMPLE is hand-formatted, not canonical.
    assert_eq!(result.diagnostics.len(), 1);
    let warning = &result.diagnostics[0];
    assert_eq!(warning.code, "not_canonical");
    assert_eq!(warning.severity, wish_core::Severity::Warning);
    assert!(warning.hint.contains("wen fmt"));
    assert_eq!(
        warning.location.file.as_deref(),
        Some("scenes/main.scene.json")
    );
    assert!(result.document.is_some(), "a warning does not stop loading");

    // Saving writes the canonical form, which then loads with no diagnostics
    // and saves to exactly the same bytes: load -> save is a no-op.
    let canonical = result.document.unwrap().to_canonical().unwrap();
    let again = load_in(&canonical, &world);
    assert!(again.diagnostics.is_empty(), "{:?}", again.diagnostics);
    assert_eq!(again.document.unwrap().to_canonical().unwrap(), canonical);
}

#[test]
fn empty_sections_are_left_out_of_the_canonical_file() {
    let world = world();
    let canonical = "{\n  \"format\": 2\n}\n";
    let empty = load_in(canonical, &world);
    assert!(empty.diagnostics.is_empty(), "{:?}", empty.diagnostics);
    assert_eq!(empty.document.unwrap(), SceneDocument::new());
    assert_eq!(SceneDocument::new().to_canonical().unwrap(), canonical);

    // Spelling the empty sections out is allowed, but not canonical.
    let spelled = load_in(
        "{\"format\": 2, \"entities\": {}, \"resources\": {}}",
        &world,
    );
    assert_eq!(spelled.diagnostics.len(), 1);
    assert_eq!(spelled.diagnostics[0].code, "not_canonical");
    assert!(spelled.document.is_some());
}

/// A file of format 1 (the format before the look lock of 1.0) is still
/// read: as a document of the current format, with a warning that says how
/// to rewrite it. Its own canonical text stays format 1 (`wen fmt` tidies,
/// it does not migrate). The engine's recorded defaults are filled in only
/// for the engine's own types: this registry's `Camera`, `Transform` and
/// `Collider` have the engine's names and are someone else's, so nothing is
/// written into them and no engine resource is added.
#[test]
fn a_file_of_format_1_is_read_as_the_current_format_with_a_warning() {
    let world = world();
    let old = "{\n  \"entities\": {\n    \"a\": {\n      \"components\": {\n        \"Camera\": {},\n        \"Collider\": {},\n        \"Transform\": {}\n      }\n    }\n  },\n  \"format\": 1\n}\n";
    let result = load_in(old, &world);
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| &*d.code).collect();
    assert_eq!(codes, ["old_format"], "{:?}", result.diagnostics);
    let warning = &result.diagnostics[0];
    assert_eq!(warning.severity, wish_core::Severity::Warning);
    assert!(warning.hint.contains("wen migrate"), "{}", warning.hint);
    assert!(warning.message.contains("format 1"), "{}", warning.message);
    assert_eq!(warning.location.path.as_deref(), Some("/format"));
    assert_eq!(
        (warning.location.line, warning.location.column),
        (Some(11), Some(13)),
        "at the number"
    );
    assert_eq!(result.older_format, Some(1));
    // Tidied in its own format: the file is canonical as it is.
    assert_eq!(result.canonical.as_deref(), Some(old));
    let document = result.document.unwrap();
    assert_eq!(
        document.entity("a").unwrap().components()["Camera"],
        json!({})
    );
    assert!(document.resources().is_empty());
    // Saved, it is the current format, and loads without a word.
    let saved = document.to_canonical().unwrap();
    assert_eq!(saved, old.replace("\"format\": 1", "\"format\": 2"));
    let again = load_in(&saved, &world);
    assert!(again.diagnostics.is_empty(), "{:?}", again.diagnostics);
    assert_eq!(again.older_format, None);
    assert_eq!(again.canonical.as_deref(), Some(saved.as_str()));

    // A loose file of format 1 gets both notes, and its tidy form is still
    // format 1.
    let loose = load_in("{\"format\": 1, \"entities\": {}}", &world);
    let codes: Vec<&str> = loose.diagnostics.iter().map(|d| &*d.code).collect();
    assert_eq!(codes, ["old_format", "not_canonical"]);
    assert_eq!(loose.canonical.as_deref(), Some("{\n  \"format\": 1\n}\n"));
    // A file with an error has neither a document nor a tidy form.
    let broken = load_in("{\"format\": 1, \"entities\": {\"A\": {}}}", &world);
    assert!(broken.has_errors());
    assert!(broken.document.is_none() && broken.canonical.is_none());
    assert_eq!(broken.older_format, None);
}

#[test]
fn scene_ids() {
    for valid in ["player", "a", "player_1", "x9", &"a".repeat(64)] {
        assert!(is_valid_scene_id(valid), "{valid}");
    }
    for invalid in [
        "",
        "Player",
        "1up",
        "_x",
        "has-dash",
        "has space",
        "é",
        &"a".repeat(65),
    ] {
        assert!(!is_valid_scene_id(invalid), "{invalid}");
    }
}

/// Every one of these files is wrong in exactly one way, and the diagnostic
/// points at that place and says how to fix it.
#[test]
fn broken_files_are_reported_where_they_are_broken() {
    let long_id = "a".repeat(65);
    let long_id_source = format!("{{\"format\": 2, \"entities\": {{\"{long_id}\": {{}}}}}}");
    // (name, source, code, needle, which occurrence, a word the hint contains)
    let cases: Vec<(&str, String, &str, String, usize, &str)> = vec![
        ("syntax: trailing comma", r#"{"format": 2,}"#.into(), "json_syntax", "}".into(), 0, "trailing"),
        ("syntax: empty file", String::new(), "json_syntax", String::new(), 0, "scene file"),
        ("syntax: duplicate entity", r#"{"format": 2, "entities": {"player": {}, "player": {}}}"#.into(), "json_duplicate_key", "\"player\"".into(), 1, "Remove"),
        ("syntax: unterminated", "{\"format\": 2, \"entities\": {\"a\": \"x}".into(), "json_syntax", "\"x}".into(), 0, "closing"),
        ("root is an array", "[]".into(), "type_mismatch", "[]".into(), 0, "object"),
        ("missing format", "{}".into(), "missing_format", "{}".into(), 0, "\"format\": 2"),
        ("format is a string", r#"{"format": "1"}"#.into(), "type_mismatch", "\"1\"".into(), 0, "number"),
        ("format from the future", r#"{"format": 3}"#.into(), "unsupported_format", "3".into(), 0, "update the engine"),
        ("format no engine wrote", r#"{"format": 0}"#.into(), "unsupported_format", "0".into(), 0, "version control"),
        ("misspelled top-level key", r#"{"format": 2, "entites": {}}"#.into(), "unknown_key", "\"entites\"".into(), 0, "Did you mean 'entities'?"),
        ("entities is an array", r#"{"format": 2, "entities": []}"#.into(), "type_mismatch", "[]".into(), 0, "object"),
        ("uppercase id", r#"{"format": 2, "entities": {"Player": {}}}"#.into(), "invalid_scene_id", "\"Player\"".into(), 0, "lowercase"),
        ("id starts with a digit", r#"{"format": 2, "entities": {"1up": {}}}"#.into(), "invalid_scene_id", "\"1up\"".into(), 0, "lowercase"),
        ("id too long", long_id_source, "invalid_scene_id", format!("\"{long_id}\""), 0, "64"),
        ("entity is a number", r#"{"format": 2, "entities": {"a": 5}}"#.into(), "type_mismatch", "5".into(), 0, "object"),
        ("misspelled entity key", r#"{"format": 2, "entities": {"a": {"komponents": {}}}}"#.into(), "unknown_key", "\"komponents\"".into(), 0, "Did you mean 'components'?"),
        ("components is an array", r#"{"format": 2, "entities": {"a": {"components": []}}}"#.into(), "type_mismatch", "[]".into(), 0, "object"),
        ("unknown component", r#"{"format": 2, "entities": {"a": {"components": {"Transfrom": {}}}}}"#.into(), "unknown_component", "\"Transfrom\"".into(), 0, "Did you mean 'Transform'?"),
        ("managed component", r#"{"format": 2, "entities": {"a": {"components": {"Hierarchy": {}}}}}"#.into(), "managed_component", "\"Hierarchy\"".into(), 0, "parent"),
        ("unknown field", r#"{"format": 2, "entities": {"a": {"components": {"Transform": {"position": [0, 1, 0]}}}}}"#.into(), "unknown_field", "\"position\"".into(), 0, "Did you mean 'translation'?"),
        ("field has the wrong type", r#"{"format": 2, "entities": {"a": {"components": {"Transform": {"translation": "up"}}}}}"#.into(), "type_mismatch", "\"up\"".into(), 0, "array"),
        ("vector has the wrong length", r#"{"format": 2, "entities": {"a": {"components": {"Transform": {"translation": [1, 2]}}}}}"#.into(), "wrong_length", "[1, 2]".into(), 0, "[x, y, z]"),
        ("number out of range", r#"{"format": 2, "entities": {"a": {"components": {"Camera": {"fov_y_deg": 500}}}}}"#.into(), "value_out_of_range", "500".into(), 0, "range"),
        ("unknown enum variant", r#"{"format": 2, "entities": {"a": {"components": {"Collider": {"shape": "Cube"}}}}}"#.into(), "unknown_variant", "\"Cube\"".into(), 0, "Sphere"),
        ("error inside an array", r#"{"format": 2, "entities": {"a": {"components": {"Transform": {"translation": [0, "x", 0]}}}}}"#.into(), "type_mismatch", "\"x\"".into(), 0, "number"),
        ("parent is a number", r#"{"format": 2, "entities": {"a": {"parent": 3}}}"#.into(), "type_mismatch", "3".into(), 0, "string"),
        ("unknown parent", r#"{"format": 2, "entities": {"player": {}, "a": {"parent": "playr"}}}"#.into(), "unknown_parent", "\"playr\"".into(), 0, "Did you mean 'player'?"),
        ("entity is its own parent", r#"{"format": 2, "entities": {"a": {"parent": "a"}}}"#.into(), "hierarchy_cycle", "\"a\"".into(), 1, "Remove"),
        ("two entities are each other's parent", r#"{"format": 2, "entities": {"a": {"parent": "b"}, "b": {"parent": "a"}}}"#.into(), "hierarchy_cycle", "\"b\"".into(), 0, "loop"),
        ("order is fractional", r#"{"format": 2, "entities": {"a": {"order": 1.5}}}"#.into(), "type_mismatch", "1.5".into(), 0, "whole number"),
        ("order is a string", r#"{"format": 2, "entities": {"a": {"order": "first"}}}"#.into(), "type_mismatch", "\"first\"".into(), 0, "whole number"),
        ("notes is a number", r#"{"format": 2, "entities": {"a": {"notes": 7}}}"#.into(), "type_mismatch", "7".into(), 0, "string"),
        ("resources is a number", r#"{"format": 2, "resources": 3}"#.into(), "type_mismatch", "3".into(), 0, "object"),
        ("unknown resource", r#"{"format": 2, "resources": {"Gravty": {}}}"#.into(), "unknown_resource", "\"Gravty\"".into(), 0, "Did you mean 'Gravity'?"),
        ("resource field has the wrong length", r#"{"format": 2, "resources": {"Gravity": {"value": [9, 9]}}}"#.into(), "wrong_length", "[9, 9]".into(), 0, "[x, y, z]"),
        ("multi-line file: error on line 3", "{\n  \"format\": 2,\n  \"entities\": { \"a\": { \"components\": { \"Camera\": { \"fov_y_deg\": 0 } } } }\n}\n".into(), "value_out_of_range", "0 }".into(), 0, "range"),
    ];
    assert!(
        cases.len() >= 30,
        "the exit criterion asks for 30 broken files"
    );

    let world = world();
    for (name, source, code, needle, nth, word) in &cases {
        let result = load_in(source, &world);
        assert!(
            result.document.is_none(),
            "{name}: a file with an error gives no document"
        );
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.is_error())
            .unwrap_or_else(|| panic!("{name}: no error reported"));
        assert_eq!(&diagnostic.code, code, "{name}: {diagnostic}");
        let expected = position_of(source, needle, *nth);
        assert_eq!(
            (diagnostic.location.line, diagnostic.location.column),
            (Some(expected.line), Some(expected.column)),
            "{name}: wrong position: {diagnostic}"
        );
        assert_eq!(
            diagnostic.location.file.as_deref(),
            Some("scenes/main.scene.json"),
            "{name}"
        );
        assert!(
            diagnostic.hint.contains(word),
            "{name}: the hint {:?} should contain {word:?}",
            diagnostic.hint
        );
        assert!(!diagnostic.message.is_empty(), "{name}");
    }
}

#[test]
fn a_loop_is_reported_once_and_every_independent_error_is_reported() {
    let world = world();
    let source = r#"{
  "format": 2,
  "entities": {
    "a": { "parent": "b" },
    "b": { "parent": "c" },
    "c": { "parent": "a" },
    "d": { "components": { "Camera": { "fov_y_deg": 0 } } },
    "e": { "components": { "Nothing": {} } },
    "f": { "parent": "zz" }
  }
}"#;
    let result = load_in(source, &world);
    let codes: Vec<(&str, u32)> = result
        .diagnostics
        .iter()
        .map(|d| (d.code.as_ref(), d.location.line.unwrap()))
        .collect();
    // Errors come in the order the file's sections are read: first
    // components (line 7, 8), then the loop and the unknown parent.
    assert_eq!(
        codes,
        [
            ("value_out_of_range", 7),
            ("unknown_component", 8),
            ("unknown_parent", 9),
            ("hierarchy_cycle", 4),
        ]
    );
    let cycle = result
        .diagnostics
        .iter()
        .find(|d| d.code == "hierarchy_cycle")
        .unwrap();
    assert_eq!(
        cycle.message,
        "These entities are each other's ancestors: a -> b -> c -> a."
    );
    assert_eq!(cycle.location.path.as_deref(), Some("/entities/a/parent"));
    assert_eq!(cycle.candidates, ["a", "b", "c"]);
}

#[test]
fn diagnostics_carry_json_pointers_too() {
    let world = world();
    let source =
        r#"{"format": 2, "entities": {"a": {"components": {"Transform": {"scale": [1, 1]}}}}}"#;
    let result = load_in(source, &world);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(
        diagnostic.location.path.as_deref(),
        Some("/entities/a/components/Transform/scale")
    );
    assert_eq!(
        diagnostic.message,
        "Expected 3 items for 'Transform.scale', found 2."
    );
}

#[test]
fn unknown_components_list_what_exists_but_never_managed_ones() {
    let world = world();
    let source = r#"{"format": 2, "entities": {"a": {"components": {"Zzz": {}}}}}"#;
    let result = load_in(source, &world);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, "unknown_component");
    assert!(diagnostic.candidates.contains(&"Transform".to_owned()));
    assert!(!diagnostic.candidates.contains(&"Hierarchy".to_owned()));
}

#[test]
fn files_without_a_path_still_report_positions() {
    let world = world();
    let result = load("{\"format\": 2,}", "", world.registry());
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.location.file, None);
    assert_eq!(
        (diagnostic.location.line, diagnostic.location.column),
        (Some(1), Some(14))
    );
}
