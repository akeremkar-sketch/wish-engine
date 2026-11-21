//! Sprite sheets: `*.sheet.json`, one picture cut into frames, with named
//! clips that play some of the frames in turn.
//!
//! ```json
//! {
//!   "format": 1,
//!   "image": "assets/sprites/crew.png",
//!   "grid": { "frame": [32, 32], "columns": 4, "rows": 2 },
//!   "clips": {
//!     "idle": { "frames": [0, 1], "fps": 2 },
//!     "walk": { "frames": [4, 5, 6, 7], "fps": 8 },
//!     "fall": { "frames": [2, 3], "fps": 6, "loop": false }
//!   }
//! }
//! ```
//!
//! The frames are cut either by a `grid` (frames of one size in columns and
//! rows, numbered left to right and top to bottom from 0, with an optional
//! `margin` before the first and `spacing` between them) or listed one by
//! one as rectangles, `"frames": [[x, y, width, height], ...]`, in the
//! picture's pixels from its top left corner. A clip lists frames by number,
//! with how many it shows a second (`fps`, 10 when left out) and whether it
//! starts over at the end (`loop`, true when left out).
//!
//! An entity shows a sheet through a `SpriteAnimator` beside its `Sprite`;
//! the engine moves the animator's time every tick, so which frame shows is
//! part of the simulation (and of replays).
//!
//! A sheet is both an engine document and an asset, like a material file
//! ([`crate::material`]): strict, canonical JSON with a format number that
//! `wen check`, `wen fmt`, `wen migrate` and `wen watch` know, and an asset
//! that scenes refer to and that `wen mv` moves (rewriting the references
//! to it, and the picture reference inside it). It is untrusted text, read
//! through [`read_sheet_file`] with a size cap; everything wrong with it is a
//! diagnostic with the file, the line and a hint.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_scene::json::{self, Json, Node};

use crate::index::AssetIndex;
use crate::refs::RefUse;

/// The sheet file format this build reads and writes.
pub const SHEET_FORMAT: u32 = 1;

/// What a sprite sheet's file name ends with.
pub const SHEET_SUFFIX: &str = ".sheet.json";

/// The largest sheet file that is read, in bytes. A sheet with every frame
/// listed by hand and every clip at its longest is well below it.
pub const MAX_SHEET_BYTES: u64 = 256 * 1024;

/// The most frames a sheet has.
pub const MAX_FRAMES: usize = 4096;

/// The most clips a sheet has.
pub const MAX_CLIPS: usize = 256;

/// The most frames one clip lists.
pub const MAX_CLIP_FRAMES: usize = 1024;

/// The longest name of a clip, in bytes.
pub const MAX_CLIP_NAME: usize = 64;

/// The largest frame side, margin or spacing, in pixels (the largest
/// picture the engine reads is 4096 pixels a side).
pub const MAX_SIDE: u64 = 4096;

/// The most frames a second a clip shows.
pub const MAX_FPS: f64 = 240.0;

/// The frames a second of a clip that does not say.
pub const DEFAULT_FPS: f32 = 10.0;

/// The top-level keys of a sheet file.
const KEYS: [&str; 5] = ["clips", "format", "frames", "grid", "image"];
/// The keys of a grid.
const GRID_KEYS: [&str; 5] = ["columns", "frame", "margin", "rows", "spacing"];
/// The keys of a clip.
const CLIP_KEYS: [&str; 3] = ["fps", "frames", "loop"];

/// Whether a file name is a sprite sheet's.
pub fn is_sheet(name: &str) -> bool {
    name.ends_with(SHEET_SUFFIX)
}

/// The diagnostic of a sheet file of `bytes` bytes, more than
/// [`MAX_SHEET_BYTES`]; `name` is its project-relative path.
pub fn sheet_too_large(name: &str, bytes: u64) -> Diagnostic {
    Diagnostic::error(
        "sheet_file_too_large",
        format!("'{name}' is {bytes} bytes; a sprite sheet file is at most {MAX_SHEET_BYTES} bytes."),
        "A sheet holds a picture's path, how it is cut and its clips. Use a grid instead of listing many frames by hand, or split the clips over several sheets of the same picture.",
    )
    .at_file(name)
}

