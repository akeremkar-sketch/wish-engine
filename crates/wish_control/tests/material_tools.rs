//! Material files and textures through the control plane (M12 package 7):
//! made, listed, reported, checked, moved, drawn, reloaded when their files
//! change, and exported.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-materials-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "M", "wen").unwrap();
    // No game code: these projects are about looks.
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let mut ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    ws.set_software(true);
    (dir, ws)
}

fn call(ws: &mut Workspace, tool: &str, args: Value) -> Value {
    ws.call(tool, &args, &parse_actor("agent:painter"))
}

fn ok(answer: Value) -> Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

/// A picture of one colour, made by the engine's own placeholder tool.
fn picture(ws: &mut Workspace, path: &str, color: [f64; 4]) {
    ok(call(
        ws,
        "asset_create",
        json!({ "path": path, "overwrite": true,
            "picture": { "width": 32, "height": 32, "shape": "fill", "color": color } }),
    ));
}

const RED: [f64; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f64; 4] = [0.0, 1.0, 0.0, 1.0];

#[test]
fn a_material_file_is_made_checked_listed_and_reported() {
    let (dir, mut ws) = project("made");
    picture(&mut ws, "assets/textures/red.png", RED);
    let args = json!({ "path": "assets/materials/red.mat.json",
        "material": { "roughness": 0.7, "base_color_texture": "assets/textures/red.png", "uv_scale": [2, 2] } });
    // A dry run says what would be made and writes nothing.
    let mut dry = args.clone();
    dry["dry_run"] = json!(true);
    let planned = ok(call(&mut ws, "asset_create", dry));
    assert_eq!(planned["kind"], "material");
    assert!(!dir.join("assets/materials/red.mat.json").exists());
    assert!(!dir.join("assets/materials").exists(), "no folder either");
    // Made: canonical text, and the answer says what it holds.
    let made = ok(call(&mut ws, "asset_create", args.clone()));
    let text = "{\n  \"format\": 1,\n  \"material\": {\n    \"base_color_texture\": \"assets/textures/red.png\",\n    \"roughness\": 0.7,\n    \"uv_scale\": [2, 2]\n  }\n}\n";
    assert_eq!(
        std::fs::read_to_string(dir.join("assets/materials/red.mat.json")).unwrap(),
        text
    );
    assert_eq!(made["kind"], "material");
    assert_eq!(made["bytes"], text.len());
    assert_eq!(made["material"]["roughness"], 0.7);
    assert_eq!(made["textures"], json!(["assets/textures/red.png"]));
    assert!(made.get("diagnostics").is_none(), "{made}");
    // Not replaced unless asked.
    let again = call(&mut ws, "asset_create", args.clone());
    assert_eq!(again["error"]["code"], "output_exists", "{again}");
    let mut replace = args.clone();
    replace["overwrite"] = json!(true);
    replace["material"]["roughness"] = json!(0.2);
    ok(call(&mut ws, "asset_create", replace));
    // Listed by kind, and reported with every field.
    let listed = ok(call(&mut ws, "asset_list", json!({ "kind": "material" })));
    assert_eq!(listed["matched"], 1, "{listed}");
    assert_eq!(listed["assets"][0]["path"], "assets/materials/red.mat.json");
    assert_eq!(listed["assets"][0]["kind"], "material");
    let info = ok(call(
        &mut ws,
        "asset_info",
        json!({ "path": "assets/materials/red.mat.json" }),
    ));
    assert_eq!(info["kind"], "material");
    assert_eq!(info["material"]["roughness"], 0.2);
    assert_eq!(info["material"]["metallic"], 0.0, "defaults are filled in");
    assert_eq!(info["material"]["alpha_mode"], "Opaque");
    assert_eq!(info["written"]["uv_scale"], json!([2, 2]));
    assert!(info["written"].get("metallic").is_none());
    assert_eq!(
        info["textures"],
        json!([{ "slot": "base_color_texture", "path": "assets/textures/red.png", "exists": true }])
    );
    assert_eq!(info["used_by"], json!([]));
    assert!(info.get("diagnostics").is_none(), "{info}");
    // The picture says who uses it: the material file.
    let info = ok(call(
        &mut ws,
        "asset_info",
        json!({ "path": "assets/textures/red.png" }),
    ));
    assert_eq!(info["used_by"], json!(["assets/materials/red.mat.json"]));
    // The project checks clean, the material file among its files.
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(
        (checked["passed"].clone(), checked["warnings"].clone()),
        (json!(true), json!(0)),
        "{checked}"
    );
    let one = ok(call(
        &mut ws,
        "check",
        json!({ "paths": ["assets/materials/red.mat.json"] }),
    ));
    assert_eq!(one["files"], 1, "{one}");
    // Who made it is on record.
    let activity = ok(call(&mut ws, "activity_get", json!({})));
    assert!(
        activity["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["tool"] == "asset_create" && entry["actor"] == "agent:painter" }),
        "{activity}"
    );
}

#[test]
fn a_material_that_is_not_one_is_refused_with_a_place_and_a_hint() {
    let (dir, mut ws) = project("refused");
    let cases = [
        (
            json!({ "path": "assets/m.mat.json", "material": { "colour": [1, 0, 0, 1] } }),
            "unknown_field",
            "/material/colour",
        ),
        (
            json!({ "path": "assets/m.mat.json", "material": { "roughness": 9 } }),
            "value_out_of_range",
            "/material/roughness",
        ),
        (
            json!({ "path": "assets/m.mat.json", "material": { "alpha_mode": "Blend" } }),
            "unknown_variant",
            "/material/alpha_mode",
        ),
        // A material file names pictures, never another material file.
        (
            json!({ "path": "assets/m.mat.json", "material": { "file": "assets/other.mat.json" } }),
            "unknown_field",
            "/material/file",
        ),
        (
            json!({ "path": "assets/m.mat.json", "material": { "normal_texture": "../../x.png" } }),
            "invalid_asset_ref",
            "/material/normal_texture",
        ),
        (
            json!({ "path": "assets/m.mat.json", "material": "red" }),
            "invalid_parameter",
            "/material",
        ),
        (
            json!({ "path": "assets/m.mat.json", "picture": { "width": 8 } }),
            "invalid_parameter",
            "/picture",
        ),
        (
            json!({ "path": "assets/m.png", "material": { "roughness": 1 } }),
            "invalid_parameter",
            "/material",
        ),
        (
            json!({ "path": "assets/M.MAT.JSON", "material": {} }),
            "invalid_output_path",
            "/path",
        ),
        (
            json!({ "path": "../m.mat.json", "material": {} }),
            "invalid_output_path",
            "/path",
        ),
        (
            json!({ "path": "scenes/main.scene.json", "material": {} }),
            "invalid_output_path",
            "/path",
        ),
    ];
    for (args, code, pointer) in cases {
        let answer = call(&mut ws, "asset_create", args.clone());
        assert_eq!(answer["ok"], false, "{args}");
        assert_eq!(answer["error"]["code"], code, "{args}: {answer}");
        assert_eq!(answer["error"]["location"]["path"], pointer, "{answer}");
        assert!(
            !answer["error"]["hint"].as_str().unwrap().is_empty(),
            "{answer}"
        );
    }
    assert!(!dir.join("assets/m.mat.json").exists());
    assert!(
        !dir.join("assets/m.png").exists(),
        "nothing was left behind"
    );
    // A texture that is not there yet is a warning: the file is made.
    let made = ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/m.mat.json", "material": { "base_color_texture": "assets/later.png" } }),
    ));
    assert_eq!(made["diagnostics"][0]["code"], "asset_not_found", "{made}");
    assert_eq!(made["diagnostics"][0]["severity"], "warning");
    assert_eq!(
        made["diagnostics"][0]["location"]["path"],
        "/material/base_color_texture"
    );
    // check then says it as the error it is, in the file, with its line.
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(checked["passed"], false);
    let problem = &checked["diagnostics"][0];
    assert_eq!(problem["code"], "asset_not_found", "{checked}");
    assert_eq!(problem["location"]["file"], "assets/m.mat.json");
    assert_eq!(problem["location"]["line"], 4);
}

