//! The Wasm engine and its configuration.

use wish_core::{Diagnostic, Result};

/// How much one call may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Fuel for one call: roughly one unit per executed Wasm instruction.
    /// Deterministic: the same call with the same input always uses the same
    /// fuel and runs out at the same instruction.
    pub fuel_per_call: u64,
    /// The most linear memory a module may have, in bytes.
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // About 50 ms of straight-line code on a desktop CPU: far more
            // than a game system needs per tick, small enough that a runaway
            // loop is noticed at once.
            fuel_per_call: 100_000_000,
            memory_bytes: 64 * 1024 * 1024,
        }
    }
}

/// The engine every module is compiled with. Create one per process (it is
/// cheap to share: modules and calls only borrow it).
#[derive(Clone)]
pub struct ScriptEngine {
    pub(crate) engine: wasmtime::Engine,
    pub(crate) limits: Limits,
}

/// The guest's native stack limit. Deep recursion traps with
/// `script_stack_overflow` instead of overflowing the engine's stack.
const MAX_WASM_STACK: usize = 512 * 1024;

/// How many calls can be in flight at once across the process.
const POOL_SLOTS: u32 = 64;

impl ScriptEngine {
    /// An engine with the given limits.
    pub fn new(limits: Limits) -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config
            .consume_fuel(true)
            // Determinism (ADR-005): NaN bit patterns are the same on every
            // CPU, and only proposals whose results do not depend on the
            // machine or on timing are on.
            .cranelift_nan_canonicalization(true)
            .wasm_relaxed_simd(false)
            .relaxed_simd_deterministic(true)
            .wasm_memory64(false)
            .wasm_multi_memory(false)
            .max_wasm_stack(MAX_WASM_STACK)
            .wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        // Instances are made often (design note §6): the pooling allocator
        // keeps instance slots and memories ready instead of mapping new
        // ones (WP1: about 20% faster than on-demand on Windows).
        let mut pool = wasmtime::PoolingAllocationConfig::default();
        pool.total_core_instances(POOL_SLOTS)
            .total_memories(POOL_SLOTS)
            .total_tables(POOL_SLOTS)
            .max_memory_size(limits.memory_bytes);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pool));
        let engine = wasmtime::Engine::new(&config).map_err(|error| {
            Diagnostic::error(
                "script_engine_failed",
                format!("The WebAssembly engine could not start: {error:#}"),
                "This is a problem with the engine on this machine, not with the game code; report it with the message.",
            )
        })?;
        Ok(Self { engine, limits })
    }

    /// The limits every call gets.
    pub fn limits(&self) -> Limits {
        self.limits
    }
}
