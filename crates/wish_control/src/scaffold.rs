//! `wen new`: a project folder ready to open with an AI agent or by hand.
//!
//! ```text
//! wish.json                 the project file
//! scenes/main.scene.json    a camera, a sun and a ground: something to see
//! assets/                   models and images go here
//! scripts/src/lib.rs        game code (Rust with wish_sdk; built by the engine)
//! tests/boots.test.json     a scenario test (`wen test`)
//! input.json                actions and the keys that drive them (move, jump)
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

/// The scenario test a new project starts with: the scene starts, and has a
/// camera, after a second of play.
const STARTING_TEST: &str = "{
  \"format\": 1,
  \"scene\": \"scenes/main.scene.json\",
  \"seed\": 1,
  \"steps\": [
    { \"frames\": 60 },
    { \"expect\": \"has(Camera)\", \"message\": \"the scene has a camera to look through\" }
  ]
}
";

/// The game code a new project starts with: one component and one system,
/// small enough to read at a glance, real enough to build on.
const STARTING_CODE: &str =
    "//! The game's code: components and systems, in Rust, with the engine's SDK.
//! The engine compiles it to WebAssembly with `script_build` (or `wen build`);
//! no Cargo or other tools are needed. A running game picks up the new code
//! between two ticks and keeps its state.

use wish_sdk::prelude::*;

/// Turns an entity around its up (Y) axis.
#[component]
pub struct Spin {
    /// How fast it turns.
    #[schema(unit = \"rad/s\", range = -50.0..=50.0, default = 1.0)]
    pub speed: f32,
}

/// Turns every entity that has a `Spin`.
#[system]
fn spin(mut spinning: Query<(&mut Transform, &Spin)>, time: Res<Time>) {
    for (transform, spin) in spinning.iter_mut() {
        transform.rotation = Quat::from_rotation_y(spin.speed * time.delta) * transform.rotation;
    }
}

wish_sdk::export! {
    components: [Spin],
    systems: [spin],
}
";

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
- `check` verifies the whole project; run it before you finish.\n\
\n\
## Game code\n\
\n\
- Behaviour lives in `scripts/src/lib.rs`: Rust with `use wish_sdk::prelude::*;`. Declare data with\n\
  `#[component]` / `#[resource]` / `#[event]` (every field gets a `///` comment), logic with `#[system]`\n\
  functions taking `Query`, `Res`, `ResMut`, `Commands`, `EventReader`, `EventWriter`; list them in\n\
  `wish_sdk::export! {{ components: [...], systems: [...] }}` at the end.\n\
- Edit the file, then `script_build`: errors come back with file, line and hint; running sessions switch\n\
  to the new code without losing their state. Then `sim_start` / `sim_step` and look.\n\
- All state lives in components and resources (the module starts fresh every tick). No clock, files or\n\
  network: time comes from `Res<Time>`, randomness from `wish_sdk::random_f32()` (deterministic).\n\
- Prove it with a scenario test: `tests/<name>.test.json` runs the game from a scene and a seed through steps
  (`{{ \"input\": {{ \"move\": [0, 1] }}, \"frames\": 60 }}`, `{{ \"until\": \"Score.points >= 3\", \"max_frames\": 600 }}`,
  `{{ \"expect\": \"Score.points >= 3\" }}`, `{{ \"capture\": {{ \"golden\": \"tests/golden/win.png\" }} }}`); `test_run`
  says which step failed, at which tick, and what the values were. Run it before you finish.
- Input is actions, not keys: `input.json` maps keys to named actions (`move`, `jump`), and a system takes\n\
  `actions: Actions` (`actions.axis2(\"move\")`, `actions.pressed(\"jump\")`). To play the game yourself, give\n\
  `sim_step` an `input` such as `{{ \"move\": [0, 1], \"jump\": true }}`: it is held for that many ticks.\n"
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
    write(dir, "scripts/src/lib.rs", STARTING_CODE, &mut written)?;
    write(dir, "tests/boots.test.json", STARTING_TEST, &mut written)?;
    write(
        dir,
        wish_input::INPUT_FILE,
        &wish_input::InputMap::starter().to_canonical()?,
        &mut written,
    )?;
    write(dir, ".mcp.json", &mcp_config(command), &mut written)?;
    write(dir, "CLAUDE.md", &guide(name), &mut written)?;
    write(
        dir,
        ".gitignore",
        ".wish/\ntarget/\n*.wish-tmp\n",
        &mut written,
    )?;
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
