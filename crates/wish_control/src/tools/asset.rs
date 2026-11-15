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
        .filter(|path| !path.ends_with(".meta.json") && !matches!(*path, "wish.json" | "CLAUDE.md"))
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

fn asset_move(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let from = args.str("from").unwrap_or_default();
    let to = args.str("to").unwrap_or_default();
    let dry_run = args.flag("dry_run");
    let plan = wish_asset::plan_move(&root, from, to, workspace.registry())?;
    if !dry_run {
        wish_asset::apply_move(&root, &plan)?;
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
    Ok(out)
}
