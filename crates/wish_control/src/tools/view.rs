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
    description: "Draws the scene (or a session's live world) from a camera and returns the picture as a PNG (image content over MCP; image.data is base64 on the command line unless out is given). The default 512x288 keeps it cheap; ask for more only to inspect detail. view picks what is drawn: lit (the real picture), ids (each entity its own color), depth (white near, black far), normals (surface direction as color), wireframe (triangle edges), colliders (the wireframe with every physics collider over it: fixed green, moving cyan, sensors yellow), lights (the wireframe with how far each point and spot light reaches), shadow_cascades (each surface in the color of the shadow map its shadow is read from: red, green, blue, yellow from the cascade nearest the camera, mixed where two blend; white with one shadow map; gray where there is no shadow, as beyond RenderSettings.shadow_distance). stats says what drawing the picture took, in counts that are the same on every machine: draw_calls, triangles, shadow_casters (draws into the shadow maps), shadow_cascades (how many shadow maps: 0 none, 1 the one map around everything, 2 to 4 cascades, set with RenderSettings.shadow_cascades, 4 by default; the shadow_cascades view answers the number of the lit picture whose maps it shows, every other debug view 0), point_lights, spot_lights, blended_surfaces (see-through surfaces: a Material with base_color alpha below 1, or a material file or glTF material with alpha_mode Blend; drawn back to front over the rest, casting no shadow; the debug views draw them as any surface), and effects, the optional look effects the picture used (sky_light, sun_disc, fog, bloom, color_grading, fxaa; set with the Ambient, Sky, Fog, Bloom, ColorGrading and RenderSettings resources; a scene has all of them by default, the look natural, and edit_apply apply_look plain turns them all off). Use describe_view to ask what is visible instead of reading pixels.",
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
            "lit, ids, depth, normals, wireframe, colliders, lights or shadow_cascades.",
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
        Param::optional(
            "checkpoint",
            Kind::String,
            "Draw the scene as this checkpoint kept it (a \"before\" picture to compare with now), with the prefabs and material files it kept; textures are drawn as they are now (checkpoints keep no pictures).",
        ),
        SHADERS,
    ],
    mutates: false,
    run: capture_frame,
};

pub(crate) const DESCRIBE_VIEW: Tool = Tool {
    name: "describe_view",
    title: "What the camera sees",
    description: "Says what a camera sees without a picture. For each visible entity: its pixel count, how much of it is not hidden by other things (visible_percent), its box on screen [left, top, right, bottom] and its distance in meters; and the entities that are drawn but not visible (hidden behind something, or outside the picture); the sprites (their box in the picture and layer; a sprite that shows a sprite sheet also says its clip, frame (its place in the clip) and sheet_frame) and particle emitters (the box their particles cover, how many are alive); and the in-game interface (interface: each box that shows something, its box in the picture, its text, whether it is a button and if the pointer is over it); and stats, what drawing the lit picture of this view takes (draw_calls, triangles, shadow_casters, shadow_cascades, point_lights, spot_lights: counts, the same on every machine; effects: which of sky_light, sun_disc, fog, bloom, color_grading and fxaa it uses). A see-through entity is marked blended: true, and what shows through it counts as visible too; stats.blended_surfaces counts them. Use it to check that what you built is on screen and not blocked, what the interface says, and what makes a scene heavy. Tilemaps are listed under tilemaps: file, layer, box in the picture, the cells on screen [x0, y0, x1, y1] and how many tiles show; with point: [x, y] (a pixel of the picture), also the cell under it and each layer's tile there.",
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
        Param::optional(
            "point",
            Kind::Array,
            "A pixel [x, y] of the picture (from its top left): each tilemap says which cell and which tile of each layer is under it.",
        ),
        SHADERS,
    ],
    mutates: false,
    run: describe_view,
};

/// How many bytes of render targets the renderer may keep after a tool
/// call. The picture of a tool call is drawn now and then, not every frame:
/// what the usual sizes take stays for the next call (the four shadow maps
/// of the default look are 64 MiB by themselves; with a 1280x720 lit
/// picture and the targets of its bloom and FXAA that is about 90 MiB), a
/// larger capture does not sit in memory until some later call. The
/// editor's views draw through the same renderer many times a second and
/// keep their targets (`Workspace::render_picture`).
const KEPT_AFTER_A_TOOL_CALL: u64 = 128 << 20;

