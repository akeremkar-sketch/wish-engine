//! Painting tilemaps by hand (design note `docs/design/1.1-tilemaps.md`
//! §8): with an entity that has a `Tilemap` selected, the inspector shows a
//! tile palette under the component (the layer to paint, the tools, the
//! tileset's tiles), and a press in the scene view paints instead of
//! picking. Every stroke is one `tilemap_edit` call by the person, the same
//! call an agent makes, and lands in the map's history.
//!
//! The mathematics (which cell is under the pointer, a cell's outline on
//! the screen, a stroke's operation) has no window in it and is tested on
//! its own.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use egui::{Pos2, Ui, vec2};
use glam::{Mat4, Vec3};
use serde_json::{Value, json};

use crate::gizmo::Eye;
use crate::i18n::t;

/// A painting tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Paints the chosen tile on every cell the pointer passes over.
    Brush,
    /// Paints the chosen tile on the rectangle between press and release.
    Rect,
    /// Paints the chosen tile on the connected cells that hold what the
    /// clicked one holds.
    Fill,
    /// Empties every cell the pointer passes over.
    Erase,
    /// Takes the tile of the clicked cell as the one to paint.
    Picker,
}

impl Tool {
    /// Every tool, in the palette's order.
    pub const ALL: [Tool; 5] = [
        Tool::Brush,
        Tool::Rect,
        Tool::Fill,
        Tool::Erase,
        Tool::Picker,
    ];

    /// Its name on its button.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Brush => t("Brush"),
            Tool::Rect => t("Rectangle"),
            Tool::Fill => t("Fill"),
            Tool::Erase => t("Erase"),
            Tool::Picker => t("Picker"),
        }
    }
}

/// What the palette knows of the selected tilemap.
#[derive(Clone, Debug)]
pub struct Info {
    /// Its map file.
    pub map: String,
    /// The map's layers, in order.
    pub layers: Vec<String>,
    /// Each tile's rectangle in the tileset's picture.
    pub tiles: Vec<[u32; 4]>,
    /// The tileset's picture, when it is known.
    pub image: Option<String>,
    /// Tiles that have a name, by number.
    pub names: BTreeMap<u32, String>,
    /// The tiles that are solid.
    pub solid: std::collections::BTreeSet<u32>,
    /// The tileset's file.
    pub tileset: String,
    /// The entity's world matrix.
    pub world: Mat4,
    /// The size of a cell (the component's `cell_size`).
    pub cell: [f32; 2],
}

/// A stroke being made: the cells it covers so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// The tool it is made with.
    pub tool: Tool,
    /// The cell it started on.
    pub anchor: (i32, i32),
    /// The cells it passed over (the brush and the eraser), in order.
    pub cells: Vec<(i32, i32)>,
    /// The cell under the pointer now.
    pub now: (i32, i32),
}

/// The palette's state.
#[derive(Default)]
pub struct TilePaint {
    /// The tool in the person's hand, if painting.
    pub tool: Option<Tool>,
    /// The layer painted on.
    pub layer: String,
    /// The tile painted.
    pub tile: i32,
    /// The selected tilemap, if one is selected.
    pub info: Option<Info>,
    /// What `info` was read for: the selection, the scene's revision and
    /// how many strokes were made.
    pub info_for: (Vec<String>, u64, u64),
    /// Strokes made so far (the map changes with each).
    pub strokes: u64,
    /// The stroke being made.
    pub stroke: Option<Stroke>,
    /// The cell under the pointer.
    pub hover: Option<(i32, i32)>,
    /// The tileset's picture as a texture, with its path.
    pub texture: RefCell<Option<(String, egui::TextureHandle)>>,
}

/// What the palette asks for.
#[derive(Default)]
pub struct PaintActions {
    /// Take up a tool (`Some(None)`: put it down).
    pub tool: Option<Option<Tool>>,
    /// Paint on this layer.
    pub layer: Option<String>,
    /// Paint this tile.
    pub tile: Option<i32>,
    /// Make the chosen tile solid (a wall) or not, in its tileset.
    pub solid: Option<bool>,
}

impl PaintActions {
    /// Whether anything was asked.
    pub fn any(&self) -> bool {
        self.tool.is_some() || self.layer.is_some() || self.tile.is_some() || self.solid.is_some()
    }
}

/// The cell of the map under the window point `at` (the line through the
/// pixel meets the map's plane), if it meets it.
pub fn cell_at(info: &Info, eye: &Eye, at: Pos2) -> Option<(i32, i32)> {
    let (origin, direction) = eye.ray(at);
    let centre = info.world.transform_point3(Vec3::ZERO);
    let normal = info.world.transform_vector3(Vec3::Z);
    let across = direction.dot(normal);
    if across.abs() < 1e-6 {
        return None;
    }
    let along = (centre - origin).dot(normal) / across;
    let hit = origin + direction * along;
    let local = info.world.inverse().transform_point3(hit);
    let (x, y) = (
        (local.x / info.cell[0]).floor(),
        (local.y / info.cell[1]).floor(),
    );
    (x.is_finite() && y.is_finite() && x.abs() < 65_536.0 && y.abs() < 65_536.0)
        .then_some((x as i32, y as i32))
}

