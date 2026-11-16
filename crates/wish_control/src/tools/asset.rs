//! Asset tools: `asset_list`, `asset_info`, `asset_move`.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

const KINDS: &[&str] = &["all", "model", "image", "other"];

pub(crate) const ASSET_LIST: Tool = Tool {
    name: "asset_list",
    title: "List asset files",
    description: "Lists the project's asset files (models, images, anything that is not a scene, prefab or meta file) with their kind and size. Filter by kind or by words in the path.",
    params: &[
        Param::optional(
            "kind",
            Kind::Choice(KINDS),
            "Only this kind: model (glTF), image (PNG, JPEG), other.",
        )
        .or("\"all\""),
        Param::optional(
            "search",
            Kind::String,
            "Only paths containing this text (case does not matter).",
        ),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 1000 },
            "At most this many.",
        )
        .or("100"),
    ],
    mutates: false,
    run: asset_list,
};

pub(crate) const ASSET_INFO: Tool = Tool {
    name: "asset_info",
    title: "Asset report",
    description: "Reports on one asset file. For a glTF/GLB model: triangle and vertex counts, size in meters, materials, textures, how many entities-worth of parts it has, and anything that was not imported (with hints). Also its import settings (meta file) and the scenes and prefabs that use it.",
    params: &[Param::required(
        "path",
        Kind::String,
        "The project-relative asset path, such as \"assets/crate.glb\".",
    )],
    mutates: false,
    run: asset_info,
};

pub(crate) const ASSET_MOVE: Tool = Tool {
    name: "asset_move",
    title: "Move an asset",
    description: "Moves or renames an asset file or folder and rewrites every reference to it in scenes and prefabs, all or nothing. dry_run lists what would change.",
    params: &[
        Param::required(
            "from",
            Kind::String,
            "The current project-relative path (file or folder).",
        ),
        Param::required("to", Kind::String, "The new project-relative path."),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Report what would change without moving anything.",
        ),
    ],
    mutates: true,
    run: asset_move,
};

pub(crate) const ASSET_CREATE: Tool = Tool {
    name: "asset_create",
    title: "Make a placeholder sound or picture",
    description: "Makes a placeholder asset file from a few numbers, so a game can have sounds and pictures before real ones exist (nothing is downloaded). A .wav path makes a sound: sound {wave: sine|square|triangle|saw|noise, frequency (Hz), end_frequency (a slide: higher for a jump, lower for a fall), seconds, volume 0-1, attack and release (fade seconds), seed}. A .png path makes a picture: picture {width, height (pixels, at most 1024), shape: fill|circle|ring|diamond|triangle, color [r,g,b,a] and background [r,g,b,a] (sRGB, 0-1)}. Examples: a jump {wave: square, frequency: 300, end_frequency: 700, seconds: 0.15, release: 0.1}; a coin {wave: sine, frequency: 900, end_frequency: 1400, seconds: 0.12}; a thud {wave: noise, seconds: 0.25, release: 0.2}. An existing file is not replaced unless overwrite is true.",
    params: &[
        Param::required(
            "path",
            Kind::String,
            "The project-relative file to write: .wav for a sound, .png for a picture.",
        ),
        Param::optional(
            "sound",
            Kind::Object,
            "For a .wav: wave, frequency, end_frequency, seconds, volume, attack, release, seed.",
        ),
        Param::optional(
            "picture",
            Kind::Object,
            "For a .png: width, height, shape, color, background.",
        ),
        Param::optional(
            "overwrite",
            Kind::Boolean,
            "Replace the file if it already exists.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check the request and report what would be made, without writing.",
        ),
    ],
    mutates: true,
    run: asset_create,
};

fn spec_error(path: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("invalid_parameter", message, hint).at_path(path.to_owned())
}

/// A number field of a spec object, within bounds.
fn spec_number(
    spec: &Map<String, Value>,
    field: &str,
    low: f64,
    high: f64,
    default: f64,
    at: &str,
) -> Result<f64> {
    match spec.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite() && (low..=high).contains(n))
            .ok_or_else(|| {
                spec_error(
                    &format!("{at}/{field}"),
                    format!("'{field}' must be a number from {low} to {high}, not {value}."),
                    "Give a number in that range, or leave the field out for its default.",
                )
            }),
    }
}

