//! Scene tools: `scene_get`, `entity_query`, `entity_get`, `edit_apply`,
//! `edit_history`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Entity, Result};
use wish_scene::{Action, Actor, Applied, SceneDocument, Transaction};

use super::DETAIL;
use crate::documents::touched;
use crate::expr::Expr;
use crate::param::{Args, Kind, Param};
use crate::sessions::{LiveSubject, entity_names};
use crate::tool::Tool;
use crate::workspace::Workspace;
use wish_ecs::World;

const SCENE: Param = Param::optional(
    "scene",
    Kind::String,
    "The scene file, project-relative (default: the project's scene from wish.json).",
);

const SESSION: Param = Param::optional(
    "session",
    Kind::String,
    "Look at this play session's live world instead of the scene file (a session id from sim_start).",
);

/// Components the engine keeps for itself and does not show by default.
const INTERNAL: &[&str] = &["SceneId", "Hierarchy", "GlobalTransform"];

pub(crate) const SCENE_GET: Tool = Tool {
    name: "scene_get",
    title: "Scene outline",
    description: "The scene's hierarchy: entity ids, their component names and children, and the scene's resources. summary lists names only; normal adds the component values as written in the file; full adds notes, sibling order and prefab overrides. Use root and depth to look at part of a big scene. Also shows what is inside a prefab: pass its path as scene (\"prefabs/crate.prefab.json\"); there is no need to read files.",
    params: &[
        SCENE,
        Param::optional(
            "root",
            Kind::String,
            "Only this entity and what is below it.",
        ),
        Param::optional(
            "depth",
            Kind::Integer { min: 1, max: 64 },
            "How many levels below the top (or root) to show.",
        )
        .or("64"),
        DETAIL,
    ],
    mutates: false,
    run: scene_get,
};

pub(crate) const ENTITY_QUERY: Tool = Tool {
    name: "entity_query",
    title: "Find entities",
    description: "Finds entities by component (with), by a condition (where, e.g. \"Transform.translation.y < 0 && has(Camera)\"), by id pattern (name, e.g. \"crate_*\") or by distance (near: { \"point\": [x, y, z], \"radius\": r }, world positions). Values are the effective ones: prefab parts expanded, defaults filled in. Returns ids in order, and with components their values.",
    params: &[
        SCENE,
        SESSION,
        Param::optional(
            "with",
            Kind::StringList,
            "Component names every result must have.",
        ),
        Param::optional(
            "where",
            Kind::String,
            "A condition: paths like Transform.translation.y, == != < <= > >=, && || !, has(Name), distance(a, b), matches(id, \"a*\").",
        ),
        Param::optional(
            "name",
            Kind::String,
            "An id pattern: * matches any run of characters, ? one.",
        ),
        Param::optional(
            "near",
            Kind::Object,
            "{ \"point\": [x, y, z], \"radius\": meters }: only entities whose world position is that close; results are nearest first.",
        ),
        Param::optional(
            "components",
            Kind::StringList,
            "Include these components' values in the results.",
        ),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 1000 },
            "At most this many results.",
        )
        .or("50"),
        Param::optional(
            "cursor",
            Kind::String,
            "Continue after a previous answer's next_cursor.",
        ),
    ],
    mutates: false,
    run: entity_query,
};

pub(crate) const ENTITY_GET: Tool = Tool {
    name: "entity_get",
    title: "Entity values",
    description: "The component values of entities, by id: the effective values (prefab parts expanded, defaults filled in), plus parent, prefab and notes; for a part of a prefab instance (id instance/part), from_prefab and the components the instance overrides (overridden). Engine-internal components (SceneId, Hierarchy, GlobalTransform) are left out unless named. Also the values of resources (world-wide data such as a game's Score or Time), by name. Pass session to read a running game.",
    params: &[
        SCENE,
        SESSION,
        Param::optional(
            "ids",
            Kind::StringList,
            "Entity ids, such as [\"player\", \"camera\"].",
        ),
        Param::optional("components", Kind::StringList, "Only these components."),
        Param::optional(
            "resources",
            Kind::StringList,
            "Resource names, such as [\"Score\", \"Time\"]: their values are returned under resources. One the world does not hold yet reads as its defaults and is listed under resources_defaulted.",
        ),
    ],
    mutates: false,
    run: entity_get,
};

