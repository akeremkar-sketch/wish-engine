//! Core types shared by every Wish Engine crate.
//!
//! - [`Entity`]: handle to an entity in a world.
//! - [`Text`]: handle to a string in a world's text table.
//! - [`Color`] and [`math`]: the value types components are built from.
//! - [`Diagnostic`]: the single error and warning format used across the engine.
//! - [`StableHasher`] and [`Hash128`]: hashing whose output never changes
//!   between runs, platforms or releases.
//! - [`suggest`]: "did you mean" helpers for writing diagnostic hints.
//!
//! This crate has no knowledge of worlds, schemas or rendering; everything else
//! in the engine depends on it.

pub mod color;
pub mod diagnostic;
pub mod entity;
pub mod hash;
pub mod math;
pub mod rng;
pub mod suggest;
pub mod text;

pub use color::Color;
pub use diagnostic::{Diagnostic, DiagnosticData, Location, Result, Severity};
pub use entity::Entity;
pub use hash::{Hash128, StableHasher, hash_bytes};
pub use rng::Rng;
pub use text::Text;
