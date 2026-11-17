//! `project_export` (M9 WP1): the game as a folder that runs on a machine
//! without the engine: the player (no editor, no MCP server, no compiler),
//! the scenes, prefabs, assets, settings and built game code, and the
//! licenses (ADR-023).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

/// The engine's own licenses, shipped with every exported game.
const LICENSE_MIT: &str = include_str!("../../../../LICENSE-MIT");
const LICENSE_APACHE: &str = include_str!("../../../../LICENSE-APACHE");

pub(crate) const PROJECT_EXPORT: Tool = Tool {
    name: "project_export",
    title: "Export the game",
    description: "Makes the game a folder that runs on a machine without the engine: exports/<name>/ inside the project, with <name>.exe (the player: no editor, no MCP server, no compiler), game/ (scenes, prefabs, assets, wish.json, input.json and the built game code) and LICENSES.txt. The project must pass check, and its game code (if any) must be built (script_build). Zip the folder to share it. An existing export of that name is replaced only with overwrite.",
    params: &[
        Param::optional(
            "name",
            Kind::String,
            "The folder's and the program's name, 1 to 40 of a-z 0-9 _ - (default: from the project's name).",
        ),
        Param::optional(
            "overwrite",
            Kind::Boolean,
            "Replace an earlier export of that name.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check and list what would be exported, without writing.",
        ),
    ],
    mutates: true,
    run: project_export,
};

/// A folder name from a project's name: `Island Hopper` → `island_hopper`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out
        .trim_end_matches('_')
        .chars()
        .take(40)
        .collect::<String>();
    if out.is_empty() {
        "game".to_owned()
    } else if crate::scaffold::reserved_on_windows(&out) {
        format!("{out}_game")
    } else {
        out
    }
}

fn valid(name: &str) -> bool {
    !name.is_empty()
        && !crate::scaffold::reserved_on_windows(name)
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The player program to copy: `Workspace::set_player_path`, `WISH_PLAYER`,
/// or `wish_player` next to the running program.
fn player(workspace: &Workspace) -> Option<PathBuf> {
    if let Some(path) = workspace.player_path.clone() {
        return Some(path);
    }
    if let Some(path) = std::env::var_os("WISH_PLAYER") {
        return Some(PathBuf::from(path));
    }
    let here = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "wish_player.exe"
    } else {
        "wish_player"
    };
    Some(here.parent()?.join(name))
}

fn copy_file(from: &Path, to: &Path) -> Result<u64> {
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder).map_err(|error| unwritable(to, &error))?;
    }
    std::fs::copy(from, to).map_err(|error| unwritable(to, &error))
}

fn unwritable(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unwritable",
        format!("Could not write '{}': {error}.", path.display()),
        "Check that the project's exports folder can be written to (close a running export first).",
    )
}

