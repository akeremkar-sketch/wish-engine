//! Tilemap files as the engine edits them (design note
//! `docs/design/1.1-tilemaps.md` §5): painting operations, one history per
//! map file with the actor of every change, undo, redo and taking one
//! change back; and what sessions need of the project's maps (their walls).
//!
//! As with scenes (`documents`), the file is the source of truth: it is
//! read again before every use; a change made outside the engine enters
//! the history as one change by [`Actor::File`]; a file a person broke by
//! hand is never overwritten (`tilemap_file_invalid`); every change is
//! written in canonical form, atomically, before the call answers.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use wish_asset::tiles::{EMPTY, MAX_CELL, MAX_LAYERS, MAX_NAME, MIN_CELL, TileLayer};
use wish_asset::{TILEMAP_SUFFIX, Tilemap};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::render::Stamp;
use crate::workspace::Workspace;

/// The most cells one call changes, and the most a fill reaches.
pub const MAX_PAINT_CELLS: usize = 65_536;
/// The most history entries kept per map (the oldest go first).
pub const MAX_HISTORY: usize = 200;
/// The most log entries kept per map.
const MAX_LOG: usize = 1000;
/// The widest rectangle `get` returns, in cells.
pub const MAX_GET_SIDE: i64 = 64;
/// The most tilemaps read for their walls when a session starts.
const MAX_TILEMAPS: usize = 2000;

/// What kind of change a history entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Edit,
    Undo,
    Redo,
    Revert,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Revert => "revert",
        }
    }
}

/// A change that can be undone: the map before and after (chunks are
/// shared between the two and with the map, so it costs what it changed).
#[derive(Clone, Debug)]
struct Entry {
    revision: u64,
    actor: Actor,
    before: Arc<Tilemap>,
    after: Arc<Tilemap>,
    /// For a revert: the revision it took back.
    reverts: Option<u64>,
    /// Taken back by a revert that is still in force.
    reverted: bool,
}

/// One line of a map's history, newest last.
#[derive(Clone, Debug)]
struct LogEntry {
    revision: u64,
    actor: Actor,
    action: Action,
    reason: Option<String>,
    cells: usize,
    layers: Vec<String>,
    reverts: Option<u64>,
}

/// A tilemap file the engine has open.
#[derive(Debug)]
pub(crate) struct OpenMap {
    map: Arc<Tilemap>,
    /// The file's text as last read or written: what "unchanged" means.
    disk_text: String,
    /// Set while the file on disk is invalid: changes are refused.
    disk_problem: Option<Diagnostic>,
    revision: u64,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    log: VecDeque<LogEntry>,
}

/// What is known of the walls of one of the project's tilemaps, kept until
/// one of its files changes.
#[derive(Clone, Debug)]
struct KnownShapes {
    stamps: Vec<(String, Stamp)>,
    shapes: wish_physics::MapShapes,
}

/// The open tilemaps and the walls of the project's maps.
#[derive(Debug, Default)]
pub(crate) struct Tilemaps {
    open: BTreeMap<String, OpenMap>,
    shapes: BTreeMap<String, KnownShapes>,
}

fn stamp_of(file: &std::path::Path) -> Stamp {
    let meta = std::fs::metadata(file).ok();
    (
        meta.as_ref().map_or(0, std::fs::Metadata::len),
        meta.and_then(|m| m.modified().ok()),
    )
}

/// The cells that differ between two maps, layer by layer (by name): the
/// layer, the cell, the tile before and after. Chunks the two share are
/// skipped without looking.
fn cell_changes(before: &Tilemap, after: &Tilemap) -> Vec<(String, i32, i32, i32, i32)> {
    let mut found = Vec::new();
    let empty = TileLayer::new("");
    let names: BTreeSet<&str> = before
        .layers
        .iter()
        .chain(&after.layers)
        .map(|layer| layer.name.as_str())
        .collect();
    for name in names {
        let old = before
            .layer_index(name)
            .map_or(&empty, |index| &before.layers[index]);
        let new = after
            .layer_index(name)
            .map_or(&empty, |index| &after.layers[index]);
        let keys: BTreeSet<(i32, i32)> = old
            .chunks
            .keys()
            .chain(new.chunks.keys())
            .copied()
            .collect();
        for key in keys {
            let (a, b) = (old.chunks.get(&key), new.chunks.get(&key));
            if let (Some(a), Some(b)) = (a, b)
                && Arc::ptr_eq(a, b)
            {
                continue;
            }
            for index in 0..wish_asset::tiles::CHUNK_CELLS {
                let was = a.map_or(EMPTY, |chunk| chunk[index]);
                let is = b.map_or(EMPTY, |chunk| chunk[index]);
                if was != is {
                    let index = index as i32;
                    let x = key.0 * wish_asset::tiles::CHUNK + index % wish_asset::tiles::CHUNK;
                    let y = key.1 * wish_asset::tiles::CHUNK + index / wish_asset::tiles::CHUNK;
                    found.push((name.to_owned(), x, y, was, is));
                }
            }
        }
    }
    found
}

/// Whether two maps differ in their layers (names, order, collide) or
/// tileset, not only in tiles.
fn structure_differs(a: &Tilemap, b: &Tilemap) -> bool {
    a.tileset != b.tileset
        || a.layers.len() != b.layers.len()
        || a.layers
            .iter()
            .zip(&b.layers)
            .any(|(x, y)| x.name != y.name || x.collide != y.collide)
}

