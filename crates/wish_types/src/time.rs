//! Simulation time.

use wish_schema::resource;

/// Simulation time, as seen by systems. Stored in the world as a resource and
/// updated by the app before every tick.
///
/// There is no wall-clock time here on purpose: a simulation that read the
/// clock would not replay the same way twice.
#[resource]
pub struct Time {
    /// The tick being simulated, counting from 0.
    pub tick: u64,
    /// The length of one tick.
    #[schema(unit = "s", default = 1.0 / 60.0)]
    pub delta: f32,
    /// Simulated time at the start of this tick: `tick * delta`, computed
    /// in f64 so that it does not drift.
    #[schema(unit = "s")]
    pub elapsed: f64,
}

impl Time {
    /// The time at the start of `tick`, at `tick_rate_hz` ticks per second.
    ///
    /// Both values come straight from the integer rate, so `elapsed` does not
    /// accumulate the rounding error of `delta` (1/60 is not exact in f32).
    pub fn at(tick: u64, tick_rate_hz: u32) -> Self {
        let rate = f64::from(tick_rate_hz.max(1));
        Self {
            tick,
            delta: (1.0 / rate) as f32,
            elapsed: tick as f64 / rate,
        }
    }
}
