//! Built-in example worlds.
//!
//! Until scene files (M2) and script modules (M5) exist, these are the only
//! way to run the engine end to end. They are also the workload of the
//! determinism tests and benchmarks, so each one exercises as much of the
//! core as it can.
//!
//! | Name | What it does |
//! |---|---|
//! | `bounce` | Bodies spawn at random, fall, bounce with random energy loss, carry tagged children and die at random. Uses commands, events, the hierarchy, text and change detection. |
//! | `stress` | Many entities moving and spinning; the benchmark workload (`wen bench`). |

pub mod bounce;
pub mod stress;

use wish_app::{App, AppConfig};
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::World;

/// The names of the built-in examples.
pub const NAMES: &[&str] = &["bounce", "stress"];

/// Registers the component, resource and event types of the example called
/// `name` in `world`, without its systems or entities. Scene files can use
/// these types, which is how `wen run --scene` has types before game
/// modules exist (M5).
pub fn register_types(name: &str, world: &mut World) -> Result<()> {
    match name {
        "bounce" => {
            world.register_component::<bounce::Body>()?;
            world.register_component::<bounce::Label>()?;
            world.register_component::<bounce::Trail>()?;
            world.register_event::<bounce::Bounced>()?;
            world.register_resource::<bounce::Stats>()?;
            world.register_resource::<bounce::Gravity>()?;
            Ok(())
        }
        "stress" => {
            world.register_component::<stress::Position>()?;
            world.register_component::<stress::Velocity>()?;
            world.register_component::<stress::Spin>()?;
            Ok(())
        }
        _ => Err(unknown_example(name)),
    }
}

fn unknown_example(name: &str) -> Diagnostic {
    Diagnostic::error(
        "unknown_example",
        format!("There is no built-in example called '{name}'."),
        unknown_name_hint(name, NAMES),
    )
    .with_candidates(NAMES.iter().copied())
}

/// Builds the example called `name`.
pub fn build(name: &str, config: AppConfig) -> Result<App> {
    match name {
        "bounce" => bounce::build(config),
        "stress" => stress::build(config),
        _ => Err(unknown_example(name)),
    }
}
