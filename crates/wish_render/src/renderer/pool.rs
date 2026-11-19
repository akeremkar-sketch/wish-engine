//! GPU objects kept from one frame to the next: render targets, and the
//! buffers every frame fills again.
//!
//! A pass asks for what it needs by name:
//!
//! - a **texture** by name, size, format and usage ([`Pool::texture`]);
//! - a **buffer** by name, usage and contents ([`Pool::buffer`]): it is
//!   filled through the queue and replaced by a larger one when the contents
//!   do not fit;
//! - a **picture** (a colour target with the buffer it is read back through)
//!   by name, size and format ([`Pool::picture`]).
//!
//! The pool forgets what is not used, by three rules:
//!
//! - of one name at most [`SIZES_KEPT`] sizes stay (the least recently used
//!   goes first);
//! - anything not used for [`IDLE_CALLS`] calls is dropped;
//! - **when a call ends**, the least recently used objects are dropped until
//!   what is kept fits [`BUDGET_BYTES`] ([`Pool::end_call`]). What the call
//!   itself used always stays, however large: the next call is most likely
//!   the same picture again.
//!
//! A caller that draws now and then rather than every frame (a tool call)
//! can let go of more with [`Pool::trim`] (`Renderer::trim`).
//! After a failed frame the renderer empties the pool ([`Pool::clear`]), so
//! nothing half-used is used again.
//!
//! **Bind groups are not kept.** Each frame builds its own over the pooled
//! objects and drops them when it is done, for two reasons:
//!
//! - a bind group that outlives a frame can point at a view that a changed
//!   size has since replaced; one built in the frame cannot;
//! - on the software adapter of Windows (WARP) every live bind group of one
//!   layout that binds a uniform buffer reads the uniform buffer of the
//!   group *created last* (measured, M12 render foundation note §3.2; the
//!   probe is in `warp_probe.rs`). Two renderers on one device share their
//!   layouts, so a kept group of one would read the other's frame data.
//!
//! Like all GPU work, [`Pool::texture`] and [`Pool::buffer`] run inside
//! `Gpu::checked`. [`Pool::picture`] opens its own check and runs outside
//! one. The calls that only let go of objects ([`Pool::begin_call`],
//! [`Pool::end_call`], [`Pool::trim`], [`Pool::clear`]) run **outside**
//! `Gpu::checked` and take the GPU lock themselves while they drop.

use wish_core::Result;

use crate::capture::Target;
use crate::gpu::{Gpu, gpu_lock};

/// How many sizes of one named texture or picture are kept. The editor draws
/// a scene view, a game view and an identity buffer of different sizes in
/// turn; four leaves room for a capture on top.
const SIZES_KEPT: usize = 4;
/// An object not used for this many calls is dropped.
const IDLE_CALLS: u64 = 64;
/// The smallest pooled buffer, in bytes.
const SMALLEST_BUFFER: u64 = 64;
/// How many bytes of targets and buffers a renderer keeps from one call to
/// the next, on top of what the last call used. A 3840x2160 window's lit
/// picture (20 bytes a pixel: the picture, its readback buffer, the HDR
/// picture and the depth buffer), its identity picture and a 2048 shadow
/// map are 238 MiB. FXAA keeps the tone mapped picture too (4 bytes a
/// pixel, 32 MiB), and bloom the bloomed picture (8 bytes a pixel, 63 MiB)
/// and its nine levels (8 bytes a pixel of each, together a third of the
/// picture: 21 MiB). So such a view with every effect on is 354 MiB and is
/// drawn without making anything; the sizes a dragged window edge leaves
/// behind do not pile up beside it.
pub(super) const BUDGET_BYTES: u64 = 384 << 20;

/// How many GPU objects a renderer's pool has created since the renderer was
/// made. (What is created beside the pool, such as a posed mesh's vertex
/// buffer, is counted by `Gpu::created`, which counts everything. Bind groups
/// are not pooled and not counted: every frame builds its own.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoolCounts {
    /// Textures: render targets, and the texture of each picture.
    pub textures: u64,
    /// Buffers: uniform, storage and vertex data, and the readback buffer of
    /// each picture.
    pub buffers: u64,
}

/// What a pooled texture is: two requests with equal keys share one texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureKey {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
}

