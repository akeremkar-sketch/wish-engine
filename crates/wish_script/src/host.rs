//! What a call's store holds, and the `wish` imports.

use std::sync::Arc;

use wish_core::{Rng, Severity};
use wish_ecs::World;

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

/// What the engine answers for game code during a call, beyond the frame and
/// the world's texts: physics questions today (`wish.physics_query`).
/// Implemented by the control plane, which knows the subsystems;
/// `wish_script` does not.
pub trait HostServices: Send + Sync {
    /// Answers a physics request (encoded by `wish_sdk::physics`) about
    /// `world` as it was when the call began; `None` if it is not a valid
    /// request.
    fn physics_query(&self, world: &World, request: &[u8]) -> Option<Vec<u8>>;

    /// [`HostServices::physics_query`], with the work the answer took in
    /// units the engine charges [`WORK_FUEL`] fuel each for (segment tests
    /// of a line-of-sight question; deterministic: the same world and
    /// question take the same work). By default no work beyond the query's
    /// fixed price.
    fn physics_query_with_work(&self, world: &World, request: &[u8]) -> Option<(Vec<u8>, u64)> {
        self.physics_query(world, request).map(|answer| (answer, 0))
    }
}

/// The fuel one unit of a query's work costs (one wall segment tested by a
/// line-of-sight question).
pub const WORK_FUEL: u64 = 4;

/// The world a call may ask about (and add texts to), by address.
pub(crate) struct WorldRef(*mut World);

// SAFETY: a `WorldRef` is created from a `&mut World` just before a guest
// call and dropped (the `query` field cleared) right after it, on the same
// thread, while the caller keeps the world borrowed and does not touch it
// until the call returns; it is only `Send` so that the store holding it can
// live in a `Mutex`.
#[allow(unsafe_code)]
unsafe impl Send for WorldRef {}

impl WorldRef {
    /// The address of `world`, valid while the caller keeps it borrowed.
    pub(crate) fn new(world: &mut World) -> Self {
        Self(world as *mut World)
    }
}

/// The world and services a call may ask, while it runs.
pub(crate) struct QueryContext {
    pub world: WorldRef,
    pub services: Option<Arc<dyn HostServices>>,
}

/// The longest text game code may add to the world, in bytes.
pub const MAX_TEXT_BYTES: usize = 4096;
/// The most new texts one call may add to the world.
pub const MAX_NEW_TEXTS_PER_CALL: u32 = 1024;
/// The most the world's text table may hold, in bytes, for game code to
/// add to it: texts are kept for the life of the world (snapshots copy
/// them, the hash reads them), so a game making a new text every tick must
/// not grow it without end (M6 closing review). Beyond it new texts are
/// refused; texts the world already has are still found.
pub const MAX_WORLD_TEXT_BYTES: usize = 16 * 1024 * 1024;
/// The most distinct texts the world may hold for game code to add one.
pub const MAX_WORLD_TEXTS: usize = 1_000_000;
/// The fuel adding or reading a text costs, plus one per byte.
const TEXT_FUEL: u64 = 2_000;

