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
//!
//! A document is separate from the running world: it holds entities by scene
//! id and components as JSON, and is turned into a world only when a scene is
//! played.
//!
//! Design: `docs/design/M2-veri-katmani.md`.

pub mod document;
pub mod edit;
pub mod json;
mod schema_check;

pub use document::{
    FORMAT, LoadResult, MAX_SCENE_ID_LEN, SceneDocument, SceneEntity, is_valid_scene_id, load,
};
pub use edit::{Action, Actor, Applied, EditOp, LogEntry, MAX_LOG, MAX_UNDO, Transaction};
