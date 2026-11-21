//! Asset tools: `asset_list`, `asset_info`, `asset_move`, `asset_create`.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

const KINDS: &[&str] = &[
    "all", "model", "image", "material", "sheet", "tileset", "tilemap", "other",
];

pub(crate) const ASSET_LIST: Tool = Tool {
    name: "asset_list",
    title: "List asset files",
    description: "Lists the project's asset files (models, images, material files, sprite sheets, tilesets, tilemaps, anything that is not a scene, prefab or meta file) with their kind and size. Filter by kind or by words in the path.",
    params: &[
        Param::optional(
            "kind",
            Kind::Choice(KINDS),
            "Only this kind: model (glTF), image (PNG, JPEG), material (*.mat.json), sheet (*.sheet.json), tileset (*.tileset.json), tilemap (*.tilemap.json), other.",
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
    description: "Reports on one asset file. For a glTF/GLB model: triangle and vertex counts, size in meters, materials, textures, how many entities-worth of parts it has, and anything that was not imported (with hints). For a material file (*.mat.json): the look it holds (material: every field with its value, written: the fields the file itself writes), its textures and whether each exists. For a sprite sheet (*.sheet.json): its picture and whether it exists, how many frames it is cut into (each frame's rectangle) and its clips (frames, fps, loop). For a tileset (*.tileset.json): its picture or sheet, how many tiles, each tile's rectangle, and the properties of the tiles that have some (solid, shape, name). For a tilemap (*.tilemap.json): its tileset, its layers (name, collide, how many tiles, the box they cover in cells) and the box of the whole map. Also its import settings (meta file) and the scenes, prefabs and material files that use it.",
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
    description: "Moves or renames an asset file or folder and rewrites every reference to it in scenes, prefabs, material files, sprite sheets, tilesets and tilemaps, all or nothing. dry_run lists what would change.",
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
    title: "Make a sound, a picture, a material file, a sprite sheet, a tileset or a tilemap",
    description: "Makes an asset file from a few values (nothing is downloaded): a placeholder sound or picture, so a game can have them before real ones exist, or a material file. A .mat.json path makes a material file from material {...}: the fields of the MaterialFile type (schema_get MaterialFile), for example {base_color_texture: \"assets/textures/brick.png\", roughness: 0.9, uv_scale: [4, 4]}; a shape gets the look (and so the texture) when its Material.file names the file, and every entity that names it changes when the file does. To change a material file, make it again with every field and overwrite: true (asset_info gives what it holds). A .sheet.json path makes a sprite sheet from sheet {image: a PNG, grid: {frame: [w, h], columns, rows, margin: [x, y], spacing: [x, y]} or frames: [[x, y, w, h], ...] (pixels from the top left; frames are numbered from 0, left to right then down), clips: {name: {frames: [0, 1, 2, 3], fps: 8, loop: true}}}; an entity with a Sprite shows it through a SpriteAnimator {sheet, clip}, and game code switches clips with animator.play(text(\"walk\")). A .tileset.json path makes a tileset from tileset {image: a PNG, grid: {frame: [w, h], columns, rows, margin, spacing} (tiles numbered from 0, left to right then down), or sheet: a sprite sheet whose frames are the tiles; tiles: {\"8\": {solid: true, name: \"wall\"}, \"9\": {solid: true, shape: [[0, 0], [1, 0], [1, 1]]}} (shape: a convex outline in tile units from the bottom left)}. A .tilemap.json path makes a tilemap from tilemap {tileset: a .tileset.json, layers: [\"floor\", {name: \"walls\", collide: true}]} (empty; paint it with tilemap_edit); an entity shows it with a Tilemap {map, cell_size}. A .wav path makes a sound: sound {wave: sine|square|triangle|saw|noise, frequency (Hz), end_frequency (a slide: higher for a jump, lower for a fall), seconds, volume 0-1, attack and release (fade seconds), seed}. A .png path makes a picture: picture {width, height (pixels, at most 1024), shape: fill|circle|ring|diamond|triangle, color [r,g,b,a] and background [r,g,b,a] (sRGB, 0-1)}. Examples: a jump {wave: square, frequency: 300, end_frequency: 700, seconds: 0.15, release: 0.1}; a coin {wave: sine, frequency: 900, end_frequency: 1400, seconds: 0.12}; a thud {wave: noise, seconds: 0.25, release: 0.2}. An existing file is not replaced unless overwrite is true.",
    params: &[
        Param::required(
            "path",
            Kind::String,
            "The project-relative file to write: .wav for a sound, .png for a picture, .mat.json for a material file, .sheet.json for a sprite sheet, .tileset.json for a tileset, .tilemap.json for a tilemap.",
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
            "material",
            Kind::Object,
            "For a .mat.json: the fields of a MaterialFile (base_color, metallic, roughness, emissive, base_color_texture, metallic_roughness_texture, normal_texture, normal_scale, occlusion_texture, occlusion_strength, emissive_texture, uv_scale, uv_offset, alpha_mode, alpha_cutoff, double_sided); fields left out keep their default.",
        ),
        Param::optional(
            "sheet",
            Kind::Object,
            "For a .sheet.json: image (a PNG), grid {frame: [w, h], columns, rows, margin, spacing} or frames [[x, y, w, h], ...], clips {name: {frames: [...], fps, loop}}.",
        ),
        Param::optional(
            "tileset",
            Kind::Object,
            "For a .tileset.json: image (a PNG) and grid {frame: [w, h], columns, rows, margin, spacing}, or sheet (a .sheet.json); tiles {\"<number>\": {solid, shape, name}}.",
        ),
        Param::optional(
            "tilemap",
            Kind::Object,
            "For a .tilemap.json: tileset (a .tileset.json), layers [names, or {name, collide, chunks}].",
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

const SOUND_FIELDS: &[&str] = &[
    "wave",
    "frequency",
    "end_frequency",
    "seconds",
    "volume",
    "attack",
    "release",
    "seed",
];
const PICTURE_FIELDS: &[&str] = &["width", "height", "shape", "color", "background"];

/// A spec with a field it does not know is a mistake (a default would be
/// used silently; M7 closing review).
fn known_fields(spec: &Map<String, Value>, known: &[&str], at: &str) -> Result<()> {
    for field in spec.keys() {
        if !known.contains(&field.as_str()) {
            return Err(spec_error(
                &format!("{at}/{field}"),
                format!(
                    "'{field}' is not a field of '{}'.",
                    at.trim_start_matches('/')
                ),
                &wish_core::suggest::unknown_name_hint(field, known),
            ));
        }
    }
    Ok(())
}

/// A name field of a spec object (a wave, a shape).
fn spec_name<'a>(
    spec: &'a Map<String, Value>,
    field: &str,
    default: &'a str,
    at: &str,
) -> Result<&'a str> {
    match spec.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(name)) => Ok(name),
        Some(other) => Err(spec_error(
            &format!("{at}/{field}"),
            format!("'{field}' must be a name in quotes, not {other}."),
            &format!("Write it like \"{field}\": \"{default}\"."),
        )),
    }
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
        &[
            "png",
            "wav",
            "mat.json",
            "sheet.json",
            "tileset.json",
            "tilemap.json",
        ],
    )
    .map_err(|problem| problem.at_path("/path"))?;
    let lower = path.to_ascii_lowercase();
    let target = crate::documents::inside_project(workspace.root(), &path)?;
    if let Some(spelled) = crate::documents::other_case(workspace.root(), &path) {
        return Err(Diagnostic::error(
            "invalid_output_path",
            format!("'{path}' is spelled '{spelled}' on disk."),
            "Use the spelling on disk: names that differ only in letter case work on Windows but not on Linux.",
        )
        .at_path("/path"));
    }
    // The spec that belongs to the file's kind, and only that one.
    let wanted = if lower.ends_with(".wav") {
        "sound"
    } else if wish_asset::is_material(&lower) {
        "material"
    } else if wish_asset::is_sheet(&lower) {
        "sheet"
    } else if wish_asset::is_tileset(&lower) {
        "tileset"
    } else if wish_asset::is_tilemap(&lower) {
        "tilemap"
    } else {
        "picture"
    };
    for other in [
        "sound", "picture", "material", "sheet", "tileset", "tilemap",
    ] {
        if other != wanted && args.value(other).is_some_and(|v| !v.is_null()) {
            return Err(spec_error(
                &format!("/{other}"),
                format!("'{other}' does not go with '{path}'."),
                &format!(
                    "A .wav file is made from \"sound\", a .png file from \"picture\", a .mat.json file from \"material\", a .sheet.json file from \"sheet\", a .tileset.json file from \"tileset\" and a .tilemap.json file from \"tilemap\"; give \"{wanted}\" for this path."
                ),
            ));
        }
    }
    if target.exists() && !args.flag("overwrite") {
        return Err(Diagnostic::error(
            "output_exists",
            format!("'{path}' already exists; it was not replaced."),
            "Choose a new name, or pass \"overwrite\": true to replace it.",
        )
        .at_path("/path"));
    }
    let mut out = Map::new();
    if wanted == "material" {
        return create_material(workspace, args, &path, out);
    }
    if wanted == "sheet" {
        return create_sheet(workspace, args, &path, out);
    }
    if wanted == "tileset" || wanted == "tilemap" {
        return create_tiles(workspace, args, &path, wanted, out);
    }
    let bytes = if lower.ends_with(".wav") {
        let spec = spec_object(args, "sound")?;
        let at = "/sound";
        known_fields(&spec, SOUND_FIELDS, at)?;
        let wave_name = spec_name(&spec, "wave", "sine", at)?;
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
        known_fields(&spec, PICTURE_FIELDS, at)?;
        let width = spec_number(&spec, "width", 1.0, 1024.0, 64.0, at)? as u32;
        let height = spec_number(&spec, "height", 1.0, 1024.0, 64.0, at)? as u32;
        const SHAPES: &[&str] = &["fill", "circle", "ring", "diamond", "triangle"];
        let shape = spec_name(&spec, "shape", "fill", at)?;
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
    let target = crate::documents::prepare_output(workspace.root(), &path)?;
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

/// A material file from the `material` argument: checked against the
/// `MaterialFile` schema, written in canonical form, all or nothing. Textures it
/// names that are not there (yet) are warnings: the file is still made.
fn create_material(
    workspace: &mut Workspace,
    args: &Args,
    path: &str,
    mut out: Map<String, Value>,
) -> Result<Map<String, Value>> {
    if !path.ends_with(wish_asset::MATERIAL_SUFFIX) {
        return Err(Diagnostic::error(
            "invalid_output_path",
            format!(
                "'{path}' must end in lower case: {}.",
                wish_asset::MATERIAL_SUFFIX
            ),
            "Material files are found by that ending; write it exactly so.",
        )
        .at_path("/path"));
    }
    let file = wish_asset::MaterialDocument {
        material: spec_object(args, "material")?,
    };
    file.check()?;
    let text = file.to_canonical()?;
    let root = workspace.root().to_path_buf();
    // What the material names, against the project as it is now. The file
    // being made counts as there.
    let index = wish_asset::AssetIndex::scan(&root)?;
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut textures = Vec::new();
    for used in file.textures() {
        textures.push(json!(used.reference));
        let problem = index.check(&used.reference).err().or_else(|| {
            (!used.reference.to_ascii_lowercase().ends_with(".png")).then(|| {
                Diagnostic::warning(
                    "texture_not_png",
                    format!("'{}' is not a PNG file: it will not be drawn.", used.reference),
                    "Textures outside models are read from PNG files only. Convert the picture to PNG (8-bit) and name that file.",
                )
            })
        });
        if let Some(problem) = problem {
            // The file is made all the same: the picture may come later.
            diagnostics.push(
                Diagnostic::warning(
                    problem.code.clone(),
                    problem.message.clone(),
                    problem.hint.clone(),
                )
                .at_path(used.pointer.clone()),
            );
        }
    }
    out.insert("kind".to_owned(), json!("material"));
    out.insert("path".to_owned(), json!(path));
    out.insert("bytes".to_owned(), json!(text.len()));
    out.insert("material".to_owned(), Value::Object(file.material.clone()));
    out.insert("textures".to_owned(), json!(textures));
    if !diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    if args.flag("dry_run") {
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    crate::documents::prepare_output(&root, path)?;
    crate::documents::write_project_file(&root, path, &text)
        .map_err(|problem| problem.at_path("/path"))?;
    Ok(out)
}

/// A sprite sheet from the `sheet` argument: checked like a sheet file,
/// written in canonical form, all or nothing. A picture it names that is not
/// there (yet) is a warning: the file is still made.
fn create_sheet(
    workspace: &mut Workspace,
    args: &Args,
    path: &str,
    mut out: Map<String, Value>,
) -> Result<Map<String, Value>> {
    if !path.ends_with(wish_asset::SHEET_SUFFIX) {
        return Err(Diagnostic::error(
            "invalid_output_path",
            format!(
                "'{path}' must end in lower case: {}.",
                wish_asset::SHEET_SUFFIX
            ),
            "Sprite sheets are found by that ending; write it exactly so.",
        )
        .at_path("/path"));
    }
    let spec = spec_object(args, "sheet")?;
    let file = wish_asset::SheetDocument::from_spec(spec)?;
    let sheet = file.decode()?;
    let text = file.to_canonical()?;
    let root = workspace.root().to_path_buf();
    let index = wish_asset::AssetIndex::scan(&root)?;
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if let Some(used) = file.image() {
        let problem = index.check(&used.reference).err().or_else(|| {
            (!used.reference.to_ascii_lowercase().ends_with(".png")).then(|| {
                Diagnostic::warning(
                    "texture_not_png",
                    format!("'{}' is not a PNG file: the sprites will not be drawn.", used.reference),
                    "Sprite sheets are cut from PNG files only. Convert the picture to PNG (8-bit) and name that file.",
                )
            })
        });
        if let Some(problem) = problem {
            // The file is made all the same: the picture may come later.
            diagnostics.push(
                Diagnostic::warning(
                    problem.code.clone(),
                    problem.message.clone(),
                    problem.hint.clone(),
                )
                .at_path(format!("/sheet{}", used.pointer)),
            );
        }
    }
    out.insert("kind".to_owned(), json!("sheet"));
    out.insert("path".to_owned(), json!(path));
    out.insert("bytes".to_owned(), json!(text.len()));
    out.insert("sheet".to_owned(), Value::Object(file.sheet.clone()));
    out.insert("frames".to_owned(), json!(sheet.frames.len()));
    out.insert("clips".to_owned(), json!(sheet.clip_names()));
    if !diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    if args.flag("dry_run") {
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    crate::documents::prepare_output(&root, path)?;
    crate::documents::write_project_file(&root, path, &text)
        .map_err(|problem| problem.at_path("/path"))?;
    Ok(out)
}

/// A tileset or a tilemap from the `tileset` or `tilemap` argument: checked
/// like a file, written in canonical form, all or nothing. What it names
/// that is not there (yet) is a warning: the file is still made.
fn create_tiles(
    workspace: &mut Workspace,
    args: &Args,
    path: &str,
    kind: &str,
    mut out: Map<String, Value>,
) -> Result<Map<String, Value>> {
    let suffix = if kind == "tileset" {
        wish_asset::TILESET_SUFFIX
    } else {
        wish_asset::TILEMAP_SUFFIX
    };
    if !path.ends_with(suffix) {
        return Err(Diagnostic::error(
            "invalid_output_path",
            format!("'{path}' must end in lower case: {suffix}."),
            "Tilesets and tilemaps are found by that ending; write it exactly so.",
        )
        .at_path("/path"));
    }
    let spec = spec_object(args, kind)?;
    let root = workspace.root().to_path_buf();
    let index = wish_asset::AssetIndex::scan(&root)?;
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut missing = |reference: &str, pointer: String| {
        if let Err(problem) = index.check(reference) {
            diagnostics.push(
                Diagnostic::warning(
                    problem.code.clone(),
                    problem.message.clone(),
                    problem.hint.clone(),
                )
                .at_path(pointer),
            );
        }
    };
    let text = if kind == "tileset" {
        let file = wish_asset::TilesetDocument::from_spec(spec)?;
        let tileset = file.decode()?;
        if let Some(used) = file.source() {
            missing(&used.reference, format!("/tileset{}", used.pointer));
        }
        let tiles = match &tileset.source {
            wish_asset::TileSource::Picture { frames, .. } => json!(frames.len()),
            wish_asset::TileSource::Sheet(sheet) => match tileset.frames(&root) {
                Ok((_, frames)) => json!(frames.len()),
                Err(_) => json!(format!("the frames of {sheet}")),
            },
        };
        out.insert("tiles".to_owned(), tiles);
        out.insert("tileset".to_owned(), Value::Object(file.tileset.clone()));
        file.to_canonical()?
    } else {
        let map = wish_asset::Tilemap::from_spec(spec)?;
        missing(&map.tileset, "/tilemap/tileset".to_owned());
        out.insert("tileset".to_owned(), json!(map.tileset));
        out.insert("layers".to_owned(), json!(map.layer_names()));
        map.to_canonical()?
    };
    out.insert("kind".to_owned(), json!(kind));
    out.insert("path".to_owned(), json!(path));
    out.insert("bytes".to_owned(), json!(text.len()));
    if !diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    if args.flag("dry_run") {
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    crate::documents::prepare_output(&root, path)?;
    crate::documents::write_project_file(&root, path, &text)
        .map_err(|problem| problem.at_path("/path"))?;
    Ok(out)
}

/// What a tileset holds, for `asset_info`.
fn tileset_info(
    root: &std::path::Path,
    path: &str,
    index: &wish_asset::AssetIndex,
    out: &mut Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source = match wish_asset::read_tileset_file(&root.join(path), path) {
        Ok(source) => source,
        Err(problem) => {
            diagnostics.push(problem);
            return;
        }
    };
    let (file, problems) = wish_asset::check_tileset(&source, path, index, Some(root));
    diagnostics.extend(problems);
    let Some(file) = file.or_else(|| wish_asset::parse_tileset(&source, path).ok()) else {
        return;
    };
    let Ok(tileset) = file.decode() else {
        return;
    };
    let mut info = json!({});
    match &tileset.source {
        wish_asset::TileSource::Picture { image, .. } => {
            info["image"] = json!(image);
            info["image_exists"] = json!(index.contains(image));
        }
        wish_asset::TileSource::Sheet(sheet) => {
            info["sheet"] = json!(sheet);
            info["sheet_exists"] = json!(index.contains(sheet));
        }
    }
    if let Ok((image, frames)) = tileset.frames(root) {
        info["image"] = json!(image);
        info["tile_count"] = json!(frames.len());
        info["tiles"] = json!(frames.iter().take(256).collect::<Vec<_>>());
    }
    let props: Map<String, Value> = tileset
        .tiles
        .iter()
        .map(|(number, props)| {
            let mut entry = json!({ "solid": props.solid });
            if let Some(shape) = &props.shape {
                entry["shape"] = json!(shape);
            }
            if let Some(name) = &props.name {
                entry["name"] = json!(name);
            }
            (number.to_string(), entry)
        })
        .collect();
    info["properties"] = Value::Object(props);
    out.insert("tileset".to_owned(), info);
    out.insert("written".to_owned(), Value::Object(file.tileset.clone()));
}

/// What a tilemap holds, for `asset_info`.
fn tilemap_info(
    root: &std::path::Path,
    path: &str,
    index: &wish_asset::AssetIndex,
    out: &mut Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source = match wish_asset::read_tilemap_file(&root.join(path), path) {
        Ok(source) => source,
        Err(problem) => {
            diagnostics.push(problem);
            return;
        }
    };
    let (map, problems) = wish_asset::check_tilemap(&source, path, index, Some(root));
    diagnostics.extend(problems);
    let Some(map) = map.or_else(|| wish_asset::parse_tilemap(&source, path).ok()) else {
        return;
    };
    let layers: Vec<Value> = map
        .layers
        .iter()
        .map(|layer| {
            let mut entry = json!({
                "name": layer.name,
                "collide": layer.collide,
                "tiles": layer.tile_count(),
            });
            if let Some(bounds) = layer.bounds() {
                entry["box"] = json!(bounds);
            }
            entry
        })
        .collect();
    let mut info = json!({
        "tileset": map.tileset,
        "tileset_exists": index.contains(&map.tileset),
        "layers": layers,
        "chunks": map.chunk_count(),
    });
    if let Some(bounds) = map.bounds() {
        info["box"] = json!(bounds);
    }
    out.insert("tilemap".to_owned(), info);
}

fn kind_of(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".glb") || lower.ends_with(".gltf") {
        "model"
    } else if wish_asset::is_material(&lower) {
        "material"
    } else if wish_asset::is_sheet(&lower) {
        "sheet"
    } else if wish_asset::is_tileset(&lower) {
        "tileset"
    } else if wish_asset::is_tilemap(&lower) {
        "tilemap"
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

/// What a material file holds, for `asset_info`.
fn material_info(
    root: &std::path::Path,
    path: &str,
    index: &wish_asset::AssetIndex,
    out: &mut Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source = match wish_asset::read_material_file(&root.join(path), path) {
        Ok(source) => source,
        Err(problem) => {
            diagnostics.push(problem);
            return;
        }
    };
    let (file, problems) = wish_asset::check_material(&source, path, index);
    diagnostics.extend(problems);
    // A file with a missing texture is still a look worth reporting.
    let Some(file) = file.or_else(|| wish_asset::parse_material(&source, path).ok()) else {
        return;
    };
    if let Ok(effective) = file.effective() {
        out.insert("material".to_owned(), effective);
    }
    out.insert("written".to_owned(), Value::Object(file.material.clone()));
    let textures: Vec<Value> = file
        .textures()
        .iter()
        .map(|used| {
            json!({
                "slot": used.pointer.trim_start_matches("/material/"),
                "path": used.reference,
                "exists": index.contains(&used.reference),
            })
        })
        .collect();
    out.insert("textures".to_owned(), json!(textures));
}

/// What a sprite sheet holds, for `asset_info`.
fn sheet_info(
    root: &std::path::Path,
    path: &str,
    index: &wish_asset::AssetIndex,
    out: &mut Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source = match wish_asset::read_sheet_file(&root.join(path), path) {
        Ok(source) => source,
        Err(problem) => {
            diagnostics.push(problem);
            return;
        }
    };
    let (file, problems) = wish_asset::check_sheet(&source, path, index, Some(root));
    diagnostics.extend(problems);
    // A sheet whose picture is missing is still worth reporting.
    let Some(file) = file.or_else(|| wish_asset::parse_sheet(&source, path).ok()) else {
        return;
    };
    let Ok(sheet) = file.decode() else {
        return;
    };
    let clips: Map<String, Value> = sheet
        .clips
        .iter()
        .map(|clip| {
            (
                clip.name.clone(),
                json!({ "frames": clip.frames, "fps": clip.fps, "loop": clip.looping }),
            )
        })
        .collect();
    out.insert(
        "sheet".to_owned(),
        json!({
            "image": sheet.image,
            "image_exists": index.contains(&sheet.image),
            "frame_count": sheet.frames.len(),
            "frames": sheet.frames.iter().take(256).collect::<Vec<_>>(),
            "clips": clips,
        }),
    );
    out.insert("written".to_owned(), Value::Object(file.sheet.clone()));
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

    if kind_of(&path) == "material" {
        material_info(&root, &path, &index, &mut out, &mut diagnostics);
    }
    if kind_of(&path) == "sheet" {
        sheet_info(&root, &path, &index, &mut out, &mut diagnostics);
    }
    if kind_of(&path) == "tileset" {
        tileset_info(&root, &path, &index, &mut out, &mut diagnostics);
    }
    if kind_of(&path) == "tilemap" {
        tilemap_info(&root, &path, &index, &mut out, &mut diagnostics);
    }

    match wish_asset::read_meta(&root, &path) {
        Ok(Some(meta)) => {
            out.insert("import_settings".to_owned(), Value::Object(meta.import));
        }
        Ok(None) => {}
        Err(problem) => diagnostics.push(problem),
    }

    // Who uses it: scenes, prefabs and material files that mention the
    // path.
    let project = wish_asset::load_project(&root, workspace.registry())?;
    let mut used_by: Vec<String> = Vec::new();
    for file in &project.files {
        if file.path != path && file.source.contains(&format!("\"{path}")) {
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
        workspace.tilemaps_moved(&plan.from, &plan.to, actor);
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