impl TextureKey {
    /// The bytes of GPU memory a texture of this key holds (one level, one
    /// sample; every pooled format has a fixed size per pixel).
    fn bytes(&self) -> u64 {
        let texel = u64::from(self.format.block_copy_size(None).unwrap_or(4));
        u64::from(self.width) * u64::from(self.height) * texel
    }
}

struct TextureEntry {
    key: TextureKey,
    view: wgpu::TextureView,
    used: u64,
}

struct PictureEntry {
    name: &'static str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    target: Target,
    used: u64,
}

struct BufferEntry {
    name: &'static str,
    usage: wgpu::BufferUsages,
    capacity: u64,
    buffer: wgpu::Buffer,
    used: u64,
}

/// The size a buffer is created with to hold `needed` bytes: the next power
/// of two, so that contents growing a little every frame do not make a new
/// buffer every frame.
fn capacity_for(needed: u64) -> u64 {
    let least = needed.max(SMALLEST_BUFFER);
    least.checked_next_power_of_two().unwrap_or(least)
}

/// Of the entries with these last-used times, the one to drop to make room
/// for one more when at most `kept` may stay: the least recently used, or
/// none while there is room.
fn to_evict(used: &[u64], kept: usize) -> Option<usize> {
    if used.len() < kept {
        return None;
    }
    used.iter()
        .enumerate()
        .min_by_key(|(_, used)| **used)
        .map(|(index, _)| index)
}

/// Which of the entries, given as (last used, bytes), to drop so that at
/// most `limit` bytes stay: the least recently used first, and of those used
/// at the same time the larger first (the earlier one if they are equal
/// too). Entries last used at `spared` or later are never dropped, even when
/// they alone are over the limit. The answer is in dropping order.
fn over_budget(entries: &[(u64, u64)], limit: u64, spared: Option<u64>) -> Vec<usize> {
    let mut total: u64 = entries.iter().map(|(_, bytes)| *bytes).sum();
    let mut candidates: Vec<usize> = (0..entries.len())
        .filter(|index| {
            entries
                .get(*index)
                .is_some_and(|(used, _)| spared.is_none_or(|spared| *used < spared))
        })
        .collect();
    candidates.sort_by_key(|index| {
        let (used, bytes) = entries.get(*index).copied().unwrap_or_default();
        (used, std::cmp::Reverse(bytes), *index)
    });
    let mut dropped = Vec::new();
    for index in candidates {
        if total <= limit {
            break;
        }
        total -= entries.get(index).map_or(0, |(_, bytes)| *bytes);
        dropped.push(index);
    }
    dropped
}

/// Drops GPU objects outside a frame, under the GPU lock like all GPU work.
/// Never call it inside `Gpu::checked` (the lock is not re-entrant); what a
/// request replaces while a frame is being built is dropped there, already
/// under the lock.
fn release<T>(objects: Vec<T>) {
    if objects.is_empty() {
        return;
    }
    let _only_one = gpu_lock();
    drop(objects);
}

/// Splits `entries` into the ones to keep and the ones `gone` names.
fn take_out<T>(entries: &mut Vec<T>, mut gone: impl FnMut(usize, &T) -> bool) -> Vec<T> {
    let mut kept = Vec::with_capacity(entries.len());
    let mut out = Vec::new();
    for (index, entry) in std::mem::take(entries).into_iter().enumerate() {
        if gone(index, &entry) {
            out.push(entry);
        } else {
            kept.push(entry);
        }
    }
    *entries = kept;
    out
}

/// The pool of one renderer.
pub(crate) struct Pool {
    /// Counts the calls of the renderer (one per picture asked for), for
    /// telling what was used lately.
    clock: u64,
    /// How many bytes may stay when a call ends ([`BUDGET_BYTES`]).
    budget: u64,
    textures: Vec<TextureEntry>,
    pictures: Vec<PictureEntry>,
    buffers: Vec<BufferEntry>,
    counts: PoolCounts,
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            clock: 0,
            budget: BUDGET_BYTES,
            textures: Vec::new(),
            pictures: Vec::new(),
            buffers: Vec::new(),
            counts: PoolCounts::default(),
        }
    }
}

impl Pool {
    /// What has been created so far.
    pub(super) fn counts(&self) -> PoolCounts {
        self.counts
    }

    /// How many bytes of GPU memory the pool holds now: its textures, its
    /// pictures with their readback buffers, and its buffers.
    pub(super) fn bytes(&self) -> u64 {
        self.sizes().iter().map(|(_, bytes)| *bytes).sum()
    }