/// What drawing needs from the world, taken while it is borrowed.
struct Prepared {
    scene: RenderScene,
    names: BTreeMap<Entity, String>,
    by_name: BTreeMap<String, Entity>,
    notes: Vec<Diagnostic>,
    source: Value,
    /// For a picture of a checkpoint: its copy of the project's files,
    /// whose material files the picture uses.
    kept: Option<std::path::PathBuf>,
}

impl Prepared {
    /// Puts the parts together and tells the scene what its entities are
    /// called, so that what the renderer warns about is named the way the
    /// scene file names it. Entities without a scene id (made while a
    /// session runs) keep their `#<id>` form.
    fn new(
        mut scene: RenderScene,
        by_name: BTreeMap<String, Entity>,
        notes: Vec<Diagnostic>,
        source: Value,
    ) -> Self {
        let names: BTreeMap<Entity, String> = by_name
            .iter()
            .map(|(name, entity)| (*entity, name.clone()))
            .collect();
        scene.names = names
            .iter()
            .filter(|(_, name)| !name.starts_with('#'))
            .map(|(entity, name)| (*entity, name.clone()))
            .collect();
        Self {
            scene,
            names,
            by_name,
            notes,
            source,
            kept: None,
        }
    }

    /// Gives the renderer's warnings about the scene's lights a place in
    /// the scene file where there is one: the first light that is not
    /// drawn, when the picture is of the scene file as it is now (not a
    /// checkpoint's copy, not a preview) and the light is an entity of
    /// that file (not one a prefab brought in); the sky's sun size, when a
    /// disc is asked for and there is no light to give it a place; the
    /// ambient colour or intensity the sky light does not use; and the
    /// shadow distance, when cascades are asked for and it reaches nothing.
    /// The place names the file, because a path without a file points
    /// into the call's arguments.
    fn locate(&self, mut warnings: Vec<Diagnostic>) -> Vec<Diagnostic> {
        let file = self
            .source
            .get("scene")
            .and_then(Value::as_str)
            .filter(|_| {
                self.source.get("checkpoint").is_none() && self.source.get("preview").is_none()
            });
        let extra = self
            .scene
            .lights
            .get(1)
            .and_then(|light| self.scene.names.get(&light.entity))
            .filter(|name| !name.contains('/'));
        if let (Some(file), Some(name)) = (file, extra) {
            for warning in &mut warnings {
                if warning.code == "extra_directional_lights" && warning.location.path.is_none() {
                    warning.location.file = Some(file.to_owned());
                    warning.location.path =
                        Some(format!("/entities/{name}/components/DirectionalLight"));
                }
            }
        }
        // Only a scene file's own `Sky` resource can ask for a disc there,
        // and only its own `RenderSettings` for cascades: the field to
        // change is the distance (the field the hint names first). For an
        // ambient colour or intensity the sky light does not use, it is the
        // field the warning names.
        if let Some(file) = file {
            for warning in &mut warnings {
                let field = match &*warning.code {
                    "sun_disc_without_light" => "/resources/Sky/sun_size_deg".to_owned(),
                    "shadows_out_of_range" => {
                        "/resources/RenderSettings/shadow_distance".to_owned()
                    }
                    "ambient_color_unused" => match self.scene.ambient_unused {
                        Some(field) => format!("/resources/Ambient/{field}"),
                        None => continue,
                    },
                    _ => continue,
                };
                if warning.location.path.is_none() {
                    warning.location.file = Some(file.to_owned());
                    warning.location.path = Some(field);
                }
            }
        }
        warnings
    }
}

fn prepare(workspace: &mut Workspace, args: &Args) -> Result<Prepared> {
    if let Some(name) = args.str("checkpoint") {
        return prepare_checkpoint(workspace, name, args.str("scene"));
    }
    prepare_source(workspace, args.str("session"), args.str("scene"))
}

/// A scene as checkpoint `name` kept it (for "before" pictures).
fn prepare_checkpoint(
    workspace: &mut Workspace,
    name: &str,
    scene: Option<&str>,
) -> Result<Prepared> {
    let path = workspace.scene_path(scene)?;
    let doc = workspace.checkpoint_document(name, &path)?;
    let view = workspace.build_view(&doc)?;
    let mut prepared_scene = extract(&view.world)?;
    if path.ends_with(".prefab.json") {
        stage(&mut prepared_scene);
    }
    let mut prepared = Prepared::new(
        prepared_scene,
        view.ids,
        Vec::new(),
        json!({ "scene": path, "checkpoint": name }),
    );
    prepared.kept = Some(workspace.checkpoint_files(name));
    Ok(prepared)
}

