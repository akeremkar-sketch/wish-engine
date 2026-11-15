//! Rendering for Wish Engine.
//!
//! - [`gpu`]: choosing an adapter (hardware or software) and opening a
//!   device; every GPU error becomes a [`wish_core::Diagnostic`] instead of a
//!   crash.
//! - [`capture`]: drawing into a texture and reading it back without a
//!   window.
//! - [`image`]: the read-back image, PNG writing and tolerant comparison for
//!   golden-image tests.
//!
//! Rendering is outside the determinism boundary: nothing here reads from or
//! writes to the simulation.
//!
//! Design: `docs/design/M3-ilk-piksel.md`.

pub mod capture;
pub mod gpu;
pub mod image;

pub use capture::{MAX_CAPTURE_SIZE, draw_probe};
pub use gpu::{AdapterSummary, Gpu, GpuOptions};
pub use image::{ImageDiff, RgbaImage, Tolerance};
