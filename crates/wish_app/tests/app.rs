//! The fixed-step loop, system ordering and determinism.

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use wish_app::{App, AppConfig, System, SystemContext, Time};
use wish_core::math::Vec3;
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::{Changed, Commands, World};
use wish_schema::{component, resource};

#[component]
pub struct Body {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: u32,
}

#[resource]
pub struct Stats {
    pub spawned: u32,
    pub bounces: u32,
}

/// Spawns up to three bodies per tick at random places.
fn spawn(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let mut commands = Commands::new();
    let count = ctx.rng.below(4);
    for _ in 0..count {
        let target = commands.spawn();
        commands.insert(
            target,
            Body {
                position: Vec3::new(
                    ctx.rng.range_f32(-5.0, 5.0),
                    ctx.rng.range_f32(1.0, 10.0),
                    0.0,
                ),
                velocity: Vec3::new(ctx.rng.range_f32(-1.0, 1.0), 0.0, 0.0),
                age: 0,
            },
        );
    }
    let report = world.apply(commands);
    if let Some(problem) = report.diagnostics.into_iter().next() {
        return Err(problem);
    }
    if let Some(mut stats) = world.resource_mut::<Stats>() {
        stats.spawned += count;
    }
    Ok(())
}

/// Gravity and motion.
fn integrate(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let dt = ctx.time.delta;
    for mut body in world.query::<&mut Body>()? {
        body.velocity.y -= 9.81 * dt;
        let velocity = body.velocity;
        body.position += velocity * dt;
        body.age += 1;
    }
    Ok(())
}

/// Bounces off the floor, losing a random amount of energy.
fn bounce(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let mut bounces = 0;
    for mut body in world.query::<&mut Body>()? {
        if body.position.y < 0.0 {
            body.position.y = -body.position.y;
            body.velocity.y = -body.velocity.y * ctx.rng.range_f32(0.5, 0.9);
            bounces += 1;
        }
    }
    if let Some(mut stats) = world.resource_mut::<Stats>() {
        stats.bounces += bounces;
    }
    Ok(())
}

/// Removes old bodies, at random.
fn reap(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let mut commands = Commands::new();
    for (entity, body) in world.query::<(Entity, &Body)>()? {
        if body.age > 90 && ctx.rng.chance(0.05) {
            commands.despawn(entity);
        }
    }
    let report = world.apply(commands);
    report.diagnostics.into_iter().next().map_or(Ok(()), Err)
}

fn noop(_: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    Ok(())
}

fn systems() -> Vec<System> {
    vec![
        System::new("spawn", spawn).before("integrate"),
        System::new("integrate", integrate),
        System::new("bounce", bounce).after("integrate"),
        System::new("reap", reap).after("bounce"),
    ]
}

fn bounce_app(seed: u64, systems: Vec<System>) -> App {
    let mut app = App::new(AppConfig {
        seed,
        ..AppConfig::default()
    });
    app.world_mut().insert_resource(Stats::default()).unwrap();
    for system in systems {
        app.add_system(system).unwrap();
    }
    app
}

/// Everything observable about the world, in a canonical form.
fn observe(world: &World) -> Value {
    let entities: Vec<Value> = world
        .entities()
        .map(|entity| json!({ "id": entity.to_string(), "components": world.entity_json(entity).unwrap() }))
        .collect();
    json!({
        "entities": entities,
        "stats": world.resource_json("Stats").unwrap(),
        "time": world.resource_json("Time").unwrap(),
    })
}

fn run_and_observe(mut app: App, ticks: u64, every: u64) -> Vec<Value> {
    let mut observations = Vec::new();
    for tick in 0..ticks {
        let report = app.step().unwrap();
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        if tick % every == 0 {
            observations.push(observe(app.world()));
        }
    }
    observations.push(observe(app.world()));
    observations
}

#[test]
fn same_seed_same_world_tick_for_tick() {
    let first = run_and_observe(bounce_app(7, systems()), 600, 50);
    let second = run_and_observe(bounce_app(7, systems()), 600, 50);
    assert_eq!(first, second);
    let last = first.last().unwrap();
    assert!(
        last["stats"]["spawned"].as_u64().unwrap() > 500,
        "the example does something"
    );
    assert!(last["stats"]["bounces"].as_u64().unwrap() > 0);
    assert!(!last["entities"].as_array().unwrap().is_empty());

    let other_seed = run_and_observe(bounce_app(8, systems()), 600, 50);
    assert_ne!(first.last(), other_seed.last());
}

#[test]
fn adding_order_and_unrelated_systems_do_not_change_the_outcome() {
    let baseline = run_and_observe(bounce_app(3, systems()), 300, 100);

    let mut reversed = systems();
    reversed.reverse();
    assert_eq!(run_and_observe(bounce_app(3, reversed), 300, 100), baseline);

    // Each system has its own random stream, so a new system that also draws
    // random numbers does not shift the others' streams.
    fn draws(_: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
        for _ in 0..10 {
            ctx.rng.next_u64();
        }
        Ok(())
    }
    let mut extended = systems();
    extended.push(System::new("aaa_draws", draws));
    extended.push(System::new("zzz_noop", noop));
    assert_eq!(run_and_observe(bounce_app(3, extended), 300, 100), baseline);
}

