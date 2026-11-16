//! `wen bench --script`: what running game logic as WebAssembly costs
//! (ADR-011's trigger: the boundary must stay under a fifth of the frame).
//!
//! The same system — move every entity by its velocity — runs natively and
//! as game code built the way `script_build` builds it, on 1, 1 000 and
//! 10 000 entities; a second game system that does nothing shows the fixed
//! cost of a tick (the fresh instance, ADR-025) and of a call.

use std::time::Instant;

use serde_json::json;
use wish_app::{App, AppConfig, System, SystemContext};
use wish_core::math::Vec3;
use wish_core::{Diagnostic, Result};
use wish_ecs::World;
use wish_script::build::{GAME_WASM, SCRIPTS_ENTRY, Toolchain, build_game};
use wish_script::{GameModule, Limits, ScriptEngine, ScriptRuntime};
use wish_transform::Transform;

/// The game code being measured.
const GAME: &str = r#"
use wish_sdk::prelude::*;

/// How fast an entity drifts.
#[component]
pub struct Velocity {
    /// Its velocity.
    #[schema(unit = "m/s")]
    pub value: Vec3,
}

#[system]
fn drift(mut things: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
    for (transform, velocity) in things.iter_mut() {
        transform.translation += velocity.value * time.delta;
    }
}

/// What glance adds up.
#[resource]
pub struct Total {
    /// The sum.
    pub value: f32,
}

#[system]
fn glance(things: Query<(&Transform, &Velocity)>, mut total: ResMut<Total>) {
    let mut sum = 0.0;
    for (transform, velocity) in things.iter() {
        sum += transform.translation.x + velocity.value.y;
    }
    total.value = sum;
}

#[system]
fn idle(time: Res<Time>) {
    let _ = time.tick;
}

wish_sdk::export! {
    components: [Velocity],
    resources: [Total],
    systems: [drift, glance, idle],
}
"#;

mod native {
    use wish_schema::component;

    /// How fast an entity drifts.
    #[component]
    pub struct Velocity {
        /// Its velocity.
        #[schema(unit = "m/s")]
        pub value: wish_core::math::Vec3,
    }
}

fn drift(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<()> {
    let delta = ctx.time.delta;
    for (mut transform, velocity) in world.query::<(&mut Transform, &native::Velocity)>()? {
        transform.translation += velocity.value * delta;
    }
    Ok(())
}

fn velocity(i: u32) -> Vec3 {
    Vec3::new((i % 7) as f32 * 0.1, 1.0, -((i % 5) as f32) * 0.2)
}

/// Milliseconds per tick over `ticks` ticks after a warm-up.
fn time_ticks(app: &mut App, ticks: u32) -> Result<f64> {
    for _ in 0..20 {
        app.step()?;
    }
    let started = Instant::now();
    for _ in 0..ticks {
        let report = app.step()?;
        if let Some(failure) = report.failures.into_iter().next() {
            return Err(failure);
        }
    }
    Ok(started.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks))
}

fn native_app(entities: u32) -> Result<App> {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut())?;
    for i in 0..entities {
        let world = app.world_mut();
        let entity = world.spawn()?;
        world.insert(entity, Transform::default())?;
        world.insert(entity, native::Velocity { value: velocity(i) })?;
    }
    app.add_system(System::new("drift", drift))?;
    Ok(app)
}

