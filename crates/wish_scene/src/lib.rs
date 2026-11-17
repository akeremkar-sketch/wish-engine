//! Scene and prefab documents.
//!
//! Scenes are plain JSON files that people, AI agents and tools all edit.
//! This crate reads them strictly and reports every problem with the file,
//! line and column it was found at, and writes them back in one canonical
//! form so that saving a file never produces noise in version control.
//!
//! - [`json`]: a strict JSON parser that remembers where every value came
//!   from, and the canonical writer.
//! - [`document`]: the [`SceneDocument`], the editable form of a scene, and
//!   [`load`], which reads and validates one against the registered types.
//! - [`edit`]: [`Transaction`]s of [`EditOp`]s, revisions, undo and redo,
//!   and the activity log of who changed what.
//! - [`instantiate`]: creating a document's entities in a world, each with a
//!   [`SceneId`].
//! - [`prefab`]: the [`PrefabLibrary`], [`load_prefab`], and expanding a
//!   scene's prefab instances into plain entities.
//!
//! A document is separate from the running world: it holds entities by scene
//! id and components as JSON, and is turned into a world only when a scene is
//! played.
//!
//! Design: `docs/design/M2-data-layer.md`.

pub mod document;
pub mod edit;
pub mod instantiate;
pub mod json;
pub mod prefab;
mod schema_check;

pub use document::{
    FORMAT, LoadResult, MAX_SCENE_ID_LEN, Overrides, SceneDocument, SceneEntity, is_valid_scene_id,
    load, load_with_prefabs,
};
pub use edit::{Action, Actor, Applied, EditOp, LogEntry, MAX_LOG, MAX_UNDO, Transaction};
pub use instantiate::{SCENE_ID, SceneEntities, SceneId};
pub use prefab::{ExpandedEntity, PREFAB_ROOT, PrefabLibrary, load_prefab};
