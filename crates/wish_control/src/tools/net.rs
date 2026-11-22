//! Online play: `net` (release 1.1, package 14; design note
//! `docs/design/1.1-net-tool.md` §3).

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Result};
use wish_scene::Actor;

use crate::param::{Args, Kind, Param};
use crate::tool::Tool;
use crate::workspace::Workspace;

pub(crate) const NET: Tool = Tool {
    name: "net",
    title: "Online play",
    description: "Hosts an online match in a session and plays it with bots over a simulated network (or real UDP), to test an online game without people. host starts a networked session (its world is the host's; players seats, bots, network {latency_ms, jitter_ms, loss, duplicate, reorder, seed}, or port for real UDP; auto_start starts once every seat is ready); join adds a player (address \"host:port\" or \"discover\"); bots adds bots (drive input: held input, inputs by seat in step; drive game: the bot's own game code); start and end are the host's lobby; step runs host and players together (the same as sim_step on the session; inputs by seat); status shows the lobby, seats, round trips, bytes and the host's counts, with player one player's machine; leave and stop end a player or the match. Over the simulator the same calls give the same match every time. entity_query, entity_get, capture_frame and describe_view take player to look at what a player's machine has; replay save writes the match.",
    params: &[
        Param::optional(
            "action",
            Kind::Choice(&[
                "host", "join", "bots", "start", "end", "status", "step", "leave", "stop",
            ]),
            "What to do.",
        )
        .or("\"status\""),
        Param::optional(
            "session",
            Kind::String,
            "The networked session (optional when only one session is open).",
        ),
        Param::optional(
            "scene",
            Kind::String,
            "host: the scene (default: the project's).",
        ),
        Param::optional(
            "seed",
            Kind::Integer {
                min: 0,
                max: i64::MAX,
            },
            "host: the random seed of the host's world and of the match's secrets (room code, keys, roles over the simulator).",
        )
        .or("0"),
        Param::optional(
            "players",
            Kind::Integer { min: 1, max: 16 },
            "host: the seats the match is for (the host's own player included when host_plays); auto_start waits for them. To give a step input by seat (sim_step's players), use inputs.",
        )
        .or("2"),
        Param::optional(
            "bots",
            Kind::Integer { min: 0, max: 15 },
            "host: bots to add at once.",
        )
        .or("0"),
        Param::optional(
            "count",
            Kind::Integer { min: 1, max: 15 },
            "bots: how many to add.",
        )
        .or("1"),
        Param::optional(
            "drive",
            Kind::Choice(&["input", "game"]),
            "bots, join: input (held input, replaced per call by step inputs) or game (the machine's own game code writes its ActionState).",
        )
        .or("\"input\""),
        Param::optional(
            "network",
            Kind::Object,
            "host: the simulated network, {latency_ms, jitter_ms, loss, duplicate, reorder, seed} (one way; chances from 0 to 1). Default: perfect, seed 0.",
        ),
        Param::optional(
            "port",
            Kind::Integer { min: 0, max: 65535 },
            "host: play over real UDP on this port (0 picks one) instead of the simulator.",
        ),
        Param::optional(
            "room_code",
            Kind::String,
            "host: the room code (default: a new one); join: the code to give (default: the session's).",
        ),
        Param::optional(
            "host_plays",
            Kind::Boolean,
            "host: the host's machine is player 0 (false: a headless host).",
        )
        .or("true"),
        Param::optional(
            "auto_start",
            Kind::Boolean,
            "host: start the first match as soon as players seats are taken and ready.",
        ),
        Param::optional(
            "discoverable",
            Kind::Boolean,
            "host: answer discovery queries (default: true over the simulator, false over UDP).",
        ),
        Param::optional(
            "name",
            Kind::String,
            "host: the host player's name; join: the player's name.",
        ),
        Param::optional(
            "color",
            Kind::Integer { min: 0, max: 63 },
            "join: the colour asked for.",
        )
        .or("0"),
        Param::optional(
            "address",
            Kind::String,
            "join: \"host:port\" (UDP), or \"discover\" to find the host on the network; default: the session's own host. With no networked session open, an address makes this process a player of that host (a new session).",
        ),
        Param::optional(
            "input",
            Kind::Object,
            "join, bots: the input the player holds (like sim_step input). step: the host's own player's.",
        ),
        Param::optional(
            "inputs",
            Kind::Object,
            "step: input by seat for these ticks, like sim_step players: {\"2\": {\"move\": [1, 0]}}. Seats not named keep what their player holds.",
        ),
        Param::optional(
            "ready",
            Kind::Boolean,
            "join, bots: ask to be ready once seated.",
        )
        .or("true"),
        Param::optional(
            "outcome",
            Kind::Integer {
                min: 0,
                max: u32::MAX as i64,
            },
            "end: the match's outcome, a number the game chooses.",
        )
        .or("0"),
        Param::optional(
            "player",
            Kind::Integer { min: 0, max: 15 },
            "status: also show this seat's machine; leave: the seat whose machine leaves.",
        ),
        Param::optional(
            "frames",
            Kind::Integer {
                min: 1,
                max: crate::sessions::MAX_FRAMES,
            },
            "step: ticks to run (default 1, or max_frames with until).",
        ),
        Param::optional(
            "until",
            Kind::String,
            "step: stop when this condition holds on the host's world.",
        ),
        Param::optional(
            "max_frames",
            Kind::Integer {
                min: 1,
                max: crate::sessions::MAX_FRAMES,
            },
            "step: the most ticks with until.",
        )
        .or("10000"),
    ],
    mutates: false,
    run: net,
};

