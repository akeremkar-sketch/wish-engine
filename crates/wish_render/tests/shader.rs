//! Shader mistakes come back as positioned diagnostics.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_render::check_wgsl;

const GOOD: &str = "@fragment\nfn main() -> @location(0) vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n";

#[test]
fn a_valid_shader_passes() {
    check_wgsl(GOOD, "shaders/red.wgsl").unwrap();
    check_wgsl(include_str!("../src/shaders/probe.wgsl"), "probe.wgsl").unwrap();
}

#[test]
fn mistakes_name_the_file_line_and_column() {
    // (name, source, code, line, column, text expected in the message)
    let cases = [
        (
            "missing semicolon",
            "fn f() -> f32 {\n    let x = 1.0\n    return x;\n}\n",
            "shader_syntax",
            3,
            5,
            "return",
        ),
        (
            "unknown identifier",
            "fn f() -> f32 {\n    return yy;\n}\n",
            "shader_syntax",
            2,
            12,
            "yy",
        ),
        (
            "type mismatch",
            // naga places a validation error at the expression with the
            // wrong type: `x` is the literal `1`, so it points there.
            "fn f() -> f32 {\n    let x: i32 = 1;\n    return x;\n}\n",
            "shader_validation",
            2,
            18,
            "return",
        ),
        (
            "non-ASCII before the mistake: columns count characters",
            "// ölçü\nfn f() -> f32 { return ğ; }\n",
            "shader_syntax",
            2,
            24,
            "",
        ),
    ];
    for (name, source, code, line, column, text) in cases {
        let problem = check_wgsl(source, "shaders/broken.wgsl").unwrap_err();
        assert_eq!(problem.code, code, "{name}: {problem}");
        assert_eq!(
            problem.location.file.as_deref(),
            Some("shaders/broken.wgsl"),
            "{name}"
        );
        assert_eq!(
            (problem.location.line, problem.location.column),
            (Some(line), Some(column)),
            "{name}: {problem}"
        );
        assert!(
            problem.message.contains(text),
            "{name}: {}",
            problem.message
        );
        assert!(!problem.hint.is_empty(), "{name}");
    }
}

#[test]
fn import_mistakes_name_the_file_line_and_a_hint() {
    use wish_render::ShaderLibrary;
    let mut library = ShaderLibrary::new();
    library.add("a", "shaders/a.wgsl", "#import b\nfn a() {}\n");
    library.add("b", "shaders/b.wgsl", "// b\n#import a\nfn b() {}\n");
    library.add("c", "shaders/c.wgsl", "\n  #import brfd\nfn c() {}\n");
    library.add("brdf", "shaders/brdf.wgsl", "fn f() {}\n");
    library.add("d", "shaders/d.wgsl", "#import two words\n");

    let cycle = library.check("a").unwrap_err();
    assert_eq!(cycle.code, "shader_import_cycle");
    assert!(cycle.message.contains("a -> b -> a"), "{}", cycle.message);
    assert!(!cycle.hint.is_empty());

    let unknown = library.check("c").unwrap_err();
    assert_eq!(unknown.code, "shader_import_unknown");
    assert_eq!(unknown.location.file.as_deref(), Some("shaders/c.wgsl"));
    assert_eq!(unknown.location.line, Some(2));
    assert_eq!(unknown.hint, "Did you mean 'brdf'?");

    let syntax = library.check("d").unwrap_err();
    assert_eq!(syntax.code, "shader_import_syntax");
    assert_eq!(syntax.location.line, Some(1));
    assert!(syntax.hint.contains("#import"));

    // An entry that does not exist at all.
    assert_eq!(
        library.check("zzz").unwrap_err().code,
        "shader_import_unknown"
    );
}
