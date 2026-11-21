//! A component's leaves: the parts of a value that are sent and compared
//! one by one.

use wish_core::{Diagnostic, Result};
use wish_schema::{TypeKind, TypeSchema};

/// Leaves a type may have before its field spans are merged into fewer,
/// larger leaves (a delta's mask then stays short).
pub const MAX_FINE_LEAVES: usize = 64;

/// Nodes a walk over one schema may visit (as `wish_schema::MAX_NODES`).
const MAX_WALK: u64 = 1 << 20;

/// What a leaf holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafKind {
    /// Plain bytes (numbers, vectors, bools, enums), sent as they are.
    Plain,
    /// A text handle (`Text` or `AssetRef`), sent as its string.
    Text,
    /// An entity reference, sent as the receiver's network id.
    Entity,
}

/// One leaf: a byte range of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaf {
    /// Where it starts in the value.
    pub offset: usize,
    /// How many bytes it covers in the value (4 for a text handle, 8 for an
    /// entity).
    pub len: usize,
    /// What it holds.
    pub kind: LeafKind,
    /// Its place among the leaves of its kind: the index of its string for
    /// a text leaf, of its reference for an entity leaf, 0 for a plain one.
    pub slot: usize,
}

/// The leaves of one type, in byte order. Padding belongs to no leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The size of a value in bytes.
    pub size: usize,
    /// The leaves, sorted by offset.
    pub leaves: Vec<Leaf>,
    /// How many leaves are text handles.
    pub texts: usize,
    /// How many leaves are entity references.
    pub refs: usize,
}

impl Layout {
    /// The leaves of `schema`.
    pub fn of(schema: &TypeSchema) -> Result<Self> {
        let mut leaves = Vec::new();
        let mut visited = 0u64;
        walk(schema, 0, &mut leaves, &mut visited)?;
        leaves.sort_by_key(|leaf| leaf.offset);
        if leaves.len() > MAX_FINE_LEAVES {
            leaves = merge(leaves);
        }
        let (mut texts, mut refs) = (0, 0);
        for leaf in &mut leaves {
            leaf.slot = match leaf.kind {
                LeafKind::Plain => 0,
                LeafKind::Text => {
                    texts += 1;
                    texts - 1
                }
                LeafKind::Entity => {
                    refs += 1;
                    refs - 1
                }
            };
        }
        let size = schema.byte_len();
        let mut end = 0usize;
        for leaf in &leaves {
            if leaf.offset < end || leaf.offset.saturating_add(leaf.len) > size {
                return Err(Diagnostic::error(
                    "net_component_not_replicable",
                    format!(
                        "the schema of {} has overlapping or out-of-range fields",
                        schema.name
                    ),
                    "Schemas made by #[component] are always well formed; a hand-made schema must place every field inside the value without overlap.",
                ));
            }
            end = leaf.offset + leaf.len;
        }
        Ok(Self {
            size,
            texts,
            refs,
            leaves,
        })
    }

    /// Bytes of a changed-leaves mask.
    pub fn mask_len(&self) -> usize {
        self.leaves.len().div_ceil(8)
    }
}

fn walk(schema: &TypeSchema, base: usize, out: &mut Vec<Leaf>, visited: &mut u64) -> Result<()> {
    *visited += 1;
    if *visited > MAX_WALK {
        return Err(Diagnostic::error(
            "net_component_not_replicable",
            format!(
                "the schema of {} has too many fields to replicate",
                schema.name
            ),
            "Replicate smaller components: split a large array into several components.",
        ));
    }
    match &schema.kind {
        TypeKind::Struct(fields) => {
            for field in fields {
                walk(&field.ty, base + field.offset as usize, out, visited)?;
            }
        }
        TypeKind::Array { element, len } => {
            let size = element.byte_len();
            for index in 0..*len as usize {
                walk(element, base + index * size, out, visited)?;
            }
        }
        TypeKind::Text | TypeKind::AssetRef => out.push(Leaf {
            offset: base,
            len: 4,
            kind: LeafKind::Text,
            slot: 0,
        }),
        TypeKind::Entity => out.push(Leaf {
            offset: base,
            len: 8,
            kind: LeafKind::Entity,
            slot: 0,
        }),
        _ => {
            if schema.size > 0 {
                out.push(Leaf {
                    offset: base,
                    len: schema.byte_len(),
                    kind: LeafKind::Plain,
                    slot: 0,
                });
            }
        }
    }
    Ok(())
}

/// Merges adjacent plain leaves that touch (the field spans), keeping text
/// and entity leaves apart.
fn merge(leaves: Vec<Leaf>) -> Vec<Leaf> {
    let mut merged: Vec<Leaf> = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        match merged.last_mut() {
            Some(last)
                if last.kind == LeafKind::Plain
                    && leaf.kind == LeafKind::Plain
                    && last.offset + last.len == leaf.offset =>
            {
                last.len += leaf.len;
            }
            _ => merged.push(leaf),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use wish_schema::Plain;
    use wish_types::{PlayerActionState, Replicated, Transform};

    #[test]
    fn primitive_fields_are_leaves() {
        let layout = Layout::of(&Transform::schema()).unwrap();
        assert_eq!(layout.size, 40);
        // translation, rotation, scale.
        assert_eq!(layout.leaves.len(), 3);
        assert_eq!(layout.mask_len(), 1);
        let layout = Layout::of(&Replicated::schema()).unwrap();
        assert_eq!(layout.leaves.len(), 5);
    }

    #[test]
    fn large_types_use_merged_spans() {
        let layout = Layout::of(&PlayerActionState::schema()).unwrap();
        // 16 players x 32 slots, each slot with padding after its bools.
        assert!(layout.leaves.len() <= 16 * 32 * 2);
        assert!(layout.leaves.iter().all(|l| l.kind == LeafKind::Plain));
        let covered: usize = layout.leaves.iter().map(|l| l.len).sum();
        assert!(covered < layout.size, "padding is not part of any leaf");
    }
}
