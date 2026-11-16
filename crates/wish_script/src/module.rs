//! Loading a module and calling it.

use wish_core::{Diagnostic, Hash128, Result, Rng, Severity, StableHasher};

use crate::ABI_VERSION;
use crate::engine::ScriptEngine;
use crate::failure::{call_failed, load_failed};
use crate::host::{self, HostState};

/// A message the guest logged during a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestLog {
    /// How serious it is.
    pub severity: Severity,
    /// The text (cut to 2000 bytes).
    pub message: String,
}

/// What a successful call did besides changing the frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallReport {
    /// The fuel the call used: deterministic, so it can be compared between
    /// runs and budgets can be set from it.
    pub fuel_used: u64,
    /// What the guest logged, in order.
    pub logs: Vec<GuestLog>,
    /// Log messages over the per-call limit that were dropped.
    pub logs_dropped: u32,
    /// The value `wish_run` returned.
    pub returned: i32,
}

/// A compiled, validated game module. Calling it never keeps anything in the
/// module: each call is a fresh instance.
pub struct ScriptModule {
    engine: ScriptEngine,
    name: String,
    fingerprint: Hash128,
    pre: wasmtime::InstancePre<HostState>,
}

impl std::fmt::Debug for ScriptModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptModule")
            .field("name", &self.name)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// The exports a module must have, and their types.
const REQUIRED_EXPORTS: &[&str] = &["memory", "wish_abi_version", "wish_alloc", "wish_run"];

impl ScriptModule {
    /// Compiles and validates `bytes`. `name` is how the module is called in
    /// diagnostics (its file, usually).
    pub fn load(engine: &ScriptEngine, name: &str, bytes: &[u8]) -> Result<Self> {
        let module = wasmtime::Module::new(&engine.engine, bytes)
            .map_err(|error| load_failed(name, &error))?;
        for import in module.imports() {
            if import.module() != "wish" || !host::IMPORTS.contains(&import.name()) {
                return Err(Diagnostic::error(
                    "script_forbidden_import",
                    format!(
                        "'{name}' imports '{}.{}', which the engine does not provide. Game code may only use what the engine gives it: no files, network, clock or WASI.",
                        import.module(),
                        import.name()
                    ),
                    "Build game code with the engine's SDK (wish_sdk) for wasm32-unknown-unknown, and remove dependencies that use the operating system.",
                )
                .at_file(name)
                .with_candidates(host::IMPORTS.iter().map(|n| format!("wish.{n}"))));
            }
        }
        for export in REQUIRED_EXPORTS {
            if module.get_export(export).is_none() {
                return Err(Diagnostic::error(
                    "script_missing_export",
                    format!("'{name}' has no '{export}' export, so it is not a Wish Engine game module."),
                    "Build game code with the engine's SDK: it adds the exports (wish_sdk::export! at the end of lib.rs).",
                )
                .at_file(name));
            }
        }
        let mut linker = wasmtime::Linker::new(&engine.engine);
        host::link(&mut linker).map_err(|error| load_failed(name, &error))?;
        let pre = linker
            .instantiate_pre(&module)
            .map_err(|error| load_failed(name, &error))?;
        let mut hasher = StableHasher::new();
        hasher.write_str("wish-script-module-v1");
        hasher.write_bytes(bytes);
        let loaded = Self {
            engine: engine.clone(),
            name: name.to_owned(),
            fingerprint: hasher.finish(),
            pre,
        };
        let version = loaded.abi_version()?;
        if version != ABI_VERSION {
            return Err(Diagnostic::error(
                "script_abi_mismatch",
                format!("'{name}' speaks game ABI version {version}; this engine speaks version {ABI_VERSION}."),
                "Rebuild the game code with this engine's SDK (script_build does this).",
            )
            .at_file(name));
        }
        Ok(loaded)
    }

    /// The module's name, as given to [`ScriptModule::load`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A stable hash of the module's bytes: part of the program fingerprint,
    /// so that "the same build" (ADR-005, D1) covers game code.
    pub fn fingerprint(&self) -> Hash128 {
        self.fingerprint
    }

    pub(crate) fn store(&self, rng: Rng) -> Result<wasmtime::Store<HostState>> {
        let limits = self.engine.limits;
        let mut store = wasmtime::Store::new(
            &self.engine.engine,
            HostState::new(limits.memory_bytes, rng),
        );
        store.limiter(|state| state);
        store
            .set_fuel(limits.fuel_per_call)
            .map_err(|error| call_failed(&self.name, "set up", &error, store.data()))?;
        Ok(store)
    }

