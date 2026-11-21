//! Tilesets (`*.tileset.json`) and tilemaps (`*.tilemap.json`): a picture
//! cut into tiles with properties per tile, and layers of tiles in chunks
//! (design note `docs/design/1.1-tilemaps.md`).
//!
//! ```json
//! {
//!   "format": 1,
//!   "grid": { "columns": 8, "frame": [16, 16], "rows": 4 },
//!   "image": "assets/tiles/station.png",
//!   "tiles": { "8": { "name": "wall", "solid": true } }
//! }
//! ```
//!
//! A tileset cuts its tiles from `image` by a `grid` (the keys of a sprite
//! sheet's grid), or takes them from a sprite sheet (`sheet`). `tiles`
//! gives properties by tile number: `solid`, a collision `shape` (a convex
//! outline in tile units), a `name`.
//!
//! ```json
//! {
//!   "format": 1,
//!   "layers": [
//!     { "chunks": { "0,0": [[-1, 8, ...16 numbers], ...16 rows, top first] },
//!       "name": "walls" }
//!   ],
//!   "tileset": "assets/tiles/station.tileset.json"
//! }
//! ```
//!
//! A tilemap's cells are whole coordinates, x right and y up; a chunk is
//! 16 × 16 cells under the key `"cx,cy"`, written as 16 rows of 16 numbers,
//! the top row first, so the text reads like the picture. `-1` is an empty
//! cell. Layers are drawn in order; a layer's solid tiles are walls unless
//! it says `"collide": false`.
//!
//! Both are documents and assets, like material files and sheets: strict,
//! canonical JSON with a format number, read with a size cap, every mistake
//! a diagnostic with the file, the JSON pointer, the line and a hint.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value};
use wish_core::{Diagnostic, Result};
use wish_scene::json::{self, Json};

use crate::index::AssetIndex;
use crate::refs::RefUse;
use crate::sheet::{at, known_keys, locate, whole_number};

/// The tileset file format this build reads and writes.
pub const TILESET_FORMAT: u32 = 1;
/// The tilemap file format this build reads and writes.
pub const TILEMAP_FORMAT: u32 = 1;
/// What a tileset's file name ends with.
pub const TILESET_SUFFIX: &str = ".tileset.json";
/// What a tilemap's file name ends with.
pub const TILEMAP_SUFFIX: &str = ".tilemap.json";
/// The largest tileset file that is read, in bytes.
pub const MAX_TILESET_BYTES: u64 = 256 * 1024;
/// The largest tilemap file that is read, in bytes: a map of 4096 full
/// chunks (a million cells) fits.
pub const MAX_TILEMAP_BYTES: u64 = 4 * 1024 * 1024;
/// The side of a chunk, in cells.
pub const CHUNK: i32 = 16;
/// The cells of a chunk.
pub const CHUNK_CELLS: usize = (CHUNK * CHUNK) as usize;
/// The most layers a tilemap has.
pub const MAX_LAYERS: usize = 16;
/// The most chunks a tilemap has, over all its layers.
pub const MAX_CHUNKS: usize = 4096;
/// Chunk coordinates are from `-MAX_CHUNK_COORD` to `MAX_CHUNK_COORD - 1`.
pub const MAX_CHUNK_COORD: i32 = 4096;
/// The lowest cell coordinate.
pub const MIN_CELL: i32 = -MAX_CHUNK_COORD * CHUNK;
/// The highest cell coordinate.
pub const MAX_CELL: i32 = MAX_CHUNK_COORD * CHUNK - 1;
/// The highest tile number a tilemap may hold.
pub const MAX_TILE: i32 = 65_535;
/// The most tiles a tileset has.
pub const MAX_TILES: usize = 4096;
/// The most points of a tile's collision outline.
pub const MAX_SHAPE_POINTS: usize = 16;
/// The longest layer or tile name, in bytes.
pub const MAX_NAME: usize = 64;
/// An empty cell.
pub const EMPTY: i32 = -1;

const TILESET_KEYS: [&str; 4] = ["grid", "image", "sheet", "tiles"];
const TILE_KEYS: [&str; 3] = ["name", "shape", "solid"];
const TILEMAP_KEYS: [&str; 2] = ["layers", "tileset"];
const LAYER_KEYS: [&str; 3] = ["chunks", "collide", "name"];

/// Whether a file name is a tileset's.
pub fn is_tileset(name: &str) -> bool {
    name.ends_with(TILESET_SUFFIX)
}

/// Whether a file name is a tilemap's.
pub fn is_tilemap(name: &str) -> bool {
    name.ends_with(TILEMAP_SUFFIX)
}

/// The diagnostic of a tileset file of `bytes` bytes, more than
/// [`MAX_TILESET_BYTES`].
pub fn tileset_too_large(name: &str, bytes: u64) -> Diagnostic {
    Diagnostic::error(
        "tileset_file_too_large",
        format!("'{name}' is {bytes} bytes; a tileset file is at most {MAX_TILESET_BYTES} bytes."),
        "A tileset holds a picture's path, how it is cut and the properties of its tiles. Give properties only to tiles that have some, or split the tiles over two tilesets.",
    )
    .at_file(name)
}

/// The diagnostic of a tilemap file of `bytes` bytes, more than
/// [`MAX_TILEMAP_BYTES`].
pub fn tilemap_too_large(name: &str, bytes: u64) -> Diagnostic {
    Diagnostic::error(
        "tilemap_file_too_large",
        format!("'{name}' is {bytes} bytes; a tilemap file is at most {MAX_TILEMAP_BYTES} bytes."),
        "Split the map into several tilemaps (each its own entity with a Tilemap), side by side.",
    )
    .at_file(name)
}

/// Reads a text file of at most `cap` bytes without reading more than that.
fn read_capped(
    file: &Path,
    name: &str,
    cap: u64,
    too_large: fn(&str, u64) -> Diagnostic,
) -> Result<String> {
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
    if bytes > cap {
        return Err(too_large(name, bytes));
    }
    let mut text = String::new();
    opened
        .take(cap + 1)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    let read = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if read > cap {
        return Err(too_large(name, read));
    }
    Ok(text)
}

/// Reads the tileset file at `file` (the project path `name`) as text,
/// refusing one larger than [`MAX_TILESET_BYTES`] without reading it.
pub fn read_tileset_file(file: &Path, name: &str) -> Result<String> {
    read_capped(file, name, MAX_TILESET_BYTES, tileset_too_large)
}

/// Reads the tilemap file at `file` (the project path `name`) as text,
/// refusing one larger than [`MAX_TILEMAP_BYTES`] without reading it.
pub fn read_tilemap_file(file: &Path, name: &str) -> Result<String> {
    read_capped(file, name, MAX_TILEMAP_BYTES, tilemap_too_large)
}

/// An error about the value at `pointer` of a file of kind `code`
/// (`tileset_invalid`, `tilemap_invalid`).
fn invalid(
    code: &'static str,
    pointer: &str,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(code, message, hint).at_path(pointer.to_owned())
}

/// A type mistake at `pointer`.
fn mismatch(pointer: &str, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("type_mismatch", message, hint).at_path(pointer.to_owned())
}

/// The `format` of a parsed file, checked, and its other members.
fn members_of(
    root: &wish_scene::json::Node,
    file: &str,
    format: u32,
    what: &str,
    example: &str,
) -> Result<Map<String, Value>> {
    let Json::Object(members) = &root.value else {
        return Err(at(
            root,
            file,
            Diagnostic::error(
                "type_mismatch",
                format!("A {what} file must be an object."),
                example.to_owned(),
            ),
        ));
    };
    let mut format_seen = false;
    let mut out = Map::new();
    for member in members {
        if member.key == "format" {
            format_seen = true;
            let supported = matches!(&member.value.value, Json::Number(n) if n.as_u64() == Some(u64::from(format)));
            if !supported {
                return Err(at(
                    &member.value,
                    file,
                    Diagnostic::error(
                        "unsupported_format",
                        format!("This {what} file is not format {format}."),
                        "Run `wen migrate` to upgrade an older file, or update the engine for a newer one.",
                    ),
                ));
            }
        } else {
            out.insert(member.key.clone(), member.value.to_value());
        }
    }
    if !format_seen {
        return Err(at(
            root,
            file,
            Diagnostic::error(
                "missing_format",
                format!("The {what} file has no \"format\" key."),
                format!("Add \"format\": {format} at the top level."),
            ),
        ));
    }
    Ok(out)
}

