//! A gallery of the Khronos glTF sample models, for looking at by eye.
//!
//! Not part of the normal run: it needs the models, which are not in the
//! repository (their licenses differ). Download them yourself and run
//!
//! ```text
//! WISH_KHRONOS_DIR=<folder with Name/Name.glb> cargo test --release -p wish_render \
//!     --test khronos -- --ignored --nocapture
//! ```
//!
//! Each model is imported (its warnings are printed), placed on a ground
//! plane under a sun, drawn from a camera fitted to its bounds, and written
//! to `<folder>/<Name>.png`.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_core::math::{Mat4, Quat, Vec3, Vec4};
use wish_ecs::World;
use wish_render::{Gpu, GpuOptions, Renderer, extract};
use wish_scene::load;
use wish_transform::update_global_transforms;

const MODELS: &[&str] = &[
    "Box",
    "Duck",
    "DamagedHelmet",
    "MetalRoughSpheres",
    "NormalTangentTest",
];

fn quaternion_json(rotation: Quat) -> String {
    let [x, y, z, w] = rotation.to_array();
    format!("[{x}, {y}, {z}, {w}]")
}

#[test]
#[ignore = "needs the Khronos sample models; see the module documentation"]
fn draw_the_khronos_samples() {
    let folder = PathBuf::from(std::env::var("WISH_KHRONOS_DIR").expect("set WISH_KHRONOS_DIR"));
    let software = std::env::var_os("WISH_SOFTWARE").is_some();
    let gpu = Gpu::shared(GpuOptions { software }).unwrap();
    eprintln!("adapter: {:?}", gpu.adapter());
    for name in MODELS {
        let path = folder.join(name).join(format!("{name}.glb"));
        let model = wish_import::load_model_file(&path, &format!("{name}.glb")).unwrap();
        let (low, high) = model.bounds().expect("the model has geometry");
        let centre = (low + high) * 0.5;
        let radius = ((high - low).length() * 0.5).max(1e-3);
        eprintln!(
            "{name}: {} mesh(es), {} material(s), {} texture(s), size {:?}, {} warning(s)",
            model.meshes.len(),
            model.materials.len(),
            model.textures.len(),
            high - low,
            model.warnings.len()
        );
        for warning in &model.warnings {
            eprintln!("  {warning}");
        }

        // A camera looking at the model's centre from a corner above.
        let direction = Vec3::new(0.55, 0.35, 1.0).normalize();
        let distance = radius / (20f32.to_radians()).sin() * 1.05;
        let eye = centre + direction * distance;
        let forward = (centre - eye).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward);
        let camera = Quat::from_mat4(&Mat4::from_cols(
            right.extend(0.0),
            up.extend(0.0),
            (-forward).extend(0.0),
            Vec4::W,
        ));
        let ground = low.y;
        let scale = radius * 6.0;
        let source = format!(
            r#"{{ "format": 1, "entities": {{
                "camera": {{ "components": {{ "Camera": {{ "fov_y_deg": 40, "near": {near}, "far": {far} }},
                    "Transform": {{ "translation": [{ex}, {ey}, {ez}], "rotation": {rot} }} }} }},
                "ground": {{ "components": {{ "Primitive": {{ "shape": "Plane" }},
                    "Material": {{ "base_color": [0.5, 0.5, 0.48, 1], "roughness": 0.95 }},
                    "Transform": {{ "translation": [{cx}, {ground}, {cz}], "scale": [{scale}, 1, {scale}] }} }} }},
                "model": {{ "components": {{ "Model": {{ "asset": "assets/{name}.glb" }} }} }},
                "sun": {{ "components": {{ "DirectionalLight": {{ "illuminance_lux": 100000 }},
                    "Transform": {{ "rotation": [-0.4545195, 0.3304543, 0.1786597, 0.8067046] }} }} }}
            }} }}"#,
            near = radius * 0.05,
            far = radius * 40.0,
            ex = eye.x,
            ey = eye.y,
            ez = eye.z,
            rot = quaternion_json(camera),
            cx = centre.x,
            cz = centre.z,
        );
        let mut world = World::new();
        wish_transform::register(&mut world).unwrap();
        wish_render::components::register(&mut world).unwrap();
        wish_scene::instantiate::register(&mut world).unwrap();
        let loaded = load(&source, "gallery.scene.json", world.registry());
        assert!(!loaded.has_errors(), "{name}: {:?}", loaded.diagnostics);
        loaded.document.unwrap().instantiate(&mut world).unwrap();
        update_global_transforms(&mut world).unwrap();
        let scene = extract(&mut world).unwrap();

        let mut renderer = Renderer::new(&gpu).unwrap();
        renderer
            .upload_model(&gpu, &format!("assets/{name}.glb"), &model)
            .unwrap();
        let frame = renderer.render(&gpu, &scene, None, 960, 640).unwrap();
        for warning in &frame.warnings {
            eprintln!("  {warning}");
        }
        let out = folder.join(format!("{name}.png"));
        std::fs::write(&out, frame.image.to_png().unwrap()).unwrap();
        eprintln!("  wrote {} ({} draw(s))", out.display(), frame.drawn);
    }
}