    fn abi_version(&self) -> Result<i32> {
        let mut store = self.store(Rng::from_seed(0))?;
        let instance = self
            .pre
            .instantiate(&mut store)
            .map_err(|error| call_failed(&self.name, "start", &error, store.data()))?;
        let version = instance
            .get_typed_func::<(), i32>(&mut store, "wish_abi_version")
            .and_then(|f| f.call(&mut store, ()))
            .map_err(|error| call_failed(&self.name, "wish_abi_version", &error, store.data()))?;
        Ok(version)
    }

    /// A fresh instance, for any number of calls. The engine makes one per
    /// tick (ADR-025) and throws it away after a failed call.
    pub(crate) fn live(&self) -> Result<Live> {
        let mut store = self.store(Rng::from_seed(0))?;
        let parts = (|| -> wasmtime::Result<_> {
            let instance = self.pre.instantiate(&mut store)?;
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| wasmtime::format_err!("the module has no memory"))?;
            let alloc = instance.get_typed_func::<i32, i32>(&mut store, "wish_alloc")?;
            let run = instance.get_typed_func::<(i32, i32), i32>(&mut store, "wish_run")?;
            let manifest = instance
                .get_typed_func::<(), i64>(&mut store, "wish_manifest")
                .ok();
            Ok((memory, alloc, run, manifest))
        })();
        let (memory, alloc, run, manifest) =
            parts.map_err(|error| call_failed(&self.name, "start", &error, store.data()))?;
        Ok(Live {
            name: self.name.clone(),
            fuel: self.engine.limits.fuel_per_call,
            store,
            memory,
            alloc,
            run,
            manifest,
        })
    }

    /// Runs `wish_run(system, frame)` in a fresh instance: the frame is
    /// copied into the guest, the guest may change it in place, and the
    /// changed bytes are copied back into `frame`. `rng` is the system's
    /// random stream; it advances by what the guest drew.
    pub fn run(&self, system: i32, frame: &mut [u8], rng: &mut Rng) -> Result<CallReport> {
        let mut live = self.live()?;
        live.call(system, frame, rng).map(|(report, _)| report)
    }
}

/// The most command bytes one call may return.
const MAX_COMMAND_BYTES: usize = 16 * 1024 * 1024;
/// The longest manifest a module may have.
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

/// An instance that lives for several calls (one tick).
pub(crate) struct Live {
    name: String,
    fuel: u64,
    store: wasmtime::Store<HostState>,
    memory: wasmtime::Memory,
    alloc: wasmtime::TypedFunc<i32, i32>,
    run: wasmtime::TypedFunc<(i32, i32), i32>,
    manifest: Option<wasmtime::TypedFunc<(), i64>>,
}

impl Live {
    fn reset(&mut self, rng: Rng) -> Result<()> {
        let state = self.store.data_mut();
        state.panic = None;
        state.memory_denied = None;
        state.logs.clear();
        state.logs_dropped = 0;
        state.rng = rng;
        let fuel = self.fuel;
        let name = self.name.clone();
        let result = self.store.set_fuel(fuel);
        result.map_err(|error| call_failed(&name, "set up", &error, self.store.data()))
    }

    /// Calls `wish_run(system, frame)`; returns the report and the command
    /// bytes the system gave.
    pub(crate) fn call(
        &mut self,
        system: i32,
        frame: &mut [u8],
        rng: &mut Rng,
    ) -> Result<(CallReport, Vec<u8>)> {
        self.reset(*rng)?;
        let result = self.call_in(system, frame);
        *rng = self.store.data().rng;
        let (returned, commands) = result
            .map_err(|error| call_failed(&self.name, "wish_run", &error, self.store.data()))?;
        if returned == -1 {
            return Err(Diagnostic::error(
                "script_call_failed",
                format!(
                    "'{}' does not know system number {system}, or could not read its frame.",
                    self.name
                ),
                "Rebuild the game code with this engine's SDK (script_build); the module and the engine disagree about the call.",
            )
            .at_file(&self.name));
        }
        let fuel_left = self.store.get_fuel().unwrap_or(0);
        let state = self.store.data_mut();
        Ok((
            CallReport {
                fuel_used: self.fuel.saturating_sub(fuel_left),
                logs: std::mem::take(&mut state.logs),
                logs_dropped: state.logs_dropped,
                returned,
            },
            commands,
        ))
    }

