//! Fonts, text layout and glyphs (M6 WP7).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_text::{Align, Font, Fonts, layout, rasterize};

fn roboto() -> &'static Font {
    Font::default_font().unwrap()
}

#[test]
fn one_line_has_the_font_height_and_a_width() {
    let laid = layout(roboto(), "Hello", 32.0, None, true, Align::Left);
    assert_eq!(laid.lines, 1);
    assert_eq!(laid.glyphs.len(), 5);
    assert!(laid.width > 50.0 && laid.width < 100.0, "{}", laid.width);
    // The size is the font's height: ascent to descent.
    assert!((laid.height - 32.0).abs() < 0.01, "{}", laid.height);
    // Glyphs go left to right on one baseline.
    assert!(
        laid.glyphs
            .windows(2)
            .all(|w| w[1].x > w[0].x && w[1].y == w[0].y)
    );
}

#[test]
fn the_same_text_lays_out_bit_for_bit_the_same() {
    // Recorded on 2026-11-16 (Windows); CI checks Linux gives the same bits.
    let laid = layout(roboto(), "Wish Engine: 1.0", 32.0, None, true, Align::Left);
    assert_eq!(
        laid,
        layout(roboto(), "Wish Engine: 1.0", 32.0, None, true, Align::Left)
    );
    assert_eq!(laid.width.to_bits(), RECORDED_WIDTH_BITS, "{}", laid.width);
}

const RECORDED_WIDTH_BITS: u32 = 1127332949;

#[test]
fn long_lines_wrap_between_words_and_line_breaks_start_lines() {
    let one = layout(roboto(), "alpha beta", 20.0, None, true, Align::Left);
    let narrow = one.width - 5.0;
    let wrapped = layout(
        roboto(),
        "alpha beta",
        20.0,
        Some(narrow),
        true,
        Align::Left,
    );
    assert_eq!(wrapped.lines, 2);
    assert!(wrapped.width <= narrow);
    // The second word starts the second line at the left edge.
    let second = wrapped.glyphs[5];
    assert!(second.x < 2.0 && second.y > wrapped.glyphs[0].y);
    // Without wrapping it stays one line, however narrow.
    let kept = layout(
        roboto(),
        "alpha beta",
        20.0,
        Some(narrow),
        false,
        Align::Left,
    );
    assert_eq!(kept.lines, 1);
    // A word longer than the box is not broken.
    let long = layout(roboto(), "alphabeta", 20.0, Some(10.0), true, Align::Left);
    assert_eq!(long.lines, 1);
    // \n always breaks; \r is ignored.
    let two = layout(roboto(), "a\r\nb", 20.0, None, true, Align::Left);
    assert_eq!((two.lines, two.glyphs.len()), (2, 2));
    // Wrapping at the measured width keeps one line (measure, then draw).
    let again = layout(
        roboto(),
        "alpha beta",
        20.0,
        Some(one.width),
        true,
        Align::Left,
    );
    assert_eq!(again.lines, 1);
}

#[test]
fn alignment_moves_lines_within_the_box() {
    let left = layout(roboto(), "ab", 20.0, Some(200.0), true, Align::Left);
    let center = layout(roboto(), "ab", 20.0, Some(200.0), true, Align::Center);
    let right = layout(roboto(), "ab", 20.0, Some(200.0), true, Align::Right);
    let first = |l: &wish_text::TextLayout| l.glyphs[0].x;
    assert!(first(&left) < 2.0);
    assert!((first(&center) - (200.0 - left.width) / 2.0).abs() < 0.01);
    assert!((first(&right) - (200.0 - left.width)).abs() < 0.01);
}

#[test]
fn glyphs_rasterize_and_spaces_do_not() {
    let font = roboto();
    let a = layout(font, "A", 40.0, None, true, Align::Left).glyphs[0];
    let bitmap = rasterize(font, a.id, 40.0).unwrap();
    assert!(bitmap.width > 10 && bitmap.height > 20);
    assert!(bitmap.top < 0, "the letter stands above the baseline");
    assert!(bitmap.coverage.contains(&255));
    assert_eq!(
        layout(font, " ", 40.0, None, true, Align::Left)
            .glyphs
            .len(),
        0
    );
    assert!(rasterize(font, a.id, 0.0).is_none());
    assert!(rasterize(font, a.id, f32::NAN).is_none());
}

#[test]
fn bad_sizes_and_empty_text_give_nothing() {
    for size in [0.0, -3.0, f32::NAN, f32::INFINITY] {
        let laid = layout(roboto(), "x", size, None, true, Align::Left);
        assert!(laid.glyphs.is_empty() || laid.width.is_finite());
    }
    let empty = layout(roboto(), "", 20.0, None, true, Align::Left);
    assert_eq!((empty.lines, empty.glyphs.len()), (1, 0));
    assert!(
        layout(roboto(), "x", 20.0, Some(f32::NAN), true, Align::Center)
            .width
            .is_finite()
    );
}

#[test]
fn hostile_font_files_are_diagnostics_never_crashes() {
    let error = Font::parse(b"not a font at all".to_vec(), "fonts/bad.ttf").unwrap_err();
    assert_eq!(error.code, "font_invalid");
    assert!(!error.hint.is_empty());
    assert!(Font::parse(Vec::new(), "empty.ttf").is_err());
    // Truncated and corrupted copies of a real font: each either reads or is
    // refused, and what reads can be laid out and drawn without a panic.
    let real = wish_text::DEFAULT_FONT;
    for cut in [12usize, 100, 1000, 30_000, real.len() / 2] {
        if let Ok(font) = Font::parse(real[..cut].to_vec(), "cut.ttf") {
            let laid = layout(&font, "Hostile", 30.0, Some(50.0), true, Align::Center);
            for glyph in laid.glyphs {
                let _ = rasterize(&font, glyph.id, 30.0);
            }
        }
    }
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..40 {
        let mut bytes = real.to_vec();
        for _ in 0..64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = (state as usize) % bytes.len();
            bytes[at] = (state >> 32) as u8;
        }
        if let Ok(font) = Font::parse(bytes, "mutated.ttf") {
            let laid = layout(
                &font,
                "Hostile text 0123",
                24.0,
                Some(80.0),
                true,
                Align::Left,
            );
            for glyph in laid.glyphs.iter().take(8) {
                let _ = rasterize(&font, glyph.id, 24.0);
            }
        }
    }
}

#[test]
fn fonts_fall_back_to_the_engine_font() {
    let mut fonts = Fonts::default();
    assert_eq!(fonts.get("").unwrap().name(), "Roboto");
    assert_eq!(fonts.get("fonts/missing.ttf").unwrap().name(), "Roboto");
    let copy = Font::parse(wish_text::DEFAULT_FONT.to_vec(), "fonts/copy.ttf").unwrap();
    fonts.insert("fonts/copy.ttf", copy);
    assert!(fonts.contains("fonts/copy.ttf"));
    assert_eq!(
        fonts.get("fonts/copy.ttf").unwrap().name(),
        "fonts/copy.ttf"
    );
}
