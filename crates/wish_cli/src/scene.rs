//! Loading a scene file (and the prefabs it uses) into an app.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wish_app::{App, AppConfig};
use wish_core::{Diagnostic, Result};
use wish_scene::{PrefabLibrary, load_prefab, load_with_prefabs};

/// An app with a scene loaded: the example supplies the component types
/// only, the scene supplies every entity and resource.
pub struct SceneApp {
    /// The app, at tick 0.
    pub app: App,
    /// Warnings found while reading the files (errors stop the load).
    pub warnings: Vec<Diagnostic>,
    /// How many entities the scene gave the world.
    pub scene_entities: usize,
}

fn unreadable(path: &Path, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "file_unreadable",
        format!("Could not read '{}': {error}.", path.display()),
        "Check that the file exists and that this project folder is the one the path is relative to (--root).",
    )
}

/// The first error of a failed load, with a note on how many more there are.
fn first_error(mut diagnostics: Vec<Diagnostic>) -> Diagnostic {
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let position = diagnostics
        .iter()
        .position(Diagnostic::is_error)
        .unwrap_or(0);
    let mut first = diagnostics.swap_remove(position);
    if errors > 1 {
        first.hint = format!(
            "{} ({} more problem(s) in this file; fix this one first.)",
            first.hint,
            errors - 1
        );
    }
    first
}

/// Every `*.prefab.json` below `root`, as project-relative `/` paths, in a
/// fixed order so that loading is deterministic.
fn prefab_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".prefab.json"))
                && let Ok(relative) = path.strip_prefix(root)
            {
                let parts: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push((parts.join("/"), path));
            }
        }
    }
    found.sort();
    found
}

/// Builds an app with the types of `example`, loads `scene` (a path relative
/// to `root`) and its prefabs, and creates the scene's entities.
pub fn load_scene_app(
    example: &str,
    scene: &str,
    root: &Path,
    config: AppConfig,
) -> Result<SceneApp> {
    let mut app = App::new(config);
    wish_examples::register_types(example, app.world_mut())?;
    wish_scene::instantiate::register(app.world_mut())?;

    let mut warnings = Vec::new();
    let mut library = PrefabLibrary::new();
    for (name, path) in prefab_files(root) {
        let source = fs::read_to_string(&path).map_err(|error| unreadable(&path, &error))?;
        let result = load_prefab(&source, &name, app.world().registry());
        match result.document {
            Some(document) => {
                library.insert(&name, document);
                warnings.extend(result.diagnostics);
            }
            None => return Err(first_error(result.diagnostics)),
        }
    }

    let path = root.join(scene);
    let source = fs::read_to_string(&path).map_err(|error| unreadable(&path, &error))?;
    let result = load_with_prefabs(&source, scene, app.world().registry(), Arc::new(library));
    let Some(document) = result.document else {
        return Err(first_error(result.diagnostics));
    };
    warnings.extend(result.diagnostics);
    let entities = document.instantiate(app.world_mut())?;
    Ok(SceneApp {
        app,
        warnings,
        scene_entities: entities.len(),
    })
}
