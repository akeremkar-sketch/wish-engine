//! The asset browser (M8a WP6): the project's files (pictures with
//! thumbnails, sounds, models) and prefabs, listed, filtered, inspected, moved
//! and dragged into the scene or onto an inspector field.
//!
//! Everything it does is a control-plane call: the list comes from
//! `asset_list`, the details from `asset_info`, a move from `asset_move`
//! (which also rewrites the references in scenes and prefabs).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use egui::{ColorImage, Sense, TextureHandle, TextureOptions, Ui, vec2};
use serde_json::Value;

/// A file being dragged out of the browser: its project-relative path. A
/// drop target takes it (the view places it; an asset field uses it).
#[derive(Clone, Debug)]
pub(crate) struct AssetDrag(pub String);

/// One row of the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asset {
    /// Project-relative path.
    pub path: String,
    /// `image`, `model`, `sound`, `prefab` or `other`.
    pub kind: &'static str,
    /// Size in bytes (0 for a prefab).
    pub bytes: u64,
}

impl Asset {
    /// The kind of a file, from its name.
    pub fn kind_of(path: &str) -> &'static str {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".prefab.json") {
            "prefab"
        } else if lower.ends_with(".glb") || lower.ends_with(".gltf") {
            "model"
        } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image"
        } else if lower.ends_with(".wav") || lower.ends_with(".ogg") {
            "sound"
        } else {
            "other"
        }
    }

    /// Whether it can be put in a scene.
    pub fn placeable(&self) -> bool {
        matches!(self.kind, "prefab" | "model")
    }
}

/// What the browser keeps between frames.
#[derive(Default)]
pub(crate) struct BrowserState {
    filter: String,
    kind: Option<&'static str>,
    /// Thumbnails, by path (`None`: could not be made).
    thumbs: BTreeMap<String, Option<TextureHandle>>,
    /// The file being renamed, and the new path typed so far.
    rename: Option<(String, String)>,
}

/// What the panel needs to know.
pub(crate) struct BrowserView<'a> {
    pub assets: &'a [Asset],
    pub selected: Option<&'a str>,
    /// `asset_info` of the selected file.
    pub detail: Option<&'a Value>,
    pub root: &'a Path,
    pub state: &'a RefCell<BrowserState>,
}

/// What the panel asked for.
#[derive(Default)]
pub(crate) struct BrowserActions {
    pub select: Option<String>,
    /// Put this file in the scene (at the centre of the view).
    pub place: Option<String>,
    /// Move a file: from, to.
    pub rename: Option<(String, String)>,
}

impl BrowserActions {
    pub fn any(&self) -> bool {
        self.select.is_some() || self.place.is_some() || self.rename.is_some()
    }
}

const THUMBNAIL: usize = 32;
/// Pictures larger than this are not decoded for a thumbnail.
const MAX_THUMBNAIL_FILE: u64 = 8 * 1024 * 1024;

/// A picture file as a small RGBA image, if it is a PNG that can be read.
fn thumbnail(root: &Path, path: &str) -> Option<ColorImage> {
    if !path.to_ascii_lowercase().ends_with(".png") {
        return None;
    }
    let file = root.join(path);
    if std::fs::metadata(&file).ok()?.len() > MAX_THUMBNAIL_FILE {
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
    if width == 0 || height == 0 {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let at = (y * width + x) * channels;
        match channels {
            4 => [data[at], data[at + 1], data[at + 2], data[at + 3]],
            3 => [data[at], data[at + 1], data[at + 2], 255],
            2 => [data[at], data[at], data[at], data[at + 1]],
            _ => [data[at], data[at], data[at], 255],
        }
    };
    // The longer side becomes THUMBNAIL pixels, nearest sample.
    let scale = THUMBNAIL as f32 / width.max(height) as f32;
    let (w, h) = (
        ((width as f32 * scale).round() as usize).max(1),
        ((height as f32 * scale).round() as usize).max(1),
    );
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b, a] = pixel(
                ((x as f32 + 0.5) / scale) as usize % width,
                ((y as f32 + 0.5) / scale) as usize % height,
            );
            pixels.push(egui::Color32::from_rgba_unmultiplied(r, g, b, a));
        }
    }
    Some(ColorImage::new([w, h], pixels))
}

fn badge(ui: &mut Ui, kind: &str) {
    let (text, color) = match kind {
        "image" => ("img", egui::Color32::from_rgb(70, 140, 200)),
        "model" => ("3d", egui::Color32::from_rgb(200, 140, 70)),
        "sound" => ("snd", egui::Color32::from_rgb(110, 180, 110)),
        "prefab" => ("pfb", egui::Color32::from_rgb(190, 110, 190)),
        _ => ("file", egui::Color32::from_rgb(130, 130, 130)),
    };
    let (rect, _) =
        ui.allocate_exact_size(vec2(THUMBNAIL as f32, THUMBNAIL as f32), Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, color.gamma_multiply(0.45));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
}

