//! Materials outside models: the looks of material files, the textures
//! they name, and the bind groups made from them.
//!
//! A model brings its materials and textures in its own file
//! (`models.rs`). Everything else comes from here:
//!
//! - **Textures** are uploaded under their project path, once for each way
//!   they are read: as colours (sRGB: base colour, emission) or as numbers
//!   (metal and roughness, normals, occlusion). Whoever owns the project's
//!   files reads and decodes them and calls [`Renderer::upload_texture`];
//!   the renderer never touches the disk.
//! - **Material files** are kept as the [`Look`] they hold, under their
//!   project path ([`Renderer::set_material`]). A draw whose `Material`
//!   names a file takes that look.
//! - **Bind groups:** one for each distinct set of five textures, made the
//!   first time a frame draws it and kept until a texture changes. They
//!   bind textures and the sampler only, never a uniform buffer, so keeping
//!   them does not meet the software adapter's fault with kept groups
//!   (`pool.rs`): the groups of uploaded models have always been kept the
//!   same way.
//!
//! A texture that is named but not there is drawn as the slot's default
//! (white, or a flat normal) with a warning that names the file and the
//! slot: one missing picture never takes a frame down.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Result};

use super::Renderer;
use super::pipelines::material_group;
use crate::extract::{Look, TEXTURE_SLOTS, slot_is_colour};
use crate::gpu::Gpu;
use crate::texture;

/// The most bind groups kept. A scene has one for each distinct set of
/// textures it draws, so this is far above any real use; game code that
/// writes a new set every tick starts again from an empty table instead of
/// growing one for ever.
const MAX_KEPT_GROUPS: usize = 4096;

/// The 1x1 textures of slots without a picture.
pub(super) struct DefaultTextures {
    /// White, read as a colour (base colour, emission).
    pub white: wgpu::TextureView,
    /// White, read as numbers (metal and roughness, occlusion).
    pub white_data: wgpu::TextureView,
    /// A normal that points straight out.
    pub flat_normal: wgpu::TextureView,
}

impl DefaultTextures {
    /// Uploads them. Call inside `Gpu::checked`.
    pub(super) fn new(gpu: &Gpu) -> Self {
        Self {
            white: texture::solid(gpu, [255; 4], true, "white"),
            white_data: texture::solid(gpu, [255; 4], false, "white data"),
            flat_normal: texture::solid(gpu, [128, 128, 255, 255], false, "flat normal"),
        }
    }

    /// The default of a slot, by its place in [`TEXTURE_SLOTS`].
    pub(super) fn of_slot(&self, slot: usize) -> &wgpu::TextureView {
        match slot {
            0 | 4 => &self.white,
            2 => &self.flat_normal,
            _ => &self.white_data,
        }
    }
}

/// A set of five textures as a bind group, and which of its slots really
/// hold the picture they name.
#[derive(Clone)]
pub(super) struct TextureSet {
    pub group: wgpu::BindGroup,
    /// For each slot: whether its picture is bound (not the default).
    pub bound: [bool; 5],
}

/// The textures, material files and bind groups a renderer keeps.
#[derive(Default)]
pub(super) struct MaterialStore {
    /// Uploaded textures, by project path and by whether they are read as
    /// colours.
    pub textures: BTreeMap<(String, bool), wgpu::TextureView>,
    /// The looks of material files, by project path.
    pub files: BTreeMap<String, Look>,
}

impl Renderer {
    /// Keeps the look of the material file `path`, replacing one of the
    /// same path: draws whose `Material.file` is `path` take it.
    pub fn set_material(&mut self, path: &str, look: Look) {
        self.materials.files.insert(path.to_owned(), look);
    }

    /// Forgets the material file `path` (it could not be read any more):
    /// draws that name it fall back to their own look, with a warning.
    pub fn remove_material(&mut self, path: &str) {
        self.materials.files.remove(path);
    }

    /// The look kept for the material file `path`.
    pub fn material(&self, path: &str) -> Option<&Look> {
        self.materials.files.get(path)
    }

