//! The Wish Engine control plane.
//!
//! One API for everything that changes or looks at a project: the command
//! line (`wen call`, `wen do` and the shortcut commands), the MCP server
//! (`wen mcp`) and, later, the editor are thin clients of
//! [`Workspace::call`]. A capability that is not here cannot exist in any of
//! them (invariant 4, ADR-009).
//!
//! - [`Workspace`]: one open project and what the tools share.
//! - [`catalog`]: every tool with its parameters ([`Param`]) and
//!   documentation; the MCP input schemas come from the same table
//!   ([`input_schema`]) as the parser and the command-line flags
//!   ([`from_flags`]).
//! - Every answer is `{ "ok": true, ... }` or `{ "ok": false, "error":
//!   Diagnostic }`, with keys in sorted order so that the same call gives
//!   the same bytes everywhere.
//!
//! Design: `docs/design/M4-control-plane.md`.

mod activity;
mod audio;
mod checkpoints;
mod concepts;
pub mod crash;
pub mod docs;
mod documents;
pub mod engines;
pub mod expr;
pub mod licenses;
mod live;
pub mod ops;
pub mod package_index;
pub mod packages;
pub mod param;
pub mod pe;
pub mod policy;
pub mod project;
pub mod render;
mod replays;
pub mod scaffold;
mod scenarios;
mod script;
mod sessions;
pub mod tool;
mod tools;
pub mod types;
mod ui;
pub mod workspace;

pub use param::{Args, Kind, Param, from_flags, input_schema};
pub use project::{PROJECT_FILE, PROJECT_FORMAT, ProjectConfig, parse_project, read_project};
pub use render::{base64, base64_decode};
pub use script::PointerAt;
pub use tool::{Tool, catalog};
pub use tools::{
    BRIEF_FIELDS, CardField, DESIGN_FILE, IdPicture, Picture, PictureSource, card_fields,
    design_summary, engines_act, engines_act_in, read_design,
};
pub use workspace::{Workspace, WorkspaceOptions, envelope, find_tool, parse_actor};
