//! Tilemaps through the tools (release 1.1, package 5, design note
//! `docs/design/1.1-tilemaps.md`): tilesets and tilemaps made, listed,
//! reported, checked, moved and kept by checkpoints; painted with
//! `tilemap_edit` (every operation, all or nothing, dry runs and limits);
//! one history per map with actors, undo, redo, taking one change back and
//! changes made on disk; walls that stop a character in a session and that
//! a replay notices; what `describe_view` says.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::project::ProjectKind;
use wish_control::scaffold::create_project_of_kind;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

const TILESET: &str = "assets/tiles/station.tileset.json";
const MAP: &str = "maps/room.tilemap.json";

fn call_as(ws: &mut Workspace, actor: &str, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor(actor));
    assert_eq!(answer["ok"], true, "{tool}: {answer}");
    answer
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    call_as(ws, "agent", tool, args)
}

fn fails(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    let answer = ws.call(tool, &args, &parse_actor("agent"));
    assert_eq!(answer["ok"], false, "{tool}: {answer}");
    answer["error"].clone()
}

fn codes(answer: &Value) -> Vec<String> {
    answer["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| d["code"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// A new project with a 32 × 8 picture cut into four 8 × 8 tiles (tile 1
/// a solid wall, tile 2 a solid slope) and an empty map of two layers.
fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tilemap-tools-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    create_project_of_kind(&dir, "Tiles", "wen", ProjectKind::D2).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/tiles/station.png",
                "picture": { "width": 32, "height": 8, "color": [0.6, 0.6, 0.7, 1] } }),
    );
    call(
        &mut ws,
        "asset_create",
        json!({ "path": TILESET, "tileset": {
            "image": "assets/tiles/station.png",
            "grid": { "frame": [8, 8], "columns": 4, "rows": 1 },
            "tiles": {
                "1": { "solid": true, "name": "wall" },
                "2": { "solid": true, "shape": [[0, 0], [1, 0], [1, 1]] }
            } } }),
    );
    call(
        &mut ws,
        "asset_create",
        json!({ "path": MAP, "tilemap": { "tileset": TILESET, "layers": ["floor", "walls"] } }),
    );
    (dir, ws)
}

/// A room of 12 × 8 cells: walls of tile 1 around a floor of tile 0, a door
/// of two cells in the south wall.
fn paint_room(ws: &mut Workspace) -> Value {
    call(
        ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "reason": "the room", "ops": [
            { "op": "rect", "layer": "walls", "from": [0, 0], "to": [11, 7], "tile": 1 },
            { "op": "erase", "layer": "walls", "from": [1, 1], "to": [10, 6] },
            { "op": "erase", "layer": "walls", "from": [5, 0], "to": [6, 0] },
            { "op": "rect", "layer": "floor", "from": [0, 0], "to": [11, 7], "tile": 0 }
        ] }),
    )
}