pub(crate) const EDIT_APPLY: Tool = Tool {
    name: "edit_apply",
    title: "Change the scene",
    description: "Applies a list of operations to a scene, all or nothing, and writes the file. Operations: spawn {id, parent?, order?, components?, notes?}, instantiate {id, prefab, ...}, despawn {id}, set {id, component, value} (merges fields), replace {id, component, value}, remove {id, component}, reparent {id, parent, order?}, rename {id, to}, set_notes {id, notes}, set_resource / replace_resource {name, value}, remove_resource {name}, set_override / replace_override / remove_override {id, entity, component, value?}. In Transform values, \"euler_deg\": [x, y, z] may stand for rotation. Siblings keep the order of their \"order\" (then by id): give order where it matters, as in an interface column. A failed operation changes nothing and says which one (location.op). Answers the new revision, what changed, and what others changed since you last looked. With session, the operations change that running game's world instead (not the file, not its history; it lasts while the session runs): spawn, despawn, set, replace, remove, reparent, rename and the resource operations.",
    params: &[
        SCENE,
        Param::optional(
            "session",
            Kind::String,
            "Change this running game (from sim_start) instead of the scene file. Not kept: make the change in the scene to keep it.",
        ),
        Param::required(
            "ops",
            Kind::Array,
            "The operations, each like { \"op\": \"spawn\", \"id\": \"crate\", \"components\": { ... } }.",
        ),
        Param::optional(
            "base_revision",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "Refuse if the scene has changed since this revision (from an earlier answer).",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check and report what would change, without changing anything.",
        ),
        Param::optional(
            "create",
            Kind::Boolean,
            "Create the scene file if it does not exist.",
        ),
        Param::optional(
            "reason",
            Kind::String,
            "Why, in a sentence (shown on this change's card in the editor's activity feed, and in edit_history).",
        ),
    ],
    mutates: true,
    run: edit_apply,
};

pub(crate) const EDIT_HISTORY: Tool = Tool {
    name: "edit_history",
    title: "History, undo and redo",
    description: "The scene's change history (who changed which entities, newest first) and undo / redo. Changes made by people, agents and by editing the file directly are all in one history.",
    params: &[
        SCENE,
        Param::optional(
            "action",
            Kind::Choice(&["list", "undo", "redo", "revert"]),
            "list the history; undo / redo the latest changes; or revert one earlier change on its own (revision), if nothing later touched what it changed.",
        )
        .or("\"list\""),
        Param::optional(
            "revision",
            Kind::Integer {
                min: 1,
                max: i64::MAX,
            },
            "For revert: the revision the change produced (from list, or from edit_apply's answer).",
        ),
        Param::optional(
            "steps",
            Kind::Integer { min: 1, max: 100 },
            "How many changes to undo or redo.",
        )
        .or("1"),
        Param::optional(
            "limit",
            Kind::Integer { min: 1, max: 100 },
            "How many history entries to list.",
        )
        .or("20"),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Report what undo / redo would change without changing anything.",
        ),
    ],
    mutates: true,
    run: edit_history,
};

/// `edit_history` revert: one earlier change taken back on its own.
fn revert_one(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
    path: &str,
    notes: Vec<Diagnostic>,
) -> Result<Map<String, Value>> {
    let Some(revision) = args.int("revision").and_then(|r| u64::try_from(r).ok()) else {
        return Err(Diagnostic::error(
            "missing_parameter",
            "revert needs \"revision\": the change to take back.",
            "Give the revision from edit_history list (or from edit_apply's answer), like \"revision\": 12.",
        )
        .at_path("/revision"));
    };
    let registry = workspace.registry().clone();
    let others = workspace.others_since(path, actor);
    let dry_run = args.flag("dry_run");
    let mut working = workspace.document(path)?.doc.clone();
    let reason = Some(format!("took back r{revision}"));
    let applied = if dry_run {
        working.revertible(revision)?;
        Applied {
            revision: working.revision(),
            changes: Vec::new(),
        }
    } else {
        working.revert(revision, actor.clone(), reason, &registry)?
    };
    if !dry_run {
        workspace.commit_document(path, working)?;
        workspace.mark_seen(path, actor);
    }
    let mut out = Map::new();
    out.insert("scene".to_owned(), json!(path));
    out.insert("action".to_owned(), json!("revert"));
    out.insert("reverted".to_owned(), json!(revision));
    out.insert("revision".to_owned(), json!(applied.revision));
    out.insert("changes".to_owned(), changes_json(&applied.changes));
    if dry_run {
        out.insert("dry_run".to_owned(), json!(true));
    }
    if let Some(others) = others {
        out.insert("changed_by_others".to_owned(), others);
    }
    finish(&mut out, notes, workspace, path);
    Ok(out)
}

