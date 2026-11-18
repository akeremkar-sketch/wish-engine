//! The draw list: a scene's draws turned into draw calls, once per frame.
//!
//! Every pass that draws meshes (shadow, opaque, and each debug view) walks
//! the same list, so what one of them draws is what the others draw. A draw
//! call is resolved here, once: its mesh, its material's textures, whether
//! back faces are culled and whether alpha cuts pixels out.

use wish_core::Diagnostic;
use wish_core::math::{Mat4, Vec3};

use super::models::{GpuMesh, Posed};
use super::pool::Pool;
use super::{Bytes, DRAW_UNIFORM_SIZE, DRAW_UNIFORM_STRIDE, Renderer, shape_index};
use crate::extract::{MaterialData, Mesh, RenderScene};
use crate::gpu::Gpu;

/// One draw call.
pub(super) struct DrawItem<'a> {
    /// What is drawn.
    pub mesh: &'a GpuMesh,
    /// The material's five textures and the sampler.
    pub textures: &'a wgpu::BindGroup,
    /// Whether back faces are drawn too.
    pub double_sided: bool,
    /// Whether the material cuts pixels out by alpha (alpha mode MASK): its
    /// shadow has the same holes.
    pub masked: bool,
    /// Where its uniform data starts in [`DrawList::bytes`].
    pub offset: u32,
}

/// The draw calls of a frame and their uniform data.
#[derive(Default)]
pub(super) struct DrawList<'a> {
    /// Every draw's uniform data, [`DRAW_UNIFORM_STRIDE`] apart (at least one
    /// stride long, so that there is always something to bind).
    pub bytes: Bytes,
    /// The draw calls, in scene order.
    pub items: Vec<DrawItem<'a>>,
    /// For each draw: its matrix, the corners of its local bounding box and
    /// its identity, to fit the shadow map around everything that can cast a
    /// shadow and to measure distances.
    pub casters: Vec<(Mat4, Vec3, Vec3, u32)>,
}

/// How a draw's surface is lit: the numbers the material side of the shader
/// reads.
#[derive(Clone, Copy)]
struct Surface<'a> {
    material: &'a MaterialData,
    normal_scale: f32,
    occlusion_strength: f32,
    /// 0 unless alpha masks.
    alpha_cutoff: f32,
    /// The scene draw this belongs to, plus one (0 means nothing); what the
    /// identity view writes.
    id: u32,
}

impl<'a> DrawList<'a> {
    fn push(
        &mut self,
        (mesh, textures, double_sided): (&'a GpuMesh, &'a wgpu::BindGroup, bool),
        world: &Mat4,
        bounds: (Vec3, Vec3),
        surface: Surface<'_>,
    ) {
        let Surface {
            material,
            normal_scale,
            occlusion_strength,
            alpha_cutoff,
            id,
        } = surface;
        self.casters.push((*world, bounds.0, bounds.1, id));
        let start = self.items.len() as u64 * DRAW_UNIFORM_STRIDE;
        self.bytes.pad_to(start);
        self.bytes.mat4(world);
        self.bytes.mat4(&world.inverse().transpose());
        self.bytes.floats(&material.base_color);
        self.bytes
            .floats(&[material.metallic, material.roughness, 0.0, 0.0]);
        self.bytes.vec4(material.emissive, 0.0);
        self.bytes
            .floats(&[normal_scale, occlusion_strength, alpha_cutoff, id as f32]);
        self.items.push(DrawItem {
            mesh,
            textures,
            double_sided,
            masked: alpha_cutoff > 0.0,
            offset: u32::try_from(start).unwrap_or(u32::MAX),
        });
    }

    /// How many triangles the draw calls submit.
    pub(super) fn triangles(&self) -> u64 {
        self.items.iter().map(|item| item.mesh.triangles()).sum()
    }

    /// Puts the uniform data into the pooled buffer (the lit passes and the
    /// debug views share it) and returns this frame's bind group, which
    /// picks one draw's data by its offset. Call inside `Gpu::checked`.
    pub(super) fn upload(
        &self,
        gpu: &Gpu,
        pool: &mut Pool,
        layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        let buffer = pool.buffer(gpu, "draws", wgpu::BufferUsages::UNIFORM, &self.bytes.0);
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("draws"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(DRAW_UNIFORM_SIZE),
                }),
            }],
        })
    }
}

