//! Hostile game modules, end to end (M5 WP9): manifests that lie, values
//! written back that the world cannot hold, command buffers that are random,
//! huge or aimed at the engine's own data. Whatever a module does, the app
//! keeps running, the world stays whole (it snapshots, restores and hashes
//! the same), and the same module fails the same way every time.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::{Value, json};
use wish_app::{App, AppConfig};
use wish_core::{Entity, Rng};
use wish_schema::wire::to_wire;
use wish_schema::{Plain, component, event};
use wish_script::{GameModule, Limits, Manifest, ScriptEngine, ScriptRuntime};
use wish_sdk::command::{Command, Target};

/// A switch.
#[component]
pub struct Switch {
    /// On or off.
    pub on: bool,
    /// How bright.
    pub level: f32,
}

/// A ping.
#[event]
pub struct Ping {
    /// Its number.
    pub n: u32,
}

const MANIFEST_AT: u32 = 0x1_0000;
const COMMANDS_AT: u32 = 0x10_0000;
const FRAME_AT: u32 = 0x20_0000;

fn escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// A module that reports `manifest`, runs `body` (which may use `$frame`)
/// for every system and returns `commands` when there are any.
fn guest(manifest: &str, body: &str, commands: &[u8], pages: u32) -> Vec<u8> {
    let mut buffer = (commands.len() as u32).to_le_bytes().to_vec();
    buffer.extend(commands);
    let packed = (u64::from(MANIFEST_AT) << 32) | manifest.len() as u64;
    let returned = if commands.is_empty() {
        "(i32.const 0)".to_owned()
    } else {
        format!("(i32.const {COMMANDS_AT})")
    };
    let text = format!(
        r#"(module
  (memory (export "memory") {pages})
  (data (i32.const {MANIFEST_AT}) "{m}")
  (data (i32.const {COMMANDS_AT}) "{c}")
  (func (export "wish_abi_version") (result i32) (i32.const 1))
  (func (export "wish_alloc") (param i32) (result i32) (i32.const {FRAME_AT}))
  (func (export "wish_manifest") (result i64) (i64.const {packed}))
  (func (export "wish_run") (param $system i32) (param $frame i32) (result i32)
    {body}
    {returned}))"#,
        m = escape(manifest.as_bytes()),
        c = escape(&buffer),
    );
    wat::parse_str(text).unwrap()
}

fn types() -> Vec<Value> {
    vec![
        json!({ "role": "component", "schema": to_wire(&Switch::schema()) }),
        json!({ "role": "component", "schema": to_wire(&wish_types::Transform::schema()) }),
        json!({ "role": "event", "schema": to_wire(&Ping::schema()) }),
    ]
}

fn manifest(systems: Value) -> String {
    json!({ "abi": 1, "sdk": "test", "types": types(), "systems": systems }).to_string()
}

fn query(columns: &[(&str, bool)]) -> Value {
    let columns: Vec<Value> = columns
        .iter()
        .map(|(name, write)| json!({ "type": name, "write": write }))
        .collect();
    json!({ "query": { "columns": columns, "with": [], "without": [] } })
}

fn engine() -> ScriptEngine {
    ScriptEngine::new(Limits::default()).unwrap()
}

struct Game {
    app: App,
    _runtime: ScriptRuntime,
    entities: Vec<Entity>,
}

/// An app running `module`, with a small family of entities: a parent with
/// two children, all with a switch and a transform.
fn game(module: &Arc<GameModule>) -> Game {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    module.install(app.world_mut()).unwrap();
    let runtime = ScriptRuntime::new(Arc::clone(module));
    for system in runtime.systems() {
        app.add_system(system).unwrap();
    }
    let world = app.world_mut();
    let mut entities = Vec::new();
    for i in 0..3 {
        let entity = world.spawn().unwrap();
        world
            .set_json(entity, "Switch", &json!({ "on": true, "level": i }))
            .unwrap();
        world
            .set_json(entity, "Transform", &json!({ "translation": [i, 0, 0] }))
            .unwrap();
        entities.push(entity);
    }
    world.set_parent(entities[1], entities[0]).unwrap();
    world.set_parent(entities[2], entities[0]).unwrap();
    Game {
        app,
        _runtime: runtime,
        entities,
    }
}

