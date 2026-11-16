//! Fonts and text layout (design note M6 §7).
//!
//! The same code measures text for the simulation (the interface layout
//! needs a text's size every tick, so it must be deterministic: only
//! additions and multiplications of the font's own numbers) and places
//! glyphs for the renderer, so what is drawn is exactly what was laid out.
//!
//! `ab_glyph` (and the font parser under it) is used only here. Font files
//! come from projects and are not trusted: reading one is guarded
//! ([`Font::parse`]) and a bad file is a `font_invalid` diagnostic, never a
//! crash.
//!
//! The engine's own font is Roboto 2.138 (Google, Apache-2.0; see
//! `fonts/LICENSE-Roboto.txt`).

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontArc, GlyphId, PxScale, ScaleFont as _};
use wish_core::{Diagnostic, Result};

/// The engine's font file (Roboto Regular).
pub const DEFAULT_FONT: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");

/// The largest font file read.
pub const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

/// The most characters of one text that are laid out; the rest is left out.
pub const MAX_CHARS: usize = 10_000;

/// A font, ready to measure and draw with. Cheap to clone.
#[derive(Clone)]
pub struct Font {
    inner: FontArc,
    name: Arc<str>,
}

/// Fonts are the same if they were read under the same name.
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font").field("name", &self.name).finish()
    }
}

fn invalid(name: &str, why: &str) -> Diagnostic {
    Diagnostic::error(
        "font_invalid",
        format!("The font '{name}' cannot be used: {why}."),
        "Use a TrueType (.ttf) or OpenType (.otf) font file; re-export or replace this one. Text that asks for it is drawn with the engine's font.",
    )
    .at_file(name)
}

impl Font {
    /// Reads a font file. `name` names it in diagnostics. A file that is not
    /// a font, is too big, or makes the font reader fail is `font_invalid`.
    pub fn parse(bytes: Vec<u8>, name: &str) -> Result<Self> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(invalid(
                name,
                &format!("it is {} bytes; the most is {MAX_FONT_BYTES}", bytes.len()),
            ));
        }
        // The reader is careful, but the file is not trusted: a panic in it
        // must stay a diagnostic. Measuring a few letters checks the tables
        // text layout uses.
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let font = FontArc::try_from_vec(bytes).ok()?;
            let scaled = font.as_scaled(PxScale::from(32.0));
            let _ = scaled.ascent() - scaled.descent() + scaled.line_gap();
            let mut previous = None;
            for c in "Ag.0 ".chars() {
                let id = font.glyph_id(c);
                let _ = scaled.h_advance(id);
                if let Some(previous) = previous {
                    let _ = scaled.kern(previous, id);
                }
                previous = Some(id);
            }
            Some(font)
        }));
        match parsed {
            Ok(Some(font)) if font.height_unscaled() > 0.0 => Ok(Self {
                inner: font,
                name: Arc::from(name),
            }),
            Ok(Some(_)) => Err(invalid(name, "its letters have no height")),
            Ok(None) => Err(invalid(name, "it is not a font the engine can read")),
            Err(_) => Err(invalid(name, "the font reader failed on it")),
        }
    }

    /// The engine's font (Roboto).
    pub fn default_font() -> Result<&'static Self> {
        static FONT: OnceLock<Option<Font>> = OnceLock::new();
        FONT.get_or_init(|| Self::parse(DEFAULT_FONT.to_vec(), "Roboto").ok())
            .as_ref()
            .ok_or_else(|| {
                Diagnostic::error(
                    "internal_error",
                    "The engine's own font could not be read.",
                    "This is a bug in the engine. Please report it.",
                )
            })
    }

    /// The name it was read under.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// How lines line up in their box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Along the left edge.
    #[default]
    Left,
    /// Centred.
    Center,
    /// Along the right edge.
    Right,
}

/// One glyph placed by [`layout`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// The glyph in the font.
    pub id: u16,
    /// Where its pen position is, from the text box's left edge.
    pub x: f32,
    /// Where its baseline is, from the text box's top edge.
    pub y: f32,
}