/// Reads the sheet file at `file` (the project path `name`, for the
/// diagnostics) as text, refusing one larger than [`MAX_SHEET_BYTES`]
/// without reading it (`sheet_file_too_large`): every place that reads a
/// sheet file whole goes through this.
pub fn read_sheet_file(file: &std::path::Path, name: &str) -> Result<String> {
    use std::io::Read;
    let unreadable = |error: std::io::Error| {
        Diagnostic::error(
            "file_unreadable",
            format!("Could not read '{name}': {error}."),
            "Check that the file can be read and is UTF-8 text.",
        )
        .at_file(name)
    };
    let opened = std::fs::File::open(file).map_err(unreadable)?;
    let bytes = opened.metadata().map_err(unreadable)?.len();
    if bytes > MAX_SHEET_BYTES {
        return Err(sheet_too_large(name, bytes));
    }
    // The file may grow between the length and the read: never more than
    // the cap (and one byte to tell) is read.
    let mut text = String::new();
    opened
        .take(MAX_SHEET_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    let read = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if read > MAX_SHEET_BYTES {
        return Err(sheet_too_large(name, read));
    }
    Ok(text)
}

/// A clip of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetClip {
    /// Its name.
    pub name: String,
    /// The sheet's frames it shows, by number, in order.
    pub frames: Vec<u32>,
    /// How many frames it shows a second.
    pub fps: f32,
    /// Whether it starts over at the end.
    pub looping: bool,
}

/// What a sheet file means: its picture, every frame's rectangle and its
/// clips.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// The picture's project path.
    pub image: String,
    /// Each frame: left, top, width, height in the picture's pixels.
    pub frames: Vec<[u32; 4]>,
    /// The clips, by name.
    pub clips: Vec<SheetClip>,
}

impl Sheet {
    /// The clip called `name`.
    pub fn clip(&self, name: &str) -> Option<&SheetClip> {
        self.clips.iter().find(|clip| clip.name == name)
    }

    /// The names of its clips, in name order.
    pub fn clip_names(&self) -> Vec<&str> {
        self.clips.iter().map(|clip| clip.name.as_str()).collect()
    }
}

/// A sheet file as a document: what it writes, everything but its format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetDocument {
    /// The keys the file writes (`image`, `grid` or `frames`, `clips`).
    pub sheet: Map<String, Value>,
}

fn at(node: &Node, file: &str, diagnostic: Diagnostic) -> Diagnostic {
    let diagnostic = diagnostic.at_line(node.span.line, Some(node.span.column));
    if file.is_empty() {
        diagnostic
    } else {
        diagnostic.at_file(file)
    }
}

/// Puts the line and column of the value a diagnostic's pointer names on
/// it, and the file.
fn locate(root: &Node, file: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    if let Some(located) = diagnostic
        .location
        .path
        .as_deref()
        .and_then(|pointer| root.locate(pointer))
    {
        let span = located.key.unwrap_or(located.value);
        diagnostic = diagnostic.at_line(span.line, Some(span.column));
    }
    if file.is_empty() {
        diagnostic
    } else {
        diagnostic.at_file(file)
    }
}

/// Reads a sheet file's text and checks what it says: its keys, the
/// picture reference's form, the frames and the clips. `file` is the
/// project-relative path shown in diagnostics (or `""`). The first problem
/// found is returned.
///
/// Whether the picture exists is a separate question ([`check_sheet`]):
/// this needs no project.
pub fn parse_sheet(source: &str, file: &str) -> Result<SheetDocument> {
    let root = json::parse(source, file)?;
    let Json::Object(members) = &root.value else {
        return Err(at(
            &root,
            file,
            Diagnostic::error(
                "type_mismatch",
                "A sprite sheet file must be an object.",
                "Write { \"format\": 1, \"image\": \"assets/sprites/hero.png\", \"grid\": { \"frame\": [32, 32], \"columns\": 4, \"rows\": 1 } }.",
            ),
        ));
    };
    let mut format_seen = false;
    let mut sheet = Map::new();
    for member in members {
        if member.key == "format" {
            format_seen = true;
            let supported = matches!(&member.value.value, Json::Number(n) if n.as_u64() == Some(u64::from(SHEET_FORMAT)));
            if !supported {
                return Err(at(
                    &member.value,
                    file,
                    Diagnostic::error(
                        "unsupported_format",
                        format!("This sprite sheet file is not format {SHEET_FORMAT}."),
                        "Run `wen migrate` to upgrade an older file, or update the engine for a newer one.",
                    ),
                ));
            }
        } else {
            sheet.insert(member.key.clone(), member.value.to_value());
        }
    }
    if !format_seen {
        return Err(at(
            &root,
            file,
            Diagnostic::error(
                "missing_format",
                "The sprite sheet file has no \"format\" key.",
                format!("Add \"format\": {SHEET_FORMAT} at the top level."),
            ),
        ));
    }
    let parsed = SheetDocument { sheet };
    parsed
        .decode()
        .map_err(|problem| locate(&root, file, problem))?;
    Ok(parsed)
}