fn load(bytes: &[u8]) -> Arc<GameModule> {
    GameModule::load(&engine(), "scripts/game.wasm", bytes).unwrap()
}

/// The world is whole: a snapshot restores to the same hash, in a world
/// that has been changed in between.
fn assert_whole(app: &mut App) {
    let world = app.world_mut();
    let before = world.hash();
    let snapshot = world.snapshot();
    let extra = world.spawn().unwrap();
    world.despawn(extra).unwrap();
    world.restore(&snapshot).unwrap();
    assert_eq!(world.hash(), before, "the world restores to itself");
    for entity in world.entities().collect::<Vec<_>>() {
        world.entity_json(entity).unwrap();
    }
}

/// Runs `ticks` ticks; returns every failure message, in order.
fn play(app: &mut App, ticks: usize) -> Vec<String> {
    let mut failures = Vec::new();
    for _ in 0..ticks {
        let report = app.step().unwrap();
        failures.extend(report.failures.iter().map(|f| f.message.clone()));
    }
    failures
}

#[test]
fn values_written_back_are_checked_like_any_input() {
    // Every byte of the frame becomes 2: `on` is no longer a bool (`level`
    // is a tiny but finite number).
    let fill = "(memory.fill (local.get $frame) (i32.const 2) (i32.const 4096))";
    let systems = json!([{ "name": "smash", "params": [query(&[("Switch", true)])] }]);
    let module = load(&guest(&manifest(systems), fill, &[], 64));
    let mut game = game(&module);
    let before = game.app.world().hash();
    let failures = play(&mut game.app, 3);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("[script_write_failed]"),
        "{failures:?}"
    );
    assert!(failures[0].contains("'smash'"));
    let world = game.app.world();
    assert_eq!(
        world.get_json(game.entities[1], "Switch").unwrap(),
        json!({ "on": true, "level": 1.0 }),
        "nothing of the bad write landed"
    );
    assert_ne!(world.hash(), before, "time went on");
    assert_eq!(game.app.disabled_systems().len(), 1);
    assert_whole(&mut game.app);
}

#[test]
fn not_a_number_cannot_be_written_into_the_world() {
    // Every float of the transforms becomes NaN.
    let systems = json!([{ "name": "poison", "params": [query(&[("Transform", true)])] }]);
    let fill = "(memory.fill (local.get $frame) (i32.const 0xff) (i32.const 4096))";
    let module = load(&guest(&manifest(systems), fill, &[], 64));
    let mut game = game(&module);
    let failures = play(&mut game.app, 2);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("[script_not_finite]"), "{failures:?}");
    assert!(
        failures[0].contains("Transform.translation[0]"),
        "{failures:?}"
    );
    let translation = game
        .app
        .world()
        .get_json(game.entities[2], "Transform")
        .unwrap()["translation"]
        .clone();
    assert_eq!(translation, json!([2.0, 0.0, 0.0]));
    assert_whole(&mut game.app);
}

#[test]
fn the_engines_own_data_cannot_be_written() {
    let hierarchy = |game: &Game| {
        game.app
            .world()
            .get_json(game.entities[1], "Hierarchy")
            .unwrap()
    };
    // A query that writes the hierarchy.
    let systems = json!([{ "name": "rewire", "params": [query(&[("Hierarchy", true)])] }]);
    let fill = "(memory.fill (local.get $frame) (i32.const 0) (i32.const 4096))";
    let module = load(&guest(&manifest(systems), fill, &[], 64));
    let mut game = game(&module);
    let before = hierarchy(&game);
    let failures = play(&mut game.app, 2);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("Hierarchy"), "{failures:?}");
    assert_eq!(hierarchy(&game), before);
    assert_whole(&mut game.app);

    // Commands that insert or remove it.
    let size = {
        let world = game.app.world();
        let id = world.component_id("Hierarchy").unwrap();
        world.registry().component(id).unwrap().size()
    };
    for command in [
        Command::Insert {
            target: Target::Existing(game.entities[1]),
            component: "Hierarchy".to_owned(),
            bytes: vec![0; size],
        },
        Command::Remove {
            target: Target::Existing(game.entities[1]),
            component: "Hierarchy".to_owned(),
        },
    ] {
        let mut bytes = Vec::new();
        command.encode(&mut bytes);
        let systems = json!([{ "name": "meddle", "params": [{ "commands": {} }] }]);
        let module = load(&guest(&manifest(systems), "", &bytes, 64));
        let mut other = game_with_entities(&module, &game.entities);
        let before = hierarchy(&other);
        let failures = play(&mut other.app, 2);
        assert!(
            failures.is_empty(),
            "a refused command is not a crash: {failures:?}"
        );
        assert_eq!(hierarchy(&other), before, "{command:?}");
        let logs = other._runtime.take_logs();
        assert!(
            logs.iter()
                .any(|log| log.message.contains("[script_command_failed]")
                    && log.message.contains("Hierarchy")),
            "the refusal is reported: {logs:?}"
        );
        assert_whole(&mut other.app);
    }
}

