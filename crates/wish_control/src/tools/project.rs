//! Project tools: `project_info`, `schema_get`, `check`.

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_ecs::TypeInfo;
use wish_scene::Actor;

use super::DETAIL;
use crate::param::{Args, Kind, Param};
use crate::project::PROJECT_FILE;
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PROJECT_INFO: Tool = Tool {
    name: "project_info",
    title: "Project overview",
    description: "Start here. Summarises the project: its name and default scene, the scene and prefab files, how many assets, how many problems `check` would report, and how many component types exist. Takes no parameters.",
    params: &[],
    mutates: false,
    run: project_info,
};

pub(crate) const SCHEMA_GET: Tool = Tool {
    name: "schema_get",
    title: "Component and resource types",
    description: "Describes the component, resource and event types scenes can use: fields, types, units, ranges and documentation. Without names or search it lists every type briefly. Use detail normal for field details, full for the JSON Schema.",
    params: &[
        Param::optional(
            "names",
            Kind::StringList,
            "Type names to describe, such as [\"Transform\", \"Camera\"].",
        ),
        Param::optional(
            "search",
            Kind::String,
            "Words to look for in type names, field names and documentation, such as \"light\".",
        ),
        DETAIL,
    ],
    mutates: false,
    run: schema_get,
};

pub(crate) const CHECK: Tool = Tool {
    name: "check",
    title: "Check the project",
    description: "Checks every scene, prefab and meta file of the project (syntax, schema, references, asset files) and the project file, and reports every problem with file, line and hint. `passed` is false if there is any error.",
    params: &[Param::optional(
        "paths",
        Kind::StringList,
        "Report only these project-relative files (all when left out).",
    )],
    mutates: false,
    run: check,
};

