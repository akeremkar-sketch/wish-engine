//! Drawing for the tools: the renderer is opened by the first tool that
//! needs it (so everything else works without a GPU), models are loaded from
//! the project and reloaded when their files change.

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
            shaders: shaders.map(Path::to_path_buf),
        })
    }

    /// Whether this drawing was opened with these shaders.
    pub fn same_shaders(&self, shaders: Option<&Path>) -> bool {
        self.shaders.as_deref() == shaders
    }

    /// Uploads the models `scene` uses (again if their files changed). A
    /// model that cannot be loaded is a warning: the rest still draws.
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
        warnings
    }
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