/// [`game`], checking the entities have the same ids as `expected` (so that
/// command buffers made for one game aim at the same entities in another).
fn game_with_entities(module: &Arc<GameModule>, expected: &[Entity]) -> Game {
    let game = game(module);
    assert_eq!(game.entities, expected);
    game
}

#[test]
fn a_module_cannot_flood_the_world() {
    // One instruction fills 16 MB with 0x01: three million spawn commands.
    let len = 16_000_000u32;
    let flood = format!(
        "(i32.store (i32.const {COMMANDS_AT}) (i32.const {len}))
         (memory.fill (i32.const {}) (i32.const 1) (i32.const {len}))
         (return (i32.const {COMMANDS_AT}))",
        COMMANDS_AT + 4
    );
    let systems = json!([{ "name": "flood", "params": [{ "commands": {} }] }]);
    let module = load(&guest(&manifest(systems), &flood, &[], 400));
    let mut game = game(&module);
    let count = game.app.world().entity_count();
    let failures = play(&mut game.app, 3);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("[script_command_failed]"),
        "{failures:?}"
    );
    assert!(failures[0].contains("commands"), "{failures:?}");
    assert_eq!(
        game.app.world().entity_count(),
        count,
        "nothing of the flood was applied"
    );
    assert_whole(&mut game.app);
}

#[test]
fn a_module_cannot_declare_types_of_unbounded_size() {
    // Sixty-four resources of almost a mebibyte each.
    let big = |i: usize| {
        json!({ "role": "resource", "schema": {
            "name": format!("Big{i}"), "doc": "Big.", "size": 1_000_000, "align": 1,
            "kind": { "struct": [{ "name": "bytes", "doc": "Bytes.", "offset": 0,
                "ty": { "name": "[u8; 1000000]", "doc": "", "size": 1_000_000, "align": 1,
                    "kind": { "array": { "len": 1_000_000,
                        "element": { "name": "u8", "doc": "", "size": 1, "align": 1, "kind": "u8" } } } } }] } } })
    };
    let text = json!({ "abi": 1, "sdk": "test", "types": (0..64).map(big).collect::<Vec<_>>(), "systems": [] })
        .to_string();
    let refused = Manifest::parse("scripts/game.wasm", &text).unwrap_err();
    assert_eq!(refused.code, "script_bad_manifest", "{refused:?}");
    assert!(refused.message.contains("MiB"), "{refused:?}");
    // A few are fine.
    let text = json!({ "abi": 1, "sdk": "test", "types": (0..4).map(big).collect::<Vec<_>>(), "systems": [] })
        .to_string();
    Manifest::parse("scripts/game.wasm", &text).unwrap();
}

#[test]
fn despawning_what_is_already_gone_is_not_an_error() {
    // Despawning one entity twice, or a child after its parent (which took
    // the child with it), is ordinary game code: two players reaching the
    // same coin in one tick. It is not a failure, and nothing is reported.
    let systems = json!([{ "name": "tidy", "params": [{ "commands": {} }] }]);
    let probe = game(&load(&guest(&manifest(systems.clone()), "", &[], 64)));
    let [parent, child, _] = probe.entities[..] else {
        panic!()
    };
    let mut bytes = Vec::new();
    for target in [child, child, parent, child] {
        Command::Despawn(Target::Existing(target)).encode(&mut bytes);
    }
    let module = load(&guest(&manifest(systems), "", &bytes, 64));
    let mut game = game_with_entities(&module, &probe.entities);
    let failures = play(&mut game.app, 3);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(game._runtime.take_logs().is_empty());
    assert_eq!(game.app.world().entity_count(), 0);
    assert!(game.app.disabled_systems().is_empty());
}

