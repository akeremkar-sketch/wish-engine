//! Flat pictures in the world (design note M6 §9): the 2D subset. A 2D game
//! is a scene seen through an orthographic `Camera`, made of entities with a
//! [`Sprite`]; physics stays the same, with `RigidBody` locks keeping things
//! in the XY plane (ADR-020).
//!
//! Sprites are not lit: they show their picture's colours as they are. They
//! are drawn after the 3D scene, hidden by solid things in front of them,
//! back to front by `layer`, then by distance.
//!
//! A sprite can show the frames of a sprite sheet (`*.sheet.json`) in turn:
//! a [`SpriteAnimator`] beside it names the sheet and the clip, and the
//! engine moves the clip on every tick (design note
//! `docs/design/1.1-sprites.md`).

use wish_core::{AssetRef, Color, Text};
use wish_schema::component;

/// A picture (or a plain rectangle) standing in its entity's XY plane,
/// facing +Z, sized by its pixels and `pixels_per_unit` and then by the
/// entity's scale.
#[component]
pub struct Sprite {
    /// The picture (a PNG in the project); empty for a plain rectangle of
    /// `color`. With a `SpriteAnimator` that names a sheet, the sheet's
    /// picture is shown instead.
    pub image: AssetRef,
    /// The part of the picture shown, in its pixels: x and y of the top left
    /// corner, width and height (a sprite sheet's frame). A width or height
    /// of 0 shows the whole picture. Without a picture, the width and height
    /// give the rectangle's size in pixels (0: 100 × 100). With a
    /// `SpriteAnimator` that names a sheet, the frame it shows is used
    /// instead.
    #[schema(unit = "px")]
    pub rect: [f32; 4],
    /// The tint: multiplies the picture's colours (linear RGB, and alpha).
    /// White shows the picture as it is; a white or grey picture takes the
    /// colour (one character, many player colours).
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
    /// The size the sprite is drawn at, in picture pixels (turned into
    /// meters by `pixels_per_unit`, like the picture's own size): width and
    /// height. 0 keeps the size of the part of the picture shown. Unlike
    /// the entity's scale, it can stretch a nine-slice picture (`slice`)
    /// without stretching its corners.
    #[schema(unit = "px")]
    pub size: [f32; 2],
    /// Nine-slice borders, in picture pixels: left, top, right, bottom.
    /// When the sprite is drawn larger or smaller than its picture (`size`),
    /// the four corners keep their size, the edges stretch along their
    /// length and the middle stretches both ways: a panel or a button frame
    /// of any size from one small picture. All 0: the picture stretches as
    /// a whole.
    #[schema(unit = "px")]
    pub slice: [f32; 4],
    /// The order within its layer: of two sprites (or tilemaps) on the same
    /// layer, the higher order is drawn over the lower, before height (a
    /// y-sorted layer) and distance decide.
    #[schema(range = -32768..=32767)]
    pub order: i32,
}

impl Sprite {
    /// Whether the sprite is drawn as nine slices.
    pub fn is_sliced(&self) -> bool {
        self.slice.iter().any(|border| *border > 0.0)
    }
}

/// Plays the clips of a sprite sheet on the entity's [`Sprite`]: the
/// engine moves `time` on every tick (after game code) while `playing`,
/// and writes which of the clip's frames shows into `frame`. The sprite
/// shows the sheet's picture, cut to that frame.
///
/// Everything that decides the future is here, in the world, so snapshots,
/// forks and replays hold the animation exactly; the frame is worked out
/// from the tick's time, never from the wall clock.
#[component]
pub struct SpriteAnimator {
    /// The sprite sheet (`*.sheet.json`) whose frames the sprite shows.
    pub sheet: AssetRef,
    /// The clip that plays, by its name in the sheet. Empty: no clip; the
    /// sprite shows the sheet's frame number `frame`, still (a sheet as a
    /// set of pictures). To switch clips from game code, use
    /// [`SpriteAnimator::play`], which starts the new clip at its start.
    pub clip: Text,
    /// Whether time moves. A clip that does not loop stops on its last
    /// frame and sets this to false (and the engine sends
    /// `AnimationFinished`).
    #[schema(default = true)]
    pub playing: bool,
    /// Playback speed: 2 is twice as fast, 0.5 half as fast, -1 plays
    /// backwards.
    #[schema(range = -10.0..=10.0, default = 1.0)]
    pub speed: f32,
    /// Seconds into the clip. The engine advances it every tick while
    /// `playing` (wrapped to the clip's length when it loops); write it to
    /// jump.
    #[schema(unit = "s")]
    pub time: f32,
    /// The frame shown. With a clip: its place in the clip's list of frames
    /// (0 is the clip's first), written by the engine from `time`. Without
    /// a clip: the sheet's own frame number, written by you.
    pub frame: u32,
}

impl SpriteAnimator {
    /// Plays the clip `clip` (a name from `text("walk")`): when it is not
    /// the clip already playing, it starts at its beginning; either way it
    /// plays. Calling it every tick with the same clip lets the clip run.
    pub fn play(&mut self, clip: Text) {
        if self.clip != clip {
            self.clip = clip;
            self.time = 0.0;
            self.frame = 0;
        }
        self.playing = true;
    }
}

/// A named sorting layer (design note `docs/design/1.1-editor-2d.md` §3):
/// gives the sprites and tilemaps whose `layer` is `layer` a name, for
/// people, agents and `describe_view`, and says how they are sorted. Put one
/// on an entity of its own for each layer a game names ("ground", "actors",
/// "roofs"); without any, layers are plain numbers, as they always were.
///
/// Drawing order is `layer` (lower first), then `order` within it, then (on
/// a y-sorted layer) height, then distance.
#[component]
pub struct SortingLayer {
    /// The layer's name.
    pub name: Text,
    /// The layer number it names (`Sprite.layer`, `Tilemap.layer`).
    #[schema(range = -1000..=1000)]
    pub layer: i32,
    /// Sorts the layer's sprites by height, for top-down games: a sprite
    /// lower on the map (its anchor at a smaller y) is drawn over one higher
    /// up, and every one of them over the layer's tilemaps. (A `Tilemap`
    /// with `y_sort` does the same for its own layer.)
    pub y_sort: bool,
}
