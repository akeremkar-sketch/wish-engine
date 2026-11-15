//! The parent/child hierarchy.
//!
//! Links live in one component, [`Hierarchy`], as an intrusive doubly linked
//! list of siblings under each parent. No component holds a list, so the
//! hierarchy stays plain data: it snapshots, hashes and serialises like
//! everything else.

use wish_core::{Diagnostic, Entity, Result};
use wish_schema::{Plain, component};

use crate::World;
use crate::internal_error;
use crate::registry::ComponentId;

/// Where an entity sits in the parent/child hierarchy.
///
/// Managed by the world: it can be read like any component, but only
/// `World::set_parent` and `World::remove_parent` change it, so the links
/// always agree with each other. Children keep the order in which they were
/// attached. Despawning an entity despawns its children too.
#[component]
pub struct Hierarchy {
    /// The parent, or null for an entity at the top.
    parent: Entity,
    /// The first child, or null.
    first_child: Entity,
    /// The last child, or null.
    last_child: Entity,
    /// The sibling attached just before this entity, or null.
    prev_sibling: Entity,
    /// The sibling attached just after this entity, or null.
    next_sibling: Entity,
}

fn non_null(entity: Entity) -> Option<Entity> {
    (!entity.is_null()).then_some(entity)
}

impl Hierarchy {
    /// The parent.
    pub fn parent(&self) -> Option<Entity> {
        non_null(self.parent)
    }

    /// The first child.
    pub fn first_child(&self) -> Option<Entity> {
        non_null(self.first_child)
    }

    /// The last child.
    pub fn last_child(&self) -> Option<Entity> {
        non_null(self.last_child)
    }

    /// The previous sibling.
    pub fn prev_sibling(&self) -> Option<Entity> {
        non_null(self.prev_sibling)
    }

    /// The next sibling.
    pub fn next_sibling(&self) -> Option<Entity> {
        non_null(self.next_sibling)
    }
}

fn hierarchy_cycle(child: Entity, parent: Entity) -> Diagnostic {
    Diagnostic::error(
        "hierarchy_cycle",
        format!(
            "Entity {parent} cannot become the parent of {child}: {child} is {parent} itself or one of its ancestors."
        ),
        "A hierarchy cannot contain loops. Choose a parent that is not the entity itself or below it.",
    )
}

impl World {
    /// The id of the hierarchy component, registering it if needed.
    pub(crate) fn hierarchy_id(&mut self) -> Result<ComponentId> {
        let id = self.registry_mut().register_component::<Hierarchy>()?;
        self.registry_mut().mark_managed(id);
        Ok(id)
    }

    fn links(&self, entity: Entity) -> Hierarchy {
        self.get::<Hierarchy>(entity).copied().unwrap_or_default()
    }

    fn set_links(&mut self, entity: Entity, links: Hierarchy) -> Result<()> {
        let id = self.hierarchy_id()?;
        let mut bytes = vec![0u8; size_of::<Hierarchy>()];
        links.encode(&mut bytes);
        self.put_component(entity, id, &bytes)
    }

    /// An upper bound on how many links a walk can follow; reaching it means
    /// the links are corrupt, which must never hang the engine.
    fn walk_limit(&self) -> usize {
        self.entity_count() + 1
    }

    /// The entity's parent.
    pub fn parent(&self, entity: Entity) -> Option<Entity> {
        self.get::<Hierarchy>(entity)?.parent()
    }

    /// The entity's children, in the order they were attached.
    pub fn children(&self, entity: Entity) -> Vec<Entity> {
        let limit = self.walk_limit();
        let mut children = Vec::new();
        let mut next = self
            .get::<Hierarchy>(entity)
            .and_then(Hierarchy::first_child);
        while let Some(child) = next {
            if children.len() >= limit {
                break;
            }
            children.push(child);
            next = self
                .get::<Hierarchy>(child)
                .and_then(Hierarchy::next_sibling);
        }
        children
    }

    /// Everything below the entity, depth first, each parent before its
    /// children and siblings in order. The entity itself is not included.
    pub fn descendants(&self, entity: Entity) -> Vec<Entity> {
        let limit = self.walk_limit();
        let mut descendants = Vec::new();
        let mut stack: Vec<Entity> = self.children(entity).into_iter().rev().collect();
        while let Some(next) = stack.pop() {
            if descendants.len() >= limit {
                break;
            }
            descendants.push(next);
            stack.extend(self.children(next).into_iter().rev());
        }
        descendants
    }

    /// Makes `child` the last child of `parent`, detaching it from its
    /// current parent first. Nothing changes if `parent` already is its parent.
    pub fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<()> {
        for entity in [child, parent] {
            if !self.contains(entity) {
                return Err(crate::world::entity_not_found(entity));
            }
        }
        let mut cursor = Some(parent);
        let mut steps = 0;
        while let Some(ancestor) = cursor {
            if ancestor == child {
                return Err(hierarchy_cycle(child, parent));
            }
            steps += 1;
            if steps > self.walk_limit() {
                return Err(internal_error("the hierarchy has a loop"));
            }
            cursor = self.parent(ancestor);
        }
        if self.parent(child) == Some(parent) {
            return Ok(());
        }
        self.detach(child)?;
        self.attach(child, parent)
    }

    /// Detaches `child` from its parent, making it a top-level entity. Its
    /// own children stay with it.
    pub fn remove_parent(&mut self, child: Entity) -> Result<()> {
        if !self.contains(child) {
            return Err(crate::world::entity_not_found(child));
        }
        self.detach(child)
    }

    /// Unlinks `child` from its parent and siblings.
    pub(crate) fn detach(&mut self, child: Entity) -> Result<()> {
        let links = self.links(child);
        let Some(parent) = links.parent() else {
            return Ok(());
        };
        match links.prev_sibling() {
            Some(prev) => {
                let mut prev_links = self.links(prev);
                prev_links.next_sibling = links.next_sibling;
                self.set_links(prev, prev_links)?;
            }
            None => {
                let mut parent_links = self.links(parent);
                parent_links.first_child = links.next_sibling;
                self.set_links(parent, parent_links)?;
            }
        }
        match links.next_sibling() {
            Some(next) => {
                let mut next_links = self.links(next);
                next_links.prev_sibling = links.prev_sibling;
                self.set_links(next, next_links)?;
            }
            None => {
                let mut parent_links = self.links(parent);
                parent_links.last_child = links.prev_sibling;
                self.set_links(parent, parent_links)?;
            }
        }
        self.set_links(
            child,
            Hierarchy {
                parent: Entity::NULL,
                prev_sibling: Entity::NULL,
                next_sibling: Entity::NULL,
                ..links
            },
        )
    }

    /// Links a detached `child` as the last child of `parent`.
    fn attach(&mut self, child: Entity, parent: Entity) -> Result<()> {
        let mut parent_links = self.links(parent);
        let last = parent_links.last_child;
        match non_null(last) {
            Some(last) => {
                let mut last_links = self.links(last);
                last_links.next_sibling = child;
                self.set_links(last, last_links)?;
            }
            None => parent_links.first_child = child,
        }
        parent_links.last_child = child;
        self.set_links(parent, parent_links)?;
        let child_links = self.links(child);
        self.set_links(
            child,
            Hierarchy {
                parent,
                prev_sibling: last,
                next_sibling: Entity::NULL,
                ..child_links
            },
        )
    }
}