fn game_app(module: &std::sync::Arc<GameModule>, entities: u32, only: &str) -> Result<App> {
    let mut app = App::new(AppConfig::default());
    wish_transform::register(app.world_mut())?;
    module.install(app.world_mut())?;
    let id = app.world().component_id("Velocity")?;
    for i in 0..entities {
        let world = app.world_mut();
        let entity = world.spawn()?;
        world.insert(entity, Transform::default())?;
        let v = velocity(i);
        let mut bytes = [0u8; 12];
        for (slot, value) in bytes.chunks_mut(4).zip([v.x, v.y, v.z]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        world.insert_bytes(entity, id, &bytes)?;
    }
    for system in ScriptRuntime::new(std::sync::Arc::clone(module)).systems() {
        if system.name() == only {
            app.add_system(system)?;
        }
    }
    Ok(app)
}

/// Runs the measurement.
pub fn bench_script(ticks: u64, as_json: bool) -> Result<String> {
    let toolchain = Toolchain::locate()?;
    let root = std::env::temp_dir().join(format!("wish-bench-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts/src")).map_err(|error| {
        Diagnostic::error(
            "bench_failed",
            format!("Could not make a folder for the measurement: {error}."),
            "Check the temporary folder can be written to.",
        )
    })?;
    std::fs::write(root.join(SCRIPTS_ENTRY), GAME).map_err(|error| {
        Diagnostic::error(
            "bench_failed",
            format!("Could not write the measured game code: {error}."),
            "Check the temporary folder can be written to.",
        )
    })?;
    let built = build_game(&toolchain, &root, true)?;
    if !built.passed {
        return Err(built.diagnostics.into_iter().next().unwrap_or_else(|| {
            Diagnostic::error(
                "bench_failed",
                "The measured game code did not build.",
                "Check `wen sdk` was run with this engine.",
            )
        }));
    }
    let bytes = std::fs::read(root.join(GAME_WASM)).unwrap_or_default();
    let engine = ScriptEngine::new(Limits::default())?;
    let module = GameModule::load(&engine, GAME_WASM, &bytes)?;
    let ticks = u32::try_from(ticks.clamp(50, 100_000)).unwrap_or(600);
    let budget = 1000.0 / 60.0;

    let idle = time_ticks(&mut game_app(&module, 0, "idle")?, ticks)?;
    let mut rows = Vec::new();
    for entities in [1u32, 1_000, 10_000] {
        let native = time_ticks(&mut native_app(entities)?, ticks)?;
        let game = time_ticks(&mut game_app(&module, entities, "drift")?, ticks)?;
        let read = time_ticks(&mut game_app(&module, entities, "glance")?, ticks)?;
        rows.push((entities, native, game, read));
    }
    let _ = std::fs::remove_dir_all(&root);
    let worst = rows.iter().map(|(_, n, g, _)| g - n).fold(0.0, f64::max);
    let fraction = worst / budget;
    Ok(if as_json {
        json!({
            "ok": true,
            "ticks": ticks,
            "frame_budget_ms": budget,
            "fixed_cost_ms": idle,
            "systems": rows.iter().map(|(e, n, g, r)| json!({
                "entities": e, "native_ms": n, "game_ms": g, "read_only_ms": r, "boundary_ms": g - n,
                "boundary_ns_per_entity": if *e > 0 { (g - n) * 1e6 / f64::from(*e) } else { 0.0 },
            })).collect::<Vec<_>>(),
            "worst_boundary_fraction": fraction,
            "trigger": fraction > 0.2,
        })
        .to_string()
    } else {
        let mut text = format!(
            "Game code cost per tick ({ticks} ticks each; frame budget {budget:.2} ms):\n  a tick with one empty game system (fresh instance + call): {idle:.4} ms\n"
        );
        for (entities, native, game, read) in &rows {
            text.push_str(&format!(
                "  {entities:>6} entities: native {native:.4} ms, game code {game:.4} ms, boundary {:.4} ms ({:.1} ns/entity); reading only {read:.4} ms\n",
                game - native,
                (game - native) * 1e6 / f64::from(*entities)
            ));
        }
        text.push_str(&format!(
            "Worst boundary: {:.2}% of the frame budget ({}).",
            fraction * 100.0,
            if fraction > 0.2 {
                "over ADR-011's 20% trigger: shared-memory views are needed"
            } else {
                "under ADR-011's 20% trigger"
            }
        ));
        text
    })
}