/// A project path in quotes at `pointer`, checked as an asset reference.
fn asset_path(value: &Value, pointer: &str, code: &'static str, example: &str) -> Result<String> {
    let Value::String(path) = value else {
        return Err(mismatch(
            pointer,
            format!("{pointer} must be a path in quotes, not {value}."),
            &format!("Write it like \"{example}\"."),
        ));
    };
    if let Err(problem) = wish_core::asset::validate_asset_ref(path) {
        return Err(Diagnostic::error(
            "invalid_asset_ref",
            format!("'{path}' is not a valid asset reference: {problem}."),
            format!(
                "Write a path relative to the project folder with '/' between folders, such as \"{example}\"."
            ),
        )
        .at_path(pointer.to_owned()));
    }
    if path.is_empty() {
        return Err(invalid(
            code,
            pointer,
            "The path is empty.",
            format!("Name the file, such as \"{example}\"."),
        ));
    }
    Ok(path.clone())
}

/// A name of 1 to [`MAX_NAME`] bytes with no control characters.
fn good_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
}

// ---------------------------------------------------------------------------
// Tilesets
// ---------------------------------------------------------------------------

/// What one tile is, besides its picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileProps {
    /// Whether it is a wall in a layer that collides.
    pub solid: bool,
    /// Its collision outline, in tile units from its bottom left corner
    /// (`[0, 0]` to `[1, 1]`); `None`: the whole square.
    pub shape: Option<Vec<[f32; 2]>>,
    /// Its name, if it has one.
    pub name: Option<String>,
}

/// Where a tileset's tiles come from.
#[derive(Clone, Debug, PartialEq)]
pub enum TileSource {
    /// Cut from a picture by a grid.
    Picture {
        /// The picture's project path.
        image: String,
        /// Each tile: left, top, width, height in the picture's pixels.
        frames: Vec<[u32; 4]>,
    },
    /// The frames of a sprite sheet, in its order.
    Sheet(String),
}

/// What a tileset file means.
#[derive(Clone, Debug, PartialEq)]
pub struct Tileset {
    /// Where its tiles come from.
    pub source: TileSource,
    /// Properties by tile number (tiles without any are not listed).
    pub tiles: BTreeMap<u32, TileProps>,
}

impl Tileset {
    /// The properties of tile `tile` (the defaults for one without any).
    pub fn props(&self, tile: u32) -> Option<&TileProps> {
        self.tiles.get(&tile)
    }

    /// The picture and every tile's rectangle: the grid's, or the sheet's
    /// (read from the project at `root` with its size cap).
    pub fn frames(&self, root: &Path) -> Result<(String, Vec<[u32; 4]>)> {
        match &self.source {
            TileSource::Picture { image, frames } => Ok((image.clone(), frames.clone())),
            TileSource::Sheet(path) => {
                let text = crate::sheet::read_sheet_file(&root.join(path), path)?;
                let sheet = crate::sheet::parse_sheet(&text, path)?.decode()?;
                Ok((sheet.image, sheet.frames))
            }
        }
    }
}

/// A tileset file as a document: what it writes, everything but its format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TilesetDocument {
    /// The keys the file writes (`image` and `grid`, or `sheet`; `tiles`).
    pub tileset: Map<String, Value>,
}

/// Reads a tileset file's text and checks what it says. `file` is the
/// project-relative path shown in diagnostics (or `""`).
pub fn parse_tileset(source: &str, file: &str) -> Result<TilesetDocument> {
    let root = json::parse(source, file)?;
    let tileset = members_of(
        &root,
        file,
        TILESET_FORMAT,
        "tileset",
        "Write { \"format\": 1, \"image\": \"assets/tiles/station.png\", \"grid\": { \"frame\": [16, 16], \"columns\": 8, \"rows\": 4 } }.",
    )?;
    let parsed = TilesetDocument { tileset };
    parsed
        .decode()
        .map_err(|problem| locate(&root, file, problem))?;
    Ok(parsed)
}

impl TilesetDocument {
    /// A tileset from the keys a request gives (`asset_create`'s
    /// `tileset`): checked like a file. Diagnostics point into `/tileset`.
    pub fn from_spec(spec: Map<String, Value>) -> Result<Self> {
        if spec.contains_key("format") {
            return Err(invalid(
                "tileset_invalid",
                "/tileset/format",
                "The format is not part of the tileset's description.",
                "Leave \"format\" out: the engine writes it.",
            ));
        }
        let document = Self { tileset: spec };
        document.decode().map_err(|mut problem| {
            let pointer = problem.location.path.clone().unwrap_or_default();
            problem.location.path = Some(format!("/tileset{pointer}"));
            problem
        })?;
        Ok(document)
    }

    /// What the tileset means. Fails with the first thing that is wrong.
    pub fn decode(&self) -> Result<Tileset> {
        const CODE: &str = "tileset_invalid";
        known_keys(&self.tileset, &TILESET_KEYS, "", "A tileset")?;
        let image = self.tileset.get("image");
        let grid = self.tileset.get("grid");
        let sheet = self.tileset.get("sheet");
        let source = match (image, grid, sheet) {
            (Some(image), Some(grid), None) => {
                let image = asset_path(image, "/image", CODE, "assets/tiles/station.png")?;
                let frames = crate::sheet::grid_frames(grid, CODE)?;
                if frames.len() > MAX_TILES {
                    return Err(invalid(
                        CODE,
                        "/grid",
                        format!(
                            "The grid cuts {} tiles; a tileset has at most {MAX_TILES}.",
                            frames.len()
                        ),
                        "Use fewer, larger tiles, or two tilesets.",
                    ));
                }
                TileSource::Picture { image, frames }
            }
            (None, None, Some(sheet)) => {
                let path = asset_path(sheet, "/sheet", CODE, "assets/tiles/station.sheet.json")?;
                if !crate::sheet::is_sheet(&path) {
                    return Err(Diagnostic::error(
                        "not_a_sheet_file",
                        format!("'{path}' is not a sprite sheet: a tileset's \"sheet\" names a file ending in .sheet.json."),
                        "Name a sheet, or cut the tiles from a picture instead: \"image\" with a \"grid\".",
                    )
                    .at_path("/sheet"));
                }
                TileSource::Sheet(path)
            }
            (_, _, Some(_)) => {
                return Err(invalid(
                    CODE,
                    "/sheet",
                    "The tileset has a \"sheet\" and an \"image\" or \"grid\" too.",
                    "Take the tiles one way: \"image\" with a \"grid\", or the frames of a \"sheet\".",
                ));
            }
            (Some(_), None, None) => {
                return Err(invalid(
                    CODE,
                    "",
                    "The tileset names a picture but does not say how it is cut (\"grid\").",
                    "Add \"grid\": { \"frame\": [16, 16], \"columns\": 8, \"rows\": 4 }: the tile size in pixels and how many across and down.",
                ));
            }
            (None, Some(_), None) => {
                return Err(invalid(
                    CODE,
                    "",
                    "The tileset has a grid but names no picture (\"image\").",
                    "Add \"image\": \"assets/tiles/station.png\": the PNG the tiles are cut from.",
                ));
            }
            (None, None, None) => {
                return Err(invalid(
                    CODE,
                    "",
                    "The tileset says nowhere its tiles come from.",
                    "Add \"image\" (a PNG) with a \"grid\", or \"sheet\" (a sprite sheet whose frames are the tiles).",
                ));
            }
        };
        let count = match &source {
            TileSource::Picture { frames, .. } => frames.len(),
            TileSource::Sheet(_) => MAX_TILES,
        };
        let tiles = match self.tileset.get("tiles") {
            None => BTreeMap::new(),
            Some(Value::Object(tiles)) => {
                let mut found = BTreeMap::new();
                for (key, props) in tiles {
                    let (number, props) = decode_tile(key, props, count)?;
                    found.insert(number, props);
                }
                found
            }
            Some(other) => {
                return Err(mismatch(
                    "/tiles",
                    format!("\"tiles\" must be an object of tiles by number, not {other}."),
                    "Write it like \"tiles\": { \"8\": { \"solid\": true } }.",
                ));
            }
        };
        Ok(Tileset { source, tiles })
    }

