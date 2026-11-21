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

use crate::i18n::{t, tf};
use egui::{ColorImage, Sense, TextureHandle, TextureOptions, Ui, vec2};
use serde_json::{Value, json};

/// A file being dragged out of the browser: its project-relative path. A
/// drop target takes it (the view places it; an asset field uses it).
#[derive(Clone, Debug)]
pub(crate) struct AssetDrag(pub String);

/// One row of the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asset {
    /// Project-relative path.
    pub path: String,
    /// `image`, `model`, `sound`, `prefab`, `material`, `sheet` or `other`.
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
        } else if lower.ends_with(".mat.json") {
            "material"
        } else if lower.ends_with(".sheet.json") {
            "sheet"
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
    /// The name of a sprite sheet's new clip, typed so far.
    new_clip: String,
}

/// What the panel needs to know.
pub(crate) struct BrowserView<'a> {
    pub assets: &'a [Asset],
    pub selected: Option<&'a str>,
    /// `asset_info` of the selected file.
    pub detail: Option<&'a Value>,
    pub root: &'a Path,
    pub state: &'a RefCell<BrowserState>,
    /// What the fields of a selected material file are edited with: the
    /// `MaterialFile` schema and the inspector's widgets.
    pub material: Option<(&'a Value, &'a crate::inspector::InspectorView<'a>)>,
    /// The inspector's widgets, for the fields of a selected sprite sheet.
    pub widgets: &'a crate::inspector::InspectorView<'a>,
}

/// What the panel asked for.
#[derive(Default)]
pub(crate) struct BrowserActions {
    pub select: Option<String>,
    /// Put this file in the scene (at the centre of the view).
    pub place: Option<String>,
    /// Move a file: from, to.
    pub rename: Option<(String, String)>,
    /// Open a file in an outside program.
    pub open: Option<String>,
    /// Make a placeholder file (`asset_create`): its path and arguments.
    pub create: Option<(String, Value)>,
    /// Change one field of a material file: the file, the field, its new
    /// value (`asset_create` over the file, with every field it writes).
    pub material: Option<(String, String, Value)>,
    /// Change a sprite sheet: the file, and its top-level keys (or one clip,
    /// `clips/<name>`) with their new values (`Null`: it goes), applied to
    /// what the file writes now (`asset_create` over the file).
    pub sheet: Option<(String, Vec<(String, Value)>)>,
}

