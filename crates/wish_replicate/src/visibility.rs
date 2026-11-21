//! Who may see each replicated entity (design note §3.1).

use std::collections::BTreeMap;

use wish_core::Entity;
use wish_core::math::Vec3;
use wish_ecs::World;
use wish_types::{
    GlobalTransform, MAX_PLAYERS, NetGroups, NetOwner, NetViewer, NetVisibility, PlayerId,
    Replicated, Transform, world_position,
};

/// One replicated entity and what decides who sees it.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    /// The entity.
    pub entity: Entity,
    /// Its rule.
    pub rule: Replicated,
    /// Its owner, if it has one.
    pub owner: Option<PlayerId>,
    /// Where it is, if it has a placement.
    pub position: Option<Vec3>,
}

/// The replicated entities of a world and the players' viewpoints, read
/// once per tick.
#[derive(Debug, Default)]
pub struct Scene {
    /// Every replicated entity, in entity order.
    pub candidates: Vec<Candidate>,
    /// Every player's viewpoints.
    pub viewers: BTreeMap<PlayerId, Vec<Vec3>>,
    /// Every player's group bits.
    pub groups: [u32; MAX_PLAYERS],
}

/// Where an entity is: its world position, else its local translation.
pub fn position_of(world: &World, entity: Entity) -> Option<Vec3> {
    if let Some(global) = world.get::<GlobalTransform>(entity) {
        return Some(world_position(global));
    }
    world.get::<Transform>(entity).map(|t| t.translation)
}

impl Scene {
    /// Reads the replicated entities and viewpoints of `world`.
    pub fn read(world: &World) -> Self {
        let mut scene = Self {
            groups: world
                .resource::<NetGroups>()
                .map_or([0; MAX_PLAYERS], |g| g.players),
            ..Self::default()
        };
        let registry = world.registry();
        let replicated = registry.component_id_of::<Replicated>();
        let viewer = registry.component_id_of::<NetViewer>();
        for archetype in world.archetypes() {
            let has_rule = replicated.is_some_and(|id| archetype.contains(id));
            let has_viewer = viewer.is_some_and(|id| archetype.contains(id));
            if !has_rule && !has_viewer {
                continue;
            }
            for &entity in archetype.entities() {
                if has_rule && let Some(rule) = world.get::<Replicated>(entity) {
                    scene.candidates.push(Candidate {
                        entity,
                        rule: *rule,
                        owner: world.get::<NetOwner>(entity).map(|o| o.player),
                        position: position_of(world, entity),
                    });
                }
                if has_viewer
                    && let Some(view) = world.get::<NetViewer>(entity)
                    && let Some(at) = position_of(world, entity)
                    && at.is_finite()
                {
                    scene.viewers.entry(view.player).or_default().push(at);
                }
            }
        }
        // Archetype order moves when components are added; entity order
        // does not.
        scene.candidates.sort_by_key(|c| c.entity);
        scene
    }

    /// Whether `player` may see `candidate`.
    pub fn sees(&self, player: PlayerId, candidate: &Candidate) -> bool {
        if candidate.owner == Some(player) {
            return true;
        }
        let rule = &candidate.rule;
        match rule.visibility {
            NetVisibility::Everyone => true,
            NetVisibility::Owner => false,
            NetVisibility::Group => {
                let mine = self.groups.get(player as usize).copied().unwrap_or(0);
                mine & rule.groups != 0
            }
            NetVisibility::Players => player < 32 && rule.players & (1 << player) != 0,
            // Line of sight narrows `Sight` in a later package (design note
            // §3.1); until then it is `Near`.
            NetVisibility::Near | NetVisibility::Sight => {
                let radius = clean_radius(rule.radius);
                let Some(at) = candidate.position.filter(|p| p.is_finite()) else {
                    return false;
                };
                self.viewers.get(&player).is_some_and(|points| {
                    points
                        .iter()
                        .any(|p| p.distance_squared(at) <= radius * radius)
                })
            }
        }
    }
}

/// A radius that is not a number, or below 0, counts as 0.
pub fn clean_radius(radius: f32) -> f32 {
    if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    }
}

/// A priority in thousandths, from 10 (0.01) to 1 000 000 (1000); a value
/// that is not a number counts as the default (1).
pub fn priority_milli(priority: f32) -> u64 {
    let p = if priority.is_finite() { priority } else { 1.0 };
    let p = p.clamp(0.01, 1000.0);
    (f64::from(p) * 1000.0).round() as u64
}