fn project_info(workspace: &mut Workspace, _: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let documents = wish_asset::index::documents(&root)?;
    let of_kind = |suffix: &str| -> Vec<String> {
        documents
            .iter()
            .filter(|(name, _)| name.ends_with(suffix))
            .map(|(name, _)| name.clone())
            .collect()
    };
    let assets = wish_asset::AssetIndex::scan(&root)?.len();
    let project = wish_asset::load_project(&root, workspace.registry())?;
    let mut errors = project.diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = project.diagnostics.len() - errors;
    let registry = workspace.registry();

    let mut out = Map::new();
    let mut diagnostics = Vec::new();
    match (workspace.config(), workspace.config_problem()) {
        (Some(config), _) => {
            out.insert(
                "project".to_owned(),
                json!({
                    "name": config.name,
                    "scene": config.scene,
                    "engine": config.engine,
                    "example": config.example,
                }),
            );
        }
        (None, Some(problem)) => {
            errors += 1;
            out.insert("project".to_owned(), Value::Null);
            diagnostics.push(problem.clone());
        }
        (None, None) => {
            out.insert("project".to_owned(), Value::Null);
            diagnostics.push(Diagnostic::warning(
                "no_project_file",
                format!("This folder has no '{PROJECT_FILE}', so it is not a Wish Engine project yet."),
                "Run `wen new <folder>` to create a project (scene, assets folder, MCP configuration), or work with the files as they are.",
            ));
        }
    }
    out.insert("engine".to_owned(), json!(env!("CARGO_PKG_VERSION")));
    out.insert("scenes".to_owned(), json!(of_kind(".scene.json")));
    out.insert("prefabs".to_owned(), json!(of_kind(".prefab.json")));
    out.insert("assets".to_owned(), json!(assets));
    out.insert("scripts".to_owned(), workspace.script_status());
    out.insert(
        "problems".to_owned(),
        json!({ "errors": errors, "warnings": warnings }),
    );
    out.insert(
        "types".to_owned(),
        json!({
            "components": registry.components().len(),
            "resources": registry.resources().len(),
            "events": registry.events().len(),
            "example": workspace.example(),
        }),
    );
    if !diagnostics.is_empty() {
        out.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    Ok(out)
}

/// One type found by `schema_get`: a component, resource or event of the
/// world, or the type a kind of file holds (`docs::file_types`).
struct Found {
    kind: &'static str,
    name: String,
    doc: String,
    managed: bool,
    schema: Value,
}

impl Found {
    fn of(kind: &'static str, info: &TypeInfo) -> Self {
        Self {
            kind,
            name: info.name().to_owned(),
            doc: info.schema().doc.clone(),
            managed: info.is_managed(),
            schema: wish_schema::json_schema::json_schema(info.schema()),
        }
    }
}

fn first_sentence(doc: &str) -> String {
    let line = doc.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    let end = line
        .find(". ")
        .map_or(line.len(), |index| index + 1)
        .min(200);
    line.chars().take(end).collect::<String>().trim().to_owned()
}

fn fields_of(schema: &Value) -> Vec<(String, Value)> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, field)| (name.clone(), field.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn field_detail(name: &str, field: &Value) -> Value {
    let mut out = Map::new();
    out.insert("name".to_owned(), json!(name));
    let kind = field
        .get("x-wish-type")
        .or_else(|| field.get("type"))
        .cloned()
        .unwrap_or(Value::Null);
    out.insert("type".to_owned(), kind);
    for (from, to) in [
        ("x-unit", "unit"),
        ("minimum", "min"),
        ("maximum", "max"),
        ("enum", "values"),
        ("default", "default"),
    ] {
        if let Some(value) = field.get(from) {
            out.insert(to.to_owned(), value.clone());
        }
    }
    if let Some(doc) = field.get("description").and_then(Value::as_str) {
        out.insert("doc".to_owned(), json!(first_sentence(doc)));
    }
    Value::Object(out)
}

fn schema_get(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let registry = workspace.registry();
    let mut all: Vec<Found> = Vec::new();
    for (_, info) in registry.components() {
        all.push(Found::of("component", info));
    }
    for (_, info) in registry.resources() {
        all.push(Found::of("resource", info));
    }
    for (_, info) in registry.events() {
        all.push(Found::of("event", info));
    }
    // What a kind of file holds: not in any world, written in a file.
    for schema in crate::docs::file_types() {
        all.push(Found {
            kind: "file",
            name: schema.name.clone(),
            doc: schema.doc.clone(),
            managed: false,
            schema: wish_schema::json_schema::json_schema(&schema),
        });
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));

    let names = args.strings("names");
    let all_names: Vec<&str> = all.iter().map(|found| found.name.as_str()).collect();
    for name in &names {
        if !all_names.contains(&name.as_str()) {
            return Err(Diagnostic::error(
                "unknown_type",
                format!("There is no component, resource, event or file type called '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &all_names),
            )
            .at_path("/names")
            .with_candidates(all_names.iter().copied()));
        }
    }
    let search = args.str("search").map(str::to_lowercase);
    let selected = all.iter().filter(|found| {
        let named = names.is_empty() || names.contains(&found.name);
        let matched = search.as_deref().is_none_or(|words| {
            let mut haystack =
                format!("{} {} {}", found.name, found.doc, search_words(&found.name));
            for (field, detail) in fields_of(&found.schema) {
                haystack.push(' ');
                haystack.push_str(&field);
                if let Some(doc) = detail.get("description").and_then(Value::as_str) {
                    haystack.push(' ');
                    haystack.push_str(doc);
                }
            }
            let haystack = haystack.to_lowercase();
            words.split_whitespace().all(|word| haystack.contains(word))
        });
        named && matched
    });

    let detail = args.str("detail").unwrap_or("summary");
    let types: Vec<Value> = selected
        .map(|found| {
            let mut out = Map::new();
            out.insert("name".to_owned(), json!(found.name));
            out.insert("kind".to_owned(), json!(found.kind));
            if found.managed {
                out.insert("read_only".to_owned(), json!(true));
            }
            match detail {
                "full" => {
                    out.insert("schema".to_owned(), found.schema.clone());
                }
                "normal" => {
                    out.insert("doc".to_owned(), json!(one_paragraph(&found.doc)));
                    let fields: Vec<Value> = fields_of(&found.schema)
                        .iter()
                        .map(|(name, field)| field_detail(name, field))
                        .collect();
                    out.insert("fields".to_owned(), json!(fields));
                }
                _ => {
                    out.insert("doc".to_owned(), json!(first_sentence(&found.doc)));
                    let fields: Vec<String> = fields_of(&found.schema)
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect();
                    out.insert("fields".to_owned(), json!(fields));
                }
            }
            Value::Object(out)
        })
        .collect();
    let mut out = Map::new();
    if types.is_empty() {
        out.insert(
            "diagnostics".to_owned(),
            json!([Diagnostic::info(
                "nothing_found",
                format!(
                    "No type matches the search '{}'.",
                    search.unwrap_or_default()
                ),
                "Search for fewer or shorter words, or leave search out to list every type.",
            )]),
        );
    }
    out.insert("count".to_owned(), json!(types.len()));
    out.insert("types".to_owned(), json!(types));
    Ok(out)
}

/// Project-relative document paths for `paths`, or `file_not_found` with
/// the closest real one: a typo must never make a check pass by checking
/// nothing.
pub(crate) fn resolve_documents(paths: &[String], root: &std::path::Path) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let documents: Vec<String> = wish_asset::index::documents(root)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let mut path = path.replace('\\', "/");
            while let Some(rest) = path.strip_prefix("./") {
                path = rest.to_owned();
            }
            if documents.contains(&path) || path == PROJECT_FILE {
                return Ok(path);
            }
            let hint = match wish_core::suggest::closest_match(
                &path,
                documents.iter().map(String::as_str),
            ) {
                Some(closest) => format!("Did you mean '{closest}'?"),
                None => "Give a scene, prefab, meta or material file path relative to the project folder."
                    .to_owned(),
            };
            Err(Diagnostic::error(
                "file_not_found",
                format!("'{path}' is not a scene, prefab, meta or material file of the project."),
                hint,
            )
            .at_path(format!("/paths/{index}")))
        })
        .collect()
}

