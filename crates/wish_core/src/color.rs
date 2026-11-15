//! Colors.

/// A color with linear (not sRGB-encoded) red, green and blue, and alpha.
///
/// Plain data: `repr(C)`, 16 bytes, alignment 4. Its JSON form is
/// `[r, g, b, a]`. Components are not clamped; values above 1 are valid and
/// mean "brighter than white" (HDR).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Color {
    /// Red, linear.
    pub r: f32,
    /// Green, linear.
    pub g: f32,
    /// Blue, linear.
    pub b: f32,
    /// Opacity: 0 is fully transparent, 1 is fully opaque.
    pub a: f32,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    /// An opaque color from linear red, green and blue.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// A color from linear red, green, blue and alpha.
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// The components as `[r, g, b, a]`.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// A color from `[r, g, b, a]`.
    pub const fn from_array(rgba: [f32; 4]) -> Self {
        let [r, g, b, a] = rgba;
        Self { r, g, b, a }
    }
}

impl Default for Color {
    /// The default color is opaque white.
    fn default() -> Self {
        Self::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_four_f32s() {
        assert_eq!((size_of::<Color>(), align_of::<Color>()), (16, 4));
    }

    #[test]
    fn array_round_trip_and_constants() {
        let color = Color::rgba(0.1, 0.2, 0.3, 0.4);
        assert_eq!(Color::from_array(color.to_array()), color);
        assert_eq!(Color::rgb(0.1, 0.2, 0.3).a, 1.0);
        assert_eq!(Color::default(), Color::WHITE);
        assert_eq!(Color::TRANSPARENT.to_array(), [0.0; 4]);
        assert_eq!(Color::BLACK.to_array(), [0.0, 0.0, 0.0, 1.0]);
    }
}
