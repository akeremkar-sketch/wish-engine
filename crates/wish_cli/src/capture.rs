//! `wen shaders`, and what `wen view` shares with the control plane: naming
//! entities and loading models.

use std::path::Path;

use serde_json::json;
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{RenderScene, Renderer};
use wish_scene::SceneId;

use crate::project_cmds::Outcome;

/// The name an entity has in the scene file, or its numeric form when it was
/// not made by the scene.
pub(crate) fn scene_id_of(world: &wish_ecs::World, entity: Entity) -> String {
    world
        .get::<SceneId>(entity)
        .and_then(|scene_id| world.text(scene_id.id))
        .map_or_else(|| entity.to_string(), str::to_owned)
}

/// Loads every model the scene's Model entities name from the project and
/// uploads it. A model that cannot be loaded is reported as a warning and not
/// drawn (the rest of the scene still is); its import warnings are passed on.
pub(crate) fn load_models(
    scene: &RenderScene,
    root: &Path,
    gpu: &wish_render::Gpu,
    renderer: &mut Renderer,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    let mut paths: Vec<&str> = scene
        .draws
        .iter()
        .filter_map(|draw| draw.model_path.as_deref())
        .map(|path| path.split('#').next().unwrap_or(path))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    for path in paths {
        let loaded = wish_import::load_model_file(&root.join(path), path)
            .and_then(|model| renderer.upload_model(gpu, path, &model).map(|()| model));
        match loaded {
            Ok(model) => warnings.extend(model.warnings),
            Err(problem) => warnings.push(
                Diagnostic::warning(
                    "model_not_loaded",
                    format!("The model '{path}' was not drawn: {}", problem.message),
                    problem.hint.clone(),
                )
                .at_file(path),
            ),
        }
    }
    warnings.extend(wish_control::render::load_ui_pictures(
        renderer,
        scene,
        root,
        &mut std::collections::BTreeMap::new(),
    ));
    warnings
}
/// Runs `wen shaders`: checks every shader of the engine (with the files of
/// `dir` in place of the built-in ones) without a GPU.
pub fn shaders(dir: Option<&str>, as_json: bool) -> Result<Outcome> {
    let (library, replaced) = match dir {
        Some(dir) => wish_render::engine_shaders_from_dir(Path::new(dir))?,
        None => (wish_render::engine_shaders(), Vec::new()),
    };
    let problems = wish_render::check_shaders(&library);
    let success = problems.is_empty();
    let text = if as_json {
        json!({
            "ok": success,
            "checked": wish_render::ENTRY_SHADERS,
            "replaced": replaced,
            "problems": problems,
        })
        .to_string()
    } else {
        let mut text = format!(
            "Checked {} shader(s){}: {} problem(s).",
            wish_render::ENTRY_SHADERS.len(),
            if replaced.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} replaced from {})",
                    replaced.join(", "),
                    dir.unwrap_or(".")
                )
            },
            problems.len()
        );
        for problem in &problems {
            text.push_str(&format!("\n{problem}"));
        }
        text
    };
    Ok(Outcome { text, success })
}