/// [`prepare`] without the tool's arguments: a session's live world, or a
/// scene file's (the project's by default).
fn prepare_source(
    workspace: &mut Workspace,
    session: Option<&str>,
    scene: Option<&str>,
) -> Result<Prepared> {
    prepare_with(workspace, session, scene, None)
}

/// [`prepare_source`], optionally with `preview` edit operations applied on a
/// copy of the scene (what it would look like after them).
fn prepare_with(
    workspace: &mut Workspace,
    session: Option<&str>,
    scene: Option<&str>,
    preview: Option<&Value>,
) -> Result<Prepared> {
    let session = match session {
        Some(id) => Some(workspace.session_id(Some(id))?),
        None => None,
    };
    let (mut scene, by_name, notes, source) = match &session {
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
            let path = workspace.scene_path(scene)?;
            let notes = workspace.open_document(&path, false)?;
            let revision = workspace.document(&path)?.doc.revision();
            if let Some(ops) = preview {
                let view = workspace.build_preview(&path, ops)?;
                (
                    extract(&view.world)?,
                    view.ids,
                    notes,
                    json!({ "scene": path, "revision": revision, "preview": true }),
                )
            } else {
                let view = workspace.view(&path)?;
                (
                    extract(&view.world)?,
                    view.ids.clone(),
                    notes,
                    json!({ "scene": path, "revision": revision }),
                )
            }
        }
    };
    // A prefab has no camera or light of its own: it is looked at from a
    // default stage (from the front and above, in daylight).
    if source["scene"]
        .as_str()
        .is_some_and(|path| path.ends_with(".prefab.json"))
    {
        stage(&mut scene);
    }
    Ok(Prepared::new(scene, by_name, notes, source))
}

/// Gives a scene with no camera one, and one with no light a sun: what a
/// prefab needs to be looked at.
fn stage(scene: &mut RenderScene) {
    use wish_core::math::Vec3;
    if scene.cameras.is_empty() {
        let eye = Vec3::new(0.0, 2.5, 6.5);
        scene.cameras.push(wish_render::CameraView {
            entity: Entity::NULL,
            view: wish_core::math::camera::rh::view::look_at_mat4(
                eye,
                Vec3::new(0.0, 0.8, 0.0),
                Vec3::Y,
            ),
            position: eye,
            fov_y: 50f32.to_radians(),
            near: 0.1,
            far: 200.0,
            ortho_height: None,
        });
    }
    if scene.lights.is_empty() {
        scene.lights.push(wish_render::LightView {
            entity: Entity::NULL,
            direction: Vec3::new(-0.4, -1.0, -0.3).normalize(),
            radiance: [100_000.0, 98_000.0, 92_000.0],
            shadows: true,
        });
    }
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
    kept: Option<&std::path::Path>,
) -> Result<(&'a mut Drawing, Vec<Diagnostic>)> {
    drawing_with(workspace, args.str("shaders"), scene, kept)
}

fn drawing_with<'a>(
    workspace: &'a mut Workspace,
    shaders: Option<&str>,
    scene: &RenderScene,
    kept: Option<&std::path::Path>,
) -> Result<(&'a mut Drawing, Vec<Diagnostic>)> {
    let shaders = shaders.map(std::path::PathBuf::from);
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
    let warnings = open.load_models(scene, &root, kept);
    Ok((open, warnings))
}

fn size(args: &Args, width: i64, height: i64) -> (u32, u32) {
    (
        u32::try_from(args.int("width").unwrap_or(width)).unwrap_or(512),
        u32::try_from(args.int("height").unwrap_or(height)).unwrap_or(288),
    )
}

