//! Replays (ADR-006): what a session did, as a file that plays back to the
//! same world.
//!
//! `replays/<name>.replay.jsonl`, one JSON object per line:
//!
//! ```text
//! {"kind":"header","format":1,"scene":"scenes/main.scene.json","scene_hash":"…","seed":7,"tick_rate":60,"code":"…","ticks":600,"engine":"1.1.0","executor":"wasmtime"}
//! {"kind":"input","tick":0,"values":{"move":[0,1]}}      (only when the player's input changes)
//! {"kind":"hashes","bits":64,"values":"<16 hex digits per tick>"}
//! ```
//!
//! Playing it back starts a new session on the scene with the seed, feeds
//! the same input at the same ticks, and compares the world's hash after
//! every tick: the first tick that differs is where the replay and the game
//! parted ways.
//!
//! **Format 2** (release 1.1) is a replay of a session with more than one
//! player: the header says `"players": N` and every input line says whose
//! input it is, `"player": 1`. A single-player replay stays format 1, so an
//! engine of 1.0 still reads every single-player replay, and refuses a
//! multi-player one instead of playing player 1's input as player 0's.
//! `engine` and `executor` (ADR-028, rule 2) were added to the header of
//! both formats in 1.1; a replay without them was recorded before.
//!
//! **Format 3** (release 1.1) is a networked match, as its host saw it: the
//! header says `"net": { "host_plays": true }`, a `match` line holds the
//! host's match log (`wish_lobby::MatchLog`, base64 of its bytes) instead of
//! input lines, and playing it back applies each tick's record to a fresh
//! host world before the tick (design note `docs/design/1.1-net-tool.md`
//! §6).

use std::path::Path;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Hash128, Result, hash_bytes};

use crate::sessions::Trail;
use crate::workspace::Workspace;

/// The folder replays live in.
pub(crate) const FOLDER: &str = "replays";
const SUFFIX: &str = ".replay.jsonl";
/// The longest replay: a quarter of an hour at 60 ticks a second.
const MAX_TICKS: u64 = 54_000;
const MAX_LINE: usize = 4 * 1024 * 1024;

/// A hash shortened to what a replay stores (64 bits are plenty to see two
/// worlds differ).
fn short(hash: Hash128) -> u64 {
    (hash.as_u128() >> 64) as u64
}

/// A replay in memory.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Replay {
    pub scene: String,
    pub scene_hash: String,
    pub seed: u64,
    pub tick_rate: u32,
    pub code: Option<String>,
    /// The session's asset fingerprint (`None` in replays from before M6).
    pub assets: Option<String>,
    /// The engine version that recorded it (`None` before 1.1).
    pub engine: Option<String>,
    /// What ran its game code (`None` before 1.1).
    pub executor: Option<String>,
    /// How many players the session had (1 in format 1).
    pub players: usize,
    /// (tick, player, what that player does from then on).
    pub inputs: Vec<(u64, usize, wish_input::Values)>,
    pub hashes: Vec<u64>,
    /// A networked match's host log (format 3): its bytes, and whether the
    /// host played.
    pub net: Option<(Vec<u8>, bool)>,
}

fn invalid(file: &str, line: usize, what: &str) -> Diagnostic {
    Diagnostic::error(
        "invalid_replay_file",
        format!("'{file}' line {line}: {what}."),
        "Replay files are written by `replay save`; record the run again rather than editing the file.",
    )
    .at_file(file)
    .at_line(u32::try_from(line).unwrap_or(0), None)
}

impl Replay {
    pub fn path(name: &str) -> String {
        format!("{FOLDER}/{name}{SUFFIX}")
    }