    /// The file as JSON, in the form the file holds.
    pub fn to_value(&self) -> Value {
        let mut object = self.tileset.clone();
        object.insert("format".to_owned(), Value::from(TILESET_FORMAT));
        Value::Object(object)
    }

    /// The tileset file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        json::to_canonical(&self.to_value())
    }

    /// The picture or sheet the tileset names, with its JSON pointer.
    pub fn source(&self) -> Option<RefUse> {
        refs_in_tileset(&self.to_value()).into_iter().next()
    }
}

fn decode_tile(key: &str, props: &Value, count: usize) -> Result<(u32, TileProps)> {
    const CODE: &str = "tileset_invalid";
    let pointer = format!("/tiles/{}", key.replace('~', "~0").replace('/', "~1"));
    let number = key
        .parse::<u32>()
        .ok()
        .filter(|number| number.to_string() == key);
    let Some(number) = number else {
        return Err(invalid(
            CODE,
            &pointer,
            format!("'{key}' is not a tile number."),
            "Tiles are numbered from 0, left to right and then down; write the number in quotes without leading zeros, such as \"8\".",
        ));
    };
    if number as usize >= count {
        return Err(Diagnostic::error(
            "value_out_of_range",
            format!(
                "Tile {number} is given properties, but the tileset has {count} tiles (0 to {}).",
                count.saturating_sub(1)
            ),
            "Tiles are numbered from 0, left to right and then down in the grid. Use the number of a tile the tileset has.",
        )
        .at_path(pointer));
    }
    let Value::Object(props) = props else {
        return Err(mismatch(
            &pointer,
            format!("The properties of tile {number} must be an object, not {props}."),
            "Write it like { \"solid\": true, \"name\": \"wall\" }.",
        ));
    };
    known_keys(props, &TILE_KEYS, &pointer, "A tile")?;
    let solid = match props.get("solid") {
        None => false,
        Some(Value::Bool(solid)) => *solid,
        Some(other) => {
            return Err(mismatch(
                &format!("{pointer}/solid"),
                format!("\"solid\" must be true or false, not {other}."),
                "true makes the tile a wall in layers that collide.",
            ));
        }
    };
    let name = match props.get("name") {
        None => None,
        Some(Value::String(name)) if good_name(name) => Some(name.clone()),
        Some(other) => {
            return Err(invalid(
                CODE,
                &format!("{pointer}/name"),
                format!("{other} cannot be a tile's name."),
                format!(
                    "A tile's name is 1 to {MAX_NAME} bytes of text in quotes, such as \"wall\"."
                ),
            ));
        }
    };
    let shape = match props.get("shape") {
        None => None,
        Some(value) => Some(decode_shape(value, &format!("{pointer}/shape"))?),
    };
    if shape.is_some() && !solid {
        return Err(invalid(
            CODE,
            &format!("{pointer}/shape"),
            format!("Tile {number} has a collision shape but is not solid."),
            "Add \"solid\": true (a shape is the outline of a solid tile), or leave the shape out.",
        ));
    }
    Ok((number, TileProps { solid, shape, name }))
}

fn decode_shape(value: &Value, pointer: &str) -> Result<Vec<[f32; 2]>> {
    const CODE: &str = "tileset_invalid";
    let Value::Array(points) = value else {
        return Err(mismatch(
            pointer,
            format!("A shape must be a list of [x, y] points, not {value}."),
            "Write it like [[0, 0], [1, 0], [1, 1]]: corners in tile units from the tile's bottom left.",
        ));
    };
    if points.len() < 3 || points.len() > MAX_SHAPE_POINTS {
        return Err(invalid(
            CODE,
            pointer,
            format!(
                "A shape has {} points; it has 3 to {MAX_SHAPE_POINTS}.",
                points.len()
            ),
            "List the outline's corners, such as [[0, 0], [1, 0], [1, 1]] for a slope.",
        ));
    }
    let mut found = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        let at = format!("{pointer}/{index}");
        let pair = point
            .as_array()
            .filter(|pair| pair.len() == 2)
            .and_then(|pair| Some([pair[0].as_f64()?, pair[1].as_f64()?]));
        match pair {
            Some([x, y]) if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => {
                found.push([x as f32, y as f32]);
            }
            _ => {
                return Err(Diagnostic::error(
                    "value_out_of_range",
                    format!("{at} must be [x, y] with both from 0 to 1, not {point}."),
                    "Points are in tile units from the tile's bottom left corner: [0, 0] to [1, 1].",
                )
                .at_path(at));
            }
        }
    }
    // Twice the area, by the shoelace formula: a shape must enclose some.
    let area: f32 = (0..found.len())
        .map(|i| {
            let [x0, y0] = found[i];
            let [x1, y1] = found[(i + 1) % found.len()];
            x0 * y1 - x1 * y0
        })
        .sum();
    if area.abs() < 1e-3 {
        return Err(invalid(
            CODE,
            pointer,
            "The shape's points lie on a line: it has no area.",
            "Give at least three corners that are not in a line, such as [[0, 0], [1, 0], [1, 1]].",
        ));
    }
    Ok(found)
}

/// Checks a tileset file's text as part of a project: everything
/// [`parse_tileset`] checks, that its picture (a PNG) or its sheet exists in
/// `index`, that tile properties name tiles a sheet has (`root` is the
/// project folder, for the sheet), and that the text is canonical (a
/// warning).
pub fn check_tileset(
    source: &str,
    file: &str,
    index: &AssetIndex,
    root: Option<&Path>,
) -> (Option<TilesetDocument>, Vec<Diagnostic>) {
    let parsed = match parse_tileset(source, file) {
        Ok(parsed) => parsed,
        Err(problem) => return (None, vec![problem]),
    };
    let Ok(node) = json::parse(source, file) else {
        return (None, Vec::new());
    };
    let mut diagnostics = Vec::new();
    if let Some(used) = parsed.source() {
        let sheet = used.pointer == "/sheet";
        let problem = index.check(&used.reference).err().or_else(|| {
            (!sheet && !used.reference.to_ascii_lowercase().ends_with(".png")).then(|| {
                Diagnostic::error(
                    "texture_not_png",
                    format!(
                        "'{}' is not a PNG file: a tileset's picture is read from PNG files only.",
                        used.reference
                    ),
                    "Convert the picture to PNG (8-bit) and name that file.",
                )
            })
        });
        match problem {
            Some(problem) => {
                diagnostics.push(locate(&node, file, problem.at_path(used.pointer.clone())));
            }
            None => {
                if let (true, Some(root), Ok(tileset)) = (sheet, root, parsed.decode()) {
                    match tileset.frames(root) {
                        Ok((_, frames)) => {
                            if let Some((&number, _)) =
                                tileset.tiles.range(frames.len() as u32..).next()
                            {
                                diagnostics.push(locate(
                                    &node,
                                    file,
                                    Diagnostic::warning(
                                        "tile_not_in_tileset",
                                        format!("Tile {number} is given properties, but the sheet '{}' has {} frames.", used.reference, frames.len()),
                                        "A tileset's tiles are its sheet's frames, numbered from 0. Give properties to frames the sheet has.",
                                    )
                                    .at_path(format!("/tiles/{number}")),
                                ));
                            }
                        }
                        Err(problem) => diagnostics.push(
                            Diagnostic::warning(
                                "sheet_not_loaded",
                                format!(
                                    "The tileset's sheet '{}' could not be read: {}",
                                    used.reference, problem.message
                                ),
                                problem.hint.clone(),
                            )
                            .at_file(file)
                            .at_path("/sheet"),
                        ),
                    }
                }
            }
        }
    }
    if parsed.to_canonical().ok().as_deref() != Some(source) {
        diagnostics.push(not_canonical(file));
    }
    let failed = diagnostics.iter().any(Diagnostic::is_error);
    ((!failed).then_some(parsed), diagnostics)
}

