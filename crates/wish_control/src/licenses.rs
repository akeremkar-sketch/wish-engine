//! The licenses of the libraries the engine uses, shipped with every
//! exported game (M9). `THIRD_PARTY_LICENSES.md` at the repository's root is
//! made by `tools/third_party.py` from the dependency tree; a test checks
//! that it names every library in `Cargo.lock`.

/// The third-party license notices.
pub const THIRD_PARTY: &str = include_str!("../../../THIRD_PARTY_LICENSES.md");
