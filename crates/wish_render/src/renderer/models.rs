//! Meshes and models on the GPU: the built-in shapes, uploaded glTF models,
//! and animated models posed for a frame.

use std::collections::BTreeMap;

use wish_core::math::{Mat4, Vec3};
use wish_core::{Diagnostic, Result};

use super::pipelines::material_group;
use super::{Renderer, buffer_with};
use crate::extract::{MaterialData, RenderScene};
use crate::gpu::Gpu;
use crate::mesh::{MeshData, VERTEX_STRIDE};
use crate::texture;

/// A mesh on the GPU.
#[derive(Clone)]
pub(super) struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
    /// The mesh's distinct edges as vertex index pairs, for the wireframe view.
    pub edges: wgpu::Buffer,
    pub edge_index_count: u32,
}

impl GpuMesh {
    /// Uploads a mesh: its vertices, its triangles and its edges.
    pub(super) fn new(gpu: &Gpu, mesh: &MeshData, label: &str) -> Self {
        let edges = crate::mesh::edge_indices(&mesh.indices);
        Self {
            vertices: buffer_with(gpu, label, &mesh.vertex_bytes(), wgpu::BufferUsages::VERTEX),
            indices: buffer_with(gpu, label, &mesh.index_bytes(), wgpu::BufferUsages::INDEX),
            index_count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
            edges: buffer_with(
                gpu,
                label,
                &edges
                    .iter()
                    .flat_map(|index| index.to_le_bytes())
                    .collect::<Vec<u8>>(),
                wgpu::BufferUsages::INDEX,
            ),
            edge_index_count: u32::try_from(edges.len()).unwrap_or(u32::MAX),
        }
    }

    /// The same triangles and edges over other vertices (a posed mesh): only
    /// the vertex buffer is new, the index and edge buffers are shared.
    /// `mesh` must have as many vertices as the mesh this one was made from.
    pub(super) fn with_vertices(&self, gpu: &Gpu, mesh: &MeshData, label: &str) -> Self {
        Self {
            vertices: buffer_with(gpu, label, &mesh.vertex_bytes(), wgpu::BufferUsages::VERTEX),
            indices: self.indices.clone(),
            index_count: self.index_count,
            edges: self.edges.clone(),
            edge_index_count: self.edge_index_count,
        }
    }

    /// How many triangles it has.
    pub(super) fn triangles(&self) -> u64 {
        u64::from(self.index_count / 3)
    }
}

/// The bounding box of some points; a point at the origin if there are none.
pub(super) fn bounds_of(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut iter = points.iter().copied();
    let Some(first) = iter.next() else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    iter.fold((first, first), |(low, high), point| {
        (low.min(point), high.max(point))
    })
}

/// One primitive of an uploaded model.
#[derive(Clone)]
pub(super) struct GpuPrimitive {
    pub mesh: GpuMesh,
    /// The material's five textures and the sampler.
    pub textures: wgpu::BindGroup,
    pub material: MaterialData,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    /// 0 unless the material masks by alpha.
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    /// Whether the material blends (glTF `BLEND`): the part is see-through.
    pub blend: bool,
    /// Primitive space to the model's space.
    pub matrix: Mat4,
    /// The primitive's bounding box in its own space.
    pub bounds: (Vec3, Vec3),
    /// Where it is in the model file (for posing animated models).
    pub placed: wish_import::Placed,
    /// Whether the file gives it texture coordinates (a material file with
    /// textures needs them).
    pub has_uvs: bool,
}

/// Posed primitives of animated draws, by draw index.
pub(super) type Posed = BTreeMap<usize, Vec<GpuPrimitive>>;

/// A model on the GPU.
pub(super) struct GpuModel {
    pub primitives: Vec<GpuPrimitive>,
    /// The model's CPU data, kept when it has skins or clips: posing needs
    /// the node tree, the keyframes and the bind-pose vertices.
    pub animated: Option<std::sync::Arc<wish_import::Model>>,
}

impl Renderer {
    /// Whether a model has been uploaded under `path`.
    pub fn has_model(&self, path: &str) -> bool {
        self.models.contains_key(path)
    }

