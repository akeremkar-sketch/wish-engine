//! The in-game interface (design note M6 §7): menus, scores, buttons.
//!
//! An interface is made of entities. Every entity with a [`UiNode`] is a
//! box; its children (in the entity hierarchy) are laid out inside it, in a
//! row or a column, like CSS flexbox. A box can show a background colour,
//! text ([`UiText`]) or a picture ([`UiImage`]), and can be a button
//! ([`UiButton`]).
//!
//! Positions and sizes are in **interface pixels** of a fixed virtual screen
//! ([`UiSettings`], 1280 × 720 by default), with (0, 0) at the top left and
//! y growing down. The real picture is scaled to fit, so the window's size
//! never reaches the simulation: the same game lays out the same way on
//! every screen, and a tool can click at a known place.
//!
//! The engine works the layout out every tick and writes each box's place
//! into [`UiLayout`]; it writes what the pointer does to a button into
//! [`UiInteraction`] and sends [`ButtonClicked`] when one is clicked.

use wish_core::{AssetRef, Color, Entity, Text};
use wish_schema::{component, event, resource, schema};

/// How a length is measured.
#[schema]
pub enum UiUnit {
    /// Decided by the layout: as big as the content (text, children), or
    /// stretched by the parent.
    Auto,
    /// Interface pixels.
    Px,
    /// Percent of the parent box (of the screen for a box without a parent
    /// box).
    Percent,
}

/// A width or height: `{ "unit": "Px", "value": 200 }`.
#[schema]
pub struct UiLength {
    /// Auto, Px or Percent.
    pub unit: UiUnit,
    /// The number of pixels or percent; ignored for Auto.
    #[schema(range = 0.0..=100000.0)]
    pub value: f32,
}

/// Which way a box lays out its children.
#[schema]
pub enum UiDirection {
    /// Top to bottom.
    Column,
    /// Left to right.
    Row,
}

/// Where children go along the direction of the layout (with free space).
#[schema]
pub enum UiJustify {
    /// Packed at the start (top of a column, left of a row).
    Start,
    /// Packed in the middle.
    Center,
    /// Packed at the end.
    End,
    /// The first at the start, the last at the end, the free space evenly
    /// between them.
    SpaceBetween,
    /// The free space evenly around every child.
    SpaceAround,
}

/// Where children go across the direction of the layout.
#[schema]
pub enum UiAlign {
    /// Stretched to fill the box (unless they have a size).
    Stretch,
    /// At the start (left in a column, top in a row).
    Start,
    /// In the middle.
    Center,
    /// At the end.
    End,
}

/// Whether a box takes part in its parent's layout.
#[schema]
pub enum UiPosition {
    /// Laid out with its siblings; `offset` then moves it from there.
    Relative,
    /// Taken out of the layout and placed at `offset` from its parent's top
    /// left corner (from the screen's, for a box without a parent box).
    Absolute,
}

/// A box of the interface. Its children (in the entity hierarchy) with a
/// `UiNode` are laid out inside it, **in sibling order**: in a scene file
/// that is the children's `order` (a spawn or reparent operation takes
/// `order`), and by id where none is given, so give `order` to put a
/// title above its buttons.
#[component]
pub struct UiNode {
    /// The box's width.
    pub width: UiLength,
    /// The box's height.
    pub height: UiLength,
    /// Space inside the box, around its content: top, right, bottom, left
    /// (as in CSS).
    #[schema(unit = "px")]
    pub padding: [f32; 4],
    /// Space outside the box: top, right, bottom, left.
    #[schema(unit = "px")]
    pub margin: [f32; 4],
    /// Children in a column (default) or a row.
    pub direction: UiDirection,
    /// Where children go along the direction.
    pub justify: UiJustify,
    /// Where children go across the direction.
    pub align: UiAlign,
    /// Space between children.
    #[schema(unit = "px", range = 0.0..=10000.0)]
    pub gap: f32,
    /// How much of the parent's free space this box takes, compared with
    /// its siblings (0: none, keeps its own size).
    #[schema(range = 0.0..=1000.0)]
    pub grow: f32,
    /// Relative (laid out with siblings) or Absolute (placed at `offset`).
    pub position: UiPosition,
    /// Moves the box: right and down, from where the layout puts it
    /// (Relative) or from the parent's top left corner (Absolute).
    #[schema(unit = "px")]
    pub offset: [f32; 2],
    /// The box's background; transparent shows nothing.
    #[schema(default = Color::TRANSPARENT)]
    pub background: Color,
    /// Whether the box and everything in it is shown (and can be clicked).
    #[schema(default = true)]
    pub visible: bool,
    /// For a box without a parent box: boxes with a higher layer are drawn
    /// over (and clicked before) lower ones.
    #[schema(range = -1000..=1000)]
    pub layer: i32,
}

