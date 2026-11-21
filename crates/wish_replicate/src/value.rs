//! The canonical form of a replicated component: the same in every world.

use std::collections::BTreeMap;
use std::sync::Arc;

use wish_core::{Entity, Result, Text};
use wish_ecs::{ComponentId, World};

use crate::layout::{Layout, LeafKind};

/// A component's value as it travels: independent of the world it came
/// from.
///
/// - `bytes`: the value's bytes, with padding, text handles and entity
///   references zeroed;
/// - `texts`: the strings of its text leaves, in leaf order;
/// - `refs`: its entity references as the receiving player's network ids, in
///   leaf order (0: none, which is also what a reference to an entity the
///   player may not see becomes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Value {
    /// The value's bytes, padding and handles zeroed.
    pub bytes: Vec<u8>,
    /// The strings of its text leaves.
    pub texts: Vec<String>,
    /// Its entity references as network ids.
    pub refs: Vec<u32>,
}

/// The replicated components of one entity, by type index (the order of
/// the host's type table).
pub type EntityView = BTreeMap<u16, Arc<Value>>;

/// What one player has (or should have) of the host's world: every entity
/// it may see, by network id.
pub type View = BTreeMap<u32, Arc<EntityView>>;

/// One leaf's content, as decoded from a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafData {
    /// Plain bytes.
    Plain(Vec<u8>),
    /// A string.
    Text(String),
    /// A network id (0: none).
    Ref(u32),
}

impl Value {
    /// A value of `layout` with every leaf zero or empty.
    pub fn empty(layout: &Layout) -> Self {
        Self {
            bytes: vec![0; layout.size],
            texts: vec![String::new(); layout.texts],
            refs: vec![0; layout.refs],
        }
    }

    /// Whether leaf `index` of `layout` is the same in both values.
    pub fn leaf_eq(&self, other: &Self, layout: &Layout, index: usize) -> bool {
        let Some(leaf) = layout.leaves.get(index) else {
            return true;
        };
        match leaf.kind {
            LeafKind::Plain => {
                let range = leaf.offset..leaf.offset + leaf.len;
                self.bytes.get(range.clone()) == other.bytes.get(range)
            }
            LeafKind::Text => self.texts.get(leaf.slot) == other.texts.get(leaf.slot),
            LeafKind::Entity => self.refs.get(leaf.slot) == other.refs.get(leaf.slot),
        }
    }

