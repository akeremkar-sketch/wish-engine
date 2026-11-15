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

/// The names of the built-in examples.
pub const NAMES: &[&str] = &["bounce", "stress"];

/// Builds the example called `name`.
pub fn build(name: &str, config: AppConfig) -> Result<App> {
    match name {
        "bounce" => bounce::build(config),
        "stress" => stress::build(config),
        _ => Err(Diagnostic::error(
            "unknown_example",
            format!("There is no built-in example called '{name}'."),
            unknown_name_hint(name, NAMES),
        )
        .with_candidates(NAMES.iter().copied())),
    }
}
