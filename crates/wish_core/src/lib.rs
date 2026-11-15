//! Core types shared by every Wish Engine crate.
//!
//! - [`Entity`]: handle to an entity in a world.
//! - [`Diagnostic`]: the single error and warning format used across the engine.
//! - [`StableHasher`] and [`Hash128`]: hashing whose output never changes
//!   between runs, platforms or releases.
//! - [`suggest`]: "did you mean" helpers for writing diagnostic hints.
//!
//! This crate has no knowledge of worlds, schemas or rendering; everything else
//! in the engine depends on it.

pub mod diagnostic;
pub mod entity;
pub mod hash;
pub mod suggest;

pub use diagnostic::{Diagnostic, Location, Result, Severity};
pub use entity::Entity;
pub use hash::{Hash128, StableHasher, hash_bytes};
