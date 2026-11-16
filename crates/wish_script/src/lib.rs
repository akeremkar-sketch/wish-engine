//! The game logic host: runs untrusted WebAssembly modules (ADR-011).
//!
//! Game code is a core Wasm module that imports only from the `wish` module
//! the engine provides: no files, no network, no clock (ADR-022). Every call
//! gets a **fresh instance**, so a module cannot keep state between calls:
//! all game state lives in the world, snapshots are complete and code can be
//! replaced between two ticks without losing anything (ADR-006).
//!
//! A call is bounded by **fuel** (a count of executed instructions, so a
//! runaway loop stops at the same instruction on every run: determinism is
//! kept), by a memory limit and by a stack limit. Whatever the guest does —
//! loop forever, trap, panic, ask for gigabytes — the call returns a
//! [`Diagnostic`](wish_core::Diagnostic) and the engine keeps running.
//!
//! Design: `docs/design/M5-oyun-mantigi.md` §3 (ABI) and §6 (sandbox).

pub mod build;
mod engine;
mod failure;
mod game;
mod host;
mod module;
pub mod source_check;

pub use engine::{Limits, ScriptEngine};
pub use game::{
    ColumnSpec, GameModule, Manifest, ModuleType, ParamSpec, Role, ScriptLog, ScriptRuntime,
    SystemSpec,
};
pub use host::HostServices;
pub use module::{CallReport, GuestLog, ScriptModule};

/// The ABI version this host speaks (design note §3). A module reports its
/// own through the `wish_abi_version` export.
pub const ABI_VERSION: i32 = 1;
