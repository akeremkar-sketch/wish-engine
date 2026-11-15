//! `wen capture` and `wen describe`: draw a scene without a window, as a PNG
//! or as a list of what is on screen.

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use wish_app::AppConfig;
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer, View, extract};
use wish_scene::SceneId;

use crate::args::{CaptureArgs, DescribeArgs};
use crate::project_cmds::Outcome;
use crate::scene::{SceneApp, load_scene_app};

/// The entity whose scene id is `id`.
fn entity_by_scene_id(world: &mut wish_ecs::World, id: &str) -> Result<Entity> {
    let mut found = None;
    let mut ids = Vec::new();
    for (entity, scene_id) in world.query::<(Entity, &SceneId)>()? {
        ids.push((entity, scene_id.id));
    }
    let mut names = Vec::new();
    for (entity, text) in ids {
        let name = world.text(text).unwrap_or_default().to_owned();
        if name == id {
            found = Some(entity);
        }
        names.push(name);
    }
    found.ok_or_else(|| {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        Diagnostic::error(
            "unknown_entity",
            format!("The scene has no entity '{id}'."),
            wish_core::suggest::unknown_name_hint(id, &names),
        )
    })
}

/// The name an entity has in the scene file, or its numeric form when it was
/// not made by the scene.
fn scene_id_of(world: &wish_ecs::World, entity: Entity) -> String {
    world
        .get::<SceneId>(entity)
        .and_then(|scene_id| world.text(scene_id.id))
        .map_or_else(|| entity.to_string(), str::to_owned)
}

/// Loads every model the scene's Model entities name from the project and
/// uploads it. A model that cannot be loaded is reported as a warning and not
/// drawn (the rest of the scene still is); its import warnings are passed on.
fn load_models(
    scene: &RenderScene,
    root: &Path,
    gpu: &Gpu,
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
    warnings
}

/// What both commands need: the scene loaded, extracted and ready to draw.
struct Prepared {
    loaded: SceneApp,
    scene: RenderScene,
    camera: Option<Entity>,
    gpu: Arc<Gpu>,
    renderer: Renderer,
    /// Warnings from loading the files and the models.
    warnings: Vec<Diagnostic>,
}

/// What to load and from which camera.
struct Source<'a> {
    example: &'a str,
    scene: &'a str,
    root: Option<&'a str>,
    camera: Option<&'a str>,
    software: bool,
    shaders: Option<&'a str>,
}

fn prepare(source: &Source<'_>) -> Result<Prepared> {
    let root = Path::new(source.root.unwrap_or("."));
    let mut loaded = load_scene_app(source.example, source.scene, root, AppConfig::default())?;
    let world = loaded.app.world_mut();
    wish_transform::update_global_transforms(world)?;
    let camera = match source.camera {
        Some(id) => Some(entity_by_scene_id(world, id)?),
        None => None,
    };
    let scene = extract(world)?;
    let gpu = Gpu::shared(GpuOptions {
        software: source.software,
    })?;
    let library = match source.shaders {
        Some(dir) => wish_render::engine_shaders_from_dir(Path::new(dir))?.0,
        None => wish_render::engine_shaders(),
    };
    let mut renderer = Renderer::with_shaders(&gpu, &library)?;
    let mut warnings = std::mem::take(&mut loaded.warnings);
    warnings.extend(load_models(&scene, root, &gpu, &mut renderer));
    Ok(Prepared {
        loaded,
        scene,
        camera,
        gpu,
        renderer,
        warnings,
    })
}

/// Drops repeats: the renderer says, per entity, what loading said once per
/// file.
fn without_repeats(mut warnings: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen: Vec<(String, String)> = Vec::new();
    warnings.retain(|warning| match warning.location.file.as_ref() {
        Some(file) => {
            let key = (warning.code.to_string(), file.clone());
            let fresh = !seen.contains(&key);
            seen.push(key);
            fresh
        }
        None => true,
    });
    warnings
}

fn adapter_json(gpu: &Gpu) -> serde_json::Value {
    let adapter = gpu.adapter();
    json!({ "name": adapter.name, "backend": adapter.backend, "software": adapter.software })
}

