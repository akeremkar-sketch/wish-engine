//! The replication messages, as pure encode and decode functions (design
//! note §4). Decoding never panics and never allocates beyond the limits:
//! every count is checked against what the remaining bytes can hold.

use crate::layout::{Layout, LeafKind};
use crate::value::{LeafData, Value};

/// Message kinds. Kinds 0 to [`kind::LAST`] belong to replication; any other
/// first byte is left to the caller (the lobby and targeted messages of
/// later packages).
pub mod kind {
    /// Host → client: the player id, the tick and the replicated types.
    pub const WELCOME: u8 = 1;
    /// Host → client: a snapshot or a delta of what the player may see.
    pub const STATE: u8 = 2;
    /// Client → host: input frames and the acknowledgement.
    pub const INPUT: u8 = 16;
    /// The last kind that belongs to replication.
    pub const LAST: u8 = 63;
}

/// The version of the replication messages (2: the state header carries
/// flags, prediction package).
pub const VERSION: u8 = 2;

/// State header flag: nothing the player may see was left out by the
/// budget, so every entity the message does not carry is unchanged since
/// the state the client acknowledged.
pub const FLAG_COMPLETE: u8 = 1;

/// The largest message a client may send, in bytes.
pub const MAX_CLIENT_MESSAGE: usize = 2048;

/// Input frames one message carries at most.
pub const MAX_FRAMES: usize = 8;

/// Actions one frame carries at most (`wish_types::MAX_ACTIONS`).
pub const MAX_FRAME_ACTIONS: usize = wish_types::MAX_ACTIONS;

/// Replicated types a welcome names at most.
pub const MAX_TYPES: usize = 256;

/// The longest type name, in bytes.
pub const MAX_TYPE_NAME: usize = 128;

/// The longest string a text leaf carries, in bytes.
pub const MAX_TEXT: usize = 64 * 1024;

/// Bytes of a schema fingerprint.
pub const FINGERPRINT_LEN: usize = 16;

/// Whether a message belongs to replication (by its first byte).
pub fn is_replication(data: &[u8]) -> bool {
    data.first().is_some_and(|k| *k <= kind::LAST)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// Appends `value` as an unsigned LEB128 varint.
pub fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The length of `value` as a varint.
pub fn varint_len(value: u64) -> usize {
    let bits = 64 - value.leading_zeros() as usize;
    bits.div_ceil(7).max(1)
}

/// Reads bytes with bounds checks; every method returns `None` past the end
/// or on a malformed value.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    /// Whether every byte was read.
    pub fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    /// The next `len` bytes.
    pub fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let out = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(out)
    }

    /// One byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// A little-endian `f32`.
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// An unsigned LEB128 varint of at most ten bytes.
    pub fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in 0..10u32 {
            let byte = self.u8()?;
            let part = u64::from(byte & 0x7F);
            if shift == 9 && part > 1 {
                return None;
            }
            value |= part << (7 * shift);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// A varint that must fit in `u32`.
    pub fn varint_u32(&mut self) -> Option<u32> {
        u32::try_from(self.varint()?).ok()
    }

    /// A count that must be at most `max` and at most the bytes left divided
    /// by `min_each` (so a hostile count never makes a large allocation).
    pub fn count(&mut self, max: usize, min_each: usize) -> Option<usize> {
        let n = usize::try_from(self.varint()?).ok()?;
        (n <= max && n.saturating_mul(min_each.max(1)) <= self.remaining()).then_some(n)
    }
}

/// `value + 1`, or 0 for none.
fn put_optional(out: &mut Vec<u8>, value: Option<u64>) {
    put_varint(out, value.map_or(0, |v| v.saturating_add(1)));
}

fn read_optional(reader: &mut Reader<'_>) -> Option<Option<u64>> {
    let raw = reader.varint()?;
    Some(raw.checked_sub(1))
}

// ---------------------------------------------------------------------------
// Welcome
// ---------------------------------------------------------------------------

/// One replicated type, as the welcome names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeEntry {
    /// The component's registered name.
    pub name: String,
    /// Its schema fingerprint: both sides must have the same layout.
    pub fingerprint: [u8; FINGERPRINT_LEN],
}

