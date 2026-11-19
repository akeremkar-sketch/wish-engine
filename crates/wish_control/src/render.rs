//! Drawing for the tools: the renderer is opened by the first tool that
//! needs it (so everything else works without a GPU), models, material
//! files and textures are loaded from the project and reloaded when their
//! files change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use wish_core::{Diagnostic, Result};
use wish_render::{Gpu, GpuOptions, RenderScene, Renderer};

/// The GPU, the renderer and which model files it holds.
pub(crate) struct Drawing {
    pub gpu: Arc<Gpu>,
    pub renderer: Renderer,
    /// Project-relative model path → (length, modified) when uploaded.
    models: BTreeMap<String, (u64, Option<SystemTime>)>,
    /// The same for the interface's pictures.
    pictures: BTreeMap<String, (u64, Option<SystemTime>)>,
    /// The same for material files and the textures of materials.
    materials: MaterialStamps,
    /// The shader folder in use, if any.
    shaders: Option<PathBuf>,
}

impl Drawing {
    pub fn open(software: bool, shaders: Option<&Path>) -> Result<Self> {
        let gpu = Gpu::shared(GpuOptions { software })?;
        let renderer = match shaders {
            Some(dir) => {
                Renderer::with_shaders(&gpu, &wish_render::engine_shaders_from_dir(dir)?.0)?
            }
            None => Renderer::new(&gpu)?,
        };
        Ok(Self {
            gpu,
            renderer,
            models: BTreeMap::new(),
            pictures: BTreeMap::new(),
            materials: MaterialStamps::default(),
            shaders: shaders.map(Path::to_path_buf),
        })
    }

    /// Whether this drawing was opened with these shaders.
    pub fn same_shaders(&self, shaders: Option<&Path>) -> bool {
        self.shaders.as_deref() == shaders
    }

    /// Uploads the models, material files and textures `scene` uses (again
    /// if their files changed). One that cannot be loaded is a warning: the
    /// rest still draws.
    pub fn load_models(&mut self, scene: &RenderScene, root: &Path) -> Vec<Diagnostic> {
        let mut warnings = Vec::new();
        let mut paths: Vec<&str> = scene
            .draws
            .iter()
            .filter_map(|draw| draw.model_path.as_deref())
            .map(|path| path.split('#').next().unwrap_or(path))
            .collect();
        paths.sort_unstable();
        paths.dedup();
        for path in paths {
            // A reference that is not a project-relative path (absolute, or
            // climbing out with ..) is never read: the project is the
            // boundary (M4 closing review).
            if wish_core::asset::validate_asset_ref(path).is_err() {
                warnings.push(
                    Diagnostic::warning(
                        "model_not_loaded",
                        format!(
                            "The model '{path}' was not drawn: it is not a path inside the project."
                        ),
                        "Model assets are project-relative paths such as \"assets/crate.glb\".",
                    )
                    .at_file(path),
                );
                continue;
            }
            let meta = std::fs::metadata(root.join(path)).ok();
            let stamp = (
                meta.as_ref().map_or(0, std::fs::Metadata::len),
                meta.and_then(|m| m.modified().ok()),
            );
            if self.models.get(path) == Some(&stamp) && self.renderer.has_model(path) {
                continue;
            }
            let loaded = wish_import::load_model_file(&root.join(path), path).and_then(|model| {
                self.renderer
                    .upload_model(&self.gpu, path, &model)
                    .map(|()| model)
            });
            match loaded {
                Ok(model) => {
                    self.models.insert(path.to_owned(), stamp);
                    warnings.extend(model.warnings);
                }
                Err(problem) => warnings.push(
                    Diagnostic::warning(
                        "model_not_loaded",
                        format!("The model '{path}' was not drawn: {}", problem.message),
                        problem.hint.clone(),
                    )
                    .at_file(path),
                ),
            }
        }
        warnings.extend(load_ui_pictures(
            &mut self.renderer,
            scene,
            root,
            &mut self.pictures,
        ));
        warnings.extend(load_materials(
            &self.gpu,
            &mut self.renderer,
            scene,
            root,
            &mut self.materials,
        ));
        warnings
    }
}