fn check(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let root = workspace.root().to_path_buf();
    let paths = resolve_documents(&args.strings("paths"), &root)?;
    let selected = |file: &str| paths.is_empty() || paths.iter().any(|path| path == file);
    let project = wish_asset::load_project(&root, workspace.registry())?;
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    if let Some(problem) = workspace.config_problem()
        && selected(PROJECT_FILE)
    {
        diagnostics.push(problem.clone());
    }
    diagnostics.extend(
        project
            .diagnostics
            .iter()
            .filter(|d| d.location.file.as_deref().is_none_or(&selected))
            .cloned(),
    );
    // The action map is a project file like wish.json.
    let has_input = root.join(wish_input::INPUT_FILE).is_file();
    if has_input
        && selected(wish_input::INPUT_FILE)
        && let Err(problem) = workspace.input_map()
    {
        diagnostics.push(problem);
    }
    // Fonts the interface would use but cannot.
    let (_, font_problems) = workspace.fonts();
    diagnostics.extend(
        font_problems
            .into_iter()
            .filter(|d| d.location.file.as_deref().is_none_or(&selected)),
    );
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let files = project
        .files
        .iter()
        .filter(|file| selected(&file.path))
        .count()
        + usize::from(root.join(PROJECT_FILE).is_file() && selected(PROJECT_FILE))
        + usize::from(has_input && selected(wish_input::INPUT_FILE));
    let mut out = Map::new();
    out.insert("passed".to_owned(), json!(errors == 0));
    out.insert("files".to_owned(), json!(files));
    out.insert("errors".to_owned(), json!(errors));
    out.insert("warnings".to_owned(), json!(diagnostics.len() - errors));
    out.insert("diagnostics".to_owned(), json!(diagnostics));
    Ok(out)
}