/// The statistics of a picture as the tools give them: counts only, the same
/// on every machine and in every run (no time is part of these answers:
/// time is measured by `wen bench --render`, on the command line).
fn stats_json(stats: &wish_render::RenderStats) -> Value {
    json!({
        "width": stats.width,
        "height": stats.height,
        "draw_calls": stats.draw_calls,
        "triangles": stats.triangles,
        "shadow_casters": stats.shadow_casters,
        "shadow_cascades": stats.shadow_cascades,
        "point_lights": stats.point_lights,
        "spot_lights": stats.spot_lights,
        "blended_surfaces": stats.blended_surfaces,
        "effects": stats.effects.names(),
    })
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
    let (open, model_warnings) =
        drawing(workspace, args, &prepared.scene, prepared.kept.as_deref())?;
    let frame =
        open.renderer
            .render_view(&open.gpu, &prepared.scene, camera, width, height, view)?;
    open.renderer.trim(KEPT_AFTER_A_TOOL_CALL);
    let png = frame.image.to_png()?;
    let adapter = adapter_json(open);
    let mut warnings = prepared.notes.clone();
    warnings.extend(model_warnings);
    warnings.extend(frame.warnings);
    let warnings = without_repeats(prepared.locate(warnings));

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
    out.insert("stats".to_owned(), stats_json(&frame.stats));
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
    let (open, model_warnings) =
        drawing(workspace, args, &prepared.scene, prepared.kept.as_deref())?;
    let description = open
        .renderer
        .describe(&open.gpu, &prepared.scene, camera, width, height)?;
    let overlay = open
        .renderer
        .overlay_elements(&prepared.scene, camera, width, height)?;
    let tilemaps = open
        .renderer
        .tilemap_elements(&prepared.scene, camera, width, height)?;
    let point = match args.value("point") {
        None => None,
        Some(value) => Some(
            value
                .as_array()
                .filter(|pair| pair.len() == 2)
                .and_then(|pair| Some([pair[0].as_f64()? as f32, pair[1].as_f64()? as f32]))
                .filter(|[x, y]| x.is_finite() && y.is_finite())
                .ok_or_else(|| {
                    Diagnostic::error(
                        "invalid_parameter",
                        "\"point\" must be a pixel [x, y] of the picture.",
                        "Write it like [320, 180]: from the picture's top left corner.",
                    )
                    .at_path("/point")
                })?,
        ),
    };
    let under = match point {
        Some(point) => {
            open.renderer
                .tiles_under(&prepared.scene, camera, (width, height), point)?
        }
        None => Vec::new(),
    };
    open.renderer.trim(KEPT_AFTER_A_TOOL_CALL);
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
            let mut listed = json!({
                "entity": name(entry.entity),
                "pixels": entry.pixels,
                "visible_percent": entry.visible_fraction.map(|f| (f * 1000.0).round() / 10.0),
                "bounding_box": entry.bounding_box,
                "distance": round(entry.distance),
            });
            // Only see-through entities say so: other answers are as they
            // were.
            if entry.blended {
                listed["blended"] = json!(true);
            }
            listed
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
    let mut warnings = prepared.notes.clone();
    warnings.extend(model_warnings);
    warnings.extend(description.warnings.iter().cloned());
    let warnings = without_repeats(prepared.locate(warnings));
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
            // A sprite that shows a sheet: which clip and which frame.
            if let Some((clip, frame, sheet_frame)) = element.animation {
                entry["clip"] = json!(clip);
                entry["frame"] = json!(frame);
                entry["sheet_frame"] = json!(sheet_frame);
            }
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
    let maps: Vec<Value> = tilemaps
        .into_iter()
        .take(limit)
        .map(|element| {
            let mut entry = json!({
                "entity": name(element.entity),
                "map": element.map,
                "layer": element.layer,
                "tiles": element.tiles,
            });
            match element.bounding_box {
                Some(found) => entry["bounding_box"] = json!(found),
                None => entry["on_screen"] = json!(false),
            }
            if let Some(cells) = element.cells {
                entry["cells"] = json!(cells);
            }
            if !element.loaded {
                entry["loaded"] = json!(false);
            }
            if let Some(found) = under.iter().find(|under| under.entity == element.entity) {
                let layers: Map<String, Value> = found
                    .layers
                    .iter()
                    .map(|(layer, tile)| (layer.clone(), json!(tile)))
                    .collect();
                entry["under_point"] = json!({ "cell": found.cell, "tiles": layers });
            }
            entry
        })
        .collect();
    if !maps.is_empty() {
        out.insert("tilemaps".to_owned(), json!(maps));
    }
    if !interface.is_empty() {
        out.insert("interface".to_owned(), json!(interface));
    }
    out.insert("stats".to_owned(), stats_json(&description.stats));
    out.insert("adapter".to_owned(), adapter);
    if !warnings.is_empty() {
        out.insert("diagnostics".to_owned(), json!(warnings));
    }
    Ok(out)
}