/// When the material files and material textures a renderer holds were
/// read, so that only changed files are read again; and the files that
/// could not be used, so that they are not read again until they change.
#[derive(Debug)]
pub struct MaterialStamps {
    /// Material files, by project path.
    files: BTreeMap<String, Stamp>,
    /// Textures, by project path and by whether they were uploaded to be
    /// read as colours; with their pixel count.
    textures: BTreeMap<(String, bool), (Stamp, u64)>,
    /// Material files that could not be used, with the stamp they had and
    /// the warning they gave.
    failed_files: BTreeMap<String, (Stamp, Diagnostic)>,
    /// Textures that could not be used (other than for the budget, which
    /// depends on the other textures), the same way.
    failed_textures: BTreeMap<(String, bool), (Stamp, Diagnostic)>,
    /// The most pixels all textures may have together.
    budget: u64,
    /// How many files were read whole (material files and pictures).
    reads: u64,
}

impl Default for MaterialStamps {
    fn default() -> Self {
        Self::with_pixel_budget(MAX_MATERIAL_TEXTURE_PIXELS)
    }
}

impl MaterialStamps {
    /// Nothing read yet, with a budget of `pixels` for all textures
    /// together instead of [`MAX_MATERIAL_TEXTURE_PIXELS`] (a smaller one
    /// lets tests reach it with small pictures).
    pub fn with_pixel_budget(pixels: u64) -> Self {
        Self {
            files: BTreeMap::new(),
            textures: BTreeMap::new(),
            failed_files: BTreeMap::new(),
            failed_textures: BTreeMap::new(),
            budget: pixels.min(MAX_MATERIAL_TEXTURE_PIXELS),
            reads: 0,
        }
    }

    /// How many files were read whole so far: material files and pictures.
    /// A picture refused from its header, or a file not read again because
    /// it has not changed, does not count.
    pub fn files_read(&self) -> u64 {
        self.reads
    }

    /// How many textures are held (each way of reading a file once).
    pub fn textures_held(&self) -> usize {
        self.textures.len()
    }

    /// The pixels of every texture held now.
    fn pixels(&self) -> u64 {
        self.textures.values().map(|(_, pixels)| *pixels).sum()
    }
}

/// The most pixels of material textures one renderer is given (all of them
/// together): the budget of a model's textures, about 1 GiB of memory.
pub const MAX_MATERIAL_TEXTURE_PIXELS: u64 = wish_import::MAX_TEXTURE_PIXELS;

fn stamp_of(file: &Path) -> Stamp {
    let meta = std::fs::metadata(file).ok();
    (
        meta.as_ref().map_or(0, std::fs::Metadata::len),
        meta.and_then(|m| m.modified().ok()),
    )
}

/// The width and height a PNG file says it has (its header), read without
/// decoding it. `None` when the file does not start like a PNG; the decoder
/// then says what is wrong with it.
fn png_size(file: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(file).ok()?.read_exact(&mut head).ok()?;
    if head.get(..8)? != b"\x89PNG\r\n\x1a\n" || head.get(12..16)? != b"IHDR" {
        return None;
    }
    let number = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?))
    };
    Some((number(16)?, number(20)?))
}

/// Reads the material file `path` of the project as the look it holds.
fn read_look(root: &Path, path: &str, bytes: u64) -> Result<wish_render::Look> {
    if bytes > wish_asset::MAX_MATERIAL_BYTES {
        return Err(Diagnostic::error(
            "material_file_too_large",
            format!(
                "it is {bytes} bytes; a material file is at most {} bytes.",
                wish_asset::MAX_MATERIAL_BYTES
            ),
            "A material file holds a few numbers and paths. Pictures go in their own PNG files, which the material names.",
        ));
    }
    let text = std::fs::read_to_string(root.join(path)).map_err(|error| {
        Diagnostic::error(
            "file_unreadable",
            format!("could not read it ({error})."),
            "Check the path: asset_list with kind material shows the project's material files.",
        )
    })?;
    let (material, texts) = wish_asset::parse_material(&text, path)?.decode()?;
    Ok(wish_render::Look::of(&material, &|reference| {
        texts.get(reference.text()).map(str::to_owned)
    }))
}