    /// Every entry as (last used, bytes): the textures, then the pictures,
    /// then the buffers, each in the order of its list.
    fn sizes(&self) -> Vec<(u64, u64)> {
        let textures = self
            .textures
            .iter()
            .map(|entry| (entry.used, entry.key.bytes()));
        let pictures = self
            .pictures
            .iter()
            .map(|entry| (entry.used, entry.target.bytes()));
        let buffers = self
            .buffers
            .iter()
            .map(|entry| (entry.used, entry.capacity));
        textures.chain(pictures).chain(buffers).collect()
    }

    /// Drops everything. The counts go on: they say what was ever created.
    pub(super) fn clear(&mut self) {
        release(std::mem::take(&mut self.textures));
        release(std::mem::take(&mut self.pictures));
        release(std::mem::take(&mut self.buffers));
    }

    /// Starts one call of the renderer: what has not been used for
    /// [`IDLE_CALLS`] calls is dropped.
    pub(super) fn begin_call(&mut self) {
        self.clock += 1;
        let clock = self.clock;
        let idle = |used: u64| clock.saturating_sub(used) > IDLE_CALLS;
        release(take_out(&mut self.textures, |_, entry| idle(entry.used)));
        release(take_out(&mut self.pictures, |_, entry| idle(entry.used)));
        release(take_out(&mut self.buffers, |_, entry| idle(entry.used)));
    }

    /// Ends one call of the renderer that went well: the least recently used
    /// objects are dropped until the pool is inside its budget. What this
    /// call used stays.
    pub(super) fn end_call(&mut self) {
        self.shrink(self.budget, Some(self.clock));
    }

    /// Drops the least recently used objects until at most `keep` bytes
    /// stay, what the last call used included.
    pub(super) fn trim(&mut self, keep: u64) {
        self.shrink(keep, None);
    }

    fn shrink(&mut self, limit: u64, spared: Option<u64>) {
        let gone = over_budget(&self.sizes(), limit, spared);
        if gone.is_empty() {
            return;
        }
        // The positions of `sizes`: textures, then pictures, then buffers.
        let pictures_from = self.textures.len();
        let buffers_from = pictures_from + self.pictures.len();
        release(take_out(&mut self.textures, |index, _| {
            gone.contains(&index)
        }));
        release(take_out(&mut self.pictures, |index, _| {
            gone.contains(&(pictures_from + index))
        }));
        release(take_out(&mut self.buffers, |index, _| {
            gone.contains(&(buffers_from + index))
        }));
    }

    /// The texture of this key, made if the pool has none. Its contents are
    /// whatever the last frame left: clear or overwrite it before reading.
    pub(crate) fn texture(&mut self, gpu: &Gpu, key: TextureKey) -> wgpu::TextureView {
        let clock = self.clock;
        if let Some(entry) = self.textures.iter_mut().find(|entry| entry.key == key) {
            entry.used = clock;
            return entry.view.clone();
        }
        // Make room among the textures of this name.
        let same: Vec<(usize, u64)> = self
            .textures
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.key.name == key.name)
            .map(|(index, entry)| (index, entry.used))
            .collect();
        let used: Vec<u64> = same.iter().map(|(_, used)| *used).collect();
        if let Some((index, _)) = to_evict(&used, SIZES_KEPT).and_then(|at| same.get(at)) {
            self.textures.remove(*index);
        }
        let view = gpu
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(key.name),
                size: wgpu::Extent3d {
                    width: key.width,
                    height: key.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: key.format,
                usage: key.usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.counts.textures += 1;
        self.textures.push(TextureEntry {
            key,
            view: view.clone(),
            used: clock,
        });
        view
    }

    /// The buffer of this name holding `contents` (a multiple of 4 bytes, as
    /// all engine data is). A buffer that is too small, or was made for
    /// another use, is replaced by a larger one; a buffer is never made
    /// smaller. It may be larger than its contents: bind or draw only the
    /// part this frame filled, or read only what this frame's counts name.
    pub(crate) fn buffer(
        &mut self,
        gpu: &Gpu,
        name: &'static str,
        usage: wgpu::BufferUsages,
        contents: &[u8],
    ) -> wgpu::Buffer {
        let clock = self.clock;
        let needed = contents.len() as u64;
        let existing = self
            .buffers
            .iter_mut()
            .find(|entry| entry.name == name && entry.usage == usage && entry.capacity >= needed)
            .map(|entry| {
                entry.used = clock;
                entry.buffer.clone()
            });
        let buffer = match existing {
            Some(buffer) => buffer,
            None => {
                // Too small, or made for another use: a new one takes its
                // place.
                self.buffers.retain(|entry| entry.name != name);
                let capacity = capacity_for(needed);
                let buffer = gpu.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(name),
                    size: capacity,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.counts.buffers += 1;
                self.buffers.push(BufferEntry {
                    name,
                    usage,
                    capacity,
                    buffer: buffer.clone(),
                    used: clock,
                });
                buffer
            }
        };
        if !contents.is_empty() {
            gpu.queue.write_buffer(&buffer, 0, contents);
        }
        buffer
    }

