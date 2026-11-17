//! The editor's look (M9): one palette, one set of spacings and the icons
//! the panels use. The icons are drawn as shapes (the interface font has no
//! symbols for play, pause or a camera), so they look the same everywhere
//! and need no image files.

use egui::epaint::{CornerRadius, Margin, Shadow, Stroke};
use egui::{
    Color32, Context, FontFamily, FontId, Painter, Rect, Response, Role, Sense, TextStyle, Ui,
    Vec2, WidgetInfo, pos2, vec2,
};

/// The canvas behind everything (the editor's backdrop).
pub const BACKDROP: [u8; 4] = [17, 18, 24, 255];
/// Panels.
pub const PANEL: Color32 = Color32::from_rgb(26, 28, 36);
/// Raised areas inside panels: cards, groups of tools.
pub const RAISED: Color32 = Color32::from_rgb(34, 37, 48);
/// Widgets at rest.
pub const WIDGET: Color32 = Color32::from_rgb(42, 46, 60);
/// Widgets under the pointer.
pub const HOVER: Color32 = Color32::from_rgb(54, 59, 77);
/// Widgets being pressed.
pub const PRESSED: Color32 = Color32::from_rgb(64, 70, 92);
/// Thin lines between areas.
pub const LINE: Color32 = Color32::from_rgb(46, 50, 64);
/// Text.
pub const TEXT: Color32 = Color32::from_rgb(229, 231, 239);
/// Quieter text: hints, paths, numbers that are not the point.
pub const WEAK: Color32 = Color32::from_rgb(146, 152, 172);
/// The engine's own colour: what is chosen, what to do next.
pub const ACCENT: Color32 = Color32::from_rgb(139, 108, 255);
/// The accent, quieter (a chosen row's background).
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(58, 50, 104);
/// Running: play.
pub const GO: Color32 = Color32::from_rgb(62, 196, 130);
/// Recording, deleting, errors.
pub const RED: Color32 = Color32::from_rgb(238, 92, 96);
/// Warnings.
pub const AMBER: Color32 = Color32::from_rgb(232, 172, 64);
/// Lights and the sun.
pub const SUN: Color32 = Color32::from_rgb(250, 204, 92);
/// Cameras.
pub const SKY: Color32 = Color32::from_rgb(108, 182, 255);

/// Puts the engine's look on a context: colours, rounded corners, room
/// around widgets, text sizes.
pub fn apply(context: &Context) {
    context.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(8.0, 4.0);
        spacing.button_padding = vec2(8.0, 3.0);
        spacing.interact_size = vec2(36.0, 20.0);
        spacing.window_margin = Margin::same(12);
        spacing.menu_margin = Margin::same(8);
        spacing.indent = 16.0;

        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.override_text_color = None;
        visuals.panel_fill = PANEL;
        visuals.window_fill = RAISED;
        visuals.window_stroke = Stroke::new(1.0, LINE);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        visuals.popup_shadow = Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(100),
        };
        visuals.faint_bg_color = RAISED;
        visuals.extreme_bg_color = Color32::from_rgb(14, 15, 20);
        visuals.code_bg_color = Color32::from_rgb(20, 22, 29);
        visuals.hyperlink_color = ACCENT;
        visuals.warn_fg_color = AMBER;
        visuals.error_fg_color = RED;
        visuals.selection.bg_fill = ACCENT_SOFT;
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.weak_text_color = Some(WEAK);
        visuals.indent_has_left_vline = true;
        visuals.collapsing_header_frame = true;
        visuals.slider_trailing_fill = true;

        let round = CornerRadius::same(6);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = PANEL;
        widgets.noninteractive.weak_bg_fill = PANEL;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.corner_radius = round;
        for (state, fill) in [
            (&mut widgets.inactive, WIDGET),
            (&mut widgets.hovered, HOVER),
            (&mut widgets.active, PRESSED),
            (&mut widgets.open, HOVER),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = round;
            state.bg_stroke = Stroke::NONE;
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(80, 86, 110));
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    });
}

/// A drawn icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Step,
    Stop,
    Record,
    Undo,
    Redo,
    Move,
    Rotate,
    Scale,
    Globe,
    Magnet,
    Spark,
    Hand,
    Camera,
    Sun,
    Bulb,
    Cube,
    Sound,
    Text,
    Image,
    Prefab,
    Folder,
    Plus,
    Trash,
    Target,
    Dot,
    Screen,
}