/// Host → client, once, before any state: who the client is and what types
/// the host replicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    /// The client's player id.
    pub player: u32,
    /// The host's tick when the client joined.
    pub tick: u64,
    /// The replicated types, in the order state messages number them.
    pub types: Vec<TypeEntry>,
}

/// Encodes a welcome.
pub fn encode_welcome(welcome: &Welcome) -> Vec<u8> {
    let mut out = vec![kind::WELCOME, VERSION];
    put_varint(&mut out, u64::from(welcome.player));
    put_varint(&mut out, welcome.tick);
    put_varint(&mut out, welcome.types.len() as u64);
    for entry in &welcome.types {
        let name = entry.name.as_bytes();
        out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(name.get(..usize::from(u8::MAX)).unwrap_or(name));
        out.extend_from_slice(&entry.fingerprint);
    }
    out
}

/// Decodes a welcome; `None` when it is malformed or of another version.
pub fn decode_welcome(bytes: &[u8]) -> Option<Welcome> {
    let mut reader = Reader::new(bytes);
    if reader.u8()? != kind::WELCOME || reader.u8()? != VERSION {
        return None;
    }
    let player = reader.varint_u32()?;
    let tick = reader.varint()?;
    let count = reader.count(MAX_TYPES, 2 + FINGERPRINT_LEN)?;
    let mut types = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::from(reader.u8()?);
        if len == 0 || len > MAX_TYPE_NAME {
            return None;
        }
        let name = std::str::from_utf8(reader.take(len)?).ok()?.to_owned();
        let fingerprint = reader.take(FINGERPRINT_LEN)?.try_into().ok()?;
        types.push(TypeEntry { name, fingerprint });
    }
    reader.done().then_some(Welcome {
        player,
        tick,
        types,
    })
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// What happens to one component of an entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The entity no longer has it.
    Remove,
    /// Its whole value (every leaf).
    Whole(Value),
    /// The leaves that changed against the baseline, by leaf index.
    Leaves(Vec<(usize, LeafData)>),
}

/// One component operation of an entity section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    /// The type's index in the welcome.
    pub ty: u16,
    /// What changes.
    pub change: Change,
}

/// The changes of one entity: from the state the client has of it at tick
/// `base` (none: from nothing) to its state at the message's tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The entity's network id.
    pub id: u32,
    /// The tick of the acknowledged state the operations apply to.
    pub base: Option<u64>,
    /// The operations, ascending by type.
    pub ops: Vec<Op>,
}

/// A decoded state message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The host's tick.
    pub tick: u64,
    /// The newest input frame of this player that the host applied.
    pub input_ack: Option<u64>,
    /// Whether nothing the player may see was left out by the budget
    /// ([`FLAG_COMPLETE`]).
    pub complete: bool,
    /// Network ids that left the player's view, ascending.
    pub removed: Vec<u32>,
    /// Changed entities, ascending by network id.
    pub entities: Vec<Section>,
}

const OP_REMOVE: u64 = 0;
const OP_WHOLE: u64 = 1;
const OP_LEAVES: u64 = 2;

/// The header of a state message: kind, tick, input ack, flags.
pub fn state_header(tick: u64, input_ack: Option<u64>, complete: bool) -> Vec<u8> {
    let mut out = vec![kind::STATE];
    put_varint(&mut out, tick);
    put_optional(&mut out, input_ack);
    out.push(if complete { FLAG_COMPLETE } else { 0 });
    out
}

/// The varint that names a section's base: 0 for none, else how many ticks
/// before the message's tick it is (at least 1).
pub fn base_gap(tick: u64, base: Option<u64>) -> u64 {
    base.map_or(0, |b| tick.saturating_sub(b).max(1))
}