#[test]
fn tilesets_and_tilemaps_are_made_listed_reported_and_checked() {
    let (dir, mut ws) = project("made");
    let text = std::fs::read_to_string(dir.join(MAP)).unwrap();
    assert_eq!(
        text,
        "{\n  \"format\": 1,\n  \"layers\": [\n    {\n      \"name\": \"floor\"\n    },\n    {\n      \"name\": \"walls\"\n    }\n  ],\n  \"tileset\": \"assets/tiles/station.tileset.json\"\n}\n"
    );
    // A dry run checks and writes nothing; mistakes point into the spec.
    let dry = call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/tiles/other.tileset.json", "dry_run": true,
                "tileset": { "image": "assets/tiles/station.png", "grid": { "frame": [8, 8], "columns": 2, "rows": 1 } } }),
    );
    assert_eq!(dry["tiles"], 2, "{dry}");
    assert!(!dir.join("assets/tiles/other.tileset.json").exists());
    let wrong = fails(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/tiles/bad.tileset.json",
                "tileset": { "image": "assets/tiles/station.png", "grid": { "frame": [8, 8], "columns": 2, "rows": 1 },
                             "tiles": { "5": { "solid": true } } } }),
    );
    assert_eq!(wrong["code"], "value_out_of_range");
    assert_eq!(wrong["location"]["path"], "/tileset/tiles/5");
    let wrong = fails(
        &mut ws,
        "asset_create",
        json!({ "path": "maps/bad.tilemap.json", "tilemap": { "tileset": "assets/tiles/station.png" } }),
    );
    assert_eq!(wrong["code"], "not_a_tileset_file");
    // Listed by kind, and reported.
    let listed = call(&mut ws, "asset_list", json!({ "kind": "tilemap" }));
    assert_eq!(listed["assets"][0]["path"], MAP, "{listed}");
    let listed = call(&mut ws, "asset_list", json!({ "kind": "tileset" }));
    assert_eq!(listed["assets"][0]["path"], TILESET, "{listed}");
    paint_room(&mut ws);
    let info = call(&mut ws, "asset_info", json!({ "path": TILESET }));
    assert_eq!(info["tileset"]["tile_count"], 4, "{info}");
    assert_eq!(info["tileset"]["properties"]["1"]["name"], "wall");
    assert_eq!(info["used_by"], json!([MAP]), "{info}");
    let info = call(&mut ws, "asset_info", json!({ "path": MAP }));
    assert_eq!(info["tilemap"]["box"], json!([0, 0, 11, 7]), "{info}");
    assert_eq!(info["tilemap"]["layers"][1]["tiles"], 2 * 12 + 2 * 6 - 2);
    assert_eq!(info["tilemap"]["layers"][0]["tiles"], 96);
    // check is clean; then a tile the tileset lacks, a broken row, a
    // Tilemap that names a picture, a file not in canonical form.
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        !checked.to_string().contains("tilemap") || codes(&checked).is_empty(),
        "{checked}"
    );
    let painted = std::fs::read_to_string(dir.join(MAP)).unwrap();
    std::fs::write(dir.join(MAP), painted.replacen("[0, 0, 0,", "[9, 0, 0,", 1)).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"tile_not_in_tileset".to_owned()),
        "{checked}"
    );
    std::fs::write(dir.join(MAP), painted.replacen("[0, 0, 0,", "[0, 0,", 1)).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    let broken = checked["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "wrong_length")
        .unwrap_or_else(|| panic!("{checked}"));
    assert_eq!(broken["location"]["file"], MAP);
    assert!(broken["location"]["line"].as_u64().unwrap() > 1);
    std::fs::write(dir.join(MAP), painted.replace("\n", "\n  ")).unwrap();
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"not_canonical".to_owned()),
        "{checked}"
    );
    std::fs::write(dir.join(MAP), &painted).unwrap();
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "map", "components": {
            "Tilemap": { "map": "assets/tiles/station.png" } } }] }),
    );
    let checked = call(&mut ws, "check", json!({}));
    assert!(
        codes(&checked).contains(&"not_a_tilemap_file".to_owned()),
        "{checked}"
    );
}

