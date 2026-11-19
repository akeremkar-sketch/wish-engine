//! What `render::load_materials` keeps between pictures (M12 package 7
//! review): what the scene no longer names is let go, a file that could not
//! be used is not read again until it changes, and a picture over the
//! budget is refused from its header without being decoded.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use wish_control::render::{MaterialStamps, load_materials};
use wish_control::scaffold::create_project;
use wish_control::{Workspace, WorkspaceOptions, parse_actor};
use wish_ecs::World;
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer, extract};

fn project(name: &str) -> (PathBuf, Workspace) {
    let dir = std::env::temp_dir().join(format!("wish-matcache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, "M", "wen").unwrap();
    std::fs::remove_dir_all(dir.join("scripts")).unwrap();
    let ws = Workspace::open(WorkspaceOptions {
        root: dir.clone(),
        example: None,
    })
    .unwrap();
    (dir, ws)
}

fn make(ws: &mut Workspace, args: Value) {
    let answer = ws.call("asset_create", &args, &parse_actor("agent:test"));
    assert_eq!(answer["ok"], true, "{answer}");
}

/// A 64x64 picture of one colour.
fn picture(ws: &mut Workspace, path: &str, red: f64) {
    make(
        ws,
        json!({ "path": path, "overwrite": true,
            "picture": { "width": 64, "height": 64, "shape": "fill", "color": [red, 0.5, 0.2, 1] } }),
    );
}

fn material(ws: &mut Workspace, path: &str, look: Value) {
    make(
        ws,
        json!({ "path": path, "overwrite": true, "material": look }),
    );
}

/// A scene of one cube that names the material file `file`.
fn scene(file: &str) -> RenderScene {
    let source = json!({ "format": 1, "entities": { "cube": { "components": {
        "Primitive": { "shape": "Cube" }, "Material": { "file": file } } } } })
    .to_string();
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    let loaded = wish_scene::load(&source, "s.scene.json", world.registry());
    assert!(!loaded.has_errors(), "{:?}", loaded.diagnostics);
    loaded.document.unwrap().instantiate(&mut world).unwrap();
    wish_transform::update_global_transforms(&mut world).unwrap();
    extract(&world).unwrap()
}

fn codes(warnings: &[wish_core::Diagnostic]) -> Vec<String> {
    warnings.iter().map(|w| w.code.to_string()).collect()
}

#[test]
fn textures_the_scene_no_longer_names_are_let_go() {
    let (dir, mut ws) = project("swap");
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let mut renderer = Renderer::new(&gpu).unwrap();
    // Room for two pictures; six are used one after another.
    let mut known = MaterialStamps::with_pixel_budget(2 * 64 * 64);
    let file = "assets/m.mat.json";
    let cube = scene(file);
    for i in 0..6 {
        let texture = format!("assets/t{i}.png");
        picture(&mut ws, &texture, f64::from(i) / 6.0);
        material(&mut ws, file, json!({ "base_color_texture": texture }));
        let warnings = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
        assert!(warnings.is_empty(), "swap {i}: {:?}", codes(&warnings));
        assert!(renderer.has_texture(&texture, true));
        assert_eq!(known.textures_held(), 1, "swap {i}");
        assert_eq!(renderer.texture_count(), 1, "swap {i}");
    }
    // A scene without the file lets go of the file and its texture.
    let other = scene("");
    load_materials(&gpu, &mut renderer, &other, &dir, &mut known);
    assert!(renderer.material(file).is_none());
    assert_eq!(renderer.texture_count(), 0);
    assert_eq!(known.textures_held(), 0);
}

#[test]
fn a_file_that_failed_is_not_read_again_until_it_changes() {
    let (dir, mut ws) = project("failed");
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let mut renderer = Renderer::new(&gpu).unwrap();
    let mut known = MaterialStamps::default();
    std::fs::write(dir.join("assets/broken.png"), b"not a picture at all").unwrap();
    let file = "assets/b.mat.json";
    material(
        &mut ws,
        file,
        json!({ "base_color_texture": "assets/broken.png" }),
    );
    let cube = scene(file);
    let first = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert_eq!(codes(&first), ["texture_not_loaded"]);
    let read = known.files_read();
    assert_eq!(read, 2, "the material file and the picture");
    // The next pictures say it again, without reading anything.
    for _ in 0..3 {
        let again = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
        assert_eq!(codes(&again), ["texture_not_loaded"]);
        assert_eq!(known.files_read(), read);
    }
    // Saved again (now a picture): read once, and drawn.
    picture(&mut ws, "assets/broken.png", 1.0);
    let fixed = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert!(fixed.is_empty(), "{:?}", codes(&fixed));
    assert_eq!(known.files_read(), read + 1);
    // A broken material file is not read again either.
    std::fs::write(dir.join(file), "{ \"format\": 1, \"material\": 3 }").unwrap();
    let broken = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert_eq!(codes(&broken), ["material_not_loaded"]);
    let read = known.files_read();
    let again = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert_eq!(codes(&again), ["material_not_loaded"]);
    assert_eq!(known.files_read(), read);
}

#[test]
fn a_picture_over_the_budget_is_refused_from_its_header() {
    let (dir, mut ws) = project("budget");
    let gpu = Gpu::shared(GpuOptions { software: true }).unwrap();
    let mut renderer = Renderer::new(&gpu).unwrap();
    // Room for one 64x64 picture; the material names two.
    let mut known = MaterialStamps::with_pixel_budget(64 * 64);
    picture(&mut ws, "assets/a.png", 0.1);
    picture(&mut ws, "assets/c.png", 0.9);
    let file = "assets/two.mat.json";
    material(
        &mut ws,
        file,
        json!({ "base_color_texture": "assets/a.png", "emissive_texture": "assets/c.png",
            "emissive": [1, 1, 1] }),
    );
    let cube = scene(file);
    let warnings = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert_eq!(codes(&warnings), ["texture_not_loaded"]);
    assert!(warnings[0].message.contains("assets/c.png"), "{warnings:?}");
    // The material file and a.png were read; c.png only as far as its
    // header.
    assert_eq!(known.files_read(), 2);
    assert!(!renderer.has_texture("assets/c.png", true));
    // The refusal depends on the other pictures, so it is looked at again,
    // still from the header only.
    let again = load_materials(&gpu, &mut renderer, &cube, &dir, &mut known);
    assert_eq!(codes(&again), ["texture_not_loaded"]);
    assert_eq!(known.files_read(), 2);
}