/// Loads what the materials of `scene` need, from the project in `root`
/// into `renderer`: the material files its entities name (`Material.file`)
/// and the textures those files name.
/// Only files that changed since `known` says they were read are read
/// again, so calling this before every picture is how a saved material
/// file or texture shows up at once. A file that could not be used is not
/// read again until it changes. What the scene no longer names is dropped,
/// from `known` and from the renderer, so the textures held (and the
/// budget below) are the scene's.
///
/// Nothing here can take the picture down. A material file that cannot be
/// used is a warning (`material_not_loaded`) and its entities are drawn
/// with their own numbers; a texture that cannot be used is a warning
/// (`texture_not_loaded`, naming the file and the slots that wanted it) and
/// its slots are drawn as if they were empty. Paths that are not inside
/// the project are never read. Pictures are decoded by
/// `wish_import::load_png` (guarded, at most 4096 pixels a side), and all
/// textures together are held to [`MAX_MATERIAL_TEXTURE_PIXELS`], checked
/// from a picture's header before it is decoded.
pub fn load_materials(
    gpu: &Gpu,
    renderer: &mut Renderer,
    scene: &RenderScene,
    root: &Path,
    known: &mut MaterialStamps,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    // The material files.
    let mut files: Vec<&str> = scene
        .draws
        .iter()
        .filter_map(|draw| draw.material_file.as_deref())
        .collect();
    files.sort_unstable();
    files.dedup();
    // Files the scene no longer names are forgotten.
    let gone: Vec<String> = known
        .files
        .keys()
        .chain(known.failed_files.keys())
        .filter(|path| files.binary_search(&path.as_str()).is_err())
        .cloned()
        .collect();
    for path in gone {
        renderer.remove_material(&path);
        known.files.remove(&path);
        known.failed_files.remove(&path);
    }
    for path in files.iter().copied() {
        let not_loaded = |why: String, hint: String| {
            Diagnostic::warning(
                "material_not_loaded",
                format!("The material file '{path}' is not used: {why}"),
                hint,
            )
            .at_file(path)
        };
        let problem = if wish_core::asset::validate_asset_ref(path).is_err() {
            Some(not_loaded(
                "it is not a path inside the project.".to_owned(),
                "Material files are project-relative paths such as \"assets/materials/brick.mat.json\".".to_owned(),
            ))
        } else if !wish_asset::is_material(path) {
            Some(not_loaded(
                "it is not a material file.".to_owned(),
                "Material.file needs a file whose name ends in .mat.json; a picture goes into a material file's base_color_texture.".to_owned(),
            ))
        } else {
            let stamp = stamp_of(&root.join(path));
            if known.files.get(path) == Some(&stamp) && renderer.material(path).is_some() {
                continue;
            }
            if let Some((at, problem)) = known.failed_files.get(path)
                && *at == stamp
            {
                // Not read again until the file changes.
                warnings.push(problem.clone());
                continue;
            }
            known.reads += 1;
            match read_look(root, path, stamp.0) {
                Ok(look) => {
                    renderer.set_material(path, look);
                    known.files.insert(path.to_owned(), stamp);
                    known.failed_files.remove(path);
                    None
                }
                Err(problem) => {
                    let problem = not_loaded(problem.message.to_string(), problem.hint.to_string());
                    known
                        .failed_files
                        .insert(path.to_owned(), (stamp, problem.clone()));
                    Some(problem)
                }
            }
        };
        if let Some(problem) = problem {
            // The last good look is not kept: what is drawn is what the
            // files say, and the warning says why it is not this file.
            renderer.remove_material(path);
            known.files.remove(path);
            warnings.push(problem);
        }
    }

    // The textures of the material files the renderer now has.
    let mut wanted: BTreeMap<(String, bool), Vec<&'static str>> = BTreeMap::new();
    for file in files.iter().copied() {
        let Some(look) = renderer.material(file) else {
            continue;
        };
        for (slot, path) in look.surface.textures.iter().enumerate() {
            let (Some(path), Some(name)) = (path, wish_render::TEXTURE_SLOTS.get(slot)) else {
                continue;
            };
            let slots = wanted
                .entry((path.clone(), wish_render::slot_is_colour(slot)))
                .or_default();
            if !slots.contains(name) {
                slots.push(name);
            }
        }
    }
    // Textures no material of the scene names are dropped, from the GPU
    // too: they no longer count against the budget.
    let unwanted: Vec<(String, bool)> = known
        .textures
        .keys()
        .chain(known.failed_textures.keys())
        .filter(|key| !wanted.contains_key(*key))
        .cloned()
        .collect();
    for key in unwanted {
        renderer.forget_texture(&key.0, key.1);
        known.textures.remove(&key);
        known.failed_textures.remove(&key);
    }
    // A file wanted both ways is decoded once.
    let mut decoded: Option<(String, wish_import::Texture)> = None;
    for ((path, colour), slots) in wanted {
        let path = path.as_str();
        let not_loaded = |why: String, hint: String| {
            Diagnostic::warning(
                "texture_not_loaded",
                format!(
                    "The texture '{path}' ({}) is not drawn: {why}",
                    slots.join(", ")
                ),
                hint,
            )
            .at_file(path)
        };
        let key = (path.to_owned(), colour);
        let file = root.join(path);
        let stamp = stamp_of(&file);
        if known.textures.get(&key).map(|(at, _)| at) == Some(&stamp)
            && renderer.has_texture(path, colour)
        {
            continue;
        }
        if let Some((at, problem)) = known.failed_textures.get(&key)
            && *at == stamp
        {
            // Not read again until the file changes.
            warnings.push(problem.clone());
            continue;
        }
        // What the other textures hold: this one replaces its own entry.
        let others = known
            .pixels()
            .saturating_sub(known.textures.get(&key).map_or(0, |(_, pixels)| *pixels));
        let budget = known.budget;
        let over_budget = |pixels: u64| {
            (others.saturating_add(pixels) > budget).then(|| {
                not_loaded(
                    format!(
                        "with it the scene's material textures would be more than {budget} pixels together."
                    ),
                    "Use smaller pictures (1024x1024 or 2048x2048 is plenty for most surfaces), or fewer different ones.".to_owned(),
                )
            })
        };
        // Problems that stay until the file changes, and the budget, which
        // changes with the other textures (checked from the header first,
        // so an over-budget picture is never decoded).
        let mut lasting = true;
        let texture = if wish_core::asset::validate_asset_ref(path).is_err() || path.contains('#') {
            Err(not_loaded(
                "it is not a path inside the project.".to_owned(),
                "Textures are project-relative paths such as \"assets/textures/brick.png\"."
                    .to_owned(),
            ))
        } else if !path.to_ascii_lowercase().ends_with(".png") {
            Err(not_loaded(
                "it is not a PNG file.".to_owned(),
                "Textures outside models are read from PNG files only. Convert the picture to PNG (8-bit) and name that file.".to_owned(),
            ))
        } else if stamp.0 > 4 * 4096 * 4096 + (1 << 20) {
            // Larger than any picture may be: not read at all.
            Err(not_loaded(
                format!(
                    "it is {} bytes, more than a 4096x4096 picture needs.",
                    stamp.0
                ),
                "Use a PNG of at most 4096x4096 pixels.".to_owned(),
            ))
        } else if let Some(problem) = png_size(&file)
            .and_then(|(width, height)| over_budget(u64::from(width) * u64::from(height)))
        {
            lasting = false;
            Err(problem)
        } else {
            let again = decoded
                .take()
                .filter(|(of, _)| of == path)
                .map(|(_, texture)| Ok(texture));
            again
                .unwrap_or_else(|| {
                    known.reads += 1;
                    std::fs::read(&file)
                        .map_err(|error| {
                            Diagnostic::error(
                                "file_unreadable",
                                format!("could not read it ({error})."),
                                "Check the path: asset_list with kind image shows the project's pictures.",
                            )
                        })
                        .and_then(|bytes| wish_import::load_png(&bytes, path))
                })
                .map_err(|problem| {
                    not_loaded(problem.message.to_string(), problem.hint.to_string())
                })
        };
        let uploaded = texture.and_then(|texture| {
            let pixels = u64::from(texture.width) * u64::from(texture.height);
            if let Some(problem) = over_budget(pixels) {
                lasting = false;
                return Err(problem);
            }
            renderer
                .upload_texture(gpu, path, colour, &texture)
                .map_err(|problem| {
                    not_loaded(problem.message.to_string(), problem.hint.to_string())
                })?;
            decoded = Some((path.to_owned(), texture));
            Ok(pixels)
        });
        match uploaded {
            Ok(pixels) => {
                known.textures.insert(key.clone(), (stamp, pixels));
                known.failed_textures.remove(&key);
            }
            Err(problem) => {
                // What the file held before is not drawn in its place.
                renderer.forget_texture(path, colour);
                known.textures.remove(&key);
                if lasting {
                    known.failed_textures.insert(key, (stamp, problem.clone()));
                }
                warnings.push(problem);
            }
        }
    }
    warnings
}