fn not_canonical(file: &str) -> Diagnostic {
    let warning = Diagnostic::warning(
        "not_canonical",
        "This file is not in canonical form.",
        "Run `wen fmt` to rewrite it. Saving through the engine does this automatically.",
    )
    .at_line(1, Some(1));
    if file.is_empty() {
        warning
    } else {
        warning.at_file(file)
    }
}

// ---------------------------------------------------------------------------
// Tilemaps
// ---------------------------------------------------------------------------

/// The cells of a chunk, row by row from its bottom row, left to right:
/// cell `(x, y)` of the chunk is at `y * 16 + x`.
pub type ChunkTiles = [i32; CHUNK_CELLS];

/// One layer of a tilemap.
#[derive(Clone, Debug, PartialEq)]
pub struct TileLayer {
    /// Its name, unique in the map.
    pub name: String,
    /// Whether its solid tiles are walls.
    pub collide: bool,
    /// Its chunks by chunk coordinates; a chunk holds at least one tile.
    /// Shared between copies of the map, copied when changed.
    pub chunks: BTreeMap<(i32, i32), Arc<ChunkTiles>>,
}

impl TileLayer {
    /// An empty layer.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            collide: true,
            chunks: BTreeMap::new(),
        }
    }

    /// The tile at cell `(x, y)` ([`EMPTY`] for none).
    pub fn get(&self, x: i32, y: i32) -> i32 {
        let (key, index) = split(x, y);
        self.chunks.get(&key).map_or(EMPTY, |chunk| chunk[index])
    }

    /// How many cells hold a tile.
    pub fn tile_count(&self) -> usize {
        self.chunks
            .values()
            .map(|chunk| chunk.iter().filter(|tile| **tile != EMPTY).count())
            .sum()
    }

    /// Every cell that holds a tile, as `(x, y, tile)`, chunk by chunk in
    /// key order, each chunk bottom row first.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.chunks.iter().flat_map(|(&(cx, cy), chunk)| {
            chunk
                .iter()
                .enumerate()
                .filter(|(_, tile)| **tile != EMPTY)
                .map(move |(index, tile)| {
                    (
                        cx * CHUNK + (index as i32 % CHUNK),
                        cy * CHUNK + (index as i32 / CHUNK),
                        *tile,
                    )
                })
        })
    }

    /// The box its tiles cover: lowest x and y, highest x and y (cells,
    /// included); `None` when it is empty.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let mut found: Option<[i32; 4]> = None;
        for (x, y, _) in self.cells() {
            found = Some(match found {
                None => [x, y, x, y],
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            });
        }
        found
    }
}

/// The chunk of cell `(x, y)` and the cell's place in it.
pub fn split(x: i32, y: i32) -> ((i32, i32), usize) {
    let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
    let index = (y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize;
    (key, index)
}

/// What a tilemap file means.
#[derive(Clone, Debug, PartialEq)]
pub struct Tilemap {
    /// The tileset's project path.
    pub tileset: String,
    /// The layers, drawn in this order.
    pub layers: Vec<TileLayer>,
}

impl Tilemap {
    /// A map of `tileset` with no layers.
    pub fn new(tileset: impl Into<String>) -> Self {
        Self {
            tileset: tileset.into(),
            layers: Vec::new(),
        }
    }

    /// The index of the layer called `name`.
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.layers.iter().position(|layer| layer.name == name)
    }

    /// The names of the layers, in order.
    pub fn layer_names(&self) -> Vec<&str> {
        self.layers
            .iter()
            .map(|layer| layer.name.as_str())
            .collect()
    }

    /// How many chunks the layers hold together.
    pub fn chunk_count(&self) -> usize {
        self.layers.iter().map(|layer| layer.chunks.len()).sum()
    }

    /// The tile of layer `layer` at cell `(x, y)`.
    pub fn get(&self, layer: usize, x: i32, y: i32) -> i32 {
        self.layers.get(layer).map_or(EMPTY, |l| l.get(x, y))
    }

    /// Puts `tile` ([`EMPTY`] to erase) into cell `(x, y)` of layer
    /// `layer`; whether the cell changed. A chunk left with no tile is
    /// dropped; a new chunk past [`MAX_CHUNKS`] is refused.
    pub fn set(&mut self, layer: usize, x: i32, y: i32, tile: i32) -> Result<bool> {
        if !(MIN_CELL..=MAX_CELL).contains(&x) || !(MIN_CELL..=MAX_CELL).contains(&y) {
            return Err(Diagnostic::error(
                "value_out_of_range",
                format!(
                    "Cell ({x}, {y}) is outside a tilemap: cells are from {MIN_CELL} to {MAX_CELL}."
                ),
                "Paint nearer the map's origin.",
            ));
        }
        if !(EMPTY..=MAX_TILE).contains(&tile) {
            return Err(Diagnostic::error(
                "value_out_of_range",
                format!("{tile} is not a tile: tiles are 0 to {MAX_TILE}, and -1 is empty."),
                "Use a tile number of the tileset (asset_info on it lists them), or -1 to erase.",
            ));
        }
        let total = self.chunk_count();
        let Some(target) = self.layers.get_mut(layer) else {
            return Err(Diagnostic::error(
                "internal_error",
                "A tilemap layer is missing.",
                "This is a bug in Wish Engine; please report it.",
            ));
        };
        let (key, index) = split(x, y);
        match target.chunks.get_mut(&key) {
            Some(chunk) => {
                if chunk[index] == tile {
                    return Ok(false);
                }
                Arc::make_mut(chunk)[index] = tile;
                if tile == EMPTY && chunk.iter().all(|cell| *cell == EMPTY) {
                    target.chunks.remove(&key);
                }
                Ok(true)
            }
            None if tile == EMPTY => Ok(false),
            None => {
                if total >= MAX_CHUNKS {
                    return Err(Diagnostic::error(
                        "tilemap_too_large",
                        format!(
                            "The tilemap would have more than {MAX_CHUNKS} chunks of 16 x 16 cells."
                        ),
                        "Split the map into several tilemaps (each its own entity with a Tilemap), or erase what is not needed.",
                    ));
                }
                let mut chunk = [EMPTY; CHUNK_CELLS];
                chunk[index] = tile;
                target.chunks.insert(key, Arc::new(chunk));
                Ok(true)
            }
        }
    }

    /// The box every layer's tiles cover together (cells, included).
    pub fn bounds(&self) -> Option<[i32; 4]> {
        self.layers
            .iter()
            .filter_map(TileLayer::bounds)
            .reduce(|[a, b, c, d], [e, f, g, h]| [a.min(e), b.min(f), c.max(g), d.max(h)])
    }

    /// The highest tile number the map holds, if any.
    pub fn highest_tile(&self) -> Option<i32> {
        self.layers
            .iter()
            .flat_map(|layer| layer.chunks.values())
            .flat_map(|chunk| chunk.iter().copied())
            .filter(|tile| *tile != EMPTY)
            .max()
    }

    /// A map from the keys a request gives (`asset_create`'s `tilemap`):
    /// `tileset`, and `layers` as a file writes them, or as names only.
    /// Diagnostics point into `/tilemap`.
    pub fn from_spec(spec: Map<String, Value>) -> Result<Self> {
        let mut spec = spec;
        if spec.contains_key("format") {
            return Err(invalid(
                "tilemap_invalid",
                "/tilemap/format",
                "The format is not part of the tilemap's description.",
                "Leave \"format\" out: the engine writes it.",
            ));
        }
        // Layers may be given by name alone.
        if let Some(Value::Array(layers)) = spec.get_mut("layers") {
            for layer in layers.iter_mut() {
                if let Value::String(name) = layer {
                    *layer = serde_json::json!({ "name": name.clone() });
                }
            }
        }
        decode_tilemap(&spec).map_err(|mut problem| {
            let pointer = problem.location.path.clone().unwrap_or_default();
            problem.location.path = Some(format!("/tilemap{pointer}"));
            problem
        })
    }

    /// The file as JSON, in canonical shape: empty chunks and defaults left
    /// out.
    pub fn to_value(&self) -> Value {
        let layers: Vec<Value> = self
            .layers
            .iter()
            .map(|layer| {
                let mut object = Map::new();
                object.insert("name".to_owned(), Value::from(layer.name.clone()));
                if !layer.collide {
                    object.insert("collide".to_owned(), Value::Bool(false));
                }
                let chunks: Map<String, Value> = layer
                    .chunks
                    .iter()
                    .filter(|(_, chunk)| chunk.iter().any(|cell| *cell != EMPTY))
                    .map(|(&(cx, cy), chunk)| {
                        let rows: Vec<Value> = (0..CHUNK)
                            .rev()
                            .map(|row| {
                                let start = (row * CHUNK) as usize;
                                Value::Array(
                                    chunk[start..start + CHUNK as usize]
                                        .iter()
                                        .map(|tile| Value::from(*tile))
                                        .collect(),
                                )
                            })
                            .collect();
                        (format!("{cx},{cy}"), Value::Array(rows))
                    })
                    .collect();
                if !chunks.is_empty() {
                    object.insert("chunks".to_owned(), Value::Object(chunks));
                }
                Value::Object(object)
            })
            .collect();
        let mut object = Map::new();
        object.insert("format".to_owned(), Value::from(TILEMAP_FORMAT));
        object.insert("tileset".to_owned(), Value::from(self.tileset.clone()));
        object.insert("layers".to_owned(), Value::Array(layers));
        Value::Object(object)
    }

    /// The tilemap file's canonical text.
    pub fn to_canonical(&self) -> Result<String> {
        json::to_canonical(&self.to_value())
    }
}