/// The window points of the corners of the cells from `low` to `high`
/// (both included): bottom left, bottom right, top right, top left.
pub fn outline(info: &Info, eye: &Eye, low: (i32, i32), high: (i32, i32)) -> Option<[Pos2; 4]> {
    let (x0, x1) = (low.0.min(high.0), low.0.max(high.0) + 1);
    let (y0, y1) = (low.1.min(high.1), low.1.max(high.1) + 1);
    let corner = |x: i32, y: i32| {
        eye.project(info.world.transform_point3(Vec3::new(
            x as f32 * info.cell[0],
            y as f32 * info.cell[1],
            0.0,
        )))
    };
    Some([
        corner(x0, y0)?,
        corner(x1, y0)?,
        corner(x1, y1)?,
        corner(x0, y1)?,
    ])
}

/// The `tilemap_edit` operation a finished stroke makes on `layer` with
/// `tile` (`None` for the picker, which paints nothing).
pub fn stroke_op(stroke: &Stroke, layer: &str, tile: i32) -> Option<Value> {
    match stroke.tool {
        Tool::Brush | Tool::Erase => {
            let tile = if stroke.tool == Tool::Erase { -1 } else { tile };
            let cells: Vec<Value> = stroke
                .cells
                .iter()
                .map(|(x, y)| json!([x, y, tile]))
                .collect();
            Some(json!({ "op": "set", "layer": layer, "cells": cells }))
        }
        Tool::Rect => Some(json!({ "op": "rect", "layer": layer,
            "from": [stroke.anchor.0, stroke.anchor.1], "to": [stroke.now.0, stroke.now.1], "tile": tile })),
        Tool::Fill => Some(json!({ "op": "fill", "layer": layer,
            "at": [stroke.anchor.0, stroke.anchor.1], "tile": tile })),
        Tool::Picker => None,
    }
}

impl Stroke {
    /// A stroke begun with `tool` on `cell`.
    pub fn begin(tool: Tool, cell: (i32, i32)) -> Self {
        Self {
            tool,
            anchor: cell,
            cells: vec![cell],
            now: cell,
        }
    }

    /// The pointer is over `cell` now: the brush and the eraser take every
    /// cell on the way from the last one (no gaps when the pointer is fast).
    pub fn reach(&mut self, cell: (i32, i32)) {
        if cell == self.now {
            return;
        }
        if matches!(self.tool, Tool::Brush | Tool::Erase) {
            let (x, y) = self.now;
            let steps = (cell.0 - x).abs().max((cell.1 - y).abs()).min(4096);
            for step in 1..=steps {
                let at = (
                    x + ((cell.0 - x) * step) / steps,
                    y + ((cell.1 - y) * step) / steps,
                );
                if !self.cells.contains(&at) {
                    self.cells.push(at);
                }
            }
        }
        self.now = cell;
    }
}

/// Decodes a PNG of the project into a picture for the palette (at most
/// 4096 pixels a side, 16 MiB of file).
fn read_picture(root: &Path, path: &str) -> Option<egui::ColorImage> {
    let file = root.join(path);
    if std::fs::metadata(&file).ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(file).ok()?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().ok()?;
    let mut data = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut data).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let pixels = (0..width * height)
        .map(|index| {
            let at = index * channels;
            let [r, g, b, a] = match channels {
                4 => [data[at], data[at + 1], data[at + 2], data[at + 3]],
                3 => [data[at], data[at + 1], data[at + 2], 255],
                2 => [data[at], data[at], data[at], data[at + 1]],
                _ => [data[at], data[at], data[at], 255],
            };
            egui::Color32::from_rgba_unmultiplied(r, g, b, a)
        })
        .collect();
    Some(egui::ColorImage::new([width, height], pixels))
}