fn net(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    match args.str("action").unwrap_or("status") {
        "host" => host(workspace, args),
        "join" => join(workspace, args),
        "bots" => bots(workspace, args),
        "start" | "end" => lobby(workspace, args),
        "step" => step(workspace, args, actor),
        "leave" => leave(workspace, args),
        "stop" => stop(workspace, args),
        _ => status(workspace, args),
    }
}

/// The simulated network a call asks for.
fn conditions(given: Option<&Value>) -> Result<(wish_net::LinkConditions, u64)> {
    let mut conditions = wish_net::LinkConditions::PERFECT;
    let Some(given) = given else {
        return Ok((conditions, 0));
    };
    let hint = "Write it like {\"latency_ms\": 100, \"jitter_ms\": 30, \"loss\": 0.05, \"duplicate\": 0.02, \"reorder\": 0.1, \"seed\": 7}: one-way milliseconds (0 to 10000) and chances from 0 to 1.";
    let Some(object) = given.as_object() else {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!("network must be an object, not {given}."),
            hint,
        )
        .at_path("/network"));
    };
    let mut seed = 0;
    for (key, value) in object {
        let path = format!("/network/{key}");
        let millis = || {
            value
                .as_u64()
                .filter(|n| *n <= 10_000)
                .and_then(|n| u32::try_from(n).ok())
        };
        let chance = || value.as_f64().filter(|n| (0.0..=1.0).contains(n));
        let fine = match key.as_str() {
            "latency_ms" => millis().map(|n| conditions.latency_ms = n).is_some(),
            "jitter_ms" => millis().map(|n| conditions.jitter_ms = n).is_some(),
            "loss" => chance().map(|n| conditions.loss = n).is_some(),
            "duplicate" => chance().map(|n| conditions.duplicate = n).is_some(),
            "reorder" => chance().map(|n| conditions.reorder = n).is_some(),
            "seed" => value.as_u64().map(|n| seed = n).is_some(),
            _ => {
                return Err(Diagnostic::error(
                    "invalid_parameter",
                    format!("network has no setting '{key}'."),
                    wish_core::suggest::unknown_name_hint(
                        key,
                        &[
                            "latency_ms",
                            "jitter_ms",
                            "loss",
                            "duplicate",
                            "reorder",
                            "seed",
                        ],
                    ),
                )
                .at_path(path));
            }
        };
        if !fine {
            return Err(Diagnostic::error(
                "invalid_parameter",
                format!("network {key} is {value}."),
                hint,
            )
            .at_path(path));
        }
    }
    Ok((conditions, seed))
}

