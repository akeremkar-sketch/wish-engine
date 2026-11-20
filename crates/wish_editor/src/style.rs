//! The editor's look (M9): one palette, one set of spacings and the icons
//! the panels use. The icons are drawn as shapes (the interface font has no
//! symbols for play, pause or a camera), so they look the same everywhere
//! and need no image files.

use egui::epaint::{CornerRadius, Margin, Shadow, Stroke};
use egui::{
    Color32, Context, FontFamily, FontId, Painter, Rect, Response, Role, Sense, TextStyle, Ui,
    Vec2, WidgetInfo, pos2, vec2,
};

/// The editor's colours and corners (M10a): a theme. The engine's default
/// is Atelier: neutral graphite greys, one calm blue for what is chosen and
/// what to do next, green only for running, red only for recording and
/// errors, small corners (redesigned before 1.0:
/// `docs/design/M12-editor-redesign.md`); theme packages bring others. Every
/// panel reads the theme through [`pal`], so a new theme needs no rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The canvas behind everything.
    pub backdrop: [u8; 4],
    /// Panels.
    pub panel: Color32,
    /// Raised areas inside panels: cards, groups of tools, windows.
    pub raised: Color32,
    /// Widgets at rest.
    pub widget: Color32,
    /// Widgets under the pointer.
    pub hover: Color32,
    /// Widgets being pressed.
    pub pressed: Color32,
    /// Thin lines between areas.
    pub line: Color32,
    /// The outline of a widget under the pointer.
    pub hover_line: Color32,
    /// Text.
    pub text: Color32,
    /// Quieter text: hints, paths, numbers that are not the point.
    pub weak: Color32,
    /// What to do next (Create, the agent), never decoration.
    pub accent: Color32,
    /// What is chosen (a selected row, a held tool, a chosen card).
    pub select: Color32,
    /// The chosen thing's background, quiet.
    pub accent_soft: Color32,
    /// Running: play.
    pub go: Color32,
    /// Recording, deleting, errors.
    pub red: Color32,
    /// Warnings.
    pub amber: Color32,
    /// Lights and the sun.
    pub sun: Color32,
    /// Cameras.
    pub sky: Color32,
    /// Text fields and other sunken areas.
    pub field: Color32,
    /// Icons on tool buttons at rest.
    pub icon: Color32,
    /// Corners of widgets, windows and menus, in pixels.
    pub radius_widget: u8,
    pub radius_window: u8,
    pub radius_menu: u8,
}

/// The engine's own theme, Atelier (the same as
/// `packages/wish.theme.atelier/atelier.json`, which a test checks).
pub const ATELIER: Theme = Theme {
    backdrop: [28, 29, 32, 255],
    panel: Color32::from_rgb(37, 38, 42),
    raised: Color32::from_rgb(44, 45, 50),
    widget: Color32::from_rgb(44, 45, 50),
    hover: Color32::from_rgb(51, 52, 58),
    pressed: Color32::from_rgb(58, 59, 66),
    line: Color32::from_rgb(54, 55, 61),
    hover_line: Color32::from_rgb(74, 76, 85),
    text: Color32::from_rgb(225, 226, 230),
    weak: Color32::from_rgb(155, 157, 166),
    accent: Color32::from_rgb(61, 111, 208),
    select: Color32::from_rgb(91, 143, 234),
    accent_soft: Color32::from_rgb(47, 78, 133),
    go: Color32::from_rgb(95, 191, 110),
    red: Color32::from_rgb(217, 96, 90),
    amber: Color32::from_rgb(226, 176, 74),
    sun: Color32::from_rgb(226, 192, 106),
    sky: Color32::from_rgb(143, 179, 233),
    field: Color32::from_rgb(28, 29, 32),
    icon: Color32::from_rgb(183, 185, 193),
    radius_widget: 3,
    radius_window: 4,
    radius_menu: 4,
};

thread_local! {
    static CURRENT: std::cell::Cell<Theme> = const { std::cell::Cell::new(ATELIER) };
}

