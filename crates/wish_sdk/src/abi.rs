//! The module's side of the ABI: the `wish` imports, the exports'
//! bodies (called from `wish_sdk::export!`), and the per-call command buffer.

#![allow(unsafe_code)]

use std::cell::RefCell;

use crate::command::Command;
use crate::frame::FrameReader;

#[cfg(target_arch = "wasm32")]
mod imports {
    #[link(wasm_import_module = "wish")]
    unsafe extern "C" {
        pub fn log(severity: i32, ptr: *const u8, len: i32);
        pub fn panic(
            ptr: *const u8,
            len: i32,
            file: *const u8,
            file_len: i32,
            line: i32,
            column: i32,
        );
        pub fn rng_next() -> i64;
        pub fn physics_query(request: *const u8, len: i32, out: *mut u8, capacity: i32) -> i32;
        pub fn text_intern(ptr: *const u8, len: i32) -> i64;
        pub fn text_read(handle: i32, out: *mut u8, capacity: i32) -> i32;
    }
}

thread_local! {
    /// The frame memory, reused by every call of the tick: all of a tick's
    /// systems share one instance, so frames that were never freed would
    /// add up to the module's memory limit (M5 closing review). The engine
    /// reads a frame back before it asks for the next one. Words, for the
    /// frame's 8-byte alignment.
    static FRAME: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// The command bytes handed to the engine by the last call, reused the
    /// same way.
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Commands of the call in progress.
    static COMMANDS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// How many entities the call has spawned so far.
    static SPAWNED: RefCell<u32> = const { RefCell::new(0) };
    /// Off the engine (the SDK's own tests): a stand-in random stream.
    #[cfg(not(target_arch = "wasm32"))]
    static RNG: RefCell<wish_core::Rng> = RefCell::new(wish_core::Rng::from_seed(0));
    /// Off the engine: a stand-in text table (the empty text first).
    #[cfg(not(target_arch = "wasm32"))]
    static TEXTS: RefCell<Vec<String>> = RefCell::new(vec![String::new()]);
}

/// The handle of `text` in the world's text table (`wish.text_intern`),
/// added if new; `None` if the engine refused it (longer than 4096 bytes, or
/// more than 1024 new texts in one call).
pub fn text_intern(text: &str) -> Option<u32> {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: the engine reads `len` bytes at `ptr` from this module's
        // memory during the call.
        let handle = unsafe { imports::text_intern(text.as_ptr(), text.len() as i32) };
        u32::try_from(handle).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        TEXTS.with(|texts| {
            let mut texts = texts.borrow_mut();
            let index = match texts.iter().position(|known| known == text) {
                Some(index) => index,
                None => {
                    texts.push(text.to_owned());
                    texts.len() - 1
                }
            };
            u32::try_from(index).ok()
        })
    }
}

/// The text behind handle `handle` (`wish.text_read`), or `None` for a
/// handle the world does not have.
pub fn text_read(handle: u32) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let handle = i32::try_from(handle).ok()?;
        let mut out = vec![0u8; 64];
        loop {
            // SAFETY: the engine writes at most `capacity` bytes at `out`,
            // inside this module's memory and alive for the call.
            let length = unsafe { imports::text_read(handle, out.as_mut_ptr(), out.len() as i32) };
            let length = usize::try_from(length).ok()?;
            if length <= out.len() {
                out.truncate(length);
                return String::from_utf8(out).ok();
            }
            out = vec![0u8; length];
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        TEXTS.with(|texts| texts.borrow().get(handle as usize).cloned())
    }
}

/// Adds a command to the call's buffer.
pub(crate) fn push_command(command: &Command) {
    COMMANDS.with(|buffer| command.encode(&mut buffer.borrow_mut()));
}

/// The index the next spawn of this call gets.
pub(crate) fn next_spawn() -> u32 {
    SPAWNED.with(|count| {
        let mut count = count.borrow_mut();
        let index = *count;
        *count += 1;
        index
    })
}

/// The commands given so far in this call (tests).
pub fn take_commands() -> Vec<u8> {
    SPAWNED.with(|count| *count.borrow_mut() = 0);
    COMMANDS.with(|buffer| std::mem::take(&mut *buffer.borrow_mut()))
}

/// Stops the call with a message. In a game module a panic is how a
/// failure reaches the engine: the panic hook sends the message and the
/// line, the module traps, and the engine turns that into a diagnostic and
/// turns the system off (the world keeps running).
#[allow(clippy::panic)]
#[track_caller]
pub fn fail(message: &str) -> ! {
    panic!("{message}")
}