#[test]
fn check_says_what_is_wrong_with_materials_written_by_hand() {
    let (dir, mut ws) = project("check");
    std::fs::create_dir_all(dir.join("assets/materials")).unwrap();
    std::fs::write(
        dir.join("assets/materials/wall.mat.json"),
        "{\n  \"format\": 1,\n  \"material\": {\n    \"roughnes\": 0.5\n  }\n}\n",
    )
    .unwrap();
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(checked["passed"], false);
    let problem = &checked["diagnostics"][0];
    assert_eq!(problem["code"], "unknown_field", "{checked}");
    assert_eq!(
        problem["location"]["file"],
        "assets/materials/wall.mat.json"
    );
    assert_eq!(problem["location"]["line"], 4);
    assert!(
        problem["hint"].as_str().unwrap().contains("roughness"),
        "{problem}"
    );
    // Fixed by hand; an entity that names it and also writes other fields
    // is told that those do nothing.
    std::fs::write(
        dir.join("assets/materials/wall.mat.json"),
        "{\n  \"format\": 1,\n  \"material\": {\n    \"roughness\": 0.5\n  }\n}\n",
    )
    .unwrap();
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "wall", "components": { "Primitive": {},
            "Material": { "file": "assets/materials/wall.mat.json", "base_color": [1, 0, 0, 1] } } }] }),
    ));
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(
        (checked["passed"].clone(), checked["warnings"].clone()),
        (json!(true), json!(1)),
        "{checked}"
    );
    assert_eq!(
        checked["diagnostics"][0]["code"], "material_fields_unused",
        "{checked}"
    );
    // A name that is not a material file, in a scene: an error where it is
    // written.
    let wrong = call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "set", "id": "wall", "component": "Material",
            "value": { "file": "scenes/main.scene.json" } }] }),
    );
    if wrong["ok"] == true {
        let checked = ok(call(&mut ws, "check", json!({})));
        assert_eq!(checked["passed"], false, "{checked}");
    }
}

