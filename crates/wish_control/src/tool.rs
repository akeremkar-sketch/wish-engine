//! The tool catalogue: every operation the control plane offers, with its
//! parameters and documentation. The MCP server lists it, the command line
//! calls into it, and the parity test checks that each tool has an example.

use serde_json::{Map, Value};
use wish_core::Result;
use wish_scene::Actor;

use crate::param::{Args, Param};
use crate::workspace::Workspace;

/// What a tool does when called.
pub type Run = fn(&mut Workspace, &Args, &Actor) -> Result<Map<String, Value>>;

/// One tool.
#[derive(Clone, Copy)]
pub struct Tool {
    /// The name, `noun_verb` (`entity_query`).
    pub name: &'static str,
    /// A short title for people.
    pub title: &'static str,
    /// What it does and when to use it, for the agent choosing a tool.
    pub description: &'static str,
    /// Its parameters.
    pub params: &'static [Param],
    /// Whether it changes files or state (and so takes `dry_run`).
    pub mutates: bool,
    /// The implementation.
    pub run: Run,
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Every tool, in a fixed order.
pub fn catalog() -> &'static [Tool] {
    crate::tools::CATALOG
}
