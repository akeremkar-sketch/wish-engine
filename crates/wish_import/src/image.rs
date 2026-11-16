//! Pictures on their own (not inside a model): PNG files for the in-game
//! interface.

use wish_core::{Diagnostic, Result};

use crate::Texture;

/// The largest picture read, in pixels on a side.
pub const MAX_IMAGE_SIDE: u32 = 4096;

fn invalid(name: &str, why: String) -> Diagnostic {
    Diagnostic::error(
        "image_invalid",
        format!("The picture '{name}' cannot be used: {why}."),
        "Use an 8- or 16-bit PNG of at most 4096x4096 pixels; re-export it from an image editor.",
    )
    .at_file(name)
}

/// Decodes a PNG file into 8-bit RGBA (sRGB, rows top to bottom). Any
/// colour type and bit depth is accepted; `name` names it in diagnostics.
/// A file that is not a PNG, is damaged or too big is `image_invalid`.
pub fn load_png(bytes: &[u8], name: &str) -> Result<Texture> {
    std::panic::catch_unwind(|| decode(bytes, name))
        .map_err(|_| invalid(name, "the PNG reader failed on it".to_owned()))?
}

fn decode(bytes: &[u8], name: &str) -> Result<Texture> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| invalid(name, format!("it is not a readable PNG ({error})")))?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(invalid(
            name,
            format!("it is {width}x{height} pixels; the most is {MAX_IMAGE_SIDE} on a side"),
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| invalid(name, "its size cannot be worked out".to_owned()))?;
    let mut buffer = vec![0; size];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|error| invalid(name, format!("it is damaged ({error})")))?;
    let data = &buffer[..frame.buffer_size()];
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(invalid(
                name,
                "its palette could not be expanded".to_owned(),
            ));
        }
    };
    let count = (width * height) as usize;
    if frame.bit_depth != png::BitDepth::Eight || data.len() < count * channels {
        return Err(invalid(
            name,
            "its pixel data is not 8-bit after decoding".to_owned(),
        ));
    }
    let mut rgba = Vec::with_capacity(count * 4);
    for pixel in data.chunks_exact(channels).take(count) {
        match channels {
            1 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            3 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            _ => rgba.extend_from_slice(pixel),
        }
    }
    Ok(Texture {
        width,
        height,
        rgba,
    })
}