    /// The picture of this name, size and format: a colour target and the
    /// buffer it is read back through. The size must have passed
    /// `capture::check_size`. Unlike the other requests this one runs
    /// **outside** `Gpu::checked`: it opens its own check when it has to make
    /// the picture, so that a picture too large for the GPU is reported as
    /// such.
    pub(super) fn picture(
        &mut self,
        gpu: &Gpu,
        name: &'static str,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Result<Target> {
        let clock = self.clock;
        if let Some(entry) = self
            .pictures
            .iter_mut()
            .find(|entry| entry.name == name && entry.size == size && entry.format == format)
        {
            entry.used = clock;
            return Ok(entry.target.clone());
        }
        let same: Vec<(usize, u64)> = self
            .pictures
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.name == name)
            .map(|(index, entry)| (index, entry.used))
            .collect();
        let used: Vec<u64> = same.iter().map(|(_, used)| *used).collect();
        if let Some((index, _)) = to_evict(&used, SIZES_KEPT).and_then(|at| same.get(at)) {
            let replaced = self.pictures.remove(*index);
            release(vec![replaced]);
        }
        let target = gpu.checked("create the capture target", |gpu| {
            Target::create(gpu, size.0, size.1, format)
        })?;
        self.counts.textures += 1;
        self.counts.buffers += 1;
        self.pictures.push(PictureEntry {
            name,
            size,
            format,
            target: target.clone(),
            used: clock,
        });
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::GpuOptions;

    #[test]
    fn buffers_are_made_a_power_of_two_large_and_never_tiny() {
        assert_eq!(capacity_for(0), 64);
        assert_eq!(capacity_for(64), 64);
        assert_eq!(capacity_for(65), 128);
        assert_eq!(capacity_for(432), 512);
        assert_eq!(capacity_for(2560), 4096);
        // Nothing to round up to: as asked.
        assert_eq!(capacity_for(u64::MAX), u64::MAX);
    }

    #[test]
    fn the_least_recently_used_makes_room_only_when_the_pool_is_full() {
        assert_eq!(to_evict(&[], 4), None);
        assert_eq!(to_evict(&[7, 3, 9], 4), None);
        assert_eq!(to_evict(&[7, 3, 9, 5], 4), Some(1));
        // Equal times: the first one, a fixed choice.
        assert_eq!(to_evict(&[2, 2, 2, 2], 4), Some(0));
    }

    #[test]
    fn over_the_budget_the_least_recently_used_go_first_and_the_last_call_stays() {
        // (last used, bytes)
        let entries = [(5, 100), (2, 40), (9, 300), (2, 70), (7, 10)];
        // Inside the limit: nothing goes.
        assert_eq!(over_budget(&entries, 520, None), Vec::<usize>::new());
        assert_eq!(over_budget(&[], 0, None), Vec::<usize>::new());
        // Over it: the oldest first, the larger of two equally old first,
        // and no more than needed.
        assert_eq!(over_budget(&entries, 519, None), [3]);
        assert_eq!(over_budget(&entries, 449, None), [3, 1]);
        assert_eq!(over_budget(&entries, 400, None), [3, 1, 0]);
        assert_eq!(over_budget(&entries, 0, None), [3, 1, 0, 4, 2]);
        // What the call that is ending used (time 9) is never dropped, even
        // when it alone is over the limit; everything older may go.
        assert_eq!(over_budget(&entries, 0, Some(9)), [3, 1, 0, 4]);
        assert_eq!(over_budget(&entries, 310, Some(9)), [3, 1, 0]);
        assert_eq!(
            over_budget(&[(4, 900)], 100, Some(4)),
            Vec::<usize>::new(),
            "a picture larger than the budget is still drawn and kept for the next call"
        );
        // Equal in age and size: the earlier one, a fixed choice.
        assert_eq!(over_budget(&[(1, 8), (1, 8)], 8, None), [0]);
    }