/// The layers a change touched (cells, or the layer itself), in order.
fn touched_layers(
    before: &Tilemap,
    after: &Tilemap,
    cells: &[(String, i32, i32, i32, i32)],
) -> Vec<String> {
    let mut layers: BTreeSet<String> = cells.iter().map(|(layer, ..)| layer.clone()).collect();
    for layer in before.layers.iter().chain(&after.layers) {
        let there = |map: &Tilemap| {
            map.layer_index(&layer.name)
                .map(|index| (map.layers[index].collide, index))
        };
        if there(before) != there(after) {
            layers.insert(layer.name.clone());
        }
    }
    layers.into_iter().collect()
}

/// A tilemap path as the tools write it: project-relative, inside the
/// project, spelled as on disk, ending in `.tilemap.json`.
pub(crate) fn normalize_map_path(root: &std::path::Path, path: &str, at: &str) -> Result<String> {
    let path = crate::documents::normalize_asset_output(path, &["tilemap.json"])
        .map_err(|problem| problem.at_path(at.to_owned()))?;
    if !path.ends_with(TILEMAP_SUFFIX) {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!("'{path}' is not a tilemap file: its name must end in {TILEMAP_SUFFIX}."),
            "Name a tilemap such as \"maps/station.tilemap.json\" (asset_list kind tilemap lists them).",
        )
        .at_path(at.to_owned()));
    }
    crate::documents::inside_project(root, &path).map_err(|p| p.at_path(at.to_owned()))?;
    if let Some(spelled) = crate::documents::other_case(root, &path) {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!("'{path}' is spelled '{spelled}' on disk."),
            "Use the spelling on disk: names that differ only in letter case work on Windows but not on Linux.",
        )
        .at_path(at.to_owned()));
    }
    Ok(path)
}

impl Workspace {
    /// The tilemap `path`, opened (or brought up to date with its file: a
    /// change made outside the engine enters its history as one change by
    /// `file`). A file that does not read leaves the last good map in place
    /// and marks it (`disk_problem`); one never read well is an error.
    pub(crate) fn open_map(&mut self, path: &str) -> Result<&mut OpenMap> {
        let file = self.root().join(path);
        let text = match wish_asset::read_tilemap_file(&file, path) {
            Ok(text) => text,
            Err(problem) => {
                if !file.exists() {
                    self.tilemaps.open.remove(path);
                    return Err(Diagnostic::error(
                        "tilemap_not_found",
                        format!("There is no tilemap '{path}'."),
                        "asset_list with kind tilemap lists the project's maps; asset_create with a .tilemap.json path makes one.",
                    )
                    .at_path("/tilemap"));
                }
                return Err(problem);
            }
        };
        let parsed = wish_asset::parse_tilemap(&text, path);
        match self.tilemaps.open.get_mut(path) {
            None => {
                let map = parsed?;
                self.tilemaps.open.insert(
                    path.to_owned(),
                    OpenMap {
                        map: Arc::new(map),
                        disk_text: text,
                        disk_problem: None,
                        revision: 0,
                        undo: VecDeque::new(),
                        redo: Vec::new(),
                        log: VecDeque::new(),
                    },
                );
            }
            Some(open) if open.disk_text != text => match parsed {
                Ok(map) => {
                    open.disk_text = text;
                    open.disk_problem = None;
                    if *open.map != map {
                        let after = Arc::new(map);
                        let before = Arc::clone(&open.map);
                        open.record(Actor::File, Action::Edit, before, after, None, None);
                    }
                }
                Err(problem) => {
                    open.disk_problem = Some(problem);
                }
            },
            Some(_) => {}
        }
        self.tilemaps.open.get_mut(path).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                "A tilemap was lost while it was opened.",
                "This is a bug in the engine. Please report it.",
            )
        })
    }

    /// Writes `after` as the map `path`, and on success records the change.
    fn commit_map(
        &mut self,
        path: &str,
        after: Tilemap,
        (actor, action, reason, reverts): (Actor, Action, Option<String>, Option<u64>),
    ) -> Result<u64> {
        let text = after.to_canonical()?;
        crate::documents::write_project_file(self.root(), path, &text)?;
        let open = self.opened(path)?;
        open.disk_text = text;
        let before = Arc::clone(&open.map);
        open.record(actor, action, before, Arc::new(after), reason, reverts);
        Ok(open.revision)
    }

    /// The tilemap `path` as it is open, without reading its file again
    /// (right after the engine wrote it).
    fn opened(&mut self, path: &str) -> Result<&mut OpenMap> {
        self.tilemaps.open.get_mut(path).ok_or_else(|| {
            Diagnostic::error(
                "internal_error",
                "A tilemap was lost while it was written.",
                "This is a bug in the engine. Please report it.",
            )
        })
    }

    /// The walls of the project's tilemaps by file (each map's solid tiles
    /// merged into boxes with its tileset's properties), for a session. A
    /// map or tileset that cannot be read has none (`check` says why). Kept
    /// until one of a map's files changes; files are read with their caps.
    pub(crate) fn tile_shapes(&mut self) -> Vec<(String, wish_physics::MapShapes)> {
        let root = self.root().to_path_buf();
        let Ok(index) = wish_asset::AssetIndex::scan(&root) else {
            return Vec::new();
        };
        let paths: Vec<String> = index
            .paths()
            .filter(|path| wish_asset::is_tilemap(path))
            .take(MAX_TILEMAPS)
            .map(str::to_owned)
            .collect();
        let mut found = Vec::new();
        for path in paths {
            let fresh = self.tilemaps.shapes.get(&path).filter(|known| {
                known
                    .stamps
                    .iter()
                    .all(|(file, stamp)| stamp_of(&root.join(file)) == *stamp)
            });
            let shapes = match fresh {
                Some(known) => known.shapes.clone(),
                None => {
                    let mut stamps = vec![(path.clone(), stamp_of(&root.join(&path)))];
                    let shapes = wish_asset::read_tilemap(&root, &path)
                        .and_then(|map| {
                            stamps.push((map.tileset.clone(), stamp_of(&root.join(&map.tileset))));
                            let tileset = wish_asset::read_tileset(&root, &map.tileset)?;
                            Ok(wish_asset::collision_shapes(&map, &tileset))
                        })
                        .map(|shapes| wish_physics::MapShapes {
                            boxes: shapes.boxes,
                            outlines: shapes.outlines,
                        })
                        .unwrap_or_default();
                    self.tilemaps.shapes.insert(
                        path.clone(),
                        KnownShapes {
                            stamps,
                            shapes: shapes.clone(),
                        },
                    );
                    shapes
                }
            };
            if !shapes.is_empty() {
                found.push((path, shapes));
            }
        }
        found
    }

    /// Follows tilemaps that moved (`asset_move`): their open documents and
    /// histories go with them, and a map whose file the move rewrote (the
    /// tileset it names moved) records that as a change by `actor`.
    pub(crate) fn tilemaps_moved(&mut self, from: &str, to: &str, actor: &Actor) {
        let moved: Vec<String> = self
            .tilemaps
            .open
            .keys()
            .filter(|path| *path == from || path.starts_with(&format!("{from}/")))
            .cloned()
            .collect();
        for path in moved {
            if let Some(open) = self.tilemaps.open.remove(&path) {
                let rest = &path[from.len()..];
                self.tilemaps.open.insert(format!("{to}{rest}"), open);
            }
        }
        let root = self.root().to_path_buf();
        for (path, open) in &mut self.tilemaps.open {
            let Ok(text) = wish_asset::read_tilemap_file(&root.join(path.as_str()), path) else {
                continue;
            };
            if text == open.disk_text {
                continue;
            }
            if let Ok(map) = wish_asset::parse_tilemap(&text, path) {
                open.disk_text = text;
                let before = Arc::clone(&open.map);
                let reason = Some(format!("{from} moved to {to}"));
                open.record(
                    actor.clone(),
                    Action::Edit,
                    before,
                    Arc::new(map),
                    reason,
                    None,
                );
            }
        }
    }
}