    pub fn ticks(&self) -> u64 {
        self.hashes.len() as u64
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        let mut line = |value: Value| {
            text.push_str(&value.to_string());
            text.push('\n');
        };
        let several = self.players > 1;
        let format = match (&self.net, several) {
            (Some(_), _) => 3,
            (None, true) => 2,
            (None, false) => 1,
        };
        let mut header = json!({
            "kind": "header", "format": format, "scene": self.scene,
            "scene_hash": self.scene_hash, "seed": self.seed, "tick_rate": self.tick_rate,
            "code": self.code, "ticks": self.ticks(), "assets": self.assets,
            "engine": self.engine, "executor": self.executor,
        });
        if several {
            header["players"] = json!(self.players);
        }
        if let Some((_, host_plays)) = &self.net {
            header["net"] = json!({ "host_plays": host_plays });
        }
        line(header);
        if let Some((log, _)) = &self.net {
            line(json!({ "kind": "match", "log": crate::render::base64(log) }));
        }
        for (tick, player, values) in &self.inputs {
            if several {
                line(json!({ "kind": "input", "tick": tick, "player": player, "values": values }));
            } else {
                line(json!({ "kind": "input", "tick": tick, "values": values }));
            }
        }
        let digits: String = self.hashes.iter().map(|h| format!("{h:016x}")).collect();
        line(json!({ "kind": "hashes", "bits": 64, "values": digits }));
        text
    }

    pub fn parse(text: &str, file: &str) -> Result<Self> {
        let mut header: Option<Value> = None;
        let mut inputs = Vec::new();
        let mut hashes: Option<Vec<u64>> = None;
        let mut log: Option<Vec<u8>> = None;
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            if line.trim().is_empty() {
                continue;
            }
            if line.len() > MAX_LINE {
                return Err(invalid(file, number, "the line is too long"));
            }
            let value: Value =
                serde_json::from_str(line).map_err(|e| invalid(file, number, &e.to_string()))?;
            match value.get("kind").and_then(Value::as_str) {
                Some("header") if header.is_none() => header = Some(value),
                Some("input") => {
                    let tick = value
                        .get("tick")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| invalid(file, number, "an input line needs a 'tick'"))?;
                    let values: wish_input::Values =
                        serde_json::from_value(value.get("values").cloned().unwrap_or(Value::Null))
                            .map_err(|_| {
                                invalid(file, number, "'values' must map actions to [x, y]")
                            })?;
                    let player = match value.get("player") {
                        None => Some(0),
                        Some(player) => player.as_u64().and_then(|p| usize::try_from(p).ok()),
                    }
                    .ok_or_else(|| {
                        invalid(file, number, "'player' must be a player id (0, 1, ...)")
                    })?;
                    inputs.push((tick, player, (number, values)));
                }
                Some("match") if log.is_none() => {
                    let bytes = value
                        .get("log")
                        .and_then(Value::as_str)
                        .and_then(crate::render::base64_decode)
                        .ok_or_else(|| {
                            invalid(file, number, "'log' must be the match log in base64")
                        })?;
                    log = Some(bytes);
                }
                Some("hashes") if hashes.is_none() => {
                    let digits = value
                        .get("values")
                        .and_then(Value::as_str)
                        .filter(|d| d.len() % 16 == 0 && d.is_ascii())
                        .ok_or_else(|| {
                            invalid(file, number, "'values' must be 16 hex digits per tick")
                        })?;
                    let parsed: Option<Vec<u64>> = (0..digits.len() / 16)
                        .map(|i| u64::from_str_radix(&digits[i * 16..i * 16 + 16], 16).ok())
                        .collect();
                    hashes = Some(parsed.ok_or_else(|| {
                        invalid(file, number, "'values' has a digit that is not hex")
                    })?);
                }
                _ => return Err(invalid(file, number, "this line is not part of a replay")),
            }
        }
        let header = header.ok_or_else(|| invalid(file, 1, "the replay has no header"))?;
        let hashes = hashes.ok_or_else(|| invalid(file, 1, "the replay has no hashes"))?;
        let net = match header["format"].as_u64() {
            Some(3) => {
                let host_plays = header["net"]["host_plays"].as_bool().unwrap_or(true);
                let log = log.take().ok_or_else(|| {
                    invalid(
                        file,
                        1,
                        "a format-3 replay (a networked match) has a match line",
                    )
                })?;
                if !inputs.is_empty() {
                    return Err(invalid(
                        file,
                        1,
                        "a format-3 replay keeps its inputs in the match log, not in input lines",
                    ));
                }
                Some((log, host_plays))
            }
            _ if log.is_some() => {
                return Err(invalid(
                    file,
                    1,
                    "only a format-3 replay (a networked match) has a match line",
                ));
            }
            _ => None,
        };
        let players = match header["format"].as_u64() {
            Some(3) => 1,
            Some(1) if header.get("players").is_none() => 1,
            Some(2) => header
                .get("players")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| (2..=wish_input::MAX_PLAYERS).contains(n))
                .ok_or_else(|| {
                    invalid(
                        file,
                        1,
                        &format!(
                            "a format-2 replay says how many players it has, 2 to {}",
                            wish_input::MAX_PLAYERS
                        ),
                    )
                })?,
            Some(1) => {
                return Err(invalid(
                    file,
                    1,
                    "a format-1 replay has one player, so its header has no 'players'",
                ));
            }
            _ => {
                return Err(invalid(
                    file,
                    1,
                    "this replay has a format the engine does not read",
                ));
            }
        };
        let mut checked = Vec::with_capacity(inputs.len());
        for (tick, player, (number, values)) in inputs {
            if player >= players {
                return Err(invalid(
                    file,
                    number,
                    &format!("player {player} is not one of the replay's {players} player(s)"),
                ));
            }
            checked.push((tick, player, values));
        }
        let inputs = checked;
        let text_of = |key: &str| {
            header
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| invalid(file, 1, &format!("the header has no '{key}'")))
        };
        if header.get("ticks").and_then(Value::as_u64) != Some(hashes.len() as u64)
            || hashes.len() as u64 > MAX_TICKS
        {
            return Err(invalid(
                file,
                1,
                "the header's tick count does not match the hashes",
            ));
        }
        Ok(Self {
            scene: text_of("scene")?,
            scene_hash: text_of("scene_hash")?,
            seed: header.get("seed").and_then(Value::as_u64).unwrap_or(0),
            tick_rate: header
                .get("tick_rate")
                .and_then(Value::as_u64)
                .and_then(|r| u32::try_from(r).ok())
                .unwrap_or(60),
            code: header
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned),
            assets: header
                .get("assets")
                .and_then(Value::as_str)
                .map(str::to_owned),
            engine: header
                .get("engine")
                .and_then(Value::as_str)
                .map(str::to_owned),
            executor: header
                .get("executor")
                .and_then(Value::as_str)
                .map(str::to_owned),
            players,
            inputs,
            hashes,
            net,
        })
    }
}