fn centre(shot: &Value) -> [u8; 4] {
    let picture = wish_render::RgbaImage::from_png(
        &wish_control::render::base64_decode(shot["image"]["data"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    picture
        .pixel(picture.width / 2, picture.height / 2)
        .unwrap()
}

fn colour(pixel: [u8; 4]) -> &'static str {
    let [r, g, b, _] = pixel.map(i32::from);
    let high = r.max(g).max(b);
    let is = |channel: i32| channel * 2 > high;
    match (is(r), is(g), is(b)) {
        _ if high < 24 => "black",
        (true, false, false) => "red",
        (false, true, false) => "green",
        (false, false, true) => "blue",
        (true, true, false) => "yellow",
        (true, true, true) => "white",
        _ => "other",
    }
}

fn shoot(ws: &mut Workspace) -> Value {
    ok(call(
        ws,
        "capture_frame",
        json!({ "width": 160, "height": 90 }),
    ))
}

fn warning_codes(answer: &Value) -> Vec<String> {
    answer["diagnostics"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|d| d["code"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_textured_cube_is_drawn_and_follows_its_files() {
    let (dir, mut ws) = project("drawn");
    picture(&mut ws, "assets/textures/skin.png", RED);
    // A cube that fills the middle of the picture, its look a material file
    // with a picture.
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json",
            "material": { "base_color_texture": "assets/textures/skin.png" } }),
    ));
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "cube", "components": {
            "Primitive": { "shape": "Cube" },
            "Transform": { "translation": [0, 1, 0], "scale": [2, 2, 2] },
            "Material": { "file": "assets/materials/skin.mat.json" } } }] }),
    ));
    let shot = shoot(&mut ws);
    assert_eq!(warning_codes(&shot), Vec::<String>::new(), "{shot}");
    assert_eq!(colour(centre(&shot)), "red");
    // The picture's file changes: the next picture shows it, in the same
    // workspace, with nothing asked for.
    picture(&mut ws, "assets/textures/skin.png", GREEN);
    assert_eq!(colour(centre(&shoot(&mut ws))), "green");
    // The material file changes (made again by the tool).
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json", "overwrite": true,
            "material": { "base_color": [0, 0, 1, 1] } }),
    ));
    let shot = shoot(&mut ws);
    assert_eq!(warning_codes(&shot), Vec::<String>::new(), "{shot}");
    assert_eq!(colour(centre(&shot)), "blue");
    // It changes on disk, by hand, with a mistake in it: the picture is
    // still drawn, the cube with the plain look of its own Material, and
    // the warning says why.
    std::fs::write(
        dir.join("assets/materials/skin.mat.json"),
        "{ \"format\": 1, \"material\": { \"roughness\": \"very\" } }",
    )
    .unwrap();
    let shot = shoot(&mut ws);
    assert_eq!(warning_codes(&shot), ["material_not_loaded"], "{shot}");
    let warning = &shot["diagnostics"][0];
    assert_eq!(
        warning["location"]["file"],
        "assets/materials/skin.mat.json"
    );
    assert!(
        warning["message"].as_str().unwrap().contains("roughness"),
        "{warning}"
    );
    assert_eq!(colour(centre(&shot)), "white");
    // Mended by hand: drawn again.
    std::fs::write(
        dir.join("assets/materials/skin.mat.json"),
        "{ \"format\": 1, \"material\": { \"base_color\": [1, 1, 0, 1] } }",
    )
    .unwrap();
    let shot = shoot(&mut ws);
    assert_eq!(warning_codes(&shot), Vec::<String>::new(), "{shot}");
    assert_eq!(colour(centre(&shot)), "yellow");

    // A texture that is not a picture, and one that is not there: each a
    // warning with the file and the slot, and the cube is still drawn.
    std::fs::write(dir.join("assets/textures/broken.png"), b"not a png").unwrap();
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json", "overwrite": true,
            "material": { "base_color": [1, 0, 0, 1],
                "base_color_texture": "assets/textures/broken.png",
                "normal_texture": "assets/textures/gone.png" } }),
    ));
    let shot = shoot(&mut ws);
    assert_eq!(
        warning_codes(&shot),
        ["texture_not_loaded", "texture_not_loaded"],
        "{shot}"
    );
    let said = shot["diagnostics"].to_string();
    assert!(said.contains("assets/textures/broken.png") && said.contains("base_color_texture"));
    assert!(said.contains("assets/textures/gone.png") && said.contains("normal_texture"));
    assert_eq!(colour(centre(&shot)), "red");
    // describe_view and a running session draw from the same files.
    let described = ok(call(&mut ws, "describe_view", json!({})));
    assert!(described.to_string().contains("texture_not_loaded"));
    let session = ok(call(&mut ws, "sim_start", json!({})));
    let shot = ok(call(
        &mut ws,
        "capture_frame",
        json!({ "session": session["session"], "width": 160, "height": 90 }),
    ));
    assert_eq!(colour(centre(&shot)), "red");
}