/// An error about the value at `pointer`.
fn invalid(pointer: &str, message: impl Into<String>, hint: impl Into<String>) -> Diagnostic {
    Diagnostic::error("sheet_invalid", message, hint).at_path(pointer.to_owned())
}

/// A JSON number that is whole and not negative (`32`, or `32.0` as a
/// number box writes it).
fn whole_number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && (0.0..=1.0e12).contains(n))
            .map(|n| n as u64)
    })
}

/// A whole number from `low` to `high` at `pointer`.
fn whole(value: &Value, pointer: &str, low: u64, high: u64) -> Result<u32> {
    let Some(number) = whole_number(value) else {
        return Err(Diagnostic::error(
            "type_mismatch",
            format!("{pointer} must be a whole number of pixels or frames, not {value}."),
            format!("Write a whole number from {low} to {high}, without a decimal point."),
        )
        .at_path(pointer.to_owned()));
    };
    if !(low..=high).contains(&number) {
        return Err(Diagnostic::error(
            "value_out_of_range",
            format!("{pointer} is {number}; it must be from {low} to {high}."),
            format!("Use a whole number from {low} to {high}."),
        )
        .at_path(pointer.to_owned()));
    }
    u32::try_from(number)
        .map_err(|_| invalid(pointer, "The number is too large.", "Use a smaller number."))
}

/// A pair of whole numbers `[a, b]` at `pointer`.
fn pair(value: &Value, pointer: &str, low: u64, high: u64) -> Result<[u32; 2]> {
    let items = value
        .as_array()
        .filter(|items| items.len() == 2)
        .ok_or_else(|| {
            Diagnostic::error(
                "wrong_length",
                format!("{pointer} must be two whole numbers [x, y], not {value}."),
                "Write it like [32, 32]: across, then down, in pixels.",
            )
            .at_path(pointer.to_owned())
        })?;
    Ok([
        whole(&items[0], &format!("{pointer}/0"), low, high)?,
        whole(&items[1], &format!("{pointer}/1"), low, high)?,
    ])
}

/// Checks that an object has only `known` keys.
fn known_keys(map: &Map<String, Value>, known: &[&str], pointer: &str, what: &str) -> Result<()> {
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            return Err(Diagnostic::error(
                "unknown_key",
                format!("{what} has no key '{key}'."),
                format!(
                    "{} Its keys are: {}.",
                    unknown_name_hint(key, known),
                    known.join(", ")
                ),
            )
            .at_path(format!("{pointer}/{key}"))
            .with_candidates(known.iter().copied()));
        }
    }
    Ok(())
}

impl SheetDocument {
    /// A sheet from the keys a request gives (`asset_create`'s `sheet`):
    /// checked like a file. Diagnostics point into `/sheet`.
    pub fn from_spec(spec: Map<String, Value>) -> Result<Self> {
        if spec.contains_key("format") {
            return Err(invalid(
                "/sheet/format",
                "The format is not part of the sheet's description.",
                "Leave \"format\" out: the engine writes it.",
            ));
        }
        let document = Self { sheet: spec };
        document.decode().map_err(|mut problem| {
            let pointer = problem.location.path.clone().unwrap_or_default();
            problem.location.path = Some(format!("/sheet{pointer}"));
            problem
        })?;
        Ok(document)
    }

