//! `wen view`: a window onto a scene file, kept in step with the files on disk.

use std::path::{Path, PathBuf};

use serde_json::json;
use wish_app::AppConfig;
use wish_core::{Entity, Result};
use wish_render::{Gpu, Renderer, View, extract};
use wish_window::{Content, Loaded, ViewerOptions, fingerprint_dir};

use crate::args::ViewArgs;
use crate::capture::{load_models, scene_id_of};
use crate::scene::{SceneApp, load_scene_app};

/// A scene file as the window's content: reloaded from disk whenever the
/// project's scenes, prefabs or models change.
struct SceneContent {
    example: String,
    scene: String,
    root: PathBuf,
    /// The last good load, kept to name entities.
    app: Option<SceneApp>,
}

impl Content for SceneContent {
    fn load(&mut self, gpu: &Gpu, renderer: &mut Renderer) -> Result<Loaded> {
        let mut loaded =
            load_scene_app(&self.example, &self.scene, &self.root, AppConfig::default())?;
        let world = loaded.app.world_mut();
        wish_transform::update_global_transforms(world)?;
        let scene = extract(world)?;
        let mut warnings = std::mem::take(&mut loaded.warnings);
        warnings.extend(load_models(&scene, &self.root, gpu, renderer));
        self.app = Some(loaded);
        Ok(Loaded { scene, warnings })
    }

    fn name_of(&mut self, entity: Entity) -> String {
        match self.app.as_mut() {
            Some(app) => scene_id_of(app.app.world_mut(), entity),
            None => entity.to_string(),
        }
    }

    fn fingerprint(&mut self) -> u64 {
        fingerprint_dir(&self.root, &["json", "glb", "gltf", "bin", "png", "jpg"])
    }
}

/// Runs `wen view`.
pub fn view(args: &ViewArgs, as_json: bool) -> Result<String> {
    let mut content = SceneContent {
        example: args.example.clone(),
        scene: args.scene.clone(),
        root: Path::new(args.root.as_deref().unwrap_or(".")).to_path_buf(),
        app: None,
    };
    let report = wish_window::run(
        ViewerOptions {
            title: format!("wen view {}", args.scene),
            size: args.size,
            software: args.software,
            shaders: args.shaders.as_ref().map(PathBuf::from),
            view: View::parse(&args.view)?,
            exit_after_frames: args.exit_after,
            on_message: Box::new(move |message| {
                if !as_json {
                    eprintln!("{message}");
                }
            }),
        },
        &mut content,
    )?;
    let picks: Vec<String> = report
        .picks
        .iter()
        .map(|entity| content.name_of(*entity))
        .collect();
    Ok(if as_json {
        json!({
            "ok": true,
            "frames": report.frames,
            "scene_reloads": report.scene_reloads,
            "shader_reloads": report.shader_reloads,
            "rejected": report.rejected,
            "picks": picks,
        })
        .to_string()
    } else {
        format!(
            "Closed after {} frame(s); {} scene reload(s), {} shader reload(s), {} rejected change(s).",
            report.frames, report.scene_reloads, report.shader_reloads, report.rejected
        )
    })
}