/// Text laid out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    /// The width of the longest line.
    pub width: f32,
    /// The height of all the lines.
    pub height: f32,
    /// How many lines.
    pub lines: u32,
    /// The glyphs (spaces left out), line by line.
    pub glyphs: Vec<Glyph>,
}

/// A word and the spaces after it, measured on its own.
struct Word {
    /// The glyphs, the spaces too.
    glyphs: Vec<GlyphId>,
    /// How many of them are the word (the rest are spaces).
    body: usize,
}

fn finite_or(value: f32, otherwise: f32) -> f32 {
    if value.is_finite() { value } else { otherwise }
}

/// Lays `text` out in `font` at `size` pixels in a box `width` wide (or as
/// wide as its longest line): lines break at `\n` and, with `wrap` and a
/// width, before a word that would make a line longer than the box (a word
/// longer than the box stays whole). Lines line up in the box by `align`.
///
/// Deterministic: the result depends only on the arguments.
pub fn layout(
    font: &Font,
    text: &str,
    size: f32,
    width: Option<f32>,
    wrap: bool,
    align: Align,
) -> TextLayout {
    let box_width = width.map(|w| finite_or(w, 0.0).max(0.0));
    let max_width = if wrap { box_width } else { None };
    let size = finite_or(size, 0.0).clamp(0.0, 10_000.0);
    if size <= 0.0 {
        return TextLayout::default();
    }
    let scaled = font.inner.as_scaled(PxScale::from(size));
    let advance = |previous: Option<GlyphId>, id: GlyphId| {
        let kern = previous.map_or(0.0, |p| scaled.kern(p, id));
        finite_or(kern, 0.0) + finite_or(scaled.h_advance(id), 0.0)
    };
    let ascent = finite_or(scaled.ascent(), 0.0);
    let descent = finite_or(scaled.descent(), 0.0);
    let line_height = ascent - descent + finite_or(scaled.line_gap(), 0.0);

    // Lines, each as its glyphs and its width without trailing spaces.
    let mut lines: Vec<(Vec<GlyphId>, f32)> = Vec::new();
    let text: String = text
        .chars()
        .filter(|c| *c != '\r')
        .take(MAX_CHARS)
        .collect();
    for paragraph in text.split('\n') {
        let mut words: Vec<Word> = Vec::new();
        let mut current = Word {
            glyphs: Vec::new(),
            body: 0,
        };
        let mut in_spaces = false;
        for c in paragraph.chars() {
            let space = c == ' ' || c == '\t';
            if !space && in_spaces {
                words.push(std::mem::replace(
                    &mut current,
                    Word {
                        glyphs: Vec::new(),
                        body: 0,
                    },
                ));
                in_spaces = false;
            }
            let id = font.inner.glyph_id(if c == '\t' { ' ' } else { c });
            current.glyphs.push(id);
            if space {
                in_spaces = true;
            } else {
                current.body = current.glyphs.len();
            }
        }
        words.push(current);

        let mut line: Vec<GlyphId> = Vec::new();
        let mut pen = 0.0f32;
        let mut trimmed = 0.0f32;
        for word in words {
            // The line's width if this word joined it, without its spaces.
            let mut x = pen;
            let mut previous = line.last().copied();
            let mut body_end = trimmed;
            for (index, id) in word.glyphs.iter().enumerate() {
                x += advance(previous, *id);
                previous = Some(*id);
                if index + 1 == word.body {
                    body_end = x;
                }
            }
            let breaks =
                max_width.is_some_and(|max| body_end > max) && !line.is_empty() && word.body > 0;
            if breaks {
                lines.push((std::mem::take(&mut line), trimmed));
                // The word starts the new line.
                let mut x = 0.0;
                let mut previous = None;
                let mut body_end = 0.0;
                for (index, id) in word.glyphs.iter().enumerate() {
                    x += advance(previous, *id);
                    previous = Some(*id);
                    if index + 1 == word.body {
                        body_end = x;
                    }
                }
                pen = x;
                trimmed = body_end;
            } else {
                pen = x;
                trimmed = body_end;
            }
            line.extend(word.glyphs);
        }
        lines.push((line, trimmed));
    }

    let width = lines.iter().map(|(_, w)| *w).fold(0.0f32, f32::max);
    let box_width = box_width.unwrap_or(width);
    let mut glyphs = Vec::new();
    let space = font.inner.glyph_id(' ');
    for (index, (line, line_width)) in lines.iter().enumerate() {
        let start = match align {
            Align::Left => 0.0,
            Align::Center => (box_width - line_width) * 0.5,
            Align::Right => box_width - line_width,
        };
        let baseline = ascent + line_height * index as f32;
        let mut x = start;
        let mut previous = None;
        for id in line {
            let kern = previous.map_or(0.0, |p| finite_or(scaled.kern(p, *id), 0.0));
            x += kern;
            if *id != space {
                glyphs.push(Glyph {
                    id: id.0,
                    x,
                    y: baseline,
                });
            }
            x += finite_or(scaled.h_advance(*id), 0.0);
            previous = Some(*id);
        }
    }
    let count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    TextLayout {
        width,
        height: if count == 0 {
            0.0
        } else {
            ascent - descent + line_height * (count - 1) as f32
        },
        lines: count,
        glyphs,
    }
}

