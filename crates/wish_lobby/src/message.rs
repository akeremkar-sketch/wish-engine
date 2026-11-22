//! Network messages: event types with a `net: Net` envelope, their values
//! in a world-independent form, and their encoding leaf by leaf (design
//! note §3.3).

use wish_core::{Diagnostic, Entity, Result, Text};
use wish_ecs::{EventId, World};
use wish_replicate::wire::{self as rwire, Reader};
use wish_replicate::{Layout, LeafKind, Value};
use wish_schema::{Plain, TypeKind};
use wish_types::{Net, NetTo, PlayerId};

/// Message types a session carries at most.
pub const MAX_MESSAGE_TYPES: usize = 64;

/// The name of the envelope field.
pub const ENVELOPE_FIELD: &str = "net";

/// One network message type, as a world knows it.
#[derive(Debug, Clone)]
pub struct MessageType {
    /// The event's registered name.
    pub name: String,
    /// Its id in this world.
    pub id: EventId,
    /// Its schema fingerprint.
    pub fingerprint: [u8; 16],
    /// Its leaves.
    pub layout: Layout,
    /// Where the envelope starts in a value.
    pub net_offset: usize,
}

impl MessageType {
    /// The envelope of `bytes` (a value of this type), or the default.
    pub fn envelope(&self, bytes: &[u8]) -> Net {
        bytes
            .get(self.net_offset..self.net_offset + size_of::<Net>())
            .and_then(Net::decode)
            .unwrap_or_default()
    }

    /// Writes `net` into `bytes` (a value of this type).
    pub fn set_envelope(&self, bytes: &mut [u8], net: &Net) {
        if let Some(slot) = bytes.get_mut(self.net_offset..self.net_offset + size_of::<Net>()) {
            net.encode(slot);
        }
    }
}

/// The envelope's offset in `schema`, when it is a network message: a
/// top-level field named `net` whose type is the engine's `Net`.
fn envelope_offset(schema: &wish_schema::TypeSchema) -> Option<usize> {
    let TypeKind::Struct(fields) = &schema.kind else {
        return None;
    };
    let net = Net::schema();
    fields
        .iter()
        .find(|f| f.name == ENVELOPE_FIELD && f.ty == net)
        .map(|f| f.offset as usize)
}

