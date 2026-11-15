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
