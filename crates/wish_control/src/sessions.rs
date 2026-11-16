//! Play sessions: a scene turned into a live, deterministic world that
//! tools step, look at, snapshot and fork. Sessions never write to disk.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};
use wish_app::{App, AppConfig, AppSnapshot};
use wish_core::{Diagnostic, Entity, Result, Severity};
use wish_ecs::World;
use wish_scene::SceneId;

use crate::expr::{Expr, Subject};
use crate::workspace::Workspace;

/// The most sessions open at once (each holds a whole world).
pub const MAX_SESSIONS: usize = 8;
/// The most snapshots one session keeps (each holds a whole world).
pub const MAX_SNAPSHOTS: usize = 32;
/// The most ticks one `sim_step` may run.
pub const MAX_FRAMES: i64 = 1_000_000;
/// How many log entries are kept.
pub const MAX_LOG: usize = 1000;
/// |y| beyond which an entity is reported as having left the world.
pub const OUT_OF_BOUNDS_M: f64 = 1000.0;

/// A snapshot a session holds.
pub(crate) struct Saved {
    pub label: String,
    pub tick: u64,
    pub hash: String,
    pub snapshot: AppSnapshot,
}

/// One play session.
pub(crate) struct Session {
    pub scene: String,
    pub seed: u64,
    pub app: App,
    pub saved: Vec<Saved>,
    /// (code, entity) pairs already reported by the inspector.
    pub reported: BTreeSet<(String, String)>,
    /// The game code running in this session, if the project has any.
    pub script: Option<wish_script::ScriptRuntime>,
    /// The project's actions (`input.json`) as they were when the session started.
    pub input: wish_input::InputMap,
}

/// One entry of the structured log.
#[derive(Clone, Debug)]
pub(crate) struct LogEntry {
    pub seq: u64,
    pub session: Option<String>,
    pub tick: Option<u64>,
    pub severity: Severity,
    pub kind: &'static str,
    pub code: String,
    pub message: String,
    pub hint: String,
}

impl LogEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "seq": self.seq,
            "session": self.session,
            "tick": self.tick,
            "severity": self.severity,
            "kind": self.kind,
            "code": self.code,
            "message": self.message,
            "hint": self.hint,
        })
    }
}

/// Every session, and the log they share.
#[derive(Default)]
pub(crate) struct Sessions {
    pub open: BTreeMap<String, Session>,
    next_id: u32,
    next_seq: u64,
    pub log: VecDeque<LogEntry>,
}

impl Sessions {
    pub fn new_id(&mut self) -> String {
        self.next_id += 1;
        format!("s{}", self.next_id)
    }

    pub fn record(
        &mut self,
        session: Option<&str>,
        tick: Option<u64>,
        kind: &'static str,
        diagnostic: &Diagnostic,
    ) {
        self.next_seq += 1;
        self.log.push_back(LogEntry {
            seq: self.next_seq,
            session: session.map(str::to_owned),
            tick,
            severity: diagnostic.severity,
            kind,
            code: diagnostic.code.to_string(),
            message: diagnostic.message.clone(),
            hint: diagnostic.hint.clone(),
        });
        if self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
    }
}

/// An entity's name in a session: its name in the scene file, or `#<id>`
/// for entities made while the simulation runs.
pub(crate) fn name_of(world: &World, entity: Entity) -> String {
    world
        .get::<SceneId>(entity)
        .and_then(|scene_id| world.text(scene_id.id))
        .map_or_else(|| format!("#{entity}"), str::to_owned)
}

/// Every entity of the world by name.
pub(crate) fn entity_names(world: &World) -> BTreeMap<String, Entity> {
    world
        .entities()
        .map(|entity| (name_of(world, entity), entity))
        .collect()
}

/// Looks at one entity of a live world, or at the world itself (`id` empty,
/// only resources).
pub(crate) struct LiveSubject<'a> {
    pub world: &'a World,
    pub id: &'a str,
    pub entity: Option<Entity>,
}