// ---------------------------------------------------------------- fuzzing

fn pick<'a, T>(rng: &mut Rng, items: &'a [T]) -> &'a T {
    &items[(rng.next_u64() % items.len() as u64) as usize]
}

fn random_target(rng: &mut Rng, entities: &[Entity]) -> Target {
    match rng.next_u64() % 4 {
        0 => Target::New((rng.next_u64() % 4) as u32),
        1 => Target::Existing(Entity::from_bits(rng.next_u64())),
        _ => Target::Existing(*pick(rng, entities)),
    }
}

fn random_bytes(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.next_u64() as u8).collect()
}

/// A command buffer: well-formed commands with wrong targets, names and
/// sizes, sometimes cut short or followed by noise.
fn random_commands(rng: &mut Rng, entities: &[Entity], sizes: &[(&str, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..rng.next_u64() % 12 {
        let (name, size) = *pick(rng, sizes);
        let size = if rng.next_u64().is_multiple_of(5) {
            (rng.next_u64() % 64) as usize
        } else {
            size
        };
        let command = match rng.next_u64() % 7 {
            0 => Command::Spawn {
                index: rng.next_u64() as u32,
            },
            1 => Command::Despawn(random_target(rng, entities)),
            2 => Command::Insert {
                target: random_target(rng, entities),
                component: name.to_owned(),
                bytes: random_bytes(rng, size),
            },
            3 => Command::Remove {
                target: random_target(rng, entities),
                component: name.to_owned(),
            },
            4 => Command::Send {
                event: name.to_owned(),
                bytes: random_bytes(rng, size),
            },
            5 => Command::SetParent {
                child: random_target(rng, entities),
                parent: random_target(rng, entities),
            },
            _ => Command::RemoveParent(random_target(rng, entities)),
        };
        command.encode(&mut out);
    }
    match rng.next_u64() % 6 {
        0 if !out.is_empty() => {
            let cut = (rng.next_u64() % out.len() as u64) as usize;
            out.truncate(cut);
        }
        1 => {
            let len = (rng.next_u64() % 32) as usize;
            let noise = random_bytes(rng, len);
            out.extend(noise);
        }
        _ => {}
    }
    out
}

#[test]
fn random_command_buffers_never_break_the_world() {
    let systems = json!([{ "name": "chaos", "params": [{ "commands": {} }] }]);
    let text = manifest(systems);
    let probe = load(&guest(&text, "", &[], 64));
    let reference = game(&probe);
    let entities = reference.entities.clone();
    let sizes = {
        let world = reference.app.world();
        let size = |name: &str| {
            let registry = world.registry();
            registry
                .component_id(name)
                .and_then(|id| registry.component(id))
                .map(|info| info.size())
                .or_else(|| {
                    registry
                        .event_id(name)
                        .and_then(|id| registry.event(id))
                        .map(|info| info.size())
                })
                .unwrap_or(4)
        };
        [
            "Switch",
            "Transform",
            "Hierarchy",
            "Ping",
            "GlobalTransform",
            "Nope",
            "",
        ]
        .map(|name| (name, size(name)))
    };
    let mut rng = Rng::from_seed(2026);
    for case in 0..120 {
        let commands = random_commands(&mut rng, &entities, &sizes);
        let module = load(&guest(&text, "", &commands, 64));
        let mut outcomes = Vec::new();
        for _ in 0..2 {
            let mut game = game_with_entities(&module, &entities);
            let failures = play(&mut game.app, 3);
            let logs: Vec<String> = game
                ._runtime
                .take_logs()
                .into_iter()
                .map(|log| log.message)
                .collect();
            assert_whole(&mut game.app);
            outcomes.push((failures, logs, game.app.world().hash()));
        }
        assert_eq!(outcomes[0], outcomes[1], "case {case} is deterministic");
    }
}

/// Replaces a random part of `value` with something it should not be.
fn mutate(rng: &mut Rng, value: &mut Value, depth: usize) {
    let children = match value {
        Value::Array(items) => items.len(),
        Value::Object(map) => map.len(),
        _ => 0,
    };
    if children > 0 && depth < 12 && !rng.next_u64().is_multiple_of(4) {
        let index = (rng.next_u64() % children as u64) as usize;
        match value {
            Value::Array(items) => {
                if rng.next_u64().is_multiple_of(8) {
                    let copy = items[index].clone();
                    items.push(copy);
                } else if rng.next_u64().is_multiple_of(8) {
                    items.remove(index);
                } else {
                    mutate(rng, &mut items[index], depth + 1);
                }
            }
            Value::Object(map) => {
                let key = map.keys().nth(index).cloned().unwrap_or_default();
                if rng.next_u64().is_multiple_of(8) {
                    map.remove(&key);
                } else if let Some(child) = map.get_mut(&key) {
                    mutate(rng, child, depth + 1);
                }
            }
            _ => {}
        }
        return;
    }
    let replacements = [
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(1),
        json!(3),
        json!(4_294_967_296u64),
        json!(1e300),
        json!(""),
        json!("Transform"),
        json!("Hierarchy"),
        json!("Switch"),
        json!("u8"),
        json!("struct"),
        json!([]),
        json!({}),
        json!({ "commands": {} }),
    ];
    let choice = pick(rng, &replacements).clone();
    *value = choice;
}

#[test]
fn manifests_that_lie_are_refused_or_survived() {
    let base: Value = serde_json::from_str(&manifest(json!([
        { "name": "a", "before": ["b"], "after": [], "params": [
            query(&[("Switch", true), ("Transform", false)]),
            { "events": { "type": "Ping" } }, { "send": { "type": "Ping" } }, { "commands": {} } ] },
        { "name": "b", "params": [{ "res": { "type": "Time", "write": false } }] }
    ])))
    .unwrap();
    let mut rng = Rng::from_seed(7);
    let mut installed = 0;
    for case in 0..600 {
        let mut value = base.clone();
        for _ in 0..=rng.next_u64() % 3 {
            mutate(&mut rng, &mut value, 0);
        }
        let text = value.to_string();
        let Ok(_) = Manifest::parse("scripts/game.wasm", &text) else {
            continue;
        };
        // Every tenth manifest the parser accepts goes all the way: install
        // and a few ticks of a module that writes nothing.
        if case % 10 != 0 {
            continue;
        }
        let module = load(&guest(&text, "", &[], 64));
        let mut app = App::new(AppConfig::default());
        wish_transform::register(app.world_mut()).unwrap();
        let before = app.world().hash();
        if let Err(refused) = module.install(app.world_mut()) {
            assert!(!refused.hint.is_empty(), "case {case}: {refused:?}");
            assert_eq!(
                app.world().hash(),
                before,
                "case {case}: a refusal changes nothing"
            );
            continue;
        }
        let runtime = ScriptRuntime::new(Arc::clone(&module));
        let mut added = true;
        for system in runtime.systems() {
            added &= app.add_system(system).is_ok();
        }
        if added && app.step().is_ok() {
            play(&mut app, 2);
        }
        assert_whole(&mut app);
        installed += 1;
    }
    assert!(
        installed > 5,
        "the fuzzer reached install {installed} times"
    );
}

/// A big event.
#[event]
pub struct Blast {
    /// Lots of floats.
    pub values: [f32; 65536],
}

#[test]
fn r2_many_refused_sends_of_a_big_event_stay_cheap() {
    // 65536 sends of a 256 KiB event, every one with the wrong size: each
    // is refused at once, without walking the event's layout.
    let mut bytes = Vec::new();
    for _ in 0..65_536 {
        Command::Send {
            event: "Blast".to_owned(),
            bytes: Vec::new(),
        }
        .encode(&mut bytes);
    }
    let text = json!({ "abi": 1, "sdk": "test",
        "types": [{ "role": "event", "schema": to_wire(&Blast::schema()) }],
        "systems": [{ "name": "blast", "params": [{ "commands": {} }] }] })
    .to_string();
    let module = load(&guest(&text, "", &bytes, 64));
    let mut app = App::new(AppConfig::default());
    module.install(app.world_mut()).unwrap();
    let runtime = ScriptRuntime::new(Arc::clone(&module));
    for system in runtime.systems() {
        app.add_system(system).unwrap();
    }
    let started = std::time::Instant::now();
    let failures = play(&mut app, 1);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    let logs = runtime.take_logs();
    assert_eq!(logs.len(), 9, "eight explained, the rest counted");
    assert!(logs[8].message.contains("65528 more"), "{:?}", logs[8]);
}

#[test]
fn r3_a_hierarchy_cannot_grow_without_end() {
    // Each call spawns 300 entities and chains them: the chain stops at the
    // depth limit, and the call stays quick.
    let mut bytes = Vec::new();
    for n in 0..300u32 {
        Command::Spawn { index: n }.encode(&mut bytes);
        if n > 0 {
            Command::SetParent {
                child: Target::New(n),
                parent: Target::New(n - 1),
            }
            .encode(&mut bytes);
        }
    }
    let systems = json!([{ "name": "chain", "params": [{ "commands": {} }] }]);
    let module = load(&guest(&manifest(systems), "", &bytes, 64));
    let mut game = game(&module);
    let failures = play(&mut game.app, 1);
    assert!(failures.is_empty(), "{failures:?}");
    let logs = game._runtime.take_logs();
    assert!(
        logs.iter().any(|l| l.message.contains("256 levels")),
        "{logs:?}"
    );
    assert_whole(&mut game.app);
}

/// Answers every physics question with a fixed hit, to see what reaches
/// the guest.
struct Echo;

impl wish_script::HostServices for Echo {
    fn physics_query(&self, _world: &wish_ecs::World, request: &[u8]) -> Option<Vec<u8>> {
        (request.len() == wish_sdk::physics::REQUEST_LEN).then(|| vec![7; 37])
    }
}

#[test]
fn physics_questions_cannot_hurt_the_host() {
    // Garbage pointers and lengths, an answer that does not fit, then a
    // loop of questions until the fuel runs out.
    let body = format!(
        "(drop (call $q (i32.const -5) (i32.const 999999) (i32.const 0) (i32.const 64)))
         (drop (call $q (i32.const 0) (i32.const {req}) (i32.const 0x7fffff00) (i32.const 64)))
         (drop (call $q (i32.const 0) (i32.const {req}) (local.get $frame) (i32.const 4)))
         (i32.store (i32.const 0x30000) (call $q (i32.const 0) (i32.const {req}) (i32.const 0x30010) (i32.const 64)))
         (loop $again
           (drop (call $q (i32.const 0) (i32.const {req}) (i32.const 0x30010) (i32.const 64)))
           (br $again))",
        req = wish_sdk::physics::REQUEST_LEN
    );
    // The guest needs the import declared, so it is written out here.
    let manifest_text = manifest(json!([{ "name": "ask", "params": [{ "commands": {} }] }]));
    let wat = format!(
        r#"(module
  (import "wish" "physics_query" (func $q (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 64)
  (data (i32.const {MANIFEST_AT}) "{m}")
  (func (export "wish_abi_version") (result i32) (i32.const 1))
  (func (export "wish_alloc") (param i32) (result i32) (i32.const {FRAME_AT}))
  (func (export "wish_manifest") (result i64) (i64.const {packed}))
  (func (export "wish_run") (param $system i32) (param $frame i32) (result i32)
    {body}
    (i32.const 0)))"#,
        m = escape(manifest_text.as_bytes()),
        packed = (u64::from(MANIFEST_AT) << 32) | manifest_text.len() as u64,
    );
    let module = load(&wat::parse_str(wat).unwrap());
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    module.install(app.world_mut()).unwrap();
    let runtime = ScriptRuntime::new(Arc::clone(&module)).with_services(Arc::new(Echo));
    for system in runtime.systems() {
        app.add_system(system).unwrap();
    }
    let report = app.step().unwrap();
    assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
    assert!(
        report.failures[0].message.contains("[script_out_of_fuel]"),
        "{:?}",
        report.failures[0]
    );
    // The app goes on.
    app.step().unwrap();
    assert_whole(&mut app);
}

#[test]
fn texts_from_game_code_cannot_hurt_the_host() {
    // Garbage pointers, a text too long, bytes that are not UTF-8, unknown
    // handles and a flood of new texts: each is refused with -1, nothing
    // traps, and at most 1024 new texts enter the world per call. The guest
    // traps (`unreachable`) if an answer is not the one expected.
    let body = "
      (if (i64.ne (call $t (i32.const -5) (i32.const 10)) (i64.const -1)) (then unreachable))
      (if (i64.ne (call $t (i32.const 0) (i32.const 999999)) (i64.const -1)) (then unreachable))
      (if (i64.ne (call $t (i32.const 0x7ffffff0) (i32.const 64)) (i64.const -1)) (then unreachable))
      (if (i64.ne (call $t (i32.const 0x31100) (i32.const 2)) (i64.const -1)) (then unreachable))
      (if (i32.ne (call $r (i32.const 999999) (i32.const 0x30000) (i32.const 64)) (i32.const -1)) (then unreachable))
      (if (i32.ne (call $r (i32.const -1) (i32.const 0x30000) (i32.const 64)) (i32.const -1)) (then unreachable))
      (local.set $hi (call $t (i32.const 0x31000) (i32.const 2)))
      (if (i64.lt_s (local.get $hi) (i64.const 1)) (then unreachable))
      (if (i64.ne (call $t (i32.const 0x31000) (i32.const 2)) (local.get $hi)) (then unreachable))
      (if (i32.ne (call $r (i32.wrap_i64 (local.get $hi)) (i32.const 0x7fffff00) (i32.const 64)) (i32.const -1)) (then unreachable))
      (if (i32.ne (call $r (i32.wrap_i64 (local.get $hi)) (i32.const 0x30000) (i32.const 1)) (i32.const 2)) (then unreachable))
      (if (i32.ne (i32.load8_u (i32.const 0x30000)) (i32.const 104)) (then unreachable))
      (loop $more
        (i32.store8 (i32.const 0x32000) (i32.add (i32.const 65) (i32.and (local.get $i) (i32.const 15))))
        (i32.store8 (i32.const 0x32001) (i32.add (i32.const 65) (i32.and (i32.shr_u (local.get $i) (i32.const 4)) (i32.const 15))))
        (i32.store8 (i32.const 0x32002) (i32.add (i32.const 65) (i32.and (i32.shr_u (local.get $i) (i32.const 8)) (i32.const 15))))
        (if (i64.ge_s (call $t (i32.const 0x32000) (i32.const 3)) (i64.const 0))
          (then (local.set $ok (i32.add (local.get $ok) (i32.const 1)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br_if $more (i32.lt_u (local.get $i) (i32.const 1500))))
      (if (i32.ne (local.get $ok) (i32.const 1023)) (then unreachable))";
    let manifest_text = manifest(json!([{ "name": "words", "params": [{ "commands": {} }] }]));
    let wat = format!(
        r#"(module
  (import "wish" "text_intern" (func $t (param i32 i32) (result i64)))
  (import "wish" "text_read" (func $r (param i32 i32 i32) (result i32)))
  (memory (export "memory") 64)
  (data (i32.const {MANIFEST_AT}) "{m}")
  (data (i32.const 0x31000) "hi")
  (data (i32.const 0x31100) "\ff\fe")
  (func (export "wish_abi_version") (result i32) (i32.const 1))
  (func (export "wish_alloc") (param i32) (result i32) (i32.const {FRAME_AT}))
  (func (export "wish_manifest") (result i64) (i64.const {packed}))
  (func (export "wish_run") (param $system i32) (param $frame i32) (result i32)
    (local $hi i64) (local $i i32) (local $ok i32)
    {body}
    (i32.const 0)))"#,
        m = escape(manifest_text.as_bytes()),
        packed = (u64::from(MANIFEST_AT) << 32) | manifest_text.len() as u64,
    );
    let module = load(&wat::parse_str(wat).unwrap());
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut()).unwrap();
    module.install(app.world_mut()).unwrap();
    // No host services: texts do not need them.
    let runtime = ScriptRuntime::new(Arc::clone(&module));
    for system in runtime.systems() {
        app.add_system(system).unwrap();
    }
    let before = app.world().texts().len();
    let report = app.step().unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(app.world().texts().len(), before + 1024);
    assert!(app.world().texts().find("hi").is_some());
    // The next tick may add its own: the limit is per call. "hi" and the
    // first 1023 are known now, so the next 1023 are new.
    let report = app.step().unwrap();
    assert_eq!(report.failures.len(), 1, "{:?}", report.failures);
    assert_whole(&mut app);
}
