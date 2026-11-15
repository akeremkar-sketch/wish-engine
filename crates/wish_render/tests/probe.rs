//! The WP1 spike: draw without a window on the software adapter, read the
//! image back, and compare it with a golden image.
//!
//! `UPDATE_GOLDEN=1` rewrites the golden image instead of comparing (review
//! the diff before committing). Every run also writes what it drew to
//! `$CARGO_TARGET_TMPDIR/captures/`, so CI can keep it as an artifact.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use wish_render::{Gpu, GpuOptions, RgbaImage, Tolerance, draw_probe};

/// Measured in WP1 between WARP (Windows) and lavapipe (Linux) on this
/// probe: the largest channel difference was 1, on 5.3% of the pixels, and
/// no edge pixel differed. A pixel counts as different above 2; at most
/// 0.5% may differ. Re-measured when lighting arrives (WP3); see the design
/// note §6 and §14.
const TOLERANCE: Tolerance = Tolerance {
    channel: 2,
    fraction: 0.005,
};

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"))
}

fn keep(name: &str, image: &RgbaImage, gpu: &Gpu) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("captures");
    std::fs::create_dir_all(&folder).unwrap();
    let adapter = gpu.adapter();
    let tag = format!(
        "{}-{}",
        std::env::consts::OS,
        adapter.backend.to_lowercase()
    );
    std::fs::write(
        folder.join(format!("{name}-{tag}.png")),
        image.to_png().unwrap(),
    )
    .unwrap();
}

fn check_golden(name: &str, image: &RgbaImage) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, image.to_png().unwrap()).unwrap();
        return;
    }
    let golden = RgbaImage::from_png(&std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "no golden image at {}; run with UPDATE_GOLDEN=1",
            path.display()
        )
    }))
    .unwrap();
    let diff = image.diff(&golden, TOLERANCE.channel).expect("same size");
    assert!(
        image.matches(&golden, TOLERANCE),
        "{name} differs from its golden image: {diff:?} (tolerance {TOLERANCE:?})"
    );
}

#[test]
fn the_software_adapter_draws_without_a_window() {
    let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
    let adapter = gpu.adapter();
    eprintln!("adapter: {adapter:?}");
    let image = draw_probe(&gpu, 256, 256).unwrap();
    keep("probe", &image, &gpu);

    assert_eq!((image.width, image.height), (256, 256));
    // The corners are background: linear 0.02 / 0.03 stored as sRGB, about
    // 39 / 48. The middle is inside the triangle, with all three colours.
    let background = image.pixel(2, 2).unwrap();
    assert!(
        (35..=44).contains(&background[0]) && (44..=52).contains(&background[2]),
        "{background:?}"
    );
    let middle = image.pixel(128, 150).unwrap();
    assert!(middle.iter().take(3).all(|&c| c > 60), "{middle:?}");
    assert_eq!(middle[3], 255);
    // Near the top corner the red vertex dominates.
    let top = image.pixel(128, 40).unwrap();
    assert!(top[0] > top[1] && top[0] > top[2], "{top:?}");

    check_golden("probe", &image);
}

#[test]
fn drawing_twice_gives_the_same_image() {
    let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
    let first = draw_probe(&gpu, 64, 48).unwrap();
    let second = draw_probe(&gpu, 64, 48).unwrap();
    assert_eq!(first, second);
}

#[test]
fn impossible_sizes_are_refused() {
    let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
    for (width, height) in [(0, 10), (10, 0), (9000, 10)] {
        let problem = draw_probe(&gpu, width, height).unwrap_err();
        assert_eq!(problem.code, "invalid_capture_size");
    }
}

/// Measures how two captures differ, for setting tolerances: run with
/// `WISH_COMPARE_A` and `WISH_COMPARE_B` set to two PNG paths and
/// `--ignored --nocapture`.
#[test]
#[ignore = "a measuring tool, run by hand"]
fn measure_the_difference_between_two_captures() {
    let read = |var: &str| {
        let path = std::env::var(var).unwrap_or_else(|_| panic!("set {var}"));
        RgbaImage::from_png(&std::fs::read(path).unwrap()).unwrap()
    };
    let (a, b) = (read("WISH_COMPARE_A"), read("WISH_COMPARE_B"));
    for channel in [0, 1, 2, 4, 8] {
        let diff = a.diff(&b, channel).expect("same size");
        println!(
            "channel > {channel}: {} pixels ({:.4}%), largest channel difference {}",
            diff.differing_pixels,
            diff.differing_fraction * 100.0,
            diff.max_channel
        );
    }
}

#[test]
fn png_round_trips() {
    let image = RgbaImage {
        width: 3,
        height: 2,
        pixels: (0..24).collect(),
    };
    let back = RgbaImage::from_png(&image.to_png().unwrap()).unwrap();
    assert_eq!(back, image);
    let mut other = image.clone();
    other.pixels[5] += 3;
    let diff = image.diff(&other, 2).unwrap();
    assert_eq!((diff.max_channel, diff.differing_pixels), (3, 1));
}
