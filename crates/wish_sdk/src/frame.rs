//! The call frame: what the engine writes into the module's memory before
//! calling a system, and how the module reads and writes it.
//!
//! Layout (ABI v1, little-endian; design note M5 §3). Every section starts
//! at a multiple of 8 bytes from the frame's start.
//!
//! ```text
//! frame:   0 u32 magic "WSHF"  4 u32 version  8 u64 tick
//!         16 u32 section count 20 u32 0       24 sections × (u32 offset, u32 len)
//! query:   0 u32 count  4 u32 columns  8 count × u64 entity bits,
//!          then each column (8-aligned): count × the component's size
//! res:     0 the resource's bytes
//! events:  0 u32 count  4 u32 0  8 count × the event's size
//! commands, event writers: empty
//! ```
//!
//! Frames are never freed while an instance lives (the engine makes a fresh
//! instance every tick), which is what makes holding a [`Section`] past the
//! call harmless.

#![allow(unsafe_code)]

/// "WSHF" read as a little-endian u32.
pub const MAGIC: u32 = 0x4648_5357;
/// The frame layout version.
pub const VERSION: u32 = 1;
/// The size of the frame header before the section table.
pub const HEADER: usize = 24;

/// Rounds up to a multiple of 8.
pub const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// One section of a frame, by address. Valid for the life of the instance.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    ptr: *mut u8,
    len: usize,
}

impl Section {
    /// An empty section.
    pub const EMPTY: Section = Section {
        ptr: core::ptr::null_mut(),
        len: 0,
    };

    /// The section's bytes.
    pub fn bytes(&self) -> &[u8] {
        if self.ptr.is_null() || self.len == 0 {
            return &[];
        }
        // SAFETY: `ptr..ptr+len` lies inside a frame the engine wrote into
        // memory from `wish_alloc`, which is never freed while the instance
        // lives; sections of one frame do not overlap.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// The section's bytes, to write back into.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        if self.ptr.is_null() || self.len == 0 {
            return &mut [];
        }
        // SAFETY: as in `bytes`; each section belongs to exactly one system
        // parameter, which is the only writer.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

/// Reads a frame's sections in order, one per system parameter.
#[derive(Debug)]
pub struct FrameReader {
    tick: u64,
    sections: Vec<Section>,
    next: usize,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl FrameReader {
    /// Reads the header and section table of the frame at `ptr`.
    ///
    /// # Safety
    /// `ptr` is a frame the engine wrote (the `frame` argument of `wish_run`).
    pub unsafe fn from_ptr(ptr: *mut u8) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        // SAFETY: the engine wrote at least the header and the table.
        let header = unsafe { core::slice::from_raw_parts(ptr, HEADER) };
        if u32_at(header, 0)? != MAGIC || u32_at(header, 4)? != VERSION {
            return None;
        }
        let tick = u64_at(header, 8)?;
        let count = u32_at(header, 16)? as usize;
        // SAFETY: the table follows the header.
        let table = unsafe { core::slice::from_raw_parts(ptr.add(HEADER), count * 8) };
        let mut sections = Vec::with_capacity(count);
        for index in 0..count {
            let offset = u32_at(table, index * 8)? as usize;
            let len = u32_at(table, index * 8 + 4)? as usize;
            sections.push(Section {
                // SAFETY: the engine places sections inside the frame.
                ptr: unsafe { ptr.add(offset) },
                len,
            });
        }
        Some(Self {
            tick,
            sections,
            next: 0,
        })
    }

    /// A reader over a frame held in a byte buffer (tests, and the host's own
    /// checks). The buffer must stay alive and unmoved while sections from it
    /// are used.
    pub fn from_buffer(buffer: &mut [u8]) -> Option<Self> {
        if buffer.len() < HEADER {
            return None;
        }
        let count = u32_at(buffer, 16)? as usize;
        let end = HEADER.checked_add(count.checked_mul(8)?)?;
        for index in 0..count {
            let offset = u32_at(buffer, HEADER + index * 8)? as usize;
            let len = u32_at(buffer, HEADER + index * 8 + 4)? as usize;
            if offset.checked_add(len)? > buffer.len() || (len > 0 && offset < end) {
                return None;
            }
        }
        // SAFETY: the header and every section were just checked to lie
        // inside `buffer`.
        unsafe { Self::from_ptr(buffer.as_mut_ptr()) }
    }

    /// The tick being simulated.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The next parameter's section (empty once they run out: a module built
    /// for different parameters fails its own checks instead of reading
    /// outside the frame).
    pub fn next_section(&mut self) -> Section {
        let section = self
            .sections
            .get(self.next)
            .copied()
            .unwrap_or(Section::EMPTY);
        self.next += 1;
        section
    }
}

/// Builds a frame (the engine's side of the layout; also used by the SDK's
/// own tests).
#[derive(Debug, Default)]
pub struct FrameWriter {
    sections: Vec<Vec<u8>>,
}

impl FrameWriter {
    /// An empty frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the next section.
    pub fn section(&mut self, bytes: Vec<u8>) {
        self.sections.push(bytes);
    }

    /// The frame's bytes, and where each section starts.
    pub fn finish(self, tick: u64) -> (Vec<u8>, Vec<(usize, usize)>) {
        let mut offset = align8(HEADER + self.sections.len() * 8);
        let mut places = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            places.push((offset, section.len()));
            offset = align8(offset + section.len());
        }
        let mut out = vec![0u8; offset];
        out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        out[4..8].copy_from_slice(&VERSION.to_le_bytes());
        out[8..16].copy_from_slice(&tick.to_le_bytes());
        out[16..20].copy_from_slice(&(self.sections.len() as u32).to_le_bytes());
        for (index, ((at, len), bytes)) in places.iter().zip(&self.sections).enumerate() {
            let entry = HEADER + index * 8;
            out[entry..entry + 4].copy_from_slice(&(*at as u32).to_le_bytes());
            out[entry + 4..entry + 8].copy_from_slice(&(*len as u32).to_le_bytes());
            out[*at..*at + len].copy_from_slice(bytes);
        }
        (out, places)
    }
}

/// Where a query section's parts are: the entity list, then each column.
pub fn query_layout(count: usize, sizes: &[usize]) -> (usize, Vec<usize>) {
    let mut offset = align8(8 + count * 8);
    let mut columns = Vec::with_capacity(sizes.len());
    for size in sizes {
        columns.push(offset);
        offset = align8(offset + count * size);
    }
    (offset, columns)
}

/// The header of a query or events section: the item count.
pub fn section_count(bytes: &[u8]) -> usize {
    u32_at(bytes, 0).unwrap_or(0) as usize
}

/// The entity bits of a query section.
pub fn query_entities(bytes: &[u8]) -> Vec<u64> {
    let count = section_count(bytes);
    (0..count)
        .map_while(|index| u64_at(bytes, 8 + index * 8))
        .collect()
}