#[test]
fn painting_is_all_or_nothing_with_dry_runs_and_limits() {
    let (dir, mut ws) = project("paint");
    let room = paint_room(&mut ws);
    assert_eq!(room["revision"], 1, "{room}");
    assert_eq!(room["changed"]["walls"], 2 * 12 + 2 * 6 - 2);
    assert_eq!(room["changed"]["floor"], 96);
    assert_eq!(room["box"], json!([0, 0, 11, 7]));
    let written = std::fs::read_to_string(dir.join(MAP)).unwrap();
    // The bottom row of chunk 0,0 is the file's last row of it: the door.
    assert!(
        written.contains("[1, 1, 1, 1, 1, -1, -1, 1, 1, 1, 1, 1, -1, -1, -1, -1]"),
        "{written}"
    );
    // A fill inside the room on the walls layer reaches the inside and out
    // through the door: every empty cell of the map's box.
    let filled = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "dry_run": true, "ops": [
            { "op": "fill", "layer": "walls", "at": [3, 3], "tile": 3 }] }),
    );
    assert_eq!(filled["cells_changed"], 10 * 6 + 2, "{filled}");
    assert_eq!(filled["dry_run"], true);
    assert_eq!(std::fs::read_to_string(dir.join(MAP)).unwrap(), written);
    // set, set_layer, add_layer, remove_layer.
    let more = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [
            { "op": "set", "layer": "walls", "cells": [[3, 3, 2], [4, 3, 2], [5, 0, -1]] },
            { "op": "add_layer", "layer": "props", "collide": false, "before": "walls" },
            { "op": "set", "layer": "props", "cells": [[2, 2, 3]] },
            { "op": "set_layer", "layer": "floor", "name": "ground" }
        ] }),
    );
    assert_eq!(more["changed"]["walls"], 2, "{more}");
    let names: Vec<&str> = more["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ground", "props", "walls"]);
    let got = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "action": "get", "from": [2, 4], "to": [5, 2] }),
    );
    assert_eq!(
        got["rows"]["walls"],
        json!([[-1, -1, -1, -1], [-1, 2, 2, -1], [-1, -1, -1, -1]]),
        "{got}"
    );
    assert_eq!(got["rows"]["props"][2][0], 3);
    assert_eq!(got["revision"], 2);
    // All or nothing: a later mistake leaves the file and the revision.
    let before = std::fs::read_to_string(dir.join(MAP)).unwrap();
    let wrong = fails(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [
            { "op": "rect", "layer": "walls", "from": [0, 0], "to": [3, 3], "tile": 0 },
            { "op": "set", "layer": "wals", "cells": [[0, 0, 1]] }] }),
    );
    assert_eq!(wrong["code"], "layer_not_found");
    assert_eq!(wrong["location"]["path"], "/ops/1/layer");
    assert!(wrong["hint"].as_str().unwrap().contains("walls"));
    assert_eq!(std::fs::read_to_string(dir.join(MAP)).unwrap(), before);
    for (ops, code) in [
        (
            json!([{ "op": "set", "layer": "walls", "cells": [[0, 0, 7]] }]),
            "unknown_tile",
        ),
        (
            json!([{ "op": "fill", "layer": "walls", "at": [40, 40], "tile": 1 }]),
            "fill_unbounded",
        ),
        (
            json!([{ "op": "rect", "layer": "walls", "from": [0, 0], "to": [300, 300], "tile": 1 }]),
            "paint_too_large",
        ),
        (
            json!([{ "op": "paint", "layer": "walls" }]),
            "invalid_parameter",
        ),
        (
            json!([{ "op": "rect", "layer": "walls", "from": [0], "to": [1, 1], "tile": 1 }]),
            "invalid_parameter",
        ),
    ] {
        let wrong = fails(
            &mut ws,
            "tilemap_edit",
            json!({ "tilemap": MAP, "ops": ops }),
        );
        assert_eq!(wrong["code"], code, "{wrong}");
        assert!(!wrong["hint"].as_str().unwrap().is_empty());
    }
    let stale = fails(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "base_revision": 1, "ops": [{ "op": "set", "layer": "walls", "cells": [[0, 0, -1]] }] }),
    );
    assert_eq!(stale["code"], "stale_revision");
    let missing = fails(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": "maps/none.tilemap.json", "action": "get" }),
    );
    assert_eq!(missing["code"], "tilemap_not_found");
    // Painting what is there already changes nothing and writes nothing.
    let same = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "set", "layer": "walls", "cells": [[0, 0, 1]] }] }),
    );
    assert_eq!(same["cells_changed"], 0);
    assert_eq!(same["revision"], 2);
    assert_eq!(std::fs::read_to_string(dir.join(MAP)).unwrap(), before);
    // The file reads back as written.
    let reread = wish_asset::read_tilemap(&dir, MAP).unwrap();
    assert_eq!(reread.to_canonical().unwrap(), before);
}