#[test]
fn the_material_file_type_is_documented_like_any_type() {
    let (_dir, mut ws) = project("schema");
    // schema_get: by name, with its fields, units, ranges and defaults.
    let got = ok(call(
        &mut ws,
        "schema_get",
        json!({ "names": ["MaterialFile"], "detail": "full" }),
    ));
    let found = &got["types"][0];
    assert_eq!(
        (found["name"].clone(), found["kind"].clone()),
        (json!("MaterialFile"), json!("file"))
    );
    let fields = found["schema"]["properties"].as_object().unwrap();
    assert_eq!(fields.len(), 16, "{found}");
    assert!(fields.values().all(|field| {
        field["description"]
            .as_str()
            .is_some_and(|doc| !doc.is_empty())
    }));
    assert_eq!(fields["roughness"]["maximum"], 1.0);
    assert_eq!(fields["uv_scale"]["default"], json!([1.0, 1.0]));
    assert_eq!(fields["base_color_texture"]["x-asset"], true);
    assert!(
        fields.get("file").is_none(),
        "a material file names no other"
    );
    // The component names the file and nothing more.
    let got = ok(call(
        &mut ws,
        "schema_get",
        json!({ "names": ["Material"] }),
    ));
    assert_eq!(
        got["types"][0]["fields"],
        json!(["base_color", "emissive", "file", "metallic", "roughness"])
    );
    // People's words find both.
    for words in ["texture", "tiling", "cutout"] {
        let found = ok(call(&mut ws, "schema_get", json!({ "search": words })));
        let names: Vec<&str> = found["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"MaterialFile"), "{words}: {names:?}");
    }
    // The documentation: a page of its own, and the search leads to it.
    let page = wish_control::docs::type_page(ws.registry(), "MaterialFile").unwrap();
    assert!(page.starts_with("# MaterialFile (file)"), "{page}");
    assert!(page.contains("| uv_scale |") && page.contains("\"material\": { ... }"));
    let hits = ok(call(
        &mut ws,
        "docs_search",
        json!({ "query": "normal map bumps" }),
    ));
    assert!(
        hits["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["uri"] == "wish://docs/components/MaterialFile"),
        "{hits}"
    );
    // A name from another engine leads here.
    let unknown = call(&mut ws, "schema_get", json!({ "names": ["Texture2D"] }));
    assert!(unknown.to_string().contains(".mat.json"), "{unknown}");
}

#[test]
fn moving_files_keeps_materials_and_scenes_together() {
    let (dir, mut ws) = project("moved");
    picture(&mut ws, "assets/skin.png", RED);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/skin.mat.json", "material": { "base_color_texture": "assets/skin.png" } }),
    ));
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "cube", "components": {
            "Primitive": {}, "Transform": { "translation": [0, 1, 0], "scale": [2, 2, 2] },
            "Material": { "file": "assets/skin.mat.json" } } }] }),
    ));
    // The texture moves: the material file is rewritten.
    let planned = ok(call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/skin.png", "to": "assets/textures/skin.png", "dry_run": true }),
    ));
    assert_eq!(
        planned["documents"],
        json!([{ "file": "assets/skin.mat.json", "references": 1 }])
    );
    ok(call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/skin.png", "to": "assets/textures/skin.png" }),
    ));
    // The material file moves: the scene is rewritten.
    let moved = ok(call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/skin.mat.json", "to": "assets/materials/skin.mat.json" }),
    ));
    assert_eq!(
        moved["documents"],
        json!([{ "file": "scenes/main.scene.json", "references": 1 }])
    );
    let material = std::fs::read_to_string(dir.join("assets/materials/skin.mat.json")).unwrap();
    assert!(
        material.contains("\"assets/textures/skin.png\""),
        "{material}"
    );
    let checked = ok(call(&mut ws, "check", json!({})));
    assert_eq!(
        (checked["passed"].clone(), checked["warnings"].clone()),
        (json!(true), json!(0)),
        "{checked}"
    );
    // And it is still drawn.
    let shot = shoot(&mut ws);
    assert_eq!(warning_codes(&shot), Vec::<String>::new(), "{shot}");
    assert_eq!(colour(centre(&shot)), "red");
    // A material file cannot become something else by a move.
    let refused = call(
        &mut ws,
        "asset_move",
        json!({ "from": "assets/materials/skin.mat.json", "to": "assets/materials/skin.json" }),
    );
    assert_eq!(refused["error"]["code"], "kind_change", "{refused}");
}