/// Draws `icon` in `rect` (it is square; the smaller side counts) in
/// `color`: the vector drawing at exactly that many pixels, on whole pixels
/// so its edges stay sharp.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let size = rect.width().min(rect.height()).round().max(8.0);
    let texture = crate::vector::icon(painter.ctx(), icon, size as u16);
    let min = (rect.center() - Vec2::splat(size * 0.5)).round();
    painter.image(
        texture.id(),
        Rect::from_min_size(min, Vec2::splat(size)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        color,
    );
}

/// What kind of thing an entity is, from its components, as an icon and
/// its colour (for the hierarchy).
pub fn entity_icon(components: &[String], prefab: bool) -> (Icon, Color32) {
    let has = |name: &str| components.iter().any(|c| c == name);
    if prefab {
        return (Icon::Prefab, ACCENT);
    }
    if has("Camera") {
        (Icon::Camera, SKY)
    } else if has("DirectionalLight") {
        (Icon::Sun, SUN)
    } else if has("PointLight") || has("SpotLight") {
        (Icon::Bulb, SUN)
    } else if components.iter().any(|c| c.starts_with("Ui")) {
        (Icon::Text, Color32::from_rgb(120, 214, 200))
    } else if has("AudioSource") || has("AudioListener") {
        (Icon::Sound, Color32::from_rgb(244, 140, 186))
    } else if has("Sprite") {
        (Icon::Image, Color32::from_rgb(244, 160, 110))
    } else if has("ParticleEmitter") {
        (Icon::Spark, Color32::from_rgb(244, 160, 110))
    } else if has("Model") || has("Primitive") {
        (Icon::Cube, Color32::from_rgb(170, 178, 204))
    } else {
        (Icon::Dot, WEAK)
    }
}

/// A toolbar button: an icon and, if given, its text; `selected` shows it
/// held down; `tint` colours the icon (green for play). The accessible
/// name is `name` either way (tests and screen readers find it by it).
pub fn tool_button(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    show_text: bool,
    selected: bool,
    enabled: bool,
    tint: Option<Color32>,
) -> Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = show_text.then(|| {
        ui.painter()
            .layout_no_wrap(name.to_owned(), font.clone(), TEXT)
    });
    let icon_size = 15.0;
    let pad = vec2(8.0, 4.0);
    let text_width = galley.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let size = vec2(pad.x * 2.0 + icon_size + text_width, 26.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            ACCENT_SOFT
        } else if !enabled {
            Color32::TRANSPARENT
        } else if response.is_pointer_button_down_on() {
            PRESSED
        } else if response.hovered() {
            HOVER
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 6.0, fill);
        if selected {
            painter.rect_stroke(
                rect,
                6.0,
                Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let color = if !enabled {
            Color32::from_rgb(86, 92, 110)
        } else {
            tint.unwrap_or(if selected {
                TEXT
            } else {
                Color32::from_rgb(200, 205, 222)
            })
        };
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + pad.x + icon_size * 0.5, rect.center().y),
            Vec2::splat(icon_size),
        );
        paint(painter, icon_rect, icon, color);
        if let Some(galley) = galley {
            let text_color = if enabled {
                TEXT
            } else {
                Color32::from_rgb(96, 102, 120)
            };
            painter.galley(
                pos2(
                    icon_rect.right() + 6.0,
                    rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                text_color,
            );
        }
    }
    response
}

/// A group of tools on a slightly raised background, about `width`
/// pixels wide: in a wrapping bar that has less room left, the group starts
/// a new row (a group is never split).
pub fn tool_group<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    // The room left on this row (in a wrapping layout the available width
    // is the whole row's).
    let left = ui.max_rect().right() - ui.cursor().left();
    if left < width {
        ui.end_row();
    }
    egui::Frame::new()
        .fill(RAISED)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// A panel's title: quiet capitals-free text with room under it.
pub fn panel_title(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(14.5).color(TEXT));
}

/// The engine's mark, in its own colours, in `rect` (square).
pub fn paint_logo(painter: &Painter, rect: Rect) {
    let size = rect.width().min(rect.height()).round().max(16.0);
    let texture = crate::vector::logo(painter.ctx(), size as u16);
    let min = (rect.center() - Vec2::splat(size * 0.5)).round();
    painter.image(
        texture.id(),
        Rect::from_min_size(min, Vec2::splat(size)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
