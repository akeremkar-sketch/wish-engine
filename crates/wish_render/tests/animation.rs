//! Animated models (M6 WP6): a skinned bar drawn at two times of its clip.
//! The identity buffer says where the bar is, whatever the driver.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::math::Vec3;
use wish_ecs::World;
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer, extract, project_to_pixel};
use wish_scene::load;
use wish_transform::update_global_transforms;

const MODEL: &str = "assets/bar.gltf";

fn scene_at(clip: &str, time: f32) -> (RenderScene, wish_core::Entity) {
    let mut world = World::new();
    wish_transform::register(&mut world).unwrap();
    wish_render::components::register(&mut world).unwrap();
    wish_scene::instantiate::register(&mut world).unwrap();
    world
        .register_component::<wish_types::anim::AnimationPlayer>()
        .unwrap();
    let source = format!(
        r#"{{
  "format": 1,
  "entities": {{
    "camera": {{ "components": {{
      "Camera": {{ "fov_y_deg": 50 }},
      "Transform": {{ "translation": [0, 1, 5] }} }} }},
    "bar": {{ "components": {{
      "Model": {{ "asset": "{MODEL}" }},
      "AnimationPlayer": {{ "clip": "{clip}", "time": {time} }},
      "Transform": {{ "scale": [3, 1, 1] }} }} }},
    "sun": {{ "components": {{ "DirectionalLight": {{}} }} }}
  }}
}}"#
    );
    let result = load(&source, "scenes/anim.scene.json", world.registry());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let entities = result.document.unwrap().instantiate(&mut world).unwrap();
    update_global_transforms(&mut world).unwrap();
    (extract(&world).unwrap(), entities["bar"])
}

#[test]
fn a_skinned_model_is_drawn_in_its_clips_pose() {
    let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
    let mut renderer = Renderer::new(&gpu).unwrap();
    let model =
        wish_import::load_model_bytes(wish_import::samples::bending_bar().as_bytes(), MODEL)
            .unwrap();
    renderer.upload_model(&gpu, MODEL, &model).unwrap();
    let (width, height) = (160, 120);
    let at = |scene: &RenderScene, point: Vec3| {
        let camera = &scene.cameras[0];
        let view_proj = camera.projection(width as f32 / height as f32) * camera.view;
        let (x, y) = project_to_pixel(&view_proj, point, width, height).unwrap();
        (x as u32, y as u32)
    };
    // At the start of `bend` the bar stands straight up.
    let (rest, bar) = scene_at("bend", 0.0);
    let ids = renderer
        .id_buffer(&gpu, &rest, None, width, height)
        .unwrap();
    let (x, y) = at(&rest, Vec3::new(0.0, 1.9, 0.0));
    assert_eq!(ids.entity_at(x, y), Some(bar), "the top, straight up");
    let (x, y) = at(&rest, Vec3::new(-0.9, 1.0, 0.0));
    assert_eq!(ids.entity_at(x, y), None, "nothing to the left yet");
    // A second in, the upper half points left (the bar is 3x wider, so the
    // bent part is 0.6 m thick: the probe sits in its middle).
    let (bent, bar) = scene_at("bend", 1.0);
    let ids = renderer
        .id_buffer(&gpu, &bent, None, width, height)
        .unwrap();
    let (x, y) = at(&bent, Vec3::new(-0.8, 1.0, 0.0));
    assert_eq!(ids.entity_at(x, y), Some(bar), "the top, bent to the left");
    let (x, y) = at(&bent, Vec3::new(0.0, 1.9, 0.0));
    assert_eq!(ids.entity_at(x, y), None, "no longer straight up");
    // The lower half, bound to the root, stays.
    let (x, y) = at(&bent, Vec3::new(0.0, 0.3, 0.0));
    assert_eq!(ids.entity_at(x, y), Some(bar));
    // The lit picture draws it too, without warnings.
    let frame = renderer.render(&gpu, &bent, None, width, height).unwrap();
    assert!(frame.warnings.is_empty(), "{:?}", frame.warnings);
}