/// The version of this engine, as replays carry it.
const ENGINE: &str = env!("CARGO_PKG_VERSION");

/// What playing `replay` here differs in from what recorded it: another
/// engine version or executor is allowed and said (ADR-028, rule 2), so a
/// divergence is reported against the right cause.
fn made_by(replay: &Replay) -> Vec<Diagnostic> {
    let mut notes = Vec::new();
    match replay.engine.as_deref() {
        Some(ENGINE) => {}
        Some(other) => notes.push(Diagnostic::warning(
            "replay_engine_changed",
            format!("The replay was recorded by engine {other}; this is engine {ENGINE}."),
            "A patch release (the same first two numbers) never changes what a simulation computes; another minor version may (ADR-028): a divergence below may just be that. Record the replay again with this engine to keep it.",
        )),
        None => notes.push(Diagnostic::warning(
            "replay_engine_changed",
            format!("The replay does not say which engine recorded it (replays say so from 1.1 on); this is engine {ENGINE}."),
            "A replay from an older engine may diverge at its first tick because the engine's types changed, not the game: record it again with this engine to keep it.",
        )),
    }
    if let Some(other) = replay.executor.as_deref()
        && other != wish_script::EXECUTOR
    {
        notes.push(Diagnostic::warning(
            "replay_executor_changed",
            format!(
                "The replay's game code ran on '{other}'; here it runs on '{}'.",
                wish_script::EXECUTOR
            ),
            "Only an executor that gives the same hashes as the others in CI is supported (ADR-028): a divergence below may be the executor, not the game.",
        ));
    }
    notes
}

/// A name a replay can have: it becomes a file name.
pub(crate) fn check_name(name: &str) -> Result<()> {
    let fine = !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if fine {
        Ok(())
    } else {
        Err(Diagnostic::error(
            "invalid_parameter",
            format!("'{name}' is not a replay name."),
            "Use 1 to 40 lowercase letters, digits, '_' or '-', such as \"jump_test\".",
        )
        .at_path("/name"))
    }
}