    fn call_in(&mut self, system: i32, frame: &mut [u8]) -> wasmtime::Result<(i32, Vec<u8>)> {
        let len = i32::try_from(frame.len())
            .map_err(|_| wasmtime::format_err!("the frame is larger than 2 GiB"))?;
        let ptr = self.alloc.call(&mut self.store, len)?;
        let start = usize::try_from(ptr)
            .map_err(|_| wasmtime::format_err!("wish_alloc returned a negative pointer"))?;
        self.memory
            .write(&mut self.store, start, frame)
            .map_err(|_| wasmtime::format_err!("wish_alloc returned memory outside the module"))?;
        let returned = self.run.call(&mut self.store, (system, ptr))?;
        self.memory
            .read(&self.store, start, frame)
            .map_err(|_| wasmtime::format_err!("the frame was freed or moved during the call"))?;
        let mut commands = Vec::new();
        if returned > 0 {
            let at = usize::try_from(returned).unwrap_or(usize::MAX);
            let mut len = [0u8; 4];
            self.memory
                .read(&self.store, at, &mut len)
                .map_err(|_| wasmtime::format_err!("the command buffer is outside the module"))?;
            let len = u32::from_le_bytes(len) as usize;
            if len > MAX_COMMAND_BYTES {
                return Err(wasmtime::format_err!(
                    "the system gave {len} bytes of commands; the limit is {MAX_COMMAND_BYTES}"
                ));
            }
            commands = vec![0u8; len];
            self.memory
                .read(&self.store, at + 4, &mut commands)
                .map_err(|_| wasmtime::format_err!("the command buffer is outside the module"))?;
        }
        Ok((returned, commands))
    }

    /// Calls `wish_manifest` and returns the manifest text.
    pub(crate) fn manifest(&mut self) -> Result<String> {
        self.reset(Rng::from_seed(0))?;
        let Some(manifest) = self.manifest.clone() else {
            return Err(Diagnostic::error(
                "script_missing_export",
                format!(
                    "'{}' has no 'wish_manifest' export, so the engine cannot see its types and systems.",
                    self.name
                ),
                "End lib.rs with wish_sdk::export! { systems: [...] }; it writes the export.",
            )
            .at_file(&self.name));
        };
        let result = (|| -> wasmtime::Result<String> {
            let packed = manifest.call(&mut self.store, ())?;
            let ptr = usize::try_from((packed >> 32) & 0xffff_ffff).unwrap_or(usize::MAX);
            let len = usize::try_from(packed & 0xffff_ffff).unwrap_or(usize::MAX);
            if len > MAX_MANIFEST_BYTES {
                return Err(wasmtime::format_err!(
                    "the manifest is {len} bytes; the limit is {MAX_MANIFEST_BYTES}"
                ));
            }
            let mut bytes = vec![0u8; len];
            self.memory
                .read(&self.store, ptr, &mut bytes)
                .map_err(|_| wasmtime::format_err!("the manifest is outside the module"))?;
            String::from_utf8(bytes).map_err(|_| wasmtime::format_err!("the manifest is not UTF-8"))
        })();
        result.map_err(|error| call_failed(&self.name, "wish_manifest", &error, self.store.data()))
    }
}

#[cfg(test)]
mod profile {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::time::Instant;

    /// `SPIKE_WASM=<file> cargo test --release -p wish_script --lib -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement"]
    fn where_a_call_spends_its_time() {
        let Some(path) = std::env::var_os("SPIKE_WASM") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let engine = ScriptEngine::new(crate::Limits::default()).unwrap();
        let module = ScriptModule::load(&engine, "spike", &bytes).unwrap();
        let mut frame = vec![0u8; 8];
        let calls = 5000u32;
        let mut t = [0f64; 5];
        for _ in 0..calls {
            let a = Instant::now();
            let mut store = module.store(Rng::from_seed(1)).unwrap();
            let b = Instant::now();
            let instance = module.pre.instantiate(&mut store).unwrap();
            let c = Instant::now();
            let memory = instance.get_memory(&mut store, "memory").unwrap();
            let alloc = instance
                .get_typed_func::<i32, i32>(&mut store, "wish_alloc")
                .unwrap();
            let run = instance
                .get_typed_func::<(i32, i32), i32>(&mut store, "wish_run")
                .unwrap();
            let ptr = alloc.call(&mut store, 8).unwrap();
            let d = Instant::now();
            memory.write(&mut store, ptr as usize, &frame).unwrap();
            run.call(&mut store, (0, ptr)).unwrap();
            memory.read(&store, ptr as usize, &mut frame).unwrap();
            let e = Instant::now();
            drop(store);
            let f = Instant::now();
            for (slot, d) in t.iter_mut().zip([b - a, c - b, d - c, e - d, f - e]) {
                *slot += d.as_secs_f64();
            }
        }
        let names = ["store", "instantiate", "exports+alloc", "run", "drop"];
        for (name, total) in names.iter().zip(t) {
            println!("{name:>14}: {:>7.2} us", total * 1e6 / f64::from(calls));
        }
    }
}
