//! The draw list: a scene's draws turned into draw calls, once per frame.
//!
//! Every pass that draws meshes (shadow, opaque, and each debug view) walks
//! the same list, so what one of them draws is what the others draw. A draw
//! call is resolved here, once: its mesh, its material's textures, whether
//! back faces are culled and whether alpha cuts pixels out.

use std::borrow::Cow;
use std::collections::BTreeSet;

use wish_core::Diagnostic;
use wish_core::math::{Mat4, Vec3};

use super::materials::{TextureSet, texture_missing};
use super::models::{GpuMesh, Posed};
use super::pool::Pool;
use super::{Bytes, DRAW_UNIFORM_SIZE, DRAW_UNIFORM_STRIDE, Renderer, shape_index};
use crate::extract::{Draw, MaterialData, Mesh, RenderScene, SurfaceData};
use crate::gpu::Gpu;

/// One draw call.
pub(super) struct DrawItem<'a> {
    /// What is drawn.
    pub mesh: &'a GpuMesh,
    /// The material's five textures and the sampler: a model's own group or
    /// the default one, borrowed, or the group of a set of project textures.
    pub textures: Cow<'a, wgpu::BindGroup>,
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
    /// How the textures are laid on the surface: scale along u and v, then
    /// offset along u and v.
    uv: [f32; 4],
    /// The scene draw this belongs to, plus one (0 means nothing); what the
    /// identity view writes.
    id: u32,
}