#[test]
fn a_map_has_one_history_with_actors_undo_redo_and_revert() {
    let (dir, mut ws) = project("history");
    paint_room(&mut ws);
    call_as(
        &mut ws,
        "human",
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "set", "layer": "walls", "cells": [[3, 3, 1]] }] }),
    );
    call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "set", "layer": "walls", "cells": [[8, 3, 1]] }] }),
    );
    let listed = call(&mut ws, "edit_history", json!({ "scene": MAP }));
    let entries = listed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{listed}");
    assert_eq!(
        (entries[1]["actor"].clone(), entries[1]["cells"].clone()),
        (json!("human"), json!(1))
    );
    assert_eq!(entries[2]["reason"], "the room");
    assert_eq!(entries[0]["revertible"], true);
    assert_eq!(listed["next_undo_by"], "agent:agent");
    // The feed carries the map and the revision, so a card can take it back.
    let feed = call(&mut ws, "activity_get", json!({}));
    let card = feed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|e| e["tool"] == "tilemap_edit")
        .unwrap()
        .clone();
    assert_eq!(
        (card["scene"].clone(), card["revision"].clone()),
        (json!(MAP), json!(3)),
        "{card}"
    );
    // The person's stroke is taken back on its own; the agent's stays.
    let reverted = call_as(
        &mut ws,
        "human",
        "edit_history",
        json!({ "scene": MAP, "action": "revert", "revision": 2 }),
    );
    assert_eq!(reverted["revision"], 4, "{reverted}");
    let got = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "action": "get", "from": [3, 3], "to": [8, 3] }),
    );
    assert_eq!(
        got["rows"]["walls"][0],
        json!([-1, -1, -1, -1, -1, 1]),
        "{got}"
    );
    // The room cannot be taken back alone: later strokes painted over it?
    // Not over its cells: (8, 3) was empty in the room. But the revert of
    // the room would erase walls the history kept: allowed, as nothing
    // later touched them. A stroke over a wall makes it conflict.
    call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "set", "layer": "walls", "cells": [[0, 0, 3]] }] }),
    );
    let conflict = fails(
        &mut ws,
        "edit_history",
        json!({ "scene": MAP, "action": "revert", "revision": 1 }),
    );
    assert_eq!(conflict["code"], "revert_conflict", "{conflict}");
    // Undo twice, redo once.
    let undone = call(
        &mut ws,
        "edit_history",
        json!({ "scene": MAP, "action": "undo", "steps": 2 }),
    );
    assert_eq!(undone["steps_done"], 2, "{undone}");
    let got = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "action": "get", "from": [0, 0], "to": [3, 3] }),
    );
    assert_eq!(
        got["rows"]["walls"][3][0], 1,
        "the stroke over the wall is undone: {got}"
    );
    assert_eq!(got["rows"]["walls"][0][3], 1, "the revert is undone: {got}");
    call(
        &mut ws,
        "edit_history",
        json!({ "scene": MAP, "action": "redo" }),
    );
    let got = call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "action": "get", "from": [3, 3], "to": [3, 3] }),
    );
    assert_eq!(
        got["rows"]["walls"],
        json!([[-1]]),
        "the revert is back: {got}"
    );
    // A change made on disk enters the history as the file's.
    let text = std::fs::read_to_string(dir.join(MAP)).unwrap();
    let mut map = wish_asset::parse_tilemap(&text, MAP).unwrap();
    map.set(1, 9, 9, 2).unwrap();
    std::fs::write(dir.join(MAP), map.to_canonical().unwrap()).unwrap();
    let listed = call(&mut ws, "edit_history", json!({ "scene": MAP }));
    assert_eq!(listed["entries"][0]["actor"], "file", "{listed}");
    assert_eq!(listed["entries"][0]["cells"], 1);
    // A file broken by hand is not overwritten.
    std::fs::write(dir.join(MAP), "{ \"format\": 1, \"tileset\": ").unwrap();
    let refused = fails(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "set", "layer": "walls", "cells": [[0, 0, 1]] }] }),
    );
    assert_eq!(refused["code"], "tilemap_file_invalid");
    let refused = fails(
        &mut ws,
        "edit_history",
        json!({ "scene": MAP, "action": "undo" }),
    );
    assert_eq!(refused["code"], "tilemap_file_invalid");
    assert_eq!(
        std::fs::read_to_string(dir.join(MAP)).unwrap(),
        "{ \"format\": 1, \"tileset\": "
    );
}