/// A glyph's coverage, one byte per pixel (0 to 255), placed relative to its
/// pen position on the baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    /// Its left edge from the pen position, in pixels.
    pub left: i32,
    /// Its top edge from the baseline (negative: above it), in pixels.
    pub top: i32,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Coverage, row by row from the top.
    pub coverage: Vec<u8>,
}

/// The largest glyph drawn, in pixels on a side.
pub const MAX_GLYPH_PX: u32 = 1024;

/// Draws glyph `id` of `font` at `size` pixels; `None` for a glyph with no
/// outline (a space) or one that cannot be drawn.
pub fn rasterize(font: &Font, id: u16, size: f32) -> Option<Bitmap> {
    let size = finite_or(size, 0.0);
    if size <= 0.0 || size > MAX_GLYPH_PX as f32 {
        return None;
    }
    let font = font.inner.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let glyph = GlyphId(id).with_scale(PxScale::from(size));
        let outlined = font.outline_glyph(glyph)?;
        let bounds = outlined.px_bounds();
        let (left, top) = (bounds.min.x, bounds.min.y);
        let width = (bounds.max.x - bounds.min.x).max(0.0) as u32;
        let height = (bounds.max.y - bounds.min.y).max(0.0) as u32;
        if width == 0 || height == 0 || width > MAX_GLYPH_PX * 2 || height > MAX_GLYPH_PX * 2 {
            return None;
        }
        let mut coverage = vec![0u8; (width * height) as usize];
        outlined.draw(|x, y, c| {
            if x < width && y < height {
                coverage[(y * width + x) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some(Bitmap {
            left: left as i32,
            top: top as i32,
            width,
            height,
            coverage,
        })
    }))
    .ok()
    .flatten()
}

/// The project's fonts, by project-relative path: an extension of the world
/// (ADR-026), filled by the control plane when a world is made. Text that
/// names a font that is not here is drawn and measured with the engine's
/// font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fonts {
    by_path: BTreeMap<String, Font>,
}

impl Fonts {
    /// Adds (or replaces) a font.
    pub fn insert(&mut self, path: &str, font: Font) {
        self.by_path.insert(path.to_owned(), font);
    }

    /// Whether the font at `path` is here.
    pub fn contains(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    /// The font at `path`, or the engine's font if `path` is empty or not
    /// here.
    pub fn get(&self, path: &str) -> Result<&Font> {
        match self.by_path.get(path) {
            Some(font) => Ok(font),
            None => Font::default_font(),
        }
    }

    /// The paths of the fonts here.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }
}
