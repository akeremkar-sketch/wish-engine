//! Players' input frames into the world's `PlayerActionState`, with the
//! same cleaning and edges as `wish_input::apply_players`.

use wish_core::Result;
use wish_ecs::World;
use wish_types::{
    ActionSlot, ActionState, MAX_ACTIONS, MAX_PLAYERS, PlayerActionSlots, PlayerActionState,
    PlayerId, Players,
};

/// `(action id, [x, y])` for every action of a player that is not at rest.
pub type Actions = Vec<(u32, [f32; 2])>;

/// A value as `wish_input` cleans it: not a number is 0, the rest is
/// clamped to -1..1.
pub fn clean(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The actions not at rest in a player's slots, as a client sends them.
pub fn actions_of(slots: &[ActionSlot; MAX_ACTIONS]) -> Actions {
    slots
        .iter()
        .filter(|slot| slot.id != 0 && (slot.x != 0.0 || slot.y != 0.0))
        .map(|slot| (slot.id, [slot.x, slot.y]))
        .collect()
}

/// One player's slots: the ids of `template` (the project's actions, in
/// order), the values in `actions` (others at rest), edges against
/// `previous`.
fn slots(
    template: &[ActionSlot; MAX_ACTIONS],
    actions: &[(u32, [f32; 2])],
    previous: Option<&[ActionSlot; MAX_ACTIONS]>,
) -> [ActionSlot; MAX_ACTIONS] {
    let mut out = [ActionSlot::default(); MAX_ACTIONS];
    for (slot, model) in out.iter_mut().zip(template) {
        if model.id == 0 {
            continue;
        }
        let id = model.id;
        let [x, y] = actions
            .iter()
            .find(|(action, _)| *action == id)
            .map_or([0.0, 0.0], |(_, v)| *v)
            .map(clean);
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
    out
}

/// Makes `Players.count` at least `count` (never lowers it). With one
/// player nothing is written, as `wish_input::set_players` does.
pub fn ensure_players(world: &mut World, count: u32) -> Result<()> {
    let count = count.min(MAX_PLAYERS as u32);
    let current = world.resource::<Players>().map_or(1, |p| p.count);
    if count > current.max(1) {
        world.insert_resource(Players { count })?;
    }
    Ok(())
}

/// Writes this machine's player's actions into `ActionState`, with the same
/// cleaning and edges as the players' (the ids are those `ActionState`
/// already holds: the project's actions, in order). Nothing is written when
/// the project has no actions, nor when nothing changed. A host that records
/// its inputs (the lobby's match log) writes its own player's this way.
pub fn write_local(world: &mut World, actions: &[(u32, [f32; 2])]) -> Result<()> {
    let Some(current) = world.resource::<ActionState>().copied() else {
        return Ok(());
    };
    let next = ActionState {
        slots: slots(&current.slots, actions, Some(&current.slots)),
    };
    if next != current {
        world.insert_resource(next)?;
    }
    Ok(())
}

/// Writes a predicting client's own actions into its prediction world: into
/// `ActionState` (what game code reads as this machine's player, as on a
/// machine alone) and into `PlayerActionState` as player `me`'s, with the
/// host's entry 0 and every other player at rest (their input is the
/// host's to know). The same cleaning and edges as everywhere; nothing is
/// written when the project has no actions, nor when nothing changed.
pub fn write_predicted(world: &mut World, me: PlayerId, actions: &[(u32, [f32; 2])]) -> Result<()> {
    write_local(world, actions)?;
    if me == 0 {
        return write_players(world, &|_| None);
    }
    ensure_players(world, me + 1)?;
    let count = world
        .resource::<Players>()
        .map_or(1, |p| p.count)
        .min(MAX_PLAYERS as u32);
    let Some(local) = world.resource::<ActionState>().copied() else {
        return Ok(());
    };
    let previous = world.resource::<PlayerActionState>().copied();
    let mut state = PlayerActionState::default();
    for (player, entry) in state.players.iter_mut().enumerate().take(count as usize) {
        let own: &[(u32, [f32; 2])] = if player == me as usize { actions } else { &[] };
        let before = previous.as_ref().and_then(|p| p.players.get(player));
        *entry = PlayerActionSlots {
            slots: slots(&local.slots, own, before.map(|b| &b.slots)),
        };
    }
    if previous != Some(state) {
        world.insert_resource(state)?;
    }
    Ok(())
}

/// Writes every player's actions into `PlayerActionState`: entry 0 is the
/// host's own `ActionState`; entry `p` comes from `remote(p)` (a connected
/// player's held frame) and is at rest when it gives `None`. Players beyond
/// `Players.count` stay at their defaults. Nothing is written with one
/// player, nor when the project has no actions (no `ActionState`), nor when
/// nothing changed.
pub fn write_players(world: &mut World, remote: &dyn Fn(u32) -> Option<Actions>) -> Result<()> {
    let count = world
        .resource::<Players>()
        .map_or(1, |p| p.count)
        .min(MAX_PLAYERS as u32);
    if count <= 1 {
        return Ok(());
    }
    let Some(local) = world.resource::<ActionState>().copied() else {
        return Ok(());
    };
    let previous = world.resource::<PlayerActionState>().copied();
    let mut state = PlayerActionState::default();
    for (player, entry) in state.players.iter_mut().enumerate().take(count as usize) {
        if player == 0 {
            *entry = PlayerActionSlots { slots: local.slots };
            continue;
        }
        let actions = remote(player as u32).unwrap_or_default();
        let before = previous.as_ref().and_then(|p| p.players.get(player));
        *entry = PlayerActionSlots {
            slots: slots(&local.slots, &actions, before.map(|b| &b.slots)),
        };
    }
    if previous != Some(state) {
        world.insert_resource(state)?;
    }
    Ok(())
}