/// An [r, g, b, a] colour field (sRGB, 0–1), as bytes.
fn spec_color(
    spec: &Map<String, Value>,
    field: &str,
    default: [u8; 4],
    at: &str,
) -> Result<[u8; 4]> {
    let Some(value) = spec.get(field).filter(|v| !v.is_null()) else {
        return Ok(default);
    };
    let parts: Option<Vec<f64>> = value.as_array().and_then(|items| {
        (items.len() == 3 || items.len() == 4)
            .then(|| {
                items
                    .iter()
                    .map(Value::as_f64)
                    .collect::<Option<Vec<f64>>>()
            })
            .flatten()
    });
    let Some(parts) = parts.filter(|p| p.iter().all(|c| (0.0..=1.0).contains(c))) else {
        return Err(spec_error(
            &format!("{at}/{field}"),
            format!(
                "'{field}' must be [r, g, b] or [r, g, b, a] with each part from 0 to 1, not {value}."
            ),
            "Write it like [1, 0.8, 0.2, 1] (sRGB, as in an image editor).",
        ));
    };
    let byte = |c: f64| (c * 255.0).round() as u8;
    Ok([
        byte(parts[0]),
        byte(parts[1]),
        byte(parts[2]),
        parts.get(3).map_or(255, |a| byte(*a)),
    ])
}

fn spec_object(args: &Args, name: &str) -> Result<Map<String, Value>> {
    match args.value(name) {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(object)) => Ok(object.clone()),
        Some(other) => Err(spec_error(
            &format!("/{name}"),
            format!("'{name}' must be an object, not {other}."),
            "Write it like { \"wave\": \"sine\", \"frequency\": 440, \"seconds\": 0.2 }.",
        )),
    }
}

/// The pixels of a placeholder picture.
fn draw_picture(
    width: u32,
    height: u32,
    shape: &str,
    color: [u8; 4],
    background: [u8; 4],
) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            // -1..1 across the picture, from the pixel's centre.
            let u = (x as f32 + 0.5) / width as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / height as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let inside = match shape {
                "circle" => r <= 1.0,
                "ring" => (0.7..=1.0).contains(&r),
                "diamond" => u.abs() + v.abs() <= 1.0,
                "triangle" => v >= -1.0 && u.abs() <= (v + 1.0) / 2.0,
                _ => true,
            };
            pixels.extend_from_slice(if inside { &color } else { &background });
        }
    }
    pixels
}

fn asset_create(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let path = crate::documents::normalize_asset_output(
        args.str("path").unwrap_or_default(),
        &["png", "wav"],
    )
    .map_err(|problem| problem.at_path("/path"))?;
    let lower = path.to_ascii_lowercase();
    let target = crate::documents::inside_project(workspace.root(), &path)?;
    if target.exists() && !args.flag("overwrite") {
        return Err(Diagnostic::error(
            "output_exists",
            format!("'{path}' already exists; it was not replaced."),
            "Choose a new name, or pass \"overwrite\": true to replace it.",
        )
        .at_path("/path"));
    }
    let mut out = Map::new();
    let bytes = if lower.ends_with(".wav") {
        let spec = spec_object(args, "sound")?;
        let at = "/sound";
        let wave_name = spec.get("wave").and_then(Value::as_str).unwrap_or("sine");
        let wave = wish_audio::Wave::parse(wave_name).ok_or_else(|| {
            spec_error(
                "/sound/wave",
                format!("'{wave_name}' is not a wave."),
                &wish_core::suggest::unknown_name_hint(wave_name, wish_audio::Wave::NAMES),
            )
        })?;
        let frequency = spec_number(&spec, "frequency", 20.0, 20_000.0, 440.0, at)?;
        let synth = wish_audio::Synth {
            wave,
            frequency: frequency as f32,
            end_frequency: spec_number(&spec, "end_frequency", 20.0, 20_000.0, frequency, at)?
                as f32,
            seconds: spec_number(
                &spec,
                "seconds",
                0.01,
                f64::from(wish_audio::MAX_SYNTH_SECONDS),
                0.3,
                at,
            )? as f32,
            volume: spec_number(&spec, "volume", 0.0, 1.0, 0.5, at)? as f32,
            attack: spec_number(&spec, "attack", 0.0, 60.0, 0.005, at)? as f32,
            release: spec_number(&spec, "release", 0.0, 60.0, 0.05, at)? as f32,
            seed: spec_number(&spec, "seed", 0.0, 4_294_967_295.0, 1.0, at)? as u64,
        };
        out.insert("kind".to_owned(), json!("sound"));
        out.insert("seconds".to_owned(), json!(synth.seconds));
        wish_audio::synth_wav(&synth)
    } else if lower.ends_with(".png") {
        let spec = spec_object(args, "picture")?;
        let at = "/picture";
        let width = spec_number(&spec, "width", 1.0, 1024.0, 64.0, at)? as u32;
        let height = spec_number(&spec, "height", 1.0, 1024.0, 64.0, at)? as u32;
        const SHAPES: &[&str] = &["fill", "circle", "ring", "diamond", "triangle"];
        let shape = spec.get("shape").and_then(Value::as_str).unwrap_or("fill");
        if !SHAPES.contains(&shape) {
            return Err(spec_error(
                "/picture/shape",
                format!("'{shape}' is not a shape."),
                &wish_core::suggest::unknown_name_hint(shape, SHAPES),
            ));
        }
        let color = spec_color(&spec, "color", [255; 4], at)?;
        let background = spec_color(&spec, "background", [0; 4], at)?;
        let image = wish_render::RgbaImage {
            width,
            height,
            pixels: draw_picture(width, height, shape, color, background),
        };
        out.insert("kind".to_owned(), json!("picture"));
        out.insert("size".to_owned(), json!([width, height]));
        image.to_png()?
    } else {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!("'{path}' is neither a .wav nor a .png file."),
            "End the path with .wav for a sound (with \"sound\") or .png for a picture (with \"picture\"), such as \"assets/sounds/jump.wav\".",
        )
        .at_path("/path"));
    };
    out.insert("path".to_owned(), json!(path));
    out.insert("bytes".to_owned(), json!(bytes.len()));
    if args.flag("dry_run") {
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder).map_err(|error| {
            Diagnostic::error(
                "file_unwritable",
                format!("Could not make the folder for '{path}': {error}."),
                "Check that the project folder can be written to.",
            )
        })?;
    }
    std::fs::write(&target, &bytes).map_err(|error| {
        Diagnostic::error(
            "file_unwritable",
            format!("Could not write '{path}': {error}."),
            "Check that the folder can be written to.",
        )
        .at_path("/path")
    })?;
    Ok(out)
}