/// What `collect` produced: the draw calls and what could not be drawn.
pub(super) struct Collected<'a> {
    pub draw_list: DrawList<'a>,
    pub warnings: Vec<Diagnostic>,
}

impl Renderer {
    /// Turns the scene's draws into draw calls. With `only`, just the draw
    /// with that index in `scene.draws` (to measure one entity on its own).
    pub(super) fn collect<'a>(
        &'a self,
        scene: &RenderScene,
        only: Option<usize>,
        posed: &'a Posed,
    ) -> Collected<'a> {
        let mut warnings = Vec::new();
        let mut draw_list = DrawList::default();
        let mut blended_models: Vec<&str> = Vec::new();
        for (index, draw) in scene.draws.iter().enumerate() {
            if only.is_some_and(|only| only != index) {
                continue;
            }
            let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if draw.world.determinant().abs() < 1e-12 || !draw.world.is_finite() {
                warnings.push(Diagnostic::warning(
                    "degenerate_transform",
                    format!(
                        "Entity {} was not drawn: its scale is zero or not finite.",
                        scene.name_of(draw.entity)
                    ),
                    "Give its Transform (and its parents') a non-zero, finite scale.",
                ));
                continue;
            }
            match draw.mesh {
                // A built-in shape, with the entity's own `Material`.
                Mesh::Primitive(shape) => {
                    let Some(mesh) = self.shapes.get(shape_index(shape)) else {
                        continue;
                    };
                    draw_list.push(
                        (mesh, &self.default_textures, false),
                        &draw.world,
                        (Vec3::splat(-0.5), Vec3::splat(0.5)),
                        Surface {
                            material: &draw.material,
                            normal_scale: 1.0,
                            occlusion_strength: 1.0,
                            alpha_cutoff: 0.0,
                            id,
                        },
                    );
                }
                // Each primitive of an uploaded model, with the material
                // from its file.
                Mesh::Model => {
                    let path = draw.model_path.as_deref().unwrap_or_default();
                    // A `#sub` part names something inside the file; the
                    // whole model is drawn for now.
                    let file = path.split('#').next().unwrap_or(path);
                    let Some((key, model)) = self.models.get_key_value(file) else {
                        warnings.push(Diagnostic::warning(
                            "model_not_loaded",
                            format!(
                                "Entity {} was not drawn: the model '{file}' has not been loaded.",
                                scene.name_of(draw.entity)
                            ),
                            "Load the model before drawing (wen capture does it from the scene's assets); check the path in the Model component.",
                        )
                        .at_file(file));
                        continue;
                    };
                    if model.blended > 0 && !blended_models.contains(&key.as_str()) {
                        blended_models.push(key.as_str());
                    }
                    let primitives = posed.get(&index).unwrap_or(&model.primitives);
                    for primitive in primitives {
                        draw_list.push(
                            (&primitive.mesh, &primitive.textures, primitive.double_sided),
                            &(draw.world * primitive.matrix),
                            primitive.bounds,
                            Surface {
                                material: &primitive.material,
                                normal_scale: primitive.normal_scale,
                                occlusion_strength: primitive.occlusion_strength,
                                alpha_cutoff: primitive.alpha_cutoff,
                                id,
                            },
                        );
                    }
                }
            }
        }
        for key in blended_models {
            warnings.push(Diagnostic::warning(
                "alpha_blend_not_blended",
                format!("The model '{key}' has materials that blend alpha; they are drawn opaque."),
                "Blended transparency is not drawn yet. For cut-outs such as leaves or fences, set the material's alpha mode to MASK in the modelling tool; glass and smoke stay solid for now.",
            ));
        }
        // Always something to bind, even with nothing to draw.
        draw_list
            .bytes
            .pad_to((draw_list.items.len().max(1) as u64) * DRAW_UNIFORM_STRIDE);

        Collected {
            draw_list,
            warnings,
        }
    }
}
