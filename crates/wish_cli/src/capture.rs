//! `wen capture`: draw a scene into a PNG without a window.

use std::path::Path;

use serde_json::json;
use wish_app::AppConfig;
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{Gpu, GpuOptions, Renderer, extract};
use wish_scene::SceneId;

use crate::args::CaptureArgs;
use crate::scene::load_scene_app;

/// The views `wen capture` can draw today.
const VIEWS: &[&str] = &["lit"];

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

fn scene_id_of(world: &wish_ecs::World, entity: Entity) -> String {
    world
        .get::<SceneId>(entity)
        .and_then(|scene_id| world.text(scene_id.id))
        .unwrap_or_default()
        .to_owned()
}

/// Runs `wen capture`.
pub fn capture(args: &CaptureArgs, as_json: bool) -> Result<String> {
    if !VIEWS.contains(&args.view.as_str()) {
        return Err(Diagnostic::error(
            "view_not_available",
            format!("The view '{}' is not available yet.", args.view),
            "Only 'lit' exists today; 'ids', 'depth', 'normals' and 'wireframe' arrive with the debug views (M3 WP6).",
        ));
    }
    let root = Path::new(args.root.as_deref().unwrap_or("."));
    let mut loaded = load_scene_app(&args.example, &args.scene, root, AppConfig::default())?;
    let world = loaded.app.world_mut();
    wish_transform::update_global_transforms(world)?;
    let camera = match &args.camera {
        Some(id) => Some(entity_by_scene_id(world, id)?),
        None => None,
    };
    let scene = extract(world)?;
    let gpu = Gpu::shared(GpuOptions {
        software: args.software,
    })?;
    let renderer = Renderer::new(&gpu)?;
    let (width, height) = args.size;
    let frame = renderer.render(&gpu, &scene, camera, width, height)?;
    let png = frame.image.to_png()?;
    std::fs::write(&args.out, &png).map_err(|error| {
        Diagnostic::error(
            "file_unwritable",
            format!("Could not write '{}': {error}.", args.out),
            "Check that the folder exists and can be written to.",
        )
    })?;
    let camera_id = scene_id_of(world, frame.camera);
    let mut warnings = loaded.warnings;
    warnings.extend(frame.warnings);
    let adapter = gpu.adapter();
    if as_json {
        return Ok(json!({
            "ok": true,
            "path": args.out,
            "width": width,
            "height": height,
            "view": args.view,
            "camera": camera_id,
            "drawn": frame.drawn,
            "adapter": { "name": adapter.name, "backend": adapter.backend, "software": adapter.software },
            "warnings": warnings,
        })
        .to_string());
    }
    let mut text = format!(
        "Wrote {} ({width}x{height}, {} view) from camera '{camera_id}': {} draw(s), on {} ({}).",
        args.out, args.view, frame.drawn, adapter.name, adapter.backend
    );
    for warning in &warnings {
        text.push_str(&format!("\n{warning}"));
    }
    Ok(text)
}