fn kind_of(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".glb") || lower.ends_with(".gltf") {
        "model"
    } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image"
    } else {
        "other"
    }
}

fn asset_list(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let index = wish_asset::AssetIndex::scan(&root)?;
    let kind = args.str("kind").unwrap_or("all");
    let search = args.str("search").map(str::to_lowercase);
    let limit = usize::try_from(args.int("limit").unwrap_or(100)).unwrap_or(100);
    let matching: Vec<&str> = index
        .paths()
        .filter(|path| {
            !path.ends_with(".meta.json")
                && !matches!(*path, "wish.json" | "CLAUDE.md" | "input.json")
        })
        .filter(|path| kind == "all" || kind_of(path) == kind)
        .filter(|path| {
            search
                .as_deref()
                .is_none_or(|text| path.to_lowercase().contains(text))
        })
        .collect();
    let files: Vec<Value> = matching
        .iter()
        .take(limit)
        .map(|path| {
            let bytes = std::fs::metadata(root.join(path)).map_or(0, |meta| meta.len());
            json!({ "path": path, "kind": kind_of(path), "bytes": bytes })
        })
        .collect();
    let mut out = Map::new();
    out.insert("matched".to_owned(), json!(matching.len()));
    if matching.len() > files.len() {
        out.insert("truncated".to_owned(), json!(true));
    }
    out.insert("assets".to_owned(), json!(files));
    Ok(out)
}

fn asset_not_found(path: &str, index: &wish_asset::AssetIndex) -> Diagnostic {
    let hint = match wish_core::suggest::closest_match(path, index.paths()) {
        Some(closest) => format!("Did you mean '{closest}'?"),
        None => "Use asset_list to see the project's asset files.".to_owned(),
    };
    Diagnostic::error(
        "asset_not_found",
        format!("The project has no asset file '{path}'."),
        hint,
    )
    .at_path("/path")
}

