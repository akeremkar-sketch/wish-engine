//! The tools, grouped as in the design note (§5).

use crate::param::{Kind, Param};
use crate::tool::Tool;

mod project;
mod scene;

/// How much a tool tells: `summary` (the default, cheapest), `normal`, `full`.
pub(crate) const DETAIL: Param = Param::optional(
    "detail",
    Kind::Choice(&["summary", "normal", "full"]),
    "How much to return: summary (short, the default), normal, or full.",
)
.or("\"summary\"");

/// Every tool, in catalogue order.
pub(crate) const CATALOG: &[Tool] = &[
    project::PROJECT_INFO,
    project::SCHEMA_GET,
    project::CHECK,
    scene::SCENE_GET,
    scene::ENTITY_QUERY,
    scene::ENTITY_GET,
    scene::EDIT_APPLY,
    scene::EDIT_HISTORY,
];
