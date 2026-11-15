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

/// Every `*.prefab.json` of the project, as project-relative `/` paths, in
/// path order. The same walk as `wen check`: hidden folders and links are
/// skipped.
fn prefab_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    Ok(wish_asset::index::documents(root)?
        .into_iter()
        .filter(|(name, _)| name.ends_with(".prefab.json"))
        .collect())
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
    crate::types::register_scene_types(example, app.world_mut())?;

    let mut warnings = Vec::new();
    let mut library = PrefabLibrary::new();
    for (name, path) in prefab_files(root)? {
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