    /// Uploads a texture under its project path, to be read as colours
    /// (`colour`: sRGB, for the base colour and emission slots) or as
    /// numbers (the other slots). Uploading again replaces the old copy (a
    /// changed file). Its mip chain is made on the CPU, as for the textures
    /// of models, so that every adapter draws from the same data.
    pub fn upload_texture(
        &mut self,
        gpu: &Gpu,
        path: &str,
        colour: bool,
        texture: &wish_import::Texture,
    ) -> Result<()> {
        let limit = gpu.device.limits().max_texture_dimension_2d;
        if texture.width.max(texture.height) > limit {
            return Err(Diagnostic::error(
                "texture_too_large_for_gpu",
                format!(
                    "The texture '{path}' is {}x{} pixels; this GPU takes at most {limit} per side.",
                    texture.width, texture.height
                ),
                "Scale the picture down in an image editor (2048x2048 is plenty for most surfaces).",
            )
            .at_file(path));
        }
        let expected = u64::from(texture.width) * u64::from(texture.height) * 4;
        if texture.width == 0 || texture.height == 0 || texture.rgba.len() as u64 != expected {
            return Err(Diagnostic::error(
                "image_invalid",
                format!("The texture '{path}' has no pixels, or not as many as its size says."),
                "Use an 8-bit PNG; re-export it from an image editor.",
            )
            .at_file(path));
        }
        let view = gpu.checked("upload a texture", |gpu| {
            texture::upload(gpu, texture, colour, path)
        })?;
        self.materials
            .textures
            .insert((path.to_owned(), colour), view);
        self.forget_groups();
        Ok(())
    }

    /// Forgets the texture `path`, however it was read (its file is gone or
    /// broken): slots that name it draw their default, with a warning.
    pub fn remove_texture(&mut self, path: &str) {
        for colour in [false, true] {
            self.materials.textures.remove(&(path.to_owned(), colour));
        }
        self.forget_groups();
    }

    /// Forgets the texture `path` as it was uploaded for one way of reading
    /// it (`colour`), freeing its GPU memory once no frame uses it; the
    /// other way, if uploaded, stays.
    pub fn forget_texture(&mut self, path: &str, colour: bool) {
        if self
            .materials
            .textures
            .remove(&(path.to_owned(), colour))
            .is_some()
        {
            self.forget_groups();
        }
    }

    /// How many textures are uploaded, counting each way of reading a file
    /// once.
    pub fn texture_count(&self) -> usize {
        self.materials.textures.len()
    }

    /// Whether a texture is uploaded under `path` for that way of reading
    /// it.
    pub fn has_texture(&self, path: &str, colour: bool) -> bool {
        self.materials
            .textures
            .contains_key(&(path.to_owned(), colour))
    }

    /// The bind groups were made over textures that have changed.
    fn forget_groups(&mut self) {
        match self.material_groups.get_mut() {
            Ok(groups) => groups.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
        self.material_groups.clear_poison();
    }

    /// The bind group of a set of textures (project paths in
    /// [`TEXTURE_SLOTS`] order), made on first use. A slot whose texture is
    /// not uploaded binds the slot's default; `bound` says which did.
    /// `None` only when the GPU refuses to make a group (the caller then
    /// draws with the default textures).
    pub(super) fn texture_set(
        &self,
        gpu: &Gpu,
        textures: &[Option<String>; 5],
    ) -> Option<TextureSet> {
        let mut groups = self.material_groups.lock().unwrap_or_else(|poisoned| {
            self.material_groups.clear_poison();
            let mut groups = poisoned.into_inner();
            groups.clear();
            groups
        });
        if let Some(set) = groups.get(textures) {
            return Some(set.clone());
        }
        let mut bound = [false; 5];
        let mut views: [&wgpu::TextureView; 5] =
            std::array::from_fn(|slot| self.default_views.of_slot(slot));
        for (slot, path) in textures.iter().enumerate() {
            let Some(path) = path else { continue };
            if let Some(view) = self
                .materials
                .textures
                .get(&(path.clone(), slot_is_colour(slot)))
                && let (Some(target), Some(flag)) = (views.get_mut(slot), bound.get_mut(slot))
            {
                *target = view;
                *flag = true;
            }
        }
        let group = gpu
            .checked("bind a material's textures", |gpu| {
                material_group(
                    gpu,
                    &self.pipelines.material_layout,
                    &self.pipelines.sampler,
                    views,
                )
            })
            .ok()?;
        let set = TextureSet { group, bound };
        if groups.len() >= MAX_KEPT_GROUPS {
            groups.clear();
        }
        groups.insert(textures.clone(), set.clone());
        Some(set)
    }
}

/// The warning for a slot whose texture is named but not uploaded.
pub(super) fn texture_missing(entity: &str, path: &str, slot: usize) -> Diagnostic {
    let slot = TEXTURE_SLOTS.get(slot).copied().unwrap_or("texture");
    Diagnostic::warning(
        "texture_not_loaded",
        format!(
            "Entity {entity} is drawn without its {slot} '{path}': that picture has not been loaded."
        ),
        "Check that the file exists and is a PNG the engine can read (asset_info on it says what is wrong); until then the surface is drawn as if the slot were empty.",
    )
    .at_file(path)
}
