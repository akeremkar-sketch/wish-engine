//! Writing the actions' state into the world.

use std::collections::BTreeMap;

use wish_core::{Diagnostic, Result};
use wish_ecs::World;
use wish_types::{
    ActionSlot, ActionState, MAX_ACTIONS, MAX_PLAYERS, PlayerActionSlots, PlayerActionState,
    Players, Pointer, action_id,
};

use crate::map::InputMap;

/// The value of each action, by name: `[x, y]` (a button uses 1 or 0 in
/// `x`; an axis uses `x`). Actions not named are at rest. Two names are the
/// pointer's, not actions: [`POINTER`] (where it is, in interface pixels)
/// and [`CLICK`] (its button, 1 or 0).
pub type Values = BTreeMap<String, [f32; 2]>;

/// The pointer's position in [`Values`]: interface pixels from the top left
/// of the virtual screen (`UiSettings`). Absent: no pointer this tick.
pub const POINTER: &str = "pointer";

/// The pointer's button in [`Values`]: 1 down, 0 up.
pub const CLICK: &str = "click";

/// The farthest a pointer position may be from the screen, in pixels.
const MAX_POINTER: f32 = 1.0e6;

/// Registers the `ActionState`, `Pointer`, `Players` and
/// `PlayerActionState` resources.
pub fn register(world: &mut World) -> Result<()> {
    world.register_resource::<ActionState>()?;
    world.register_resource::<Pointer>()?;
    world.register_resource::<Players>()?;
    world.register_resource::<PlayerActionState>()?;
    Ok(())
}

fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Writes this tick's state of every action, and of the pointer: `values`
/// is what the player (or a tool, or a replay) is doing now; whether an
/// action was just pressed or released comes from comparing with the
/// previous tick's state, which is in the world. Nothing is written if
/// nothing changed: a project without actions gets no `ActionState`, and a
/// world that never had a pointer gets no `Pointer` until one comes.
pub fn apply(world: &mut World, map: &InputMap, values: &Values) -> Result<()> {
    apply_pointer(world, values)?;
    if map.is_empty() {
        return Ok(());
    }
    let previous = world.resource::<ActionState>().copied();
    let state = ActionState {
        slots: slots(map, values, previous.as_ref().map(|p| &p.slots)),
    };
    if previous != Some(state) {
        world.insert_resource(state)?;
    }
    Ok(())
}

/// The state of every action for one player: `values` now, edges against
/// `previous` (that player's slots at the last tick).
fn slots(
    map: &InputMap,
    values: &Values,
    previous: Option<&[ActionSlot; MAX_ACTIONS]>,
) -> [ActionSlot; MAX_ACTIONS] {
    let mut slots = [ActionSlot::default(); MAX_ACTIONS];
    for (slot, name) in slots.iter_mut().zip(map.names()).take(MAX_ACTIONS) {
        let [x, y] = values.get(name).copied().unwrap_or([0.0, 0.0]).map(clean);
        let id = action_id(name);
        let was_held = previous
            .and_then(|p| p.iter().find(|s| s.id == id).map(|s| s.held))
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
    slots
}

/// Says how many players the world's session has, before its first tick:
/// with more than one, the `Players` resource is written; with one, nothing
/// is (a world without it has one player, so a single-player world stays as
/// it always was).
pub fn set_players(world: &mut World, count: usize) -> Result<()> {
    if count == 0 || count > MAX_PLAYERS {
        return Err(Diagnostic::error(
            "invalid_parameter",
            format!("A session can have 1 to {MAX_PLAYERS} players, not {count}."),
            "Start the session with fewer players.",
        ));
    }
    if count > 1 {
        let players = Players {
            count: u32::try_from(count).unwrap_or(1),
        };
        if world.resource::<Players>() != Some(&players) {
            world.insert_resource(players)?;
        }
    }
    Ok(())
}

/// [`apply`] for a session of `inputs.len()` players: `inputs[p]` is what
/// player `p` does now. Player 0 is written to `ActionState` (and is the
/// only one with a pointer, which is this machine's interface). With more
/// than one player, `Players` and every player's actions in
/// `PlayerActionState` are written too (only what changed); with one, this
/// is exactly [`apply`].
pub fn apply_players(world: &mut World, map: &InputMap, inputs: &[Values]) -> Result<()> {
    let rest = Values::new();
    apply(world, map, inputs.first().unwrap_or(&rest))?;
    if inputs.len() <= 1 {
        return Ok(());
    }
    set_players(world, inputs.len())?;
    if map.is_empty() {
        return Ok(());
    }
    let previous = world.resource::<PlayerActionState>().copied();
    let mut state = PlayerActionState::default();
    for (entry, (player, values)) in state.players.iter_mut().zip(inputs.iter().enumerate()) {
        let before = previous.as_ref().and_then(|p| p.players.get(player));
        *entry = PlayerActionSlots {
            slots: slots(map, values, before.map(|b| &b.slots)),
        };
    }
    if previous != Some(state) {
        world.insert_resource(state)?;
    }
    Ok(())
}

/// Writes the `Pointer` resource from [`POINTER`] and [`CLICK`].
fn apply_pointer(world: &mut World, values: &Values) -> Result<()> {
    let before = world.resource::<Pointer>().copied();
    let given = values.get(POINTER).copied();
    if before.is_none() && given.is_none() && !values.contains_key(CLICK) {
        return Ok(());
    }
    let before = before.unwrap_or_default();
    let position = given
        .filter(|[x, y]| x.is_finite() && y.is_finite())
        .map(|p| p.map(|v| v.clamp(-MAX_POINTER, MAX_POINTER)));
    let present = position.is_some();
    let held = present && values.get(CLICK).is_some_and(|[x, _]| *x != 0.0);
    let pointer = Pointer {
        present,
        position: position.unwrap_or(before.position),
        held,
        pressed: held && !before.held,
        released: !held && before.held,
    };
    if world.resource::<Pointer>() != Some(&pointer) {
        world.insert_resource(pointer)?;
    }
    Ok(())
}