impl OpenMap {
    /// Makes `after` the map, as a change by `actor`.
    fn record(
        &mut self,
        actor: Actor,
        action: Action,
        before: Arc<Tilemap>,
        after: Arc<Tilemap>,
        reason: Option<String>,
        reverts: Option<u64>,
    ) {
        self.revision += 1;
        let cells = cell_changes(&before, &after);
        self.log.push_back(LogEntry {
            revision: self.revision,
            actor: actor.clone(),
            action,
            reason,
            cells: cells.len(),
            layers: touched_layers(&before, &after, &cells),
            reverts,
        });
        while self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
        if matches!(action, Action::Edit | Action::Revert) {
            self.redo.clear();
            self.undo.push_back(Entry {
                revision: self.revision,
                actor,
                before,
                after: Arc::clone(&after),
                reverts,
                reverted: false,
            });
            while self.undo.len() > MAX_HISTORY {
                self.undo.pop_front();
            }
        }
        self.map = after;
    }

    /// Whether the change of `revision` can be taken back on its own now.
    fn revertible(&self, revision: u64) -> Result<&Entry> {
        let Some(entry) = self.undo.iter().find(|entry| entry.revision == revision) else {
            let known = self.log.iter().any(|log| log.revision == revision);
            return Err(Diagnostic::error(
                "not_revertible",
                format!("The change of revision {revision} cannot be taken back on its own."),
                if known {
                    "It was undone already, or was itself an undo or redo. Undo, or paint by hand instead."
                } else {
                    "No change has that revision in this tilemap's history; edit_history lists the revisions."
                },
            ));
        };
        if entry.reverted {
            return Err(Diagnostic::error(
                "already_reverted",
                format!("The change of revision {revision} was taken back already."),
                "edit_history lists the revert; undo it to bring the change back.",
            ));
        }
        if structure_differs(&entry.before, &entry.after) {
            return Err(Diagnostic::error(
                "not_revertible",
                format!(
                    "The change of revision {revision} added, removed or changed a layer: it is taken back only by undo."
                ),
                "Undo (newest first), or change the layers again with tilemap_edit.",
            ));
        }
        let moved: Vec<(String, i32, i32)> = cell_changes(&entry.before, &entry.after)
            .into_iter()
            .filter(|(layer, x, y, _, is)| {
                let now = self
                    .map
                    .layer_index(layer)
                    .map(|index| self.map.get(index, *x, *y));
                now != Some(*is)
            })
            .map(|(layer, x, y, ..)| (layer, x, y))
            .collect();
        if !moved.is_empty() {
            let later: Vec<String> = self
                .log
                .iter()
                .filter(|log| log.revision > revision && log.cells > 0)
                .map(|log| format!("r{} by {}", log.revision, log.actor.label()))
                .collect();
            let (layer, x, y) = &moved[0];
            return Err(Diagnostic::error(
                "revert_conflict",
                format!(
                    "Later changes painted over what revision {revision} painted ({} cells, the first ({x}, {y}) on '{layer}'): {}.",
                    moved.len(),
                    if later.is_empty() {
                        "a change no longer in the history".to_owned()
                    } else {
                        later.join(", ")
                    }
                ),
                "Take those back first (newest first), or paint by hand: reverting now would undo part of them.",
            ));
        }
        Ok(entry)
    }
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

fn op_error(
    index: usize,
    field: &str,
    code: &'static str,
    message: String,
    hint: &str,
) -> Diagnostic {
    let pointer = if field.is_empty() {
        format!("/ops/{index}")
    } else {
        format!("/ops/{index}/{field}")
    };
    Diagnostic::error(code, message, hint).at_path(pointer)
}

/// `[x, y]` whole numbers within the map's cells.
fn cell_of(op: &Map<String, Value>, field: &str, index: usize) -> Result<(i32, i32)> {
    let pair = op
        .get(field)
        .and_then(Value::as_array)
        .filter(|p| p.len() == 2);
    let numbers = pair.and_then(|p| Some((p[0].as_i64()?, p[1].as_i64()?)));
    match numbers {
        Some((x, y))
            if (i64::from(MIN_CELL)..=i64::from(MAX_CELL)).contains(&x)
                && (i64::from(MIN_CELL)..=i64::from(MAX_CELL)).contains(&y) =>
        {
            Ok((x as i32, y as i32))
        }
        _ => Err(op_error(
            index,
            field,
            "invalid_parameter",
            format!(
                "\"{field}\" must be a cell [x, y]: two whole numbers from {MIN_CELL} to {MAX_CELL}."
            ),
            "Cells are whole numbers, x to the right and y up from the map's origin, such as [3, 4].",
        )),
    }
}

/// A tile number (or -1 to erase) at `field`.
fn tile_of(op: &Map<String, Value>, field: &str, index: usize) -> Result<i32> {
    match op.get(field).and_then(Value::as_i64) {
        Some(tile) if (-1..=i64::from(wish_asset::tiles::MAX_TILE)).contains(&tile) => {
            Ok(tile as i32)
        }
        _ => Err(op_error(
            index,
            field,
            "invalid_parameter",
            format!(
                "\"{field}\" must be a tile number from 0 to {}, or -1 to erase.",
                wish_asset::tiles::MAX_TILE
            ),
            "Tiles are numbered from 0 in their tileset (asset_info on the tileset lists them).",
        )),
    }
}

/// The layer an operation names, by name, in `map`.
fn layer_of(map: &Tilemap, op: &Map<String, Value>, index: usize) -> Result<usize> {
    let name = op.get("layer").and_then(Value::as_str).ok_or_else(|| {
        op_error(
            index,
            "layer",
            "invalid_parameter",
            "The operation names no layer (\"layer\": its name).".to_owned(),
            &format!("The map's layers are: {}.", map.layer_names().join(", ")),
        )
    })?;
    map.layer_index(name).ok_or_else(|| {
        let names = map.layer_names();
        op_error(
            index,
            "layer",
            "layer_not_found",
            format!("The tilemap has no layer '{name}'."),
            &if names.is_empty() {
                "The map has no layers: add one first ({\"op\": \"add_layer\", \"layer\": \"floor\"}).".to_owned()
            } else {
                format!(
                    "{} The map's layers are: {}.",
                    wish_core::suggest::unknown_name_hint(name, &names),
                    names.join(", ")
                )
            },
        )
    })
}

/// What a paint call did, kept while it is applied: the layers it touched
/// (cells changed on the way, per layer name) and how many changes it
/// made, to stop at [`MAX_PAINT_CELLS`].
#[derive(Default)]
struct Painted {
    changed: BTreeMap<String, usize>,
    total: usize,
}

impl Painted {
    fn note(&mut self, layer: &str) -> Result<()> {
        *self.changed.entry(layer.to_owned()).or_default() += 1;
        self.total += 1;
        if self.total > MAX_PAINT_CELLS {
            return Err(Diagnostic::error(
                "paint_too_large",
                format!("The call changes more than {MAX_PAINT_CELLS} cells."),
                "Paint in several calls (each is its own step in the history), or use smaller rectangles.",
            )
            .at_path("/ops"));
        }
        Ok(())
    }
}

/// Puts `tile` into one cell, checked against the tileset's size.
fn put(
    map: &mut Tilemap,
    painted: &mut Painted,
    (layer, x, y, tile): (usize, i32, i32, i32),
    tiles: Option<usize>,
    index: usize,
) -> Result<()> {
    if let Some(count) = tiles
        && tile >= 0
        && tile as usize >= count
    {
        return Err(op_error(
            index,
            "",
            "unknown_tile",
            format!(
                "Tile {tile} is not in the tileset '{}', which has {count} tiles (0 to {}).",
                map.tileset,
                count.saturating_sub(1)
            ),
            "Use a tile the tileset has (asset_info on it lists them), or -1 to erase.",
        ));
    }
    let name = map.layers[layer].name.clone();
    if map.set(layer, x, y, tile).map_err(|p| {
        let mut p = p;
        p.location.path = Some(format!("/ops/{index}"));
        p
    })? {
        painted.note(&name)?;
    }
    Ok(())
}

/// The cells a rectangle between two corners covers (both included),
/// bounded.
fn rect_cells(from: (i32, i32), to: (i32, i32), index: usize) -> Result<Vec<(i32, i32)>> {
    let (x0, x1) = (from.0.min(to.0), from.0.max(to.0));
    let (y0, y1) = (from.1.min(to.1), from.1.max(to.1));
    let area = (i64::from(x1) - i64::from(x0) + 1) * (i64::from(y1) - i64::from(y0) + 1);
    if area > MAX_PAINT_CELLS as i64 {
        return Err(op_error(
            index,
            "",
            "paint_too_large",
            format!(
                "The rectangle covers {area} cells; one call changes at most {MAX_PAINT_CELLS}."
            ),
            "Paint it in several smaller rectangles.",
        ));
    }
    Ok((y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .collect())
}

/// The 4-connected cells of `layer` that hold what `at` holds, within the
/// box of every tile of the map; refused when `at` is outside that box or
/// the region is larger than [`MAX_PAINT_CELLS`].
fn fill_cells(
    map: &Tilemap,
    layer: usize,
    at: (i32, i32),
    index: usize,
) -> Result<Vec<(i32, i32)>> {
    let Some([bx0, by0, bx1, by1]) = map.bounds() else {
        return Err(op_error(
            index,
            "at",
            "fill_unbounded",
            "The map is empty: a fill has nothing to stop it.".to_owned(),
            "Paint the edges first (rect, set), or fill a rectangle with rect.",
        ));
    };
    if at.0 < bx0 || at.0 > bx1 || at.1 < by0 || at.1 > by1 {
        return Err(op_error(
            index,
            "at",
            "fill_unbounded",
            format!(
                "Cell ({}, {}) is outside the painted part of the map (cells {bx0}..{bx1} across, {by0}..{by1} up): a fill there would not stop.",
                at.0, at.1
            ),
            "Fill inside the map's painted part, or use rect for a rectangle.",
        ));
    }
    let target = map.get(layer, at.0, at.1);
    let mut seen: BTreeSet<(i32, i32)> = BTreeSet::from([at]);
    let mut queue = VecDeque::from([at]);
    while let Some((x, y)) = queue.pop_front() {
        for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if next.0 < bx0 || next.0 > bx1 || next.1 < by0 || next.1 > by1 {
                continue;
            }
            if map.get(layer, next.0, next.1) == target && seen.insert(next) {
                if seen.len() > MAX_PAINT_CELLS {
                    return Err(op_error(
                        index,
                        "at",
                        "fill_too_large",
                        format!("The region to fill has more than {MAX_PAINT_CELLS} cells."),
                        "Fill a smaller region (close it with walls first), or paint rectangles.",
                    ));
                }
                queue.push_back(next);
            }
        }
    }
    Ok(seen.into_iter().collect())
}

/// Applies the operations `ops` to `map`, in order; what changed.
fn apply_ops(map: &mut Tilemap, ops: &[Value], tiles: Option<usize>) -> Result<Painted> {
    let mut painted = Painted::default();
    for (index, op) in ops.iter().enumerate() {
        let Some(op) = op.as_object() else {
            return Err(op_error(
                index,
                "",
                "invalid_parameter",
                format!("Operation {index} must be an object."),
                "Write it like { \"op\": \"rect\", \"layer\": \"walls\", \"from\": [0, 0], \"to\": [9, 0], \"tile\": 8 }.",
            ));
        };
        let kind = op.get("op").and_then(Value::as_str).unwrap_or("");
        const KINDS: [&str; 7] = [
            "set",
            "rect",
            "fill",
            "erase",
            "add_layer",
            "remove_layer",
            "set_layer",
        ];
        match kind {
            "set" => {
                let layer = layer_of(map, op, index)?;
                let cells = op.get("cells").and_then(Value::as_array).ok_or_else(|| {
                    op_error(
                        index,
                        "cells",
                        "invalid_parameter",
                        "\"set\" needs \"cells\": a list of [x, y, tile].".to_owned(),
                        "Write it like \"cells\": [[3, 4, 1], [4, 4, 1]] (-1 as the tile erases).",
                    )
                })?;
                if cells.len() > MAX_PAINT_CELLS {
                    return Err(op_error(
                        index,
                        "cells",
                        "paint_too_large",
                        format!(
                            "{} cells; one call changes at most {MAX_PAINT_CELLS}.",
                            cells.len()
                        ),
                        "Paint in several calls.",
                    ));
                }
                for (number, cell) in cells.iter().enumerate() {
                    let triple = cell
                        .as_array()
                        .filter(|c| c.len() == 3)
                        .and_then(|c| Some((c[0].as_i64()?, c[1].as_i64()?, c[2].as_i64()?)));
                    let Some((x, y, tile)) = triple.filter(|(x, y, tile)| {
                        let range = i64::from(MIN_CELL)..=i64::from(MAX_CELL);
                        range.contains(x)
                            && range.contains(y)
                            && (-1..=i64::from(wish_asset::tiles::MAX_TILE)).contains(tile)
                    }) else {
                        return Err(op_error(
                            index,
                            &format!("cells/{number}"),
                            "invalid_parameter",
                            format!(
                                "Cell {number} must be [x, y, tile]: whole numbers, the tile from 0 to {} or -1 to erase.",
                                wish_asset::tiles::MAX_TILE
                            ),
                            "Write it like [3, 4, 1].",
                        ));
                    };
                    put(
                        map,
                        &mut painted,
                        (layer, x as i32, y as i32, tile as i32),
                        tiles,
                        index,
                    )?;
                }
            }
            "rect" | "erase" => {
                let layer = layer_of(map, op, index)?;
                let from = cell_of(op, "from", index)?;
                let to = cell_of(op, "to", index)?;
                let tile = if kind == "erase" {
                    EMPTY
                } else {
                    tile_of(op, "tile", index)?
                };
                for (x, y) in rect_cells(from, to, index)? {
                    put(map, &mut painted, (layer, x, y, tile), tiles, index)?;
                }
            }
            "fill" => {
                let layer = layer_of(map, op, index)?;
                let at = cell_of(op, "at", index)?;
                let tile = tile_of(op, "tile", index)?;
                for (x, y) in fill_cells(map, layer, at, index)? {
                    put(map, &mut painted, (layer, x, y, tile), tiles, index)?;
                }
            }
            "add_layer" => {
                let name = op.get("layer").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control) {
                    return Err(op_error(
                        index,
                        "layer",
                        "invalid_parameter",
                        format!("'{name}' cannot be a layer's name."),
                        &format!(
                            "A layer's name is 1 to {MAX_NAME} bytes of text, such as \"walls\"."
                        ),
                    ));
                }
                if map.layer_index(name).is_some() {
                    return Err(op_error(
                        index,
                        "layer",
                        "invalid_parameter",
                        format!("The map has a layer '{name}' already."),
                        "Give the new layer a name of its own.",
                    ));
                }
                if map.layers.len() >= MAX_LAYERS {
                    return Err(op_error(
                        index,
                        "",
                        "tilemap_too_large",
                        format!("A tilemap has at most {MAX_LAYERS} layers."),
                        "Put tiles that never overlap on one layer, or use another tilemap.",
                    ));
                }
                let mut layer = TileLayer::new(name);
                if let Some(collide) = op.get("collide") {
                    layer.collide = collide.as_bool().ok_or_else(|| {
                        op_error(
                            index,
                            "collide",
                            "invalid_parameter",
                            "\"collide\" must be true or false.".to_owned(),
                            "false makes the layer's solid tiles pass-through.",
                        )
                    })?;
                }
                let at = match op.get("before").and_then(Value::as_str) {
                    Some(before) => {
                        let mut probe = op.clone();
                        probe.insert("layer".to_owned(), json!(before));
                        layer_of(map, &probe, index).map_err(|mut p| {
                            p.location.path = Some(format!("/ops/{index}/before"));
                            p
                        })?
                    }
                    None => map.layers.len(),
                };
                map.layers.insert(at, layer);
                painted.changed.entry(name.to_owned()).or_default();
            }
            "remove_layer" => {
                let layer = layer_of(map, op, index)?;
                let removed = map.layers.remove(layer);
                let cells = removed.tile_count();
                *painted.changed.entry(removed.name.clone()).or_default() += cells;
                painted.total += cells;
            }
            "set_layer" => {
                let layer = layer_of(map, op, index)?;
                if let Some(collide) = op.get("collide") {
                    map.layers[layer].collide = collide.as_bool().ok_or_else(|| {
                        op_error(
                            index,
                            "collide",
                            "invalid_parameter",
                            "\"collide\" must be true or false.".to_owned(),
                            "false makes the layer's solid tiles pass-through.",
                        )
                    })?;
                }
                if let Some(name) = op.get("name") {
                    let name = name.as_str().unwrap_or("");
                    if name.is_empty()
                        || name.len() > MAX_NAME
                        || name.chars().any(char::is_control)
                    {
                        return Err(op_error(
                            index,
                            "name",
                            "invalid_parameter",
                            format!("'{name}' cannot be a layer's name."),
                            &format!("A layer's name is 1 to {MAX_NAME} bytes of text."),
                        ));
                    }
                    if map.layer_index(name).is_some_and(|other| other != layer) {
                        return Err(op_error(
                            index,
                            "name",
                            "invalid_parameter",
                            format!("The map has a layer '{name}' already."),
                            "Give the layer a name of its own.",
                        ));
                    }
                    map.layers[layer].name = name.to_owned();
                }
                let name = map.layers[layer].name.clone();
                painted.changed.entry(name).or_default();
            }
            other => {
                return Err(op_error(
                    index,
                    "op",
                    "invalid_parameter",
                    format!("'{other}' is not a tilemap operation."),
                    &format!(
                        "{} The operations are: {}.",
                        wish_core::suggest::unknown_name_hint(other, &KINDS),
                        KINDS.join(", ")
                    ),
                ));
            }
        }
    }
    Ok(painted)
}

