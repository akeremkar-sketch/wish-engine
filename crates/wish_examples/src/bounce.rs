//! The `bounce` example: bodies that spawn, fall, bounce and die.

use serde_json::json;
use wish_app::{App, AppConfig, System, SystemContext};
use wish_core::math::Vec3;
use wish_core::{Entity, Result, Text};
use wish_ecs::{Changed, Commands, EventCursor, World};
use wish_schema::{component, event, resource};

/// A falling, bouncing body.
#[component]
pub struct Body {
    /// Position in world space.
    #[schema(unit = "m")]
    pub position: Vec3,
    /// Velocity.
    #[schema(unit = "m/s")]
    pub velocity: Vec3,
    /// How much speed survives a bounce, from 0 to 1.
    #[schema(range = 0.0..=1.0, default = 0.8)]
    pub restitution: f32,
    /// Ticks since the body spawned.
    pub age: u32,
}

/// A name shown for some bodies.
#[component]
pub struct Label {
    /// The name.
    pub text: Text,
}

/// A small marker that rides along under a body in the hierarchy; despawned
/// with its body.
#[component]
pub struct Trail {
    /// How far behind its body the marker sits.
    #[schema(unit = "m")]
    pub offset: f32,
}

/// Sent whenever a body hits the floor.
#[event]
pub struct Bounced {
    /// The body that bounced.
    pub body: Entity,
    /// Its downward speed just before the bounce.
    #[schema(unit = "m/s")]
    pub speed: f32,
}

/// Running totals.
#[resource]
pub struct Stats {
    /// Bodies spawned so far.
    pub spawned: u32,
    /// Bodies despawned so far.
    pub despawned: u32,
    /// Bounces counted from `Bounced` events.
    pub bounces: u32,
    /// Bodies whose state changed in the last tick, as seen by change detection.
    pub moved_last_tick: u32,
    /// Where the bounce counter is in the event stream.
    pub bounce_cursor: u64,
}

/// Downward acceleration.
#[resource]
pub struct Gravity {
    /// Acceleration.
    #[schema(unit = "m/s^2", default = Vec3::new(0.0, -9.81, 0.0))]
    pub value: Vec3,
}

const NAMES: [&str; 4] = ["Ada", "Bora", "Cem", "Defne"];

fn spawn(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let count = ctx.rng.below(3);
    let mut commands = Commands::new();
    for _ in 0..count {
        let body = commands.spawn();
        commands.insert(
            body,
            Body {
                position: Vec3::new(
                    ctx.rng.range_f32(-10.0, 10.0),
                    ctx.rng.range_f32(2.0, 20.0),
                    ctx.rng.range_f32(-10.0, 10.0),
                ),
                velocity: Vec3::new(
                    ctx.rng.range_f32(-2.0, 2.0),
                    0.0,
                    ctx.rng.range_f32(-2.0, 2.0),
                ),
                restitution: ctx.rng.range_f32(0.5, 0.95),
                age: 0,
            },
        );
        if ctx.rng.chance(0.25) {
            let name = ctx.rng.pick(&NAMES).copied().unwrap_or("Ada");
            // Labels go through the JSON path, so the example exercises it too.
            commands.set_json(body, "Label", json!({ "text": name }));
        }
        if ctx.rng.chance(0.3) {
            let trail = commands.spawn();
            commands.insert(
                trail,
                Trail {
                    offset: ctx.rng.range_f32(0.1, 1.0),
                },
            );
            commands.set_parent(trail, body);
        }
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

fn integrate(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let gravity = world
        .resource::<Gravity>()
        .map_or(Vec3::new(0.0, -9.81, 0.0), |g| g.value);
    let dt = ctx.time.delta;
    for mut body in world.query::<&mut Body>()? {
        let velocity = body.velocity + gravity * dt;
        body.velocity = velocity;
        body.position += velocity * dt;
        body.age += 1;
    }
    Ok(())
}

fn bounce(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let mut bounced = Vec::new();
    for (entity, mut body) in world.query::<(Entity, &mut Body)>()? {
        if body.position.y < 0.0 && body.velocity.y < 0.0 {
            let speed = -body.velocity.y;
            // A little random energy loss on top of the body's restitution.
            let loss = ctx.rng.range_f32(0.95, 1.0);
            body.position.y = -body.position.y;
            body.velocity.y = speed * body.restitution * loss;
            bounced.push(Bounced {
                body: entity,
                speed,
            });
        }
    }
    for event in bounced {
        world.send(event)?;
    }
    Ok(())
}

fn count_bounces(world: &mut World, _: &mut SystemContext<'_>) -> Result<()> {
    let position = world
        .resource::<Stats>()
        .map_or(0, |stats| stats.bounce_cursor);
    let mut cursor = EventCursor::<Bounced>::from_position(position);
    let new = world.read_events(&mut cursor)?.len() as u32;
    if let Some(mut stats) = world.resource_mut::<Stats>() {
        stats.bounces += new;
        stats.bounce_cursor = cursor.position();
    }
    Ok(())
}

fn watch(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let moved = world
        .query_since::<&Body, Changed<Body>>(ctx.last_run)?
        .count() as u32;
    if let Some(mut stats) = world.resource_mut::<Stats>() {
        stats.moved_last_tick = moved;
    }
    Ok(())
}

fn reap(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let mut commands = Commands::new();
    let mut count = 0;
    for (entity, body) in world.query::<(Entity, &Body)>()? {
        if body.age > 120 && ctx.rng.chance(0.03) {
            commands.despawn(entity);
            count += 1;
        }
    }
    let report = world.apply(commands);
    if let Some(problem) = report.diagnostics.into_iter().next() {
        return Err(problem);
    }
    if let Some(mut stats) = world.resource_mut::<Stats>() {
        stats.despawned += count;
    }
    Ok(())
}

/// Builds the example app.
pub fn build(config: AppConfig) -> Result<App> {
    let mut app = App::new(config);
    let world = app.world_mut();
    world.register_component::<Body>()?;
    world.register_component::<Label>()?;
    world.register_component::<Trail>()?;
    world.register_event::<Bounced>()?;
    world.insert_resource(Stats::default())?;
    world.insert_resource(Gravity::default())?;
    app.add_system(System::new("spawn", spawn).before("integrate"))?;
    app.add_system(System::new("integrate", integrate))?;
    app.add_system(System::new("bounce", bounce).after("integrate"))?;
    app.add_system(System::new("count_bounces", count_bounces).after("bounce"))?;
    app.add_system(System::new("watch", watch).after("bounce"))?;
    app.add_system(
        System::new("reap", reap)
            .after("count_bounces")
            .after("watch"),
    )?;
    Ok(app)
}