// --------------------------------------------------------------------------

/// An entity's world position, from its `GlobalTransform`.
fn world_position(world: &World, entity: Entity) -> Option<[f64; 3]> {
    let matrix = world.get_json(entity, "GlobalTransform").ok()?;
    let values = matrix.get("matrix")?.as_array()?;
    let at = |i: usize| values.get(i).and_then(Value::as_f64);
    Some([at(12)?, at(13)?, at(14)?])
}

fn check_component_names(workspace: &Workspace, names: &[String], param: &str) -> Result<()> {
    let known: Vec<&str> = workspace
        .registry()
        .components()
        .map(|(_, info)| info.name())
        .collect();
    for name in names {
        if !known.contains(&name.as_str()) {
            // A resource asked for as a component (M7 friction log, F7).
            let is_resource = workspace.registry().resource_id(name).is_some();
            return Err(Diagnostic::error(
                "unknown_component",
                format!("There is no component called '{name}'."),
                if is_resource {
                    format!("'{name}' is a resource (world-wide data), not a component: read it with entity_get and \"resources\": [\"{name}\"] (pass session for a running game), or test it in a condition such as \"{name}.<field> > 0\".")
                } else {
                    wish_core::suggest::unknown_name_hint(name, &known)
                },
            )
            .at_path(format!("/{param}"))
            .with_candidates(known.iter().copied()));
        }
    }
    Ok(())
}

fn unknown_entity(id: &str, ids: &[&str], pointer: &str) -> Diagnostic {
    let mut candidates: Vec<&str> = ids.to_vec();
    candidates.truncate(20);
    Diagnostic::error(
        "unknown_entity",
        format!("The scene has no entity '{id}'."),
        match wish_core::suggest::closest_match(id, ids.iter().copied()) {
            Some(closest) => format!("Did you mean '{closest}'?"),
            None => "Use entity_query or scene_get to see the ids.".to_owned(),
        },
    )
    .at_path(pointer.to_owned())
    .with_candidates(candidates)
}

/// Opens the scene a tool names, returning its path and notes.
fn open(workspace: &mut Workspace, args: &Args, create: bool) -> Result<(String, Vec<Diagnostic>)> {
    let path = workspace.scene_path(args.str("scene"))?;
    let notes = workspace.open_document(&path, create)?;
    Ok((path, notes))
}

fn finish(
    out: &mut Map<String, Value>,
    mut notes: Vec<Diagnostic>,
    workspace: &Workspace,
    path: &str,
) {
    if let Ok(open) = workspace.document(path)
        && let Some(problem) = &open.disk_problem
        && !notes.iter().any(|n| n.code == problem.code)
    {
        notes.push(problem.clone());
    }
    if !notes.is_empty() {
        out.insert("diagnostics".to_owned(), json!(notes));
    }
}

// --------------------------------------------------------------------------

fn outline(doc: &SceneDocument, id: &str, depth: i64, detail: &str, budget: &mut usize) -> Value {
    let mut node = Map::new();
    node.insert("id".to_owned(), json!(id));
    let Some(entity) = doc.entity(id) else {
        return Value::Object(node);
    };
    if let Some(prefab) = entity.prefab() {
        node.insert("prefab".to_owned(), json!(prefab));
    }
    match detail {
        "summary" => {
            let names: Vec<&String> = entity.components().keys().collect();
            node.insert("components".to_owned(), json!(names));
        }
        _ => {
            node.insert("components".to_owned(), json!(entity.components()));
        }
    }
    if detail == "full" {
        node.insert("order".to_owned(), json!(entity.order()));
        if let Some(notes) = entity.notes() {
            node.insert("notes".to_owned(), json!(notes));
        }
        if !entity.overrides().is_empty() {
            node.insert("overrides".to_owned(), json!(entity.overrides()));
        }
    }
    let children = doc.children(id);
    if !children.is_empty() {
        if depth <= 1 {
            node.insert("children_hidden".to_owned(), json!(children.len()));
        } else {
            let mut shown = Vec::new();
            for child in &children {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                shown.push(outline(doc, child, depth - 1, detail, budget));
            }
            node.insert("children".to_owned(), json!(shown));
        }
    }
    Value::Object(node)
}