/// Appends one leaf of `value`.
pub fn put_leaf(out: &mut Vec<u8>, value: &Value, layout: &Layout, index: usize) {
    let Some(leaf) = layout.leaves.get(index) else {
        return;
    };
    match leaf.kind {
        LeafKind::Plain => {
            let bytes = value.bytes.get(leaf.offset..leaf.offset + leaf.len);
            match bytes {
                Some(bytes) => out.extend_from_slice(bytes),
                None => out.extend(std::iter::repeat_n(0, leaf.len)),
            }
        }
        LeafKind::Text => {
            let text = value.texts.get(leaf.slot).map_or("", String::as_str);
            // A string longer than the limit cannot be sent whole; the
            // host cuts it at a character boundary.
            let mut end = text.len().min(MAX_TEXT);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            put_varint(out, end as u64);
            out.extend_from_slice(text.as_bytes().get(..end).unwrap_or_default());
        }
        LeafKind::Entity => put_varint(
            out,
            u64::from(value.refs.get(leaf.slot).copied().unwrap_or(0)),
        ),
    }
}

/// Reads one leaf of `layout` (the counterpart of [`put_leaf`]); `None` when
/// it is malformed or does not fit.
pub fn read_leaf(reader: &mut Reader<'_>, layout: &Layout, index: usize) -> Option<LeafData> {
    let leaf = layout.leaves.get(index)?;
    Some(match leaf.kind {
        LeafKind::Plain => LeafData::Plain(reader.take(leaf.len)?.to_vec()),
        LeafKind::Text => {
            let len = usize::try_from(reader.varint()?).ok()?;
            if len > MAX_TEXT {
                return None;
            }
            LeafData::Text(std::str::from_utf8(reader.take(len)?).ok()?.to_owned())
        }
        LeafKind::Entity => LeafData::Ref(reader.varint_u32()?),
    })
}

/// Appends the operation that turns `base` (none: absent) into `target`
/// (none: removed) for type `ty`; nothing when they are equal.
pub fn put_op(
    out: &mut Vec<u8>,
    ty: u16,
    layout: &Layout,
    base: Option<&Value>,
    target: Option<&Value>,
) -> bool {
    let tag = u64::from(ty) << 2;
    match (base, target) {
        (_, None) => {
            if base.is_none() {
                return false;
            }
            put_varint(out, tag | OP_REMOVE);
        }
        (None, Some(target)) => {
            put_varint(out, tag | OP_WHOLE);
            for index in 0..layout.leaves.len() {
                put_leaf(out, target, layout, index);
            }
        }
        (Some(base), Some(target)) => {
            if base == target {
                return false;
            }
            let mut mask = vec![0u8; layout.mask_len()];
            let mut any = false;
            for index in 0..layout.leaves.len() {
                if !base.leaf_eq(target, layout, index) {
                    mask[index / 8] |= 1 << (index % 8);
                    any = true;
                }
            }
            if !any {
                return false;
            }
            put_varint(out, tag | OP_LEAVES);
            out.extend_from_slice(&mask);
            for index in 0..layout.leaves.len() {
                if mask[index / 8] & (1 << (index % 8)) != 0 {
                    put_leaf(out, target, layout, index);
                }
            }
        }
    }
    true
}

