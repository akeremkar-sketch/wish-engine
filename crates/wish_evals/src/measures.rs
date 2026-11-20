//! The scorecard's measures of a finished result that need no model: the
//! quality of `check`, whether a replay of the result holds, and how fast
//! the result's game code runs at 10,000 entities. Each works on any project
//! (an agent's result, or a reference solution in the self-test).

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::probe::Probe;

/// What `check` says about a result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quality {
    pub errors: u64,
    pub warnings: u64,
}

impl Quality {
    /// No error and no warning.
    pub fn clean(self) -> bool {
        self.errors == 0 && self.warnings == 0
    }
}

/// Runs `check` on the project.
pub fn quality(dir: &Path) -> Result<Quality, String> {
    let mut probe = Probe::open(dir)?;
    let answer = probe.call("check", json!({}))?;
    Ok(Quality {
        errors: answer["errors"].as_u64().unwrap_or(0),
        warnings: answer["warnings"].as_u64().unwrap_or(0),
    })
}

/// The name of the replay the scorecard records (never one of the agent's).
pub const REPLAY: &str = "zz_scorecard";

fn replay_file(dir: &Path) -> std::path::PathBuf {
    dir.join("replays").join(format!("{REPLAY}.replay.jsonl"))
}

/// The game module's own types, if the project has built game code.
pub fn game_types(probe: &mut Probe) -> Result<Option<Vec<String>>, String> {
    let info = probe.call("project_info", json!({}))?;
    let module = &info["scripts"]["module"];
    if module.is_null() {
        return Ok(None);
    }
    Ok(Some(
        module["types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
    ))
}

/// Input that presses every action of `input.json` in a fixed pattern, so a
/// replay exercises whatever the game reads: four phases of 60 ticks
/// (nothing; everything on with two-axis actions to the right; two-axis
/// actions forward; nothing).
fn input_phases(dir: &Path) -> Vec<Value> {
    let actions: Value = std::fs::read_to_string(dir.join("input.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let mut right = serde_json::Map::new();
    let mut forward = serde_json::Map::new();
    for (name, action) in actions["actions"].as_object().into_iter().flatten() {
        match action["kind"].as_str() {
            Some("button") => {
                right.insert(name.clone(), json!(true));
            }
            Some("axis") => {
                right.insert(name.clone(), json!(1.0));
                forward.insert(name.clone(), json!(-1.0));
            }
            Some("axis2") => {
                right.insert(name.clone(), json!([1.0, 0.0]));
                forward.insert(name.clone(), json!([0.0, 1.0]));
            }
            _ => {}
        }
    }
    vec![
        json!({}),
        Value::Object(right),
        Value::Object(forward),
        json!({}),
    ]
}

/// Records a replay of the result (240 ticks of the default scene with
/// every action pressed in a fixed pattern) and plays it back in a fresh
/// workspace: `Some(true)` when every tick's world hash matches. `None` when
/// the project has no game code (nothing of the agent's runs, so there is
/// nothing to hold). The replay file is removed afterwards.
pub fn replay_holds(dir: &Path) -> Result<Option<bool>, String> {
    let recorded = record_replay(dir)?;
    if !recorded {
        return Ok(None);
    }
    let held = play_back(dir);
    let _ = std::fs::remove_file(replay_file(dir));
    held.map(Some)
}

/// Records the scorecard's replay; `false` when there is no game code.
fn record_replay(dir: &Path) -> Result<bool, String> {
    let mut probe = Probe::open(dir)?;
    if game_types(&mut probe)?.is_none() {
        return Ok(false);
    }
    let started = probe.call("sim_start", json!({ "seed": 1 }))?;
    let session = started["session"]
        .as_str()
        .ok_or("sim_start gave no session")?
        .to_owned();
    probe.call("replay", json!({ "action": "record", "session": session }))?;
    for input in input_phases(dir) {
        probe.call(
            "sim_step",
            json!({ "session": session, "frames": 60, "input": input }),
        )?;
    }
    probe.call(
        "replay",
        json!({ "action": "save", "session": session, "name": REPLAY, "overwrite": true }),
    )?;
    let _ = probe.call("sim_stop", json!({ "session": session }));
    Ok(true)
}

/// Plays the scorecard's replay back in a workspace of its own.
fn play_back(dir: &Path) -> Result<bool, String> {
    let mut fresh = Probe::open(dir)?;
    let ran = fresh.call("replay", json!({ "action": "run", "name": REPLAY }))?;
    Ok(ran["passed"] == true)
}

/// For the self-test: a replay whose recording was changed must not hold
/// (proves the replay measure can fail).
pub fn tampered_replay_is_refuted(dir: &Path) -> Result<(), String> {
    if !record_replay(dir)? {
        return Err("the project has no game code to record".to_owned());
    }
    let path = replay_file(dir);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // Change one digit of the last recorded tick's hash.
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|l| l.contains("\"hashes\""))
        .ok_or("the replay has no recorded hashes")?;
    let mut record: Value = serde_json::from_str(&lines[at]).map_err(|e| e.to_string())?;
    let mut digits: Vec<char> = record["values"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .collect();
    let last = digits.last_mut().ok_or("the replay recorded no tick")?;
    *last = if *last == '0' { '1' } else { '0' };
    record["values"] = json!(digits.into_iter().collect::<String>());
    lines[at] = record.to_string();
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    let held = play_back(dir);
    let _ = std::fs::remove_file(&path);
    match held {
        Ok(false) => Ok(()),
        Ok(true) => Err("a replay with a changed hash still holds".to_owned()),
        Err(reason) => Err(reason),
    }
}

/// How fast the result's game code runs at 10,000 entities.
#[derive(Clone, Debug, PartialEq)]
pub enum Speed {
    /// Milliseconds per tick (the whole tick: game code and the engine's
    /// systems), measured by the harness's clock over `ticks` ticks.
    Measured {
        ms_per_tick: f64,
        entities: u64,
        ticks: u64,
    },
    /// Why it was not measured.
    NotMeaningful(String),
}

/// How many entities the measurement grows the scene to.
pub const BENCH_ENTITIES: usize = 10_000;
const BENCH_TICKS: u64 = 30;
const BENCH_SCENE: &str = "scenes/zz_scorecard_bench.scene.json";

/// Components whose cost is the physics engine's, not the game code's.
const PHYSICS: &[&str] = &["RigidBody", "Collider", "CharacterController"];

/// Grows the default scene to 10,000 entities by copying the entities that
/// carry one of the game module's components (spread on a grid 40 m apart,
/// so that no two copies meet), then times `BENCH_TICKS` ticks after a
/// warm-up, the way `wen bench --script` times game code. Not meaningful
/// without game code, without an entity that carries a game component, or
/// when those entities are physics bodies (the number would then measure
/// the physics engine).
pub fn speed(dir: &Path) -> Result<Speed, String> {
    let mut probe = Probe::open(dir)?;
    let Some(types) = game_types(&mut probe)? else {
        return Ok(Speed::NotMeaningful("no game code".to_owned()));
    };
    let info = probe.call("project_info", json!({}))?;
    let scene_path = info["project"]["scene"]
        .as_str()
        .unwrap_or("scenes/main.scene.json")
        .to_owned();
    let mut scene: Value = std::fs::read_to_string(dir.join(&scene_path))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("{scene_path} cannot be read"))?;
    let entities = scene["entities"].as_object().cloned().unwrap_or_default();
    let carriers: Vec<(String, Value)> = entities
        .iter()
        .filter(|(_, e)| {
            e["components"]
                .as_object()
                .is_some_and(|c| c.keys().any(|k| types.contains(k)))
                && e.get("prefab").is_none()
                && e.get("parent").is_none()
        })
        .map(|(id, e)| (id.clone(), e.clone()))
        .collect();
    if carriers.is_empty() {
        return Ok(Speed::NotMeaningful(
            "no top-level entity of the scene carries a game component".to_owned(),
        ));
    }
    if carriers.iter().any(|(_, e)| {
        e["components"]
            .as_object()
            .is_some_and(|c| c.keys().any(|k| PHYSICS.contains(&k.as_str())))
    }) {
        return Ok(Speed::NotMeaningful(
            "the game's entities are physics bodies: the time would be the physics engine's"
                .to_owned(),
        ));
    }
    let total = entities.len();
    let Some(map) = scene["entities"].as_object_mut() else {
        return Err("the scene has no entities".to_owned());
    };
    for n in 0..BENCH_ENTITIES.saturating_sub(total) {
        let (id, entity) = &carriers[n % carriers.len()];
        let mut copy = entity.clone();
        let cell = n / carriers.len() + 1;
        let (gx, gz) = ((cell % 100) as f64 * 40.0, (cell / 100) as f64 * 40.0);
        let at = &mut copy["components"]["Transform"]["translation"];
        let base = |i: usize| at.get(i).and_then(Value::as_f64).unwrap_or(0.0);
        let moved = json!([base(0) + gx, base(1), base(2) + gz]);
        if copy["components"]["Transform"].is_object() {
            copy["components"]["Transform"]["translation"] = moved;
        }
        map.insert(format!("zz_bench_{n}_{id}"), copy);
    }
    let path = dir.join(BENCH_SCENE);
    std::fs::write(&path, scene.to_string()).map_err(|e| e.to_string())?;
    let measured = (|| -> Result<Speed, String> {
        let started = probe.call("sim_start", json!({ "scene": BENCH_SCENE, "seed": 1 }))?;
        let session = started["session"]
            .as_str()
            .ok_or("sim_start gave no session")?
            .to_owned();
        probe.call("sim_step", json!({ "session": session, "frames": 5 }))?;
        let clock = Instant::now();
        probe.call(
            "sim_step",
            json!({ "session": session, "frames": BENCH_TICKS }),
        )?;
        let ms = clock.elapsed().as_secs_f64() * 1000.0 / BENCH_TICKS as f64;
        let _ = probe.call("sim_stop", json!({ "session": session }));
        Ok(Speed::Measured {
            ms_per_tick: (ms * 1000.0).round() / 1000.0,
            entities: BENCH_ENTITIES.max(total) as u64,
            ticks: BENCH_TICKS,
        })
    })();
    let _ = std::fs::remove_file(&path);
    measured
}
