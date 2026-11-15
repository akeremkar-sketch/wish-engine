//! The game ABI v1 written by hand (no SDK): what `wish_sdk` will generate.
//!
//! Frame for system 0 ("mover"), a struct-of-arrays slice like the ECS
//! columns: `count: u32`, `dt: f32`, then `count` positions `[f32; 3]`, then
//! `count` velocities `[f32; 3]`. Positions are written back.

use std::alloc::{Layout, alloc};

#[link(wasm_import_module = "wish")]
unsafe extern "C" {
    #[link_name = "panic"]
    fn wish_panic(ptr: *const u8, len: i32, file: *const u8, file_len: i32, line: i32, column: i32);
}

#[unsafe(no_mangle)]
pub extern "C" fn wish_abi_version() -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn wish_alloc(len: i32) -> *mut u8 {
    let Ok(layout) = Layout::from_size_align(len.max(1) as usize, 8) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the layout has a non-zero size.
    unsafe { alloc(layout) }
}

fn report_panics() {
    std::panic::set_hook(Box::new(|info| {
        let message = match info.payload_as_str() {
            Some(text) => text.to_owned(),
            None => "Box<dyn Any>".to_owned(),
        };
        let (file, line, column) = info
            .location()
            .map_or(("", 0, 0), |l| (l.file(), l.line(), l.column()));
        // SAFETY: the host reads the given byte ranges from this module's memory.
        unsafe {
            wish_panic(
                message.as_ptr(),
                message.len() as i32,
                file.as_ptr(),
                file.len() as i32,
                line as i32,
                column as i32,
            )
        };
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn wish_run(system: i32, frame: *mut u8) -> i32 {
    report_panics();
    match system {
        0 => mover(frame),
        1 => index_out_of_range(),
        2 => unwrap_none(),
        3 => trap_here(),
        _ => -1,
    }
}

fn mover(frame: *mut u8) -> i32 {
    // SAFETY: the host wrote a frame of this layout at `frame`, 8-aligned.
    unsafe {
        let count = *(frame as *const u32) as usize;
        let dt = *(frame.add(4) as *const f32);
        let positions = std::slice::from_raw_parts_mut(frame.add(8) as *mut [f32; 3], count);
        let velocities =
            std::slice::from_raw_parts(frame.add(8 + count * 12) as *const [f32; 3], count);
        for (p, v) in positions.iter_mut().zip(velocities) {
            p[0] += v[0] * dt;
            p[1] += v[1] * dt;
            p[2] += v[2] * dt;
        }
        count as i32
    }
}

#[inline(never)]
fn index_out_of_range() -> i32 {
    let values: Vec<i32> = Vec::new();
    values[std::hint::black_box(3)]
}

#[inline(never)]
fn unwrap_none() -> i32 {
    let nothing: Option<i32> = std::hint::black_box(None);
    nothing.unwrap()
}

#[inline(never)]
fn trap_here() -> i32 {
    core::arch::wasm32::unreachable()
}
