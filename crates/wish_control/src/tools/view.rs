//! Observation tools: `capture_frame` and `describe_view`.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Entity, Result};
use wish_render::{RenderScene, View, extract};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::render::{Drawing, base64, without_repeats};
use crate::sessions::entity_names;
use crate::tool::Tool;
use crate::workspace::Workspace;

const SCENE: Param = Param::optional(
    "scene",
    Kind::String,
    "The scene file to draw (default: the project's scene). Ignored when session is given.",
);
const SESSION: Param = Param::optional(
    "session",
    Kind::String,
    "Draw this play session's live world instead of the scene file.",
);
const CAMERA: Param = Param::optional(
    "camera",
    Kind::String,
    "The id of the camera entity to look through (default: the first camera).",
);
const SHADERS: Param = Param::optional(
    "shaders",
    Kind::String,
    "For engine development: a folder of <name>.wgsl files that replace the built-in shaders.",
);

pub(crate) const CAPTURE_FRAME: Tool = Tool {
    name: "capture_frame",
    title: "Take a picture",
    description: "Draws the scene (or a session's live world) from a camera and returns the picture as a PNG (image content over MCP; image.data is base64 on the command line unless out is given). The default 512x288 keeps it cheap; ask for more only to inspect detail. view picks what is drawn: lit (the real picture), ids (each entity its own color), depth (white near, black far), normals (surface direction as color), wireframe (triangle edges), colliders (the wireframe with every physics collider over it: fixed green, moving cyan, sensors yellow), lights (the wireframe with how far each point and spot light reaches). Use describe_view to ask what is visible instead of reading pixels.",
    params: &[
        SCENE,
        SESSION,
        CAMERA,
        Param::optional(
            "width",
            Kind::Integer { min: 16, max: 2048 },
            "Picture width in pixels.",
        )
        .or("512"),
        Param::optional(
            "height",
            Kind::Integer { min: 16, max: 2048 },
            "Picture height in pixels.",
        )
        .or("288"),
        Param::optional(
            "view",
            Kind::Choice(View::NAMES),
            "lit, ids, depth, normals, wireframe, colliders or lights.",
        )
        .or("\"lit\""),
        Param::optional(
            "out",
            Kind::String,
            "Also write the PNG to this project-relative .png file (then image.data is left out). An existing file is not replaced unless overwrite is true.",
        ),
        Param::optional(
            "overwrite",
            Kind::Boolean,
            "Replace the out file if it already exists.",
        ),
        SHADERS,
    ],
    mutates: false,
    run: capture_frame,
};

pub(crate) const DESCRIBE_VIEW: Tool = Tool {
    name: "describe_view",
    title: "What the camera sees",
    description: "Says what a camera sees without a picture: for each visible entity its pixel count, how much of it is not hidden by other things (visible_percent), its box on screen [left, top, right, bottom] and its distance in meters; and the entities that are drawn but not visible (hidden behind something, or outside the picture); the sprites (their box in the picture and layer) and particle emitters (the box their particles cover, how many are alive); and the in-game interface (interface: each box that shows something, its box in the picture, its text, whether it is a button and if the pointer is over it). Use it to check that what you built is on screen and not blocked, and what the interface says.",
    params: &[
        SCENE,
        SESSION,
        CAMERA,
        Param::optional(
            "width",
            Kind::Integer { min: 16, max: 2048 },
            "The size of the picture it measures in.",
        )
        .or("640"),
        Param::optional(
            "height",
            Kind::Integer { min: 16, max: 2048 },
            "The size of the picture it measures in.",
        )
        .or("360"),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 500 },
            "List at most this many visible entities (nearest first).",
        )
        .or("50"),
        SHADERS,
    ],
    mutates: false,
    run: describe_view,
};

/// What drawing needs from the world, taken while it is borrowed.
struct Prepared {
    scene: RenderScene,
    names: BTreeMap<Entity, String>,
    by_name: BTreeMap<String, Entity>,
    notes: Vec<Diagnostic>,
    source: Value,
}

fn prepare(workspace: &mut Workspace, args: &Args) -> Result<Prepared> {
    let session = match args.str("session") {
        Some(id) => Some(workspace.session_id(Some(id))?),
        None => None,
    };
    let (scene, by_name, notes, source) = match &session {
        Some(id) => {
            let session = workspace.session(id)?;
            let world = session.app.world();
            (
                extract(world)?,
                entity_names(world),
                Vec::new(),
                json!({ "session": id, "tick": session.app.tick() }),
            )
        }
        None => {
            let path = workspace.scene_path(args.str("scene"))?;
            let notes = workspace.open_document(&path, false)?;
            let revision = workspace.document(&path)?.doc.revision();
            let view = workspace.view(&path)?;
            (
                extract(&view.world)?,
                view.ids.clone(),
                notes,
                json!({ "scene": path, "revision": revision }),
            )
        }
    };
    let names = by_name
        .iter()
        .map(|(name, entity)| (*entity, name.clone()))
        .collect();
    Ok(Prepared {
        scene,
        names,
        by_name,
        notes,
        source,
    })
}

