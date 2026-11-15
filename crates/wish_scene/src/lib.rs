//! Scene and prefab documents.
//!
//! Scenes are plain JSON files that people, AI agents and tools all edit.
//! This crate reads them strictly and reports every problem with the file,
//! line and column it was found at, and writes them back in one canonical
//! form so that saving a file never produces noise in version control.
//!
//! - [`json`]: a strict JSON parser that remembers where every value came
//!   from, and the canonical writer.
//!
//! Design: `docs/design/M2-veri-katmani.md`.

pub mod json;