/// Documentation text as flowing paragraphs: the line breaks of a doc
/// comment become spaces, blank lines stay paragraph breaks.
fn one_paragraph(doc: &str) -> String {
    doc.trim()
        .split("\n\n")
        .map(|paragraph| {
            paragraph
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) const DOCS_SEARCH: Tool = Tool {
    name: "docs_search",
    title: "Search the documentation",
    description: "Searches the engine guide, the pages of every component, resource and event type, and the pages of every diagnostic code. Returns the best matches with their wish:// address and a snippet; read a whole page as an MCP resource.",
    params: &[
        Param::required(
            "query",
            Kind::String,
            "Words to look for, such as \"camera field of view\".",
        ),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 20 },
            "At most this many results.",
        )
        .or("5"),
    ],
    mutates: false,
    run: docs_search,
};

fn docs_search(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let query = args.str("query").unwrap_or_default();
    let limit = usize::try_from(args.int("limit").unwrap_or(5)).unwrap_or(5);
    let recipes = crate::docs::package_recipes(workspace.root());
    let hits = crate::docs::search_with(workspace.registry(), query, limit, &recipes);
    let mut out = Map::new();
    if hits.is_empty() {
        out.insert(
            "diagnostics".to_owned(),
            json!([Diagnostic::info(
                "nothing_found",
                format!("Nothing in the documentation mentions all of: {query}."),
                "Search for fewer or shorter words; wish://docs/guide is the starting page.",
            )]),
        );
    }
    out.insert("results".to_owned(), json!(hits));
    Ok(out)
}

/// Words people (and agents) use for a type that its name and docs do not
/// say, so that a search for them finds it (M10b efficiency work: an agent
/// searched "mesh" and found nothing).
const SEARCH_WORDS: &[(&str, &str)] = &[
    (
        "Transform",
        "position location place move moved rotate rotation orientation scale size resize",
    ),
    (
        "Primitive",
        "mesh shape geometry cube box sphere ball plane quad",
    ),
    ("Model", "mesh gltf glb 3d model asset import"),
    (
        "Material",
        "color colour paint texture textures textured skin surface appearance look shiny metal rough glow glass transparent transparency see-through translucent opacity alpha fade fading invisible ghost water window",
    ),
    (
        "MaterialFile",
        "color colour paint texture textures textured image picture skin tile tiles tiling repeat uv bump bumps normal map cutout cut-out transparent transparency hole holes alpha mask leaves fence shared mat.json look surface appearance shiny metal rough glow glass see-through translucent blend blended opacity fade water window",
    ),
    (
        "RigidBody",
        "physics gravity fall falling dynamic static fixed kinematic mass",
    ),
    ("Collider", "collision hitbox physics shape trigger sensor"),
    ("PointLight", "lamp bulb light omni"),
    ("SpotLight", "spotlight flashlight torch cone light"),
    ("DirectionalLight", "sun sunlight light"),
    (
        "RenderSettings",
        "graphics quality performance shadow shadows resolution settings options antialiasing anti-aliasing aa fxaa jagged jaggies edges smooth cascade cascades cascaded csm blurry blocky sharp sharper distance",
    ),
    (
        "Ambient",
        "ambient skylight indirect environment fill light shade shadows dark",
    ),
    ("Sky", "background horizon sun disc sunset skybox"),
    (
        "Fog",
        "mist haze atmosphere distance depth weather smog fade far",
    ),
    (
        "Bloom",
        "glow glowing glare halo shine shining neon emissive bright light lens post processing dreamy",
    ),
    (
        "ColorGrading",
        "color colour grade grading contrast saturation vivid white balance temperature warm cool tint vignette post processing filter look mood tone",
    ),
    ("Camera", "view viewpoint eye lens fov"),
    ("Sprite", "2d image picture texture"),
    ("AudioSource", "sound music sfx"),
    ("ParticleEmitter", "particles fire smoke sparks effect vfx"),
];

fn search_words(name: &str) -> &'static str {
    SEARCH_WORDS
        .iter()
        .find(|(type_name, _)| *type_name == name)
        .map_or("", |(_, words)| words)
}