const OUTLINE_BUDGET: usize = 500;

fn scene_get(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let (path, notes) = open(workspace, args, false)?;
    let others = workspace.changes_by_others(&path, actor);
    let doc = &workspace.document(&path)?.doc;
    let detail = args.str("detail").unwrap_or("summary");
    let depth = args.int("depth").unwrap_or(64);
    let roots: Vec<String> = match args.str("root") {
        Some(root) => {
            if doc.entity(root).is_none() {
                let ids: Vec<&str> = doc.entities().map(|(id, _)| id).collect();
                return Err(unknown_entity(root, &ids, "/root"));
            }
            vec![root.to_owned()]
        }
        None => doc.roots().into_iter().map(str::to_owned).collect(),
    };
    let mut budget = OUTLINE_BUDGET;
    let mut tree = Vec::new();
    for root in &roots {
        if budget == 0 {
            break;
        }
        budget -= 1;
        tree.push(outline(doc, root, depth, detail, &mut budget));
    }
    let mut out = Map::new();
    out.insert("scene".to_owned(), json!(path));
    out.insert("revision".to_owned(), json!(doc.revision()));
    out.insert("entities".to_owned(), json!(doc.entities().count()));
    out.insert("tree".to_owned(), json!(tree));
    let resources: Value = if detail == "summary" {
        json!(doc.resources().keys().collect::<Vec<_>>())
    } else {
        json!(doc.resources())
    };
    out.insert("resources".to_owned(), resources);
    if budget == 0 {
        out.insert("truncated".to_owned(), json!(true));
        out.insert(
            "truncated_hint".to_owned(),
            json!("The outline stops at 500 entities. Pass root (an entity id) and depth to look at part of the scene."),
        );
    }
    if let Some(others) = others {
        out.insert("changed_by_others".to_owned(), others);
    }
    finish(&mut out, notes, workspace, &path);
    Ok(out)
}

