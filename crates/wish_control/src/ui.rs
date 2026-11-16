//! The interface in the control plane (design note M6 §7): the project's
//! fonts, read once and kept until a file changes, put into every world the
//! tools look at (sessions and scene views) so that text is measured and
//! drawn with them.

use std::collections::BTreeMap;
use std::time::SystemTime;

use wish_core::{Diagnostic, Result};
use wish_ecs::World;
use wish_text::{Font, Fonts};

use crate::workspace::Workspace;

/// The most font files read.
const MAX_FONTS: usize = 200;

/// What is known of a font file, kept until it changes on disk.
#[derive(Clone, Debug)]
pub(crate) struct KnownFont {
    bytes: u64,
    modified: Option<SystemTime>,
    font: std::result::Result<Font, Diagnostic>,
}

/// The fonts read so far (part of the sessions).
pub(crate) type FontCache = BTreeMap<String, KnownFont>;

/// Whether `path` names a font file.
pub(crate) fn is_font(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".ttf") || lower.ends_with(".otf")
}

impl Workspace {
    /// The project's fonts (`.ttf` and `.otf` files), and what is wrong with
    /// the ones that cannot be used (`font_invalid`).
    pub(crate) fn fonts(&mut self) -> (Fonts, Vec<Diagnostic>) {
        let root = self.root().to_path_buf();
        let mut fonts = Fonts::default();
        let mut problems = Vec::new();
        let Ok(index) = wish_asset::AssetIndex::scan(&root) else {
            return (fonts, problems);
        };
        let paths: Vec<String> = index
            .paths()
            .filter(|path| is_font(path))
            .take(MAX_FONTS)
            .map(str::to_owned)
            .collect();
        for path in paths {
            let file = root.join(&path);
            let Ok(meta) = std::fs::metadata(&file) else {
                continue;
            };
            let (bytes, modified) = (meta.len(), meta.modified().ok());
            let fresh = self
                .sessions
                .fonts
                .get(&path)
                .is_some_and(|known| known.bytes == bytes && known.modified == modified);
            if !fresh {
                let font = if bytes > wish_text::MAX_FONT_BYTES as u64 {
                    Font::parse(vec![0; wish_text::MAX_FONT_BYTES + 1], &path)
                } else {
                    std::fs::read(&file)
                        .map_err(|error| {
                            Diagnostic::error(
                                "file_unreadable",
                                format!("Could not read '{path}': {error}."),
                                "Check that the file can be read.",
                            )
                            .at_file(&path)
                        })
                        .and_then(|data| Font::parse(data, &path))
                };
                self.sessions.fonts.insert(
                    path.clone(),
                    KnownFont {
                        bytes,
                        modified,
                        font,
                    },
                );
            }
            match self.sessions.fonts.get(&path).map(|known| &known.font) {
                Some(Ok(font)) => fonts.insert(&path, font.clone()),
                Some(Err(problem)) => problems.push(problem.clone()),
                None => {}
            }
        }
        (fonts, problems)
    }
}

/// Makes a world ready for the interface: the project's fonts, and the
/// layout worked out (a world about to tick, or one only drawn).
pub(crate) fn prepare(world: &mut World, fonts: Fonts) -> Result<()> {
    world.insert_extension(fonts);
    wish_ui::layout(world)
}
