//! The tools, grouped as in the design note (§5).

use crate::param::{Kind, Param};
use crate::tool::Tool;

mod activity;
mod asset;
mod physics;
mod project;
mod replay;
mod scene;
mod script;
mod settings;
mod sim;
mod testing;
mod view;

pub(crate) use sim::parse_input;
pub use view::{IdPicture, Picture, PictureSource};

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
    project::DOCS_SEARCH,
    project::CHECK,
    scene::SCENE_GET,
    scene::ENTITY_QUERY,
    scene::ENTITY_GET,
    scene::EDIT_APPLY,
    scene::EDIT_HISTORY,
    asset::ASSET_LIST,
    asset::ASSET_INFO,
    asset::ASSET_MOVE,
    asset::ASSET_CREATE,
    sim::SIM_START,
    sim::SIM_STEP,
    sim::SIM_SNAPSHOT,
    sim::SIM_RESTORE,
    sim::SIM_STOP,
    sim::LOGS_GET,
    view::CAPTURE_FRAME,
    view::DESCRIBE_VIEW,
    physics::PHYSICS_QUERY,
    script::SCRIPT_BUILD,
    replay::REPLAY,
    testing::TEST_RUN,
    settings::PROJECT_SETTINGS,
    activity::ACTIVITY_GET,
];