#[test]
fn an_export_takes_the_material_files_and_their_textures() {
    let (dir, mut ws) = project("export");
    // Any file stands for the player here (wish_player's tests run it).
    ws.set_player_path(std::env::current_exe().unwrap());
    picture(&mut ws, "assets/textures/skin.png", RED);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json",
            "material": { "base_color_texture": "assets/textures/skin.png" } }),
    ));
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "cube", "components": {
            "Primitive": {}, "Material": { "file": "assets/materials/skin.mat.json" } } }] }),
    ));
    let exported = ok(call(&mut ws, "project_export", json!({})));
    let game = dir.join("exports/m/game");
    for file in [
        "assets/materials/skin.mat.json",
        "assets/textures/skin.png",
        "scenes/main.scene.json",
    ] {
        assert_eq!(
            std::fs::read(game.join(file)).unwrap(),
            std::fs::read(dir.join(file)).unwrap(),
            "{file}: {exported}"
        );
    }
    // The exported copies are not the project's: check and the asset list
    // leave them alone.
    let listed = ok(call(&mut ws, "asset_list", json!({ "kind": "material" })));
    assert_eq!(listed["matched"], 1, "{listed}");
    assert_eq!(ok(call(&mut ws, "check", json!({})))["passed"], true);
}

#[test]
fn a_replay_does_not_depend_on_how_things_look() {
    // The asset fingerprint of a session is what the simulation reads from
    // project files (clip lengths, fonts). Materials and textures are read
    // by the renderer only, so changing them must not make a replay warn
    // that it may diverge, and must not change the world's hash.
    let (_dir, mut ws) = project("replay");
    picture(&mut ws, "assets/textures/skin.png", RED);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json",
            "material": { "base_color_texture": "assets/textures/skin.png" } }),
    ));
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "cube", "components": {
            "Primitive": {}, "Material": { "file": "assets/materials/skin.mat.json" } } }] }),
    ));
    let session = ok(call(&mut ws, "sim_start", json!({})));
    let id = session["session"].clone();
    ok(call(
        &mut ws,
        "replay",
        json!({ "action": "record", "session": id }),
    ));
    let stepped = ok(call(
        &mut ws,
        "sim_step",
        json!({ "session": id, "frames": 10 }),
    ));
    ok(call(
        &mut ws,
        "replay",
        json!({ "action": "save", "session": id, "name": "looks" }),
    ));
    // The look changes; the scene does not.
    picture(&mut ws, "assets/textures/skin.png", GREEN);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": "assets/materials/skin.mat.json", "overwrite": true,
            "material": { "base_color": [0, 0, 1, 1], "roughness": 1 } }),
    ));
    let played = ok(call(
        &mut ws,
        "replay",
        json!({ "action": "run", "name": "looks" }),
    ));
    assert_eq!(played["passed"], true, "{played}");
    assert!(
        !played.to_string().contains("replay_assets_changed"),
        "{played}"
    );
    assert!(stepped["hash"].is_string() || stepped["hash"].is_null());
}

