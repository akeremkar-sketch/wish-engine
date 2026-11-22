//! The tools, grouped as in the design note (§5).

use crate::param::{Kind, Param};
use crate::tool::Tool;

mod activity;
mod asset;
mod checkpoint;
mod design;
mod engines;
mod export;
mod net;
mod packages;
mod permissions;
mod physics;
mod project;
mod replay;
mod scene;
mod script;
mod settings;
mod sim;
mod testing;
mod tilemap;
mod view;

pub use design::{
    BRIEF_FIELDS, CardField, DESIGN_FILE, card_fields, read_design, summary as design_summary,
};
pub use engines::{act as engines_act, act_in as engines_act_in};
pub(crate) use net::{not_networked as net_not_networked, unknown_player as net_unknown_player};
pub(crate) use sim::{named_players, parse_input, parse_inputs};
pub use view::{FlatPicture, IdPicture, Picture, PictureSource};

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
    checkpoint::CHECKPOINT,
    permissions::PERMISSIONS,
    permissions::PROPOSALS,
    export::PROJECT_EXPORT,
    design::GAME_DESIGN,
    packages::PACKAGES,
    engines::ENGINES,
    tilemap::TILEMAP_EDIT,
    net::NET,
];