#[test]
fn moves_rewrite_references_and_checkpoints_keep_maps() {
    let (dir, mut ws) = project("move");
    paint_room(&mut ws);
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "map", "components": {
            "Transform": {}, "Tilemap": { "map": MAP } } }] }),
    );
    call(
        &mut ws,
        "checkpoint",
        json!({ "action": "save", "name": "before" }),
    );
    let before = std::fs::read_to_string(dir.join(MAP)).unwrap();
    call(
        &mut ws,
        "asset_move",
        json!({ "from": TILESET, "to": "assets/tiles/base.tileset.json" }),
    );
    let moved = std::fs::read_to_string(dir.join(MAP)).unwrap();
    assert!(
        moved.contains("\"tileset\": \"assets/tiles/base.tileset.json\""),
        "{moved}"
    );
    call(
        &mut ws,
        "asset_move",
        json!({ "from": "maps", "to": "levels" }),
    );
    let scene = call(&mut ws, "entity_get", json!({ "ids": ["map"] }));
    assert_eq!(
        scene["entities"][0]["components"]["Tilemap"]["map"],
        "levels/room.tilemap.json"
    );
    // The history went with the map; the move of its tileset is in it as
    // the mover's change.
    let listed = call(
        &mut ws,
        "edit_history",
        json!({ "scene": "levels/room.tilemap.json" }),
    );
    assert_eq!(listed["entries"].as_array().unwrap().len(), 2, "{listed}");
    assert_eq!(listed["entries"][0]["actor"], "agent:agent", "{listed}");
    assert_eq!(
        listed["entries"][0]["reason"],
        format!("{TILESET} moved to assets/tiles/base.tileset.json")
    );
    // A move that would change the kind is refused.
    let wrong = fails(
        &mut ws,
        "asset_move",
        json!({ "from": "levels/room.tilemap.json", "to": "levels/room.json" }),
    );
    assert_eq!(wrong["code"], "kind_change");
    call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": "before" }),
    );
    assert_eq!(std::fs::read_to_string(dir.join(MAP)).unwrap(), before);
}

/// A top-down scene: the room's map at (-6, -4) with cells of 1 m, a hero
/// inside walking east, an orthographic camera.
fn top_down(ws: &mut Workspace) {
    paint_room(ws);
    call(
        ws,
        "edit_apply",
        json!({ "ops": [
            { "op": "set_resource", "name": "PhysicsSettings", "value": { "profile": "TopDown2D" } },
            { "op": "set", "id": "camera", "component": "Camera",
              "value": { "ortho_height": 9 } },
            { "op": "spawn", "id": "map", "components": {
                "Transform": { "translation": [-6, -4, 0] },
                "Tilemap": { "map": MAP, "layer": -1 } } },
            { "op": "spawn", "id": "hero", "components": {
                "Transform": { "translation": [0, 0, 0] },
                "Collider": { "shape": "Sphere", "size": [0.6, 0.6, 0.6] },
                "RigidBody": { "kind": "Kinematic" },
                "CharacterController": {},
                "CharacterMotion": { "velocity": [3, 0, 0] } } }
        ] }),
    );
}