fn entity_query(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let session_id = match args.str("session") {
        Some(id) => Some(workspace.session_id(Some(id))?),
        None => None,
    };
    let (path, mut notes) = if session_id.is_some() {
        (String::new(), Vec::new())
    } else {
        open(workspace, args, false)?
    };
    if session_id.is_none() {
        notes.extend(scene_not_session(workspace));
    }
    let with = args.strings("with");
    check_component_names(workspace, &with, "with")?;
    let include = args.strings("components");
    check_component_names(workspace, &include, "components")?;
    let condition = match args.str("where") {
        Some(text) => Some(
            Expr::compile(text, workspace.registry()).map_err(|problem| {
                let column = problem.location.column;
                let mut problem = problem.at_path("/where");
                problem.location.line = None;
                problem.location.column = column;
                problem
            })?,
        ),
        None => None,
    };
    let near = match args.value("near") {
        None => None,
        Some(value) => {
            let point: Option<Vec<f64>> = value
                .get("point")
                .and_then(Value::as_array)
                .filter(|items| items.len() == 3)
                .and_then(|items| items.iter().map(Value::as_f64).collect());
            let radius = value.get("radius").and_then(Value::as_f64);
            match (point, radius) {
                (Some(point), Some(radius)) if radius >= 0.0 && radius.is_finite() => {
                    Some(([point[0], point[1], point[2]], radius))
                }
                _ => {
                    return Err(Diagnostic::error(
                        "invalid_parameter",
                        "'near' must be { \"point\": [x, y, z], \"radius\": meters }.",
                        "Write it like \"near\": { \"point\": [0, 0, 0], \"radius\": 5 }.",
                    )
                    .at_path("/near"));
                }
            }
        }
    };
    let pattern = args.str("name").map(str::to_owned);
    let limit = usize::try_from(args.int("limit").unwrap_or(50)).unwrap_or(50);
    let cursor = args.str("cursor").map(str::to_owned);
    let revision = match &session_id {
        Some(id) => workspace.session(id)?.app.tick(),
        None => workspace.document(&path)?.doc.revision(),
    };
    let (world, ids): (&World, Cow<'_, BTreeMap<String, Entity>>) = match &session_id {
        Some(id) => {
            let world = workspace.session(id)?.app.world();
            (world, Cow::Owned(entity_names(world)))
        }
        None => {
            let view = workspace.view(&path)?;
            (&view.world, Cow::Borrowed(&view.ids))
        }
    };

    let mut matched: Vec<(String, Entity, Option<f64>)> = Vec::new();
    for (id, &entity) in ids.iter() {
        if let Some(pattern) = &pattern
            && !crate::expr::glob(pattern, id)
        {
            continue;
        }
        if !with.iter().all(|name| world.get_json(entity, name).is_ok()) {
            continue;
        }
        let distance = match near {
            Some((point, radius)) => {
                let Some(at) = world_position(world, entity) else {
                    continue;
                };
                let d = ((at[0] - point[0]).powi(2)
                    + (at[1] - point[1]).powi(2)
                    + (at[2] - point[2]).powi(2))
                .sqrt();
                if d > radius {
                    continue;
                }
                Some(d)
            }
            None => None,
        };
        if let Some(condition) = &condition
            && !condition.matches(&LiveSubject {
                world,
                id,
                entity: Some(entity),
            })
        {
            continue;
        }
        matched.push((id.clone(), entity, distance));
    }
    if near.is_some() {
        matched.sort_by(|a, b| {
            a.2.unwrap_or(f64::MAX)
                .total_cmp(&b.2.unwrap_or(f64::MAX))
                .then_with(|| a.0.cmp(&b.0))
        });
    }
    let total = matched.len();
    let start = match &cursor {
        Some(cursor) => {
            let after = cursor.strip_prefix("after:").unwrap_or(cursor);
            match matched.iter().position(|(id, _, _)| id == after) {
                Some(index) => index + 1,
                None => {
                    return Err(Diagnostic::error(
                        "invalid_cursor",
                        format!("The cursor '{cursor}' does not belong to this query's results."),
                        "Pass the next_cursor of the previous answer unchanged, with the same other parameters; or leave cursor out to start again.",
                    )
                    .at_path("/cursor"));
                }
            }
        }
        None => 0,
    };
    let page: Vec<&(String, Entity, Option<f64>)> =
        matched.iter().skip(start).take(limit).collect();
    let mut entities = Vec::new();
    for (id, entity, distance) in &page {
        let mut item = Map::new();
        item.insert("id".to_owned(), json!(id));
        if let Some(distance) = distance {
            item.insert(
                "distance".to_owned(),
                json!((distance * 1000.0).round() / 1000.0),
            );
        }
        if !include.is_empty() {
            let mut values = Map::new();
            for name in &include {
                if let Ok(value) = world.get_json(*entity, name) {
                    values.insert(name.clone(), value);
                }
            }
            item.insert("components".to_owned(), Value::Object(values));
        }
        entities.push(Value::Object(item));
    }
    let mut out = Map::new();
    match &session_id {
        Some(id) => {
            out.insert("session".to_owned(), json!(id));
            out.insert("tick".to_owned(), json!(revision));
        }
        None => {
            out.insert("scene".to_owned(), json!(path));
            out.insert("revision".to_owned(), json!(revision));
        }
    }
    out.insert("matched".to_owned(), json!(total));
    if start + page.len() < total
        && let Some((last, _, _)) = page.last()
    {
        out.insert("next_cursor".to_owned(), json!(format!("after:{last}")));
    }
    out.insert("entities".to_owned(), json!(entities));
    finish(&mut out, notes, workspace, &path);
    Ok(out)
}

