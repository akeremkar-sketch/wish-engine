//! Replays: `replay` (design note M5 §9, §11).

use serde_json::{Map, Value, json};
use wish_core::Result;
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const REPLAY: Tool = Tool {
    name: "replay",
    title: "Record and replay a run",
    description: "A replay is a run of the game as a file (replays/<name>.replay.jsonl): the scene, the seed, what the player did and the world's hash after every tick. record: start recording a session that is at tick 0 (before its first sim_step). save: write what was recorded under a name. run: play a saved replay back in a new session and compare every tick with the recording; `passed` means the game plays out exactly as it did, otherwise diverged_at is the first tick that differs and the session is left open there to look at. Running replays after changing the code answers \"does the game still play the same?\". Games are deterministic, so a replay of 10 000 ticks passes bit for bit.",
    params: &[
        Param::required(
            "action",
            Kind::Choice(&["record", "save", "run"]),
            "record, save or run.",
        ),
        Param::optional(
            "session",
            Kind::String,
            "For record and save: the session (optional when only one is open).",
        ),
        Param::optional(
            "name",
            Kind::String,
            "For save and run: the replay's name, 1 to 40 of a-z 0-9 _ - (such as \"jump_test\").",
        ),
        Param::optional(
            "overwrite",
            Kind::Boolean,
            "For save: replace a replay of that name.",
        ),
        Param::optional(
            "dry_run",
            Kind::Boolean,
            "For save: report what would be written without writing the file.",
        ),
        Param::optional(
            "keep",
            Kind::Boolean,
            "For run: leave the replay's session open when it passed too.",
        ),
    ],
    mutates: true,
    run: replay,
};

fn replay(workspace: &mut Workspace, args: &Args, _: &Actor) -> Result<Map<String, Value>> {
    let action = args.str("action").unwrap_or_default();
    let named = || -> Result<&str> {
        args.str("name").ok_or_else(|| {
            wish_core::Diagnostic::error(
                "missing_parameter",
                format!("replay {action} needs a name."),
                "Pass \"name\": \"jump_test\".",
            )
            .at_path("/name")
        })
    };
    match action {
        "record" => {
            let id = workspace.session_id(args.str("session"))?;
            workspace.replay_record(&id)?;
            let mut out = Map::new();
            out.insert("session".to_owned(), json!(id));
            out.insert("recording".to_owned(), json!(true));
            Ok(out)
        }
        "save" => {
            let id = workspace.session_id(args.str("session"))?;
            workspace.replay_save(&id, named()?, args.flag("overwrite"), args.flag("dry_run"))
        }
        _ => workspace.replay_run(named()?, args.flag("keep")),
    }
}