impl<'a> DrawList<'a> {
    fn push(
        &mut self,
        (mesh, textures, double_sided): (&'a GpuMesh, Cow<'a, wgpu::BindGroup>, bool),
        world: &Mat4,
        bounds: (Vec3, Vec3),
        surface: Surface<'_>,
    ) {
        let Surface {
            material,
            normal_scale,
            occlusion_strength,
            alpha_cutoff,
            uv,
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
        self.bytes.floats(&uv);
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
        gpu: &Gpu,
        scene: &RenderScene,
        only: Option<usize>,
        posed: &'a Posed,
    ) -> Collected<'a> {
        let mut warnings = Vec::new();
        let mut draw_list = DrawList::default();
        let mut blended_models: Vec<&str> = Vec::new();
        // What has been said already, so that a file a hundred entities
        // share is one warning.
        let mut said = Said::default();
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
            // The look of the material file the draw names, when the
            // renderer keeps it; one it does not keep is said once and the
            // draw falls back to what it has itself.
            let from_file = draw.material_file.as_deref().and_then(|file| {
                let look = self.materials.files.get(file);
                if look.is_none() && said.first("material_not_loaded", file) {
                    warnings.push(material_missing(scene, draw, file));
                }
                look
            });
            match draw.mesh {
                // A built-in shape, with the entity's `Material` or the
                // material file it names.
                Mesh::Primitive(shape) => {
                    // The look of a file with textures: the shape's textured
                    // mesh and the textures' bind group.
                    let textured =
                        from_file
                            .filter(|look| look.surface.textured())
                            .and_then(|look| {
                                let set = self.texture_set(gpu, &look.surface.textures);
                                if set.is_none() {
                                    said.not_bound(
                                        scene,
                                        draw,
                                        draw.material_file.as_deref().unwrap_or_default(),
                                        &mut warnings,
                                    );
                                }
                                Some((look, set?))
                            });
                    match textured {
                        Some((look, set)) => {
                            let Some(mesh) = self.textured_shapes.get(shape_index(shape)) else {
                                continue;
                            };
                            said.missing_textures(
                                scene,
                                draw,
                                &look.surface,
                                &set.bound,
                                &mut warnings,
                            );
                            draw_list.push(
                                (mesh, Cow::Owned(set.group), look.surface.double_sided),
                                &draw.world,
                                (Vec3::splat(-0.5), Vec3::splat(0.5)),
                                textured_surface(&look.material, &look.surface, &set.bound, id),
                            );
                        }
                        // No textures: the shape as it was always drawn,
                        // from the mesh and with the textures it always
                        // had. Without a file every number below is what it
                        // always was, too.
                        None => {
                            let Some(mesh) = self.shapes.get(shape_index(shape)) else {
                                continue;
                            };
                            let (material, double_sided, alpha_cutoff) = match from_file {
                                Some(look) => (
                                    &look.material,
                                    look.surface.double_sided,
                                    look.surface.alpha_cutoff,
                                ),
                                None => (&draw.material, false, 0.0),
                            };
                            draw_list.push(
                                (mesh, Cow::Borrowed(&self.default_textures), double_sided),
                                &draw.world,
                                (Vec3::splat(-0.5), Vec3::splat(0.5)),
                                Surface {
                                    material,
                                    normal_scale: 1.0,
                                    occlusion_strength: 1.0,
                                    alpha_cutoff,
                                    uv: [1.0, 1.0, 0.0, 0.0],
                                    id,
                                },
                            );
                        }
                    }
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
                    // A material file on a model replaces every material of
                    // the model: each part keeps its mesh and takes the
                    // file's look.
                    let replaced = from_file.and_then(|look| {
                        let set = if look.surface.textured() {
                            let set = self.texture_set(gpu, &look.surface.textures);
                            if set.is_none() {
                                said.not_bound(
                                    scene,
                                    draw,
                                    draw.material_file.as_deref().unwrap_or_default(),
                                    &mut warnings,
                                );
                            }
                            set?
                        } else {
                            TextureSet {
                                group: self.default_textures.clone(),
                                bound: [false; 5],
                            }
                        };
                        Some((look, set))
                    });
                    if let Some((look, set)) = &replaced {
                        said.missing_textures(
                            scene,
                            draw,
                            &look.surface,
                            &set.bound,
                            &mut warnings,
                        );
                        if look.surface.textured()
                            && primitives.iter().any(|primitive| !primitive.has_uvs)
                            && said.first("material_needs_uvs", key)
                        {
                            warnings.push(
                                Diagnostic::warning(
                                    "material_needs_uvs",
                                    format!(
                                        "The model '{key}' has parts without texture coordinates; on them the textures of the material file '{}' show as one flat colour.",
                                        draw.material_file.as_deref().unwrap_or_default()
                                    ),
                                    "Unwrap the model in the modelling tool and export it with texture coordinates (UVs), or give this entity a material file without textures.",
                                )
                                .at_file(key.clone()),
                            );
                        }
                    }
                    for primitive in primitives {
                        match &replaced {
                            Some((look, set)) => draw_list.push(
                                (
                                    &primitive.mesh,
                                    Cow::Owned(set.group.clone()),
                                    look.surface.double_sided,
                                ),
                                &(draw.world * primitive.matrix),
                                primitive.bounds,
                                textured_surface(&look.material, &look.surface, &set.bound, id),
                            ),
                            None => draw_list.push(
                                (
                                    &primitive.mesh,
                                    Cow::Borrowed(&primitive.textures),
                                    primitive.double_sided,
                                ),
                                &(draw.world * primitive.matrix),
                                primitive.bounds,
                                Surface {
                                    material: &primitive.material,
                                    normal_scale: primitive.normal_scale,
                                    occlusion_strength: primitive.occlusion_strength,
                                    alpha_cutoff: primitive.alpha_cutoff,
                                    uv: [1.0, 1.0, 0.0, 0.0],
                                    id,
                                },
                            ),
                        }
                    }
                }
            }
        }
        warnings.extend(said.summary());
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

/// The most material warnings one picture gives; the rest are counted in
/// one `warnings_cut_short`.
const MAX_SAID: usize = 32;

/// What a collection has warned about already (a code and what it was
/// about), and how many more warnings there were than [`MAX_SAID`].
#[derive(Default)]
struct Said {
    seen: BTreeSet<(&'static str, String)>,
    more: usize,
}

impl Said {
    /// Whether to warn now: the first time `code` comes up for `what`, and
    /// fewer than [`MAX_SAID`] warnings so far. A new one beyond that is
    /// counted instead.
    fn first(&mut self, code: &'static str, what: &str) -> bool {
        if self.seen.contains(&(code, what.to_owned())) {
            return false;
        }
        self.seen.insert((code, what.to_owned()));
        if self.seen.len() > MAX_SAID {
            self.more += 1;
            return false;
        }
        true
    }