fn entity_get(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let session_id = match args.str("session") {
        Some(id) => Some(workspace.session_id(Some(id))?),
        None => None,
    };
    let (path, mut notes) = if session_id.is_some() {
        (String::new(), Vec::new())
    } else {
        open(workspace, args, false)?
    };
    if session_id.is_none() {
        notes.extend(scene_not_session(workspace));
    }
    let ids = args.strings("ids");
    let wanted_resources = args.strings("resources");
    if ids.len() > 100 || wanted_resources.len() > 50 {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!(
                "Ask for at most 100 entity ids and 50 resources at a time (asked {} and {}).",
                ids.len(),
                wanted_resources.len()
            ),
            "Ask for fewer at a time, or use entity_query with components to read many entities.",
        )
        .at_path("/ids"));
    }
    if ids.is_empty() && wanted_resources.is_empty() {
        return Err(Diagnostic::error(
            "missing_parameter",
            "entity_get needs ids (entities) or resources (world-wide data) to read.",
            "Pass ids such as [\"player\"], or resources such as [\"Score\"]; scene_get lists what the scene has.",
        )
        .at_path("/ids"));
    }
    let only = args.strings("components");
    check_component_names(workspace, &only, "components")?;
    let revision = match &session_id {
        Some(id) => workspace.session(id)?.app.tick(),
        None => workspace.document(&path)?.doc.revision(),
    };
    let doc = match &session_id {
        Some(_) => None,
        None => Some(workspace.document(&path)?.doc.clone()),
    };
    let (world, names): (&World, Cow<'_, BTreeMap<String, Entity>>) = match &session_id {
        Some(id) => {
            let world = workspace.session(id)?.app.world();
            (world, Cow::Owned(entity_names(world)))
        }
        None => {
            let view = workspace.view(&path)?;
            (&view.world, Cow::Borrowed(&view.ids))
        }
    };
    let known: Vec<&str> = names.keys().map(String::as_str).collect();
    let mut entities = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let Some(&entity) = names.get(id) else {
            return Err(unknown_entity(id, &known, &format!("/ids/{index}")));
        };
        let mut item = Map::new();
        item.insert("id".to_owned(), json!(id));
        if let Some(described) = doc.as_ref().and_then(|doc| doc.entity(id)) {
            if let Some(parent) = described.parent() {
                item.insert("parent".to_owned(), json!(parent));
            }
            if let Some(prefab) = described.prefab() {
                item.insert("prefab".to_owned(), json!(prefab));
            }
            if let Some(notes) = described.notes() {
                item.insert("notes".to_owned(), json!(notes));
            }
        } else if let Some(doc) = &doc {
            item.insert("from_prefab".to_owned(), json!(true));
            // Which of its components the instance changes (overrides).
            if let Some((instance, inner)) = id.split_once('/')
                && let Some(changed) = doc
                    .entity(instance)
                    .and_then(|owner| owner.overrides().get(inner))
                && !changed.is_empty()
            {
                item.insert(
                    "overridden".to_owned(),
                    json!(changed.keys().collect::<Vec<_>>()),
                );
            }
        }
        let mut components = Map::new();
        if let Value::Object(all) = world.entity_json(entity)? {
            for (name, value) in all {
                let wanted = if only.is_empty() {
                    !INTERNAL.contains(&name.as_str())
                } else {
                    only.contains(&name)
                };
                if wanted {
                    components.insert(name, value);
                }
            }
        }
        item.insert("components".to_owned(), Value::Object(components));
        entities.push(Value::Object(item));
    }
    let resource_names: Vec<&str> = world
        .registry()
        .resources()
        .map(|(_, info)| info.name())
        .collect();
    let mut resources = Map::new();
    let mut defaulted = Vec::new();
    for (index, name) in wanted_resources.iter().enumerate() {
        if !resource_names.contains(&name.as_str()) {
            return Err(Diagnostic::error(
                "unknown_resource",
                format!("There is no resource called '{name}'."),
                wish_core::suggest::unknown_name_hint(name, &resource_names),
            )
            .at_path(format!("/resources/{index}"))
            .with_candidates(resource_names.iter().copied()));
        }
        // A resource the world does not hold reads as its defaults (what the
        // engine uses in its place), and the answer says so; any other
        // problem is reported (M7 closing review).
        let value = match world.resource_json(name) {
            Ok(value) => value,
            Err(problem) if problem.code != "resource_not_found" => return Err(problem),
            Err(_) => {
                defaulted.push(name.clone());
                world
                    .registry()
                    .resource_id(name)
                    .and_then(|id| world.registry().resource(id))
                    .and_then(|info| {
                        wish_schema::to_json(info.schema(), info.default_bytes(), world.texts())
                            .ok()
                    })
                    .unwrap_or(Value::Null)
            }
        };
        resources.insert(name.clone(), value);
    }
    let mut out = Map::new();
    match &session_id {
        Some(id) => {
            out.insert("session".to_owned(), json!(id));
            out.insert("tick".to_owned(), json!(revision));
        }
        None => {
            out.insert("scene".to_owned(), json!(path));
            out.insert("revision".to_owned(), json!(revision));
        }
    }
    if !ids.is_empty() {
        out.insert("entities".to_owned(), json!(entities));
    }
    if !wanted_resources.is_empty() {
        out.insert("resources".to_owned(), Value::Object(resources));
    }
    if !defaulted.is_empty() {
        out.insert("resources_defaulted".to_owned(), json!(defaulted));
    }
    finish(&mut out, notes, workspace, &path);
    Ok(out)
}