/// How many tiles the tileset of `map` has, when it can be read.
fn tileset_size(workspace: &Workspace, map: &Tilemap) -> Option<usize> {
    wish_asset::tiles::tile_count(workspace.root(), &map.tileset).ok()
}

/// `tilemap_edit` paint: the operations, all or nothing, as one change.
pub(crate) fn paint(
    workspace: &mut Workspace,
    path: &str,
    ops: &[Value],
    (actor, reason): (&Actor, Option<String>),
    (dry_run, base): (bool, Option<u64>),
) -> Result<Map<String, Value>> {
    let open = workspace.open_map(path)?;
    if let Some(problem) = &open.disk_problem {
        return Err(file_invalid(path, problem));
    }
    if let Some(base) = base
        && base != open.revision
    {
        return Err(Diagnostic::error(
            "stale_revision",
            format!(
                "The changes were prepared against revision {base}, but the tilemap is now at revision {}.",
                open.revision
            ),
            "Someone else changed the map in the meantime. Read it again (tilemap_edit action get) and paint on top of the current revision.",
        )
        .at_path("/base_revision"));
    }
    if ops.is_empty() {
        return Err(Diagnostic::error(
            "invalid_parameter",
            "There are no operations to apply.",
            "Give \"ops\": a list such as [{ \"op\": \"rect\", \"layer\": \"walls\", \"from\": [0, 0], \"to\": [9, 0], \"tile\": 8 }].",
        )
        .at_path("/ops"));
    }
    let original = Arc::clone(&open.map);
    let mut working: Tilemap = (*open.map).clone();
    let revision = open.revision;
    let tiles = tileset_size(workspace, &working);
    let painted = apply_ops(&mut working, ops, tiles)?;
    // What the call changed in the end (a cell painted and erased again
    // did not change), per layer as it is named after the call.
    let net = cell_changes(&original, &working);
    let mut changed: BTreeMap<String, usize> = painted
        .changed
        .keys()
        .filter(|layer| working.layer_index(layer).is_some())
        .map(|layer| (layer.clone(), 0))
        .collect();
    let mut bounds: Option<[i32; 4]> = None;
    for (layer, x, y, ..) in &net {
        *changed.entry(layer.clone()).or_default() += 1;
        bounds = Some(match bounds {
            None => [*x, *y, *x, *y],
            Some([x0, y0, x1, y1]) => [x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)],
        });
    }
    let mut out = Map::new();
    out.insert("tilemap".to_owned(), json!(path));
    out.insert(
        "changed".to_owned(),
        Value::Object(
            changed
                .iter()
                .map(|(layer, count)| (layer.clone(), json!(count)))
                .collect(),
        ),
    );
    out.insert("cells_changed".to_owned(), json!(net.len()));
    if let Some(bounds) = bounds {
        out.insert("box".to_owned(), json!(bounds));
    }
    out.insert("layers".to_owned(), layers_json(&working));
    let unchanged = *workspace.open_map(path)?.map == working;
    if dry_run || unchanged {
        out.insert("revision".to_owned(), json!(revision));
        if dry_run {
            out.insert("dry_run".to_owned(), json!(true));
        } else {
            out.insert(
                "diagnostics".to_owned(),
                json!([Diagnostic::info(
                    "nothing_to_do",
                    "The operations change nothing: every cell already holds that tile.",
                    "tilemap_edit with action get shows what the map holds.",
                )]),
            );
        }
        return Ok(out);
    }
    let revision =
        workspace.commit_map(path, working, (actor.clone(), Action::Edit, reason, None))?;
    out.insert("revision".to_owned(), json!(revision));
    Ok(out)
}