fn size_text(bytes: u64) -> String {
    match bytes {
        0 => String::new(),
        b if b < 10_000 => format!("{b} B"),
        b if b < 10_000_000 => format!("{:.0} KB", b as f64 / 1000.0),
        b => format!("{:.1} MB", b as f64 / 1_000_000.0),
    }
}

/// The browser panel's contents.
pub(crate) fn panel(ui: &mut Ui, view: &BrowserView<'_>, actions: &mut BrowserActions) {
    ui.horizontal(|ui| {
        ui.strong("Assets");
        let mut state = view.state.borrow_mut();
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Filter")
                .desired_width(160.0),
        );
        for (label, kind) in [
            ("All", None),
            ("Images", Some("image")),
            ("Models", Some("model")),
            ("Sounds", Some("sound")),
            ("Prefabs", Some("prefab")),
        ] {
            if ui.selectable_label(state.kind == kind, label).clicked() {
                state.kind = kind;
            }
        }
        ui.weak(format!("{} files", view.assets.len()));
    });
    ui.separator();
    let (filter, kind) = {
        let state = view.state.borrow();
        (state.filter.to_lowercase(), state.kind)
    };
    ui.horizontal_top(|ui| {
        egui::ScrollArea::vertical()
            .id_salt("asset_list")
            .max_width(ui.available_width() * 0.62)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    for asset in view
                        .assets
                        .iter()
                        .filter(|a| kind.is_none_or(|k| a.kind == k))
                        .filter(|a| filter.is_empty() || a.path.to_lowercase().contains(&filter))
                        .take(500)
                    {
                        row(ui, view, asset, actions);
                    }
                });
            });
        ui.separator();
        ui.vertical(|ui| detail(ui, view));
    });
}

fn row(ui: &mut Ui, view: &BrowserView<'_>, asset: &Asset, actions: &mut BrowserActions) {
    ui.horizontal(|ui| {
        // Thumbnail (made once) or a badge by kind.
        let texture = if asset.kind == "image" {
            let mut state = view.state.borrow_mut();
            state
                .thumbs
                .entry(asset.path.clone())
                .or_insert_with(|| {
                    thumbnail(view.root, &asset.path).map(|image| {
                        ui.ctx().load_texture(
                            format!("thumb:{}", asset.path),
                            image,
                            TextureOptions::NEAREST,
                        )
                    })
                })
                .clone()
        } else {
            None
        };
        match texture {
            Some(texture) => {
                ui.add(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(vec2(THUMBNAIL as f32, THUMBNAIL as f32)),
                );
            }
            None => badge(ui, asset.kind),
        }
        let renaming = view
            .state
            .borrow()
            .rename
            .as_ref()
            .filter(|(from, _)| *from == asset.path)
            .map(|(_, to)| to.clone());
        if let Some(mut to) = renaming {
            let editor = ui.add(egui::TextEdit::singleline(&mut to).desired_width(260.0));
            if !editor.has_focus() && !editor.lost_focus() {
                editor.request_focus();
            }
            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
            let mut state = view.state.borrow_mut();
            if enter {
                actions.rename = Some((asset.path.clone(), to));
                state.rename = None;
            } else if escape || editor.lost_focus() {
                state.rename = None;
            } else if let Some(pending) = state.rename.as_mut() {
                pending.1 = to;
            }
            return;
        }
        let selected = view.selected == Some(asset.path.as_str());
        let response = ui
            .selectable_label(selected, &asset.path)
            .interact(Sense::click_and_drag());
        if response.clicked() {
            actions.select = Some(asset.path.clone());
        }
        if response.double_clicked() && asset.placeable() {
            actions.place = Some(asset.path.clone());
        }
        if response.drag_started() {
            response.dnd_set_drag_payload(AssetDrag(asset.path.clone()));
        }
        ui.weak(size_text(asset.bytes));
        response.context_menu(|ui| {
            if asset.placeable() && ui.button("Add to scene").clicked() {
                actions.place = Some(asset.path.clone());
                ui.close();
            }
            if asset.kind != "prefab" && ui.button("Rename / move...").clicked() {
                view.state.borrow_mut().rename = Some((asset.path.clone(), asset.path.clone()));
                ui.close();
            }
        });
    });
}

/// What `asset_info` says about the selected file.
fn detail(ui: &mut Ui, view: &BrowserView<'_>) {
    let Some(path) = view.selected else {
        ui.weak("Select a file to see what it is and who uses it.");
        return;
    };
    ui.strong(path);
    let Some(info) = view.detail else {
        return;
    };
    for (key, value) in info.as_object().into_iter().flatten() {
        if matches!(key.as_str(), "ok" | "path") {
            continue;
        }
        let text = match value {
            Value::Array(items) if items.is_empty() => "(none)".to_owned(),
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map_or_else(|| item.to_string(), str::to_owned)
                })
                .collect::<Vec<_>>()
                .join(", "),
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!("{key}:"));
            ui.label(text);
        });
    }
    ui.add_space(4.0);
    ui.weak("Drag it into the view or onto a field of the inspector.");
}