const MAX_CHANGES_SHOWN: usize = 50;

fn changes_json(changes: &[String]) -> Value {
    let entities: Vec<String> = touched(changes).into_iter().collect();
    let mut out = json!({
        "count": changes.len(),
        "entities": entities,
        "paths": changes.iter().take(MAX_CHANGES_SHOWN).collect::<Vec<_>>(),
    });
    if changes.len() > MAX_CHANGES_SHOWN {
        out["paths_hidden"] = json!(changes.len() - MAX_CHANGES_SHOWN);
    }
    out
}

fn refuse_if_file_invalid(workspace: &Workspace, path: &str) -> Result<()> {
    if let Some(problem) = &workspace.document(path)?.disk_problem {
        let mut refusal = problem.clone();
        refusal.severity = wish_core::Severity::Error;
        refusal.code = "scene_file_invalid".into();
        refusal.docs = "wish://docs/diagnostics/scene_file_invalid".to_owned();
        return Err(refusal);
    }
    Ok(())
}

fn edit_apply(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    if let Some(session) = args.str("session") {
        let id = workspace.session_id(Some(session))?;
        let ops = crate::ops::parse_ops(args.value("ops").unwrap_or(&Value::Null))?;
        return workspace.edit_session(&id, &ops, actor, args.flag("dry_run"));
    }
    let create = args.flag("create");
    let (path, notes) = open(workspace, args, create)?;
    refuse_if_file_invalid(workspace, &path)?;
    let ops = crate::ops::parse_ops(args.value("ops").unwrap_or(&Value::Null))?;
    let transaction = Transaction {
        actor: actor.clone(),
        reason: args
            .str("reason")
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(|reason| reason.chars().take(500).collect()),
        base_revision: args
            .int("base_revision")
            .and_then(|r| u64::try_from(r).ok()),
        ops,
    };
    // Not marked as seen until the change has worked: a refused edit (a
    // stale base_revision above all) must still tell what others did.
    let others = workspace.others_since(&path, actor);
    let registry = workspace.registry().clone();
    let dry_run = args.flag("dry_run");
    let mut working = workspace.document(&path)?.doc.clone();
    let applied = if dry_run {
        if let Some(base) = transaction.base_revision
            && base != working.revision()
        {
            // Same refusal as a real apply.
            working.apply(&transaction, &registry)?;
        }
        working.preview(&transaction, &registry)?
    } else {
        working.apply(&transaction, &registry)?
    };
    if !dry_run {
        // Written first, kept only if the write worked.
        workspace.commit_document(&path, working)?;
        workspace.mark_seen(&path, actor);
    }
    let mut out = Map::new();
    out.insert("scene".to_owned(), json!(path));
    out.insert("revision".to_owned(), json!(applied.revision));
    out.insert("changes".to_owned(), changes_json(&applied.changes));
    if dry_run {
        out.insert("dry_run".to_owned(), json!(true));
    }
    if let Some(others) = others {
        out.insert("changed_by_others".to_owned(), others);
    }
    finish(&mut out, notes, workspace, &path);
    Ok(out)
}