fn camera_entity(prepared: &Prepared, args: &Args) -> Result<Option<Entity>> {
    let Some(name) = args.str("camera") else {
        return Ok(None);
    };
    prepared.by_name.get(name).copied().map(Some).ok_or_else(|| {
        let cameras: Vec<&str> = prepared
            .scene
            .cameras
            .iter()
            .filter_map(|view| prepared.names.get(&view.entity).map(String::as_str))
            .collect();
        Diagnostic::error(
            "unknown_entity",
            format!("The scene has no entity '{name}'."),
            match wish_core::suggest::closest_match(name, cameras.iter().copied()) {
                Some(closest) => format!("Did you mean '{closest}'?"),
                None => "Name an entity that has a Camera component (entity_query with [\"Camera\"]).".to_owned(),
            },
        )
        .at_path("/camera")
        .with_candidates(cameras)
    })
}

/// Opens the renderer (or reuses it) and loads the scene's models.
fn drawing<'a>(
    workspace: &'a mut Workspace,
    args: &Args,
    scene: &RenderScene,
) -> Result<(&'a mut Drawing, Vec<Diagnostic>)> {
    let shaders = args.str("shaders").map(std::path::PathBuf::from);
    let shaders = shaders.map(|dir| {
        if dir.is_absolute() {
            dir
        } else {
            workspace.root().join(dir)
        }
    });
    // Shader files are read only from inside the project (M4 closing review).
    if let Some(dir) = &shaders {
        let inside = std::fs::canonicalize(dir)
            .ok()
            .zip(std::fs::canonicalize(workspace.root()).ok())
            .is_some_and(|(dir, root)| dir.starts_with(root));
        if !inside {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!(
                    "The shaders folder '{}' is not a folder inside the project.",
                    dir.display()
                ),
                "Give a project-relative folder such as \"shaders\" that holds <name>.wgsl files.",
            )
            .at_path("/shaders"));
        }
    }
    let software = workspace.software;
    let reopen = workspace
        .renderer
        .as_ref()
        .is_none_or(|open| !open.same_shaders(shaders.as_deref()));
    if reopen {
        workspace.renderer = Some(Drawing::open(software, shaders.as_deref())?);
    }
    let root = workspace.root().to_path_buf();
    let Some(open) = workspace.renderer.as_mut() else {
        return Err(Diagnostic::error(
            "internal_error",
            "The renderer did not open.",
            "This is a bug in the engine. Please report it.",
        ));
    };
    let warnings = open.load_models(scene, &root);
    Ok((open, warnings))
}

fn size(args: &Args, width: i64, height: i64) -> (u32, u32) {
    (
        u32::try_from(args.int("width").unwrap_or(width)).unwrap_or(512),
        u32::try_from(args.int("height").unwrap_or(height)).unwrap_or(288),
    )
}

fn adapter_json(open: &Drawing) -> Value {
    let adapter = open.gpu.adapter();
    json!({ "name": adapter.name, "backend": adapter.backend, "software": adapter.software })
}

fn capture_frame(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let view = View::parse(args.str("view").unwrap_or("lit"))?;
    let (width, height) = size(args, 512, 288);
    let out_path = match args.str("out") {
        Some(path) => Some(crate::documents::normalize_output_path(path)?),
        None => None,
    };
    let prepared = prepare(workspace, args)?;
    let camera = camera_entity(&prepared, args)?;
    let (open, model_warnings) = drawing(workspace, args, &prepared.scene)?;
    let frame =
        open.renderer
            .render_view(&open.gpu, &prepared.scene, camera, width, height, view)?;
    let png = frame.image.to_png()?;
    let adapter = adapter_json(open);
    let mut warnings = prepared.notes;
    warnings.extend(model_warnings);
    warnings.extend(frame.warnings);
    let warnings = without_repeats(warnings);

    let mut image =
        json!({ "mime": "image/png", "width": width, "height": height, "bytes": png.len() });
    match &out_path {
        Some(path) => {
            let target = crate::documents::prepare_output(workspace.root(), path)?;
            if target.exists() && !args.flag("overwrite") {
                return Err(Diagnostic::error(
                    "output_exists",
                    format!("'{path}' already exists; it was not replaced."),
                    "Choose a new name, or pass \"overwrite\": true to replace it (for example a capture you took before).",
                )
                .at_path("/out"));
            }
            std::fs::write(&target, &png).map_err(|error| {
                Diagnostic::error(
                    "file_unwritable",
                    format!("Could not write '{path}': {error}."),
                    "Check that the folder can be written to.",
                )
                .at_path("/out")
            })?;
            image["path"] = json!(path);
        }
        None => image["data"] = json!(base64(&png)),
    }
    let mut out = Map::new();
    if let Value::Object(source) = prepared.source {
        out.extend(source);
    }
    out.insert("view".to_owned(), json!(view.name()));
    out.insert(
        "camera".to_owned(),
        json!(
            prepared
                .names
                .get(&frame.camera)
                .cloned()
                .unwrap_or_else(|| format!("#{}", frame.camera))
        ),
    );
    out.insert("drawn".to_owned(), json!(frame.drawn));
    out.insert("image".to_owned(), image);
    out.insert("adapter".to_owned(), adapter);
    if !warnings.is_empty() {
        out.insert("diagnostics".to_owned(), json!(warnings));
    }
    Ok(out)
}