/// Reads a tilemap file's text and checks what it says. `file` is the
/// project-relative path shown in diagnostics (or `""`).
pub fn parse_tilemap(source: &str, file: &str) -> Result<Tilemap> {
    let root = json::parse(source, file)?;
    let members = members_of(
        &root,
        file,
        TILEMAP_FORMAT,
        "tilemap",
        "Write { \"format\": 1, \"tileset\": \"assets/tiles/station.tileset.json\", \"layers\": [{ \"name\": \"floor\" }] }.",
    )?;
    decode_tilemap(&members).map_err(|problem| locate(&root, file, problem))
}

fn decode_tilemap(members: &Map<String, Value>) -> Result<Tilemap> {
    const CODE: &str = "tilemap_invalid";
    known_keys(members, &TILEMAP_KEYS, "", "A tilemap")?;
    let tileset = match members.get("tileset") {
        Some(value) => asset_path(value, "/tileset", CODE, "assets/tiles/station.tileset.json")?,
        None => {
            return Err(invalid(
                CODE,
                "",
                "The tilemap names no tileset (\"tileset\").",
                "Add \"tileset\": \"assets/tiles/station.tileset.json\": the file that says what each tile number looks like.",
            ));
        }
    };
    if !is_tileset(&tileset) {
        return Err(Diagnostic::error(
            "not_a_tileset_file",
            format!("'{tileset}' is not a tileset: a tilemap's \"tileset\" names a file ending in {TILESET_SUFFIX}."),
            "Name a tileset (asset_create makes one from a picture and a grid).",
        )
        .at_path("/tileset"));
    }
    let listed = match members.get("layers") {
        None => &Vec::new(),
        Some(Value::Array(layers)) => layers,
        Some(other) => {
            return Err(mismatch(
                "/layers",
                format!("\"layers\" must be a list of layers, not {other}."),
                "Write it like \"layers\": [{ \"name\": \"floor\" }, { \"name\": \"walls\" }].",
            ));
        }
    };
    if listed.len() > MAX_LAYERS {
        return Err(invalid(
            CODE,
            "/layers",
            format!(
                "The tilemap has {} layers; at most {MAX_LAYERS}.",
                listed.len()
            ),
            "Put tiles that never overlap on one layer, or split the map into several tilemaps.",
        ));
    }
    let mut layers = Vec::with_capacity(listed.len());
    let mut names = BTreeSet::new();
    let mut chunk_count = 0usize;
    for (number, layer) in listed.iter().enumerate() {
        let pointer = format!("/layers/{number}");
        let Value::Object(layer) = layer else {
            return Err(mismatch(
                &pointer,
                format!("A layer must be an object, not {layer}."),
                "Write it like { \"name\": \"floor\", \"chunks\": { ... } }.",
            ));
        };
        known_keys(layer, &LAYER_KEYS, &pointer, "A layer")?;
        let name = match layer.get("name") {
            Some(Value::String(name)) if good_name(name) => name.clone(),
            Some(other) => {
                return Err(invalid(
                    CODE,
                    &format!("{pointer}/name"),
                    format!("{other} cannot be a layer's name."),
                    format!(
                        "A layer's name is 1 to {MAX_NAME} bytes of text in quotes, such as \"walls\"."
                    ),
                ));
            }
            None => {
                return Err(invalid(
                    CODE,
                    &pointer,
                    "The layer has no name.",
                    "Add \"name\": \"floor\" (each layer's name is different).",
                ));
            }
        };
        if !names.insert(name.clone()) {
            return Err(invalid(
                CODE,
                &format!("{pointer}/name"),
                format!("Two layers are called '{name}'."),
                "Give each layer a name of its own.",
            ));
        }
        let collide = match layer.get("collide") {
            None => true,
            Some(Value::Bool(collide)) => *collide,
            Some(other) => {
                return Err(mismatch(
                    &format!("{pointer}/collide"),
                    format!("\"collide\" must be true or false, not {other}."),
                    "false makes the layer's solid tiles pass-through (decoration).",
                ));
            }
        };
        let mut chunks = BTreeMap::new();
        match layer.get("chunks") {
            None => {}
            Some(Value::Object(listed)) => {
                for (key, rows) in listed {
                    let at = format!(
                        "{pointer}/chunks/{}",
                        key.replace('~', "~0").replace('/', "~1")
                    );
                    let coordinates = chunk_key(key).ok_or_else(|| {
                        invalid(
                            CODE,
                            &at,
                            format!("'{key}' is not a chunk's coordinates."),
                            format!(
                                "A chunk's key is two whole numbers from -{MAX_CHUNK_COORD} to {} with a comma and no spaces, such as \"0,-1\" (cell x is in chunk floor(x / 16)).",
                                MAX_CHUNK_COORD - 1
                            ),
                        )
                    })?;
                    let chunk = decode_chunk(rows, &at)?;
                    if chunk.iter().any(|cell| *cell != EMPTY) {
                        chunks.insert(coordinates, Arc::new(chunk));
                    }
                }
            }
            Some(other) => {
                return Err(mismatch(
                    &format!("{pointer}/chunks"),
                    format!("\"chunks\" must be an object of chunks by \"x,y\", not {other}."),
                    "Write it like \"chunks\": { \"0,0\": [[...16 numbers], ...16 rows] }.",
                ));
            }
        }
        chunk_count += chunks.len();
        if chunk_count > MAX_CHUNKS {
            return Err(invalid(
                "tilemap_too_large",
                &format!("{pointer}/chunks"),
                format!("The tilemap has more than {MAX_CHUNKS} chunks of 16 x 16 cells."),
                "Split the map into several tilemaps (each its own entity with a Tilemap).",
            ));
        }
        layers.push(TileLayer {
            name,
            collide,
            chunks,
        });
    }
    Ok(Tilemap { tileset, layers })
}

/// The coordinates a chunk key such as `"0,-1"` names, if it is written
/// exactly so and in range.
fn chunk_key(key: &str) -> Option<(i32, i32)> {
    let (x, y) = key.split_once(',')?;
    let x: i32 = x.parse().ok()?;
    let y: i32 = y.parse().ok()?;
    let range = -MAX_CHUNK_COORD..MAX_CHUNK_COORD;
    (format!("{x},{y}") == key && range.contains(&x) && range.contains(&y)).then_some((x, y))
}