impl Subject for LiveSubject<'_> {
    fn id(&self) -> &str {
        self.id
    }

    fn component(&self, name: &str) -> Option<Value> {
        if let Some(entity) = self.entity
            && let Ok(value) = self.world.get_json(entity, name)
        {
            return Some(value);
        }
        self.world.resource_json(name).ok()
    }
}

/// Builds the world of a session: the project's types, the example's
/// systems (if the project names one), the game code's systems, the
/// scene's entities.
fn build_app(
    workspace: &mut Workspace,
    scene: &str,
    seed: u64,
) -> Result<(App, Option<wish_script::ScriptRuntime>)> {
    let example = workspace.example().map(str::to_owned);
    let mut app = App::new(AppConfig {
        seed,
        ..AppConfig::default()
    });
    workspace.register_types(app.world_mut())?;
    // The scene first, so that its resource values win over system defaults.
    let open = workspace.document(scene)?;
    open.doc.instantiate(app.world_mut())?;
    if let Some(name) = &example {
        wish_examples::add_systems(name, &mut app)?;
    }
    let runtime = workspace
        .game_module()
        .map(|module| wish_script::ScriptRuntime::new(std::sync::Arc::clone(module)));
    if let Some(runtime) = &runtime {
        crate::script::add_game_systems(&mut app, runtime)?;
    }
    crate::script::propagation_after_all(&mut app)?;
    wish_transform::update_global_transforms(app.world_mut())?;
    Ok((app, runtime))
}

/// What a run of ticks did.
pub(crate) struct Stepped {
    pub ticks_run: u64,
    pub stopped_by: &'static str,
    pub events: BTreeMap<String, u64>,
    pub matched: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

fn event_totals(world: &World) -> BTreeMap<String, u64> {
    world
        .registry()
        .events()
        .filter_map(|(_, info)| {
            world
                .events_sent(info.name())
                .ok()
                .map(|total| (info.name().to_owned(), total))
        })
        .collect()
}

/// Whether `until` holds: for the world (resources only) or for any entity.
/// Returns the names that satisfy it (up to ten).
fn until_holds(world: &World, until: &Expr) -> Vec<String> {
    if until.matches(&LiveSubject {
        world,
        id: "",
        entity: None,
    }) {
        return vec!["(world)".to_owned()];
    }
    let mut found = Vec::new();
    for entity in world.entities() {
        let id = name_of(world, entity);
        if until.matches(&LiveSubject {
            world,
            id: &id,
            entity: Some(entity),
        }) {
            found.push(id);
            if found.len() >= 10 {
                break;
            }
        }
    }
    found
}

/// Simulates up to `frames` ticks, stopping early when `until` holds.
fn step_app(
    session: &mut Session,
    frames: u64,
    until: Option<&Expr>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    input: &wish_input::Values,
) -> Result<Stepped> {
    let before = event_totals(session.app.world());
    let mut diagnostics = Vec::new();
    let mut ticks_run = 0;
    let mut matched = Vec::new();
    let mut stopped_by = if until.is_some() && frames == 0 {
        "max_frames"
    } else {
        "frames"
    };
    while ticks_run < frames {
        // The client may cancel a long run; it is checked at tick
        // boundaries, so the world is always whole (and the tick reported).
        if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            stopped_by = "cancelled";
            break;
        }
        // The tick's input, before any system runs. A project without
        // actions has none, and its world stays as it was.
        if !session.input.is_empty() {
            wish_input::apply(session.app.world_mut(), &session.input, input)?;
        }
        let report = session.app.step()?;
        ticks_run += 1;
        diagnostics.extend(report.failures);
        if let Some(until) = until {
            matched = until_holds(session.app.world(), until);
            if !matched.is_empty() {
                stopped_by = "until";
                break;
            }
        }
    }
    if until.is_some() && stopped_by != "until" && stopped_by != "cancelled" {
        stopped_by = "max_frames";
        diagnostics.push(Diagnostic::warning(
            "until_not_reached",
            format!("The condition did not become true within {ticks_run} tick(s)."),
            "Raise max_frames, or check the condition with entity_query on the session to see what the values are now.",
        ));
    }
    let after = event_totals(session.app.world());
    let events = after
        .into_iter()
        .filter_map(|(name, total)| {
            let delta = total - before.get(&name).copied().unwrap_or(0);
            (delta > 0).then_some((name, delta))
        })
        .collect();
    Ok(Stepped {
        ticks_run,
        stopped_by,
        events,
        matched,
        diagnostics,
    })
}