/// Where a picture comes from.
#[derive(Clone, Copy, Debug)]
pub enum PictureSource<'a> {
    /// A scene file (`None`: the project's scene).
    Scene(Option<&'a str>),
    /// A scene file with these edit operations applied on a copy: what it
    /// would look like after them (the file and the history are untouched).
    /// For showing a change while it is still being made.
    Preview(Option<&'a str>, &'a Value),
    /// A play session's live world.
    Session(&'a str),
    /// A scene file as a checkpoint kept it (`None`: the project's scene):
    /// the "before" of a before/after comparison.
    Checkpoint(Option<&'a str>, &'a str),
}

/// A picture of a scene or session, as the editor's view shows it.
pub struct Picture {
    /// The pixels (sRGB, 8-bit RGBA).
    pub image: wish_render::RgbaImage,
    /// Entity names of what could be drawn, by entity.
    pub names: BTreeMap<Entity, String>,
    /// What is worth telling the person looking (a model that did not load).
    pub warnings: Vec<Diagnostic>,
}

impl Workspace {
    /// Draws the scene or session the way `capture_frame` does, but takes
    /// the camera as a function of the scene's own first camera (a fly
    /// camera moves it) and returns the picture itself instead of a PNG:
    /// for the editor's view, which draws many pictures a second.
    ///
    /// `camera` is applied to the scene's first camera (a scene without one
    /// is an error, as for `capture_frame`).
    pub fn render_picture(
        &mut self,
        source: PictureSource<'_>,
        camera: Option<&dyn Fn(&wish_render::CameraView) -> wish_render::CameraView>,
        width: u32,
        height: u32,
        view: View,
    ) -> Result<Picture> {
        let mut prepared = match source {
            PictureSource::Scene(scene) => prepare_with(self, None, scene, None)?,
            PictureSource::Preview(scene, ops) => prepare_with(self, None, scene, Some(ops))?,
            PictureSource::Session(id) => prepare_with(self, Some(id), None, None)?,
            PictureSource::Checkpoint(scene, name) => prepare_checkpoint(self, name, scene)?,
        };
        if let (Some(camera), Some(first)) = (camera, prepared.scene.cameras.first_mut()) {
            *first = camera(first);
        }
        let (open, model_warnings) =
            drawing_with(self, None, &prepared.scene, prepared.kept.as_deref())?;
        let frame =
            open.renderer
                .render_view(&open.gpu, &prepared.scene, None, width, height, view)?;
        let mut warnings = prepared.notes.clone();
        warnings.extend(model_warnings);
        warnings.extend(frame.warnings);
        let warnings = without_repeats(prepared.locate(warnings));
        Ok(Picture {
            image: frame.image,
            names: prepared.names,
            warnings,
        })
    }
}

impl Workspace {
    /// How many bytes of render targets this workspace's renderer keeps for
    /// its next picture (0 before the first one): for tests of what a tool
    /// call leaves behind in memory.
    #[doc(hidden)]
    pub fn kept_render_bytes(&self) -> u64 {
        self.renderer
            .as_ref()
            .map_or(0, |open| open.renderer.kept_bytes())
    }
}

/// Which entity is drawn at each pixel of a picture, with the entities' names.
pub struct IdPicture {
    /// The identity buffer: [`wish_render::IdBuffer::entity_at`] says who is at a pixel.
    pub ids: wish_render::IdBuffer,
    /// Entity names, by entity.
    pub names: BTreeMap<Entity, String>,
}

impl Workspace {
    /// The identity buffer of the scene or session from the same camera
    /// [`Workspace::render_picture`] would use: "which entity is at this
    /// pixel", for clicking in the editor's view.
    pub fn render_ids(
        &mut self,
        source: PictureSource<'_>,
        camera: Option<&dyn Fn(&wish_render::CameraView) -> wish_render::CameraView>,
        width: u32,
        height: u32,
    ) -> Result<IdPicture> {
        let mut prepared = match source {
            PictureSource::Scene(scene) => prepare_with(self, None, scene, None)?,
            PictureSource::Preview(scene, ops) => prepare_with(self, None, scene, Some(ops))?,
            PictureSource::Session(id) => prepare_with(self, Some(id), None, None)?,
            PictureSource::Checkpoint(scene, name) => prepare_checkpoint(self, name, scene)?,
        };
        if let (Some(camera), Some(first)) = (camera, prepared.scene.cameras.first_mut()) {
            *first = camera(first);
        }
        let (open, _) = drawing_with(self, None, &prepared.scene, prepared.kept.as_deref())?;
        let ids = open
            .renderer
            .id_buffer(&open.gpu, &prepared.scene, None, width, height)?;
        Ok(IdPicture {
            ids,
            names: prepared.names,
        })
    }
}