    /// Uploads a model so that `Model` entities whose asset is `path` are
    /// drawn. Uploading again replaces the old copy (a changed file).
    pub fn upload_model(
        &mut self,
        gpu: &Gpu,
        path: &str,
        model: &wish_import::Model,
    ) -> Result<()> {
        // The device's limits, checked here so that a big model is refused
        // with its real cause instead of as a driver validation error.
        let limits = gpu.device.limits();
        for texture in &model.textures {
            if texture.width.max(texture.height) > limits.max_texture_dimension_2d {
                return Err(Diagnostic::error(
                    "texture_too_large_for_gpu",
                    format!(
                        "A texture in '{path}' is {}x{} pixels; this GPU takes at most {} per side.",
                        texture.width, texture.height, limits.max_texture_dimension_2d
                    ),
                    "Scale the texture down in the modelling tool (4096x4096 is plenty for most surfaces).",
                )
                .at_file(path));
            }
        }
        for mesh in &model.meshes {
            for primitive in &mesh.primitives {
                let bytes = primitive.positions.len() as u64 * VERTEX_STRIDE;
                if bytes > limits.max_buffer_size {
                    return Err(Diagnostic::error(
                        "mesh_too_large_for_gpu",
                        format!(
                            "A mesh in '{path}' needs {bytes} bytes of vertices; this GPU allows buffers of at most {}.",
                            limits.max_buffer_size
                        ),
                        "Split the mesh into several parts in the modelling tool, or reduce its detail.",
                    )
                    .at_file(path));
                }
            }
        }
        let uploaded = gpu.checked("upload a model", |gpu| {
            // Each texture is uploaded once per way it is used (colour or
            // data), because that decides how the GPU decodes it.
            let mut views: BTreeMap<(usize, bool), wgpu::TextureView> = BTreeMap::new();
            let mut view_of = |slot: Option<wish_import::TextureRef>, srgb: bool| {
                let reference = slot?;
                let texture = model.textures.get(reference.texture)?;
                Some(
                    views
                        .entry((reference.texture, srgb))
                        .or_insert_with(|| texture::upload(gpu, texture, srgb, path))
                        .clone(),
                )
            };
            let white = texture::solid(gpu, [255; 4], true, "white");
            let white_linear = texture::solid(gpu, [255; 4], false, "white data");
            let flat_normal = texture::solid(gpu, [128, 128, 255, 255], false, "flat normal");
            let default_material = wish_import::Material::default();
            // One bind group per material; primitives without one use the
            // default material.
            let mut groups = Vec::new();
            for material in model.materials.iter().chain([&default_material]) {
                let base =
                    view_of(material.base_color_texture, true).unwrap_or_else(|| white.clone());
                let mr = view_of(material.metallic_roughness_texture, false)
                    .unwrap_or_else(|| white_linear.clone());
                let normal =
                    view_of(material.normal_texture, false).unwrap_or_else(|| flat_normal.clone());
                let occlusion = view_of(material.occlusion_texture, false)
                    .unwrap_or_else(|| white_linear.clone());
                let emissive =
                    view_of(material.emissive_texture, true).unwrap_or_else(|| white.clone());
                groups.push(material_group(
                    gpu,
                    &self.pipelines.material_layout,
                    &self.pipelines.sampler,
                    [&base, &mr, &normal, &occlusion, &emissive],
                ));
            }
            let mut primitives = Vec::new();
            for placed in model.placed() {
                let Some(source) = model
                    .meshes
                    .get(placed.mesh)
                    .and_then(|mesh| mesh.primitives.get(placed.primitive))
                else {
                    continue;
                };
                let index = source
                    .material
                    .filter(|index| *index < model.materials.len());
                let material = index
                    .and_then(|index| model.materials.get(index))
                    .unwrap_or(&default_material);
                let group_index = index.unwrap_or(model.materials.len());
                let Some(textures) = groups.get(group_index) else {
                    continue;
                };
                primitives.push(GpuPrimitive {
                    mesh: GpuMesh::new(gpu, &MeshData::from(source), path),
                    textures: textures.clone(),
                    material: MaterialData {
                        base_color: material.base_color,
                        metallic: material.metallic,
                        roughness: material.roughness,
                        emissive: material.emissive,
                    },
                    normal_scale: material.normal_scale,
                    occlusion_strength: material.occlusion_strength,
                    alpha_cutoff: if material.alpha_mode == wish_import::AlphaMode::Mask {
                        material.alpha_cutoff.max(1e-4)
                    } else {
                        0.0
                    },
                    double_sided: material.double_sided,
                    blend: material.alpha_mode == wish_import::AlphaMode::Blend,
                    matrix: placed.matrix,
                    bounds: bounds_of(&source.positions),
                    placed,
                    has_uvs: source
                        .uvs
                        .iter()
                        .any(|uv| *uv != wish_core::math::Vec2::ZERO),
                });
            }
            let animated = (!model.skins.is_empty() || !model.animations.is_empty())
                .then(|| std::sync::Arc::new(model.clone()));
            GpuModel {
                primitives,
                animated,
            }
        })?;
        self.models.insert(path.to_owned(), uploaded);
        Ok(())
    }

