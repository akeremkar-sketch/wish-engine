//! Scenario tests: `test_run` (design note M5 §10).

use serde_json::{Map, Value};
use wish_core::Result;
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const TEST_RUN: Tool = Tool {
    name: "test_run",
    title: "Run scenario tests",
    description: "Runs the project's scenario tests (tests/*.test.json): each starts a play session on a scene with a seed and goes through its steps: run ticks (frames, with input held, or until a condition), expect a condition (same language as entity_query's where; with \"entity\" it is checked on that entity, without it it holds if the resources or some entity satisfy it, so say \"no coin is left\" as count(Coin) == 0), capture a picture and compare it with a golden image. `passed` is false if any test fails; the first failing step of each test is explained with its step number, the tick, and the values the condition read. Pictures are drawn on the software adapter, the same on every machine. update_golden saves the pictures as the golden images (after you have looked at them). Tests are deterministic: a failure is a real change in how the game plays.",
    params: &[
        Param::optional(
            "paths",
            Kind::StringList,
            "Run only these test files (project-relative, such as tests/win.test.json); all when left out.",
        ),
        Param::optional(
            "update_golden",
            Kind::Boolean,
            "Write the pictures the tests take as their golden images instead of comparing.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "Run the tests without writing anything (no golden images, no failed pictures).",
        ),
    ],
    mutates: true,
    run: test_run,
};

fn test_run(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    workspace.test_run(
        &args.strings("paths"),
        args.flag("update_golden"),
        args.flag("dry_run"),
    )
}
