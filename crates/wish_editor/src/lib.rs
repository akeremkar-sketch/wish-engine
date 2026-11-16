//! The editor of Wish Engine: panels over the control plane, drawn on the CPU.
//!
//! M8a WP1 is the proof that this works: [`Editor::run_frame`] runs egui on
//! given input and [`raster`] draws the result into a [`raster::Canvas`] with
//! no GPU, the same on every machine (see the M8a design note, §2).

pub mod raster;

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
        Self {
            context,
            textures: Textures::default(),
            time: 0.0,
        }
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
        self.textures.apply(&output.textures_delta);
        let primitives = self
            .context
            .tessellate(std::mem::take(&mut output.shapes), 1.0);
        let free = output.textures_delta.free.iter().copied().collect();
        // Handled: egui asserts that no change is dropped unapplied.
        output.textures_delta.clear();
        Frame { primitives, free }
    }

    /// Draws a frame's triangles onto the canvas.
    pub fn paint(&mut self, frame: &Frame, canvas: &mut Canvas) {
        raster::paint(canvas, &self.textures, &frame.primitives, 1.0);
        self.textures.free_ids(&frame.free);
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
    free: Vec<egui::TextureId>,
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