fn file_invalid(path: &str, problem: &Diagnostic) -> Diagnostic {
    Diagnostic::error(
        "tilemap_file_invalid",
        format!(
            "'{path}' was changed on disk and no longer reads ({}); it is not changed until it is fixed.",
            problem.message
        ),
        format!("Fix the file by hand (check shows where), or restore it from a checkpoint. {}", problem.hint),
    )
    .at_file(path)
}

/// The layers of a map, for answers: name, collide, tile count, the box
/// their tiles cover.
fn layers_json(map: &Tilemap) -> Value {
    Value::Array(
        map.layers
            .iter()
            .map(|layer| {
                let mut entry = json!({
                    "name": layer.name,
                    "collide": layer.collide,
                    "tiles": layer.tile_count(),
                });
                if let Some(bounds) = layer.bounds() {
                    entry["box"] = json!(bounds);
                }
                entry
            })
            .collect(),
    )
}

/// `tilemap_edit` get: the map's layers, and the tiles in a rectangle.
pub(crate) fn get(
    workspace: &mut Workspace,
    path: &str,
    region: Option<((i32, i32), (i32, i32))>,
) -> Result<Map<String, Value>> {
    let open = workspace.open_map(path)?;
    let map = Arc::clone(&open.map);
    let mut out = Map::new();
    out.insert("tilemap".to_owned(), json!(path));
    out.insert("revision".to_owned(), json!(open.revision));
    out.insert("can_undo".to_owned(), json!(!open.undo.is_empty()));
    out.insert("can_redo".to_owned(), json!(!open.redo.is_empty()));
    if let Some(problem) = &open.disk_problem {
        out.insert(
            "diagnostics".to_owned(),
            json!([file_invalid(path, problem)]),
        );
    }
    out.insert("tileset".to_owned(), json!(map.tileset));
    if let Some(count) = tileset_size(workspace, &map) {
        out.insert("tileset_tiles".to_owned(), json!(count));
    }
    if let Some(bounds) = map.bounds() {
        out.insert("box".to_owned(), json!(bounds));
    }
    out.insert("layers".to_owned(), layers_json(&map));
    if let Some(((x0, y0), (x1, y1))) = region {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        if i64::from(x1) - i64::from(x0) >= MAX_GET_SIDE
            || i64::from(y1) - i64::from(y0) >= MAX_GET_SIDE
        {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("get returns at most {MAX_GET_SIDE} x {MAX_GET_SIDE} cells."),
                "Ask for a smaller rectangle (from, to), or several.",
            )
            .at_path("/to"));
        }
        let rows: Map<String, Value> = map
            .layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                let rows: Vec<Value> = (y0..=y1)
                    .rev()
                    .map(|y| json!((x0..=x1).map(|x| map.get(index, x, y)).collect::<Vec<_>>()))
                    .collect();
                (layer.name.clone(), Value::Array(rows))
            })
            .collect();
        out.insert(
            "region".to_owned(),
            json!({ "from": [x0, y0], "to": [x1, y1] }),
        );
        out.insert("rows".to_owned(), Value::Object(rows));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// History (edit_history on a tilemap)