fn decode_chunk(rows: &Value, pointer: &str) -> Result<ChunkTiles> {
    let shape_hint = "A chunk is 16 rows of 16 numbers, the top row first: [[-1, -1, 8, ...], ...]. -1 is an empty cell.";
    let Some(rows) = rows.as_array().filter(|rows| rows.len() == CHUNK as usize) else {
        return Err(Diagnostic::error(
            "wrong_length",
            format!("{pointer} must be 16 rows of 16 numbers."),
            shape_hint,
        )
        .at_path(pointer.to_owned()));
    };
    let mut chunk = [EMPTY; CHUNK_CELLS];
    for (row, cells) in rows.iter().enumerate() {
        let at = format!("{pointer}/{row}");
        let Some(cells) = cells.as_array().filter(|c| c.len() == CHUNK as usize) else {
            return Err(Diagnostic::error(
                "wrong_length",
                format!("{at} must be a row of 16 numbers."),
                shape_hint,
            )
            .at_path(at));
        };
        // The file's first row is the chunk's top.
        let y = CHUNK as usize - 1 - row;
        for (x, cell) in cells.iter().enumerate() {
            let tile = cell
                .as_i64()
                .or_else(|| whole_number(cell).and_then(|n| i64::try_from(n).ok()));
            match tile {
                Some(tile) if (i64::from(EMPTY)..=i64::from(MAX_TILE)).contains(&tile) => {
                    chunk[y * CHUNK as usize + x] = tile as i32;
                }
                _ => {
                    return Err(Diagnostic::error(
                        "value_out_of_range",
                        format!("{at}/{x} is {cell}; a cell holds a tile number from 0 to {MAX_TILE}, or -1 for none."),
                        "Use the number of a tile of the tileset (asset_info on it lists them), or -1.",
                    )
                    .at_path(format!("{at}/{x}")));
                }
            }
        }
    }
    Ok(chunk)
}

/// Checks a tilemap file's text as part of a project: everything
/// [`parse_tilemap`] checks, that its tileset exists in `index`, that every
/// tile number is one of the tileset's (`tile_not_in_tileset`, a warning:
/// such a cell draws nothing; `root` is the project folder, to read the
/// tileset), and that the text is canonical (a warning).
pub fn check_tilemap(
    source: &str,
    file: &str,
    index: &AssetIndex,
    root: Option<&Path>,
) -> (Option<Tilemap>, Vec<Diagnostic>) {
    let map = match parse_tilemap(source, file) {
        Ok(map) => map,
        Err(problem) => return (None, vec![problem]),
    };
    let mut diagnostics = Vec::new();
    let located = |problem: Diagnostic| match json::parse(source, file) {
        Ok(node) => locate(&node, file, problem),
        Err(_) => problem.at_file(file),
    };
    match index.check(&map.tileset) {
        Err(problem) => diagnostics.push(located(problem.at_path("/tileset"))),
        Ok(()) => {
            let count = root.and_then(|root| tile_count(root, &map.tileset).ok());
            if let (Some(count), Some(highest)) = (count, map.highest_tile())
                && highest as usize >= count
            {
                // The first cell (layer order, chunk order) that names it.
                let pointer = first_cell_beyond(&map, count);
                diagnostics.push(located(
                    Diagnostic::warning(
                        "tile_not_in_tileset",
                        format!(
                            "The map holds tile {highest}, but its tileset '{}' has {count} tiles (0 to {}); such cells draw nothing.",
                            map.tileset,
                            count.saturating_sub(1)
                        ),
                        "Paint tiles the tileset has (asset_info on it lists them), or make the tileset's grid larger.",
                    )
                    .at_path(pointer),
                ));
            }
        }
    }
    if map.to_canonical().ok().as_deref() != Some(source) {
        diagnostics.push(not_canonical(file));
    }
    let failed = diagnostics.iter().any(Diagnostic::is_error);
    ((!failed).then_some(map), diagnostics)
}

/// The JSON pointer of the first cell holding a tile of `count` or more.
fn first_cell_beyond(map: &Tilemap, count: usize) -> String {
    for (number, layer) in map.layers.iter().enumerate() {
        for (&(cx, cy), chunk) in &layer.chunks {
            for row in 0..CHUNK as usize {
                let y = CHUNK as usize - 1 - row;
                for x in 0..CHUNK as usize {
                    let tile = chunk[y * CHUNK as usize + x];
                    if tile != EMPTY && tile as usize >= count {
                        return format!("/layers/{number}/chunks/{cx},{cy}/{row}/{x}");
                    }
                }
            }
        }
    }
    "/layers".to_owned()
}

/// How many tiles the tileset at `path` (in the project at `root`) has.
pub fn tile_count(root: &Path, path: &str) -> Result<usize> {
    let text = read_tileset_file(&root.join(path), path)?;
    let tileset = parse_tileset(&text, path)?.decode()?;
    Ok(tileset.frames(root)?.1.len())
}

/// Reads and decodes the tileset at `path` in the project at `root`.
pub fn read_tileset(root: &Path, path: &str) -> Result<Tileset> {
    let text = read_tileset_file(&root.join(path), path)?;
    parse_tileset(&text, path)?.decode()
}