impl BrowserActions {
    pub fn any(&self) -> bool {
        self.select.is_some()
            || self.place.is_some()
            || self.rename.is_some()
            || self.open.is_some()
            || self.create.is_some()
            || self.material.is_some()
            || self.sheet.is_some()
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
        "image" => (t("img"), egui::Color32::from_rgb(70, 140, 200)),
        "model" => (t("3d"), egui::Color32::from_rgb(200, 140, 70)),
        "sound" => (t("snd"), egui::Color32::from_rgb(110, 180, 110)),
        "prefab" => (t("pfb"), egui::Color32::from_rgb(190, 110, 190)),
        _ => (t("file"), egui::Color32::from_rgb(130, 130, 130)),
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

/// A free name for a new file: `<base>.<ext>`, `<base>_2.<ext>`...
fn free_name(view: &BrowserView<'_>, folder: &str, base: &str, ext: &str) -> String {
    (1..1000)
        .map(|n| {
            if n == 1 {
                format!("{folder}/{base}.{ext}")
            } else {
                format!("{folder}/{base}_{n}.{ext}")
            }
        })
        .find(|path| !view.assets.iter().any(|a| a.path == *path) && !view.root.join(path).exists())
        .unwrap_or_else(|| format!("{folder}/{base}_new.{ext}"))
}

/// "New...": a placeholder sound or picture, made from a few numbers, or a
/// material file with the default look (the `asset_create` tool, as an
/// agent makes them).
fn create_menu(ui: &mut Ui, view: &BrowserView<'_>, actions: &mut BrowserActions) {
    ui.menu_button(t("New..."), |ui| {
        ui.weak(t("A placeholder until the real file exists:"));
        let sounds: [(&str, Value); 4] = [
            (t("Jump sound"), json!({ "wave": "square", "frequency": 300, "end_frequency": 700, "seconds": 0.15, "release": 0.1 })),
            (t("Coin sound"), json!({ "wave": "sine", "frequency": 900, "end_frequency": 1400, "seconds": 0.12 })),
            (t("Thud sound"), json!({ "wave": "noise", "seconds": 0.25, "release": 0.2 })),
            (t("Beep sound"), json!({ "wave": "sine", "frequency": 440, "seconds": 0.2, "release": 0.05 })),
        ];
        for (label, sound) in sounds {
            if ui.button(label).clicked() {
                let path = free_name(view, "assets/sounds", "sound", "wav");
                actions.create = Some((path, json!({ "sound": sound })));
                ui.close();
            }
        }
        ui.separator();
        let pictures: [(&str, Value); 3] = [
            (t("Circle picture"), json!({ "width": 64, "height": 64, "shape": "circle", "color": [1.0, 0.8, 0.2, 1.0], "background": [0, 0, 0, 0] })),
            (t("Square picture"), json!({ "width": 64, "height": 64, "shape": "fill", "color": [0.3, 0.6, 1.0, 1.0] })),
            (t("Diamond picture"), json!({ "width": 64, "height": 64, "shape": "diamond", "color": [0.9, 0.3, 0.4, 1.0], "background": [0, 0, 0, 0] })),
        ];
        for (label, picture) in pictures {
            if ui.button(label).clicked() {
                let path = free_name(view, "assets/images", "picture", "png");
                actions.create = Some((path, json!({ "picture": picture })));
                ui.close();
            }
        }
        ui.separator();
        // One look for many entities: its fields are edited where the file
        // is shown, and entities name it in their Material's `file`.
        if ui.button(t("Material file")).clicked() {
            let path = free_name(view, "assets/materials", "material", "mat.json");
            actions.create = Some((path, json!({ "material": {} })));
            ui.close();
        }
        // A sheet of the selected picture (or the project's first): one
        // frame the size of the picture, cut finer and given clips where the
        // file is shown.
        let picture = view
            .selected
            .filter(|path| path.to_ascii_lowercase().ends_with(".png"))
            .or_else(|| {
                view.assets
                    .iter()
                    .map(|asset| asset.path.as_str())
                    .find(|path| path.to_ascii_lowercase().ends_with(".png"))
            });
        if ui
            .add_enabled(picture.is_some(), egui::Button::new(t("Sprite sheet")))
            .clicked()
            && let Some(picture) = picture
        {
            let (width, height) = png_size(&view.root.join(picture)).unwrap_or((32, 32));
            let stem = picture
                .rsplit('/')
                .next()
                .unwrap_or(picture)
                .trim_end_matches(".png")
                .trim_end_matches(".PNG");
            let folder = picture.rsplit_once('/').map_or("assets", |(folder, _)| folder);
            let path = free_name(view, folder, stem, "sheet.json");
            actions.create = Some((
                path,
                json!({ "sheet": {
                    "image": picture,
                    "grid": { "frame": [width.clamp(1, 4096), height.clamp(1, 4096)], "columns": 1, "rows": 1 }
                } }),
            ));
            ui.close();
        }
    });
}

/// The size a PNG file says it has, from its header.
fn png_size(file: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut header = [0u8; 24];
    std::fs::File::open(file)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    if &header[1..4] != b"PNG" || &header[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    Some((word(16), word(20)))
}

/// The JSON schema the inspector's widgets edit a sprite sheet with: its
/// picture, how it is cut (a grid, or rectangles as JSON) and each clip
/// (frames as JSON, fps, loop). Field names are the file's own.
fn sheet_schema(written: &Value) -> Value {
    let pair = |doc: &str| json!({ "type": "array", "items": { "type": "number" }, "maxItems": 2, "description": doc });
    let mut properties = serde_json::Map::new();
    properties.insert(
        "image".to_owned(),
        json!({ "type": "string", "x-asset": true, "description": "The picture (a PNG) the frames are cut from." }),
    );
    if written.get("frames").is_some() {
        properties.insert(
            "frames".to_owned(),
            json!({ "description": "The frames: [[x, y, width, height], ...] in the picture's pixels from its top left corner." }),
        );
    } else {
        properties.insert(
            "grid".to_owned(),
            json!({ "type": "object", "description": "Frames of one size, numbered from 0 left to right, then down.", "properties": {
                "frame": pair("A frame's width and height in pixels."),
                "columns": { "type": "integer", "minimum": 1, "maximum": 4096, "default": 1, "description": "Frames across." },
                "rows": { "type": "integer", "minimum": 1, "maximum": 4096, "default": 1, "description": "Frames down." },
                "margin": pair("Pixels before the first frame: left, top."),
                "spacing": pair("Pixels between frames: across, down."),
            } }),
        );
    }
    let mut clips = serde_json::Map::new();
    for name in written["clips"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name)
    {
        clips.insert(
            name.clone(),
            json!({ "type": "object", "properties": {
                "frames": { "description": "The sheet's frames it shows, by number, in order: [0, 1, 2, 3]." },
                "fps": { "type": "number", "minimum": 0.1, "maximum": 240, "default": 10, "description": "Frames a second." },
                "loop": { "type": "boolean", "default": true, "description": "Starts over at the end; false stops on the last frame." },
            } }),
        );
    }
    if !clips.is_empty() {
        properties.insert(
            "clips".to_owned(),
            json!({ "type": "object", "properties": clips, "description": "Named runs of frames." }),
        );
    }
    json!({ "properties": properties })
}

/// A sprite sheet's fields where the browser shows the file: the
/// inspector's widgets over [`sheet_schema`], a clip added or removed, the
/// picture cut by a grid or by rectangles. Every change is one
/// `asset_create` over the file, by the person.
fn sheet_panel(
    ui: &mut Ui,
    view: &BrowserView<'_>,
    path: &str,
    info: &Value,
    actions: &mut BrowserActions,
) {
    let written = &info["written"];
    ui.weak(t(
        "A change here is written to the file: everything that uses it changes.",
    ));
    let schema = sheet_schema(written);
    if let Some((field, new)) =
        crate::inspector::sheet_fields(ui, view.widgets, path, &schema, written)
    {
        // A clip is changed on its own, so that a clip someone else changed
        // meanwhile is kept.
        let changes = if field == "clips" {
            new.as_object()
                .into_iter()
                .flatten()
                .filter(|(name, clip)| written["clips"].get(name.as_str()) != Some(*clip))
                .map(|(name, clip)| (format!("clips/{name}"), clip.clone()))
                .collect()
        } else {
            vec![(field, new)]
        };
        actions.sheet = Some((path.to_owned(), changes));
    }
    let clips = written["clips"].as_object().cloned().unwrap_or_default();
    for name in clips.keys() {
        ui.horizontal(|ui| {
            ui.weak(name);
            if ui.small_button(t("Remove")).clicked() {
                actions.sheet = Some((
                    path.to_owned(),
                    vec![(format!("clips/{name}"), Value::Null)],
                ));
            }
        });
    }
    ui.horizontal(|ui| {
        let mut state = view.state.borrow_mut();
        ui.add(
            egui::TextEdit::singleline(&mut state.new_clip)
                .hint_text(t("new clip name"))
                .desired_width(140.0),
        );
        let name = state.new_clip.trim().to_owned();
        if ui
            .add_enabled(
                !name.is_empty() && !clips.contains_key(&name),
                egui::Button::new(t("Add clip")),
            )
            .clicked()
        {
            actions.sheet = Some((
                path.to_owned(),
                vec![(format!("clips/{name}"), json!({ "frames": [0] }))],
            ));
            state.new_clip.clear();
        }
    });
    // The other way of cutting the picture, with the frames it has now.
    let frames: Vec<Value> = info["sheet"]["frames"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if written.get("grid").is_some() {
        if ui.button(t("Switch to rectangles")).clicked() {
            actions.sheet = Some((
                path.to_owned(),
                vec![
                    ("grid".to_owned(), Value::Null),
                    ("frames".to_owned(), Value::Array(frames)),
                ],
            ));
        }
    } else {
        ui.weak(t(
            "Rectangles: [x, y, width, height] each, in pixels from the top left",
        ));
        if ui.button(t("Switch to a grid")).clicked() {
            let first = frames.first().cloned().unwrap_or(json!([0, 0, 32, 32]));
            let grid = json!({ "frame": [first[2].clone(), first[3].clone()], "columns": frames.len().max(1), "rows": 1 });
            actions.sheet = Some((
                path.to_owned(),
                vec![
                    ("frames".to_owned(), Value::Null),
                    ("grid".to_owned(), grid),
                ],
            ));
        }
    }
}

/// The browser panel's contents.
pub(crate) fn panel(ui: &mut Ui, view: &BrowserView<'_>, actions: &mut BrowserActions) {
    ui.horizontal(|ui| {
        ui.weak(t("Filter:"));
        let mut state = view.state.borrow_mut();
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text(t("Filter"))
                .desired_width(160.0),
        );
        for (label, kind) in [
            (t("All"), None),
            (t("Images"), Some("image")),
            (t("Models"), Some("model")),
            (t("Sounds"), Some("sound")),
            (t("Prefabs"), Some("prefab")),
        ] {
            if ui.selectable_label(state.kind == kind, label).clicked() {
                state.kind = kind;
            }
        }
        ui.weak(tf("{count} files", &[("count", &view.assets.len())]));
        drop(state);
        create_menu(ui, view, actions);
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
        // The scroll area lays its contents out as its parent does (side by
        // side here): the detail is a column.
        egui::ScrollArea::vertical()
            .id_salt("asset_detail")
            .auto_shrink([false, false])
            .show(ui, |ui| ui.vertical(|ui| detail(ui, view, actions)));
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
            if asset.placeable() && ui.button(t("Add to scene")).clicked() {
                actions.place = Some(asset.path.clone());
                ui.close();
            }
            if ui.button(t("Open outside")).clicked() {
                actions.open = Some(asset.path.clone());
                ui.close();
            }
            if asset.kind != "prefab" && ui.button(t("Rename / move...")).clicked() {
                view.state.borrow_mut().rename = Some((asset.path.clone(), asset.path.clone()));
                ui.close();
            }
        });
    });
}

/// What `asset_info` says about the selected file. A material file's
/// fields are shown first, and can be changed.
fn detail(ui: &mut Ui, view: &BrowserView<'_>, actions: &mut BrowserActions) {
    let Some(path) = view.selected else {
        ui.weak(t("Select a file to see what it is and who uses it."));
        return;
    };
    ui.strong(path);
    let Some(info) = view.detail else {
        return;
    };
    let sheet = info["sheet"].is_object() && info["written"].is_object();
    if sheet {
        sheet_panel(ui, view, path, info, actions);
        ui.separator();
    }
    let look = info["material"]
        .is_object()
        .then_some(view.material)
        .flatten();
    if let Some((schema, inspector)) = look {
        ui.weak(t(
            "A change here is written to the file: everything that uses it changes.",
        ));
        if let Some((field, new)) =
            crate::inspector::material_fields(ui, inspector, path, schema, &info["material"])
        {
            actions.material = Some((path.to_owned(), field, new));
        }
        ui.separator();
    }
    for (key, value) in info.as_object().into_iter().flatten() {
        if matches!(key.as_str(), "ok" | "path") {
            continue;
        }
        // Shown as fields above.
        if look.is_some() && matches!(key.as_str(), "material" | "written") {
            continue;
        }
        if sheet && key == "written" {
            continue;
        }
        let text = match value {
            Value::Array(items) if items.is_empty() => t("(none)").to_owned(),
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
    ui.weak(t("Drag it into the view or onto a field of the inspector."));
}
