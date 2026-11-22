//! 2D lights and line-of-sight vision (design note
//! `docs/design/1.1-lights-2d.md`).
//!
//! In a 2D scene (the X–Y plane, seen along −Z) walls block sight and light:
//! the solid tiles of a `Tilemap`'s colliding layers, and entities with an
//! [`Occluder`] beside their `Collider`. Game code asks what can be seen
//! (`wish_sdk::sight`), online play sends a player only what its character
//! can see (`NetVisibility::Sight`), [`Light2D`]s light sprites and tilemaps
//! with shadows, and a [`Vision2D`] covers everything a character cannot
//! see with darkness.
//!
//! All of it is off until a scene asks for it: 2D lighting needs
//! `Lighting2D.enabled`, and darkness needs a `Vision2D`.

use wish_core::Color;
use wish_schema::{component, resource};

/// 2D lighting for the whole scene: when enabled, sprites and tilemaps are
/// lit by the ambient level plus every [`Light2D`] (particles and the
/// interface are not lit). Turn the ambient down to make a room dark, for
/// example when the lights are sabotaged.
#[resource]
pub struct Lighting2D {
    /// Whether sprites and tilemaps are lit. Off, they show their colours as
    /// they are and `Light2D`s do nothing.
    pub enabled: bool,
    /// The light everything gets without any `Light2D` (linear RGB): white
    /// shows sprites as they are, black leaves only what lights reach.
    #[schema(default = Color::rgb(0.2, 0.2, 0.2))]
    pub ambient: Color,
}

/// A light in the X–Y plane at its entity's position: it lights sprites and
/// tilemaps within `radius`, fading smoothly to nothing there, and (with
/// `shadows`) only where walls do not block it. Needs
/// `Lighting2D.enabled`.
#[component]
pub struct Light2D {
    /// The light's colour (linear RGB).
    #[schema(default = Color::WHITE)]
    pub color: Color,
    /// How strong it is at its centre: 1 adds its colour once; 0 is off.
    #[schema(range = 0.0..=100.0, default = 1.0)]
    pub intensity: f32,
    /// How far it reaches.
    #[schema(unit = "m", range = 0.0..=1000.0, default = 5.0)]
    pub radius: f32,
    /// Whether walls (tilemap walls and `Occluder`s) cast shadows: the light
    /// reaches only what its centre can see.
    #[schema(default = true)]
    pub shadows: bool,
}

/// What a character can see, drawn as darkness over everything else: a
/// circle of `radius` around the entity, cut by walls, with a soft edge.
/// With several, what any of them sees is shown. Put it on the player's
/// own character (online, enable only the local player's).
#[component]
pub struct Vision2D {
    /// Whether it is shown.
    #[schema(default = true)]
    pub enabled: bool,
    /// How far the character sees.
    #[schema(unit = "m", range = 0.0..=1000.0, default = 6.0)]
    pub radius: f32,
    /// How wide the soft edge at the end of the radius is.
    #[schema(unit = "m", range = 0.0..=1000.0, default = 1.5)]
    pub softness: f32,
    /// The darkness over what cannot be seen (linear RGB); its alpha is how
    /// dark: 1 hides everything, 0 nothing. With several visions, the
    /// first's (in entity order).
    #[schema(default = Color::rgba(0.02, 0.02, 0.04, 0.9))]
    pub darkness: Color,
}

/// Makes the entity's `Collider` block sight and 2D light: its outline in
/// the X–Y plane is a wall for `can_see`, for `NetVisibility::Sight` and for
/// the shadows of `Light2D`s. A door: enabled while closed.
#[component]
pub struct Occluder {
    /// Whether it blocks now.
    #[schema(default = true)]
    pub enabled: bool,
}