/// Runs `wen capture`.
pub fn capture(args: &CaptureArgs, as_json: bool) -> Result<String> {
    let view = View::parse(&args.view)?;
    let mut prepared = prepare(&Source {
        example: &args.example,
        scene: &args.scene,
        root: args.root.as_deref(),
        camera: args.camera.as_deref(),
        software: args.software,
        shaders: args.shaders.as_deref(),
    })?;
    let (width, height) = args.size;
    let frame = prepared.renderer.render_view(
        &prepared.gpu,
        &prepared.scene,
        prepared.camera,
        width,
        height,
        view,
    )?;
    let png = frame.image.to_png()?;
    std::fs::write(&args.out, &png).map_err(|error| {
        Diagnostic::error(
            "file_unwritable",
            format!("Could not write '{}': {error}.", args.out),
            "Check that the folder exists and can be written to.",
        )
    })?;
    let world = prepared.loaded.app.world_mut();
    let camera_id = scene_id_of(world, frame.camera);
    let mut warnings = prepared.warnings;
    warnings.extend(frame.warnings);
    let warnings = without_repeats(warnings);
    let adapter = prepared.gpu.adapter();
    if as_json {
        return Ok(json!({
            "ok": true,
            "path": args.out,
            "width": width,
            "height": height,
            "view": view.name(),
            "camera": camera_id,
            "drawn": frame.drawn,
            "adapter": adapter_json(&prepared.gpu),
            "warnings": warnings,
        })
        .to_string());
    }
    let mut text = format!(
        "Wrote {} ({width}x{height}, {} view) from camera '{camera_id}': {} draw(s), on {} ({}).",
        args.out,
        view.name(),
        frame.drawn,
        adapter.name,
        adapter.backend
    );
    for warning in &warnings {
        text.push_str(&format!("\n{warning}"));
    }
    Ok(text)
}

/// Runs `wen describe`.
pub fn describe(args: &DescribeArgs, as_json: bool) -> Result<String> {
    let mut prepared = prepare(&Source {
        example: &args.example,
        scene: &args.scene,
        root: args.root.as_deref(),
        camera: args.camera.as_deref(),
        software: args.software,
        shaders: args.shaders.as_deref(),
    })?;
    let (width, height) = args.size;
    let description = prepared.renderer.describe(
        &prepared.gpu,
        &prepared.scene,
        prepared.camera,
        width,
        height,
    )?;
    let world = prepared.loaded.app.world_mut();
    let camera_id = scene_id_of(world, description.camera);
    let mut warnings = prepared.warnings;
    warnings.extend(description.warnings.iter().cloned());
    let warnings = without_repeats(warnings);

    let percent = |fraction: Option<f32>| fraction.map(|f| (f * 1000.0).round() / 10.0);
    let visible: Vec<_> = description
        .visible
        .iter()
        .map(|entry| {
            (
                scene_id_of(world, entry.entity),
                entry,
                percent(entry.visible_fraction),
            )
        })
        .collect();
    let hidden: Vec<String> = description
        .hidden
        .iter()
        .map(|entity| scene_id_of(world, *entity))
        .collect();

    if as_json {
        return Ok(json!({
            "ok": true,
            "camera": camera_id,
            "width": width,
            "height": height,
            "visible": visible.iter().map(|(id, entry, percent)| json!({
                "entity": id,
                "pixels": entry.pixels,
                "visible_percent": percent,
                "bounding_box": entry.bounding_box,
                "distance": (entry.distance * 1000.0).round() / 1000.0,
            })).collect::<Vec<_>>(),
            "hidden": hidden,
            "adapter": adapter_json(&prepared.gpu),
            "warnings": warnings,
        })
        .to_string());
    }
    let mut text = format!(
        "From camera '{camera_id}' ({width}x{height}): {} visible, {} hidden.",
        visible.len(),
        hidden.len()
    );
    for (id, entry, percent) in &visible {
        let [left, top, right, bottom] = entry.bounding_box;
        let shown = percent.map_or_else(|| "not measured".to_owned(), |p| format!("{p}% visible"));
        text.push_str(&format!(
            "\n  {id}: {} px, {shown}, box ({left},{top})-({right},{bottom}), {:.2} m away",
            entry.pixels, entry.distance
        ));
    }
    if !hidden.is_empty() {
        text.push_str(&format!(
            "\n  not visible (hidden or outside the picture): {}",
            hidden.join(", ")
        ));
    }
    for warning in &warnings {
        text.push_str(&format!("\n{warning}"));
    }
    Ok(text)
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