/// The theme in use (on this thread: each editor sets its own before its
/// frame, as with the language).
pub fn pal() -> Theme {
    CURRENT.with(std::cell::Cell::get)
}

/// Text drawn on the accent colour: dark on a light accent (a yellow, a
/// pale blue), white on a dark one, so it stays readable in every theme.
pub fn on_accent() -> Color32 {
    let accent = pal().accent;
    let light = 0.299 * f32::from(accent.r())
        + 0.587 * f32::from(accent.g())
        + 0.114 * f32::from(accent.b());
    if light > 170.0 {
        Color32::from_rgb(16, 16, 16)
    } else {
        Color32::WHITE
    }
}

/// The accent as text or as an icon on a panel: a dark accent (made for
/// filled buttons with white text) is lightened toward white so it stays
/// readable on the panels; a light accent is used as it is. Worked out from
/// the theme, so a theme file needs no key of its own for it.
pub fn accent_text() -> Color32 {
    let accent = pal().accent;
    let light = 0.299 * f32::from(accent.r())
        + 0.587 * f32::from(accent.g())
        + 0.114 * f32::from(accent.b());
    if light >= 150.0 {
        return accent;
    }
    let toward = |channel: u8| {
        let c = f32::from(channel);
        (c + (255.0 - c) * 0.45).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(toward(accent.r()), toward(accent.g()), toward(accent.b()))
}

impl Theme {
    /// The backdrop as a colour (the page behind cards).
    pub fn panel_backdrop(&self) -> Color32 {
        Color32::from_rgb(self.backdrop[0], self.backdrop[1], self.backdrop[2])
    }
}

/// Uses `theme` from now on (on this thread).
pub fn set_theme(theme: Theme) {
    CURRENT.with(|current| current.set(theme));
}

/// The colour keys of a theme file, and how to set each.
const COLOR_KEYS: &[&str] = &[
    "accent",
    "accent_soft",
    "amber",
    "backdrop",
    "field",
    "go",
    "hover",
    "hover_line",
    "icon",
    "line",
    "panel",
    "pressed",
    "raised",
    "red",
    "select",
    "sky",
    "sun",
    "text",
    "weak",
    "widget",
];

fn set_color(theme: &mut Theme, key: &str, color: Color32) {
    match key {
        "accent" => theme.accent = color,
        "accent_soft" => theme.accent_soft = color,
        "amber" => theme.amber = color,
        "backdrop" => theme.backdrop = [color.r(), color.g(), color.b(), 255],
        "field" => theme.field = color,
        "go" => theme.go = color,
        "hover" => theme.hover = color,
        "hover_line" => theme.hover_line = color,
        "icon" => theme.icon = color,
        "line" => theme.line = color,
        "panel" => theme.panel = color,
        "pressed" => theme.pressed = color,
        "raised" => theme.raised = color,
        "red" => theme.red = color,
        "select" => theme.select = color,
        "sky" => theme.sky = color,
        "sun" => theme.sun = color,
        "text" => theme.text = color,
        "weak" => theme.weak = color,
        _ => theme.widget = color,
    }
}

/// Reads a theme file: `{ "format": 1, "name": "...", "colors": { "accent":
/// "#ff6b2c", ... }, "radius": { "widget": 3, "window": 5, "menu": 4 } }`.
/// Colours it leaves out are Atelier's. The error says what is wrong and
/// how to fix it.
pub fn parse_theme(source: &str) -> Result<(String, Theme), (String, String)> {
    let value: serde_json::Value = serde_json::from_str(source).map_err(|error| {
        (
            format!("not valid JSON: {error}"),
            "Fix the JSON.".to_owned(),
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err((
            "not a JSON object".to_owned(),
            "Start it with { \"format\": 1 }.".to_owned(),
        ));
    };
    for key in object.keys() {
        if !["colors", "format", "name", "radius"].contains(&key.as_str()) {
            return Err((
                format!("it has no setting '{key}'"),
                wish_core::suggest::unknown_name_hint(key, &["colors", "format", "name", "radius"]),
            ));
        }
    }
    if value["format"] != 1 {
        return Err((
            "it needs \"format\": 1".to_owned(),
            "Add \"format\": 1.".to_owned(),
        ));
    }
    let name = value["name"].as_str().unwrap_or("").trim().to_owned();
    if name.is_empty() || name.chars().count() > 40 {
        return Err((
            "it needs a \"name\" of 1 to 40 characters".to_owned(),
            "Write it like \"name\": \"Dusk\".".to_owned(),
        ));
    }
    let mut theme = ATELIER;
    for (key, color) in value["colors"].as_object().into_iter().flatten() {
        if !COLOR_KEYS.contains(&key.as_str()) {
            return Err((
                format!("it has no colour '{key}'"),
                wish_core::suggest::unknown_name_hint(key, COLOR_KEYS),
            ));
        }
        let parsed = color.as_str().and_then(|text| {
            let hex = text.strip_prefix('#')?;
            (hex.len() == 6).then_some(())?;
            let byte = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
            Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
        });
        let Some(parsed) = parsed else {
            return Err((
                format!("the colour '{key}' is not #rrggbb"),
                "Write colours like \"#ff6b2c\".".to_owned(),
            ));
        };
        set_color(&mut theme, key, parsed);
    }
    for (key, radius) in value["radius"].as_object().into_iter().flatten() {
        let Some(radius) = radius
            .as_u64()
            .filter(|r| *r <= 16)
            .and_then(|r| u8::try_from(r).ok())
        else {
            return Err((
                format!("the radius '{key}' is not a whole number from 0 to 16"),
                "Write it like \"widget\": 3.".to_owned(),
            ));
        };
        match key.as_str() {
            "widget" => theme.radius_widget = radius,
            "window" => theme.radius_window = radius,
            "menu" => theme.radius_menu = radius,
            other => {
                return Err((
                    format!("it has no radius '{other}'"),
                    wish_core::suggest::unknown_name_hint(other, &["menu", "widget", "window"]),
                ));
            }
        }
    }
    Ok((name, theme))
}

/// Puts the engine's look on a context: colours, rounded corners, room
/// around widgets, text sizes.
pub fn apply(context: &Context) {
    let theme = pal();
    context.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(12.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(12.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
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
        visuals.panel_fill = pal().panel;
        visuals.window_fill = pal().raised;
        visuals.window_stroke = Stroke::new(1.0, pal().line);
        visuals.window_corner_radius = CornerRadius::same(theme.radius_window);
        visuals.menu_corner_radius = CornerRadius::same(theme.radius_menu);
        visuals.window_shadow = Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        visuals.popup_shadow = Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        visuals.faint_bg_color = pal().raised;
        visuals.extreme_bg_color = theme.field;
        visuals.code_bg_color = theme.field;
        visuals.hyperlink_color = accent_text();
        visuals.warn_fg_color = pal().amber;
        visuals.error_fg_color = pal().red;
        visuals.selection.bg_fill = pal().accent_soft;
        visuals.selection.stroke = Stroke::new(1.0, pal().select);
        visuals.weak_text_color = Some(pal().weak);
        visuals.indent_has_left_vline = true;
        visuals.collapsing_header_frame = true;
        visuals.slider_trailing_fill = true;

        let round = CornerRadius::same(theme.radius_widget);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = pal().panel;
        widgets.noninteractive.weak_bg_fill = pal().panel;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal().line);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal().text);
        widgets.noninteractive.corner_radius = round;
        for (state, fill) in [
            (&mut widgets.inactive, pal().widget),
            (&mut widgets.hovered, pal().hover),
            (&mut widgets.active, pal().pressed),
            (&mut widgets.open, pal().hover),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = round;
            // Widgets carry a thin outline, as in professional tools: a
            // field or a button reads as one without a loud fill.
            state.bg_stroke = Stroke::new(1.0, pal().line);
            state.fg_stroke = Stroke::new(1.0, pal().text);
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(1.0, theme.hover_line);
        widgets.active.bg_stroke = Stroke::new(1.0, pal().select);
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
    Chat,
    /// The engine's wishing wand (the AI mode).
    Wish,
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
    // Quiet icons: only prefabs (blue), cameras and lights carry a colour,
    // so the tree reads as a list, not a palette.
    if prefab {
        return (Icon::Prefab, pal().sky);
    }
    if has("Camera") {
        (Icon::Camera, pal().sky)
    } else if has("DirectionalLight") {
        (Icon::Sun, pal().sun)
    } else if has("PointLight") || has("SpotLight") {
        (Icon::Bulb, pal().sun)
    } else if components.iter().any(|c| c.starts_with("Ui")) {
        (Icon::Text, pal().weak)
    } else if has("AudioSource") || has("AudioListener") {
        (Icon::Sound, pal().weak)
    } else if has("Sprite") {
        (Icon::Image, pal().weak)
    } else if has("ParticleEmitter") {
        (Icon::Spark, pal().weak)
    } else if has("Model") || has("Primitive") {
        (Icon::Cube, pal().weak)
    } else {
        (Icon::Dot, pal().weak)
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
            .layout_no_wrap(name.to_owned(), font.clone(), pal().text)
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
            pal().accent_soft
        } else if !enabled {
            Color32::TRANSPARENT
        } else if response.is_pointer_button_down_on() {
            pal().pressed
        } else if response.hovered() {
            pal().hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, f32::from(pal().radius_widget), fill);
        let color = if !enabled {
            Color32::from_rgb(86, 92, 110)
        } else {
            tint.unwrap_or(if selected { pal().text } else { pal().icon })
        };
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + pad.x + icon_size * 0.5, rect.center().y),
            Vec2::splat(icon_size),
        );
        paint(painter, icon_rect, icon, color);
        if let Some(galley) = galley {
            let text_color = if enabled {
                pal().text
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
        .fill(pal().field)
        .stroke(Stroke::new(1.0, pal().line))
        .corner_radius(CornerRadius::same(pal().radius_widget))
        .inner_margin(Margin::same(1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// A panel's title: plain words in the text colour, as a professional
/// tool's panel tab reads (not shouted in capitals).
pub fn panel_title(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.5).color(pal().text));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_theme_is_the_atelier_package() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/wish.theme.atelier/atelier.json");
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let parsed = parse_theme(&text);
        assert_eq!(parsed, Ok(("Atelier".to_owned(), ATELIER)));
    }

    #[test]
    fn a_wrong_theme_file_says_what_and_how() {
        let wrong = [
            r##"{ "format": 1, "name": "X", "colours": {} }"##,
            r##"{ "format": 1, "name": "X", "colors": { "acent": "#ffffff" } }"##,
            r##"{ "format": 1, "name": "X", "colors": { "accent": "orange" } }"##,
            r##"{ "format": 1, "name": "X", "radius": { "widget": 40 } }"##,
            r##"{ "format": 2, "name": "X" }"##,
            r##"{ "format": 1 }"##,
        ];
        for text in wrong {
            let (message, hint) = parse_theme(text).expect_err(text);
            assert!(!message.is_empty() && !hint.is_empty(), "{text}");
        }
        let (_, hint) =
            parse_theme(r##"{ "format": 1, "name": "X", "colors": { "acent": "#ffffff" } }"##)
                .expect_err("a typo");
        assert!(hint.contains("accent"), "{hint}");
        // Colours left out are Atelier's.
        let (_, theme) =
            parse_theme(r##"{ "format": 1, "name": "X", "colors": { "accent": "#010203" } }"##)
                .expect("a small theme");
        assert_eq!(theme.accent, Color32::from_rgb(1, 2, 3));
        assert_eq!(theme.text, ATELIER.text);
    }
}