/// The fuel one physics question costs: a fixed price, so a guest cannot
/// ask without limit and the cost stays deterministic.
const QUERY_FUEL: u64 = 20_000;
/// The longest request read from the guest.
const MAX_REQUEST: usize = 256;

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
    /// What the guest may ask about during this call.
    pub query: Option<QueryContext>,
    /// How many texts the call added to the world.
    pub texts_added: u32,
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
            query: None,
            texts_added: 0,
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
pub(crate) const IMPORTS: &[&str] = &[
    "log",
    "panic",
    "rng_next",
    "physics_query",
    "text_intern",
    "text_read",
];

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
    // physics_query(request, len, out, capacity) -> bytes written, or -1.
    linker.func_wrap(
        "wish",
        "physics_query",
        |mut caller: wasmtime::Caller<'_, HostState>,
         request: i32,
         len: i32,
         out: i32,
         capacity: i32|
         -> i32 {
            let fuel = caller.get_fuel().unwrap_or(0);
            let _ = caller.set_fuel(fuel.saturating_sub(QUERY_FUEL));
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                return -1;
            };
            let (Ok(start), Ok(len)) = (usize::try_from(request), usize::try_from(len)) else {
                return -1;
            };
            if len > MAX_REQUEST {
                return -1;
            }
            let Some(bytes) = memory
                .data(&caller)
                .get(start..start.saturating_add(len))
                .map(<[u8]>::to_vec)
            else {
                return -1;
            };
            let answer = {
                let Some(context) = caller.data().query.as_ref() else {
                    return -1;
                };
                // SAFETY: `query` is only set for the duration of a call by
                // `Live::call`, from a `&World` its caller keeps borrowed
                // and does not change until the call returns (see
                // `WorldRef`); this closure runs inside that call.
                #[allow(unsafe_code)]
                let world = unsafe { &*context.world.0 };
                let Some(services) = context.services.as_ref() else {
                    return -1;
                };
                services.physics_query_with_work(world, &bytes)
            };
            let Some((answer, work)) = answer else {
                return -1;
            };
            let fuel = caller.get_fuel().unwrap_or(0);
            let _ = caller.set_fuel(fuel.saturating_sub(work.saturating_mul(WORK_FUEL)));
            let (Ok(at), Ok(capacity)) = (usize::try_from(out), usize::try_from(capacity)) else {
                return -1;
            };
            if answer.len() > capacity || memory.write(&mut caller, at, &answer).is_err() {
                return -1;
            }
            i32::try_from(answer.len()).unwrap_or(-1)
        },
    )?;
    // text_intern(ptr, len) -> the handle of that (UTF-8) text in the
    // world's text table, added if new; -1 if refused (not UTF-8, too long,
    // too many new texts in one call, or outside a call).
    linker.func_wrap(
        "wish",
        "text_intern",
        |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| -> i64 {
            let (Ok(start), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
                return -1;
            };
            let fuel = caller.get_fuel().unwrap_or(0);
            let _ = caller.set_fuel(fuel.saturating_sub(TEXT_FUEL + len as u64));
            if len > MAX_TEXT_BYTES {
                return -1;
            }
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                return -1;
            };
            let Some(text) = memory
                .data(&caller)
                .get(start..start.saturating_add(len))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(str::to_owned)
            else {
                return -1;
            };
            let state = caller.data_mut();
            let Some(context) = state.query.as_ref() else {
                return -1;
            };
            // SAFETY: `query` is only set for the duration of a call by
            // `Live::call`, from a `&mut World` its caller keeps borrowed and
            // does not use until the call returns (see `WorldRef`); this
            // closure runs inside that call, and nothing else holds a
            // reference into the world meanwhile.
            #[allow(unsafe_code)]
            let world = unsafe { &mut *context.world.0 };
            let known = world.texts().find(&text);
            let full = world.texts().bytes() + text.len() > MAX_WORLD_TEXT_BYTES
                || world.texts().len() >= MAX_WORLD_TEXTS;
            if known.is_none() && (state.texts_added >= MAX_NEW_TEXTS_PER_CALL || full) {
                if full && state.logs.len() < MAX_LOGS_PER_CALL {
                    state.logs.push(GuestLog {
                        severity: Severity::Warning,
                        message: format!(
                            "The world holds as much text as game code may add ({} MiB, {MAX_WORLD_TEXTS} texts); a new text was refused. Reuse texts (a fixed set of messages) rather than making a new one every tick.",
                            MAX_WORLD_TEXT_BYTES / (1024 * 1024)
                        ),
                    });
                }
                return -1;
            }
            match known.map_or_else(|| world.intern(&text), Ok) {
                Ok(handle) => {
                    if known.is_none() {
                        state.texts_added += 1;
                    }
                    i64::from(handle.index())
                }
                Err(_) => -1,
            }
        },
    )?;
    // text_read(handle, out, capacity) -> the text's length in bytes (its
    // first `capacity` bytes written at `out`), or -1 for an unknown handle.
    linker.func_wrap(
        "wish",
        "text_read",
        |mut caller: wasmtime::Caller<'_, HostState>,
         handle: i32,
         out: i32,
         capacity: i32|
         -> i32 {
            let Ok(index) = u32::try_from(handle) else {
                return -1;
            };
            let text = {
                let Some(context) = caller.data().query.as_ref() else {
                    return -1;
                };
                // SAFETY: as for `text_intern`; only read here.
                #[allow(unsafe_code)]
                let world = unsafe { &*context.world.0 };
                let Some(text) = world.text(wish_core::Text::from_index(index)) else {
                    return -1;
                };
                text.as_bytes().to_vec()
            };
            let fuel = caller.get_fuel().unwrap_or(0);
            let _ = caller.set_fuel(fuel.saturating_sub(TEXT_FUEL + text.len() as u64));
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                return -1;
            };
            let (Ok(at), Ok(capacity)) = (usize::try_from(out), usize::try_from(capacity)) else {
                return -1;
            };
            let shown = &text[..text.len().min(capacity)];
            if !shown.is_empty() && memory.write(&mut caller, at, shown).is_err() {
                return -1;
            }
            i32::try_from(text.len()).unwrap_or(-1)
        },
    )?;
    Ok(())
}