    /// The primitives of every animated model draw of `scene`, posed at its
    /// clip's time (skinned vertices bent on the CPU), by draw index. Draws
    /// of models without skins or clips are not here.
    pub(super) fn pose(&self, gpu: &Gpu, scene: &RenderScene) -> Result<Posed> {
        let mut posed = Posed::new();
        for (index, draw) in scene.draws.iter().enumerate() {
            let Some((clip, time)) = &draw.animation else {
                continue;
            };
            let file = draw
                .model_path
                .as_deref()
                .unwrap_or_default()
                .split('#')
                .next()
                .unwrap_or_default();
            let Some(model) = self.models.get(file) else {
                continue;
            };
            let Some(source) = &model.animated else {
                continue;
            };
            let pose = source.pose(source.animation_index(clip), *time);
            let mut primitives = Vec::with_capacity(model.primitives.len());
            for primitive in &model.primitives {
                let mut moved = primitive.clone();
                let skinned = source.skin(&primitive.placed, &pose);
                match skinned {
                    Some((positions, normals)) => {
                        let Some(original) = source
                            .meshes
                            .get(primitive.placed.mesh)
                            .and_then(|m| m.primitives.get(primitive.placed.primitive))
                        else {
                            continue;
                        };
                        moved.bounds = bounds_of(&positions);
                        // The triangles do not change with the pose: only
                        // the vertices are uploaded again.
                        let data = MeshData {
                            positions,
                            normals,
                            uvs: original.uvs.clone(),
                            tangents: original.tangents.clone(),
                            indices: Vec::new(),
                        };
                        moved.mesh = gpu.checked("upload a posed mesh", |gpu| {
                            primitive.mesh.with_vertices(gpu, &data, file)
                        })?;
                        moved.matrix = Mat4::IDENTITY;
                    }
                    None => {
                        moved.matrix = pose
                            .get(primitive.placed.node)
                            .copied()
                            .unwrap_or(primitive.matrix);
                    }
                }
                primitives.push(moved);
            }
            posed.insert(index, primitives);
        }
        Ok(posed)
    }
}

#[cfg(test)]
mod tests {
    use wish_ecs::World;

    use super::*;
    use crate::gpu::GpuOptions;

    const MODEL: &str = "assets/bar.gltf";

    /// A scene with the bending bar a second into its clip.
    fn bent_bar() -> RenderScene {
        let mut world = World::new();
        wish_transform::register(&mut world).unwrap();
        crate::components::register(&mut world).unwrap();
        wish_scene::instantiate::register(&mut world).unwrap();
        world
            .register_component::<wish_types::anim::AnimationPlayer>()
            .unwrap();
        let source = format!(
            r#"{{ "format": 1, "entities": {{
                "camera": {{ "components": {{ "Camera": {{}}, "Transform": {{ "translation": [0, 1, 5] }} }} }},
                "bar": {{ "components": {{ "Model": {{ "asset": "{MODEL}" }},
                    "AnimationPlayer": {{ "clip": "bend", "time": 1.0 }} }} }}
            }} }}"#
        );
        let result = wish_scene::load(&source, "scenes/anim.scene.json", world.registry());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        result.document.unwrap().instantiate(&mut world).unwrap();
        wish_transform::update_global_transforms(&mut world).unwrap();
        crate::extract::extract(&world).unwrap()
    }

    #[test]
    fn a_posed_mesh_shares_its_triangles_and_edges_with_the_uploaded_one() {
        let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
        let mut renderer = Renderer::new(&gpu).unwrap();
        let model =
            wish_import::load_model_bytes(wish_import::samples::bending_bar().as_bytes(), MODEL)
                .unwrap();
        renderer.upload_model(&gpu, MODEL, &model).unwrap();
        let scene = bent_bar();
        let posed = renderer.pose(&gpu, &scene).unwrap();
        let uploaded = &renderer.models[MODEL].primitives;
        let posed = posed.values().next().expect("the bar is animated");
        assert_eq!(posed.len(), uploaded.len());
        let mut skinned = 0;
        for (posed, uploaded) in posed.iter().zip(uploaded) {
            // The same buffers, not copies of them.
            assert_eq!(posed.mesh.indices, uploaded.mesh.indices);
            assert_eq!(posed.mesh.edges, uploaded.mesh.edges);
            assert_eq!(posed.mesh.index_count, uploaded.mesh.index_count);
            assert_eq!(posed.mesh.edge_index_count, uploaded.mesh.edge_index_count);
            if posed.mesh.vertices != uploaded.mesh.vertices {
                skinned += 1;
            }
        }
        assert!(skinned > 0, "the bent part has vertices of its own");
        // Posing again makes new vertices and still no new triangles.
        let again = renderer.pose(&gpu, &scene).unwrap();
        let again = again.values().next().expect("the bar is animated");
        for (again, uploaded) in again.iter().zip(uploaded) {
            assert_eq!(again.mesh.indices, uploaded.mesh.indices);
        }
    }
}