fn project_export(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
) -> Result<Map<String, Value>> {
    let project_name = workspace
        .config()
        .map_or_else(|| "game".to_owned(), |config| config.name.clone());
    let name = args
        .str("name")
        .map_or_else(|| slug(&project_name), str::to_owned);
    if !valid(&name) {
        return Err(Diagnostic::error(
            "invalid_export_name",
            format!("'{name}' cannot name an export."),
            "Use 1 to 40 of a-z, 0-9, _ and -, such as \"island_hopper\".",
        )
        .at_path("/name"));
    }
    // Unbuilt game code first: its types are what check would miss.
    let root = workspace.root().to_path_buf();
    let has_code = root.join(wish_script::build::SCRIPTS_ENTRY).is_file();
    let wasm = root.join(wish_script::build::GAME_WASM);
    let needs_build = |message: &str| {
        Diagnostic::error(
            "export_needs_build",
            format!("{message}, so it was not exported."),
            "Build it first (script_build, or Build code in the editor), then export again.",
        )
    };
    // The code shipped is the code the project runs: built, loaded, and
    // not older than its sources.
    let mut code = None;
    if has_code {
        let (Some(module), Ok(bytes)) = (workspace.game_module(), std::fs::read(&wasm)) else {
            return Err(needs_build("The project has game code that is not built"));
        };
        if wish_script::fingerprint_of(&bytes) != module.fingerprint() {
            return Err(needs_build(
                "The built game code on disk is not the code the project runs (another program built it)",
            ));
        }
        if sources_newer_than(&root.join("scripts"), &wasm) {
            return Err(needs_build("The game code changed since it was last built"));
        }
        code = Some(bytes);
    }
    // Only a project that is right goes out.
    let checked = workspace.run_tool("check", &json!({}), actor)?;
    if checked["passed"] != true {
        let first = checked["diagnostics"]
            .as_array()
            .and_then(|all| all.iter().find(|d| d["severity"] == "error"))
            .and_then(|d| d["message"].as_str())
            .unwrap_or("check found errors")
            .to_owned();
        return Err(Diagnostic::error(
            "export_has_errors",
            format!("The project has errors, so it was not exported: {first}"),
            "Fix what check reports (the editor's Problems tab lists them), then export again.",
        ));
    }
    let Some(player) = player(workspace).filter(|path| path.is_file()) else {
        return Err(Diagnostic::error(
            "player_missing",
            "The player program (wish_player) was not found next to the engine.",
            "Build it (cargo build --release -p wish_player) or set WISH_PLAYER to its path.",
        ));
    };
    let folder = format!("{}{name}", wish_asset::index::EXPORTS_FOLDER);
    let out = root.join(&folder);
    let exports = root.join(wish_asset::index::EXPORTS_FOLDER.trim_end_matches('/'));
    // Never through a link: the export is written and replaced only inside
    // the project.
    for path in [&exports, &out] {
        if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(Diagnostic::error(
                "export_outside_project",
                format!(
                    "'{}' is a link, so nothing was exported through it.",
                    path.display()
                ),
                "Remove the link (exports/ must be a real folder inside the project), then export again.",
            ));
        }
    }
    if out.exists() && !args.flag("overwrite") {
        return Err(Diagnostic::error(
            "export_exists",
            format!("'{folder}' exists already; it was not replaced."),
            "Pass \"overwrite\": true to replace it, or give another name.",
        )
        .at_path("/name"));
    }
    // What goes in: the documents, the assets, the settings, the code.
    let mut files: Vec<String> = wish_asset::index::documents(&root)?
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    files.extend(
        wish_asset::index::AssetIndex::scan(&root)?
            .paths()
            .map(str::to_owned),
    );
    for path in [crate::project::PROJECT_FILE, wish_input::INPUT_FILE] {
        if root.join(path).is_file() {
            files.push(path.to_owned());
        }
    }
    files.sort();
    files.dedup();
    let program = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.clone()
    };
    let mut out_map = Map::new();
    out_map.insert("folder".to_owned(), json!(folder));
    out_map.insert("program".to_owned(), json!(format!("{folder}/{program}")));
    out_map.insert("files".to_owned(), json!(files.len()));
    let player_bytes = std::fs::read(&player).map_err(|error| {
        Diagnostic::error(
            "player_missing",
            format!(
                "The player program '{}' could not be read: {error}.",
                player.display()
            ),
            "Build it (cargo build --release -p wish_player) or set WISH_PLAYER to its path.",
        )
    })?;
    let mut warnings = Vec::new();
    if crate::pe::needs_c_runtime_dll(&player_bytes) {
        warnings.push(Diagnostic::warning(
            "export_needs_c_runtime",
            "The player was built with the C runtime as a DLL, so the game may not start on a Windows without the Visual C++ runtime.",
            "Use the player of an installed engine, or build it with RUSTFLAGS=-C target-feature=+crt-static.",
        ));
    }
    if !warnings.is_empty() {
        out_map.insert("diagnostics".to_owned(), json!(warnings));
    }
    if args.flag("dry_run") {
        let mut would = files.clone();
        if code.is_some() {
            would.push(wish_script::build::GAME_WASM.to_owned());
        }
        out_map.insert("would_copy".to_owned(), json!(would));
        out_map.insert("dry_run".to_owned(), json!(true));
        return Ok(out_map);
    }
    std::fs::create_dir_all(&exports).map_err(|error| unwritable(&exports, &error))?;
    let inside = std::fs::canonicalize(&exports)
        .ok()
        .zip(std::fs::canonicalize(&root).ok())
        .is_some_and(|(exports, root)| exports.starts_with(root));
    if !inside {
        return Err(Diagnostic::error(
            "export_outside_project",
            format!("'{}' is not inside the project.", exports.display()),
            "exports/ must be a real folder inside the project; remove what is there, then export again.",
        ));
    }
    // Made beside the old one and swapped in only when complete: a failure
    // halfway leaves the earlier export as it was.
    let building = exports.join(format!(".{name}.exporting"));
    let _ = std::fs::remove_dir_all(&building);
    let made = write_export(
        &building,
        &program,
        &player_bytes,
        &root,
        &files,
        code.as_deref(),
        &project_name,
    );
    let bytes = match made {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&building);
            return Err(error);
        }
    };
    if out.exists() {
        let old = exports.join(format!(".{name}.replaced"));
        let _ = std::fs::remove_dir_all(&old);
        if let Err(error) = std::fs::rename(&out, &old) {
            let _ = std::fs::remove_dir_all(&building);
            return Err(unwritable(&out, &error));
        }
        let _ = std::fs::remove_dir_all(&old);
    }
    std::fs::rename(&building, &out).map_err(|error| unwritable(&out, &error))?;
    out_map.insert("bytes".to_owned(), json!(bytes));
    Ok(out_map)
}