#[test]
fn a_checkpoint_brings_material_and_meta_files_back() {
    // The guides say to save a checkpoint before a large change, because a
    // material file has no undo of its own: going back must take material
    // files (and meta files) with it, beside the scenes.
    let (dir, mut ws) = project("checkpoint");
    let material = "assets/materials/skin.mat.json";
    let meta = "assets/textures/skin.png.meta.json";
    picture(&mut ws, "assets/textures/skin.png", RED);
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": material, "material": { "roughness": 0.25 } }),
    ));
    std::fs::write(dir.join(meta), "{\n  \"format\": 1\n}\n").unwrap();
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "spawn", "id": "cube", "components": {
            "Primitive": {}, "Material": { "file": material } } }] }),
    ));
    let saved = ok(call(&mut ws, "checkpoint", json!({ "action": "save" })));
    let name = saved["checkpoint"]["name"].as_str().unwrap().to_owned();
    let material_then = std::fs::read_to_string(dir.join(material)).unwrap();
    // A large change: the look, a meta file and the scene.
    ok(call(
        &mut ws,
        "asset_create",
        json!({ "path": material, "overwrite": true,
            "material": { "roughness": 0.9, "double_sided": true } }),
    ));
    std::fs::write(dir.join(meta), "{\n  \"format\": 1,\n  \"x\": 2\n}\n").unwrap();
    ok(call(
        &mut ws,
        "edit_apply",
        json!({ "ops": [{ "op": "despawn", "id": "cube" }] }),
    ));
    // A dry run says what would come back and writes nothing.
    let dry = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": name, "dry_run": true }),
    ));
    let paths = |answer: &Value| -> Vec<String> {
        answer["restored"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_owned())
            .collect()
    };
    for path in [material, meta, "scenes/main.scene.json"] {
        assert!(paths(&dry).contains(&path.to_owned()), "{path}: {dry}");
    }
    assert_ne!(
        std::fs::read_to_string(dir.join(material)).unwrap(),
        material_then
    );
    // Back: every file as it was.
    let back = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": name }),
    ));
    for path in [material, meta, "scenes/main.scene.json"] {
        assert!(paths(&back).contains(&path.to_owned()), "{path}: {back}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join(material)).unwrap(),
        material_then
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(meta)).unwrap(),
        "{\n  \"format\": 1\n}\n"
    );
    ok(call(&mut ws, "entity_get", json!({ "ids": ["cube"] })));
    // Once more: nothing left to bring back.
    let again = ok(call(
        &mut ws,
        "checkpoint",
        json!({ "action": "restore", "name": name, "dry_run": true }),
    ));
    assert_eq!(again["restored"], json!([]), "{again}");
    assert!(
        again["unchanged"]
            .as_array()
            .unwrap()
            .contains(&json!(material)),
        "{again}"
    );
}