#[test]
fn order_follows_constraints_then_names() {
    let mut app = bounce_app(0, systems());
    app.add_system(System::new("audio", noop)).unwrap();
    app.add_system(System::new("zeta", noop).before("spawn"))
        .unwrap();
    assert_eq!(
        app.system_order().unwrap(),
        ["audio", "zeta", "spawn", "integrate", "bounce", "reap"]
    );
}

#[test]
fn bad_schedules_are_explained_and_nothing_runs() {
    let mut app = App::new(AppConfig::default());
    app.add_system(System::new("a", noop).after("b")).unwrap();
    app.add_system(System::new("b", noop).after("a")).unwrap();
    assert_eq!(app.step().unwrap_err().code, "system_order_cycle");
    assert_eq!(app.tick(), 0);

    let mut app = App::new(AppConfig::default());
    app.add_system(System::new("move", noop).after("inptu"))
        .unwrap();
    app.add_system(System::new("input", noop)).unwrap();
    let error = app.step().unwrap_err();
    assert_eq!(error.code, "unknown_system");
    assert_eq!(error.hint, "Did you mean 'input'?");

    assert_eq!(
        app.add_system(System::new("input", noop)).unwrap_err().code,
        "duplicate_system"
    );
    assert_eq!(
        app.add_system(System::new(" ", noop)).unwrap_err().code,
        "invalid_system_name"
    );
}

#[resource]
pub struct Counter {
    pub value: u32,
}

#[test]
fn a_failing_system_is_disabled_and_the_rest_keep_running() {
    fn count(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
        if let Some(mut counter) = world.resource_mut::<Counter>() {
            counter.value += 1;
        }
        Ok(())
    }
    fn explode(_: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
        if ctx.time.tick == 2 {
            Err(Diagnostic::error(
                "boom",
                "Something broke.",
                "Fix the bug in explode.",
            ))
        } else {
            Ok(())
        }
    }
    let mut app = App::new(AppConfig::default());
    app.world_mut().insert_resource(Counter::default()).unwrap();
    app.add_system(System::new("count", count)).unwrap();
    app.add_system(System::new("explode", explode)).unwrap();

    let failures = app.run(5).unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code, "system_failed");
    assert_eq!(
        failures[0].message,
        "System 'explode' failed at tick 2 and was disabled: [boom] Something broke."
    );
    assert_eq!(failures[0].hint, "Fix the bug in explode.");
    assert_eq!(
        app.world().resource::<Counter>().unwrap().value,
        5,
        "the other system kept running"
    );
    assert_eq!(
        app.disabled_systems().into_iter().collect::<Vec<_>>(),
        ["explode"]
    );

    app.enable_system("explode").unwrap();
    assert!(app.disabled_systems().is_empty());
    assert_eq!(
        app.enable_system("expload").unwrap_err().hint,
        "Did you mean 'explode'?"
    );
}

#[test]
fn systems_see_changes_made_since_their_last_run() {
    fn poke(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
        // Touch one body on even ticks.
        if ctx.time.tick.is_multiple_of(2)
            && let Some(mut body) = world.query::<&mut Body>()?.next()
        {
            body.age += 1;
        }
        Ok(())
    }
    fn watch(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
        let changed = world
            .query_since::<&Body, Changed<Body>>(ctx.last_run)?
            .count() as u32;
        if let Some(mut counter) = world.resource_mut::<Counter>() {
            counter.value += changed;
        }
        Ok(())
    }
    let mut app = App::new(AppConfig::default());
    app.world_mut().insert_resource(Counter::default()).unwrap();
    for _ in 0..3 {
        let entity = app.world_mut().spawn().unwrap();
        app.world_mut().insert(entity, Body::default()).unwrap();
    }
    app.add_system(System::new("poke", poke).before("watch"))
        .unwrap();
    app.add_system(System::new("watch", watch)).unwrap();
    app.run(6).unwrap();
    // Tick 0 sees the three new bodies; ticks 2 and 4 see one poked body each.
    assert_eq!(app.world().resource::<Counter>().unwrap().value, 3 + 2);
}

#[test]
fn time_is_a_tick_counter_not_a_clock() {
    let mut app = App::new(AppConfig {
        tick_rate_hz: 50,
        ..AppConfig::default()
    });
    assert_eq!(app.world().resource::<Time>().unwrap().tick, 0);
    app.run(100).unwrap();
    let time = *app.world().resource::<Time>().unwrap();
    assert_eq!(
        time.tick, 99,
        "the resource describes the last simulated tick"
    );
    assert_eq!(time.delta, 0.02);
    assert!((time.elapsed - 1.98).abs() < 1e-9);
    assert_eq!(app.tick(), 100);
}

#[test]
fn update_turns_real_time_into_whole_ticks() {
    let mut app = App::new(AppConfig::default());
    let report = app.update(0.05).unwrap();
    assert_eq!(report.steps, 3);
    assert!(report.alpha < 1e-3, "{}", report.alpha);
    let report = app.update(1.0 / 120.0).unwrap();
    assert_eq!(report.steps, 0);
    assert!(report.alpha > 0.4 && report.alpha < 0.6, "{}", report.alpha);
    assert_eq!(app.tick(), 3);

    // A huge stall runs at most `max_steps_per_update` ticks and drops the rest.
    let report = app.update(10.0).unwrap();
    assert_eq!(report.steps, 8);
    assert!(report.skipped_steps > 500);
    assert_eq!(app.tick(), 11);

    // Nonsense input is ignored.
    assert_eq!(app.update(f64::NAN).unwrap().steps, 0);
    assert_eq!(app.update(-1.0).unwrap().steps, 0);
}