    /// What the sheet means. Fails with the first thing that is wrong; the
    /// diagnostic's pointer is from the file's top (`/clips/walk/fps`), with
    /// no file or line.
    pub fn decode(&self) -> Result<Sheet> {
        known_keys(&self.sheet, &KEYS, "", "A sprite sheet")?;
        let image = match self.sheet.get("image") {
            Some(Value::String(path)) => {
                if let Err(problem) = wish_core::asset::validate_asset_ref(path) {
                    return Err(Diagnostic::error(
                        "invalid_asset_ref",
                        format!("'{path}' is not a valid asset reference: {problem}."),
                        "Write a path relative to the project folder with '/' between folders, such as \"assets/sprites/hero.png\".",
                    )
                    .at_path("/image"));
                }
                if path.is_empty() {
                    return Err(invalid(
                        "/image",
                        "The sheet names no picture.",
                        "Name the PNG the frames are cut from: \"image\": \"assets/sprites/hero.png\".",
                    ));
                }
                path.clone()
            }
            Some(other) => {
                return Err(Diagnostic::error(
                    "type_mismatch",
                    format!("\"image\" must be a path in quotes, not {other}."),
                    "Write it like \"image\": \"assets/sprites/hero.png\".",
                )
                .at_path("/image"));
            }
            None => {
                return Err(invalid(
                    "",
                    "The sheet names no picture (\"image\").",
                    "Add \"image\": \"assets/sprites/hero.png\": the PNG the frames are cut from.",
                ));
            }
        };
        let frames = match (self.sheet.get("grid"), self.sheet.get("frames")) {
            (Some(grid), None) => grid_frames(grid)?,
            (None, Some(list)) => listed_frames(list)?,
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "/frames",
                    "The sheet has both a \"grid\" and a list of \"frames\".",
                    "Cut the picture one way: a grid for frames of one size in rows and columns, or a list of [x, y, width, height] rectangles.",
                ));
            }
            (None, None) => {
                return Err(invalid(
                    "",
                    "The sheet does not say how the picture is cut into frames.",
                    "Add \"grid\": { \"frame\": [32, 32], \"columns\": 4, \"rows\": 1 }, or \"frames\": [[0, 0, 32, 32], ...].",
                ));
            }
        };
        let clips = match self.sheet.get("clips") {
            None => Vec::new(),
            Some(Value::Object(clips)) => {
                if clips.len() > MAX_CLIPS {
                    return Err(invalid(
                        "/clips",
                        format!("The sheet has {} clips; at most {MAX_CLIPS}.", clips.len()),
                        "Split the clips over several sheets of the same picture.",
                    ));
                }
                let mut found = Vec::with_capacity(clips.len());
                // serde_json's map keeps keys in order: clips come out by name.
                for (name, clip) in clips {
                    found.push(decode_clip(name, clip, frames.len())?);
                }
                found
            }
            Some(other) => {
                return Err(Diagnostic::error(
                    "type_mismatch",
                    format!("\"clips\" must be an object of clips by name, not {other}."),
                    "Write it like \"clips\": { \"walk\": { \"frames\": [0, 1, 2, 3], \"fps\": 8 } }.",
                )
                .at_path("/clips"));
            }
        };
        Ok(Sheet {
            image,
            frames,
            clips,
        })
    }

    /// The file as JSON, in the form the file holds.
    pub fn to_value(&self) -> Value {
        let mut object = self.sheet.clone();
        object.insert("format".to_owned(), Value::from(SHEET_FORMAT));
        Value::Object(object)
    }

    /// The sheet file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        json::to_canonical(&self.to_value())
    }

    /// The picture the sheet names, with its JSON pointer.
    pub fn image(&self) -> Option<RefUse> {
        crate::refs::refs_in_sheet(&self.to_value())
            .into_iter()
            .next()
    }
}