/// Every network message type registered in `world`, ascending by name (at
/// most [`MAX_MESSAGE_TYPES`]; a warning says when there are more).
pub fn message_types(world: &World) -> (Vec<MessageType>, Vec<Diagnostic>) {
    let mut found: Vec<MessageType> = Vec::new();
    let mut problems = Vec::new();
    for (id, info) in world.registry().events() {
        let Some(net_offset) = envelope_offset(info.schema()) else {
            continue;
        };
        match Layout::of(info.schema()) {
            Ok(layout) => found.push(MessageType {
                name: info.name().to_owned(),
                id,
                fingerprint: info.fingerprint().as_u128().to_le_bytes(),
                layout,
                net_offset,
            }),
            Err(problem) => problems.push(problem),
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    if found.len() > MAX_MESSAGE_TYPES {
        let dropped: Vec<String> = found.drain(MAX_MESSAGE_TYPES..).map(|t| t.name).collect();
        problems.push(
            Diagnostic::warning(
                "net_config_invalid",
                format!(
                    "the game has more than {MAX_MESSAGE_TYPES} network message types; these are not carried: {}",
                    dropped.join(", ")
                ),
                "Merge message types: one event type with a kind field can carry many messages.",
            )
            .with_candidates(dropped),
        );
    }
    (found, problems)
}

/// A message's value, independent of the world: its bytes with text handles
/// and entity references zeroed (padding is zero already), its strings in
/// leaf order, and its entity references as the host's entities in leaf
/// order (the host's match log keeps these).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageValue {
    /// The value's bytes, handles zeroed.
    pub bytes: Vec<u8>,
    /// The strings of its text leaves.
    pub texts: Vec<String>,
    /// Its entity references.
    pub refs: Vec<Entity>,
}

fn image(bytes: &[u8], offset: usize) -> Option<[u8; 8]> {
    bytes.get(offset..offset + 8)?.try_into().ok()
}

impl MessageValue {
    /// Reads event bytes of `ty` from `world` (texts looked up there).
    pub fn read(world: &World, ty: &MessageType, raw: &[u8]) -> Self {
        let mut bytes = raw.to_vec();
        bytes.resize(ty.layout.size, 0);
        let mut value = Self::default();
        for leaf in &ty.layout.leaves {
            let range = leaf.offset..leaf.offset + leaf.len;
            match leaf.kind {
                LeafKind::Plain => {}
                LeafKind::Text => {
                    let handle = bytes
                        .get(range.clone())
                        .and_then(|s| <[u8; 4]>::try_from(s).ok())
                        .map_or(0, u32::from_le_bytes);
                    let text = world.text(Text::from_index(handle)).unwrap_or_default();
                    value.texts.push(text.to_owned());
                    zero(&mut bytes, range);
                }
                LeafKind::Entity => {
                    let entity = image(&bytes, leaf.offset)
                        .map_or(Entity::NULL, |i| Entity::from_bits(u64::from_le_bytes(i)));
                    value.refs.push(entity);
                    zero(&mut bytes, range);
                }
            }
        }
        value.bytes = bytes;
        value
    }

    /// The event bytes of this value in `world`: strings interned there,
    /// references written as they are.
    pub fn write(&self, world: &mut World, ty: &MessageType) -> Result<Vec<u8>> {
        let mut bytes = self.bytes.clone();
        bytes.resize(ty.layout.size, 0);
        for leaf in &ty.layout.leaves {
            let range = leaf.offset..leaf.offset + leaf.len;
            match leaf.kind {
                LeafKind::Plain => {}
                LeafKind::Text => {
                    let text = self.texts.get(leaf.slot).map_or("", String::as_str);
                    let handle = world.intern(text)?;
                    if let Some(slot) = bytes.get_mut(range) {
                        slot.copy_from_slice(&handle.index().to_le_bytes());
                    }
                }
                LeafKind::Entity => {
                    let entity = self.refs.get(leaf.slot).copied().unwrap_or(Entity::NULL);
                    if let Some(slot) = bytes.get_mut(range) {
                        slot.copy_from_slice(&entity.to_bits().to_le_bytes());
                    }
                }
            }
        }
        Ok(bytes)
    }

    /// The value as it travels to or from one player: references through
    /// `map` (the player's network id, or 0).
    pub fn to_wire(&self, map: &dyn Fn(Entity) -> u32) -> Value {
        Value {
            bytes: self.bytes.clone(),
            texts: self.texts.clone(),
            refs: self.refs.iter().map(|e| map(*e)).collect(),
        }
    }

    /// A value received from one player: network ids through `map` (an
    /// unknown one becomes the null entity).
    pub fn from_wire(value: Value, map: &dyn Fn(u32) -> Entity) -> Self {
        Self {
            refs: value
                .refs
                .iter()
                .map(|n| if *n == 0 { Entity::NULL } else { map(*n) })
                .collect(),
            bytes: value.bytes,
            texts: value.texts,
        }
    }
}

fn zero(bytes: &mut [u8], range: std::ops::Range<usize>) {
    if let Some(slot) = bytes.get_mut(range) {
        slot.fill(0);
    }
}

/// Appends every leaf of `value` (of `layout`).
pub fn put_value(out: &mut Vec<u8>, value: &Value, layout: &Layout) {
    for index in 0..layout.leaves.len() {
        rwire::put_leaf(out, value, layout, index);
    }
}

/// Reads every leaf of a value of `layout`; `None` when malformed.
pub fn read_value(reader: &mut Reader<'_>, layout: &Layout) -> Option<Value> {
    let mut value = Value::empty(layout);
    for index in 0..layout.leaves.len() {
        let data = rwire::read_leaf(reader, layout, index)?;
        if !value.set_leaf(layout, index, data) {
            return None;
        }
    }
    Some(value)
}

/// Whether `bytes` (texts and references zeroed) hold a valid value of the
/// type with `id` in `world` (bools and enums in range).
pub fn valid(world: &World, id: EventId, bytes: &[u8]) -> bool {
    world.registry().event(id).is_some_and(|info| {
        bytes.len() == info.size() && wish_schema::validate_bytes(info.schema(), bytes).is_ok()
    })
}

/// The envelope a delivered copy carries: what the sender asked, with
/// `from` and `delivered` from the engine.
pub fn delivered(sent: &Net, from: PlayerId) -> Net {
    Net {
        from,
        delivered: true,
        ..*sent
    }
}

/// The envelope of a message a player sent: always to the host, whatever
/// its bytes said, from the seat of its connection.
pub fn from_player(from: PlayerId) -> Net {
    Net {
        to: NetTo::Host,
        player: 0,
        groups: 0,
        from,
        delivered: true,
    }
}