fn host(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let scene = workspace.scene_path(args.str("scene"))?;
    let notes = workspace.open_document(&scene, false)?;
    let (conditions, network_seed) = conditions(args.value("network"))?;
    let port = args.int("port").map(|p| u16::try_from(p).unwrap_or(0));
    let expected = usize::try_from(args.int("players").unwrap_or(2)).unwrap_or(2);
    let host_plays = args.value("host_plays").and_then(Value::as_bool) != Some(false);
    let setup = crate::netplay::HostSetup {
        seed: u64::try_from(args.int("seed").unwrap_or(0)).unwrap_or(0),
        expected,
        host_plays,
        host_name: args.str("name").unwrap_or("Host").to_owned(),
        auto_start: args.flag("auto_start"),
        room: args.str("room_code").map(str::to_owned),
        port,
        conditions,
        network_seed,
        discoverable: args
            .value("discoverable")
            .and_then(Value::as_bool)
            .unwrap_or(port.is_none()),
        game: workspace
            .config()
            .map_or_else(|| "wish".to_owned(), |c| c.name.clone()),
        tick_rate: 60,
    };
    let id = workspace.start_net_session(&scene, &setup)?;
    let bots = usize::try_from(args.int("bots").unwrap_or(0)).unwrap_or(0);
    let drive = crate::netplay::Drive::parse(args.str("drive"));
    let mut added = Vec::new();
    for _ in 0..bots {
        added.push(workspace.net_add_bot(&id, drive, args)?);
    }
    let session = workspace.session(&id)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("scene".to_owned(), json!(scene));
    out.insert("seed".to_owned(), json!(session.seed));
    out.insert("tick".to_owned(), json!(session.app.tick()));
    out.insert(
        "world_hash".to_owned(),
        json!(session.app.hash().to_string()),
    );
    if let Some(net) = session.net.as_deref() {
        out.insert("room_code".to_owned(), json!(net.room.as_str()));
        out.insert("seats_expected".to_owned(), json!(expected));
        out.insert("host_plays".to_owned(), json!(host_plays));
        out.insert(
            "network".to_owned(),
            net.status(&session.app, 60)
                .remove("network")
                .unwrap_or(Value::Null),
        );
    }
    if !added.is_empty() {
        out.insert("bots".to_owned(), json!(added));
    }
    if !notes.is_empty() {
        out.insert("diagnostics".to_owned(), json!(notes));
    }
    Ok(out)
}

/// The id of the networked session a call names (or the only one open).
fn net_session(workspace: &Workspace, args: &Args) -> Result<String> {
    let id = workspace.session_id(args.str("session"))?;
    if workspace.session(&id)?.net.is_none() {
        return Err(not_networked(&id));
    }
    Ok(id)
}

pub(crate) fn not_networked(id: &str) -> Diagnostic {
    Diagnostic::error(
        "net_not_networked",
        format!("Session {id} is not a networked match."),
        "Host one with net (action host): its world is the host's, and join and bots add players.",
    )
    .at_path("/session")
}

fn join(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let address = args.str("address");
    let any_networked = workspace.sessions.open.values().any(|s| s.net.is_some());
    if args.str("session").is_none()
        && !any_networked
        && let Some(address) = address
    {
        let id = workspace.join_outside(address, args)?;
        let mut out = Map::new();
        out.insert("session".to_owned(), json!(id));
        out.insert("address".to_owned(), json!(address));
        out.insert(
            "joining".to_owned(),
            json!(args.str("name").unwrap_or("Player")),
        );
        return Ok(out);
    }
    let id = net_session(workspace, args)?;
    let drive = crate::netplay::Drive::parse(args.str("drive"));
    let name = args.str("name").unwrap_or("Player").to_owned();
    workspace.net_add_machine(&id, (&name, false), drive, args)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("joining".to_owned(), json!(name));
    out.insert(
        "machines".to_owned(),
        json!(
            workspace
                .session(&id)?
                .net
                .as_deref()
                .map_or(0, |n| n.players.len())
        ),
    );
    out.insert(
        "next".to_owned(),
        json!("The player joins over the next ticks: step the session, then net status shows its seat."),
    );
    Ok(out)
}

fn bots(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let count = usize::try_from(args.int("count").unwrap_or(1)).unwrap_or(1);
    let drive = crate::netplay::Drive::parse(args.str("drive"));
    let mut added = Vec::new();
    for _ in 0..count {
        added.push(workspace.net_add_bot(&id, drive, args)?);
    }
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("bots".to_owned(), json!(added));
    out.insert(
        "drive".to_owned(),
        json!(args.str("drive").unwrap_or("input")),
    );
    Ok(out)
}

