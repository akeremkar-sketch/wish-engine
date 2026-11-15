//! Images read back from the GPU: PNG writing and tolerant comparison for
//! golden-image tests.

use wish_core::{Diagnostic, Result};

/// An 8-bit RGBA image, rows top to bottom, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

/// How two images differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageDiff {
    /// The largest difference in any channel of any pixel (0–255).
    pub max_channel: u8,
    /// How many pixels differ at all.
    pub differing_pixels: u64,
    /// `differing_pixels` as a fraction of all pixels.
    pub differing_fraction: f64,
}

/// Golden-image tolerance: how far a rendering may be from the reference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Largest allowed channel difference for a pixel to count as equal.
    pub channel: u8,
    /// Largest allowed fraction of pixels that differ by more than `channel`.
    pub fraction: f64,
}

impl RgbaImage {
    /// The pixel at `(x, y)` as `[r, g, b, a]`.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        let bytes = self.pixels.get(at..at + 4)?;
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// The image as PNG bytes.
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let failed = |error: png::EncodingError| {
            Diagnostic::error(
                "png_encode",
                format!("Could not encode the image as PNG: {error}."),
                "This is a bug in wish_render; please report it.",
            )
        };
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(failed)?;
            writer.write_image_data(&self.pixels).map_err(failed)?;
        }
        Ok(out)
    }

    /// Reads a PNG (8-bit RGBA, as [`RgbaImage::to_png`] writes).
    pub fn from_png(bytes: &[u8]) -> Result<Self> {
        let failed = |what: String| {
            Diagnostic::error(
                "png_decode",
                format!("Could not read the PNG: {what}."),
                "Golden images must be 8-bit RGBA PNGs as written by the engine; regenerate it.",
            )
        };
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().map_err(|e| failed(e.to_string()))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| failed("it is too large".to_owned()))?;
        let mut pixels = vec![0; size];
        let info = reader
            .next_frame(&mut pixels)
            .map_err(|e| failed(e.to_string()))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(failed(format!(
                "it is {:?} {:?}, not 8-bit RGBA",
                info.color_type, info.bit_depth
            )));
        }
        pixels.truncate(info.buffer_size());
        Ok(Self {
            width: info.width,
            height: info.height,
            pixels,
        })
    }

    /// How this image differs from `other` (same size required), counting a
    /// pixel as different when any channel differs by more than `channel`.
    pub fn diff(&self, other: &Self, channel: u8) -> Option<ImageDiff> {
        if (self.width, self.height) != (other.width, other.height) {
            return None;
        }
        let mut max_channel = 0;
        let mut differing_pixels = 0;
        let (ours, _) = self.pixels.as_chunks::<4>();
        let (theirs, _) = other.pixels.as_chunks::<4>();
        for (a, b) in ours.iter().zip(theirs) {
            let worst = a
                .iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y))
                .max()
                .unwrap_or(0);
            max_channel = max_channel.max(worst);
            if worst > channel {
                differing_pixels += 1;
            }
        }
        let total = u64::from(self.width) * u64::from(self.height);
        Some(ImageDiff {
            max_channel,
            differing_pixels,
            differing_fraction: if total == 0 {
                0.0
            } else {
                differing_pixels as f64 / total as f64
            },
        })
    }

    /// Whether this image matches `golden` within `tolerance`.
    pub fn matches(&self, golden: &Self, tolerance: Tolerance) -> bool {
        self.diff(golden, tolerance.channel)
            .is_some_and(|diff| diff.differing_fraction <= tolerance.fraction)
    }
}
