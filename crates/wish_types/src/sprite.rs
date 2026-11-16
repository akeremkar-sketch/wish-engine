//! Flat pictures in the world (design note M6 §9): the 2D subset. A 2D game
//! is a scene seen through an orthographic `Camera`, made of entities with a
//! [`Sprite`]; physics stays the same, with `RigidBody` locks keeping things
//! in the XY plane (ADR-020).
//!
//! Sprites are not lit: they show their picture's colours as they are. They
//! are drawn after the 3D scene, hidden by solid things in front of them,
//! back to front by `layer`, then by distance.

use wish_core::{AssetRef, Color};
use wish_schema::component;

/// A picture (or a plain rectangle) standing in its entity's XY plane,
/// facing +Z, sized by its pixels and `pixels_per_unit` and then by the
/// entity's scale.
#[component]
pub struct Sprite {
    /// The picture (a PNG in the project); empty for a plain rectangle of
    /// `color`.
    pub image: AssetRef,
    /// The part of the picture shown, in its pixels: x and y of the top left
    /// corner, width and height (a sprite sheet's frame). A width or height
    /// of 0 shows the whole picture. Without a picture, the width and height
    /// give the rectangle's size in pixels (0: 100 × 100).
    #[schema(unit = "px")]
    pub rect: [f32; 4],
    /// Multiplies the picture's colours (linear RGB, and alpha).
    #[schema(default = Color::WHITE)]
    pub color: Color,
    /// How many picture pixels make one meter.
    #[schema(range = 0.001..=100000.0, default = 100.0)]
    pub pixels_per_unit: f32,
    /// The point of the sprite at the entity's position: [0, 0] is the
    /// bottom left corner, [1, 1] the top right, [0.5, 0.5] the middle.
    #[schema(default = [0.5, 0.5])]
    pub anchor: [f32; 2],
    /// Mirrors the picture left to right.
    pub flip_x: bool,
    /// Mirrors the picture top to bottom.
    pub flip_y: bool,
    /// Sprites with a higher layer are drawn over lower ones; within a
    /// layer, nearer ones over farther ones.
    #[schema(range = -1000..=1000)]
    pub layer: i32,
    /// Smooth scaling (true) or sharp pixels (false, for pixel art).
    #[schema(default = true)]
    pub smooth: bool,
}