    /// Sets leaf `index` of `layout`. Returns `false` when the data does not
    /// fit the leaf (wrong kind or length).
    pub fn set_leaf(&mut self, layout: &Layout, index: usize, data: LeafData) -> bool {
        let Some(leaf) = layout.leaves.get(index) else {
            return false;
        };
        match (leaf.kind, data) {
            (LeafKind::Plain, LeafData::Plain(bytes)) if bytes.len() == leaf.len => {
                match self.bytes.get_mut(leaf.offset..leaf.offset + leaf.len) {
                    Some(slot) => {
                        slot.copy_from_slice(&bytes);
                        true
                    }
                    None => false,
                }
            }
            (LeafKind::Text, LeafData::Text(text)) => match self.texts.get_mut(leaf.slot) {
                Some(slot) => {
                    *slot = text;
                    true
                }
                None => false,
            },
            (LeafKind::Entity, LeafData::Ref(id)) => match self.refs.get_mut(leaf.slot) {
                Some(slot) => {
                    *slot = id;
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
}

/// Reads entity `entity`'s component `id` from `world` in canonical form,
/// with every entity reference (the null entity included) passed through
/// `map`, in leaf order (the receiver's network id, or 0). `Ok(None)` when
/// the entity does not have it.
pub fn read_value(
    world: &World,
    entity: Entity,
    id: ComponentId,
    layout: &Layout,
    map: &mut dyn FnMut(Entity) -> u32,
) -> Result<Option<Value>> {
    // `component_bytes` copies field spans only: padding comes back zero.
    let Some(mut bytes) = world.component_bytes(entity, id)? else {
        return Ok(None);
    };
    bytes.resize(layout.size, 0);
    let mut value = Value {
        bytes: Vec::new(),
        texts: Vec::with_capacity(layout.texts),
        refs: Vec::with_capacity(layout.refs),
    };
    for leaf in &layout.leaves {
        let range = leaf.offset..leaf.offset + leaf.len;
        match leaf.kind {
            LeafKind::Plain => {}
            LeafKind::Text => {
                let handle = bytes
                    .get(range.clone())
                    .and_then(|slot| <[u8; 4]>::try_from(slot).ok())
                    .map_or(0, u32::from_le_bytes);
                let text = world.text(Text::from_index(handle)).unwrap_or_default();
                value.texts.push(text.to_owned());
                zero(&mut bytes, range);
            }
            LeafKind::Entity => {
                let target = bytes
                    .get(range.clone())
                    .and_then(|slot| <[u8; 8]>::try_from(slot).ok())
                    .map_or(Entity::NULL, entity_from_image);
                value.refs.push(map(target));
                zero(&mut bytes, range);
            }
        }
    }
    value.bytes = bytes;
    Ok(Some(value))
}

/// Writes `value` as entity `entity`'s component `id` in `world`: strings
/// are interned in that world, network ids pass through `map` (an unknown
/// one becomes the null entity). The bytes are checked by the world (bools,
/// enums), and managed components are refused by it.
pub fn write_value(
    world: &mut World,
    entity: Entity,
    id: ComponentId,
    layout: &Layout,
    value: &Value,
    map: &dyn Fn(u32) -> Entity,
) -> Result<()> {
    let mut bytes = value.bytes.clone();
    bytes.resize(layout.size, 0);
    for leaf in &layout.leaves {
        let range = leaf.offset..leaf.offset + leaf.len;
        match leaf.kind {
            LeafKind::Plain => {}
            LeafKind::Text => {
                let text = value.texts.get(leaf.slot).map_or("", String::as_str);
                let handle = world.intern(text)?;
                if let Some(slot) = bytes.get_mut(range) {
                    slot.copy_from_slice(&handle.index().to_le_bytes());
                }
            }
            LeafKind::Entity => {
                let net = value.refs.get(leaf.slot).copied().unwrap_or(0);
                let target = if net == 0 { Entity::NULL } else { map(net) };
                if let Some(slot) = bytes.get_mut(range) {
                    slot.copy_from_slice(&entity_to_image(target));
                }
            }
        }
    }
    world.insert_bytes(entity, id, &bytes)
}

fn zero(bytes: &mut [u8], range: std::ops::Range<usize>) {
    if let Some(slot) = bytes.get_mut(range) {
        slot.fill(0);
    }
}

/// The stored bytes of an entity handle: index, then generation (as
/// `wish_schema` stores them).
fn entity_to_image(entity: Entity) -> [u8; 8] {
    let mut image = [0u8; 8];
    let (index, generation) = image.split_at_mut(4);
    index.copy_from_slice(&entity.index().to_le_bytes());
    generation.copy_from_slice(&entity.generation().to_le_bytes());
    image
}

fn entity_from_image(image: [u8; 8]) -> Entity {
    let [a, b, c, d, e, f, g, h] = image;
    Entity::new(
        u32::from_le_bytes([a, b, c, d]),
        u32::from_le_bytes([e, f, g, h]),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use wish_schema::{Plain, component};

    /// A component with every kind of leaf.
    #[component]
    pub struct Mixed {
        /// A number.
        pub a: u8,
        /// A name.
        pub name: Text,
        /// Another entity.
        pub other: Entity,
        /// A flag.
        pub flag: bool,
        /// A big number.
        pub big: u64,
    }

    #[test]
    fn values_round_trip_between_worlds() {
        let layout = Layout::of(&Mixed::schema()).unwrap();
        let mut host = World::new();
        let target = host.spawn().unwrap();
        let entity = host.spawn().unwrap();
        let name = host.intern("Ada").unwrap();
        host.insert(
            entity,
            Mixed {
                a: 7,
                name,
                other: target,
                flag: true,
                big: u64::MAX,
            },
        )
        .unwrap();
        let id = host.component_id("Mixed").unwrap();
        let value = read_value(&host, entity, id, &layout, &mut |e| {
            if e == target { 9 } else { 0 }
        })
        .unwrap()
        .unwrap();
        assert_eq!(value.texts, vec!["Ada".to_owned()]);
        assert_eq!(value.refs, vec![9]);

        let mut client = World::new();
        client.register_component::<Mixed>().unwrap();
        let there = client.spawn().unwrap();
        let mirror = client.spawn().unwrap();
        let cid = client.component_id("Mixed").unwrap();
        write_value(&mut client, mirror, cid, &layout, &value, &|net| {
            if net == 9 { there } else { Entity::NULL }
        })
        .unwrap();
        let got = client.get::<Mixed>(mirror).unwrap();
        assert_eq!(got.a, 7);
        assert_eq!(got.other, there);
        assert!(got.flag);
        assert_eq!(got.big, u64::MAX);
        assert_eq!(client.text(got.name), Some("Ada"));
    }

    #[test]
    fn invalid_bytes_are_refused_by_the_world() {
        let layout = Layout::of(&Mixed::schema()).unwrap();
        let mut world = World::new();
        world.register_component::<Mixed>().unwrap();
        let entity = world.spawn().unwrap();
        let id = world.component_id("Mixed").unwrap();
        let mut value = Value::empty(&layout);
        let flag = layout.leaves.iter().position(|l| l.offset == 16).unwrap();
        assert!(value.set_leaf(&layout, flag, LeafData::Plain(vec![7])));
        assert!(write_value(&mut world, entity, id, &layout, &value, &|_| Entity::NULL).is_err());
        assert!(!world.has::<Mixed>(entity));
    }
}