fn write_atomic(root: &Path, relative: &str, text: &str) -> Result<()> {
    let target = crate::documents::prepare_output(root, relative)?;
    let temporary = target.with_extension("jsonl.wish-tmp");
    let unwritable = |error: std::io::Error| {
        Diagnostic::error(
            "file_unwritable",
            format!("Could not write '{relative}': {error}."),
            "Check that the project folder can be written to.",
        )
        .at_file(relative)
    };
    std::fs::write(&temporary, text).map_err(unwritable)?;
    crate::documents::replace_file(&temporary, &target).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        unwritable(error)
    })
}

impl Workspace {
    /// Starts recording session `id`. It must be at tick 0: a replay plays
    /// from the start of the scene.
    pub(crate) fn replay_record(&mut self, id: &str) -> Result<()> {
        let session = self.session_mut(id)?;
        session.not_networked(id, "recording (the match log is kept from tick 0)")?;
        if session.app.tick() != 0 {
            return Err(Diagnostic::error(
                "replay_not_at_start",
                format!(
                    "Session {id} is at tick {}; a replay is recorded from tick 0.",
                    session.app.tick()
                ),
                "Start a new session with sim_start, then call replay with action \"record\" before the first sim_step (or restore a snapshot taken at tick 0).",
            ));
        }
        // What the replay depends on is taken now, not when it is saved: code
        // or scene changed later would make the file impossible to replay.
        let code = session
            .script
            .as_ref()
            .map(|script| script.module().fingerprint().to_string());
        let scene = session.scene.clone();
        let assets = session.assets.clone();
        let text = std::fs::read(self.root().join(&scene)).unwrap_or_default();
        let session = self.session_mut(id)?;
        session.recording = Some(Trail {
            code,
            scene_hash: hash_bytes(&text).to_string(),
            assets,
            ..Trail::default()
        });
        Ok(())
    }

    /// Writes the recording of session `id` as replay `name`.
    pub(crate) fn replay_save(
        &mut self,
        id: &str,
        name: &str,
        overwrite: bool,
        dry_run: bool,
    ) -> Result<Map<String, Value>> {
        check_name(name)?;
        let path = Replay::path(name);
        let root = self.root().to_path_buf();
        if root.join(&path).exists() && !overwrite {
            return Err(Diagnostic::error(
                "replay_exists",
                format!("There is already a replay '{name}'."),
                "Choose another name, or pass \"overwrite\": true to replace it.",
            )
            .at_path("/name"));
        }
        let session = self.session(id)?;
        if let Some(net) = session.net.as_deref() {
            return self.match_save(id, net, (name, &path), dry_run);
        }
        let Some(trail) = &session.recording else {
            return Err(Diagnostic::error(
                "replay_not_recording",
                format!("Session {id} is not recording."),
                "Call replay with action \"record\" on a session at tick 0, play it with sim_step, then save.",
            ));
        };
        if trail.hashes.is_empty() {
            return Err(Diagnostic::error(
                "replay_empty",
                "Nothing was recorded yet.",
                "Run some ticks with sim_step first.",
            ));
        }
        let replay = Replay {
            scene: session.scene.clone(),
            scene_hash: trail.scene_hash.clone(),
            seed: session.seed,
            tick_rate: session.app.config().tick_rate_hz,
            code: trail.code.clone(),
            assets: Some(trail.assets.clone()),
            engine: Some(ENGINE.to_owned()),
            executor: Some(wish_script::EXECUTOR.to_owned()),
            players: session.players.max(1),
            inputs: trail.inputs.clone(),
            hashes: trail.hashes.iter().map(|h| short(*h)).collect(),
            net: None,
        };
        if replay.ticks() > MAX_TICKS {
            return Err(Diagnostic::error(
                "replay_too_long",
                format!(
                    "The recording has {} ticks; a replay can have {MAX_TICKS}.",
                    replay.ticks()
                ),
                "Record shorter scenes: a quarter of an hour is the longest.",
            ));
        }
        if !dry_run {
            write_atomic(&root, &path, &replay.to_text())?;
        }
        let mut out = Map::new();
        out.insert("session".to_owned(), json!(id));
        out.insert("name".to_owned(), json!(name));
        out.insert("path".to_owned(), json!(path));
        out.insert("ticks".to_owned(), json!(replay.ticks()));
        out.insert("input_changes".to_owned(), json!(replay.inputs.len()));
        if replay.players > 1 {
            out.insert("players".to_owned(), json!(replay.players));
        }
        out.insert("dry_run".to_owned(), json!(dry_run));
        out.insert(
            "final_hash".to_owned(),
            json!(format!(
                "{:016x}",
                replay.hashes.last().copied().unwrap_or(0)
            )),
        );
        Ok(out)
    }