/// Decodes a state message against the layouts of the welcome's types.
/// `None` when anything is malformed: a truncated or extended message, an
/// unknown type, ids out of order, a leaf that does not fit its layout.
pub fn decode_state(bytes: &[u8], layouts: &[Layout]) -> Option<State> {
    let mut reader = Reader::new(bytes);
    if reader.u8()? != kind::STATE {
        return None;
    }
    let tick = reader.varint()?;
    let input_ack = read_optional(&mut reader)?;
    let flags = reader.u8()?;
    if flags & !FLAG_COMPLETE != 0 {
        return None;
    }
    let removed_count = reader.count(usize::MAX, 1)?;
    let mut removed = Vec::with_capacity(removed_count);
    for _ in 0..removed_count {
        let id = reader.varint_u32()?;
        if removed.last().is_some_and(|last| *last >= id) {
            return None;
        }
        removed.push(id);
    }
    let entity_count = reader.count(usize::MAX, 3)?;
    let mut entities: Vec<Section> = Vec::with_capacity(entity_count);
    for _ in 0..entity_count {
        let id = reader.varint_u32()?;
        if id == 0 || entities.last().is_some_and(|last| last.id >= id) {
            return None;
        }
        let base = match reader.varint()? {
            0 => None,
            gap => Some(tick.checked_sub(gap)?),
        };
        let op_count = reader.count(layouts.len(), 1)?;
        let mut ops: Vec<Op> = Vec::with_capacity(op_count);
        for _ in 0..op_count {
            let tag = reader.varint()?;
            let ty = u16::try_from(tag >> 2).ok()?;
            if ops.last().is_some_and(|last| last.ty >= ty) {
                return None;
            }
            let layout = layouts.get(usize::from(ty))?;
            let change = match tag & 3 {
                OP_REMOVE => Change::Remove,
                OP_WHOLE => {
                    let mut value = Value::empty(layout);
                    for index in 0..layout.leaves.len() {
                        let data = read_leaf(&mut reader, layout, index)?;
                        if !value.set_leaf(layout, index, data) {
                            return None;
                        }
                    }
                    Change::Whole(value)
                }
                OP_LEAVES => {
                    let mask = reader.take(layout.mask_len())?;
                    let mut leaves = Vec::new();
                    for index in 0..layout.leaves.len() {
                        if mask[index / 8] & (1 << (index % 8)) != 0 {
                            leaves.push((index, read_leaf(&mut reader, layout, index)?));
                        }
                    }
                    // Bits past the last leaf, or an empty mask, are not a
                    // message this host writes.
                    let used = layout.leaves.len();
                    let stray = mask.iter().enumerate().any(|(byte, bits)| {
                        (0..8).any(|bit| bits & (1 << bit) != 0 && byte * 8 + bit >= used)
                    });
                    if leaves.is_empty() || stray {
                        return None;
                    }
                    Change::Leaves(leaves)
                }
                _ => return None,
            };
            // Changing leaves needs a value to change.
            if base.is_none() && matches!(change, Change::Leaves(_)) {
                return None;
            }
            ops.push(Op { ty, change });
        }
        entities.push(Section { id, base, ops });
    }
    reader.done().then_some(State {
        tick,
        input_ack,
        complete: flags & FLAG_COMPLETE != 0,
        removed,
        entities,
    })
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// One tick of a player's input: the actions not at rest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    /// The host tick the input is meant for.
    pub tick: u64,
    /// `(action id, [x, y])` for every action not at rest.
    pub actions: Vec<(u32, [f32; 2])>,
}

/// Client → host: input frames, newest first, and the acknowledgements.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Input {
    /// The player the input is for (must be the sender's own).
    pub player: u32,
    /// The newest state message the client received whole.
    pub ack: Option<u64>,
    /// Which of the 64 messages before `ack` it received whole: bit `i` is
    /// tick `ack - 1 - i`.
    pub ack_mask: u64,
    /// The client lost its baseline and asks for a new snapshot.
    pub resync: bool,
    /// Frames, newest first, at most [`MAX_FRAMES`], ticks strictly falling.
    pub frames: Vec<Frame>,
}

