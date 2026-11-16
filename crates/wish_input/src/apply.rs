//! Writing the actions' state into the world.

use std::collections::BTreeMap;

use wish_core::Result;
use wish_ecs::World;
use wish_types::{ActionSlot, ActionState, MAX_ACTIONS, action_id};

use crate::map::InputMap;

/// The value of each action, by name: `[x, y]` (a button uses 1 or 0 in
/// `x`; an axis uses `x`). Actions not named are at rest.
pub type Values = BTreeMap<String, [f32; 2]>;

/// Registers the `ActionState` resource.
pub fn register(world: &mut World) -> Result<()> {
    world.register_resource::<ActionState>()?;
    Ok(())
}

fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Writes this tick's state of every action: `values` is what the player
/// (or a tool, or a replay) is doing now; whether an action was just
/// pressed or released comes from comparing with the previous tick's state,
/// which is in the world. Nothing is written if nothing changed.
pub fn apply(world: &mut World, map: &InputMap, values: &Values) -> Result<()> {
    let previous = world.resource::<ActionState>().copied();
    let mut state = ActionState::default();
    for (slot, name) in state.slots.iter_mut().zip(map.names()).take(MAX_ACTIONS) {
        let [x, y] = values.get(name).copied().unwrap_or([0.0, 0.0]).map(clean);
        let id = action_id(name);
        let was_held = previous
            .and_then(|p| p.slots.iter().find(|s| s.id == id).map(|s| s.held))
            .unwrap_or(false);
        let held = x != 0.0 || y != 0.0;
        *slot = ActionSlot {
            id,
            held,
            pressed: held && !was_held,
            released: !held && was_held,
            x,
            y,
        };
    }
    if previous != Some(state) {
        world.insert_resource(state)?;
    }
    Ok(())
}