/// Writes a message to the engine's log (severity 0 info, 1 warning, 2 error).
pub fn log(severity: i32, message: &str) {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the engine reads `len` bytes at `ptr` from this module's memory.
    unsafe {
        imports::log(severity, message.as_ptr(), message.len() as i32)
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (severity, message);
    }
}

/// Asks the engine a physics question (`wish.physics_query`); the answer's
/// bytes, or `None` off the engine or if the engine could not answer.
pub fn physics_query(request: &[u8]) -> Option<Vec<u8>> {
    physics_query_into(request, crate::physics::ANSWER_LEN)
}

/// Asks the engine a question through `wish.physics_query` (a ray, or a
/// line-of-sight question) whose answer is at most `capacity` bytes; the
/// answer's bytes, or `None` off the engine or if the engine could not
/// answer.
pub fn physics_query_into(request: &[u8], capacity: usize) -> Option<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut out = vec![0u8; capacity];
        // SAFETY: the engine reads `len` bytes at `request` and writes at
        // most `capacity` bytes at `out`, both inside this module's memory
        // and alive for the call.
        let written = unsafe {
            imports::physics_query(
                request.as_ptr(),
                request.len() as i32,
                out.as_mut_ptr(),
                i32::try_from(out.len()).unwrap_or(i32::MAX),
            )
        };
        let written = usize::try_from(written).ok()?;
        out.truncate(written);
        Some(out)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (request, capacity);
        None
    }
}

/// The next number from this system's own random stream (seeded by the
/// engine from the app's seed and the system's name: deterministic).
pub fn random_u64() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: an import without arguments.
        let value = unsafe { imports::rng_next() };
        u64::from_ne_bytes(value.to_ne_bytes())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        RNG.with(|rng| rng.borrow_mut().next_u64())
    }
}

/// Sends panics to the engine with their location, once per instance.
fn report_panics() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let message = info
                .payload_as_str()
                .map_or_else(|| "a panic without a message".to_owned(), str::to_owned);
            let (file, line, column) = info
                .location()
                .map_or(("", 0, 0), |l| (l.file(), l.line(), l.column()));
            #[cfg(target_arch = "wasm32")]
            // SAFETY: the engine reads the given byte ranges from this
            // module's memory.
            unsafe {
                imports::panic(
                    message.as_ptr(),
                    message.len() as i32,
                    file.as_ptr(),
                    file.len() as i32,
                    line as i32,
                    column as i32,
                )
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = (message, file, line, column);
            }
        }));
    });
}

/// Memory for a frame, zeroed: the same buffer for every call of the tick
/// (grown when a frame is larger). 0 if the memory could not be had.
pub fn alloc(len: i32) -> i32 {
    let words = (len.max(1) as usize).div_ceil(8);
    FRAME.with(|frame| {
        let mut frame = frame.borrow_mut();
        frame.clear();
        if frame.try_reserve_exact(words).is_err() {
            return 0;
        }
        frame.resize(words, 0);
        frame.as_mut_ptr() as usize as i32
    })
}

/// Packs a pointer and a length into the `i64` `wish_manifest` returns.
pub fn manifest(text: String) -> i64 {
    let bytes = text.into_bytes().leak();
    ((bytes.as_ptr() as usize as i64) << 32) | (bytes.len() as i64 & 0xffff_ffff)
}

/// The body of `wish_run`: runs system `system` on the frame at `frame` and
/// returns where its commands are (0 for none, -1 for an unknown system or
/// frame).
pub fn run(systems: &[fn(&mut FrameReader)], system: i32, frame: i32) -> i32 {
    report_panics();
    let Some(run) = usize::try_from(system).ok().and_then(|i| systems.get(i)) else {
        return -1;
    };
    // SAFETY: `frame` is the pointer the engine passed to `wish_run`.
    let Some(mut reader) = (unsafe { FrameReader::from_ptr(frame as usize as *mut u8) }) else {
        return -1;
    };
    take_commands();
    run(&mut reader);
    let commands = take_commands();
    if commands.is_empty() {
        return 0;
    }
    OUT.with(|out| {
        let mut out = out.borrow_mut();
        out.clear();
        out.extend((commands.len() as u32).to_le_bytes());
        out.extend(commands);
        out.as_ptr() as usize as i32
    })
}