/// Whether any file under `folder` changed after `built` was written.
fn sources_newer_than(folder: &Path, built: &Path) -> bool {
    let Ok(built) = std::fs::metadata(built).and_then(|meta| meta.modified()) else {
        return true;
    };
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                folders.push(entry.path());
            } else if meta.modified().is_ok_and(|changed| changed > built) {
                return true;
            }
        }
    }
    false
}

/// Writes the whole export into `out`; returns the bytes written.
fn write_export(
    out: &Path,
    program: &str,
    player: &[u8],
    root: &Path,
    files: &[String],
    code: Option<&[u8]>,
    project_name: &str,
) -> Result<u64> {
    std::fs::create_dir_all(out).map_err(|error| unwritable(out, &error))?;
    std::fs::write(out.join(program), player)
        .map_err(|error| unwritable(&out.join(program), &error))?;
    // Written as bytes, the program must be made runnable again (Unix).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out.join(program), std::fs::Permissions::from_mode(0o755))
            .map_err(|error| unwritable(&out.join(program), &error))?;
    }
    let mut bytes = player.len() as u64;
    for path in files {
        bytes += copy_file(&root.join(path), &out.join("game").join(path))?;
    }
    if let Some(code) = code {
        let to = out.join("game").join(wish_script::build::GAME_WASM);
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(|error| unwritable(&to, &error))?;
        }
        std::fs::write(&to, code).map_err(|error| unwritable(&to, &error))?;
        bytes += code.len() as u64;
    }
    let licenses = format!(
        "{project_name} was made with Wish Engine, which is free and open source under the MIT license or the Apache License 2.0, at your option. Both texts follow, then the licenses of the libraries the engine uses.\n\n==== MIT ====\n\n{LICENSE_MIT}\n\n==== Apache License 2.0 ====\n\n{LICENSE_APACHE}\n\n==== Third-party libraries ====\n\n{}",
        crate::licenses::THIRD_PARTY
    );
    std::fs::write(out.join("LICENSES.txt"), &licenses)
        .map_err(|error| unwritable(&out.join("LICENSES.txt"), &error))?;
    Ok(bytes + licenses.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_from_the_project() {
        assert_eq!(slug("Island Hopper"), "island_hopper");
        assert_eq!(slug("  Ada Atlama!  "), "ada_atlama");
        assert_eq!(slug("ÜÇ"), "game");
        assert!(valid("island_hopper") && !valid("Island") && !valid(""));
        // Names Windows keeps for devices.
        assert_eq!(slug("Con"), "con_game");
        assert!(!valid("nul") && !valid("com1") && valid("console"));
    }
}