/// Encodes an input message. Frames beyond [`MAX_FRAMES`] and actions beyond
/// [`MAX_FRAME_ACTIONS`] are left out; frames must be newest first.
pub fn encode_input(input: &Input) -> Vec<u8> {
    let mut out = vec![kind::INPUT];
    put_varint(&mut out, u64::from(input.player));
    put_optional(&mut out, input.ack);
    out.extend_from_slice(&input.ack_mask.to_le_bytes());
    out.push(u8::from(input.resync));
    let frames = &input.frames[..input.frames.len().min(MAX_FRAMES)];
    out.push(frames.len() as u8);
    let mut previous: Option<u64> = None;
    for frame in frames {
        match previous {
            None => put_varint(&mut out, frame.tick),
            Some(p) => put_varint(&mut out, p.saturating_sub(frame.tick)),
        }
        previous = Some(frame.tick);
        let actions = &frame.actions[..frame.actions.len().min(MAX_FRAME_ACTIONS)];
        out.push(actions.len() as u8);
        for (id, [x, y]) in actions {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
    }
    out
}

/// Decodes an input message; `None` when it is malformed or breaks a limit.
pub fn decode_input(bytes: &[u8]) -> Option<Input> {
    if bytes.len() > MAX_CLIENT_MESSAGE {
        return None;
    }
    let mut reader = Reader::new(bytes);
    if reader.u8()? != kind::INPUT {
        return None;
    }
    let player = reader.varint_u32()?;
    let ack = read_optional(&mut reader)?;
    let ack_mask = u64::from_le_bytes(reader.take(8)?.try_into().ok()?);
    let flags = reader.u8()?;
    if flags > 1 {
        return None;
    }
    let count = usize::from(reader.u8()?);
    if count > MAX_FRAMES {
        return None;
    }
    let mut frames: Vec<Frame> = Vec::with_capacity(count);
    for _ in 0..count {
        let raw = reader.varint()?;
        let tick = match frames.last() {
            None => raw,
            Some(previous) => {
                if raw == 0 {
                    return None;
                }
                previous.tick.checked_sub(raw)?
            }
        };
        let actions = usize::from(reader.u8()?);
        if actions > MAX_FRAME_ACTIONS || actions * 12 > reader.remaining() {
            return None;
        }
        let mut list = Vec::with_capacity(actions);
        for _ in 0..actions {
            list.push((reader.u32()?, [reader.f32()?, reader.f32()?]));
        }
        frames.push(Frame {
            tick,
            actions: list,
        });
    }
    reader.done().then_some(Input {
        player,
        ack,
        ack_mask,
        resync: flags & 1 != 0,
        frames,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use wish_core::Rng;

    #[test]
    fn varints_round_trip_and_refuse_overflow() {
        for value in [0, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            let mut out = Vec::new();
            put_varint(&mut out, value);
            assert_eq!(out.len(), varint_len(value));
            assert_eq!(Reader::new(&out).varint(), Some(value));
        }
        // Eleven bytes, and a tenth byte that overflows.
        assert_eq!(Reader::new(&[0x80; 11]).varint(), None);
        let mut big = vec![0xFF; 9];
        big.push(0x02);
        assert_eq!(Reader::new(&big).varint(), None);
    }

    #[test]
    fn inputs_and_welcomes_round_trip() {
        let input = Input {
            player: 3,
            ack: Some(41),
            ack_mask: 0b101,
            resync: true,
            frames: vec![
                Frame {
                    tick: 50,
                    actions: vec![(7, [1.0, -0.5])],
                },
                Frame {
                    tick: 48,
                    actions: vec![],
                },
            ],
        };
        assert_eq!(decode_input(&encode_input(&input)), Some(input));
        let welcome = Welcome {
            player: 2,
            tick: 120,
            types: vec![TypeEntry {
                name: "Transform".to_owned(),
                fingerprint: [9; 16],
            }],
        };
        assert_eq!(decode_welcome(&encode_welcome(&welcome)), Some(welcome));
    }

    #[test]
    fn the_state_header_carries_the_complete_flag_and_nothing_else() {
        for complete in [false, true] {
            let mut bytes = state_header(30, Some(28), complete);
            put_varint(&mut bytes, 0);
            put_varint(&mut bytes, 0);
            let state = decode_state(&bytes, &[]).unwrap();
            assert_eq!(state.tick, 30);
            assert_eq!(state.input_ack, Some(28));
            assert_eq!(state.complete, complete);
        }
        // An unknown flag is malformed.
        let mut bytes = state_header(30, None, true);
        if let Some(flags) = bytes.last_mut() {
            *flags |= 2;
        }
        put_varint(&mut bytes, 0);
        put_varint(&mut bytes, 0);
        assert_eq!(decode_state(&bytes, &[]), None);
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut rng = Rng::from_seed(5);
        let layouts =
            [Layout::of(&<wish_types::Transform as wish_schema::Plain>::schema()).unwrap()];
        for round in 0..20_000 {
            let len = rng.below(64) as usize;
            let mut bytes: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = [kind::WELCOME, kind::STATE, kind::INPUT][round % 3];
            }
            let _ = decode_input(&bytes);
            let _ = decode_welcome(&bytes);
            let _ = decode_state(&bytes, &layouts);
        }
    }
}
