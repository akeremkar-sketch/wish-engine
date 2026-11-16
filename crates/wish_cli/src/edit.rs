//! `wen edit`: the editor, in a window or (for scripts and tests) as one
//! picture drawn from an input script.

use std::path::Path;

use serde_json::json;
use wish_control::{Workspace, WorkspaceOptions};
use wish_core::{Diagnostic, Result};
use wish_editor::EditorApp;
use wish_render::RgbaImage;
use wish_window::ShellOptions;

use crate::args::EditArgs;

fn unwritable(path: &str, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unwritable",
        format!("Could not write '{path}': {error}."),
        "Check that the folder exists and can be written to.",
    )
}

/// Runs `wen edit`.
pub fn edit(args: &EditArgs, as_json: bool) -> Result<String> {
    let root = Path::new(args.root.as_deref().unwrap_or(".")).to_path_buf();
    let mut workspace = Workspace::open(WorkspaceOptions {
        root,
        example: None,
    })?;
    workspace.set_software(args.software);
    let mut app = EditorApp::new(workspace);
    // A window keeps the person's choice of language; a scripted picture
    // depends only on its arguments.
    if args.headless_frame.is_none()
        && let Some(path) = wish_editor::preferences_path()
    {
        app.use_preferences(path);
    }
    if let Some(code) = &args.lang
        && !app.set_language(code)
    {
        let known: Vec<_> = wish_editor::i18n::LANGUAGES
            .iter()
            .map(|language| language.code)
            .collect();
        return Err(Diagnostic::error(
            "unknown_language",
            format!("The editor has no language '{code}'."),
            format!("Use one of: {}.", known.join(", ")),
        ));
    }
    if let Some(script_path) = &args.headless_frame {
        let Some(out) = &args.out else {
            return Err(Diagnostic::error(
                "missing_parameter",
                "--headless-frame needs --out, the PNG to write.",
                "Write it like: wen edit --headless-frame input.json --out frame.png",
            ));
        };
        let text = std::fs::read_to_string(script_path).map_err(|error| {
            Diagnostic::error(
                "file_unreadable",
                format!("Could not read '{script_path}': {error}."),
                "Give the path of an editor input script (JSON); see wish_editor::script for the format.",
            )
        })?;
        let mut script: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
            Diagnostic::error(
                "invalid_editor_script",
                format!("'{script_path}' is not JSON: {error}."),
                "Write { \"size\": [1280, 720], \"steps\": [ { \"events\": [ { \"click\": [100, 40] } ] } ] }.",
            )
        })?;
        if script.get("size").is_none() {
            script["size"] = json!([args.size.0, args.size.1]);
        }
        let ran = wish_editor::script::run_script(&mut app, &script)?;
        let picture = RgbaImage {
            width: ran.canvas.width,
            height: ran.canvas.height,
            pixels: ran.canvas.pixels,
        };
        std::fs::write(out, picture.to_png()?).map_err(|error| unwritable(out, &error))?;
        return Ok(if as_json {
            json!({ "ok": true, "frames": ran.frames, "out": out, "status": app.status() })
                .to_string()
        } else {
            format!("Ran {} frame(s); wrote {out}. {}", ran.frames, app.status())
        });
    }
    let report = wish_window::run_shell(
        ShellOptions {
            size: args.size,
            exit_after_frames: args.exit_after,
        },
        &mut app,
    )?;
    Ok(if as_json {
        json!({ "ok": true, "frames": report.frames }).to_string()
    } else {
        format!("Closed after {} frame(s).", report.frames)
    })
}