    /// The warning that stands for the ones not given, if any.
    fn summary(&self) -> Option<Diagnostic> {
        (self.more > 0).then(|| {
            Diagnostic::warning(
                "warnings_cut_short",
                format!(
                    "{MAX_SAID} warnings about materials and textures are listed; there are {} more.",
                    self.more
                ),
                "Fix the ones listed (often one missing file stands behind many); the rest show up when they are the first.",
            )
        })
    }

    /// The warning for a material file whose textures the GPU would not
    /// bind, once for each file.
    fn not_bound(
        &mut self,
        scene: &RenderScene,
        draw: &Draw,
        file: &str,
        warnings: &mut Vec<Diagnostic>,
    ) {
        if self.first("texture_not_loaded", &format!("bind {file}")) {
            warnings.push(
                Diagnostic::warning(
                    "texture_not_loaded",
                    format!(
                        "Entity {} is drawn without the textures of '{file}': the GPU would not bind them (a shape keeps the file's numbers, a model its own materials).",
                        scene.name_of(draw.entity)
                    ),
                    "This is a GPU problem, not one of the file: check the adapter's log (capture_frame's warnings), or try the software adapter.",
                )
                .at_file(file),
            );
        }
    }

    /// Warns, once for each file and slot, about the textures of `surface`
    /// that a draw names but that are not uploaded.
    fn missing_textures(
        &mut self,
        scene: &RenderScene,
        draw: &Draw,
        surface: &SurfaceData,
        bound: &[bool; 5],
        warnings: &mut Vec<Diagnostic>,
    ) {
        for (slot, (path, bound)) in surface.textures.iter().zip(bound).enumerate() {
            let Some(path) = path else { continue };
            if !*bound && self.first("texture_not_loaded", &format!("{slot} {path}")) {
                warnings.push(texture_missing(&scene.name_of(draw.entity), path, slot));
            }
        }
    }
}

/// The numbers of a surface drawn with a set of project textures.
fn textured_surface<'a>(
    material: &'a MaterialData,
    surface: &SurfaceData,
    bound: &[bool; 5],
    id: u32,
) -> Surface<'a> {
    let [_, _, normal_map, _, _] = *bound;
    Surface {
        material,
        // Without a normal map the surface keeps its own normal exactly:
        // the default texture's "straight out" is one step of 255 off.
        normal_scale: if normal_map {
            surface.normal_scale
        } else {
            0.0
        },
        occlusion_strength: surface.occlusion_strength,
        alpha_cutoff: surface.alpha_cutoff,
        uv: [
            surface.uv_scale[0],
            surface.uv_scale[1],
            surface.uv_offset[0],
            surface.uv_offset[1],
        ],
        id,
    }
}

/// The warning for a draw whose material file the renderer does not keep.
fn material_missing(scene: &RenderScene, draw: &Draw, file: &str) -> Diagnostic {
    let fallback = match draw.mesh {
        Mesh::Primitive(_) => "the numbers of its own Material",
        Mesh::Model => "the model's own materials",
    };
    Diagnostic::warning(
        "material_not_loaded",
        format!(
            "Entity {} is drawn with {fallback}: its material file '{file}' has not been loaded.",
            scene.name_of(draw.entity)
        ),
        "Check the path in Material.file and that the file is a valid material file (check says what is wrong with it); asset_list with kind material lists the project's material files.",
    )
    .at_file(file)
}
