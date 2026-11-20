//! The licenses, shipped with every engine package and every exported game
//! (M9, M12 release pipeline). `THIRD_PARTY_LICENSES.md` at the repository's
//! root is made by `tools/third_party.py` from the dependency tree (plus the
//! third-party works that are not crates, such as the engine's font); a test
//! checks that it names every library in `Cargo.lock`.

/// The engine's own license under MIT (the engine is MIT OR Apache-2.0).
pub const MIT: &str = include_str!("../../../LICENSE-MIT");

/// The engine's own license under the Apache License 2.0.
pub const APACHE: &str = include_str!("../../../LICENSE-APACHE");

/// The third-party license notices.
pub const THIRD_PARTY: &str = include_str!("../../../THIRD_PARTY_LICENSES.md");