    /// Plays replay `name` back and compares every tick's hash.
    pub(crate) fn replay_run(&mut self, name: &str, keep: bool) -> Result<Map<String, Value>> {
        check_name(name)?;
        let path = Replay::path(name);
        let root = self.root().to_path_buf();
        let text = std::fs::read_to_string(root.join(&path)).map_err(|error| {
            let names: Vec<String> = std::fs::read_dir(root.join(FOLDER))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                .filter_map(|n| n.strip_suffix(SUFFIX).map(str::to_owned))
                .collect();
            let hint = if names.is_empty() {
                "There are no replays yet: record one with replay (record, then save).".to_owned()
            } else {
                wish_core::suggest::unknown_name_hint(
                    name,
                    &names.iter().map(String::as_str).collect::<Vec<_>>(),
                )
            };
            Diagnostic::error(
                "replay_not_found",
                format!("There is no replay '{name}' ({error})."),
                hint,
            )
            .at_path("/name")
            .with_candidates(names)
        })?;
        let replay = Replay::parse(&text, &path)?;
        // Replay files travel with projects: the scene they name is checked
        // like any scene path (inside the project, no `..`).
        let scene_path = crate::documents::normalize_scene_path(&replay.scene)
            .map(|scene| root.join(scene))
            .map_err(|problem| problem.at_file(&path))?;
        let mut notes = Vec::new();
        let scene_text =
            std::fs::read(&scene_path).map_err(|_| {
                Diagnostic::error(
                "replay_scene_missing",
                format!("The replay's scene '{}' does not exist any more.", replay.scene),
                "Restore the scene file (or the project's history of it) and run the replay again.",
            )
            .at_file(&replay.scene)
            })?;
        if hash_bytes(&scene_text).to_string() != replay.scene_hash {
            notes.push(Diagnostic::warning(
                "replay_scene_changed",
                format!("The scene '{}' has changed since the replay was recorded.", replay.scene),
                "A different scene usually plays out differently; a divergence below may just be that.",
            ));
        }
        let code = self.game_module().map(|m| m.fingerprint().to_string());
        if code != replay.code {
            notes.push(Diagnostic::warning(
                "replay_code_changed",
                "The game code is not the code the replay was recorded with.",
                "That is how to ask \"does the game still play the same?\": if the replay passes, the change did not alter the run.",
            ));
        }
        notes.extend(made_by(&replay));
        let opened = self.open_document(&replay.scene, false)?;
        notes.extend(opened);
        let (id, _) = if replay.net.is_some() {
            self.start_match_replay(&replay.scene, replay.seed)?
        } else {
            self.start_session(&replay.scene, replay.seed, replay.players, Vec::new())?
        };
        let now = self.session(&id)?.assets.clone();
        if replay
            .assets
            .as_ref()
            .is_some_and(|recorded| *recorded != now)
        {
            notes.push(Diagnostic::warning(
                "replay_assets_changed",
                "Project files the game reads (sound or animation clips, fonts) changed since the replay was recorded.",
                "Clip lengths decide when sounds and animations end, fonts decide the interface layout and where clicks land: a divergence below may just be that.",
            ));
        }
        if let Some((log, _)) = &replay.net {
            return self.match_run(name, &id, (&replay, log), keep, notes);
        }
        let cancel = self.cancel.clone();
        let session = self.session_mut(&id)?;
        let mut next = 0;
        let mut values = vec![wish_input::Values::new(); replay.players];
        let mut diverged: Option<(u64, u64, u64)> = None;
        let mut cancelled = false;
        let mut failures = Vec::new();
        let mut ran = 0;
        for (tick, expected) in (0..replay.ticks()).zip(&replay.hashes) {
            while let Some((at, player, next_values)) = replay.inputs.get(next)
                && *at <= tick
            {
                if let Some(slot) = values.get_mut(*player) {
                    slot.clone_from(next_values);
                }
                next += 1;
            }
            if cancel
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
            {
                cancelled = true;
                break;
            }
            wish_input::apply_players(session.app.world_mut(), &session.input, &values)?;
            failures.extend(session.app.step()?.failures);
            ran += 1;
            let actual = short(session.app.hash());
            if actual != *expected {
                diverged = Some((tick, *expected, actual));
                break;
            }
        }
        let mut out = Map::new();
        out.insert("name".to_owned(), json!(name));
        out.insert("ticks".to_owned(), json!(replay.ticks()));
        out.insert("ticks_run".to_owned(), json!(ran));
        if replay.players > 1 {
            out.insert("players".to_owned(), json!(replay.players));
        }
        let passed = diverged.is_none() && !cancelled;
        out.insert("passed".to_owned(), json!(passed));
        if cancelled {
            out.insert("stopped_by".to_owned(), json!("cancelled"));
        }
        if let Some((tick, expected, actual)) = diverged {
            out.insert("diverged_at".to_owned(), json!(tick));
            out.insert(
                "expected_hash".to_owned(),
                json!(format!("{expected:016x}")),
            );
            out.insert("actual_hash".to_owned(), json!(format!("{actual:016x}")));
            notes.push(Diagnostic::error(
                "replay_diverged",
                format!("The replay and the game parted ways at tick {tick}: the world there is not the one that was recorded."),
                format!("Session {id} is left open at that tick (after it ran): look with entity_query / entity_get, and compare with what the code at that tick does. Close it with sim_stop."),
            ));
        }
        for failure in failures {
            notes.push(failure);
        }
        if passed {
            out.insert(
                "final_hash".to_owned(),
                json!(format!(
                    "{:016x}",
                    replay.hashes.last().copied().unwrap_or(0)
                )),
            );
        }
        if diverged.is_some() || keep {
            out.insert("session".to_owned(), json!(id));
        } else {
            self.sessions.open.remove(&id);
        }
        if !notes.is_empty() {
            out.insert("diagnostics".to_owned(), json!(notes));
        }
        Ok(out)
    }
}