/// How lines of text line up in their box.
#[schema]
pub enum UiTextAlign {
    /// Along the left edge.
    Left,
    /// Centred.
    Center,
    /// Along the right edge.
    Right,
}

/// Text in a box (on an entity with a [`UiNode`]). Without a set size, the
/// box is as big as the text; with a width, long lines wrap.
#[component]
pub struct UiText {
    /// The text. A line break (`\n`) starts a new line.
    pub text: Text,
    /// The font file (`.ttf` or `.otf` in the project); empty for the
    /// engine's font (Roboto).
    pub font: AssetRef,
    /// The height of the font.
    #[schema(unit = "px", range = 1.0..=1000.0, default = 24.0)]
    pub size: f32,
    /// The colour of the letters (linear RGB, and alpha).
    #[schema(default = Color::WHITE)]
    pub color: Color,
    /// How lines line up.
    pub align: UiTextAlign,
    /// Whether lines longer than the box's width wrap onto the next line.
    #[schema(default = true)]
    pub wrap: bool,
}

/// A picture filling a box (on an entity with a [`UiNode`]). Give the box a
/// size: a picture does not size its box.
#[component]
pub struct UiImage {
    /// The picture file (`.png` in the project).
    pub image: AssetRef,
    /// Multiplies the picture's colours (white: unchanged).
    #[schema(default = Color::WHITE)]
    pub tint: Color,
}

/// Makes a box (an entity with a [`UiNode`]) clickable. The engine writes
/// what the pointer does to it into [`UiInteraction`] and sends
/// [`ButtonClicked`] when it is clicked (pressed and released over it).
#[component]
pub struct UiButton {
    /// A disabled button does not react to the pointer.
    #[schema(default = true)]
    pub enabled: bool,
}

/// What the pointer does to a button, written by the engine every tick.
#[component]
pub struct UiInteraction {
    /// The pointer is over the button.
    pub hovered: bool,
    /// The button is held down (pressed over it, not released yet).
    pub pressed: bool,
    /// How many times it has been clicked.
    pub clicks: u32,
    /// The tick of the last click (0: never).
    pub last_click_tick: u64,
}

/// Where the layout put a box, written by the engine every tick: interface
/// pixels from the top left of the virtual screen.
#[component]
pub struct UiLayout {
    /// The box's left and top edge.
    #[schema(unit = "px")]
    pub position: [f32; 2],
    /// The box's width and height.
    #[schema(unit = "px")]
    pub size: [f32; 2],
    /// Whether it is shown (it and all its parent boxes are visible).
    pub shown: bool,
    /// Its place in drawing order: higher is drawn later (on top).
    pub order: u32,
}

/// A button was clicked.
#[event]
pub struct ButtonClicked {
    /// The button's entity.
    pub entity: Entity,
}

/// The interface's virtual screen.
#[resource]
pub struct UiSettings {
    /// Its width.
    #[schema(unit = "px", range = 16.0..=16384.0, default = 1280.0)]
    pub width: f32,
    /// Its height.
    #[schema(unit = "px", range = 16.0..=16384.0, default = 720.0)]
    pub height: f32,
}

impl UiSettings {
    /// The virtual screen's size, made sane (at least 16 × 16 pixels).
    pub fn size(&self) -> [f32; 2] {
        let clean = |v: f32| if v.is_finite() { v.max(16.0) } else { 16.0 };
        [clean(self.width), clean(self.height)]
    }

    /// How the virtual screen fits a picture `width` × `height` pixels:
    /// scaled as big as fits whole, centred. Returns the scale (picture
    /// pixels per interface pixel) and where the screen's top left corner
    /// is in the picture.
    pub fn fit(&self, width: f32, height: f32) -> (f32, [f32; 2]) {
        let [w, h] = self.size();
        let scale = (width / w).min(height / h).max(0.0);
        (
            scale,
            [(width - w * scale) * 0.5, (height - h * scale) * 0.5],
        )
    }

    /// The interface position of picture pixel (`x`, `y`) of a picture
    /// `width` × `height` pixels (the inverse of [`UiSettings::fit`]).
    pub fn from_picture(&self, width: f32, height: f32, x: f32, y: f32) -> [f32; 2] {
        let (scale, [left, top]) = self.fit(width, height);
        if scale <= 0.0 {
            return [0.0, 0.0];
        }
        [(x - left) / scale, (y - top) / scale]
    }
}

/// The pointer (mouse, touch, or a tool's `pointer` input), written by the
/// engine before the systems run.
#[resource]
pub struct Pointer {
    /// Whether there is a pointer over the screen.
    pub present: bool,
    /// Where it is: interface pixels from the top left of the virtual
    /// screen.
    #[schema(unit = "px")]
    pub position: [f32; 2],
    /// Whether its button (the left mouse button) is down.
    pub held: bool,
    /// Whether the button went down this tick.
    pub pressed: bool,
    /// Whether the button went up this tick.
    pub released: bool,
}