fn grid_frames(grid: &Value) -> Result<Vec<[u32; 4]>> {
    let Value::Object(grid) = grid else {
        return Err(Diagnostic::error(
            "type_mismatch",
            format!("\"grid\" must be an object, not {grid}."),
            "Write it like \"grid\": { \"frame\": [32, 32], \"columns\": 4, \"rows\": 2 }.",
        )
        .at_path("/grid"));
    };
    known_keys(grid, &GRID_KEYS, "/grid", "A grid")?;
    let frame = match grid.get("frame") {
        Some(value) => pair(value, "/grid/frame", 1, MAX_SIDE)?,
        None => {
            return Err(invalid(
                "/grid",
                "The grid does not say how large a frame is (\"frame\").",
                "Add \"frame\": [width, height] in pixels, such as [32, 32].",
            ));
        }
    };
    let count = |key: &str| -> Result<u32> {
        match grid.get(key) {
            Some(value) => whole(value, &format!("/grid/{key}"), 1, MAX_FRAMES as u64),
            None => Err(invalid(
                "/grid",
                format!("The grid does not say how many {key} of frames it has (\"{key}\")."),
                "Give \"columns\" (frames across) and \"rows\" (frames down).",
            )),
        }
    };
    let (columns, rows) = (count("columns")?, count("rows")?);
    let margin = match grid.get("margin") {
        Some(value) => pair(value, "/grid/margin", 0, MAX_SIDE)?,
        None => [0, 0],
    };
    let spacing = match grid.get("spacing") {
        Some(value) => pair(value, "/grid/spacing", 0, MAX_SIDE)?,
        None => [0, 0],
    };
    if u64::from(columns) * u64::from(rows) > MAX_FRAMES as u64 {
        return Err(invalid(
            "/grid",
            format!("The grid has {columns} x {rows} frames; a sheet has at most {MAX_FRAMES}."),
            "Use fewer, larger frames, or several sheets.",
        ));
    }
    let mut frames = Vec::with_capacity((columns * rows) as usize);
    for row in 0..rows {
        for column in 0..columns {
            // At most 4096 frames of 4096 px with 4096 px gaps: fits u32.
            frames.push([
                margin[0] + column * (frame[0] + spacing[0]),
                margin[1] + row * (frame[1] + spacing[1]),
                frame[0],
                frame[1],
            ]);
        }
    }
    Ok(frames)
}

fn listed_frames(list: &Value) -> Result<Vec<[u32; 4]>> {
    let Value::Array(items) = list else {
        return Err(Diagnostic::error(
            "type_mismatch",
            format!("\"frames\" must be a list of [x, y, width, height] rectangles, not {list}."),
            "Write it like \"frames\": [[0, 0, 32, 32], [32, 0, 32, 32]].",
        )
        .at_path("/frames"));
    };
    if items.is_empty() || items.len() > MAX_FRAMES {
        return Err(invalid(
            "/frames",
            format!(
                "The sheet lists {} frames; a sheet has 1 to {MAX_FRAMES}.",
                items.len()
            ),
            "List each frame as [x, y, width, height] in the picture's pixels from its top left corner.",
        ));
    }
    let mut frames = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let pointer = format!("/frames/{index}");
        let parts = item.as_array().filter(|p| p.len() == 4).ok_or_else(|| {
            Diagnostic::error(
                "wrong_length",
                format!("{pointer} must be [x, y, width, height], not {item}."),
                "Four whole numbers: the frame's left and top in the picture's pixels, then its width and height.",
            )
            .at_path(pointer.clone())
        })?;
        frames.push([
            whole(&parts[0], &format!("{pointer}/0"), 0, MAX_SIDE * 4)?,
            whole(&parts[1], &format!("{pointer}/1"), 0, MAX_SIDE * 4)?,
            whole(&parts[2], &format!("{pointer}/2"), 1, MAX_SIDE)?,
            whole(&parts[3], &format!("{pointer}/3"), 1, MAX_SIDE)?,
        ]);
    }
    Ok(frames)
}