/// The palette, under the `Tilemap` component in the inspector.
pub(crate) fn section(ui: &mut Ui, paint: &TilePaint, root: &Path, actions: &mut PaintActions) {
    let Some(info) = &paint.info else {
        return;
    };
    ui.separator();
    ui.strong(t("Tile palette"));
    ui.horizontal(|ui| {
        ui.label(t("Paint on layer"));
        egui::ComboBox::from_id_salt("paint-layer")
            .selected_text(paint.layer.as_str())
            .show_ui(ui, |ui| {
                for layer in &info.layers {
                    if ui
                        .selectable_label(*layer == paint.layer, layer.as_str())
                        .clicked()
                    {
                        actions.layer = Some(layer.clone());
                    }
                }
            });
    });
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            if ui
                .selectable_label(paint.tool == Some(tool), tool.label())
                .clicked()
            {
                actions.tool = Some((paint.tool != Some(tool)).then_some(tool));
            }
        }
    });
    if paint.tool.is_some() {
        ui.weak(t(
            "Press and drag in the view to paint; Ctrl+Z takes the last stroke back.",
        ));
    }
    // The tileset's picture, made a texture once.
    let texture = info.image.as_ref().and_then(|image| {
        let mut kept = paint.texture.borrow_mut();
        if kept.as_ref().is_none_or(|(path, _)| path != image) {
            *kept = read_picture(root, image).map(|picture| {
                (
                    image.clone(),
                    ui.ctx().load_texture(
                        format!("tileset:{image}"),
                        picture,
                        egui::TextureOptions::NEAREST,
                    ),
                )
            });
        }
        kept.as_ref().map(|(_, texture)| texture.clone())
    });
    ui.horizontal_wrapped(|ui| {
        for (number, [x, y, w, h]) in info.tiles.iter().enumerate().take(1024) {
            let number = number as i32;
            let chosen = paint.tile == number;
            let text = number.to_string();
            let button = match &texture {
                Some(texture) => {
                    let size = texture.size_vec2();
                    let uv = egui::Rect::from_min_max(
                        egui::pos2(*x as f32 / size.x, *y as f32 / size.y),
                        egui::pos2((x + w) as f32 / size.x, (y + h) as f32 / size.y),
                    );
                    let image = egui::Image::new(texture)
                        .uv(uv)
                        .fit_to_exact_size(vec2(24.0, 24.0));
                    egui::Button::image_and_text(image, text).selected(chosen)
                }
                None => egui::Button::new(text).selected(chosen),
            };
            let mut response = ui.add(button);
            if let Some(name) = info.names.get(&(number as u32)) {
                response = response.on_hover_text(name);
            }
            if response.clicked() {
                actions.tile = Some(number);
            }
        }
    });
    // Whether the chosen tile is a wall: a property of the tileset.
    if let Ok(tile) = u32::try_from(paint.tile) {
        let mut solid = info.solid.contains(&tile);
        if ui.checkbox(&mut solid, t("Solid tile")).changed() {
            actions.solid = Some(solid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};
    use wish_core::Entity;
    use wish_render::CameraView;

    /// An orthographic eye 10 m tall over a 200 × 100 window at (0, 0),
    /// looking along -Z from (0, 0, 10): 10 pixels a meter, the origin at
    /// (100, 50).
    fn eye() -> Eye {
        let camera = CameraView {
            entity: Entity::default(),
            view: Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0)),
            position: Vec3::new(0.0, 0.0, 10.0),
            fov_y: 1.0,
            near: 0.1,
            far: 100.0,
            ortho_height: Some(10.0),
        };
        Eye::new(
            &camera,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0)),
        )
    }

    fn info() -> Info {
        Info {
            map: "maps/a.tilemap.json".into(),
            layers: vec!["floor".into()],
            tiles: vec![[0, 0, 8, 8]],
            image: None,
            names: BTreeMap::new(),
            solid: std::collections::BTreeSet::new(),
            tileset: "tiles/a.tileset.json".into(),
            world: Mat4::from_translation(Vec3::new(-2.0, -1.0, 0.0)),
            cell: [0.5, 0.5],
        }
    }

    #[test]
    fn the_cell_under_the_pointer() {
        // World (0, 0) is cell (4, 2) of a map at (-2, -1) with cells of
        // half a meter.
        assert_eq!(cell_at(&info(), &eye(), pos2(101.0, 49.0)), Some((4, 2)));
        assert_eq!(cell_at(&info(), &eye(), pos2(99.0, 51.0)), Some((3, 1)));
        let corners = outline(&info(), &eye(), (4, 2), (4, 2)).unwrap();
        assert!(corners[0].distance(pos2(100.0, 50.0)) < 1e-3, "{corners:?}");
        assert!(corners[2].distance(pos2(105.0, 45.0)) < 1e-3, "{corners:?}");
    }

    #[test]
    fn a_fast_brush_leaves_no_gaps_and_strokes_make_one_operation() {
        let mut stroke = Stroke::begin(Tool::Brush, (0, 0));
        stroke.reach((3, 1));
        assert_eq!(stroke.cells, [(0, 0), (1, 0), (2, 0), (3, 1)]);
        stroke.reach((3, 1));
        assert_eq!(stroke.cells.len(), 4);
        assert_eq!(
            stroke_op(&stroke, "floor", 2).unwrap(),
            json!({ "op": "set", "layer": "floor", "cells": [[0, 0, 2], [1, 0, 2], [2, 0, 2], [3, 1, 2]] })
        );
        let mut rect = Stroke::begin(Tool::Rect, (1, 1));
        rect.reach((4, 2));
        assert_eq!(rect.cells, [(1, 1)]);
        assert_eq!(
            stroke_op(&rect, "walls", 1).unwrap(),
            json!({ "op": "rect", "layer": "walls", "from": [1, 1], "to": [4, 2], "tile": 1 })
        );
        let erase = Stroke::begin(Tool::Erase, (5, 5));
        assert_eq!(
            stroke_op(&erase, "walls", 1).unwrap()["cells"],
            json!([[5, 5, -1]])
        );
        assert!(stroke_op(&Stroke::begin(Tool::Picker, (0, 0)), "walls", 1).is_none());
    }
}