impl Workspace {
    /// Writes a networked session's match (format 3).
    fn match_save(
        &self,
        id: &str,
        net: &crate::netplay::NetMatch,
        (name, path): (&str, &str),
        dry_run: bool,
    ) -> Result<Map<String, Value>> {
        let session = self.session(id)?;
        let Some(host) = net.host.as_ref() else {
            return Err(Diagnostic::error(
                "net_session_unsupported",
                format!(
                    "Session {id} plays in a match hosted elsewhere; the match log is the host's."
                ),
                "Save the replay on the machine that hosts the match.",
            ));
        };
        if net.hashes.is_empty() {
            return Err(Diagnostic::error(
                "replay_empty",
                "The match has not run a tick yet.",
                "Step it with net step (or sim_step) first.",
            ));
        }
        let replay = Replay {
            scene: session.scene.clone(),
            scene_hash: net.scene_hash.clone(),
            seed: session.seed,
            tick_rate: session.app.config().tick_rate_hz,
            code: net.code.clone(),
            assets: Some(session.assets.clone()),
            engine: Some(ENGINE.to_owned()),
            executor: Some(wish_script::EXECUTOR.to_owned()),
            players: 1,
            inputs: Vec::new(),
            hashes: net.hashes.iter().map(|h| short(*h)).collect(),
            net: Some((host.session.log().to_bytes(), net.host_plays)),
        };
        if replay.ticks() > MAX_TICKS {
            return Err(Diagnostic::error(
                "replay_too_long",
                format!(
                    "The match has {} ticks; a replay can have {MAX_TICKS}.",
                    replay.ticks()
                ),
                "Record shorter matches: a quarter of an hour is the longest.",
            ));
        }
        if !dry_run {
            write_atomic(self.root(), path, &replay.to_text())?;
        }
        let mut out = Map::new();
        out.insert("session".to_owned(), json!(id));
        out.insert("name".to_owned(), json!(name));
        out.insert("path".to_owned(), json!(path));
        out.insert("ticks".to_owned(), json!(replay.ticks()));
        out.insert("format".to_owned(), json!(3));
        out.insert(
            "match_records".to_owned(),
            json!(host.session.log().records.len()),
        );
        out.insert("dry_run".to_owned(), json!(dry_run));
        out.insert(
            "final_hash".to_owned(),
            json!(format!(
                "{:016x}",
                replay.hashes.last().copied().unwrap_or(0)
            )),
        );
        Ok(out)
    }