/// Reads and decodes the tilemap at `path` in the project at `root`.
pub fn read_tilemap(root: &Path, path: &str) -> Result<Tilemap> {
    let text = read_tilemap_file(&root.join(path), path)?;
    parse_tilemap(&text, path)
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// Calls `visit` with the JSON pointer and a mutable string for each asset
/// reference in a tileset file's JSON: its `image` or its `sheet`.
pub(crate) fn visit_tileset(document: &mut Value, visit: &mut crate::refs::Visitor<'_>) {
    for key in ["image", "sheet"] {
        if let Some(Value::String(path)) = document.get_mut(key) {
            visit(&format!("/{key}"), path);
        }
    }
}

/// Calls `visit` with the JSON pointer and a mutable string for the asset
/// reference in a tilemap file's JSON: its `tileset`.
pub(crate) fn visit_tilemap(document: &mut Value, visit: &mut crate::refs::Visitor<'_>) {
    if let Some(Value::String(path)) = document.get_mut("tileset") {
        visit("/tileset", path);
    }
}

fn refs_with(
    document: &Value,
    visit: fn(&mut Value, &mut crate::refs::Visitor<'_>),
) -> Vec<RefUse> {
    let mut copy = document.clone();
    let mut found = Vec::new();
    visit(&mut copy, &mut |pointer, reference| {
        if !reference.is_empty() {
            found.push(RefUse {
                pointer: pointer.to_owned(),
                reference: reference.clone(),
            });
        }
    });
    found
}

/// Every non-empty asset reference in a tileset file's JSON.
pub fn refs_in_tileset(document: &Value) -> Vec<RefUse> {
    refs_with(document, visit_tileset)
}

/// Every non-empty asset reference in a tilemap file's JSON.
pub fn refs_in_tilemap(document: &Value) -> Vec<RefUse> {
    refs_with(document, visit_tilemap)
}

/// What kind of file `Tilemap.map` in a scene or prefab must name: a
/// tilemap. `None` when the reference fits, or is not a tilemap's.
pub(crate) fn kind_problem(pointer: &str, reference: &str) -> Option<Diagnostic> {
    let (parent, field) = pointer.rsplit_once('/')?;
    if reference.is_empty() || field != "map" || !parent.ends_with("/components/Tilemap") {
        return None;
    }
    let path = crate::refs::path_and_suffix(reference).0;
    (!is_tilemap(path)).then(|| {
        Diagnostic::error(
            "not_a_tilemap_file",
            format!("'{path}' is not a tilemap: Tilemap.map needs a file whose name ends in {TILEMAP_SUFFIX}."),
            "Name a tilemap such as \"maps/station.tilemap.json\" (asset_create makes one from a tileset).",
        )
    })
}

// ---------------------------------------------------------------------------
// Collision
// ---------------------------------------------------------------------------

/// The collision of a tilemap, in cells: solid squares merged into boxes,
/// and solid tiles with an outline of their own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileShapes {
    /// Boxes of whole solid cells: lowest x and y, then one past the highest
    /// x and y (a single cell `(3, 4)` is `[3, 4, 4, 5]`).
    pub boxes: Vec<[i32; 4]>,
    /// Cells whose tile has a collision outline: the cell and the outline in
    /// tile units from its bottom left corner.
    pub outlines: Vec<([i32; 2], Vec<[f32; 2]>)>,
}

/// The collision of `map` with the tile properties of `tileset`: a cell is
/// solid when a layer that collides holds a solid tile there. Whole solid
/// cells are merged into boxes, deterministically: cells are taken in
/// order (y up, then x right); each one not yet in a box starts one that
/// grows right as far as such cells go, then up while the whole row above
/// it is such cells. Cells with an outlined solid tile (and no whole solid
/// tile in another layer) are listed in the same order.
pub fn collision_shapes(map: &Tilemap, tileset: &Tileset) -> TileShapes {
    // (y, x): sorted the way cells are taken.
    let mut full: BTreeSet<(i32, i32)> = BTreeSet::new();
    let mut outlined: BTreeMap<(i32, i32), Vec<[f32; 2]>> = BTreeMap::new();
    for layer in map.layers.iter().filter(|layer| layer.collide) {
        for (x, y, tile) in layer.cells() {
            let Some(props) = u32::try_from(tile).ok().and_then(|t| tileset.props(t)) else {
                continue;
            };
            if !props.solid {
                continue;
            }
            match &props.shape {
                None => {
                    full.insert((y, x));
                }
                Some(shape) => {
                    outlined.entry((y, x)).or_insert_with(|| shape.clone());
                }
            }
        }
    }
    let mut used: BTreeSet<(i32, i32)> = BTreeSet::new();
    let free = |cell: &(i32, i32), used: &BTreeSet<(i32, i32)>| {
        full.contains(cell) && !used.contains(cell)
    };
    let mut boxes = Vec::new();
    for &(y, x) in &full {
        if used.contains(&(y, x)) {
            continue;
        }
        let mut width = 1;
        while free(&(y, x + width), &used) {
            width += 1;
        }
        let mut height = 1;
        while (0..width).all(|i| free(&(y + height, x + i), &used)) {
            height += 1;
        }
        for dy in 0..height {
            for dx in 0..width {
                used.insert((y + dy, x + dx));
            }
        }
        boxes.push([x, y, x + width, y + height]);
    }
    let outlines = outlined
        .into_iter()
        .filter(|(cell, _)| !full.contains(cell))
        .map(|((y, x), shape)| ([x, y], shape))
        .collect();
    TileShapes { boxes, outlines }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILESET: &str = r#"{
  "format": 1,
  "grid": { "columns": 4, "frame": [16, 16], "rows": 2 },
  "image": "tiles/station.png",
  "tiles": {
    "1": { "name": "wall", "solid": true },
    "2": { "shape": [[0, 0], [1, 0], [1, 1]], "solid": true }
  }
}"#;

    fn chunk_text(rows: &[[i32; 16]; 16]) -> String {
        let rows: Vec<String> = rows
            .iter()
            .map(|row| {
                format!(
                    "[{}]",
                    row.iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect();
        format!("[{}]", rows.join(", "))
    }

    fn map_with(rows: &[[i32; 16]; 16]) -> String {
        format!(
            r#"{{ "format": 1, "tileset": "tiles/station.tileset.json",
              "layers": [ {{ "name": "walls", "chunks": {{ "0,0": {} }} }} ] }}"#,
            chunk_text(rows)
        )
    }

    #[test]
    fn a_tileset_cuts_a_grid_and_keeps_tile_properties() {
        let tileset = parse_tileset(TILESET, "t.tileset.json")
            .unwrap()
            .decode()
            .unwrap();
        let TileSource::Picture { image, frames } = &tileset.source else {
            panic!("a picture");
        };
        assert_eq!(image, "tiles/station.png");
        assert_eq!((frames.len(), frames[5]), (8, [16, 16, 16, 16]));
        assert!(tileset.props(1).unwrap().solid);
        assert_eq!(tileset.props(1).unwrap().name.as_deref(), Some("wall"));
        assert_eq!(tileset.props(2).unwrap().shape.as_ref().unwrap().len(), 3);
        assert!(tileset.props(0).is_none());
    }

    #[test]
    fn tileset_mistakes_are_located_with_a_hint() {
        let cases = [
            (
                TILESET.replace("\"1\":", "\"01\":"),
                "tileset_invalid",
                "/tiles/01",
            ),
            (
                TILESET.replace("\"1\":", "\"9\":"),
                "value_out_of_range",
                "/tiles/9",
            ),
            (
                TILESET.replace("\"name\": \"wall\", \"solid\": true", "\"solid\": 1"),
                "type_mismatch",
                "/tiles/1/solid",
            ),
            (
                TILESET.replace("[1, 1]]", "[1, 2]]"),
                "value_out_of_range",
                "/tiles/2/shape/2",
            ),
            (
                TILESET.replace(
                    "[[0, 0], [1, 0], [1, 1]], \"solid\": true",
                    "[[0, 0], [1, 0], [1, 1]]",
                ),
                "tileset_invalid",
                "/tiles/2/shape",
            ),
            (
                TILESET.replace("[[0, 0], [1, 0], [1, 1]]", "[[0, 0], [0.5, 0.5], [1, 1]]"),
                "tileset_invalid",
                "/tiles/2/shape",
            ),
            (
                TILESET.replace("\"name\": \"wall\", ", "\"nmae\": \"wall\", "),
                "unknown_key",
                "/tiles/1/nmae",
            ),
        ];
        for (text, code, pointer) in cases {
            let problem = parse_tileset(&text, "t.tileset.json").unwrap_err();
            assert_eq!(problem.code, code, "{text}");
            assert_eq!(problem.location.path.as_deref(), Some(pointer), "{text}");
            assert!(problem.location.line.is_some());
            assert!(!problem.hint.is_empty());
        }
        let both = TILESET.replace("\"image\"", "\"sheet\": \"a.sheet.json\", \"image\"");
        assert_eq!(
            parse_tileset(&both, "t").unwrap_err().code,
            "tileset_invalid"
        );
        let sheet = r#"{ "format": 1, "sheet": "a.png" }"#;
        assert_eq!(
            parse_tileset(sheet, "t").unwrap_err().code,
            "not_a_sheet_file"
        );
        let from_sheet = r#"{ "format": 1, "sheet": "tiles/a.sheet.json", "tiles": { "40": { "solid": true } } }"#;
        let decoded = parse_tileset(from_sheet, "t").unwrap().decode().unwrap();
        assert_eq!(
            decoded.source,
            TileSource::Sheet("tiles/a.sheet.json".into())
        );
    }

    #[test]
    fn a_tileset_round_trips_canonically() {
        let parsed = parse_tileset(TILESET, "t").unwrap();
        let canonical = parsed.to_canonical().unwrap();
        let again = parse_tileset(&canonical, "t").unwrap();
        assert_eq!(again, parsed);
        assert_eq!(again.to_canonical().unwrap(), canonical);
    }

    #[test]
    fn chunk_rows_are_written_top_first() {
        let mut rows = [[EMPTY; 16]; 16];
        rows[0][0] = 1; // the top left cell: (0, 15)
        rows[15][3] = 2; // the bottom row: (3, 0)
        let map = parse_tilemap(&map_with(&rows), "m.tilemap.json").unwrap();
        assert_eq!(map.get(0, 0, 15), 1);
        assert_eq!(map.get(0, 3, 0), 2);
        assert_eq!(map.get(0, 0, 0), EMPTY);
        assert_eq!(map.layers[0].bounds(), Some([0, 0, 3, 15]));
        let canonical = map.to_canonical().unwrap();
        // A row of numbers is one line.
        assert!(
            canonical.contains("[1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1]")
        );
        let again = parse_tilemap(&canonical, "m").unwrap();
        assert_eq!(again, map);
        assert_eq!(again.to_canonical().unwrap(), canonical);
    }

    #[test]
    fn painting_shares_chunks_and_drops_empty_ones() {
        let mut map = Tilemap::new("tiles/station.tileset.json");
        map.layers.push(TileLayer::new("floor"));
        assert!(map.set(0, -1, -1, 3).unwrap());
        assert!(!map.set(0, -1, -1, 3).unwrap());
        assert_eq!(
            map.layers[0].chunks.keys().copied().collect::<Vec<_>>(),
            [(-1, -1)]
        );
        assert_eq!(map.get(0, -1, -1), 3);
        let before = map.clone();
        map.set(0, 20, 0, 1).unwrap();
        // The untouched chunk is shared with the copy.
        assert!(Arc::ptr_eq(
            &before.layers[0].chunks[&(-1, -1)],
            &map.layers[0].chunks[&(-1, -1)]
        ));
        map.set(0, -1, -1, EMPTY).unwrap();
        assert_eq!(
            map.layers[0].chunks.keys().copied().collect::<Vec<_>>(),
            [(1, 0)]
        );
        assert_eq!(
            map.set(0, 0, 0, 70_000).unwrap_err().code,
            "value_out_of_range"
        );
        assert_eq!(
            map.set(0, MAX_CELL + 1, 0, 1).unwrap_err().code,
            "value_out_of_range"
        );
        let canonical = map.to_canonical().unwrap();
        assert!(canonical.contains("\"1,0\""));
        assert!(!canonical.contains("\"-1,-1\""));
    }

    #[test]
    fn tilemap_mistakes_are_located_with_a_hint() {
        let rows = [[EMPTY; 16]; 16];
        let good = map_with(&rows);
        let cases = [
            (
                good.replace("\"0,0\"", "\"00,0\""),
                "tilemap_invalid",
                "/layers/0/chunks/00,0",
            ),
            (
                good.replace("\"0,0\"", "\"9000,0\""),
                "tilemap_invalid",
                "/layers/0/chunks/9000,0",
            ),
            (
                good.replacen("[-1, ", "[-2, ", 1),
                "value_out_of_range",
                "/layers/0/chunks/0,0/0/0",
            ),
            (
                good.replacen("[-1, ", "[", 1),
                "wrong_length",
                "/layers/0/chunks/0,0/0",
            ),
            (
                good.replace("\"name\": \"walls\"", "\"name\": \"\""),
                "tilemap_invalid",
                "/layers/0/name",
            ),
            (
                good.replace("tiles/station.tileset.json", "tiles/a.png"),
                "not_a_tileset_file",
                "/tileset",
            ),
            (
                good.replace("\"walls\",", "\"walls\", \"colide\": false,"),
                "unknown_key",
                "/layers/0/colide",
            ),
        ];
        for (text, code, pointer) in cases {
            let problem = parse_tilemap(&text, "m.tilemap.json").unwrap_err();
            assert_eq!(problem.code, code, "{text}");
            assert_eq!(problem.location.path.as_deref(), Some(pointer), "{text}");
            assert!(problem.location.line.is_some(), "{text}");
            assert!(!problem.hint.is_empty());
        }
        let twice = r#"{ "format": 1, "tileset": "t.tileset.json", "layers": [{ "name": "a" }, { "name": "a" }] }"#;
        assert_eq!(
            parse_tilemap(twice, "m").unwrap_err().code,
            "tilemap_invalid"
        );
        let none = r#"{ "tileset": "t.tileset.json" }"#;
        assert_eq!(parse_tilemap(none, "m").unwrap_err().code, "missing_format");
    }

    #[test]
    fn a_spec_takes_layer_names_and_points_into_it() {
        let Value::Object(spec) = serde_json::json!({
            "tileset": "tiles/station.tileset.json",
            "layers": ["floor", { "name": "walls", "collide": false }]
        }) else {
            unreachable!()
        };
        let map = Tilemap::from_spec(spec).unwrap();
        assert_eq!(map.layer_names(), ["floor", "walls"]);
        assert!(!map.layers[1].collide);
        let text = map.to_canonical().unwrap();
        assert!(text.contains("\"collide\": false"));
        assert_eq!(parse_tilemap(&text, "m").unwrap(), map);
        let Value::Object(bad) = serde_json::json!({ "tileset": "x.png" }) else {
            unreachable!()
        };
        let problem = Tilemap::from_spec(bad).unwrap_err();
        assert_eq!(problem.location.path.as_deref(), Some("/tilemap/tileset"));
    }

    fn tileset() -> Tileset {
        parse_tileset(TILESET, "t").unwrap().decode().unwrap()
    }

    #[test]
    fn solid_cells_merge_into_few_boxes_in_a_fixed_order() {
        let mut map = Tilemap::new("t.tileset.json");
        map.layers.push(TileLayer::new("walls"));
        // A room's walls: 10 wide, 6 tall, a door of 2 in the bottom wall.
        for x in 0..10 {
            if !(4..6).contains(&x) {
                map.set(0, x, 0, 1).unwrap();
            }
            map.set(0, x, 5, 1).unwrap();
        }
        for y in 1..5 {
            map.set(0, 0, y, 1).unwrap();
            map.set(0, 9, y, 1).unwrap();
        }
        // A floor tile (not solid) and a slope.
        map.set(0, 3, 3, 0).unwrap();
        map.set(0, 5, 3, 2).unwrap();
        let shapes = collision_shapes(&map, &tileset());
        assert_eq!(
            shapes.boxes,
            [
                [0, 0, 4, 1],
                [6, 0, 10, 1],
                [0, 1, 1, 6],
                [9, 1, 10, 6],
                [1, 5, 9, 6]
            ]
        );
        assert_eq!(shapes.outlines.len(), 1);
        assert_eq!(shapes.outlines[0].0, [5, 3]);
        // The same map built in another order gives the same boxes.
        let mut other = Tilemap::new("t.tileset.json");
        other.layers.push(TileLayer::new("walls"));
        let mut cells: Vec<(i32, i32, i32)> = map.layers[0].cells().collect();
        cells.reverse();
        for (x, y, tile) in cells {
            other.set(0, x, y, tile).unwrap();
        }
        assert_eq!(collision_shapes(&other, &tileset()), shapes);
        // A layer that does not collide adds nothing.
        other.layers[0].collide = false;
        assert_eq!(collision_shapes(&other, &tileset()), TileShapes::default());
    }

    #[test]
    fn a_block_is_one_box_and_an_l_is_two() {
        let mut map = Tilemap::new("t.tileset.json");
        map.layers.push(TileLayer::new("walls"));
        for x in 0..20 {
            for y in 0..20 {
                map.set(0, x, y, 1).unwrap();
            }
        }
        assert_eq!(collision_shapes(&map, &tileset()).boxes, [[0, 0, 20, 20]]);
        for x in 5..20 {
            for y in 5..20 {
                map.set(0, x, y, EMPTY).unwrap();
            }
        }
        assert_eq!(
            collision_shapes(&map, &tileset()).boxes,
            [[0, 0, 20, 5], [0, 5, 5, 20]]
        );
    }

    #[test]
    fn only_a_tilemap_component_s_map_has_a_kind() {
        let at = "/entities/a/components/Tilemap/map";
        assert!(kind_problem(at, "maps/a.tilemap.json").is_none());
        assert_eq!(
            kind_problem(at, "a.png").unwrap().code,
            "not_a_tilemap_file"
        );
        assert!(kind_problem("/entities/a/components/Sprite/image", "a.png").is_none());
    }

    #[test]
    fn references_are_found_for_moves() {
        let mut tileset: Value = serde_json::from_str(TILESET).unwrap();
        assert_eq!(refs_in_tileset(&tileset)[0].reference, "tiles/station.png");
        visit_tileset(&mut tileset, &mut |_, reference| {
            *reference = "moved.png".into()
        });
        assert_eq!(tileset["image"], "moved.png");
        let map: Value = serde_json::json!({ "format": 1, "tileset": "t.tileset.json" });
        assert_eq!(refs_in_tilemap(&map)[0].pointer, "/tileset");
    }
}
