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
//! - [`material`]: material files (`*.mat.json`), which are both: a
//!   checked, canonical document, and an asset other files refer to.
//! - [`sheet`]: sprite sheets (`*.sheet.json`), documents and assets like
//!   material files: a picture cut into frames, and named clips.
//! - [`refs`]: finding asset references in scene and prefab files, and
//!   checking that their files exist, with positions.
//! - [`moves`]: moving an asset (or a folder) and updating every reference
//!   in the same operation (`wen mv`).
//!
//! Importing assets (glTF, textures) is M3; watching files for changes is
//! part of hot reloading (M2 WP7).
//!
//! Design: `docs/design/M2-data-layer.md` §5.

pub mod hot;
pub mod index;
pub mod material;
pub mod meta;
pub mod moves;
pub mod project;
pub mod refs;
pub mod sheet;

pub use hot::{HotScene, Reload, Watcher};
pub use project::{FileKind, Project, ProjectFile, load_project};

pub use index::{AssetIndex, is_document, is_document_asset, is_too_large, read_document_asset};
pub use material::{
    MATERIAL_FORMAT, MATERIAL_SUFFIX, MAX_MATERIAL_BYTES, MaterialDocument, check_material,
    is_material, material_schema, material_too_large, parse_material, read_material_file,
};
pub use meta::{AssetMeta, META_FORMAT, meta_path, parse_meta, read_meta, write_meta};
pub use moves::{DocumentEdit, MovePlan, apply_move, plan_move};
pub use refs::{RefUse, check_source, refs_in_material, refs_in_sheet, refs_in_value};
pub use sheet::{
    MAX_SHEET_BYTES, SHEET_FORMAT, SHEET_SUFFIX, Sheet, SheetClip, SheetDocument, check_sheet,
    is_sheet, parse_sheet, read_sheet_file, sheet_too_large,
};