    /// A session for playing a match back: the host's world as `net host`
    /// builds it, not networked.
    fn start_match_replay(&mut self, scene: &str, seed: u64) -> Result<(String, Vec<Diagnostic>)> {
        let (id, notes) = self.start_session(scene, seed, 1, Vec::new())?;
        let (app, script, _) = self.net_host_app(scene, seed)?;
        let session = self.session_mut(&id)?;
        session.app = app;
        session.script = script;
        Ok((id, notes))
    }

    /// Plays a match's log back in session `id` (format 3).
    fn match_run(
        &mut self,
        name: &str,
        id: &str,
        (replay, log): (&Replay, &[u8]),
        keep: bool,
        mut notes: Vec<Diagnostic>,
    ) -> Result<Map<String, Value>> {
        let log = wish_lobby::MatchLog::from_bytes(log)?;
        let cancel = self.cancel.clone();
        let session = self.session_mut(id)?;
        let mut carry = wish_lobby::Carry::new();
        let mut diverged: Option<(u64, u64, u64)> = None;
        let mut cancelled = false;
        let mut failures = Vec::new();
        let mut ran = 0;
        for (tick, expected) in (0..replay.ticks()).zip(&replay.hashes) {
            if cancel
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
            {
                cancelled = true;
                break;
            }
            let (_, problems) =
                wish_lobby::apply(session.app.world_mut(), &log.record(tick), &mut carry)?;
            failures.extend(problems);
            failures.extend(session.app.step()?.failures);
            ran += 1;
            let actual = short(session.app.hash());
            if actual != *expected {
                diverged = Some((tick, *expected, actual));
                break;
            }
        }
        let mut out = Map::new();
        out.insert("name".to_owned(), json!(name));
        out.insert("ticks".to_owned(), json!(replay.ticks()));
        out.insert("ticks_run".to_owned(), json!(ran));
        out.insert("format".to_owned(), json!(3));
        let passed = diverged.is_none() && !cancelled;
        out.insert("passed".to_owned(), json!(passed));
        if cancelled {
            out.insert("stopped_by".to_owned(), json!("cancelled"));
        }
        if let Some((tick, expected, actual)) = diverged {
            out.insert("diverged_at".to_owned(), json!(tick));
            out.insert(
                "expected_hash".to_owned(),
                json!(format!("{expected:016x}")),
            );
            out.insert("actual_hash".to_owned(), json!(format!("{actual:016x}")));
            notes.push(Diagnostic::error(
                "replay_diverged",
                format!("The match's replay and the game parted ways at tick {tick}: the host's world there is not the one that was recorded."),
                format!("Session {id} is left open at that tick (the host's world, after it ran): look with entity_query / entity_get. Close it with sim_stop."),
            ));
        }
        notes.extend(failures);
        if passed {
            out.insert(
                "final_hash".to_owned(),
                json!(format!(
                    "{:016x}",
                    replay.hashes.last().copied().unwrap_or(0)
                )),
            );
        }
        if diverged.is_some() || keep {
            out.insert("session".to_owned(), json!(id));
        } else {
            self.sessions.open.remove(id);
        }
        if !notes.is_empty() {
            out.insert("diagnostics".to_owned(), json!(notes));
        }
        Ok(out)
    }
}
