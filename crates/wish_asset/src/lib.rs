//! Asset files: discovery, meta files, reference checking and moves.
//!
//! An *asset reference* is a project-relative path with `/` separators and an
//! optional `#sub` object (`"assets/models/crate.glb#Mesh0"`). Components
//! hold them in fields of type [`wish_core::AssetRef`], so the schema tells
//! tools exactly where references are, and this crate can find, check and
//! rewrite them in scene and prefab files without guessing.
//!
//! - [`index`]: which asset files exist ([`AssetIndex`]), and whether a
//!   reference points at one.
//! - [`meta`]: the `<asset>.meta.json` file next to an asset, which holds
//!   its import settings.
//! - [`refs`]: finding asset references in scene and prefab files, and
//!   checking that their files exist, with positions.
//! - [`moves`]: moving an asset (or a folder) and updating every reference
//!   in the same operation (`wen mv`).
//!
//! Importing assets (glTF, textures) is M3; watching files for changes is
//! part of hot reloading (M2 WP7).
//!
//! Design: `docs/design/M2-veri-katmani.md` §5.

pub mod index;
pub mod meta;
pub mod moves;
pub mod refs;

pub use index::{AssetIndex, is_document};
pub use meta::{AssetMeta, META_FORMAT, meta_path, parse_meta, read_meta, write_meta};
pub use moves::{DocumentEdit, MovePlan, apply_move, plan_move};
pub use refs::{RefUse, check_source, refs_in_value};