fn lobby(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let action = args.str("action").unwrap_or("start").to_owned();
    let session = workspace.session_mut(&id)?;
    let tick = session.app.tick();
    let Some(host) = session.net.as_mut().and_then(|n| n.host.as_mut()) else {
        return Err(Diagnostic::error(
            "lobby_not_host",
            format!(
                "Session {id} plays in a match hosted elsewhere; only its host starts and ends it."
            ),
            "Ask the host to start, or host a match of your own with net (action host).",
        ));
    };
    if action == "start" {
        host.session.start()?;
    } else {
        let outcome = u32::try_from(args.int("outcome").unwrap_or(0)).unwrap_or(0);
        host.session.end_match(outcome);
    }
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("action".to_owned(), json!(action));
    out.insert("at_tick".to_owned(), json!(tick));
    out.insert(
        "next".to_owned(),
        json!("It takes effect at the next tick boundary: step the session."),
    );
    Ok(out)
}

fn step(workspace: &mut Workspace, args: &Args, actor: &Actor) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let mut call = json!({ "session": id });
    for (from, to) in [
        ("frames", "frames"),
        ("until", "until"),
        ("max_frames", "max_frames"),
        ("input", "input"),
        ("inputs", "players"),
    ] {
        if let Some(value) = args.value(from) {
            call[to] = value.clone();
        }
    }
    if args.value("until").is_none()
        && let Some(object) = call.as_object_mut()
    {
        object.remove("max_frames");
    }
    let mut out = workspace
        .run_tool("sim_step", &call, actor)
        .map_err(|problem| {
            // The step's own parameter names in the mistake.
            if problem.location.path.as_deref() == Some("/players") {
                problem.at_path("/inputs")
            } else {
                problem
            }
        })?;
    let session = workspace.session(&id)?;
    if let Some(net) = session.net.as_deref() {
        let status = net.status(&session.app, 60);
        for key in ["lobby", "seats"] {
            if let Some(value) = status.get(key) {
                out.insert(key.to_owned(), value.clone());
            }
        }
    }
    Ok(out)
}

fn status(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let session = workspace.session(&id)?;
    let Some(net) = session.net.as_deref() else {
        return Err(not_networked(&id));
    };
    let mut out = net.status(&session.app, 60);
    out.insert("session".to_owned(), json!(id));
    if let Some(player) = args.int("player") {
        let seat = u32::try_from(player).unwrap_or(0);
        match net.player_status(&session.app, seat) {
            Some(found) => {
                out.insert("player".to_owned(), found);
            }
            None => return Err(unknown_player(&id, seat, &net.seats())),
        }
    }
    Ok(out)
}

pub(crate) fn unknown_player(id: &str, seat: u32, seats: &[u32]) -> Diagnostic {
    Diagnostic::error(
        "net_player_unknown",
        format!("Session {id} has no player machine in seat {seat} here."),
        if seats.is_empty() {
            "No player is seated yet: step the session until net status lists the seats.".to_owned()
        } else {
            format!(
                "The seats taken are {}; a seat of a player on another machine cannot be looked at from here.",
                seats
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    )
    .at_path("/player")
}

fn leave(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let seat = args.int("player").and_then(|p| u32::try_from(p).ok());
    let name = workspace.net_leave(&id, seat)?;
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("left".to_owned(), json!(name));
    if let Some(seat) = seat {
        out.insert("player".to_owned(), json!(seat));
    }
    out.insert(
        "next".to_owned(),
        json!("The host hears it at the next ticks: step the session."),
    );
    Ok(out)
}

fn stop(workspace: &mut Workspace, args: &Args) -> Result<Map<String, Value>> {
    let id = net_session(workspace, args)?;
    let ticks = workspace.net_close(&id)?;
    let tick = workspace.session(&id).map(|s| s.app.tick()).ok();
    workspace.sessions.open.remove(&id);
    let stopped = Diagnostic::info(
        "session_stopped",
        format!("Session {id} (a networked match) was closed for every player."),
        "Host another with net (action host).",
    );
    workspace
        .sessions
        .record(Some(&id), tick, "session", &stopped);
    let mut out = Map::new();
    out.insert("session".to_owned(), json!(id));
    out.insert("stopped_at_tick".to_owned(), json!(tick));
    out.insert("closing_ticks".to_owned(), json!(ticks));
    out.insert(
        "open_sessions".to_owned(),
        json!(workspace.sessions.open.keys().collect::<Vec<_>>()),
    );
    Ok(out)
}
