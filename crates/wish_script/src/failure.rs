//! Turning what went wrong inside the guest into diagnostics.

use wish_core::Diagnostic;

use crate::host::HostState;

/// A module that could not be compiled or linked.
pub(crate) fn load_failed(name: &str, error: &wasmtime::Error) -> Diagnostic {
    Diagnostic::error(
        "script_invalid_module",
        format!("'{name}' is not a valid WebAssembly module: {error:#}"),
        "Build game code with script_build (wasm32-unknown-unknown with the engine's SDK); do not edit the .wasm file by hand.",
    )
    .at_file(name)
}

/// A place in the guest's source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourcePlace {
    pub function: Option<String>,
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// Whether a source file belongs to the Rust standard library or a
/// downloaded crate rather than to the game: the place to report is the
/// game's own line that led there.
fn is_library(file: &str) -> bool {
    file.starts_with("/rustc/")
        || file.contains("/.cargo/registry/")
        || file.contains("/rustlib/")
        // The engine's own crates the SDK is made of: the place to report is
        // the game's line that called into them.
        || ["wish_sdk", "wish_sdk_macros", "wish_schema", "wish_core", "wish_types"]
            .iter()
            .any(|krate| file.contains(&format!("/{krate}/src/")))
}

/// Where in the game's source the failure happened: the innermost frame in
/// game code that has line information (game code is built with line tables).
fn place(error: &wasmtime::Error) -> Option<SourcePlace> {
    let trace = error.downcast_ref::<wasmtime::WasmBacktrace>()?;
    trace.frames().iter().find_map(|frame| {
        frame.symbols().iter().find_map(|symbol| {
            let file = symbol.file()?.replace('\\', "/");
            if is_library(&file) {
                return None;
            }
            Some(SourcePlace {
                function: symbol
                    .name()
                    .or(frame.func_name())
                    .map(|name| name.to_string()),
                file,
                line: symbol.line()?,
                column: symbol.column(),
            })
        })
    })
}

/// A call that failed: trap, out of fuel or memory, panic, or a host-side
/// problem. `what` names the export being called.
pub(crate) fn call_failed(
    name: &str,
    what: &str,
    error: &wasmtime::Error,
    state: &HostState,
) -> Diagnostic {
    let mut place = place(error);
    let inside = place
        .as_ref()
        .and_then(|p| p.function.as_deref())
        .map(|f| format!(" in {f}"))
        .unwrap_or_default();
    let trap = error.downcast_ref::<wasmtime::Trap>().copied();
    let diagnostic = if let Some(panic) = &state.panic {
        // The panic's own location is where the game code went wrong; the
        // trace would point into the standard library's panic machinery.
        if let Some(at) = &panic.place {
            place = Some(SourcePlace {
                function: None,
                file: at.file.clone(),
                line: at.line,
                column: at.column,
            });
        }
        Diagnostic::error(
            "script_panicked",
            format!("Game code panicked: {}", panic.message),
            "Fix the cause on the line named; the system is turned off until the code is rebuilt.",
        )
    } else if let Some(wanted) = state.memory_denied {
        Diagnostic::error(
            "script_out_of_memory",
            format!(
                "Game code{inside} asked for {} MiB of memory; the limit is {} MiB.",
                wanted / (1024 * 1024),
                state.memory_limit / (1024 * 1024)
            ),
            "Look for a collection that grows every tick, or a very large allocation. State belongs in components, which live outside the module.",
        )
    } else if trap == Some(wasmtime::Trap::OutOfFuel) {
        Diagnostic::error(
            "script_out_of_fuel",
            format!(
                "Game code{inside} ran out of fuel: it did too much work in one call, most likely a loop that never ends."
            ),
            "Check loop conditions. Work that takes many ticks should advance a little each tick, keeping its progress in a component.",
        )
    } else if trap == Some(wasmtime::Trap::StackOverflow) {
        Diagnostic::error(
            "script_stack_overflow",
            format!(
                "Game code{inside} overflowed its stack, most likely through recursion that never stops."
            ),
            "Check the recursion's base case, or turn the recursion into a loop.",
        )
    } else if let Some(trap) = trap {
        Diagnostic::error(
            "script_trap",
            format!("Game code{inside} stopped with a WebAssembly trap: {trap}."),
            "The location points to the failing line. Common causes: an index out of range, division by zero, an unwrap on None.",
        )
    } else {
        Diagnostic::error(
            "script_call_failed",
            format!("Calling '{what}' in '{name}' failed: {error:#}"),
            "Rebuild the game code with this engine's SDK; if it keeps failing, report the message.",
        )
    };
    match place {
        Some(at) => diagnostic.at_file(at.file).at_line(at.line, at.column),
        None => diagnostic.at_file(name),
    }
}