fn asset_info(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let index = wish_asset::AssetIndex::scan(&root)?;
    let path = args.str("path").unwrap_or_default().replace('\\', "/");
    if !index.contains(&path) {
        return Err(asset_not_found(&path, &index));
    }
    let mut out = Map::new();
    out.insert("path".to_owned(), json!(path));
    out.insert("kind".to_owned(), json!(kind_of(&path)));
    let bytes = std::fs::metadata(root.join(&path)).map_or(0, |meta| meta.len());
    out.insert("bytes".to_owned(), json!(bytes));
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    if kind_of(&path) == "model" {
        match wish_import::load_model_file(&root.join(&path), &path) {
            Ok(model) => {
                let (mut vertices, mut triangles) = (0usize, 0usize);
                for mesh in &model.meshes {
                    for primitive in &mesh.primitives {
                        vertices += primitive.positions.len();
                        triangles += primitive.indices.len() / 3;
                    }
                }
                let placed = model.placed();
                let mut placed_triangles = 0usize;
                for item in &placed {
                    if let Some(primitive) = model
                        .meshes
                        .get(item.mesh)
                        .and_then(|mesh| mesh.primitives.get(item.primitive))
                    {
                        placed_triangles += primitive.indices.len() / 3;
                    }
                }
                let mut info = json!({
                    "meshes": model.meshes.len(),
                    "materials": model.materials.len(),
                    "textures": model.textures.len(),
                    "vertices": vertices,
                    "triangles": triangles,
                    "triangles_drawn": placed_triangles,
                    "parts_drawn": placed.len(),
                });
                if let Some((low, high)) = model.bounds() {
                    let size = high - low;
                    let round = |v: f32| (f64::from(v) * 1000.0).round() / 1000.0;
                    info["size_m"] = json!([round(size.x), round(size.y), round(size.z)]);
                    info["bounds_min"] = json!([round(low.x), round(low.y), round(low.z)]);
                    info["bounds_max"] = json!([round(high.x), round(high.y), round(high.z)]);
                    if size.max_element() > 100.0
                        || (size.max_element() > 0.0 && size.max_element() < 0.01)
                    {
                        diagnostics.push(
                            Diagnostic::warning(
                                "model_scale_unusual",
                                format!(
                                    "The model is {:.3} x {:.3} x {:.3} m: its largest side is {}.",
                                    size.x,
                                    size.y,
                                    size.z,
                                    if size.max_element() > 100.0 { "over 100 m" } else { "under 1 cm" }
                                ),
                                "glTF units are meters. Rescale the model in the modelling tool, or scale the Transform of the entity that uses it.",
                            )
                            .at_file(path.clone()),
                        );
                    }
                }
                out.insert("model".to_owned(), info);
                diagnostics.extend(model.warnings);
            }
            Err(problem) => diagnostics.push(problem),
        }
    }

    match wish_asset::read_meta(&root, &path) {
        Ok(Some(meta)) => {
            out.insert("import_settings".to_owned(), Value::Object(meta.import));
        }
        Ok(None) => {}
        Err(problem) => diagnostics.push(problem),
    }

    // Who uses it: scenes and prefabs that mention the path.
    let project = wish_asset::load_project(&root, workspace.registry())?;
    let mut used_by: Vec<String> = Vec::new();
    for file in &project.files {
        if file.source.contains(&format!("\"{path}")) {
            used_by.push(file.path.clone());
        }
    }
    out.insert("used_by".to_owned(), json!(used_by));
    if !diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    Ok(out)
}

fn asset_move(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let from = args.str("from").unwrap_or_default();
    let to = args.str("to").unwrap_or_default();
    let dry_run = args.flag("dry_run");
    let plan = wish_asset::plan_move(&root, from, to, workspace.registry())?;
    let mut notes = Vec::new();
    if !dry_run {
        // Open scenes first take in anything changed on disk before the move,
        // so the move is the only thing recorded as this actor's.
        let open: Vec<String> = workspace.open_scene_paths();
        for path in &open {
            notes.extend(workspace.open_document(path, false)?);
        }
        wish_asset::apply_move(&root, &plan)?;
        // Documents (open ones, the default scene) that the move took along.
        let mut known = open;
        if let Some(config) = workspace.config() {
            known.push(config.scene.clone());
        }
        let moved: Vec<(String, String)> = known
            .into_iter()
            .filter_map(|path| {
                let rest = if path == plan.from {
                    Some(String::new())
                } else {
                    path.strip_prefix(&format!("{}/", plan.from))
                        .map(|rest| format!("/{rest}"))
                }?;
                Some((path, format!("{}{rest}", plan.to)))
            })
            .collect();
        notes.extend(workspace.after_move(&moved, actor)?);
    }
    let documents: Vec<Value> = plan
        .documents
        .iter()
        .map(|edit| json!({ "file": edit.file, "references": edit.changes }))
        .collect();
    let mut out = Map::new();
    out.insert("from".to_owned(), json!(plan.from));
    out.insert("to".to_owned(), json!(plan.to));
    out.insert("dry_run".to_owned(), json!(dry_run));
    out.insert(
        "meta".to_owned(),
        json!(
            plan.meta
                .as_ref()
                .map(|(old, new)| json!({ "from": old, "to": new }))
        ),
    );
    out.insert("documents".to_owned(), json!(documents));
    if !notes.is_empty() {
        out.insert("diagnostics".to_owned(), json!(notes));
    }
    Ok(out)
}
