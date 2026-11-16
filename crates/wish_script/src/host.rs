//! What a call's store holds, and the `wish` imports.

use wish_core::{Rng, Severity};

use crate::failure::SourcePlace;
use crate::module::GuestLog;

/// What the guest's panic hook reported.
pub(crate) struct GuestPanic {
    pub message: String,
    pub place: Option<SourcePlace>,
}

/// The most a guest may log in one call, so a loop of `log` calls cannot
/// fill the engine's memory.
const MAX_LOGS_PER_CALL: usize = 100;
/// The longest message kept from the guest, in bytes.
const MAX_MESSAGE: usize = 2000;

/// The state of one call: limits, what the guest reported, its random stream.
pub(crate) struct HostState {
    pub memory_limit: usize,
    /// Set when the guest asked for more memory than its limit.
    pub memory_denied: Option<usize>,
    /// The guest's panic (`wish.panic`), if it panicked.
    pub panic: Option<GuestPanic>,
    pub logs: Vec<GuestLog>,
    pub logs_dropped: u32,
    pub rng: Rng,
}

impl HostState {
    pub fn new(memory_limit: usize, rng: Rng) -> Self {
        Self {
            memory_limit,
            memory_denied: None,
            panic: None,
            logs: Vec::new(),
            logs_dropped: 0,
            rng,
        }
    }
}

impl wasmtime::ResourceLimiter for HostState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_limit {
            self.memory_denied = Some(desired);
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 100_000)
    }
}

/// The names the `wish` module provides. Any other import makes a module
/// unloadable (`script_forbidden_import`).
pub(crate) const IMPORTS: &[&str] = &["log", "panic", "rng_next"];

/// Reads `len` bytes at `ptr` from the guest's memory as text (lossy, cut to
/// [`MAX_MESSAGE`]). Out-of-range pointers give an empty string: a guest
/// cannot make the host fail by passing garbage.
fn guest_text(caller: &mut wasmtime::Caller<'_, HostState>, ptr: i32, len: i32) -> String {
    let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
        return String::new();
    };
    let data = memory.data(&caller);
    let (Ok(start), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return String::new();
    };
    let end = start.saturating_add(len.min(MAX_MESSAGE));
    data.get(start..end.min(data.len()))
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}

/// Adds the `wish` imports to a linker.
pub(crate) fn link(linker: &mut wasmtime::Linker<HostState>) -> wasmtime::Result<()> {
    linker.func_wrap(
        "wish",
        "log",
        |mut caller: wasmtime::Caller<'_, HostState>, severity: i32, ptr: i32, len: i32| {
            let message = guest_text(&mut caller, ptr, len);
            let state = caller.data_mut();
            if state.logs.len() >= MAX_LOGS_PER_CALL {
                state.logs_dropped = state.logs_dropped.saturating_add(1);
                return;
            }
            let severity = match severity {
                2 => Severity::Error,
                1 => Severity::Warning,
                _ => Severity::Info,
            };
            state.logs.push(GuestLog { severity, message });
        },
    )?;
    // panic(message, file, line, column): the SDK's panic hook calls it just
    // before the module aborts. A file of length 0 means "no location".
    linker.func_wrap(
        "wish",
        "panic",
        |mut caller: wasmtime::Caller<'_, HostState>,
         ptr: i32,
         len: i32,
         file_ptr: i32,
         file_len: i32,
         line: i32,
         column: i32| {
            let message = guest_text(&mut caller, ptr, len);
            let file = guest_text(&mut caller, file_ptr, file_len).replace('\\', "/");
            let place = (!file.is_empty()).then(|| SourcePlace {
                function: None,
                file,
                line: u32::try_from(line).unwrap_or(0),
                column: u32::try_from(column).ok().filter(|c| *c > 0),
            });
            caller.data_mut().panic = Some(GuestPanic { message, place });
        },
    )?;
    linker.func_wrap(
        "wish",
        "rng_next",
        |mut caller: wasmtime::Caller<'_, HostState>| -> i64 {
            i64::from_ne_bytes(caller.data_mut().rng.next_u64().to_ne_bytes())
        },
    )?;
    Ok(())
}
