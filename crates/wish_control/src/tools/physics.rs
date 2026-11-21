//! `physics_query` (design note M6 §3.4): what a ray hits, what overlaps a
//! shape, what touches an entity, in a scene or a play session.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use wish_core::math::{Quat, Vec3};
use wish_core::{Diagnostic, Entity, Result};
use wish_physics::{OverlapShape, PhysicsProfile, query_state};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::sessions::entity_names;
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const PHYSICS_QUERY: Tool = Tool {
    name: "physics_query",
    title: "Ask the physics world",
    description: "Asks the physics world one question, in a play session (the world as of its last step) or in a scene file (colliders where the scene puts them). Give exactly one of: ray {from: [x,y,z], dir: [x,y,z], max (m, default 100), ignore: id} → the first collider hit, with point, normal and distance (sensors are not hit); overlap {sphere: radius | box: [half x, half y, half z], at: [x,y,z]} → every entity whose collider overlaps (sensors too); touching: id → the entities in contact with it or overlapping it. mask limits hits to colliders whose layer has one of its bits. Use it to check what is under a character, whether a path is clear, or why something does not fall. A character controller's collider is found where it stood before its last move (physics places characters after its step). When PhysicsSettings.profile is 2D (TopDown2D or SideView2D) every question is asked in the X-Y plane: positions and directions may be [x, y], z is not used, {circle: radius} is the same as sphere, and the answer names the profile. Line of sight in the X-Y plane, against walls (solid tiles of tilemaps' colliding layers and entities with an Occluder and a Collider): sight {from: [x, y], to: [x, y], ignore?: id} → visible, and blocked_by (the first wall's entity) when not; sight {from: [x, y], radius: r} → points, the outline of what is visible from there (a circle cut by walls, counter-clockwise). These are the answers game code's can_see/visible_area, online play's NetVisibility::Sight and the shadows of Light2D use.",
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
            "A shape: {\"sphere\": radius} or {\"box\": [hx, hy, hz]}, with \"at\": [x, y, z] (in 2D also {\"circle\": radius}, [hx, hy] and [x, y]).",
        ),
        Param::optional(
            "touching",
            Kind::String,
            "An entity id: what touches or overlaps it.",
        ),
        Param::optional(
            "sight",
            Kind::Object,
            "Line of sight in the X-Y plane: {\"from\": [x, y], \"to\": [x, y], \"ignore\": \"entity id\"} (can it be seen?) or {\"from\": [x, y], \"radius\": meters} (what is visible from there?).",
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

/// Three numbers; in a 2D physics profile (`flat`) two are enough (z is not
/// used there).
fn vec3(value: Option<&Value>, path: &str, flat: bool) -> Result<Vec3> {
    let numbers: Option<Vec<f32>> = value
        .and_then(Value::as_array)
        .and_then(|items| items.iter().map(|n| n.as_f64().map(|n| n as f32)).collect());
    match numbers.as_deref() {
        Some([x, y, z]) => Ok(Vec3::new(*x, *y, *z)),
        Some([x, y]) if flat => Ok(Vec3::new(*x, *y, 0.0)),
        _ if flat => Err(bad(
            path,
            format!("'{path}' must be two numbers [x, y] (or three, [x, y, z])."),
            "The physics profile is 2D: write positions and directions in the X-Y plane as [x, y] in meters (Y is up); a z is not used.",
        )),
        _ => Err(bad(
            path,
            format!("'{path}' must be three numbers [x, y, z]."),
            "Write positions and directions as [x, y, z] in meters (Y is up).",
        )),
    }
}

fn physics_query(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let asked: Vec<&str> = ["ray", "overlap", "touching", "sight"]
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
            "Give exactly one of ray, overlap, touching or sight.",
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
    // A scene with tilemaps is asked in a world of its own that knows their
    // walls as they are on disk now.
    let with_tiles;
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
            let shapes = workspace.tile_shapes();
            if shapes.is_empty() {
                let view = workspace.view(&path)?;
                (&view.world, view.ids.clone())
            } else {
                let doc = workspace.document(&path)?.doc.clone();
                let mut view = workspace.build_view(&doc)?;
                wish_physics::install_tile_shapes(&mut view.world, shapes);
                with_tiles = view;
                (&with_tiles.world, with_tiles.ids.clone())
            }
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
    if let Some(sight) = args.value("sight") {
        sight_question(world, sight, &find, &name, &mut out)?;
        return Ok(out);
    }
    let state = query_state(world)?;
    let flat = state.profile() != PhysicsProfile::Full3D;
    if flat {
        // The answer says how the question was read: in the plane.
        out.insert("profile".to_owned(), json!(profile_name(state.profile())));
    }
    if let Some(ray) = args.value("ray") {
        let from = vec3(ray.get("from"), "/ray/from", flat)?;
        let dir = vec3(ray.get("dir"), "/ray/dir", flat)?;
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
        let at = vec3(overlap.get("at"), "/overlap/at", flat)?;
        // A circle is the 2D word for the same question as a sphere.
        let round = match (overlap.get("sphere"), overlap.get("circle")) {
            (Some(radius), None) => Some(("sphere", radius)),
            (None, Some(radius)) => Some(("circle", radius)),
            (None, None) => None,
            (Some(_), Some(_)) => {
                return Err(bad(
                    "/overlap",
                    "An overlap needs exactly one of 'sphere', 'circle' or 'box'.".to_owned(),
                    "'circle' and 'sphere' are the same question (a circle in 2D): give one.",
                ));
            }
        };
        let shape = match (round, overlap.get("box")) {
            (Some((name, radius)), None) => {
                OverlapShape::Sphere(radius.as_f64().ok_or_else(|| {
                    bad(
                        &format!("/overlap/{name}"),
                        format!("'{name}' must be a radius in meters."),
                        "Write it like {\"sphere\": 0.5, \"at\": [0, 1, 0]} (in 2D: {\"circle\": 0.5, \"at\": [0, 1]}).",
                    )
                })? as f32)
            }
            (None, Some(half)) => OverlapShape::Box(vec3(Some(half), "/overlap/box", flat)?),
            _ => {
                return Err(bad(
                    "/overlap",
                    "An overlap needs exactly one of 'sphere', 'circle' or 'box'.".to_owned(),
                    "Write {\"sphere\": radius, \"at\": [x, y, z]} or {\"box\": [hx, hy, hz], \"at\": [x, y, z]} (in 2D: {\"circle\": radius, \"at\": [x, y]} or {\"box\": [hx, hy], \"at\": [x, y]}).",
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

/// Two numbers `[x, y]` (a third, z, is allowed and not used).
fn point(value: Option<&Value>, path: &str) -> Result<[f32; 2]> {
    let numbers: Option<Vec<f32>> = value
        .and_then(Value::as_array)
        .and_then(|items| items.iter().map(|n| n.as_f64().map(|n| n as f32)).collect());
    match numbers.as_deref() {
        Some([x, y]) | Some([x, y, _]) => Ok([*x, *y]),
        _ => Err(bad(
            path,
            format!("'{path}' must be two numbers [x, y]."),
            "Line of sight is asked in the X-Y plane: write points as [x, y] in meters (Y is up).",
        )),
    }
}

/// A number rounded to millimetres, for an answer.
fn mm(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

/// `physics_query sight`: can a point be seen from another, or what is
/// visible from a point (design note 1.1-lights-2d §7).
fn sight_question(
    world: &wish_ecs::World,
    sight: &Value,
    find: &dyn Fn(&str, &str) -> Result<Entity>,
    name: &dyn Fn(&Entity) -> String,
    out: &mut Map<String, Value>,
) -> Result<()> {
    let from = point(sight.get("from"), "/sight/from")?;
    let walls = wish_sight::current(world);
    if walls.left_out() > 0 {
        out.insert(
            "diagnostics".to_owned(),
            json!([Diagnostic::warning(
                "sight_occluders_cut_short",
                format!(
                    "The world has more than {} wall segments; {} were left out, so some walls do not block sight.",
                    wish_sight::MAX_SEGMENTS,
                    walls.left_out()
                ),
                "Fewer occluders, or simpler tilemap walls: solid cells merge into boxes, so long straight walls cost little.",
            )]),
        );
    }
    match (sight.get("to"), sight.get("radius")) {
        (Some(to), None) => {
            let to = point(Some(to), "/sight/to")?;
            let ignore = match sight.get("ignore").and_then(Value::as_str) {
                Some(id) => Some(find(id, "/sight/ignore")?),
                None => None,
            };
            let seen = walls.can_see(from, to, [ignore, None]);
            out.insert("visible".to_owned(), json!(seen.visible));
            if let Some(blocker) = seen.blocked_by {
                out.insert("blocked_by".to_owned(), json!(name(&blocker)));
            }
        }
        (None, Some(radius)) => {
            let radius = radius.as_f64().filter(|r| *r > 0.0).ok_or_else(|| {
                bad(
                    "/sight/radius",
                    "'radius' must be a distance in meters above 0.".to_owned(),
                    "Write it like {\"from\": [2, 3], \"radius\": 6}.",
                )
            })? as f32;
            let area = walls.visible_polygon(from, radius);
            out.insert(
                "points".to_owned(),
                json!(
                    area.points
                        .iter()
                        .map(|[x, y]| [mm(*x), mm(*y)])
                        .collect::<Vec<_>>()
                ),
            );
            out.insert("walls".to_owned(), json!(walls.segments().len()));
        }
        _ => {
            return Err(bad(
                "/sight",
                "A sight question needs exactly one of 'to' or 'radius'.".to_owned(),
                "Ask {\"from\": [x, y], \"to\": [x, y]} (can it be seen?) or {\"from\": [x, y], \"radius\": meters} (what is visible from there?).",
            ));
        }
    }
    Ok(())
}

/// A profile's name as scene files write it.
fn profile_name(profile: PhysicsProfile) -> &'static str {
    match profile {
        PhysicsProfile::Full3D => "Full3D",
        PhysicsProfile::TopDown2D => "TopDown2D",
        PhysicsProfile::SideView2D => "SideView2D",
    }
}
