//! `tilemap_edit`: painting a tilemap file (design note
//! `docs/design/1.1-tilemaps.md` §5).

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const TILEMAP_EDIT: Tool = Tool {
    name: "tilemap_edit",
    title: "Paint a tilemap",
    description: "Paints a tilemap file (*.tilemap.json). Its layers hold tiles of a tileset in cells (x right, y up from the map's origin); an entity shows it with a Tilemap {map}. ops, all or nothing, one change in the map's history: set {layer, cells: [[x, y, tile], ...]} (tile -1 erases), rect {layer, from: [x, y], to: [x, y], tile} (both corners included), fill {layer, at: [x, y], tile} (the connected cells holding what at holds, inside the painted part of the map), erase {layer, from, to}, add_layer {layer, collide?, before?}, remove_layer {layer}, set_layer {layer, collide?, name?}. Tiles are numbers of the tileset (asset_info on it lists them and which are solid); a layer's solid tiles are walls for physics unless its collide is false; later layers are drawn over earlier ones. The file is written by this call. Answers the revision, the cells changed per layer, their box and the layers; edit_history with the tilemap as scene lists, undoes, redoes or takes back a change, as for a scene. action get only looks: the layers, the box of each, and with from and to the tiles in that rectangle as rows, the top row first (at most 64 x 64).",
    params: &[
        Param::required(
            "tilemap",
            Kind::String,
            "The tilemap file, project-relative, such as \"maps/station.tilemap.json\".",
        ),
        Param::optional(
            "action",
            Kind::Choice(&["paint", "get"]),
            "paint applies ops; get reads the map.",
        )
        .or("\"paint\""),
        Param::optional(
            "ops",
            Kind::Array,
            "For paint: the operations, each like { \"op\": \"rect\", \"layer\": \"walls\", \"from\": [0, 0], \"to\": [9, 0], \"tile\": 8 }.",
        ),
        Param::optional(
            "from",
            Kind::Array,
            "For get: one corner [x, y] of the rectangle of cells to return.",
        ),
        Param::optional(
            "to",
            Kind::Array,
            "For get: the other corner [x, y] (included).",
        ),
        Param::optional(
            "base_revision",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "Refuse if the map has changed since this revision (from an earlier answer).",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Check and report what would change, without changing anything.",
        ),
        Param::optional(
            "reason",
            Kind::String,
            "Why, in a sentence (shown on this change's card in the editor's activity feed, and in edit_history).",
        ),
    ],
    mutates: true,
    run: tilemap_edit,
};

/// A cell `[x, y]` of the arguments.
fn corner(args: &Args, name: &str) -> Result<Option<(i32, i32)>> {
    let Some(value) = args.value(name) else {
        return Ok(None);
    };
    let pair = value
        .as_array()
        .filter(|pair| pair.len() == 2)
        .and_then(|pair| Some((pair[0].as_i64()?, pair[1].as_i64()?)))
        .and_then(|(x, y)| Some((i32::try_from(x).ok()?, i32::try_from(y).ok()?)));
    pair.map(Some).ok_or_else(|| {
        Diagnostic::error(
            "invalid_parameter",
            format!("\"{name}\" must be a cell [x, y]: two whole numbers."),
            "Write it like [0, 0].",
        )
        .at_path(format!("/{name}"))
    })
}

fn tilemap_edit(
    workspace: &mut Workspace,
    args: &Args,
    actor: &Actor,
) -> Result<Map<String, Value>> {
    let path = crate::tilemaps::normalize_map_path(
        workspace.root(),
        args.str("tilemap").unwrap_or_default(),
        "/tilemap",
    )?;
    if args.str("action") == Some("get") {
        let region = match (corner(args, "from")?, corner(args, "to")?) {
            (Some(from), Some(to)) => Some((from, to)),
            (None, None) => None,
            _ => {
                return Err(Diagnostic::error(
                    "invalid_parameter",
                    "get needs both corners of the rectangle, or neither.",
                    "Give \"from\": [x, y] and \"to\": [x, y], or leave both out for the layers only.",
                )
                .at_path("/to"));
            }
        };
        return crate::tilemaps::get(workspace, &path, region);
    }
    let ops = match args.value("ops") {
        Some(Value::Array(ops)) => ops.clone(),
        _ => {
            return Err(Diagnostic::error(
                "missing_parameter",
                "paint needs \"ops\": the operations to apply.",
                "Give \"ops\": [{ \"op\": \"set\", \"layer\": \"floor\", \"cells\": [[0, 0, 1]] }].",
            )
            .at_path("/ops"));
        }
    };
    crate::tilemaps::paint(
        workspace,
        &path,
        &ops,
        (actor, args.str("reason").map(str::to_owned)),
        (
            args.flag("dry_run"),
            args.int("base_revision")
                .and_then(|r| u64::try_from(r).ok()),
        ),
    )
}