/// When a file was read: its length and modification time.
pub type Stamp = (u64, Option<SystemTime>);

/// Loads the pictures the interface and the sprites of `scene` show
/// (`UiImage`, `Sprite`), again if
/// their files changed since `known` says they were read. A picture that
/// cannot be loaded is a warning (`ui_image_not_loaded`): its box shows its
/// background only.
pub fn load_ui_pictures(
    renderer: &mut Renderer,
    scene: &RenderScene,
    root: &Path,
    known: &mut BTreeMap<String, Stamp>,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    let mut paths: Vec<&str> = scene
        .ui
        .iter()
        .filter_map(|view| view.image.as_ref().map(|(path, _)| path.as_str()))
        .chain(
            scene
                .sprites
                .iter()
                .filter_map(|view| view.image.as_deref()),
        )
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let not_loaded = |path: &str, why: String, hint: String| {
        Diagnostic::warning(
            "ui_image_not_loaded",
            format!("The picture '{path}' was not drawn: {why}"),
            hint,
        )
        .at_file(path)
    };
    for path in paths {
        if wish_core::asset::validate_asset_ref(path).is_err() {
            warnings.push(not_loaded(
                path,
                "it is not a path inside the project.".to_owned(),
                "Pictures are project-relative paths such as \"assets/ui/heart.png\".".to_owned(),
            ));
            continue;
        }
        let file = root.join(path);
        let meta = std::fs::metadata(&file).ok();
        let stamp = (
            meta.as_ref().map_or(0, std::fs::Metadata::len),
            meta.and_then(|m| m.modified().ok()),
        );
        if known.get(path) == Some(&stamp) && renderer.has_ui_picture(path) {
            continue;
        }
        // Larger than any picture may be: not read at all.
        let too_big = stamp.0 > 4 * 4096 * 4096 + (1 << 20);
        let loaded = (if too_big {
            Err(Diagnostic::error(
                "image_invalid",
                format!(
                    "it is {} bytes, more than a 4096x4096 picture needs.",
                    stamp.0
                ),
                "Use a PNG of at most 4096x4096 pixels.",
            ))
        } else {
            std::fs::read(&file).map_err(|error| {
                Diagnostic::error(
                    "file_unreadable",
                    format!("could not read it ({error})."),
                    "Check the path: asset_list shows the project's pictures.",
                )
            })
        })
        .and_then(|bytes| wish_import::load_png(&bytes, path));
        match loaded {
            Ok(texture) => {
                renderer.set_ui_picture(
                    path,
                    wish_render::UiPicture {
                        width: texture.width,
                        height: texture.height,
                        rgba: texture.rgba,
                    },
                );
                known.insert(path.to_owned(), stamp);
            }
            Err(problem) => warnings.push(not_loaded(
                path,
                problem.message.to_string(),
                problem.hint.to_string(),
            )),
        }
    }
    warnings
}

/// Repeats of one warning per file are dropped (the renderer says, per
/// entity, what loading said once).
pub(crate) fn without_repeats(mut warnings: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen: Vec<(String, String)> = Vec::new();
    warnings.retain(|warning| match warning.location.file.as_ref() {
        Some(file) => {
            let key = (warning.code.to_string(), file.clone());
            let fresh = !seen.contains(&key);
            seen.push(key);
            fresh
        }
        None => true,
    });
    warnings
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        out.push(char::from(ALPHABET[(n >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

/// Decodes standard base64 (padded); `None` for anything else.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &byte in chunk {
            let value = if byte == b'=' {
                pad += 1;
                0
            } else if pad > 0 {
                return None;
            } else {
                u32::try_from(ALPHABET.iter().position(|&c| c == byte)?).ok()?
            };
            n = (n << 6) | value;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}
#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_round_trips_and_rejects_garbage() {
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(super::base64_decode(&base64(&data)).unwrap(), data);
        assert!(super::base64_decode("abc").is_none() && super::base64_decode("a=bc").is_none());
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }
}