fn edit_history(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
) -> Result<Map<String, Value>> {
    let (path, notes) = open(workspace, args, false)?;
    let action = args.str("action").unwrap_or("list");
    let mut out = Map::new();
    out.insert("scene".to_owned(), json!(path));
    if action == "list" {
        let limit = usize::try_from(args.int("limit").unwrap_or(20)).unwrap_or(20);
        let doc = &workspace.document(&path)?.doc;
        let entries: Vec<Value> = doc
            .log()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(limit)
            .map(|entry| {
                let mut item = json!({
                    "revision": entry.revision,
                    "actor": entry.actor.label(),
                    "action": match entry.action {
                        Action::Edit => "edit",
                        Action::Undo => "undo",
                        Action::Redo => "redo",
                        Action::Revert => "revert",
                    },
                    "entities": touched(&entry.changes),
                    "changes": entry.changes.len(),
                });
                if let Some(reason) = &entry.reason {
                    item["reason"] = json!(reason);
                }
                if let Some(reverts) = entry.reverts {
                    item["reverts"] = json!(reverts);
                }
                // Whether this one change can be taken back on its own now,
                // and if not, why (a card's "Undo this").
                match doc.revertible(entry.revision) {
                    Ok(()) => item["revertible"] = json!(true),
                    Err(problem) => {
                        item["revertible"] = json!(false);
                        if problem.code != "not_revertible" {
                            item["not_revertible"] = json!(problem.code);
                        }
                    }
                }
                if doc.is_reverted(entry.revision) {
                    item["reverted"] = json!(true);
                }
                item
            })
            .collect();
        out.insert("revision".to_owned(), json!(doc.revision()));
        out.insert("can_undo".to_owned(), json!(doc.can_undo()));
        out.insert("can_redo".to_owned(), json!(doc.can_redo()));
        if let Some(next) = doc.next_undo_actor() {
            out.insert("next_undo_by".to_owned(), json!(next.label()));
        }
        out.insert("entries".to_owned(), json!(entries));
        finish(&mut out, notes, workspace, &path);
        return Ok(out);
    }
    refuse_if_file_invalid(workspace, &path)?;
    if action == "revert" {
        return revert_one(workspace, args, actor, &path, notes);
    }
    let steps = args.int("steps").unwrap_or(1);
    let dry_run = args.flag("dry_run");
    let registry = workspace.registry().clone();
    let others = workspace.others_since(&path, actor);
    let mut changes: Vec<String> = Vec::new();
    let mut done = 0;
    // Worked on a copy, written, and only then kept (as edit_apply does).
    let mut working = workspace.document(&path)?.doc.clone();
    let before = working.revision();
    let mut failure = None;
    for _ in 0..steps {
        let step = if action == "undo" {
            working.undo(actor.clone(), &registry)
        } else {
            working.redo(actor.clone(), &registry)
        };
        match step {
            Ok(Some(applied)) => {
                changes.extend(applied.changes);
                done += 1;
            }
            Ok(None) => break,
            Err(problem) => {
                failure = Some(problem);
                break;
            }
        }
    }
    if let Some(problem) = failure
        && done == 0
    {
        return Err(problem);
    }
    out.insert("revision".to_owned(), json!(before + done));
    if done > 0 && !dry_run {
        workspace.commit_document(&path, working)?;
        workspace.mark_seen(&path, actor);
    }
    changes.sort();
    changes.dedup();
    out.insert("action".to_owned(), json!(action));
    out.insert("steps_done".to_owned(), json!(done));
    if done == 0 {
        out.insert(
            "diagnostics".to_owned(),
            json!([Diagnostic::info(
                "nothing_to_do",
                format!("There is nothing to {action}."),
                "Use action \"list\" to see the history.",
            )]),
        );
    }
    out.insert("changes".to_owned(), changes_json(&changes));
    if dry_run {
        out.insert("dry_run".to_owned(), json!(true));
    }
    if let Some(others) = others {
        out.insert("changed_by_others".to_owned(), others);
    }
    if done > 0 {
        finish(&mut out, notes, workspace, &path);
    }
    Ok(out)
}

/// A note for reading the scene file while play sessions are open: the
/// values are the file's, not the live world's (M7 friction log, F3).
fn scene_not_session(workspace: &Workspace) -> Option<Diagnostic> {
    let open: Vec<String> = workspace
        .sessions
        .open
        .iter()
        .map(|(id, session)| format!("{id} (tick {})", session.app.tick()))
        .collect();
    (!open.is_empty()).then(|| {
        Diagnostic::info(
            "scene_not_session",
            format!(
                "These values are the scene file's, not a play session's; open: {}.",
                open.join(", ")
            ),
            "Pass \"session\" to read the live world (sim_step and the other session tools use the only open session by themselves; scene tools do not).",
        )
    })
}