/// Rules that look at the world after a run and say what looks wrong.
fn inspect(session: &mut Session) -> Vec<Diagnostic> {
    let world = session.app.world();
    let mut found: Vec<(String, Diagnostic)> = Vec::new();
    for entity in world.entities() {
        let Ok(transform) = world.get_json(entity, "Transform") else {
            continue;
        };
        let name = name_of(world, entity);
        let numbers: Vec<f64> = ["translation", "rotation", "scale"]
            .iter()
            .filter_map(|key| transform.get(*key).and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        if numbers.len() != 10 || numbers.iter().any(|n| !n.is_finite()) {
            found.push((
                name.clone(),
                Diagnostic::error(
                    "transform_not_finite",
                    format!("Entity '{name}' has a Transform with a NaN or infinite value."),
                    "Something divided by zero or overflowed. Find the system or edit that wrote it; entity_get shows the values.",
                ),
            ));
            continue;
        }
        let y = transform
            .get("translation")
            .and_then(|t| t.get(1))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if y.abs() > OUT_OF_BOUNDS_M {
            found.push((
                name.clone(),
                Diagnostic::warning(
                    "entity_out_of_bounds",
                    format!(
                        "Entity '{name}' is at y = {y:.0} m; it probably fell out of the world."
                    ),
                    "Add a floor under it, or something that removes entities that fall too far.",
                ),
            ));
        }
    }
    found
        .into_iter()
        .filter(|(name, diagnostic)| {
            session
                .reported
                .insert((diagnostic.code.to_string(), name.clone()))
        })
        .map(|(_, diagnostic)| diagnostic)
        .collect()
}

impl Workspace {
    /// The only open session, if exactly one is open; otherwise the one
    /// named. Mistakes list what is open.
    pub(crate) fn session_id(&self, wanted: Option<&str>) -> Result<String> {
        let open: Vec<&str> = self.sessions.open.keys().map(String::as_str).collect();
        match wanted {
            Some(id) if self.sessions.open.contains_key(id) => Ok(id.to_owned()),
            Some(id) => Err(Diagnostic::error(
                "unknown_session",
                format!("There is no open session '{id}'."),
                if open.is_empty() {
                    "Start one with sim_start.".to_owned()
                } else {
                    wish_core::suggest::unknown_name_hint(id, &open)
                },
            )
            .at_path("/session")
            .with_candidates(open)),
            None => match open.as_slice() {
                [only] => Ok((*only).to_owned()),
                [] => Err(Diagnostic::error(
                    "no_session",
                    "No session is open.",
                    "Start one with sim_start.",
                )
                .at_path("/session")),
                _ => Err(Diagnostic::error(
                    "session_required",
                    format!(
                        "Several sessions are open ({}); say which one.",
                        open.join(", ")
                    ),
                    "Pass \"session\": \"<id>\".",
                )
                .at_path("/session")
                .with_candidates(open)),
            },
        }
    }

    pub(crate) fn session(&self, id: &str) -> Result<&Session> {
        self.sessions.open.get(id).ok_or_else(|| {
            Diagnostic::error(
                "unknown_session",
                format!("There is no open session '{id}'."),
                "Start one with sim_start.",
            )
        })
    }

    pub(crate) fn session_mut(&mut self, id: &str) -> Result<&mut Session> {
        self.sessions.open.get_mut(id).ok_or_else(|| {
            Diagnostic::error(
                "unknown_session",
                format!("There is no open session '{id}'."),
                "Start one with sim_start.",
            )
        })
    }

    /// Starts a session from a scene. `from` restores a snapshot of another
    /// session into the new one (a fork).
    pub(crate) fn start_session(
        &mut self,
        scene: &str,
        seed: u64,
        notes: Vec<Diagnostic>,
    ) -> Result<(String, Vec<Diagnostic>)> {
        if self.sessions.open.len() >= MAX_SESSIONS {
            return Err(Diagnostic::error(
                "too_many_sessions",
                format!("{MAX_SESSIONS} sessions are already open."),
                "Close one with sim_stop first.",
            ));
        }
        let (app, script) = build_app(self, scene, seed)?;
        let input = self.input_map()?;
        let id = self.sessions.new_id();
        let tick = app.tick();
        self.sessions.open.insert(
            id.clone(),
            Session {
                scene: scene.to_owned(),
                seed,
                app,
                saved: Vec::new(),
                reported: BTreeSet::new(),
                script,
                input,
            },
        );
        let started = Diagnostic::info(
            "session_started",
            format!("Session {id} started from '{scene}' with seed {seed}."),
            "Step it with sim_step.",
        );
        self.sessions
            .record(Some(&id), Some(tick), "session", &started);
        Ok((id, notes))
    }

    /// A new session with the same scene and seed, in the state of a
    /// snapshot of `from`.
    pub(crate) fn fork_session(&mut self, from: &str, label: &str) -> Result<String> {
        let (scene, seed, snapshot) = {
            let source = self.session(from)?;
            let saved = source
                .saved
                .iter()
                .find(|s| s.label == label)
                .ok_or_else(|| unknown_snapshot(label, &source.saved))?;
            (source.scene.clone(), source.seed, saved.snapshot.clone())
        };
        let (id, _) = self.start_session(&scene, seed, Vec::new())?;
        let restored = self
            .session_mut(&id)
            .and_then(|session| session.app.restore(&snapshot));
        if let Err(problem) = restored {
            self.sessions.open.remove(&id);
            return Err(problem);
        }
        Ok(id)
    }

    /// Runs ticks in a session; the inspector looks at the result.
    pub(crate) fn step_session(
        &mut self,
        id: &str,
        frames: u64,
        until: Option<&Expr>,
        input: &wish_input::Values,
    ) -> Result<(Stepped, u64, String, usize)> {
        let cancel = self.cancel.clone();
        let session = self.session_mut(id)?;
        let mut stepped = step_app(session, frames, until, cancel.as_deref(), input)?;
        stepped.diagnostics.extend(inspect(session));
        let tick = session.app.tick();
        let hash = session.app.hash().to_string();
        let entities = session.app.world().entities().count();
        let logs = session
            .script
            .as_ref()
            .map(wish_script::ScriptRuntime::take_logs)
            .unwrap_or_default();
        for log in logs {
            let message = format!("[{}] {}", log.system, log.message);
            let hint = "Logged by game code (wish_sdk::info!, warn! or error!).";
            let entry = match log.severity {
                Severity::Error => Diagnostic::error("script_log", message, hint),
                Severity::Warning => Diagnostic::warning("script_log", message, hint),
                Severity::Info => Diagnostic::info("script_log", message, hint),
            };
            self.sessions
                .record(Some(id), Some(log.tick), "script", &entry);
        }
        for diagnostic in &stepped.diagnostics {
            let kind = if diagnostic.code == "system_failed" {
                "system"
            } else {
                "inspector"
            };
            self.sessions.record(Some(id), Some(tick), kind, diagnostic);
        }
        Ok((stepped, tick, hash, entities))
    }
}

pub(crate) fn unknown_snapshot(label: &str, saved: &[Saved]) -> Diagnostic {
    let labels: Vec<&str> = saved.iter().map(|s| s.label.as_str()).collect();
    Diagnostic::error(
        "unknown_snapshot",
        format!("The session has no snapshot '{label}'."),
        if labels.is_empty() {
            "Save one first with sim_snapshot.".to_owned()
        } else {
            wish_core::suggest::unknown_name_hint(label, &labels)
        },
    )
    .at_path("/snapshot")
    .with_candidates(labels)
}
