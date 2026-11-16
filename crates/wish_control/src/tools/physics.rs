//! `physics_query` (design note M6 §3.4): what a ray hits, what overlaps a
//! shape, what touches an entity, in a scene or a play session.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use wish_core::math::{Quat, Vec3};
use wish_core::{Diagnostic, Entity, Result};
use wish_physics::{OverlapShape, query_state};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::sessions::entity_names;
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PHYSICS_QUERY: Tool = Tool {
    name: "physics_query",
    title: "Ask the physics world",
    description: "Asks the physics world one question, in a play session (the world as of its last step) or in a scene file (colliders where the scene puts them). Give exactly one of: ray {from: [x,y,z], dir: [x,y,z], max (m, default 100), ignore: id} → the first collider hit, with point, normal and distance (sensors are not hit); overlap {sphere: radius | box: [half x, half y, half z], at: [x,y,z]} → every entity whose collider overlaps (sensors too); touching: id → the entities in contact with it or overlapping it. mask limits hits to colliders whose layer has one of its bits. Use it to check what is under a character, whether a path is clear, or why something does not fall. A character controller's collider is found where it stood before its last move (physics places characters after its step).",
    params: &[
        Param::optional(
            "scene",
            Kind::String,
            "The scene file to ask about (default: the project's scene). Ignored when session is given.",
        ),
        Param::optional(
            "session",
            Kind::String,
            "Ask about this play session's live world instead of the scene file.",
        ),
        Param::optional(
            "ray",
            Kind::Object,
            "A ray: {\"from\": [x, y, z], \"dir\": [x, y, z], \"max\": meters, \"ignore\": \"entity id\"}.",
        ),
        Param::optional(
            "overlap",
            Kind::Object,
            "A shape: {\"sphere\": radius} or {\"box\": [hx, hy, hz]}, with \"at\": [x, y, z].",
        ),
        Param::optional(
            "touching",
            Kind::String,
            "An entity id: what touches or overlaps it.",
        ),
        Param::optional(
            "mask",
            Kind::Integer {
                min: 0,
                max: 4_294_967_295,
            },
            "Only colliders whose layer shares a bit with this mask (default: all).",
        ),
    ],
    mutates: false,
    run: physics_query,
};

fn bad(path: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("invalid_parameter", message, hint).at_path(path)
}

fn vec3(value: Option<&Value>, path: &str) -> Result<Vec3> {
    let numbers: Option<Vec<f32>> = value
        .and_then(Value::as_array)
        .and_then(|items| items.iter().map(|n| n.as_f64().map(|n| n as f32)).collect());
    match numbers.as_deref() {
        Some([x, y, z]) => Ok(Vec3::new(*x, *y, *z)),
        _ => Err(bad(
            path,
            format!("'{path}' must be three numbers [x, y, z]."),
            "Write positions and directions as [x, y, z] in meters (Y is up).",
        )),
    }
}

fn physics_query(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let asked: Vec<&str> = ["ray", "overlap", "touching"]
        .into_iter()
        .filter(|name| args.value(name).is_some())
        .collect();
    if asked.len() != 1 {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!(
                "physics_query asks one question at a time; it got {}.",
                if asked.is_empty() {
                    "none".to_owned()
                } else {
                    asked.join(", ")
                }
            ),
            "Give exactly one of ray, overlap or touching.",
        ));
    }
    let mask = args
        .int("mask")
        .map_or(u32::MAX, |m| u32::try_from(m).unwrap_or(u32::MAX));
    let mut out = Map::new();
    let session = match args.str("session") {
        Some(id) => Some(workspace.session_id(Some(id))?),
        None => None,
    };
    let (world, ids) = match &session {
        Some(id) => {
            let session = workspace.session(id)?;
            out.insert("session".to_owned(), json!(id));
            out.insert("tick".to_owned(), json!(session.app.tick()));
            let world = session.app.world();
            (world, entity_names(world))
        }
        None => {
            let path = workspace.scene_path(args.str("scene"))?;
            workspace.open_document(&path, false)?;
            out.insert("scene".to_owned(), json!(path));
            let view = workspace.view(&path)?;
            (&view.world, view.ids.clone())
        }
    };
    let names: BTreeMap<Entity, String> = ids.iter().map(|(n, e)| (*e, n.clone())).collect();
    let name = |entity: &Entity| {
        names
            .get(entity)
            .cloned()
            .unwrap_or_else(|| entity.to_string())
    };
    let find = |id: &str, path: &str| -> Result<Entity> {
        ids.get(id).copied().ok_or_else(|| {
            Diagnostic::error(
                "entity_not_found",
                format!("There is no entity '{id}'."),
                wish_core::suggest::unknown_name_hint(
                    id,
                    &ids.keys().map(String::as_str).collect::<Vec<_>>(),
                ),
            )
            .at_path(path)
        })
    };
    let state = query_state(world)?;
    if let Some(ray) = args.value("ray") {
        let from = vec3(ray.get("from"), "/ray/from")?;
        let dir = vec3(ray.get("dir"), "/ray/dir")?;
        let max = ray.get("max").and_then(Value::as_f64).unwrap_or(100.0) as f32;
        let ignore = match ray.get("ignore").and_then(Value::as_str) {
            Some(id) => Some(find(id, "/ray/ignore")?),
            None => None,
        };
        let hit = state
            .raycast(from, dir, max, mask, ignore)
            .map_err(|e| e.at_path("/ray"))?;
        out.insert(
            "hit".to_owned(),
            hit.map_or(Value::Null, |hit| {
                json!({
                    "id": name(&hit.entity),
                    "point": [hit.point.x, hit.point.y, hit.point.z],
                    "normal": [hit.normal.x, hit.normal.y, hit.normal.z],
                    "distance": hit.distance,
                })
            }),
        );
    } else if let Some(overlap) = args.value("overlap") {
        let at = vec3(overlap.get("at"), "/overlap/at")?;
        let shape = match (overlap.get("sphere"), overlap.get("box")) {
            (Some(radius), None) => OverlapShape::Sphere(radius.as_f64().ok_or_else(|| {
                bad(
                    "/overlap/sphere",
                    "'sphere' must be a radius in meters.".to_owned(),
                    "Write it like {\"sphere\": 0.5, \"at\": [0, 1, 0]}.",
                )
            })? as f32),
            (None, Some(half)) => OverlapShape::Box(vec3(Some(half), "/overlap/box")?),
            _ => {
                return Err(bad(
                    "/overlap",
                    "An overlap needs exactly one of 'sphere' or 'box'.".to_owned(),
                    "Write {\"sphere\": radius, \"at\": [x, y, z]} or {\"box\": [hx, hy, hz], \"at\": [x, y, z]}.",
                ));
            }
        };
        let found = state
            .overlap(shape, at, Quat::IDENTITY, mask)
            .map_err(|e| e.at_path("/overlap"))?;
        out.insert(
            "ids".to_owned(),
            json!(found.iter().map(name).collect::<Vec<_>>()),
        );
    } else if let Some(id) = args.str("touching") {
        let entity = find(id, "/touching")?;
        out.insert(
            "ids".to_owned(),
            json!(state.touching(entity).iter().map(name).collect::<Vec<_>>()),
        );
    }
    Ok(out)
}
