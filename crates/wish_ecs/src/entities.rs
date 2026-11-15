//! Entity allocation and the table of where each entity's data lives.

use wish_core::{Diagnostic, Entity, Result};

/// Where an entity's components are: which archetype, which row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntityLocation {
    pub archetype: usize,
    pub row: usize,
}

#[derive(Clone, Debug)]
struct Slot {
    generation: u32,
    location: Option<EntityLocation>,
}

/// Hands out entity handles and tracks their locations.
///
/// Freed indices are reused last-in, first-out, with the generation bumped so
/// that old handles stop matching. Reuse order depends only on the sequence
/// of spawns and despawns, so it is deterministic.
#[derive(Clone, Debug, Default)]
pub(crate) struct Entities {
    slots: Vec<Slot>,
    free: Vec<u32>,
    alive: usize,
}

impl Entities {
    pub fn alloc(&mut self, location: EntityLocation) -> Result<Entity> {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.slots.len())
                    .ok()
                    .filter(|index| *index < Entity::NULL.index())
                    .ok_or_else(|| {
                        Diagnostic::error(
                            "too_many_entities",
                            "The world cannot hold more entities.",
                            "A world holds at most about four billion entities at once. Despawn entities you no longer need.",
                        )
                    })?;
                self.slots.push(Slot {
                    generation: 0,
                    location: None,
                });
                index
            }
        };
        let Some(slot) = self.slots.get_mut(index as usize) else {
            return Err(crate::internal_error(
                "free list points past the entity table",
            ));
        };
        slot.location = Some(location);
        self.alive += 1;
        Ok(Entity::new(index, slot.generation))
    }

    fn slot_mut(&mut self, entity: Entity) -> Option<&mut Slot> {
        self.slots
            .get_mut(entity.index() as usize)
            .filter(|slot| slot.generation == entity.generation() && slot.location.is_some())
    }

    /// Frees a living entity and returns where its data was.
    pub fn free(&mut self, entity: Entity) -> Option<EntityLocation> {
        let slot = self.slot_mut(entity)?;
        let location = slot.location.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(entity.index());
        self.alive -= 1;
        Some(location)
    }

    /// Where a living entity's data is; `None` for dead or unknown handles.
    pub fn location(&self, entity: Entity) -> Option<EntityLocation> {
        let slot = self.slots.get(entity.index() as usize)?;
        if slot.generation == entity.generation() {
            slot.location
        } else {
            None
        }
    }

    /// Records that a living entity's data moved.
    pub fn set_location(&mut self, entity: Entity, location: EntityLocation) {
        if let Some(slot) = self.slot_mut(entity) {
            slot.location = Some(location);
        }
    }

    pub fn alive(&self) -> usize {
        self.alive
    }

    /// Every living entity, in ascending index order.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        (0u32..)
            .zip(&self.slots)
            .filter(|(_, slot)| slot.location.is_some())
            .map(|(index, slot)| Entity::new(index, slot.generation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: EntityLocation = EntityLocation {
        archetype: 0,
        row: 0,
    };

    #[test]
    fn freed_indices_are_reused_with_a_new_generation() {
        let mut entities = Entities::default();
        let a = entities.alloc(HERE).unwrap();
        let b = entities.alloc(HERE).unwrap();
        assert_eq!((a, b), (Entity::new(0, 0), Entity::new(1, 0)));
        assert_eq!(entities.free(a), Some(HERE));
        assert_eq!(entities.free(a), None, "a handle can only be freed once");
        assert_eq!(entities.location(a), None);
        let c = entities.alloc(HERE).unwrap();
        assert_eq!(c, Entity::new(0, 1));
        assert_eq!(entities.location(a), None, "the old handle stays dead");
        assert_eq!(entities.alive(), 2);
        assert_eq!(entities.iter().collect::<Vec<_>>(), [c, b]);
    }

    #[test]
    fn reuse_is_last_in_first_out() {
        let mut entities = Entities::default();
        let handles: Vec<Entity> = (0..4).map(|_| entities.alloc(HERE).unwrap()).collect();
        entities.free(handles[1]);
        entities.free(handles[3]);
        assert_eq!(entities.alloc(HERE).unwrap().index(), 3);
        assert_eq!(entities.alloc(HERE).unwrap().index(), 1);
        assert_eq!(entities.alloc(HERE).unwrap().index(), 4);
    }

    #[test]
    fn unknown_and_null_handles_have_no_location() {
        let entities = Entities::default();
        assert_eq!(entities.location(Entity::new(7, 0)), None);
        assert_eq!(entities.location(Entity::NULL), None);
    }
}
