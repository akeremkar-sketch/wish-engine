//! `wen new`: a project folder ready to open with an AI agent or by hand.
//!
//! ```text
//! wish.json                 the project file
//! scenes/main.scene.json    a camera, a sun and a ground: something to see
//! assets/                   models and images go here
//! .mcp.json                 connects Claude Code and other MCP clients (`wen mcp`)
//! CLAUDE.md                 how an agent should work in this project
//! .gitignore
//! ```

use std::path::{Path, PathBuf};

use serde_json::json;
use wish_core::{Diagnostic, Result};

use crate::project::{PROJECT_FILE, ProjectConfig};

fn unwritable(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unwritable",
        format!("Could not write '{}': {error}.", path.display()),
        "Check that the folder can be written to.",
    )
}

fn write(root: &Path, relative: &str, text: &str, written: &mut Vec<String>) -> Result<()> {
    let path = root.join(relative);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| unwritable(folder, &error))?;
    }
    std::fs::write(&path, text).map_err(|error| unwritable(&path, &error))?;
    written.push(relative.to_owned());
    Ok(())
}

/// The scene a new project starts with.
fn starting_scene() -> Result<String> {
    wish_scene::json::to_canonical(&json!({
        "format": 1,
        "entities": {
            "camera": { "components": {
                "Camera": { "fov_y_deg": 50 },
                "Transform": { "translation": [0, 2, 7], "rotation": [-0.0871557, 0, 0, 0.9961947] }
            } },
            "ground": { "components": {
                "Primitive": { "shape": "Plane" },
                "Material": { "base_color": [0.45, 0.45, 0.42, 1], "roughness": 0.9 },
                "Transform": { "scale": [30, 1, 30] }
            } },
            "sun": { "components": {
                "DirectionalLight": { "illuminance_lux": 100000 },
                "Transform": { "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] }
            } }
        }
    }))
}

fn mcp_config(command: &str) -> String {
    let text = serde_json::to_string_pretty(&json!({
        "mcpServers": { "wish": { "command": command, "args": ["mcp"] } }
    }))
    .unwrap_or_default();
    format!("{text}\n")
}

fn guide(name: &str) -> String {
    format!(
        "# {name}\n\
\n\
A Wish Engine project. You work on it through the engine's tools (MCP server `wish`, started by\n\
`wen mcp`; the same tools run on the command line as `wen call <tool>`), not by editing files by hand.\n\
\n\
## How to work\n\
\n\
1. `project_info` first: what is here, and how many problems there are.\n\
2. `schema_get` (with `search`) before writing component values: every field has a type, unit and range.\n\
3. Change scenes with `edit_apply`: many operations in one call, all or nothing. Use `dry_run` to check first.\n\
4. Look before you claim: `capture_frame` for a picture, `describe_view` for what is on screen and how\n\
   visible it is, `entity_query` for values.\n\
5. Every error has a `hint`. Read it; it says how to fix the call.\n\
\n\
## Rules\n\
\n\
- Units: meters, seconds, kilograms, radians in the API (Transform also takes `euler_deg`, in degrees).\n\
- Coordinates: right-handed, +Y up, a camera looks along -Z.\n\
- A person may edit the files or the scene by hand at the same time. `edit_apply` tells you what changed\n\
  (`changed_by_others`); never overwrite their work: use `base_revision` when you prepared changes earlier.\n\
- Scene files are canonical JSON written by the engine. If you must edit one as text, run `wen fmt` after.\n\
- `check` verifies the whole project; run it before you finish.\n"
    )
}

/// Creates a project in `dir` (made if missing). Refuses a folder that
/// already has a `wish.json`. `command` is how MCP clients start the engine
/// (`wen`, or its full path). Returns the files written, project-relative.
pub fn create_project(dir: &Path, name: &str, command: &str) -> Result<Vec<String>> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(Diagnostic::error(
            "invalid_project_name",
            "A project name must be 1 to 80 characters without control characters.",
            "Pass --name \"My game\".",
        ));
    }
    if dir.join(PROJECT_FILE).exists() {
        return Err(Diagnostic::error(
            "project_exists",
            format!("'{}' already has a {PROJECT_FILE}.", dir.display()),
            "Choose a new folder, or open this project with `wen call project_info --root <folder>`.",
        ));
    }
    let mut written = Vec::new();
    write(
        dir,
        PROJECT_FILE,
        &ProjectConfig::new(name).to_canonical()?,
        &mut written,
    )?;
    write(
        dir,
        "scenes/main.scene.json",
        &starting_scene()?,
        &mut written,
    )?;
    write(dir, "assets/.gitkeep", "", &mut written)?;
    write(dir, ".mcp.json", &mcp_config(command), &mut written)?;
    write(dir, "CLAUDE.md", &guide(name), &mut written)?;
    write(dir, ".gitignore", "target/\n*.wish-tmp\n", &mut written)?;
    Ok(written)
}

/// The default project name for a folder: its last component.
pub fn name_for(dir: &Path) -> String {
    let absolute: PathBuf = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    absolute
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("My game")
        .to_owned()
}