#[test]
fn tile_walls_stop_a_character_in_a_session_and_queries_see_them() {
    let (_dir, mut ws) = project("session");
    top_down(&mut ws);
    // In the scene: a ray east from the middle hits the east wall's inner
    // face, x = -6 + 11 = 5, and names the map.
    let hit = call(
        &mut ws,
        "physics_query",
        json!({ "ray": { "from": [0, 0], "dir": [1, 0], "ignore": "hero" } }),
    );
    assert_eq!(hit["hit"]["id"], "map", "{hit}");
    assert!(
        (hit["hit"]["point"][0].as_f64().unwrap() - 5.0).abs() < 1e-3,
        "{hit}"
    );
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 120 }));
    let hero = call(
        &mut ws,
        "entity_get",
        json!({ "session": s, "ids": ["hero"], "components": ["Transform"] }),
    );
    let x = hero["entities"][0]["components"]["Transform"]["translation"][0]
        .as_f64()
        .unwrap();
    assert!((4.6..4.71).contains(&x), "stopped by the east wall: {x}");
    call(&mut ws, "sim_stop", json!({ "session": s }));
}

#[test]
fn a_replay_notices_a_repainted_map() {
    let (_dir, mut ws) = project("replay");
    top_down(&mut ws);
    let s = call(&mut ws, "sim_start", json!({}))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": s }),
    );
    call(&mut ws, "sim_step", json!({ "session": s, "frames": 90 }));
    call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": s, "name": "walk" }),
    );
    let same = call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "walk" }),
    );
    assert_eq!(same["passed"], true, "{same}");
    // The east wall moves one cell in: the hero stops sooner.
    call(
        &mut ws,
        "tilemap_edit",
        json!({ "tilemap": MAP, "ops": [{ "op": "rect", "layer": "walls", "from": [10, 1], "to": [10, 6], "tile": 1 }] }),
    );
    let changed = ws.call(
        "replay",
        &json!({ "action": "run", "name": "walk" }),
        &parse_actor("agent"),
    );
    assert!(
        changed.to_string().contains("replay_assets_changed"),
        "{changed}"
    );
    assert_eq!(changed["passed"], false, "{changed}");
}

#[test]
fn describe_view_lists_the_map_and_the_tile_under_a_point() {
    let (_dir, mut ws) = project("describe");
    top_down(&mut ws);
    // 9 m tall over 360 pixels: 40 pixels a meter, the origin at (320, 180).
    let seen = call(
        &mut ws,
        "describe_view",
        json!({ "width": 640, "height": 360, "point": [460, 120] }),
    );
    let map = &seen["tilemaps"][0];
    assert_eq!(map["entity"], "map", "{seen}");
    assert_eq!(map["map"], MAP);
    assert_eq!(map["cells"], json!([0, 0, 11, 7]));
    assert_eq!(map["tiles"], 96 + 2 * 12 + 2 * 6 - 2);
    // Pixel (460, 120): world (3.5, 1.5), cell (9, 5) of the map: floor
    // inside the room, no wall.
    assert_eq!(
        map["under_point"],
        json!({ "cell": [9, 5], "tiles": { "floor": 0, "walls": -1 } }),
        "{seen}"
    );
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "width": 320, "height": 180 }),
    );
    assert!(
        !picture.to_string().contains("tilemap_not_loaded"),
        "{picture}"
    );
    // A map whose file is gone is said to be so, and not drawn.
    call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "map", "component": "Tilemap",
                          "value": { "map": "maps/none.tilemap.json" } }] }),
    );
    let picture = call(
        &mut ws,
        "capture_frame",
        json!({ "width": 320, "height": 180 }),
    );
    assert!(
        codes(&picture).contains(&"tilemap_not_loaded".to_owned()),
        "{picture}"
    );
}