fn decode_clip(name: &str, clip: &Value, frame_count: usize) -> Result<SheetClip> {
    let pointer = format!("/clips/{}", name.replace('~', "~0").replace('/', "~1"));
    if name.is_empty() || name.len() > MAX_CLIP_NAME || name.chars().any(char::is_control) {
        return Err(invalid(
            &pointer,
            format!("'{name}' cannot be a clip's name."),
            format!("A clip's name is 1 to {MAX_CLIP_NAME} bytes of text, such as \"walk\"."),
        ));
    }
    let Value::Object(clip) = clip else {
        return Err(Diagnostic::error(
            "type_mismatch",
            format!("The clip '{name}' must be an object, not {clip}."),
            "Write it like { \"frames\": [0, 1, 2, 3], \"fps\": 8 }.",
        )
        .at_path(pointer));
    };
    known_keys(clip, &CLIP_KEYS, &pointer, "A clip")?;
    let Some(Value::Array(listed)) = clip.get("frames") else {
        return Err(invalid(
            &format!("{pointer}/frames"),
            format!("The clip '{name}' does not list its frames."),
            "Add \"frames\": [0, 1, 2, 3]: the sheet's frames by number, in the order they show.",
        ));
    };
    if listed.is_empty() || listed.len() > MAX_CLIP_FRAMES {
        return Err(invalid(
            &format!("{pointer}/frames"),
            format!(
                "The clip '{name}' lists {} frames; a clip has 1 to {MAX_CLIP_FRAMES}.",
                listed.len()
            ),
            "List at least one frame; a frame may be listed more than once to hold it longer.",
        ));
    }
    let last = frame_count.saturating_sub(1) as u64;
    let mut frames = Vec::with_capacity(listed.len());
    for (index, frame) in listed.iter().enumerate() {
        let at = format!("{pointer}/frames/{index}");
        let number = whole_number(frame).ok_or_else(|| {
            Diagnostic::error(
                "type_mismatch",
                format!("{at} must be a frame's number, not {frame}."),
                "Frames are numbered from 0, left to right and top to bottom in a grid, or in the order of the frames list.",
            )
            .at_path(at.clone())
        })?;
        if number > last {
            return Err(Diagnostic::error(
                "value_out_of_range",
                format!(
                    "The clip '{name}' shows frame {number}; the sheet has {frame_count} frames (0 to {last})."
                ),
                "Frames are numbered from 0, left to right and top to bottom in a grid. Use a number of a frame the sheet has, or make the grid larger.",
            )
            .at_path(at));
        }
        frames.push(u32::try_from(number).unwrap_or(0));
    }
    let fps = match clip.get("fps") {
        None => DEFAULT_FPS,
        Some(value) => match value.as_f64() {
            Some(fps) if fps.is_finite() && fps > 0.0 && fps <= MAX_FPS => fps as f32,
            _ => {
                return Err(Diagnostic::error(
                    "value_out_of_range",
                    format!("The clip '{name}' shows {value} frames a second; it must be more than 0 and at most {MAX_FPS}."),
                    "Most walk cycles are 8 to 12 frames a second.",
                )
                .at_path(format!("{pointer}/fps")));
            }
        },
    };
    let looping = match clip.get("loop") {
        None => true,
        Some(Value::Bool(looping)) => *looping,
        Some(other) => {
            return Err(Diagnostic::error(
                "type_mismatch",
                format!("\"loop\" must be true or false, not {other}."),
                "true starts the clip over at its end; false stops on its last frame.",
            )
            .at_path(format!("{pointer}/loop")));
        }
    };
    Ok(SheetClip {
        name: name.to_owned(),
        frames,
        fps,
        looping,
    })
}

/// The size of a PNG picture from its header, if `bytes` start like one.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    Some((word(16)?, word(20)?))
}

/// Checks a sheet file's text as part of a project: everything
/// [`parse_sheet`] checks, that its picture exists in `index` and is a PNG,
/// that the frames lie inside the picture (`sheet_frames_outside_picture`,
/// a warning: the part outside shows nothing), and that the text is
/// canonical (a warning). `root` is the project folder, for the picture's
/// size (only its header is read).
///
/// Returns the file, when it has no errors, and every problem found.
pub fn check_sheet(
    source: &str,
    file: &str,
    index: &AssetIndex,
    root: Option<&std::path::Path>,
) -> (Option<SheetDocument>, Vec<Diagnostic>) {
    let parsed = match parse_sheet(source, file) {
        Ok(parsed) => parsed,
        Err(problem) => return (None, vec![problem]),
    };
    let Ok(node) = json::parse(source, file) else {
        return (None, Vec::new());
    };
    let mut diagnostics = Vec::new();
    if let Some(used) = parsed.image() {
        let problem = index.check(&used.reference).err().or_else(|| {
            (!used.reference.to_ascii_lowercase().ends_with(".png")).then(|| {
                Diagnostic::error(
                    "texture_not_png",
                    format!("'{}' is not a PNG file: a sprite sheet's picture is read from PNG files only.", used.reference),
                    "Convert the picture to PNG (8-bit) and name that file.",
                )
            })
        });
        match problem {
            Some(problem) => {
                diagnostics.push(locate(&node, file, problem.at_path(used.pointer.clone())));
            }
            None => {
                if let (Some(root), Ok(sheet)) = (root, parsed.decode()) {
                    let grid = parsed.sheet.contains_key("grid");
                    diagnostics.extend(
                        frames_outside(root, &sheet, grid)
                            .map(|problem| locate(&node, file, problem)),
                    );
                }
            }
        }
    }
    if parsed.to_canonical().ok().as_deref() != Some(source) {
        let warning = Diagnostic::warning(
            "not_canonical",
            "This file is not in canonical form.",
            "Run `wen fmt` to rewrite it. Saving through the engine does this automatically.",
        )
        .at_line(1, Some(1));
        diagnostics.push(if file.is_empty() {
            warning
        } else {
            warning.at_file(file)
        });
    }
    let failed = diagnostics.iter().any(Diagnostic::is_error);
    ((!failed).then_some(parsed), diagnostics)
}

