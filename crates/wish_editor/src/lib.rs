//! The editor of Wish Engine: panels over the control plane, drawn on the CPU.
//!
//! M8a WP1 is the proof that this works: [`Editor::run_frame`] runs egui on
//! given input and [`raster`] draws the result into a [`raster::Canvas`] with
//! no GPU, the same on every machine (see the M8a design note, §2).

mod app;
mod browser;
mod gizmo;
pub mod i18n;
mod inspector;
mod panels;
pub mod parity;
pub mod raster;
pub mod script;
mod settings;

pub use app::{EditorApp, Opener, Row, ids};

/// Where the person's editor preferences are kept: `WISH_CONFIG_DIR`, else
/// the system's place for settings (`%APPDATA%\\Wish` on Windows,
/// `$XDG_CONFIG_HOME/wish` or `~/.config/wish` elsewhere), file
/// `editor.json`.
pub fn preferences_path() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let folder = if let Some(dir) = var("WISH_CONFIG_DIR") {
        std::path::PathBuf::from(dir)
    } else if cfg!(windows) {
        std::path::PathBuf::from(var("APPDATA")?).join("Wish")
    } else if let Some(dir) = var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(dir).join("wish")
    } else {
        std::path::PathBuf::from(var("HOME")?)
            .join(".config")
            .join("wish")
    };
    Some(folder.join("editor.json"))
}
pub use inspector::has_widget;

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, RawInput, Rect, Ui, pos2, vec2};
use raster::{Canvas, Textures};

/// The editor's drawing surface: egui, its fonts and the textures it made.
pub struct Editor {
    context: Context,
    textures: Textures,
    /// Seconds since the editor started, as egui's animations see them. It
    /// advances by what the caller says each frame, never by a clock.
    time: f64,
    /// The widgets of the last frame, from egui's accessibility tree.
    widgets: Vec<Widget>,
    /// Whether egui asked for another frame right away (a menu opening, an
    /// animation).
    repaint: bool,
    /// Textures egui no longer needs, freed when the next frame is painted
    /// (a frame laid out but not painted keeps its list here).
    to_free: Vec<egui::TextureId>,
}

/// A widget on screen, as the accessibility tree describes it: what a
/// screen reader would say, and where it is. Tests find buttons and fields
/// by name with it instead of by pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    /// Its role (`Button`, `Label`, `CheckBox`, `SpinButton`...).
    pub role: String,
    /// Its label, if it has one.
    pub label: Option<String>,
    /// Its value (a field's number or text), if it has one.
    pub value: Option<String>,
    /// Where it is, in pixels.
    pub rect: Rect,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    /// An editor with the engine's own font (Roboto) in both families.
    pub fn new() -> Self {
        let context = Context::default();
        let mut fonts = FontDefinitions::empty();
        fonts.font_data.insert(
            "roboto".to_owned(),
            Arc::new(FontData::from_static(wish_text::DEFAULT_FONT)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("roboto".to_owned());
        }
        context.set_fonts(fonts);
        context.enable_accesskit();
        Self {
            context,
            textures: Textures::default(),
            time: 0.0,
            widgets: Vec::new(),
            repaint: false,
            to_free: Vec::new(),
        }
    }

    /// Whether the last frame asked for another one right away (a menu that
    /// opens draws on the next frame).
    pub fn wants_repaint(&self) -> bool {
        self.repaint
    }

    /// The widgets of the last frame (role, label, value, place).
    pub fn widgets(&self) -> &[Widget] {
        &self.widgets
    }

    /// The egui context (to set styles; panels receive the [`Ui`]).
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Runs one frame of the interface: layout, input, and the triangles to
    /// draw (no pixels yet; see [`Editor::paint`]).
    ///
    /// `events` are the input of this frame (mouse, keys, text); `seconds` is
    /// how long the frame lasted; `size` is the surface in pixels. Nothing
    /// reads a clock: the same calls give the same frame.
    pub fn layout(
        &mut self,
        size: (u32, u32),
        seconds: f64,
        events: Vec<egui::Event>,
        ui: impl FnMut(&mut Ui),
    ) -> Frame {
        self.time += seconds;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(size.0 as f32, size.1 as f32),
            )),
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let mut output = self.context.run_ui(input, ui);
        self.repaint = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay.is_zero());
        if let Some(update) = output.platform_output.accesskit_update.take() {
            self.widgets = update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    Some(Widget {
                        role: format!("{:?}", node.role()),
                        label: node.label().map(str::to_owned),
                        value: node.value().map(str::to_owned),
                        rect: Rect::from_min_max(
                            pos2(bounds.x0 as f32, bounds.y0 as f32),
                            pos2(bounds.x1 as f32, bounds.y1 as f32),
                        ),
                    })
                })
                .collect();
        }
        self.textures.apply(&output.textures_delta);
        let primitives = self
            .context
            .tessellate(std::mem::take(&mut output.shapes), 1.0);
        self.to_free
            .extend(output.textures_delta.free.iter().copied());
        // Handled: egui asserts that no change is dropped unapplied.
        output.textures_delta.clear();
        Frame { primitives }
    }

    /// Draws a frame's triangles onto the canvas.
    pub fn paint(&mut self, frame: &Frame, canvas: &mut Canvas) {
        raster::paint(canvas, &self.textures, &frame.primitives, 1.0);
        let free = std::mem::take(&mut self.to_free);
        self.textures.free_ids(&free);
    }

    /// [`Editor::layout`] and [`Editor::paint`] in one: one frame of the
    /// interface onto `canvas`.
    pub fn run_frame(
        &mut self,
        canvas: &mut Canvas,
        seconds: f64,
        events: Vec<egui::Event>,
        ui: impl FnMut(&mut Ui),
    ) {
        let frame = self.layout((canvas.width, canvas.height), seconds, events, ui);
        self.paint(&frame, canvas);
    }
}

/// One frame's triangles, ready to draw.
pub struct Frame {
    primitives: Vec<egui::ClippedPrimitive>,
}

impl Frame {
    /// How many triangles the frame has.
    pub fn triangles(&self) -> usize {
        self.primitives
            .iter()
            .map(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh.indices.len() / 3,
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum()
    }
}