    fn key(name: &'static str, side: u32) -> TextureKey {
        TextureKey {
            name,
            width: side,
            height: side,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        }
    }

    #[test]
    fn a_pool_over_its_budget_lets_go_when_a_call_ends() {
        let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
        // A 64x64 half-float picture is 32 KiB; the pool may keep 80 KiB
        // between calls.
        let mut pool = Pool {
            budget: 80 << 10,
            ..Pool::default()
        };
        let call = |pool: &mut Pool, names: &[&'static str]| {
            pool.begin_call();
            gpu.checked("make test targets", |gpu| {
                for name in names {
                    pool.texture(gpu, key(name, 64));
                }
            })
            .unwrap();
            pool.end_call();
        };
        call(&mut pool, &["a"]);
        call(&mut pool, &["b"]);
        assert_eq!(pool.bytes(), 64 << 10, "two pictures fit");
        assert_eq!(pool.counts().textures, 2);
        // A third does not: the least recently used one goes, when the call
        // ends and not at the start of some later call.
        call(&mut pool, &["c"]);
        assert_eq!(pool.bytes(), 64 << 10);
        let names: Vec<&str> = pool.textures.iter().map(|entry| entry.key.name).collect();
        assert_eq!(names, ["b", "c"]);
        // Using one keeps it: now "b" is the older of the two.
        call(&mut pool, &["b"]);
        call(&mut pool, &["d"]);
        let names: Vec<&str> = pool.textures.iter().map(|entry| entry.key.name).collect();
        assert_eq!(names, ["b", "d"]);
        // A call that needs more than the budget keeps all it used (it is
        // most likely drawn again), and nothing else.
        call(&mut pool, &["x", "y", "z"]);
        let names: Vec<&str> = pool.textures.iter().map(|entry| entry.key.name).collect();
        assert_eq!(names, ["x", "y", "z"]);
        assert_eq!(pool.bytes(), 96 << 10);
        // Asked to, the pool lets go of that too: the larger first among
        // what was used at the same time, then in order.
        pool.begin_call();
        gpu.checked("make test targets", |gpu| {
            pool.texture(gpu, key("x", 64));
            pool.texture(gpu, key("large", 128));
            pool.buffer(gpu, "data", wgpu::BufferUsages::UNIFORM, &[0; 100]);
        })
        .unwrap();
        pool.end_call();
        assert_eq!(pool.bytes(), (128 << 10) + (32 << 10) + 128);
        pool.trim(40 << 10);
        let names: Vec<&str> = pool.textures.iter().map(|entry| entry.key.name).collect();
        assert_eq!(names, ["x"], "the 128 KiB picture went first");
        assert_eq!(pool.buffers.len(), 1, "the small buffer fits");
        pool.trim(0);
        assert_eq!(pool.bytes(), 0);
        assert!(pool.textures.is_empty() && pool.buffers.is_empty());
        // The counts say what was ever made.
        assert_eq!(pool.counts().textures, 8);
        assert_eq!(pool.counts().buffers, 1);
    }

    #[test]
    fn the_bytes_of_a_pooled_object_are_its_size() {
        let gpu = Gpu::new(GpuOptions { software: true }).expect("a software adapter");
        let mut pool = Pool::default();
        pool.begin_call();
        gpu.checked("make test targets", |gpu| {
            pool.texture(gpu, key("half float", 10));
            pool.texture(
                gpu,
                TextureKey {
                    format: wgpu::TextureFormat::Depth32Float,
                    ..key("depth", 10)
                },
            );
            pool.buffer(gpu, "data", wgpu::BufferUsages::UNIFORM, &[0; 432]);
        })
        .unwrap();
        // 8 bytes a pixel, 4 bytes a pixel, and a buffer of 512.
        assert_eq!(pool.bytes(), 800 + 400 + 512);
        // A picture is its texture and its readback buffer, whose rows are
        // padded to 256 bytes.
        pool.picture(&gpu, "picture", (10, 3), wgpu::TextureFormat::Rgba8Unorm)
            .unwrap();
        assert_eq!(pool.bytes(), 800 + 400 + 512 + (10 * 3 * 4) + (256 * 3));
        pool.clear();
        assert_eq!(pool.bytes(), 0);
    }
}