/// A warning when some frame of `sheet` reaches past its picture (whose
/// header is read from the project at `root`); `grid` says whether the
/// frames were cut by a grid (the warning points at it).
fn frames_outside(root: &std::path::Path, sheet: &Sheet, grid: bool) -> Option<Diagnostic> {
    use std::io::Read;
    let mut header = [0u8; 24];
    let mut opened = std::fs::File::open(root.join(&sheet.image)).ok()?;
    opened.read_exact(&mut header).ok()?;
    let (width, height) = png_size(&header)?;
    let outside: BTreeSet<usize> = sheet
        .frames
        .iter()
        .enumerate()
        .filter(|(_, [x, y, w, h])| {
            u64::from(*x) + u64::from(*w) > u64::from(width)
                || u64::from(*y) + u64::from(*h) > u64::from(height)
        })
        .map(|(index, _)| index)
        .collect();
    let first = *outside.first()?;
    let pointer = if grid {
        "/grid".to_owned()
    } else {
        format!("/frames/{first}")
    };
    Some(
        Diagnostic::warning(
            "sheet_frames_outside_picture",
            format!(
                "{} of the sheet's {} frames reach past its {width}x{height} picture (the first is frame {first}); the part outside shows nothing.",
                outside.len(),
                sheet.frames.len()
            ),
            "Check the frame size, the columns and rows (or the rectangles) against the picture: frames are cut from its top left corner.",
        )
        .at_path(pointer),
    )
}

