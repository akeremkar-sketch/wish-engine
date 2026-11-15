//! A stand-in for `wish_sdk`: a vector type, a frame reader and the macro
//! that writes the ABI exports. Enough code that compiling against it is
//! representative.

use std::alloc::{Layout, alloc};

/// A 3D vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

/// The frame of one call: column slices the host wrote.
pub struct Frame {
    base: *mut u8,
}

impl Frame {
    /// # Safety
    /// `base` is the frame pointer the host passed to `wish_run`.
    pub unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }
    pub fn count(&self) -> usize {
        // SAFETY: the frame starts with the count.
        unsafe { *(self.base as *const u32) as usize }
    }
    pub fn dt(&self) -> f32 {
        // SAFETY: the count is followed by dt.
        unsafe { *(self.base.add(4) as *const f32) }
    }
    pub fn column_mut<T>(&mut self, index: usize) -> &mut [T] {
        let count = self.count();
        let size = std::mem::size_of::<T>();
        // SAFETY: column `index` follows the header, each `count * size` bytes.
        unsafe {
            std::slice::from_raw_parts_mut(self.base.add(8 + index * count * size) as *mut T, count)
        }
    }
}

#[doc(hidden)]
pub fn alloc_frame(len: i32) -> *mut u8 {
    let Ok(layout) = Layout::from_size_align(len.max(1) as usize, 8) else {
        return std::ptr::null_mut();
    };
    // SAFETY: non-zero size.
    unsafe { alloc(layout) }
}

/// Writes the ABI exports for the listed systems, in order.
#[macro_export]
macro_rules! export {
    ($($system:ident),* $(,)?) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_abi_version() -> i32 { 1 }
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_alloc(len: i32) -> *mut u8 { $crate::alloc_frame(len) }
        #[unsafe(no_mangle)]
        pub extern "C" fn wish_run(system: i32, frame: *mut u8) -> i32 {
            // SAFETY: the host passes the frame it wrote.
            let mut frame = unsafe { $crate::Frame::new(frame) };
            let systems: &[fn(&mut $crate::Frame)] = &[$($system),*];
            match systems.get(system as usize) {
                Some(run) => { run(&mut frame); 0 }
                None => -1,
            }
        }
    };
}