// ---------------------------------------------------------------------------

/// One step of an undo or redo: the map before and after it, the revert it
/// undoes or redoes (if it is one), and whether it goes forward (redo).
type Move = (Arc<Tilemap>, Arc<Tilemap>, Option<u64>, bool);

/// `edit_history` on a tilemap file: list, undo, redo, revert.
pub(crate) fn history(
    workspace: &mut Workspace,
    path: &str,
    (action, revision, steps, limit): (&str, Option<u64>, i64, usize),
    actor: &Actor,
    dry_run: bool,
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    out.insert("scene".to_owned(), json!(path));
    let open = workspace.open_map(path)?;
    if action == "list" {
        let entries: Vec<Value> = open
            .log
            .iter()
            .rev()
            .take(limit)
            .map(|log| {
                let mut item = json!({
                    "revision": log.revision,
                    "actor": log.actor.label(),
                    "action": log.action.name(),
                    "cells": log.cells,
                    "layers": log.layers,
                });
                if let Some(reason) = &log.reason {
                    item["reason"] = json!(reason);
                }
                if let Some(reverts) = log.reverts {
                    item["reverts"] = json!(reverts);
                }
                match open.revertible(log.revision) {
                    Ok(_) => item["revertible"] = json!(true),
                    Err(problem) => {
                        item["revertible"] = json!(false);
                        if problem.code != "not_revertible" {
                            item["not_revertible"] = json!(problem.code);
                        }
                    }
                }
                if open
                    .undo
                    .iter()
                    .any(|entry| entry.revision == log.revision && entry.reverted)
                {
                    item["reverted"] = json!(true);
                }
                item
            })
            .collect();
        out.insert("revision".to_owned(), json!(open.revision));
        out.insert("can_undo".to_owned(), json!(!open.undo.is_empty()));
        out.insert("can_redo".to_owned(), json!(!open.redo.is_empty()));
        if let Some(next) = open.undo.back() {
            out.insert("next_undo_by".to_owned(), json!(next.actor.label()));
        }
        out.insert("entries".to_owned(), json!(entries));
        return Ok(out);
    }
    if let Some(problem) = &open.disk_problem {
        return Err(file_invalid(path, problem));
    }
    out.insert("action".to_owned(), json!(action));
    if action == "revert" {
        let Some(revision) = revision else {
            return Err(Diagnostic::error(
                "missing_parameter",
                "revert needs \"revision\": the change to take back.",
                "Give the revision from edit_history list (or from tilemap_edit's answer), like \"revision\": 12.",
            )
            .at_path("/revision"));
        };
        let entry = open.revertible(revision)?;
        let mut working = (*open.map).clone();
        let mut cells = 0;
        for (layer, x, y, was, _) in cell_changes(&entry.before, &entry.after) {
            if let Some(index) = working.layer_index(&layer) {
                working.set(index, x, y, was)?;
                cells += 1;
            }
        }
        out.insert("reverted".to_owned(), json!(revision));
        out.insert("cells_changed".to_owned(), json!(cells));
        if dry_run {
            out.insert("revision".to_owned(), json!(open.revision));
            out.insert("dry_run".to_owned(), json!(true));
            return Ok(out);
        }
        let reason = Some(format!("took back r{revision}"));
        let new = workspace.commit_map(
            path,
            working,
            (actor.clone(), Action::Revert, reason, Some(revision)),
        )?;
        let open = workspace.opened(path)?;
        if let Some(entry) = open
            .undo
            .iter_mut()
            .find(|entry| entry.revision == revision)
        {
            entry.reverted = true;
        }
        out.insert("revision".to_owned(), json!(new));
        return Ok(out);
    }
    // Undo and redo, `steps` times: one written file at the end.
    let mut undo = open.undo.clone();
    let mut redo = open.redo.clone();
    let mut map = Arc::clone(&open.map);
    let mut moves: Vec<Move> = Vec::new();
    for _ in 0..steps {
        if action == "undo" {
            let Some(entry) = undo.pop_back() else { break };
            moves.push((
                Arc::clone(&map),
                Arc::clone(&entry.before),
                entry.reverts,
                false,
            ));
            map = Arc::clone(&entry.before);
            redo.push(entry);
        } else {
            let Some(entry) = redo.pop() else { break };
            moves.push((
                Arc::clone(&map),
                Arc::clone(&entry.after),
                entry.reverts,
                true,
            ));
            map = Arc::clone(&entry.after);
            undo.push_back(entry);
        }
    }
    let done = moves.len();
    out.insert("steps_done".to_owned(), json!(done));
    if done == 0 {
        out.insert("revision".to_owned(), json!(open.revision));
        out.insert(
            "diagnostics".to_owned(),
            json!([Diagnostic::info(
                "nothing_to_do",
                format!("There is nothing to {action}."),
                "Use action \"list\" to see the history.",
            )]),
        );
        return Ok(out);
    }
    let cells: usize = moves
        .iter()
        .map(|(before, after, ..)| cell_changes(before, after).len())
        .sum();
    out.insert("cells_changed".to_owned(), json!(cells));
    if dry_run {
        out.insert("revision".to_owned(), json!(open.revision + done as u64));
        out.insert("dry_run".to_owned(), json!(true));
        return Ok(out);
    }
    let text = map.to_canonical()?;
    crate::documents::write_project_file(workspace.root(), path, &text)?;
    let open = workspace.opened(path)?;
    open.disk_text = text;
    open.undo = undo;
    open.redo = redo;
    let kind = if action == "undo" {
        Action::Undo
    } else {
        Action::Redo
    };
    for (before, after, reverts, forward) in moves {
        // Undoing a revert puts the change it took back in force again;
        // redoing it takes it back again.
        if let Some(reverted) = reverts
            && let Some(entry) = open
                .undo
                .iter_mut()
                .find(|entry| entry.revision == reverted)
        {
            entry.reverted = forward;
        }
        open.revision += 1;
        let changed = cell_changes(&before, &after);
        open.log.push_back(LogEntry {
            revision: open.revision,
            actor: actor.clone(),
            action: kind,
            reason: None,
            cells: changed.len(),
            layers: touched_layers(&before, &after, &changed),
            reverts: None,
        });
        while open.log.len() > MAX_LOG {
            open.log.pop_front();
        }
    }
    open.map = map;
    out.insert("revision".to_owned(), json!(open.revision));
    Ok(out)
}