/// What kind of file a sheet reference must name, by where it is written:
/// `SpriteAnimator.sheet` in a scene or prefab a sheet file. `None` when
/// the reference fits, or is not one of an animator's.
pub(crate) fn kind_problem(pointer: &str, reference: &str) -> Option<Diagnostic> {
    let (parent, field) = pointer.rsplit_once('/')?;
    if reference.is_empty() || field != "sheet" || !parent.ends_with("/components/SpriteAnimator") {
        return None;
    }
    let path = crate::refs::path_and_suffix(reference).0;
    (!is_sheet(path)).then(|| {
        Diagnostic::error(
            "not_a_sheet_file",
            format!(
                "'{path}' is not a sprite sheet: SpriteAnimator.sheet needs a file whose name ends in {SHEET_SUFFIX}."
            ),
            "Name a sheet such as \"assets/sprites/hero.sheet.json\" (asset_create makes one from a picture). The picture itself goes into the sheet's \"image\".",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = r#"{
  "clips": {
    "idle": { "fps": 2, "frames": [0, 1] },
    "walk": { "fps": 8, "frames": [4, 5, 6, 7] }
  },
  "format": 1,
  "grid": { "columns": 4, "frame": [32, 16], "rows": 2, "spacing": [2, 1] },
  "image": "sprites/hero.png"
}"#;

    #[test]
    fn a_grid_is_cut_left_to_right_then_down() {
        let sheet = parse_sheet(GRID, "a.sheet.json").unwrap().decode().unwrap();
        assert_eq!(sheet.image, "sprites/hero.png");
        assert_eq!(sheet.frames.len(), 8);
        assert_eq!(sheet.frames[1], [34, 0, 32, 16]);
        assert_eq!(sheet.frames[4], [0, 17, 32, 16]);
        assert_eq!(sheet.clip_names(), ["idle", "walk"]);
        let walk = sheet.clip("walk").unwrap();
        assert_eq!(
            (walk.frames.clone(), walk.fps, walk.looping),
            (vec![4, 5, 6, 7], 8.0, true)
        );
    }

    #[test]
    fn mistakes_are_located_with_a_hint() {
        let bad = GRID.replace("[4, 5, 6, 7]", "[4, 5, 6, 9]");
        let problem = parse_sheet(&bad, "a.sheet.json").unwrap_err();
        assert_eq!(problem.code, "value_out_of_range");
        assert_eq!(
            problem.location.path.as_deref(),
            Some("/clips/walk/frames/3")
        );
        assert_eq!(problem.location.line, Some(4));
        assert!(!problem.hint.is_empty());
        let both = GRID.replace("\"image\"", "\"frames\": [[0, 0, 1, 1]], \"image\"");
        assert_eq!(parse_sheet(&both, "a").unwrap_err().code, "sheet_invalid");
        let unknown = GRID.replace("\"fps\": 2", "\"speed\": 2");
        let problem = parse_sheet(&unknown, "a").unwrap_err();
        assert_eq!(problem.code, "unknown_key");
        assert_eq!(problem.location.path.as_deref(), Some("/clips/idle/speed"));
        let no_format = GRID.replace("\"format\": 1,", "");
        assert_eq!(
            parse_sheet(&no_format, "a").unwrap_err().code,
            "missing_format"
        );
        let outside = GRID.replace("sprites/hero.png", "../hero.png");
        assert_eq!(
            parse_sheet(&outside, "a").unwrap_err().code,
            "invalid_asset_ref"
        );
    }

    #[test]
    fn the_canonical_text_round_trips() {
        let parsed = parse_sheet(GRID, "a").unwrap();
        let canonical = parsed.to_canonical().unwrap();
        assert_eq!(parse_sheet(&canonical, "a").unwrap(), parsed);
        assert_eq!(
            parse_sheet(&canonical, "a")
                .unwrap()
                .to_canonical()
                .unwrap(),
            canonical
        );
    }

    #[test]
    fn listed_frames_and_defaults() {
        let source = r#"{ "format": 1, "image": "a.png", "frames": [[0, 0, 8, 8], [8, 0, 8, 8]],
          "clips": { "blink": { "frames": [1, 0, 1], "loop": false } } }"#;
        let sheet = parse_sheet(source, "a").unwrap().decode().unwrap();
        assert_eq!(sheet.frames, [[0, 0, 8, 8], [8, 0, 8, 8]]);
        let blink = sheet.clip("blink").unwrap();
        assert_eq!((blink.fps, blink.looping), (DEFAULT_FPS, false));
    }

    #[test]
    fn a_spec_is_checked_with_pointers_into_it() {
        let Value::Object(mut spec) = serde_json::from_str::<Value>(GRID).unwrap() else {
            unreachable!()
        };
        spec.remove("format");
        assert!(SheetDocument::from_spec(spec.clone()).is_ok());
        spec.insert(
            "grid".to_owned(),
            serde_json::json!({ "frame": [0, 4], "columns": 1, "rows": 1 }),
        );
        let problem = SheetDocument::from_spec(spec).unwrap_err();
        assert_eq!(
            problem.location.path.as_deref(),
            Some("/sheet/grid/frame/0")
        );
    }

    #[test]
    fn a_png_header_gives_the_size() {
        let mut header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&256u32.to_be_bytes());
        header.extend_from_slice(&64u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((256, 64)));
        assert_eq!(png_size(b"GIF89a"), None);
    }

    #[test]
    fn only_an_animator_s_sheet_has_a_kind() {
        assert!(
            kind_problem(
                "/entities/a/components/SpriteAnimator/sheet",
                "a.sheet.json"
            )
            .is_none()
        );
        assert_eq!(
            kind_problem("/entities/a/components/SpriteAnimator/sheet", "a.png")
                .unwrap()
                .code,
            "not_a_sheet_file"
        );
        assert!(kind_problem("/entities/a/components/Sprite/image", "a.png").is_none());
    }
}