fn describe_view(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let (width, height) = size(args, 640, 360);
    let limit = usize::try_from(args.int("limit").unwrap_or(50)).unwrap_or(50);
    let prepared = prepare(workspace, args)?;
    let camera = camera_entity(&prepared, args)?;
    let (open, model_warnings) = drawing(workspace, args, &prepared.scene)?;
    let description = open
        .renderer
        .describe(&open.gpu, &prepared.scene, camera, width, height)?;
    let overlay = open
        .renderer
        .overlay_elements(&prepared.scene, camera, width, height)?;
    let adapter = adapter_json(open);
    let name = |entity: Entity| {
        prepared
            .names
            .get(&entity)
            .cloned()
            .unwrap_or_else(|| format!("#{entity}"))
    };
    let round = |v: f32| (f64::from(v) * 1000.0).round() / 1000.0;
    let mut visible: Vec<_> = description.visible.iter().collect();
    visible.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let total = visible.len();
    let listed: Vec<Value> = visible
        .into_iter()
        .take(limit)
        .map(|entry| {
            json!({
                "entity": name(entry.entity),
                "pixels": entry.pixels,
                "visible_percent": entry.visible_fraction.map(|f| (f * 1000.0).round() / 10.0),
                "bounding_box": entry.bounding_box,
                "distance": round(entry.distance),
            })
        })
        .collect();
    let hidden: Vec<String> = description
        .hidden
        .iter()
        .map(|entity| name(*entity))
        .collect();
    // The interface, over the picture: what each element shows and where.
    let interface: Vec<Value> = wish_render::ui_elements(&prepared.scene, width, height)
        .into_iter()
        .take(limit)
        .map(|element| {
            let mut entry = json!({
                "entity": name(element.entity),
                "bounding_box": element.bounding_box,
            });
            if let Some(text) = element.text {
                entry["text"] = json!(text);
            }
            if element.image {
                entry["image"] = json!(true);
            }
            if let Some((hovered, pressed)) = element.button {
                entry["button"] = json!({ "hovered": hovered, "pressed": pressed });
            }
            entry
        })
        .collect();
    let mut warnings = prepared.notes;
    warnings.extend(model_warnings);
    warnings.extend(description.warnings.iter().cloned());
    let warnings = without_repeats(warnings);
    let mut out = Map::new();
    if let Value::Object(source) = prepared.source {
        out.extend(source);
    }
    out.insert("camera".to_owned(), json!(name(description.camera)));
    out.insert("width".to_owned(), json!(width));
    out.insert("height".to_owned(), json!(height));
    out.insert("visible_count".to_owned(), json!(total));
    out.insert("visible".to_owned(), json!(listed));
    if total > limit {
        out.insert("visible_not_listed".to_owned(), json!(total - limit));
    }
    out.insert("hidden".to_owned(), json!(hidden));
    // Sprites and particle emitters are not in the depth picture: their
    // boxes are worked out from where they stand.
    let mut sprites = Vec::new();
    let mut particles = Vec::new();
    for element in overlay.into_iter().take(limit) {
        let mut entry = json!({ "entity": name(element.entity) });
        match element.bounding_box {
            Some(found) => entry["bounding_box"] = json!(found),
            None => entry["on_screen"] = json!(false),
        }
        if let Some(layer) = element.layer {
            entry["layer"] = json!(layer);
            sprites.push(entry);
        } else {
            entry["alive"] = json!(element.particles.unwrap_or(0));
            particles.push(entry);
        }
    }
    if !sprites.is_empty() {
        out.insert("sprites".to_owned(), json!(sprites));
    }
    if !particles.is_empty() {
        out.insert("particles".to_owned(), json!(particles));
    }
    if !interface.is_empty() {
        out.insert("interface".to_owned(), json!(interface));
    }
    out.insert("adapter".to_owned(), adapter);
    if !warnings.is_empty() {
        out.insert("diagnostics".to_owned(), json!(warnings));
    }
    Ok(out)
}
